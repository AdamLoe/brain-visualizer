//! Morphology — procedural per-neuron geometry (V2 Beauty-First).
//!
//! Each neuron is drawn not as an abstract billboard glow alone but as a real
//! cell: a soma with a bushy **dendrite** tree (local, receiving) and a
//! branching **axon** arbor (projecting, terminating near the neuron's real
//! synaptic targets). The whole thing is a list of line SEGMENTS, generated on
//! the CPU at `initialize()` time and uploaded once to a GPU storage buffer.
//! The morphology renderer (render_morphology.wgsl) draws each segment as a
//! camera-facing tapered tube. When a neuron fires, its actual synaptic
//! connections (axon segments) light up instantly and fade with the same
//! `exp(-tick_diff/glow_tau)` curve as the far-glow neuron dot — keyed off the
//! segment's source (`neuron_id`) for downstream lighting and its `target_id`
//! for upstream lighting.
//!
//! ALL randomness is drawn from the locked BV22 hash (`mix_key`/`hash32`) so the
//! morphology is bit-reproducible for a given seed (BV16 determinism).
//!
//! `MorphSegment` field order + size (48 bytes) MUST match the WGSL struct in
//! render_morphology.wgsl verbatim (#1 corruption source — see the doc on the
//! struct). Host-testable; no GPU dependency.

use crate::connectivity::hash::{hash32, mix_key};
use crate::connectivity::spatial::SpatialGrid;
use crate::connectivity::{self};
use crate::manifold::RegionKind;
use crate::sim::backend::neuron_type_byte;
use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
struct MorphTimer(Instant);

#[cfg(not(target_arch = "wasm32"))]
impl MorphTimer {
    fn start() -> Self {
        Self(Instant::now())
    }

    fn elapsed_ms(&self) -> f32 {
        self.0.elapsed().as_secs_f32() * 1000.0
    }
}

#[cfg(target_arch = "wasm32")]
struct MorphTimer;

#[cfg(target_arch = "wasm32")]
impl MorphTimer {
    fn start() -> Self {
        Self
    }

    fn elapsed_ms(&self) -> f32 {
        0.0
    }
}

// ─── Salts (decorrelate the different morphology hash uses) ───────────────────
// Distinct from connectivity::salt values (those go up to 4); pick a high,
// disjoint range so a morphology draw never collides with a target/weight draw.
mod salt {
    pub const DENDRITE_DIR: u32 = 0x00A0_0001; // primary dendrite direction
    pub const DENDRITE_CURL: u32 = 0x00A0_0002; // per-segment curl jitter
    pub const DENDRITE_COUNT: u32 = 0x00A0_0003; // how many primary dendrites
    pub const AXON_BOW: u32 = 0x00A0_0004; // axon perpendicular arc seed
}

/// Per-neuron morphology tuning parameters (world units; tuned to the ~0.15
/// inter-neuron spacing at N=1200 so neighbouring trees nearly touch but do not
/// fuse into a hairball).
pub mod params {
    /// Soma-end dendrite/axon radius (world units).
    pub const R0: f32 = 0.006;
    /// Dendrites: minimum primary count (D = MIN + hash % SPAN).
    pub const DENDRITE_MIN: u32 = 3;
    pub const DENDRITE_SPAN: u32 = 2; // → 3..=4 primary dendrites
    /// Dendrite total reach (soma → tip), randomized per dendrite in this band.
    pub const DENDRITE_REACH_LO: f32 = 0.035;
    pub const DENDRITE_REACH_HI: f32 = 0.058;
    /// Axon stops short of the target so boutons cluster near the target's
    /// dendrites rather than inside its soma.
    pub const AXON_STOP_FRACTION: f32 = 0.85;
    /// Axon trunk radius at the soma (fraction of R0).
    pub const AXON_R0_FRACTION: f32 = 0.66;
}

/// Locked morphology parameter preset used by the generator.
///
/// Classification notes for the current stream:
/// - generator-default: base radius, dendrite reach/count tuning, axon stop/radius
/// - review-override: axon curve lift (mirrors the live visual curve setting)
/// - protected: allocation slack and branch-segment count
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphologyParams {
    pub base_radius: f32,
    pub dendrite_primary_min: u32,
    pub dendrite_primary_span: u32,
    pub dendrite_reach_lo: f32,
    pub dendrite_reach_hi: f32,
    pub axon_stop_fraction: f32,
    pub axon_root_radius_fraction: f32,
    pub axon_curve_lift: f32,
    pub socket_count_min: usize,
    pub socket_count_max: usize,
    pub socket_radius_lo: f32,
    pub socket_radius_hi: f32,
    pub socket_tip_preference: f32,
    pub cluster_min: usize,
    pub cluster_max: usize,
    pub trunk_root_samples: usize,
    pub cluster_branch_samples: usize,
    pub terminal_twig_samples: usize,
    pub trunk_length_fraction: f32,
    pub cluster_split_fraction: f32,
    pub root_radius_fraction: f32,
    pub cluster_radius_fraction: f32,
    pub twig_radius_fraction: f32,
    pub taper_curve: f32,
    /// Dendrite mid-point radius as a fraction of base_radius (soma → bifurcation).
    pub dendrite_mid_radius_fraction: f32,
    /// Dendrite tip radius as a fraction of base_radius (bifurcation → tip).
    pub dendrite_tip_radius_fraction: f32,
    pub dendrite_budget: usize,
    pub trunk_cluster_budget: usize,
    pub terminal_twig_budget: usize,
    pub cap_slack: usize,
}

impl MorphologyParams {
    /// Locked default preset that preserves the current visual shape.
    pub const fn locked_default() -> Self {
        Self {
            base_radius: params::R0,
            dendrite_primary_min: params::DENDRITE_MIN,
            dendrite_primary_span: params::DENDRITE_SPAN,
            dendrite_reach_lo: params::DENDRITE_REACH_LO,
            dendrite_reach_hi: params::DENDRITE_REACH_HI,
            axon_stop_fraction: params::AXON_STOP_FRACTION,
            axon_root_radius_fraction: 0.66,
            axon_curve_lift: 0.15,
            socket_count_min: 2,
            socket_count_max: 4,
            socket_radius_lo: 0.008,
            socket_radius_hi: 0.018,
            socket_tip_preference: 0.78,
            cluster_min: 2,
            cluster_max: 5,
            trunk_root_samples: 2,
            cluster_branch_samples: 2,
            terminal_twig_samples: 3,
            trunk_length_fraction: 0.32,
            cluster_split_fraction: 0.62,
            root_radius_fraction: 0.62,
            cluster_radius_fraction: 0.44,
            twig_radius_fraction: 0.16,
            taper_curve: 2.1,
            dendrite_mid_radius_fraction: 0.6,
            dendrite_tip_radius_fraction: 0.3,
            dendrite_budget: DENDRITE_MAX,
            trunk_cluster_budget: 14,
            terminal_twig_budget: 4,
            cap_slack: 4,
        }
    }

    /// Convenience alias for the locked default preset.
    pub const fn default_preset() -> Self {
        Self::locked_default()
    }

    /// Override the live review curve while keeping the rest of the preset
    /// locked to the current default.
    pub const fn with_curve_lift(mut self, curve_lift: f32) -> Self {
        self.axon_curve_lift = curve_lift;
        self
    }

    /// Hard segment cap per neuron for the current all-K branch grammar.
    pub fn segment_cap(&self, k: usize) -> usize {
        self.dendrite_budget
            + self.trunk_cluster_budget
            + k * self.terminal_twig_budget
            + self.cap_slack
    }

    /// Segment cap in bytes for `n` neurons at out-degree `k`.
    pub fn segment_cap_bytes(&self, n: usize, k: usize) -> usize {
        n * self.segment_cap(k) * std::mem::size_of::<MorphSegment>()
    }

    /// Compact JSON snapshot used by review artifacts.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"base_radius\":{:.6},\"dendrite_primary_min\":{},\"dendrite_primary_span\":{},\"dendrite_reach_lo\":{:.6},\"dendrite_reach_hi\":{:.6},\"axon_stop_fraction\":{:.6},\"axon_root_radius_fraction\":{:.6},\"axon_curve_lift\":{:.6},\"socket_count_min\":{},\"socket_count_max\":{},\"socket_radius_lo\":{:.6},\"socket_radius_hi\":{:.6},\"socket_tip_preference\":{:.6},\"cluster_min\":{},\"cluster_max\":{},\"trunk_root_samples\":{},\"cluster_branch_samples\":{},\"terminal_twig_samples\":{},\"trunk_length_fraction\":{:.6},\"cluster_split_fraction\":{:.6},\"root_radius_fraction\":{:.6},\"cluster_radius_fraction\":{:.6},\"twig_radius_fraction\":{:.6},\"taper_curve\":{:.6},\"dendrite_budget\":{},\"trunk_cluster_budget\":{},\"terminal_twig_budget\":{},\"cap_slack\":{}}}",
            self.base_radius,
            self.dendrite_primary_min,
            self.dendrite_primary_span,
            self.dendrite_reach_lo,
            self.dendrite_reach_hi,
            self.axon_stop_fraction,
            self.axon_root_radius_fraction,
            self.axon_curve_lift,
            self.socket_count_min,
            self.socket_count_max,
            self.socket_radius_lo,
            self.socket_radius_hi,
            self.socket_tip_preference,
            self.cluster_min,
            self.cluster_max,
            self.trunk_root_samples,
            self.cluster_branch_samples,
            self.terminal_twig_samples,
            self.trunk_length_fraction,
            self.cluster_split_fraction,
            self.root_radius_fraction,
            self.cluster_radius_fraction,
            self.twig_radius_fraction,
            self.taper_curve,
            self.dendrite_budget,
            self.trunk_cluster_budget,
            self.terminal_twig_budget,
            self.cap_slack,
        )
    }
}

impl Default for MorphologyParams {
    fn default() -> Self {
        Self::locked_default()
    }
}

// ─── v0.3.1 morphology config (JSON round-trip) ───────────────────────────────
//
// The dev-panel morphology config crosses the WASM boundary as a single JSON
// blob `{ generator: {...}, renderQuality: {...}, lighting: {...} }`. The field
// names/shape are LOCKED by the `## Config Contract` table in
// docs/plans/v0.3.1-morph-config.md — both this Rust side and the TS side verify
// against that table, NOT against each other.
//
// Discipline: budgets/slack/salts are deliberately EXCLUDED from the generator
// group (allocation/determinism boundaries, not visual tuning). When mapping the
// config into `MorphologyParams`, the protected budget fields keep their
// `locked_default()` values.

/// Generator controls — the 24 exposed `MorphologyParams` fields (budgets/slack
/// and salts are protected and excluded). `#[serde(default)]` so a partial JSON
/// blob falls back to the locked defaults per field.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorConfig {
    pub base_radius: f32,
    pub dendrite_primary_min: u32,
    pub dendrite_primary_span: u32,
    pub dendrite_reach_lo: f32,
    pub dendrite_reach_hi: f32,
    pub axon_stop_fraction: f32,
    pub axon_root_radius_fraction: f32,
    pub axon_curve_lift: f32,
    pub socket_count_min: usize,
    pub socket_count_max: usize,
    pub socket_radius_lo: f32,
    pub socket_radius_hi: f32,
    pub socket_tip_preference: f32,
    pub cluster_min: usize,
    pub cluster_max: usize,
    pub trunk_root_samples: usize,
    pub cluster_branch_samples: usize,
    pub terminal_twig_samples: usize,
    pub trunk_length_fraction: f32,
    pub cluster_split_fraction: f32,
    pub root_radius_fraction: f32,
    pub cluster_radius_fraction: f32,
    pub twig_radius_fraction: f32,
    pub taper_curve: f32,
    pub dendrite_mid_radius_fraction: f32,
    pub dendrite_tip_radius_fraction: f32,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self::from_params(&MorphologyParams::locked_default())
    }
}

impl GeneratorConfig {
    /// Extract the 24 generator-tunable fields from a `MorphologyParams`.
    pub fn from_params(p: &MorphologyParams) -> Self {
        Self {
            base_radius: p.base_radius,
            dendrite_primary_min: p.dendrite_primary_min,
            dendrite_primary_span: p.dendrite_primary_span,
            dendrite_reach_lo: p.dendrite_reach_lo,
            dendrite_reach_hi: p.dendrite_reach_hi,
            axon_stop_fraction: p.axon_stop_fraction,
            axon_root_radius_fraction: p.axon_root_radius_fraction,
            axon_curve_lift: p.axon_curve_lift,
            socket_count_min: p.socket_count_min,
            socket_count_max: p.socket_count_max,
            socket_radius_lo: p.socket_radius_lo,
            socket_radius_hi: p.socket_radius_hi,
            socket_tip_preference: p.socket_tip_preference,
            cluster_min: p.cluster_min,
            cluster_max: p.cluster_max,
            trunk_root_samples: p.trunk_root_samples,
            cluster_branch_samples: p.cluster_branch_samples,
            terminal_twig_samples: p.terminal_twig_samples,
            trunk_length_fraction: p.trunk_length_fraction,
            cluster_split_fraction: p.cluster_split_fraction,
            root_radius_fraction: p.root_radius_fraction,
            cluster_radius_fraction: p.cluster_radius_fraction,
            twig_radius_fraction: p.twig_radius_fraction,
            taper_curve: p.taper_curve,
            dendrite_mid_radius_fraction: p.dendrite_mid_radius_fraction,
            dendrite_tip_radius_fraction: p.dendrite_tip_radius_fraction,
        }
    }

    /// Apply these 24 fields onto a base `MorphologyParams`, preserving the base's
    /// protected budget/slack fields (`dendrite_budget`, `trunk_cluster_budget`,
    /// `terminal_twig_budget`, `cap_slack`).
    pub fn apply_to(&self, base: &MorphologyParams) -> MorphologyParams {
        MorphologyParams {
            base_radius: self.base_radius,
            dendrite_primary_min: self.dendrite_primary_min,
            dendrite_primary_span: self.dendrite_primary_span,
            dendrite_reach_lo: self.dendrite_reach_lo,
            dendrite_reach_hi: self.dendrite_reach_hi,
            axon_stop_fraction: self.axon_stop_fraction,
            axon_root_radius_fraction: self.axon_root_radius_fraction,
            axon_curve_lift: self.axon_curve_lift,
            socket_count_min: self.socket_count_min,
            socket_count_max: self.socket_count_max,
            socket_radius_lo: self.socket_radius_lo,
            socket_radius_hi: self.socket_radius_hi,
            socket_tip_preference: self.socket_tip_preference,
            cluster_min: self.cluster_min,
            cluster_max: self.cluster_max,
            trunk_root_samples: self.trunk_root_samples,
            cluster_branch_samples: self.cluster_branch_samples,
            terminal_twig_samples: self.terminal_twig_samples,
            trunk_length_fraction: self.trunk_length_fraction,
            cluster_split_fraction: self.cluster_split_fraction,
            root_radius_fraction: self.root_radius_fraction,
            cluster_radius_fraction: self.cluster_radius_fraction,
            twig_radius_fraction: self.twig_radius_fraction,
            taper_curve: self.taper_curve,
            dendrite_mid_radius_fraction: self.dendrite_mid_radius_fraction,
            dendrite_tip_radius_fraction: self.dendrite_tip_radius_fraction,
            // Protected: keep the base preset's budgets/slack.
            dendrite_budget: base.dendrite_budget,
            trunk_cluster_budget: base.trunk_cluster_budget,
            terminal_twig_budget: base.terminal_twig_budget,
            cap_slack: base.cap_slack,
        }
    }
}

/// Render-quality controls — tube/sphere tessellation. These drive WGSL pipeline
/// override constants AND the Rust draw vert-counts; a change triggers a morph
/// render-pipeline rebuild (applyKind = pipeline-rebuild).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderQualityConfig {
    pub tube_sides: u32,
    pub sphere_slices: u32,
    pub sphere_stacks: u32,
}

impl Default for RenderQualityConfig {
    fn default() -> Self {
        // Inherited v0.3.0 defaults (TUBE_SIDES=6, SPHERE_SLICES=8, SPHERE_STACKS=6).
        Self {
            tube_sides: 6,
            sphere_slices: 8,
            sphere_stacks: 6,
        }
    }
}

/// Lighting / brightness controls. All uniform-only (applyKind = uniform) — no
/// regeneration or pipeline rebuild. Direction is re-normalised CPU-side before
/// it reaches the uniform. `resting_brightness` and `active_boost` map to the two
/// NEW `MorphUniforms` fields (the latter replaces the WGSL `const BOOST=1.8`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LightingConfig {
    pub light_dir_x: f32,
    pub light_dir_y: f32,
    pub light_dir_z: f32,
    pub ambient: f32,
    pub diffuse_intensity: f32,
    pub rim_intensity: f32,
    pub rim_power: f32,
    pub resting_brightness: f32,
    pub active_boost: f32,
}

impl Default for LightingConfig {
    fn default() -> Self {
        // Defaults locked to the v0.3.0 lighting consts + the brightness split
        // (resting 0.20 ≈ current morph_resting_opacity, activeBoost 1.8 = current
        // shader BOOST). light_dir defaults are the pre-normalised components of
        // normalize(-0.35, 0.55, 0.75).
        Self {
            light_dir_x: -0.352,
            light_dir_y: 0.553,
            light_dir_z: 0.755,
            ambient: 0.55,
            diffuse_intensity: 0.35,
            rim_intensity: 0.30,
            rim_power: 2.0,
            resting_brightness: 0.20,
            active_boost: 1.8,
        }
    }
}

/// Full morphology config blob — the JSON contract for the dev-panel
/// `set_morphology_config` WASM entry point. `Default` equals the contract
/// defaults (generator == `locked_default()`, lighting == v0.3.0 consts).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MorphologyConfig {
    pub generator: GeneratorConfig,
    pub render_quality: RenderQualityConfig,
    pub lighting: LightingConfig,
}

impl MorphologyConfig {
    /// Parse a JSON blob into a config, falling back to per-field defaults for any
    /// missing keys (`#[serde(default)]`).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialise to a JSON string (camelCase keys, nested groups).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Build the `MorphologyParams` this config implies, layering the generator
    /// group over the locked default (so protected budgets stay locked).
    pub fn to_params(&self) -> MorphologyParams {
        self.generator.apply_to(&MorphologyParams::locked_default())
    }
}

/// Coarse build-profile timings for the morphology generator.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MorphologyTimings {
    pub setup_ms: f32,
    pub dendrite_ms: f32,
    pub axon_ms: f32,
    pub finalize_ms: f32,
    pub total_ms: f32,
}

/// Structured build/profile facts for the current morphology generation pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MorphologyStats {
    pub segment_count: usize,
    pub dropped_count: usize,
    pub segment_cap: usize,
    pub segment_cap_bytes: usize,
    pub segment_buffer_bytes: usize,
    pub cap_utilization: f32,
    pub duplicate_targets: usize,
    pub self_targets: usize,
    pub source_type_bytes: usize,
    pub source_type_excitatory: usize,
    pub source_type_inhibitory: usize,
    pub cluster_count_histogram: [u32; 6],
    pub terminal_socket_distance_bands: [u32; 4],
    pub socket_reuse_bands: [u32; 4],
    pub unique_targets_expected: usize,
    pub unique_targets_emitted: usize,
    pub unique_target_coverage: f32,
    pub all_k_coverage: bool,
    pub timings: MorphologyTimings,
}

impl MorphologyStats {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"segment_count\":{},\"dropped_count\":{},\"segment_cap\":{},\"segment_cap_bytes\":{},\"segment_buffer_bytes\":{},\"cap_utilization\":{:.6},\"duplicate_targets\":{},\"self_targets\":{},\"source_type_bytes\":{},\"source_type_excitatory\":{},\"source_type_inhibitory\":{},\"cluster_count_histogram\":[{},{},{},{},{},{}],\"terminal_socket_distance_bands\":[{},{},{},{}],\"socket_reuse_bands\":[{},{},{},{}],\"unique_targets_expected\":{},\"unique_targets_emitted\":{},\"unique_target_coverage\":{:.6},\"all_k_coverage\":{},\"timings\":{{\"setup_ms\":{:.3},\"dendrite_ms\":{:.3},\"axon_ms\":{:.3},\"finalize_ms\":{:.3},\"total_ms\":{:.3}}}}}",
            self.segment_count,
            self.dropped_count,
            self.segment_cap,
            self.segment_cap_bytes,
            self.segment_buffer_bytes,
            self.cap_utilization,
            self.duplicate_targets,
            self.self_targets,
            self.source_type_bytes,
            self.source_type_excitatory,
            self.source_type_inhibitory,
            self.cluster_count_histogram[0],
            self.cluster_count_histogram[1],
            self.cluster_count_histogram[2],
            self.cluster_count_histogram[3],
            self.cluster_count_histogram[4],
            self.cluster_count_histogram[5],
            self.terminal_socket_distance_bands[0],
            self.terminal_socket_distance_bands[1],
            self.terminal_socket_distance_bands[2],
            self.terminal_socket_distance_bands[3],
            self.socket_reuse_bands[0],
            self.socket_reuse_bands[1],
            self.socket_reuse_bands[2],
            self.socket_reuse_bands[3],
            self.unique_targets_expected,
            self.unique_targets_emitted,
            self.unique_target_coverage,
            self.all_k_coverage,
            self.timings.setup_ms,
            self.timings.dendrite_ms,
            self.timings.axon_ms,
            self.timings.finalize_ms,
            self.timings.total_ms,
        )
    }
}

/// One morphology line segment. 48 bytes, std430, 16-aligned.
///
/// Field order + size MUST match the `MorphSegment` struct in
/// render_morphology.wgsl verbatim:
/// ```text
///   a: vec3<f32>,  radius_a: f32   // 16
///   b: vec3<f32>,  radius_b: f32   // 16
///   neuron_id: u32, path_len: f32, kind: u32, target_id: u32  // 16
/// ```
/// `kind`: 0 = dendrite, 1 = axon. `neuron_id` = the segment's SOURCE neuron
/// (drives downstream "next" lighting). `target_id` = the axon segment's
/// destination neuron (drives upstream "past" lighting); for dendrites it is set
/// to `neuron_id` (self) and is unused. `path_len` = cumulative path length FROM
/// THE SOMA to endpoint `a` (retained for the renderer; no longer drives timing).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MorphSegment {
    pub a: [f32; 3],
    pub radius_a: f32,
    pub b: [f32; 3],
    pub radius_b: f32,
    pub neuron_id: u32,
    pub path_len: f32,
    pub kind: u32,
    pub target_id: u32,
}

/// Soma sphere instance for the soma-sphere render pass (Wave 2 / Stream 2).
/// 32 bytes, 16-aligned. Field order + size MUST match `SphereInstance` in
/// render_morphology.wgsl (vs_sphere reads it from a storage buffer).
///
/// Layout (32 B):
///   center: [f32;3] (12 B), radius: f32 (4 B)     → 16 B block 0
///   neuron_id: u32  (4 B),  kind: u32 (4 B),
///   _pad0: u32      (4 B),  _pad1: u32 (4 B)       → 16 B block 1
///
/// `kind` = 2 (soma). `neuron_id` keys last_spike for spike brightness.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MorphSphereInstance {
    pub center: [f32; 3],
    pub radius: f32,
    pub neuron_id: u32,
    pub kind: u32,   // 2 = soma
    pub _pad0: u32,
    pub _pad1: u32,
}

/// Generated morphology: the flat segment list plus the total count. (Per-neuron
/// ranges are implicit in `neuron_id`; the renderer keys off that.)
pub struct Morphology {
    pub segments: Vec<MorphSegment>,
    /// Upper bound used for the allocation cap; segments past this were dropped
    /// (logged — "no silent caps").
    pub dropped: usize,
    /// Structured build/profile facts for tests and review artifacts.
    pub stats: MorphologyStats,
}

/// Build the flat soma-sphere instance list from neuron positions and source
/// types. Emits exactly one `MorphSphereInstance` per neuron (index == neuron_id).
///
/// Soma radius = `params.base_radius` (same R0 that seeds the dendrite/axon
/// radius at the soma attachment point — this makes the sphere diameter
/// visually match the tube's root footprint).
pub fn emit_soma_spheres(
    positions: &[[f32; 3]],
    source_types: &[u8],
    params: &MorphologyParams,
) -> Vec<MorphSphereInstance> {
    positions
        .iter()
        .enumerate()
        .map(|(i, &pos)| {
            let _ = source_types.get(i); // bounds check only; kind is always 2 (soma)
            MorphSphereInstance {
                center: pos,
                radius: params.base_radius,
                neuron_id: i as u32,
                kind: 2,
                _pad0: 0,
                _pad1: 0,
            }
        })
        .collect()
}

const DENDRITE_STEM_SAMPLES: usize = 2;
const DENDRITE_TWIG_SAMPLES: usize = 2;

/// Worst-case dendrite segments per neuron for the locked grammar: up to 5
/// primaries × (2 sampled stem segments + 2 twigs × 2 sampled segments) = 30.
pub const DENDRITE_MAX: usize = 30;

/// Build the production type-byte slice from the manifold region assignment and
/// seed, matching `initial_last_spike()` / the integrate-shader path.
pub fn build_source_types(seed_lo: u32, regions: &[RegionKind]) -> Vec<u8> {
    regions
        .iter()
        .enumerate()
        .map(|(i, &region)| neuron_type_byte(i as u32, seed_lo, region))
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct TargetPlan {
    target_id: u32,
    source_pos: [f32; 3],
    target_pos: [f32; 3],
    direction: [f32; 3],
    distance: f32,
    socket_idx: usize,
    socket_pos: [f32; 3],
    socket_distance: f32,
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[inline]
fn cubic_bezier(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3], t: f32) -> [f32; 3] {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    let uuu = uu * u;
    let ttt = tt * t;
    [
        uuu * p0[0] + 3.0 * uu * t * p1[0] + 3.0 * u * tt * p2[0] + ttt * p3[0],
        uuu * p0[1] + 3.0 * uu * t * p1[1] + 3.0 * u * tt * p2[1] + ttt * p3[1],
        uuu * p0[2] + 3.0 * uu * t * p1[2] + 3.0 * u * tt * p2[2] + ttt * p3[2],
    ]
}

fn bezier_basis(dir: [f32; 3], seed: u32) -> ([f32; 3], [f32; 3]) {
    let right = perp(dir, seed);
    let up = norm(cross(dir, right));
    (right, up)
}

fn bend_vector(dir: [f32; 3], seed: u32, magnitude: f32) -> [f32; 3] {
    let (right, up) = bezier_basis(dir, seed);
    let j0 = unit(hash32(seed ^ 0x3141_5926));
    let j1 = unit(hash32(seed ^ 0x2718_2818));
    scale(
        add(scale(right, j0 * 2.0 - 1.0), scale(up, j1 * 2.0 - 1.0)),
        magnitude,
    )
}

fn emit_bezier_path(
    segments: &mut Vec<MorphSegment>,
    cap: usize,
    dropped: &mut usize,
    source_id: u32,
    target_id: u32,
    kind: u32,
    p0: [f32; 3],
    p1: [f32; 3],
    p2: [f32; 3],
    p3: [f32; 3],
    r0: f32,
    r3: f32,
    samples: usize,
    path_len_start: f32,
    taper_curve: f32,
) -> (f32, bool, f32) {
    let mut prev = p0;
    let mut prev_r = r0;
    let mut prev_path = path_len_start;
    let mut emitted_all = true;
    let mut total_len = 0.0f32;
    for s in 1..=samples.max(1) {
        let t = s as f32 / samples.max(1) as f32;
        let pt = cubic_bezier(p0, p1, p2, p3, t);
        let rr = lerp(r0, r3, clamp01(t).powf(taper_curve.max(0.01)));
        let next_len = len(sub(pt, prev));
        if segments.len() < cap {
            segments.push(MorphSegment {
                a: prev,
                radius_a: prev_r,
                b: pt,
                radius_b: rr,
                neuron_id: source_id,
                path_len: prev_path,
                kind,
                target_id,
            });
        } else {
            *dropped += 1;
            emitted_all = false;
        }
        prev_path += next_len;
        total_len += next_len;
        prev = pt;
        prev_r = rr;
    }
    (prev_path, emitted_all, total_len)
}

fn target_socket(
    seed_lo: u32,
    source_id: u32,
    target: &TargetPlan,
    params: &MorphologyParams,
) -> ([f32; 3], usize, f32) {
    let socket_span = params
        .socket_count_max
        .saturating_sub(params.socket_count_min);
    let socket_count = (params.socket_count_min
        + if socket_span == 0 {
            0
        } else {
            mix_key(seed_lo, source_id, target.target_id, salt::AXON_BOW) as usize
                % (socket_span + 1)
        })
    .max(1);
    let socket_idx = if socket_count == 1 {
        0
    } else {
        mix_key(seed_lo, source_id, target.target_id, salt::DENDRITE_COUNT) as usize % socket_count
    };
    let source_dir = norm(sub(target.source_pos, target.target_pos));
    let dendrite_hint = dir_from_hashes(
        mix_key(
            seed_lo,
            target.target_id,
            socket_idx as u32,
            salt::DENDRITE_DIR,
        ),
        mix_key(
            seed_lo,
            target.target_id,
            socket_idx as u32,
            salt::DENDRITE_CURL,
        ),
    );
    let facing = norm(add(
        scale(source_dir, params.socket_tip_preference),
        scale(dendrite_hint, 1.0 - params.socket_tip_preference),
    ));
    let radius = lerp(
        params.socket_radius_lo,
        params.socket_radius_hi,
        unit(mix_key(
            seed_lo,
            source_id,
            target.target_id,
            salt::AXON_BOW ^ 0x55aa_55aa,
        )),
    ) * params.axon_stop_fraction.max(0.05);
    let socket_pos = add(target.target_pos, scale(facing, radius));
    let socket_distance = len(sub(socket_pos, target.target_pos));
    (socket_pos, socket_idx, socket_distance)
}

fn cluster_sort_key(seed_lo: u32, source_id: u32, direction: [f32; 3]) -> f32 {
    let axis = dir_from_hashes(
        mix_key(seed_lo, source_id, 0, salt::AXON_BOW),
        mix_key(seed_lo, source_id, 1, salt::DENDRITE_DIR),
    );
    let (right, up) = bezier_basis(axis, mix_key(seed_lo, source_id, 2, salt::DENDRITE_CURL));
    let x = direction[0] * right[0] + direction[1] * right[1] + direction[2] * right[2];
    let y = direction[0] * up[0] + direction[1] * up[1] + direction[2] * up[2];
    y.atan2(x)
}

/// Worst-case segments per neuron for a given fan-out `k`, used to size the GPU
/// buffer cap. The named `MorphologyParams` budgets now own the actual formula;
/// this helper keeps the default preset path available for callers that only
/// know `k`.
#[inline]
pub fn max_segs_per_neuron(k: usize) -> usize {
    MorphologyParams::locked_default().segment_cap(k)
}

/// Decode a hash value into a float in [0,1).
#[inline]
fn unit(h: u32) -> f32 {
    (h as f32) / (u32::MAX as f32 + 1.0)
}

/// Roughly-uniform direction on the sphere from two hash draws.
#[inline]
fn dir_from_hashes(h0: u32, h1: u32) -> [f32; 3] {
    use std::f32::consts::TAU;
    let cos_theta = unit(h0) * 2.0 - 1.0;
    let phi = unit(h1) * TAU;
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]
}

#[inline]
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
#[inline]
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = len(a).max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}
#[inline]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A unit vector perpendicular to `dir`, seeded so the axon arc is reproducible.
fn perp(dir: [f32; 3], seed: u32) -> [f32; 3] {
    let r = dir_from_hashes(hash32(seed ^ 0x1111_1111), hash32(seed ^ 0x2222_2222));
    let d = norm(dir);
    // Remove component along dir → perpendicular.
    let dot = r[0] * d[0] + r[1] * d[1] + r[2] * d[2];
    let mut p = sub(r, scale(d, dot));
    if len(p) < 1e-5 {
        p = cross(d, [0.0, 1.0, 0.0]);
        if len(p) < 1e-5 {
            p = cross(d, [1.0, 0.0, 0.0]);
        }
    }
    norm(p)
}

/// Generate the full morphology for all `n` neurons. Deterministic in
/// `(seed_lo, positions, grid, k)`. Caps the total at `n * max_segs_per_neuron(k)`
/// (never hit in practice; logged if it is — no silent truncation).
pub fn generate(
    positions: &[[f32; 3]],
    grid: &SpatialGrid,
    k: usize,
    seed_lo: u32,
    params: &MorphologyParams,
    source_types: &[u8],
) -> Morphology {
    let n = positions.len();
    let setup_start = MorphTimer::start();
    assert_eq!(
        source_types.len(),
        n,
        "source type slice must cover every neuron"
    );
    let cap = n * params.segment_cap(k);
    let mut segments: Vec<MorphSegment> = Vec::with_capacity(cap.min(n * (DENDRITE_MAX + k * 4)));
    let mut dropped = 0usize;
    let mut duplicate_targets = 0usize;
    let mut self_targets = 0usize;
    let mut unique_targets_expected = 0usize;
    let mut unique_targets_emitted = 0usize;
    let mut all_k_coverage = true;
    let mut source_type_excitatory = 0usize;
    let mut source_type_inhibitory = 0usize;
    let mut cluster_count_histogram = [0u32; 6];
    let mut terminal_socket_distance_bands = [0u32; 4];
    let mut socket_reuse_bands = [0u32; 4];

    // Precompute each neuron's grid cell once (O(N)) so the axon-arbor loop below
    // can use the hot-path `target_with_cell` entry. The uncached
    // `connectivity::target` re-derives the cell with an O(N) `cell_of_index`
    // scan per call, which made morphology generation O(N²·K) and dominated
    // network-rebuild time at high N. The CPU/GPU paths already cache this map.
    let cell_of_neuron = grid.cell_of_neuron_map();

    // Local helper: push a segment unless the cap is hit (count drops instead).
    let push = |segments: &mut Vec<MorphSegment>, seg: MorphSegment, dropped: &mut usize| {
        if segments.len() < cap {
            segments.push(seg);
            true
        } else {
            *dropped += 1;
            false
        }
    };

    let setup_ms = setup_start.elapsed_ms();
    let mut dendrite_ms = 0.0f32;
    let mut axon_ms = 0.0f32;

    for i in 0..n {
        let soma = positions[i];
        let id = i as u32;

        // ── Dendrites (kind 0): bushy local tree, decorative. ────────────────
        let dendrite_start = MorphTimer::start();
        let src_type = source_types[i];
        if src_type & 0x01 == 0 {
            source_type_excitatory += 1;
        } else {
            source_type_inhibitory += 1;
        }
        let dcount = params.dendrite_primary_min
            + (mix_key(seed_lo, id, 0, salt::DENDRITE_COUNT) % params.dendrite_primary_span);
        for d in 0..dcount {
            let primary_dir = dir_from_hashes(
                mix_key(seed_lo, id, d, salt::DENDRITE_DIR),
                mix_key(seed_lo, id, d.wrapping_add(64), salt::DENDRITE_DIR),
            );
            let reach = params.dendrite_reach_lo
                + unit(mix_key(seed_lo, id, d, salt::DENDRITE_CURL))
                    * (params.dendrite_reach_hi - params.dendrite_reach_lo);
            let stem_len = reach
                * lerp(
                    0.42,
                    0.58,
                    unit(mix_key(seed_lo, id, d, salt::DENDRITE_COUNT ^ 0x7f4a_7c15)),
                );
            let r_soma = params.base_radius;
            let r_mid = params.base_radius * params.dendrite_mid_radius_fraction;
            let r_tip = params.base_radius * params.dendrite_tip_radius_fraction;

            let stem_bend = bend_vector(
                primary_dir,
                mix_key(seed_lo, id, d, salt::DENDRITE_CURL),
                stem_len * 0.45,
            );
            let stem_tip = add(
                add(soma, scale(primary_dir, stem_len)),
                scale(stem_bend, 0.35),
            );
            let stem_p1 = add(
                add(soma, scale(primary_dir, stem_len * 0.36)),
                scale(stem_bend, 0.44),
            );
            let stem_p2 = add(
                add(stem_tip, scale(primary_dir, -stem_len * 0.26)),
                scale(stem_bend, -0.18),
            );
            let (stem_path_end, _, _) = emit_bezier_path(
                &mut segments,
                cap,
                &mut dropped,
                id,
                id,
                0,
                soma,
                stem_p1,
                stem_p2,
                stem_tip,
                r_soma,
                r_mid,
                DENDRITE_STEM_SAMPLES,
                0.0,
                params.taper_curve * 0.8,
            );

            for c in 0..2u32 {
                let child_seed = mix_key(
                    seed_lo,
                    id,
                    d.wrapping_mul(19).wrapping_add(c),
                    salt::DENDRITE_CURL ^ 0x9e37_79b1,
                );
                let sign = if c == 0 { 1.0 } else { -1.0 };
                let spread = perp(primary_dir, child_seed ^ 0x00ff_00ff);
                let split_strength =
                    lerp(0.38, 0.74, unit(hash32(child_seed ^ 0x1357_9bdf))) * sign;
                let child_dir = norm(add(
                    add(scale(primary_dir, 1.0), scale(spread, split_strength)),
                    scale(norm(sub(stem_tip, soma)), 0.22),
                ));
                let twig_len = (reach - stem_len).max(reach * 0.25)
                    * lerp(0.88, 1.08, unit(hash32(child_seed ^ 0x2468_ace0)));
                let twig_bend = bend_vector(
                    child_dir,
                    child_seed,
                    twig_len * 0.38,
                );
                let tip = add(
                    add(stem_tip, scale(child_dir, twig_len)),
                    scale(twig_bend, 0.24),
                );
                let twig_p1 = add(
                    add(stem_tip, scale(child_dir, twig_len * 0.32)),
                    scale(twig_bend, 0.40),
                );
                let twig_p2 = add(
                    add(tip, scale(child_dir, -twig_len * 0.21)),
                    scale(twig_bend, -0.14),
                );
                let _ = emit_bezier_path(
                    &mut segments,
                    cap,
                    &mut dropped,
                    id,
                    id,
                    0,
                    stem_tip,
                    twig_p1,
                    twig_p2,
                    tip,
                    r_mid,
                    r_tip,
                    DENDRITE_TWIG_SAMPLES,
                    stem_path_end,
                    params.taper_curve * 0.75,
                );
            }
        }
        dendrite_ms += dendrite_start.elapsed_ms();

        // ── Axon arbor (kind 1): shared root -> clusters -> terminal twigs. ───
        let axon_start = MorphTimer::start();
        let src_cell = grid.unpack(cell_of_neuron[i]);
        let mut unique_targets = Vec::<u32>::new();
        let mut seen_targets = HashSet::new();
        for j in 0..k as u32 {
            let tgt_id =
                connectivity::target_with_cell(id, j, grid, k, seed_lo, src_type, src_cell);
            if tgt_id == id {
                self_targets += 1;
                continue;
            }
            if !seen_targets.insert(tgt_id) {
                duplicate_targets += 1;
                continue;
            }
            unique_targets.push(tgt_id);
        }
        unique_targets.sort_unstable();
        let unique_count = unique_targets.len();
        unique_targets_expected += unique_count;
        if unique_count == 0 {
            cluster_count_histogram[0] += 1;
            axon_ms += axon_start.elapsed_ms();
            continue;
        }

        let mut plans: Vec<TargetPlan> = unique_targets
            .iter()
            .map(|&tgt_id| {
                let target_pos = positions[tgt_id as usize];
                let full = sub(target_pos, soma);
                let distance = len(full).max(1e-6);
                let direction = norm(full);
                TargetPlan {
                    target_id: tgt_id,
                    source_pos: soma,
                    target_pos,
                    direction,
                    distance,
                    socket_idx: 0,
                    socket_pos: target_pos,
                    socket_distance: 0.0,
                }
            })
            .collect();

        let cluster_count = if unique_count == 1 {
            1
        } else {
            let min_c = params.cluster_min.max(1).min(unique_count);
            let max_c = params.cluster_max.max(min_c).min(unique_count);
            let span = max_c.saturating_sub(min_c) + 1;
            let draw = if span == 1 {
                0
            } else {
                mix_key(seed_lo, id, unique_count as u32, salt::DENDRITE_COUNT) as usize % span
            };
            (min_c + draw).clamp(1, unique_count)
        };
        cluster_count_histogram[cluster_count.min(5)] += 1;

        let mut axis = [0.0f32; 3];
        for p in &plans {
            axis = add(axis, p.direction);
        }
        axis = norm(add(
            axis,
            dir_from_hashes(
                mix_key(seed_lo, id, unique_count as u32, salt::AXON_BOW),
                mix_key(seed_lo, id, unique_count as u32, salt::DENDRITE_DIR),
            ),
        ));

        plans.sort_by(|a, b| {
            let ka = cluster_sort_key(seed_lo, id, a.direction);
            let kb = cluster_sort_key(seed_lo, id, b.direction);
            ka.partial_cmp(&kb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.target_id.cmp(&b.target_id))
        });

        let base = unique_count / cluster_count;
        let extra = unique_count % cluster_count;
        let mut clusters: Vec<Vec<usize>> = Vec::with_capacity(cluster_count);
        let mut cursor = 0usize;
        for cidx in 0..cluster_count {
            let size = base + usize::from(cidx < extra);
            let mut cluster = Vec::with_capacity(size);
            for _ in 0..size {
                cluster.push(cursor);
                cursor += 1;
            }
            clusters.push(cluster);
        }

        let root_radius = params.base_radius * params.axon_root_radius_fraction;
        let shared_root_radius = root_radius * params.root_radius_fraction;
        let cluster_start_radius = root_radius * params.cluster_radius_fraction;
        let twig_start_radius = root_radius * params.cluster_radius_fraction;
        let twig_end_radius = root_radius * params.twig_radius_fraction;

        if unique_count == 1 {
            let plan = &mut plans[0];
            let (socket_pos, socket_idx, socket_distance) =
                target_socket(seed_lo, id, plan, params);
            plan.socket_pos = socket_pos;
            plan.socket_idx = socket_idx;
            plan.socket_distance = socket_distance;
            terminal_socket_distance_bands[if socket_distance < params.socket_radius_lo * 0.5 {
                0
            } else if socket_distance < params.socket_radius_lo * 0.9 {
                1
            } else if socket_distance < params.socket_radius_hi * 1.1 {
                2
            } else {
                3
            }] += 1;
            socket_reuse_bands[socket_idx.min(3)] += 1;
            let path_dir = norm(sub(socket_pos, soma));
            let path_len = len(sub(socket_pos, soma)).max(0.03);
            let bend = bend_vector(
                path_dir,
                mix_key(seed_lo, id, plan.target_id, salt::AXON_BOW),
                path_len * params.axon_curve_lift.max(0.0),
            );
            let p1 = add(
                add(soma, scale(path_dir, path_len * 0.33)),
                scale(bend, 0.28),
            );
            let p2 = add(
                add(socket_pos, scale(path_dir, -path_len * 0.27)),
                scale(bend, -0.16),
            );
            let (next_path, complete, _) = emit_bezier_path(
                &mut segments,
                cap,
                &mut dropped,
                id,
                plan.target_id,
                1,
                soma,
                p1,
                p2,
                socket_pos,
                root_radius,
                twig_end_radius,
                params.terminal_twig_samples.max(1),
                0.0,
                params.taper_curve,
            );
            if !complete {
                all_k_coverage = false;
            } else {
                unique_targets_emitted += 1;
            }
            let _ = next_path;
            axon_ms += axon_start.elapsed_ms();
            continue;
        }

        let avg_distance = plans.iter().map(|p| p.distance).sum::<f32>() / unique_count as f32;
        let root_len = (avg_distance * params.trunk_length_fraction.max(0.05)).max(0.03);
        let root_dir = axis;
        let root_anchor = add(soma, scale(root_dir, root_len));
        let root_bend = bend_vector(
            root_dir,
            mix_key(seed_lo, id, unique_count as u32, salt::AXON_BOW),
            root_len * params.axon_curve_lift.max(0.0),
        );
        let root_p1 = add(
            add(soma, scale(root_dir, root_len * 0.33)),
            scale(root_bend, 0.35),
        );
        let root_p2 = add(
            add(root_anchor, scale(root_dir, -root_len * 0.27)),
            scale(root_bend, -0.18),
        );
        let (next_path, root_complete, _) = emit_bezier_path(
            &mut segments,
            cap,
            &mut dropped,
            id,
            id,
            1,
            soma,
            root_p1,
            root_p2,
            root_anchor,
            shared_root_radius,
            cluster_start_radius,
            params.trunk_root_samples.max(1),
            0.0,
            params.taper_curve,
        );
        if !root_complete {
            all_k_coverage = false;
        }
        let root_path_end = next_path;

        for (cluster_idx, cluster) in clusters.iter().enumerate() {
            let mut cluster_dir = [0.0f32; 3];
            let mut cluster_avg = 0.0f32;
            for &plan_idx in cluster {
                cluster_dir = add(cluster_dir, plans[plan_idx].direction);
                cluster_avg += plans[plan_idx].distance;
            }
            if len(cluster_dir) < 1e-6 {
                cluster_dir = root_dir;
            } else {
                cluster_dir = norm(cluster_dir);
            }
            cluster_avg /= cluster.len().max(1) as f32;
            let cluster_len = (cluster_avg * params.cluster_split_fraction.max(0.05)).max(0.02);
            let cluster_anchor = add(root_anchor, scale(cluster_dir, cluster_len));
            let cluster_bend = bend_vector(
                cluster_dir,
                mix_key(seed_lo, id, cluster_idx as u32, salt::DENDRITE_CURL),
                cluster_len * params.axon_curve_lift.max(0.0),
            );
            let cluster_p1 = add(
                add(root_anchor, scale(cluster_dir, cluster_len * 0.35)),
                scale(cluster_bend, 0.30),
            );
            let cluster_p2 = add(
                add(cluster_anchor, scale(cluster_dir, -cluster_len * 0.28)),
                scale(cluster_bend, -0.15),
            );
            let (next_path, cluster_complete, _) = emit_bezier_path(
                &mut segments,
                cap,
                &mut dropped,
                id,
                id,
                1,
                root_anchor,
                cluster_p1,
                cluster_p2,
                cluster_anchor,
                cluster_start_radius,
                twig_start_radius,
                params.cluster_branch_samples.max(1),
                root_path_end,
                params.taper_curve,
            );
            if !cluster_complete {
                all_k_coverage = false;
            }
            let cluster_path_end = next_path;

            for &plan_idx in cluster {
                let plan = &mut plans[plan_idx];
                let (socket_pos, socket_idx, socket_distance) =
                    target_socket(seed_lo, id, plan, params);
                plan.socket_pos = socket_pos;
                plan.socket_idx = socket_idx;
                plan.socket_distance = socket_distance;
                terminal_socket_distance_bands[if socket_distance < params.socket_radius_lo * 0.5 {
                    0
                } else if socket_distance < params.socket_radius_lo * 0.9 {
                    1
                } else if socket_distance < params.socket_radius_hi * 1.1 {
                    2
                } else {
                    3
                }] += 1;
                socket_reuse_bands[socket_idx.min(3)] += 1;

                let twig_dir = norm(sub(socket_pos, cluster_anchor));
                let twig_len = len(sub(socket_pos, cluster_anchor)).max(1e-4);
                let twig_bend = bend_vector(
                    twig_dir,
                    mix_key(seed_lo, id, plan.target_id, salt::AXON_BOW),
                    twig_len * params.axon_curve_lift.max(0.0),
                );
                let twig_p1 = add(
                    add(cluster_anchor, scale(twig_dir, twig_len * 0.32)),
                    scale(twig_bend, 0.30),
                );
                let twig_p2 = add(
                    add(socket_pos, scale(twig_dir, -twig_len * 0.24)),
                    scale(twig_bend, -0.15),
                );
                let (next_path, twig_complete, _) = emit_bezier_path(
                    &mut segments,
                    cap,
                    &mut dropped,
                    id,
                    plan.target_id,
                    1,
                    cluster_anchor,
                    twig_p1,
                    twig_p2,
                    socket_pos,
                    twig_start_radius,
                    twig_end_radius,
                    params.terminal_twig_samples.max(1),
                    cluster_path_end,
                    params.taper_curve,
                );
                if !twig_complete {
                    all_k_coverage = false;
                } else {
                    unique_targets_emitted += 1;
                }
                let _ = next_path;
            }
        }
        axon_ms += axon_start.elapsed_ms();
    }

    let finalize_start = MorphTimer::start();

    if dropped > 0 {
        eprintln!(
            "[morphology] segment cap {cap} hit: {dropped} segments dropped (raise max_segs_per_neuron)"
        );
    }

    let finalize_ms = finalize_start.elapsed_ms();
    let total_ms = setup_ms + dendrite_ms + axon_ms + finalize_ms;
    let segment_count = segments.len();
    let segment_cap_bytes = cap * std::mem::size_of::<MorphSegment>();
    let segment_buffer_bytes = segment_count * std::mem::size_of::<MorphSegment>();
    let cap_utilization = if cap == 0 {
        0.0
    } else {
        segment_count as f32 / cap as f32
    };
    let unique_target_coverage = if unique_targets_expected == 0 {
        1.0
    } else {
        unique_targets_emitted as f32 / unique_targets_expected as f32
    };
    Morphology {
        segments,
        dropped,
        stats: MorphologyStats {
            segment_count,
            dropped_count: dropped,
            segment_cap: cap,
            segment_cap_bytes,
            segment_buffer_bytes,
            cap_utilization,
            duplicate_targets,
            self_targets,
            unique_targets_expected,
            unique_targets_emitted,
            unique_target_coverage,
            all_k_coverage,
            timings: MorphologyTimings {
                setup_ms,
                dendrite_ms,
                axon_ms,
                finalize_ms,
                total_ms,
            },
            source_type_bytes: source_types.len(),
            source_type_excitatory,
            source_type_inhibitory,
            cluster_count_histogram,
            terminal_socket_distance_bands,
            socket_reuse_bands,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_grid() -> (Vec<[f32; 3]>, SpatialGrid) {
        // A little cube of neurons so connectivity::target has real cells.
        let mut pos = Vec::new();
        let side = 6;
        for z in 0..side {
            for y in 0..side {
                for x in 0..side {
                    pos.push([x as f32 * 0.15, y as f32 * 0.15, z as f32 * 0.15]);
                }
            }
        }
        let g = SpatialGrid::build(&pos, side as u32);
        (pos, g)
    }

    fn small_regions(len: usize) -> Vec<RegionKind> {
        (0..len)
            .map(|i| match i % 3 {
                0 => RegionKind::Input,
                1 => RegionKind::Association,
                _ => RegionKind::Output,
            })
            .collect()
    }

    #[test]
    fn locked_default_matches_current_constants() {
        let p = MorphologyParams::locked_default();
        assert_eq!(p.base_radius, params::R0);
        assert_eq!(p.dendrite_primary_min, params::DENDRITE_MIN);
        assert_eq!(p.dendrite_primary_span, params::DENDRITE_SPAN);
        assert_eq!(p.axon_stop_fraction, params::AXON_STOP_FRACTION);
        assert_eq!(p.axon_root_radius_fraction, params::AXON_R0_FRACTION);
        assert_eq!(p.axon_curve_lift, 0.15);
        assert_eq!(p.socket_count_min, 2);
        assert_eq!(p.socket_count_max, 4);
        assert_eq!(p.cluster_min, 2);
        assert_eq!(p.cluster_max, 5);
        assert_eq!(p.trunk_root_samples, 2);
        assert_eq!(p.cluster_branch_samples, 2);
        assert_eq!(p.terminal_twig_samples, 3);
        assert_eq!(p.dendrite_budget, DENDRITE_MAX);
        assert_eq!(p.terminal_twig_budget, 4);
        assert_eq!(p.cap_slack, 4);
    }

    #[test]
    fn segment_layout_is_48_bytes() {
        assert_eq!(std::mem::size_of::<MorphSegment>(), 48);
        assert_eq!(std::mem::size_of::<MorphSegment>() % 16, 0);
    }

    #[test]
    fn sphere_instance_layout_is_32_bytes() {
        assert_eq!(std::mem::size_of::<MorphSphereInstance>(), 32);
        assert_eq!(std::mem::size_of::<MorphSphereInstance>() % 16, 0);
    }

    #[test]
    fn generates_segments_for_every_neuron() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(1234, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(&pos, &g, 16, 1234, &params, &source_types);
        // Every neuron contributes at least one dendrite + (usually) axon segment.
        assert!(!m.segments.is_empty());
        assert_eq!(m.dropped, 0, "should not hit the cap at this size");
        assert_eq!(m.stats.segment_count, m.segments.len());
        assert!(m.stats.all_k_coverage, "expected current all-K coverage");
        assert_eq!(m.stats.unique_target_coverage, 1.0);
        assert_eq!(m.stats.source_type_bytes, pos.len());
        assert_eq!(
            m.stats.source_type_excitatory + m.stats.source_type_inhibitory,
            pos.len()
        );
        assert_eq!(
            m.stats.cluster_count_histogram.iter().sum::<u32>() as usize,
            pos.len()
        );
        assert!(m.stats.terminal_socket_distance_bands.iter().sum::<u32>() > 0);
        assert!(m.stats.socket_reuse_bands.iter().sum::<u32>() > 0);
        assert!(m.stats.cap_utilization > 0.0 && m.stats.cap_utilization <= 1.0);
        // All segment neuron_ids and target_ids are in range.
        for s in &m.segments {
            assert!((s.neuron_id as usize) < pos.len());
            assert!((s.target_id as usize) < pos.len(), "target_id out of range");
            assert!(s.kind == 0 || s.kind == 1);
            assert!(s.radius_a > 0.0 && s.radius_b > 0.0);
            if s.kind == 0 {
                // Dendrites carry self as target (unused).
                assert_eq!(s.target_id, s.neuron_id, "dendrite target_id must be self");
            }
        }
        // Both kinds present.
        assert!(m.segments.iter().any(|s| s.kind == 0), "no dendrites");
        assert!(m.segments.iter().any(|s| s.kind == 1), "no axons");
        // Axon segments carry a real (non-self) target neuron.
        assert!(
            m.segments
                .iter()
                .any(|s| s.kind == 1 && s.target_id != s.neuron_id),
            "axon segments should point at distinct target neurons"
        );
    }

    #[test]
    fn emits_one_terminal_per_unique_target_under_real_source_types() {
        let (pos, g) = small_grid();
        let k = 8usize;
        let seed = 4242u32;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(&pos, &g, k, seed, &params, &source_types);
        let probe = (0..pos.len() as u32)
            .find(|&nid| {
                let src_type = source_types[nid as usize];
                let src_cell = g.unpack(g.cell_of_index(nid));
                let mut expected: Vec<u32> = (0..k as u32)
                    .map(|j| {
                        connectivity::target_with_cell(nid, j, &g, k, seed, src_type, src_cell)
                    })
                    .filter(|&t| t != nid)
                    .collect();
                expected.sort_unstable();
                expected.dedup();
                expected.len() > 1
            })
            .expect("need a probe with >1 unique targets");

        let probe_type = source_types[probe as usize];
        let probe_cell = g.unpack(g.cell_of_index(probe));
        let mut expected: Vec<u32> = (0..k as u32)
            .map(|j| connectivity::target_with_cell(probe, j, &g, k, seed, probe_type, probe_cell))
            .filter(|&t| t != probe)
            .collect();
        expected.sort_unstable();
        expected.dedup();

        let mut got: Vec<u32> = m
            .segments
            .iter()
            .filter(|s| s.kind == 1 && s.neuron_id == probe && s.target_id != probe)
            .map(|s| s.target_id)
            .collect();
        got.sort_unstable();
        got.dedup();

        assert_eq!(
            got, expected,
            "terminal twigs must cover all unique non-self targets"
        );
        assert!(
            m.segments
                .iter()
                .any(|s| s.kind == 1 && s.neuron_id == probe && s.target_id == probe),
            "shared root/cluster segments should carry source target_id"
        );
        assert_eq!(m.dropped, 0, "all-K cap should not drop at this size");
    }

    #[test]
    fn single_target_path_emits_direct_twig_without_shared_root() {
        let (pos, g) = small_grid();
        let seed = 2468u32;
        let k = 1usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(&pos, &g, k, seed, &params, &source_types);
        let probe = (0..pos.len() as u32)
            .find(|&nid| {
                let src_type = source_types[nid as usize];
                let src_cell = g.unpack(g.cell_of_index(nid));
                connectivity::target_with_cell(nid, 0, &g, k, seed, src_type, src_cell) != nid
            })
            .expect("need a single-target probe");
        let src_type = source_types[probe as usize];
        let src_cell = g.unpack(g.cell_of_index(probe));
        let expected = connectivity::target_with_cell(probe, 0, &g, k, seed, src_type, src_cell);
        assert_ne!(expected, probe);
        let got: Vec<u32> = m
            .segments
            .iter()
            .filter(|s| s.kind == 1 && s.neuron_id == probe)
            .map(|s| s.target_id)
            .collect();
        assert!(
            !got.is_empty(),
            "single-target probe should still emit axon segments"
        );
        assert!(
            got.iter().all(|&t| t == expected),
            "direct twig should point only at the unique target"
        );
        assert!(
            !m.segments
                .iter()
                .any(|s| s.kind == 1 && s.neuron_id == probe && s.target_id == probe),
            "single-target path should not emit shared-root segments"
        );
    }

    #[test]
    fn mixed_ei_source_types_match_target_with_cell() {
        let (pos, g) = small_grid();
        let seed = 5150u32;
        let k = 8usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(&pos, &g, k, seed, &params, &source_types);

        let mut exc_probe = None;
        let mut inh_probe = None;
        for (i, &t) in source_types.iter().enumerate() {
            if t & 0x01 == 0 && exc_probe.is_none() {
                exc_probe = Some(i as u32);
            }
            if t & 0x01 == 1 && inh_probe.is_none() {
                inh_probe = Some(i as u32);
            }
            if exc_probe.is_some() && inh_probe.is_some() {
                break;
            }
        }
        let exc_probe = exc_probe.expect("need an excitatory probe");
        let inh_probe = inh_probe.expect("need an inhibitory probe");
        assert_ne!(
            source_types[exc_probe as usize] & 0x01,
            source_types[inh_probe as usize] & 0x01
        );

        for &probe in [exc_probe, inh_probe].iter() {
            let src_type = source_types[probe as usize];
            let src_cell = g.unpack(g.cell_of_index(probe));
            let mut expected: Vec<u32> = (0..k as u32)
                .map(|j| connectivity::target_with_cell(probe, j, &g, k, seed, src_type, src_cell))
                .filter(|&t| t != probe)
                .collect();
            expected.sort_unstable();
            expected.dedup();

            let mut got: Vec<u32> = m
                .segments
                .iter()
                .filter(|s| s.kind == 1 && s.neuron_id == probe && s.target_id != probe)
                .map(|s| s.target_id)
                .collect();
            got.sort_unstable();
            got.dedup();

            assert_eq!(
                got, expected,
                "morphology target bytes must match target_with_cell for probe {probe}"
            );
        }
    }

    #[test]
    fn deterministic_for_same_seed() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(99, &regions);
        let params = MorphologyParams::locked_default();
        let a = generate(&pos, &g, 16, 99, &params, &source_types);
        let b = generate(&pos, &g, 16, 99, &params, &source_types);
        assert_eq!(a.segments.len(), b.segments.len());
        assert_eq!(
            a.stats.cluster_count_histogram,
            b.stats.cluster_count_histogram
        );
        assert_eq!(
            a.stats.terminal_socket_distance_bands,
            b.stats.terminal_socket_distance_bands
        );
        assert_eq!(a.stats.socket_reuse_bands, b.stats.socket_reuse_bands);
        for (x, y) in a.segments.iter().zip(b.segments.iter()) {
            assert_eq!(x.a, y.a);
            assert_eq!(x.b, y.b);
            assert_eq!(x.path_len, y.path_len);
        }
    }

    #[test]
    fn seed_changes_morphology() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(1, &regions);
        let source_types_b = build_source_types(2, &regions);
        let params = MorphologyParams::locked_default();
        let a = generate(&pos, &g, 16, 1, &params, &source_types);
        let b = generate(&pos, &g, 16, 2, &params, &source_types_b);
        let differ = a
            .segments
            .iter()
            .zip(b.segments.iter())
            .filter(|(x, y)| x.a != y.a || x.b != y.b)
            .count();
        assert!(differ > 0, "seed had no effect on morphology");
    }

    #[test]
    fn soma_segments_start_at_path_zero() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(7, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(&pos, &g, 16, 7, &params, &source_types);
        // The first segment of each branch (touching the soma) has path_len 0.
        let zero_count = m.segments.iter().filter(|s| s.path_len == 0.0).count();
        assert!(
            zero_count >= pos.len(),
            "expected ≥1 root segment per neuron"
        );
    }

    #[test]
    fn socket_landing_distance_is_bounded() {
        let (pos, g) = small_grid();
        let seed = 9001u32;
        let k = 8usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let _m = generate(&pos, &g, k, seed, &params, &source_types);
        let probe = (0..pos.len() as u32)
            .find(|&nid| {
                let src_type = source_types[nid as usize];
                let src_cell = g.unpack(g.cell_of_index(nid));
                let mut expected: Vec<u32> = (0..k as u32)
                    .map(|j| {
                        connectivity::target_with_cell(nid, j, &g, k, seed, src_type, src_cell)
                    })
                    .filter(|&t| t != nid)
                    .collect();
                expected.sort_unstable();
                expected.dedup();
                expected.len() > 1
            })
            .expect("need a probe with terminals");
        let src_type = source_types[probe as usize];
        let src_cell = g.unpack(g.cell_of_index(probe));
        let target_id = (0..k as u32)
            .map(|j| connectivity::target_with_cell(probe, j, &g, k, seed, src_type, src_cell))
            .find(|&t| t != probe)
            .expect("need a non-self target");
        let plan = TargetPlan {
            target_id,
            source_pos: pos[probe as usize],
            target_pos: pos[target_id as usize],
            direction: norm(sub(pos[target_id as usize], pos[probe as usize])),
            distance: len(sub(pos[target_id as usize], pos[probe as usize])),
            socket_idx: 0,
            socket_pos: pos[target_id as usize],
            socket_distance: 0.0,
        };
        let (socket_pos, socket_idx, socket_distance) = target_socket(seed, probe, &plan, &params);
        assert!(socket_idx < params.socket_count_max);
        assert!(socket_distance >= params.socket_radius_lo * params.axon_stop_fraction * 0.99);
        assert!(socket_distance <= params.socket_radius_hi * params.axon_stop_fraction * 1.01);
        let expected_socket_gap = len(sub(socket_pos, pos[target_id as usize]));
        assert!((expected_socket_gap - socket_distance).abs() < 1e-6);
    }

    #[test]
    fn stats_json_contains_core_fields() {
        let stats = MorphologyStats {
            segment_count: 10,
            dropped_count: 1,
            segment_cap: 12,
            segment_cap_bytes: 576,
            segment_buffer_bytes: 480,
            cap_utilization: 0.8333333,
            duplicate_targets: 2,
            self_targets: 3,
            source_type_bytes: 4,
            source_type_excitatory: 3,
            source_type_inhibitory: 1,
            cluster_count_histogram: [0, 1, 2, 3, 4, 5],
            terminal_socket_distance_bands: [1, 2, 3, 4],
            socket_reuse_bands: [5, 6, 7, 8],
            unique_targets_expected: 7,
            unique_targets_emitted: 7,
            unique_target_coverage: 1.0,
            all_k_coverage: true,
            timings: MorphologyTimings {
                setup_ms: 0.1,
                dendrite_ms: 0.2,
                axon_ms: 0.3,
                finalize_ms: 0.0,
                total_ms: 0.6,
            },
        };
        let json = stats.to_json();
        for field in [
            "\"segment_count\":10",
            "\"dropped_count\":1",
            "\"segment_cap\":12",
            "\"segment_buffer_bytes\":480",
            "\"cluster_count_histogram\":[0,1,2,3,4,5]",
            "\"terminal_socket_distance_bands\":[1,2,3,4]",
            "\"socket_reuse_bands\":[5,6,7,8]",
            "\"all_k_coverage\":true",
            "\"setup_ms\":0.100",
            "\"total_ms\":0.600",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }
    }
}
