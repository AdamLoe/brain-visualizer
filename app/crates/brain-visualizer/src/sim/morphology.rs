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
    /// Axon perpendicular arc seed.
    pub const AXON_BOW: u32 = 0x00A0_0004;
    // Prim-like axon tree growth + relaxation draws (morphology-branching-tree).
    // Disjoint from connectivity::salt (≤4) AND from the dendrite salts above so
    // a tree draw never collides with a target/weight or dendrite draw.
    pub const TREE_SPLIT: u32 = 0x00A0_0005; // mid-edge split-point jitter
    pub const TREE_BEND: u32 = 0x00A0_0006; // per-edge Bézier bow seed
                                            // Local-branching grammar (Stream D — bushy dendrites). Decorrelated from the
                                            // dendrite/tree salts above so a branchlet/twig draw never collides with a
                                            // root/fork/leaf curve draw or a target/weight draw.
    pub const DENDRITE_BRANCHLET: u32 = 0x00A0_0007; // secondary branchlet direction/length
    pub const DENDRITE_TWIG: u32 = 0x00A0_0008; // terminal twig direction/length
    pub const DENDRITE_TWIG_CURL: u32 = 0x00A0_0009; // per-twig curl variation
}

/// Per-neuron morphology tuning parameters (world units; tuned to the ~0.15
/// inter-neuron spacing at N=1200 so neighbouring trees nearly touch but do not
/// fuse into a hairball).
pub mod params {
    /// Soma-end dendrite/axon radius (world units).
    pub const R0: f32 = 0.006;
    /// Axon stops short of the target so boutons cluster near the target's
    /// dendrites rather than inside its soma.
    pub const AXON_STOP_FRACTION: f32 = 0.85;
    /// Axon trunk radius at the soma (fraction of R0).
    pub const AXON_R0_FRACTION: f32 = 0.90;
}

/// Locked morphology parameter preset used by the generator.
///
/// Classification notes for the current stream:
/// - generator-default: base radius, dendrite socket tuning, axon stop/radius
/// - review-override: axon curve lift (mirrors the live visual curve setting)
/// - protected: allocation slack and branch-segment count
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphologyParams {
    pub base_radius: f32,
    pub axon_stop_fraction: f32,
    pub axon_root_radius_fraction: f32,
    pub axon_curve_lift: f32,
    pub socket_count_min: usize,
    pub socket_count_max: usize,
    pub socket_radius_lo: f32,
    pub socket_radius_hi: f32,
    pub socket_tip_preference: f32,
    pub trunk_length_fraction: f32,
    pub twig_radius_fraction: f32,
    pub taper_curve: f32,
    /// Bounded count of soma-proximal dendrite collars/root forks per target.
    pub dendrite_primary_root_count: usize,
    /// First-fork distance from soma center, expressed as a base-radius multiple.
    pub dendrite_fork_distance: f32,
    /// Tangential bend strength for dendrite root/fork curves.
    pub dendrite_curve_tightness: f32,
    /// Dendrite mid-point radius as a fraction of base_radius (soma → bifurcation).
    pub dendrite_mid_radius_fraction: f32,
    /// Dendrite tip radius as a fraction of base_radius (bifurcation → tip).
    pub dendrite_tip_radius_fraction: f32,
    /// Tangential separation between source-specific incoming groups.
    pub dendrite_group_spacing: f32,
    // ── Local bushy branching (Stream D — protected, not web-exposed) ───────────
    /// Decorative secondary branchlets sprouted off each group's mid-fork to read
    /// as a bushy local arbor. These carry NO synapse — owner is the neuron's own
    /// id (light with the neuron), never a fake target. Bounded count.
    pub dendrite_branchlet_count: usize,
    /// Length of a secondary branchlet as a fraction of its parent fork edge.
    pub dendrite_branchlet_length_fraction: f32,
    /// Tip radius of a secondary branchlet as a fraction of base_radius.
    pub dendrite_branchlet_radius_fraction: f32,
    /// Decorative terminal twigs sprouted at each group's tip (group_fork). Owner
    /// is the neuron's own id (decorative, no synapse). Bounded count.
    pub dendrite_twig_count: usize,
    /// Length of a terminal twig as a fraction of base_radius.
    pub dendrite_twig_length_fraction: f32,
    /// Tip radius of a terminal twig as a fraction of base_radius (very thin — the
    /// thin end of the trunk→twig hierarchy).
    pub dendrite_twig_radius_fraction: f32,
    /// Curvature multiplier applied to branchlet/twig bends so local processes
    /// curl more than the trunk (curvature VARIATION, not one bow per edge).
    pub dendrite_twig_curl: f32,
    /// Max number of incoming groups (per neuron, in deterministic sort order) that
    /// receive decorative branchlets/twigs. Caps the extra segment growth so a very
    /// high in-degree target does not explode the dendrite budget — the bushy look
    /// reads from the soma-proximal groups regardless. The segment cap is sized
    /// against this bound.
    pub dendrite_decor_group_max: usize,
    // ── Prim-like axon tree growth + relaxation (morphology-branching-tree) ──
    /// Curvature-penalty weight in the attach score (resist sharp bends).
    pub tree_score_curvature: f32,
    /// Density/repel-penalty weight in the attach score (avoid crowding).
    pub tree_score_density: f32,
    /// Degree-penalty weight in the attach score (soft 2–3-child fork tendency).
    pub tree_score_degree: f32,
    /// Relaxation pull-to-mean strength per pass.
    pub relax_lerp: f32,
    /// Relaxation sibling/branch repulsion strength.
    pub relax_repel: f32,
    /// Ancestor-window depth relaxed per attach.
    pub relax_window: usize,
    /// Bézier samples emitted per tree edge (curvature smoothness).
    ///
    /// Legacy/global control retained for the dev-panel slider and as the
    /// `min_subsegments` floor for the adaptive rule. Per-edge subsegment counts
    /// are now driven by [`adaptive_subsegments`] (length + curvature aware), not
    /// by this flat value.
    pub edge_subsegments: usize,
    // ── Adaptive subdivision (length + curvature aware; protected) ──────────────
    /// Target max world length of one emitted sub-segment on a LOCAL edge. Edges
    /// longer than this get `ceil(len / max_segment_length)` samples. Protected
    /// (not exposed to web config) so the GPU buffer cap is sized deterministically.
    pub max_segment_length: f32,
    /// Target max sub-segment length on a LONG-RANGE edge. Deliberately SMALLER
    /// than `max_segment_length` so long fibers carry more spatial samples for
    /// readable pulse motion.
    pub long_range_max_segment_length: f32,
    /// Extra samples added in proportion to an edge's curvature (bend magnitude
    /// over chord length). Deterministic — curvature comes from the salt-seeded
    /// bend vector, not runtime state.
    pub curvature_subsegment_boost: f32,
    /// Hard upper clamp on the adaptive per-edge subsegment count. The segment
    /// cap is sized against this (protected).
    pub edge_subsegments_max: usize,
    /// Hard lower clamp on the adaptive per-edge subsegment count.
    pub min_subsegments: usize,
    // ── Long-range waypoint routing (protected) ─────────────────────────────────
    /// A leaf axon edge whose chord exceeds `long_range_chord_cells * cell_size`
    /// is "visually long" and is routed through intermediate waypoints instead of
    /// one giant span. Distance heuristic — connectivity exposes no per-synapse
    /// long-range flag (the heavy-tail coin is baked into the target id).
    pub long_range_chord_cells: f32,
    /// Maximum number of intermediate waypoints inserted on a long-range leaf
    /// edge (1..=this). Bounded so the segment budget stays predictable.
    pub long_range_max_waypoints: usize,
    /// Approximate world span each waypoint-to-waypoint hop should cover; the
    /// waypoint count grows with chord length up to `long_range_max_waypoints`.
    pub long_range_waypoint_span: f32,
    /// Lateral detour magnitude (world units) applied at waypoints to curve the
    /// fiber around the brain volume rather than crossing straight through it.
    pub long_range_lateral_offset: f32,
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
            axon_stop_fraction: params::AXON_STOP_FRACTION,
            axon_root_radius_fraction: params::AXON_R0_FRACTION,
            axon_curve_lift: 0.15,
            socket_count_min: 2,
            socket_count_max: 4,
            socket_radius_lo: 0.008,
            socket_radius_hi: 0.018,
            socket_tip_preference: 0.78,
            trunk_length_fraction: 0.32,
            twig_radius_fraction: 0.16,
            taper_curve: 2.1,
            dendrite_primary_root_count: 4,
            dendrite_fork_distance: 1.45,
            dendrite_curve_tightness: 0.55,
            dendrite_mid_radius_fraction: 0.78,
            dendrite_tip_radius_fraction: 0.42,
            dendrite_group_spacing: 0.55,
            // Local bushy branching. Bounded so the dendrite arbor stays under
            // DENDRITE_MAX even for high in-degree targets: decoration is
            // sprouted on only the first `dendrite_decor_group_max` groups per
            // neuron, each adding at most (branchlet_count + twig_count) short
            // edges.
            dendrite_branchlet_count: 1,
            dendrite_branchlet_length_fraction: 0.6,
            dendrite_branchlet_radius_fraction: 0.30,
            dendrite_twig_count: 1,
            dendrite_twig_length_fraction: 1.1,
            dendrite_twig_radius_fraction: 0.18,
            dendrite_twig_curl: 1.6,
            dendrite_decor_group_max: 12,
            tree_score_curvature: 0.5,
            tree_score_density: 0.5,
            tree_score_degree: 0.7,
            relax_lerp: 0.25,
            relax_repel: 0.15,
            relax_window: 3,
            edge_subsegments: 3,
            // Adaptive subdivision (length + curvature aware). Tuned to the
            // ~0.15 inter-neuron spacing at N=1200: local edges are a fraction of
            // that (≈1–3 samples), long-range fibers get a smaller max so pulse
            // motion has enough spatial samples to read.
            max_segment_length: 0.05,
            long_range_max_segment_length: 0.025,
            curvature_subsegment_boost: 2.0,
            edge_subsegments_max: EDGE_SUBSEGMENTS_MAX,
            min_subsegments: 1,
            // Long-range waypoint routing. A leaf chord longer than
            // long_range_chord_cells × cell_size routes through up to
            // long_range_max_waypoints intermediate points, each hop bounded near
            // long_range_waypoint_span so no single span crosses the whole view.
            long_range_chord_cells: 3.0,
            long_range_max_waypoints: 3,
            long_range_waypoint_span: 0.20,
            long_range_lateral_offset: 0.12,
            dendrite_budget: DENDRITE_MAX,
            // Prim tree: ≤ ~2k edges. Sized for the worst-case adaptive emission
            // so the cap never under-allocates. trunk_cluster_budget is the
            // per-arbor fixed overhead (root edge headroom, one adaptive edge).
            // The per-target term carries the bulk: a long-range leaf can route
            // through (long_range_max_waypoints + 1) hops, each emitting up to
            // edge_subsegments_max samples; plus a non-leaf trunk/fork edge.
            trunk_cluster_budget: EDGE_SUBSEGMENTS_MAX,
            terminal_twig_budget: (LONG_RANGE_MAX_WAYPOINTS + 1) * EDGE_SUBSEGMENTS_MAX
                + EDGE_SUBSEGMENTS_MAX,
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
            "{{\"base_radius\":{:.6},\"axon_stop_fraction\":{:.6},\"axon_root_radius_fraction\":{:.6},\"axon_curve_lift\":{:.6},\"socket_count_min\":{},\"socket_count_max\":{},\"socket_radius_lo\":{:.6},\"socket_radius_hi\":{:.6},\"socket_tip_preference\":{:.6},\"trunk_length_fraction\":{:.6},\"twig_radius_fraction\":{:.6},\"taper_curve\":{:.6},\"dendrite_primary_root_count\":{},\"dendrite_fork_distance\":{:.6},\"dendrite_curve_tightness\":{:.6},\"dendrite_mid_radius_fraction\":{:.6},\"dendrite_tip_radius_fraction\":{:.6},\"dendrite_group_spacing\":{:.6},\"tree_score_curvature\":{:.6},\"tree_score_density\":{:.6},\"tree_score_degree\":{:.6},\"relax_lerp\":{:.6},\"relax_repel\":{:.6},\"relax_window\":{},\"edge_subsegments\":{},\"dendrite_budget\":{},\"trunk_cluster_budget\":{},\"terminal_twig_budget\":{},\"cap_slack\":{}}}",
            self.base_radius,
            self.axon_stop_fraction,
            self.axon_root_radius_fraction,
            self.axon_curve_lift,
            self.socket_count_min,
            self.socket_count_max,
            self.socket_radius_lo,
            self.socket_radius_hi,
            self.socket_tip_preference,
            self.trunk_length_fraction,
            self.twig_radius_fraction,
            self.taper_curve,
            self.dendrite_primary_root_count,
            self.dendrite_fork_distance,
            self.dendrite_curve_tightness,
            self.dendrite_mid_radius_fraction,
            self.dendrite_tip_radius_fraction,
            self.dendrite_group_spacing,
            self.tree_score_curvature,
            self.tree_score_density,
            self.tree_score_degree,
            self.relax_lerp,
            self.relax_repel,
            self.relax_window,
            self.edge_subsegments,
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

/// Generator controls — the exposed `MorphologyParams` fields (budgets/slack
/// and salts are protected and excluded). `#[serde(default)]` so a partial JSON
/// blob falls back to the locked defaults per field.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorConfig {
    pub base_radius: f32,
    pub axon_stop_fraction: f32,
    pub axon_root_radius_fraction: f32,
    pub axon_curve_lift: f32,
    pub socket_count_min: usize,
    pub socket_count_max: usize,
    pub socket_radius_lo: f32,
    pub socket_radius_hi: f32,
    pub socket_tip_preference: f32,
    pub trunk_length_fraction: f32,
    pub twig_radius_fraction: f32,
    pub taper_curve: f32,
    pub dendrite_primary_root_count: usize,
    pub dendrite_fork_distance: f32,
    pub dendrite_curve_tightness: f32,
    pub dendrite_mid_radius_fraction: f32,
    pub dendrite_tip_radius_fraction: f32,
    pub dendrite_group_spacing: f32,
    pub tree_score_curvature: f32,
    pub tree_score_density: f32,
    pub tree_score_degree: f32,
    pub relax_lerp: f32,
    pub relax_repel: f32,
    pub relax_window: usize,
    pub edge_subsegments: usize,
    pub max_segment_length: f32,
    pub long_range_max_segment_length: f32,
    pub curvature_subsegment_boost: f32,
    pub edge_subsegments_max: usize,
    pub min_subsegments: usize,
    // ── Dendrite decoration controls (Stream F) ──────────────────────────────
    /// Decorative secondary branchlets per group (0 = none, 1 = one per group).
    /// Clamped to DENDRITE_BRANCHLET_MAX at generation time.
    pub dendrite_branchlet_count: usize,
    /// Decorative terminal twigs per group (0 = none, 1–2 = bushy).
    /// Clamped to DENDRITE_TWIG_MAX at generation time.
    pub dendrite_twig_count: usize,
    /// How many incoming groups (per neuron) receive bushy decoration.
    /// Clamped to DENDRITE_DECOR_GROUP_MAX at generation time.
    pub dendrite_decor_group_max: usize,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self::from_params(&MorphologyParams::locked_default())
    }
}

impl GeneratorConfig {
    /// Extract the generator-tunable fields from a `MorphologyParams`.
    pub fn from_params(p: &MorphologyParams) -> Self {
        Self {
            base_radius: p.base_radius,
            axon_stop_fraction: p.axon_stop_fraction,
            axon_root_radius_fraction: p.axon_root_radius_fraction,
            axon_curve_lift: p.axon_curve_lift,
            socket_count_min: p.socket_count_min,
            socket_count_max: p.socket_count_max,
            socket_radius_lo: p.socket_radius_lo,
            socket_radius_hi: p.socket_radius_hi,
            socket_tip_preference: p.socket_tip_preference,
            trunk_length_fraction: p.trunk_length_fraction,
            twig_radius_fraction: p.twig_radius_fraction,
            taper_curve: p.taper_curve,
            dendrite_primary_root_count: p.dendrite_primary_root_count,
            dendrite_fork_distance: p.dendrite_fork_distance,
            dendrite_curve_tightness: p.dendrite_curve_tightness,
            dendrite_mid_radius_fraction: p.dendrite_mid_radius_fraction,
            dendrite_tip_radius_fraction: p.dendrite_tip_radius_fraction,
            dendrite_group_spacing: p.dendrite_group_spacing,
            tree_score_curvature: p.tree_score_curvature,
            tree_score_density: p.tree_score_density,
            tree_score_degree: p.tree_score_degree,
            relax_lerp: p.relax_lerp,
            relax_repel: p.relax_repel,
            relax_window: p.relax_window,
            edge_subsegments: p.edge_subsegments,
            max_segment_length: p.max_segment_length,
            long_range_max_segment_length: p.long_range_max_segment_length,
            curvature_subsegment_boost: p.curvature_subsegment_boost,
            edge_subsegments_max: p.edge_subsegments_max,
            min_subsegments: p.min_subsegments,
            dendrite_branchlet_count: p.dendrite_branchlet_count,
            dendrite_twig_count: p.dendrite_twig_count,
            dendrite_decor_group_max: p.dendrite_decor_group_max,
        }
    }

    /// Apply these exposed fields onto a base `MorphologyParams`, preserving the base's
    /// protected budget/slack fields (`dendrite_budget`, `trunk_cluster_budget`,
    /// `terminal_twig_budget`, `cap_slack`).
    pub fn apply_to(&self, base: &MorphologyParams) -> MorphologyParams {
        let min_subsegments = self.min_subsegments.clamp(1, EDGE_SUBSEGMENTS_MAX);
        let edge_subsegments_max = self
            .edge_subsegments_max
            .clamp(min_subsegments, EDGE_SUBSEGMENTS_MAX);
        MorphologyParams {
            base_radius: self.base_radius,
            axon_stop_fraction: self.axon_stop_fraction,
            axon_root_radius_fraction: self.axon_root_radius_fraction,
            axon_curve_lift: self.axon_curve_lift,
            socket_count_min: self.socket_count_min,
            socket_count_max: self.socket_count_max,
            socket_radius_lo: self.socket_radius_lo,
            socket_radius_hi: self.socket_radius_hi,
            socket_tip_preference: self.socket_tip_preference,
            trunk_length_fraction: self.trunk_length_fraction,
            twig_radius_fraction: self.twig_radius_fraction,
            taper_curve: self.taper_curve,
            dendrite_primary_root_count: self.dendrite_primary_root_count,
            dendrite_fork_distance: self.dendrite_fork_distance,
            dendrite_curve_tightness: self.dendrite_curve_tightness,
            dendrite_mid_radius_fraction: self.dendrite_mid_radius_fraction,
            dendrite_tip_radius_fraction: self.dendrite_tip_radius_fraction,
            dendrite_group_spacing: self.dendrite_group_spacing,
            tree_score_curvature: self.tree_score_curvature,
            tree_score_density: self.tree_score_density,
            tree_score_degree: self.tree_score_degree,
            relax_lerp: self.relax_lerp,
            relax_repel: self.relax_repel,
            relax_window: self.relax_window,
            edge_subsegments: self.edge_subsegments,
            // Dendrite decoration: web-exposed counts come from self (Stream F);
            // the sub-edge shape params (length/radius/curl fractions) stay locked
            // to the base preset — they are fine-tuned visual constants, not
            // user-tunable knobs. Budgets/slack and waypoint params stay locked;
            // adaptive subdivision controls are clamped to the descriptor/constant
            // maxima that the segment cap is sized against.
            dendrite_branchlet_count: self.dendrite_branchlet_count,
            dendrite_branchlet_length_fraction: base.dendrite_branchlet_length_fraction,
            dendrite_branchlet_radius_fraction: base.dendrite_branchlet_radius_fraction,
            dendrite_twig_count: self.dendrite_twig_count,
            dendrite_twig_length_fraction: base.dendrite_twig_length_fraction,
            dendrite_twig_radius_fraction: base.dendrite_twig_radius_fraction,
            dendrite_twig_curl: base.dendrite_twig_curl,
            dendrite_decor_group_max: self.dendrite_decor_group_max,
            max_segment_length: self.max_segment_length.clamp(0.018, 0.12),
            long_range_max_segment_length: self.long_range_max_segment_length.clamp(0.012, 0.08),
            curvature_subsegment_boost: self.curvature_subsegment_boost.clamp(0.0, 4.0),
            edge_subsegments_max,
            min_subsegments,
            long_range_chord_cells: base.long_range_chord_cells,
            long_range_max_waypoints: base.long_range_max_waypoints,
            long_range_waypoint_span: base.long_range_waypoint_span,
            long_range_lateral_offset: base.long_range_lateral_offset,
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
    // ── True-opacity active layer (active-opacity-render-pass) ─────────────────
    // `active_opacity`: the active-opacity CEILING — a freshly-fired neuron's
    //   alpha in the depth-tested active pass ramps toward this (default 1.0 =
    //   fully opaque). 0.0 skips the active pass entirely (pure additive look).
    // `inactive_opacity_floor`: the inactive-opacity FLOOR in the active layer —
    //   default 0.0 so resting structure is fully hidden in the opaque pass (the
    //   additive resting layer still shows it softly).
    pub active_opacity: f32,
    pub inactive_opacity_floor: f32,
}

impl Default for LightingConfig {
    fn default() -> Self {
        // Defaults locked to the accepted product lighting split. light_dir
        // defaults are the pre-normalised components of
        // normalize(-0.35, 0.55, 0.75).
        Self {
            light_dir_x: -0.352,
            light_dir_y: 0.553,
            light_dir_z: 0.755,
            ambient: 0.55,
            diffuse_intensity: 0.35,
            rim_intensity: 0.30,
            rim_power: 2.0,
            resting_brightness: 0.0,
            active_boost: 1.8,
            active_opacity: 1.0,
            inactive_opacity_floor: 0.0,
        }
    }
}

/// Full morphology config blob — the JSON contract for the dev-panel
/// `set_morphology_config` WASM entry point. `Default` equals the product
/// defaults (generator == `locked_default()` plus current lighting defaults).
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
    pub incoming_ms: f32,
    pub dendrite_ms: f32,
    pub axon_ms: f32,
    pub finalize_ms: f32,
    pub total_ms: f32,
}

/// Structured build/profile facts for the current morphology generation pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MorphologyStats {
    pub neuron_count: usize,
    pub fanout_k: usize,
    pub segment_count: usize,
    pub dropped_count: usize,
    pub segment_cap_per_neuron: usize,
    pub segment_cap: usize,
    pub segment_cap_bytes: usize,
    pub segment_buffer_bytes: usize,
    pub segments_per_neuron_p99: usize,
    pub segments_per_neuron_max: usize,
    pub cap_utilization: f32,
    pub duplicate_targets: usize,
    pub self_targets: usize,
    pub incoming_raw_count: usize,
    pub incoming_socket_group_count: usize,
    pub incoming_in_degree_mean: f32,
    pub incoming_in_degree_p99: usize,
    pub incoming_in_degree_max: usize,
    pub incoming_visible_groups_mean: f32,
    pub incoming_visible_groups_p99: usize,
    pub incoming_visible_groups_max: usize,
    pub incoming_capped_count: usize,
    pub incoming_dropped_count: usize,
    pub incoming_raw_bytes: usize,
    pub incoming_range_bytes: usize,
    pub incoming_group_bytes: usize,
    pub incoming_storage_bytes: usize,
    pub source_type_bytes: usize,
    pub source_type_excitatory: usize,
    pub source_type_inhibitory: usize,
    /// Internal-node fork-degree histogram for the Prim axon tree: index =
    /// child count, index 5 = "5+" (soft-fork evidence). Neurons with no axon
    /// arbor (0 unique targets) increment index 0. Repurposed from the legacy
    /// cluster-count histogram (morphology-branching-tree step 9).
    pub cluster_count_histogram: [u32; 6],
    pub terminal_socket_distance_bands: [u32; 4],
    pub socket_reuse_bands: [u32; 4],
    /// Max axon-tree depth across neurons (shared-trunk evidence).
    pub tree_depth_max: u32,
    /// Mean axon-tree depth across neurons that grew an arbor.
    pub tree_depth_mean: f32,
    /// Axon-segment radius distribution into 4 √-width bands (thinnest→thickest),
    /// fractions of R_trunk: [<0.25, 0.25–0.5, 0.5–0.75, ≥0.75].
    pub radius_bands: [u32; 4],
    pub unique_targets_expected: usize,
    pub unique_targets_emitted: usize,
    pub unique_target_coverage: f32,
    pub all_k_coverage: bool,
    pub timings: MorphologyTimings,
}

impl MorphologyStats {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"neuron_count\":{},\"fanout_k\":{},\"segment_count\":{},\"dropped_count\":{},\"segment_cap_per_neuron\":{},\"segment_cap\":{},\"segment_cap_bytes\":{},\"segment_buffer_bytes\":{},\"segments_per_neuron_p99\":{},\"segments_per_neuron_max\":{},\"cap_utilization\":{:.6},\"duplicate_targets\":{},\"self_targets\":{},\"incoming_raw_count\":{},\"incoming_socket_group_count\":{},\"incoming_in_degree_mean\":{:.6},\"incoming_in_degree_p99\":{},\"incoming_in_degree_max\":{},\"incoming_visible_groups_mean\":{:.6},\"incoming_visible_groups_p99\":{},\"incoming_visible_groups_max\":{},\"incoming_capped_count\":{},\"incoming_dropped_count\":{},\"incoming_raw_bytes\":{},\"incoming_range_bytes\":{},\"incoming_group_bytes\":{},\"incoming_storage_bytes\":{},\"source_type_bytes\":{},\"source_type_excitatory\":{},\"source_type_inhibitory\":{},\"cluster_count_histogram\":[{},{},{},{},{},{}],\"terminal_socket_distance_bands\":[{},{},{},{}],\"socket_reuse_bands\":[{},{},{},{}],\"tree_depth_max\":{},\"tree_depth_mean\":{:.6},\"radius_bands\":[{},{},{},{}],\"unique_targets_expected\":{},\"unique_targets_emitted\":{},\"unique_target_coverage\":{:.6},\"all_k_coverage\":{},\"timings\":{{\"setup_ms\":{:.3},\"incoming_ms\":{:.3},\"dendrite_ms\":{:.3},\"axon_ms\":{:.3},\"finalize_ms\":{:.3},\"total_ms\":{:.3}}}}}",
            self.neuron_count,
            self.fanout_k,
            self.segment_count,
            self.dropped_count,
            self.segment_cap_per_neuron,
            self.segment_cap,
            self.segment_cap_bytes,
            self.segment_buffer_bytes,
            self.segments_per_neuron_p99,
            self.segments_per_neuron_max,
            self.cap_utilization,
            self.duplicate_targets,
            self.self_targets,
            self.incoming_raw_count,
            self.incoming_socket_group_count,
            self.incoming_in_degree_mean,
            self.incoming_in_degree_p99,
            self.incoming_in_degree_max,
            self.incoming_visible_groups_mean,
            self.incoming_visible_groups_p99,
            self.incoming_visible_groups_max,
            self.incoming_capped_count,
            self.incoming_dropped_count,
            self.incoming_raw_bytes,
            self.incoming_range_bytes,
            self.incoming_group_bytes,
            self.incoming_storage_bytes,
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
            self.tree_depth_max,
            self.tree_depth_mean,
            self.radius_bands[0],
            self.radius_bands[1],
            self.radius_bands[2],
            self.radius_bands[3],
            self.unique_targets_expected,
            self.unique_targets_emitted,
            self.unique_target_coverage,
            self.all_k_coverage,
            self.timings.setup_ms,
            self.timings.incoming_ms,
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
/// THE BRANCH ROOT to endpoint `a`; dendrites root at the soma center, axons root
/// at `ProcessRoot::soma_root` (retained for the renderer; no longer drives timing).
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
/// 48 bytes, 16-aligned. Field order + size MUST match `SphereInstance` in
/// render_morphology.wgsl (vs_sphere reads it from a storage buffer).
///
/// Layout (48 B):
///   center: [f32;3] (12 B), radius: f32 (4 B)     → 16 B block 0
///   neuron_id: u32  (4 B),  kind: u32 (4 B),
///   _pad0: u32      (4 B),  _pad1: u32 (4 B)       → 16 B block 1
///   root_dir: [f32;3] (12 B), root_pull: f32 (4 B) → 16 B block 2
///
/// `kind` = 2 (soma). `neuron_id` keys last_spike for spike brightness.
/// `root_dir/root_pull` are baked from `ProcessRoot` so the shader can stretch
/// the soma toward the dominant axon root without scanning branch segments.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MorphSphereInstance {
    pub center: [f32; 3],
    pub radius: f32,
    pub neuron_id: u32,
    pub kind: u32, // 2 = soma
    pub _pad0: u32,
    pub _pad1: u32,
    pub root_dir: [f32; 3],
    pub root_pull: f32,
}

/// Host-side source process-root descriptor. It is generated once per source
/// neuron and is intentionally not uploaded yet; later axon/soma/dendrite
/// streams consume the same deterministic root/socket convention from here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessRoot {
    pub neuron_id: u32,
    pub soma_center: [f32; 3],
    pub direction: [f32; 3],
    pub soma_root: [f32; 3],
    pub first_fork: [f32; 3],
    pub root_radius: f32,
    pub root_weight: u32,
    pub unique_target_count: u32,
}

/// One raw non-self incoming synapse, sorted into the target neuron's range.
/// This is host-side morphology input only; it mirrors production
/// `target_with_cell` without changing the connectivity rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IncomingSynapse {
    pub source_id: u32,
    pub synapse_index: u32,
    pub target_id: u32,
    pub socket_idx: u32,
    pub socket_pos: [f32; 3],
    pub weight: i32,
}

/// Half-open range into an incoming vector (`start..start + len`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncomingRange {
    pub start: usize,
    pub len: usize,
}

/// A visible incoming socket group after duplicate `(source,target,socket)`
/// records are aggregated by absolute synaptic weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IncomingSocketGroup {
    pub source_id: u32,
    pub target_id: u32,
    pub socket_idx: u32,
    pub socket_pos: [f32; 3],
    pub weight: i64,
    pub raw_count: u32,
}

/// Generated morphology: the flat segment list plus the total count. (Per-neuron
/// ranges are implicit in `neuron_id`; the renderer keys off that.)
pub struct Morphology {
    pub segments: Vec<MorphSegment>,
    /// One deterministic source process-root descriptor per neuron.
    pub process_roots: Vec<ProcessRoot>,
    /// Raw non-self incoming synapses, sorted by target range.
    pub incoming_synapses: Vec<IncomingSynapse>,
    /// One range per target neuron into `incoming_synapses`.
    pub incoming_ranges: Vec<IncomingRange>,
    /// Aggregated visible incoming socket groups, sorted by target range.
    pub incoming_socket_groups: Vec<IncomingSocketGroup>,
    /// One range per target neuron into `incoming_socket_groups`.
    pub incoming_socket_group_ranges: Vec<IncomingRange>,
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
    process_roots: &[ProcessRoot],
) -> Vec<MorphSphereInstance> {
    positions
        .iter()
        .enumerate()
        .map(|(i, &pos)| {
            let _ = source_types.get(i); // bounds check only; kind is always 2 (soma)
            let (root_dir, root_pull) = process_roots
                .get(i)
                .filter(|root| root.unique_target_count > 0 && len(root.direction) > 1e-6)
                .map(|root| {
                    let radius_ratio = if params.base_radius > 1e-6 {
                        (root.root_radius / params.base_radius).clamp(0.0, 1.5)
                    } else {
                        0.0
                    };
                    let count_factor = (root.unique_target_count as f32 / 8.0).clamp(0.0, 1.0);
                    let pull = (0.10 + radius_ratio * 0.22 + count_factor * 0.08).clamp(0.0, 0.42);
                    (norm(root.direction), pull)
                })
                .unwrap_or(([0.0; 3], 0.0));
            MorphSphereInstance {
                center: pos,
                radius: params.base_radius,
                neuron_id: i as u32,
                kind: 2,
                _pad0: 0,
                _pad1: 0,
                root_dir,
                root_pull,
            }
        })
        .collect()
}

const DENDRITE_ROOT_SAMPLES: usize = 2;
const DENDRITE_FORK_SAMPLES: usize = 2;
const DENDRITE_TWIG_SAMPLES: usize = 2;
/// Hard upper bound on decorative secondary branchlets per group (the budget is
/// sized against this, so raising `dendrite_branchlet_count` never under-allocates).
pub const DENDRITE_BRANCHLET_MAX: usize = 1;
/// Hard upper bound on decorative terminal twigs per group.
pub const DENDRITE_TWIG_MAX: usize = 2;
/// Hard upper bound on how many incoming groups (per neuron) get the decorative
/// bushy grammar. Bounds the extra-segment growth independent of in-degree; the
/// segment cap is sized against this.
pub const DENDRITE_DECOR_GROUP_MAX: usize = 16;

/// Per-neuron decorated-group budget. Deterministic and bounded by
/// `DENDRITE_DECOR_GROUP_MAX`; high-N storage binding limits are handled by
/// chunked GPU segment buffers rather than by suppressing this grammar.
fn effective_decor_group_max(_n: usize, configured: usize) -> usize {
    configured.min(DENDRITE_DECOR_GROUP_MAX)
}
/// Per-decorative-branch sample clamp. Branchlets/twigs are short soma-proximal
/// processes (a few base radii), so they flow through [`adaptive_subsegments`] but
/// are clamped here — they do not carry travelling pulses, so they never need the
/// fine long-range sampling, and the tighter clamp keeps the dendrite budget small.
const DENDRITE_DECOR_SAMPLES_MAX: usize = 2;
/// Worst-case extra dendrite segments per decorated group from the bushy local
/// grammar (Stream D): the decorative branchlets + terminal twigs, each emitted
/// through [`adaptive_subsegments`] clamped to `DENDRITE_DECOR_SAMPLES_MAX`. The
/// fork/leaf edges already existed pre-D and are budgeted by the legacy headroom.
const DENDRITE_DECOR_PER_GROUP_MAX: usize =
    (DENDRITE_BRANCHLET_MAX + DENDRITE_TWIG_MAX) * DENDRITE_DECOR_SAMPLES_MAX;

/// Dendrite cap headroom for the soma-proximal incoming grammar. The legacy 160
/// covers the soma-proximal root/fork/leaf grammar; the Stream-D bushy local
/// grammar (decorative branchlets + terminal twigs) adds at most
/// `DENDRITE_DECOR_GROUP_MAX × DENDRITE_DECOR_PER_GROUP_MAX` per neuron, and only
/// to the first `dendrite_decor_group_max` groups (bounded independent of
/// in-degree).
pub const DENDRITE_MAX: usize = 160 + DENDRITE_DECOR_GROUP_MAX * DENDRITE_DECOR_PER_GROUP_MAX;

/// Descriptor-max for the exposed `edge_subsegments` knob (MORPH_DESCRIPTORS
/// generator.edgeSubsegments max). The protected segment cap is sized against
/// THIS value, not the default, so raising the slider never under-allocates the
/// GPU buffer (morphology-branching-tree step 8).
pub const EDGE_SUBSEGMENTS_MAX: usize = 4;

/// Max intermediate waypoints inserted on a long-range leaf edge. The segment
/// cap is sized against this (a long leaf routes through up to
/// `LONG_RANGE_MAX_WAYPOINTS + 1` hops). Mirrors the locked-default
/// `long_range_max_waypoints`; both are asserted equal in tests.
pub const LONG_RANGE_MAX_WAYPOINTS: usize = 3;

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
// `target_socket` reads only target_id/source_pos/target_pos; the remaining
// fields are retained for the planning struct shape and the bounded-landing test.
#[allow(dead_code)]
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

/// Deterministic length + curvature aware subsegment count for one edge.
///
/// `subsegments = clamp(ceil(len / max_seg_len) + curvature_bonus, min, max)`
/// where `max_seg_len` is the long-range max when `is_long_range` (smaller, so
/// long fibers get more spatial samples for readable pulse motion) and the
/// curvature bonus is `round(curvature * curvature_subsegment_boost)`. Inputs are
/// already-deterministic geometry (chord length + salt-seeded bend magnitude), so
/// the count never depends on runtime/camera/float-reduction state.
fn adaptive_subsegments(
    edge_len: f32,
    curvature: f32,
    is_long_range: bool,
    params: &MorphologyParams,
) -> usize {
    let max_seg_len = if is_long_range {
        params.long_range_max_segment_length
    } else {
        params.max_segment_length
    }
    .max(1e-4);
    let length_samples = (edge_len / max_seg_len).ceil() as i64;
    let curvature_bonus = (curvature.max(0.0) * params.curvature_subsegment_boost).round() as i64;
    let lo = params.min_subsegments.max(1) as i64;
    let hi = params
        .edge_subsegments_max
        .max(params.min_subsegments)
        .max(1) as i64;
    (length_samples + curvature_bonus).clamp(lo, hi) as usize
}

#[derive(Clone, Copy, Debug)]
struct BrainBounds {
    center: [f32; 3],
    axes: [f32; 3],
}

impl BrainBounds {
    fn from_positions(positions: &[[f32; 3]], inside_margin: f32) -> Self {
        if positions.is_empty() {
            return Self {
                center: [0.0; 3],
                axes: [1.0; 3],
            };
        }

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }

        let center = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];
        let mut axes = [1.0; 3];
        for axis in 0..3 {
            let half = ((max[axis] - min[axis]) * 0.5).max(1e-3);
            let margin = inside_margin.max(0.0).min(half * 0.04);
            axes[axis] = (half - margin).max(half * 0.92).max(1e-3);
        }
        Self { center, axes }
    }

    fn clamp_point(&self, p: [f32; 3]) -> [f32; 3] {
        let v = sub(p, self.center);
        let q = v[0] * v[0] / (self.axes[0] * self.axes[0])
            + v[1] * v[1] / (self.axes[1] * self.axes[1])
            + v[2] * v[2] / (self.axes[2] * self.axes[2]);
        if q <= 1.0 {
            return p;
        }
        add(self.center, scale(v, 1.0 / q.sqrt().max(1e-6)))
    }

    #[cfg(test)]
    fn contains(&self, p: [f32; 3]) -> bool {
        let v = sub(p, self.center);
        let q = v[0] * v[0] / (self.axes[0] * self.axes[0])
            + v[1] * v[1] / (self.axes[1] * self.axes[1])
            + v[2] * v[2] / (self.axes[2] * self.axes[2]);
        q <= 1.0 + 2e-5
    }
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
) -> (f32, bool, f32, usize) {
    let mut prev = p0;
    let mut prev_r = r0;
    let mut prev_path = path_len_start;
    let mut emitted_all = true;
    let mut total_len = 0.0f32;
    let mut local_dropped = 0usize;
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
            local_dropped += 1;
            emitted_all = false;
        }
        prev_path += next_len;
        total_len += next_len;
        prev = pt;
        prev_r = rr;
    }
    (prev_path, emitted_all, total_len, local_dropped)
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

struct IncomingBuild {
    raw: Vec<IncomingSynapse>,
    ranges: Vec<IncomingRange>,
    groups: Vec<IncomingSocketGroup>,
    group_ranges: Vec<IncomingRange>,
    outgoing_groups: Vec<Vec<IncomingSocketGroup>>,
    duplicate_targets: usize,
    self_targets: usize,
}

fn build_ranges_by_target<T, F>(items: &[T], n: usize, target_id: F) -> Vec<IncomingRange>
where
    F: Fn(&T) -> u32,
{
    let mut ranges = vec![IncomingRange::default(); n];
    let mut idx = 0usize;
    while idx < items.len() {
        let target = target_id(&items[idx]) as usize;
        let start = idx;
        while idx < items.len() && target_id(&items[idx]) as usize == target {
            idx += 1;
        }
        if target < n {
            ranges[target] = IncomingRange {
                start,
                len: idx - start,
            };
        }
    }
    ranges
}

fn mean_usize(values: &[usize]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<usize>() as f32 / values.len() as f32
    }
}

fn percentile_p99(values: &[usize]) -> usize {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f32 * 0.99).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

fn build_incoming_view(
    positions: &[[f32; 3]],
    grid: &SpatialGrid,
    k: usize,
    seed_lo: u32,
    params: &MorphologyParams,
    source_types: &[u8],
    reach: connectivity::ReachParams,
    cell_of_neuron: &[u32],
) -> IncomingBuild {
    let n = positions.len();
    let mut raw = Vec::<IncomingSynapse>::with_capacity(n.saturating_mul(k));
    let mut duplicate_targets = 0usize;
    let mut self_targets = 0usize;

    for source in 0..n {
        let source_id = source as u32;
        let source_pos = positions[source];
        let source_type = source_types[source];
        let src_cell = grid.unpack(cell_of_neuron[source]);
        // Per-source duplicate-target stamp: a small Vec<u32> with linear scan
        // replaces HashSet::new() per neuron. At K≤32 (typical 16), the scan is
        // O(K²) total per source — cheaper than the hash-table allocation overhead
        // that was dominating at high N. Determinism is identical: we still push
        // every non-self synapse to `raw` regardless.
        let mut seen_targets: Vec<u32> = Vec::with_capacity(k);

        for synapse_index in 0..k as u32 {
            let target_id = connectivity::target_with_cell(
                source_id,
                synapse_index,
                grid,
                k,
                seed_lo,
                source_type,
                src_cell,
                reach,
            );
            if target_id == source_id {
                self_targets += 1;
                continue;
            }
            if seen_targets.contains(&target_id) {
                duplicate_targets += 1;
            } else {
                seen_targets.push(target_id);
            }

            let target_pos = positions[target_id as usize];
            let full = sub(target_pos, source_pos);
            let plan = TargetPlan {
                target_id,
                source_pos,
                target_pos,
                direction: norm(full),
                distance: len(full).max(1e-6),
                socket_idx: 0,
                socket_pos: target_pos,
                socket_distance: 0.0,
            };
            let (socket_pos, socket_idx, _) = target_socket(seed_lo, source_id, &plan, params);
            raw.push(IncomingSynapse {
                source_id,
                synapse_index,
                target_id,
                socket_idx: socket_idx as u32,
                socket_pos,
                weight: connectivity::weight(source_id, synapse_index, source_type),
            });
        }
    }

    raw.sort_unstable_by(|a, b| {
        (a.target_id, a.source_id, a.socket_idx, a.synapse_index).cmp(&(
            b.target_id,
            b.source_id,
            b.socket_idx,
            b.synapse_index,
        ))
    });
    let ranges = build_ranges_by_target(&raw, n, |record| record.target_id);

    let mut groups = Vec::<IncomingSocketGroup>::new();
    let mut idx = 0usize;
    while idx < raw.len() {
        let first = raw[idx];
        let start = idx;
        let mut weight_sum = 0i64;
        while idx < raw.len()
            && raw[idx].target_id == first.target_id
            && raw[idx].source_id == first.source_id
            && raw[idx].socket_idx == first.socket_idx
        {
            weight_sum += raw[idx].weight.unsigned_abs().max(1) as i64;
            idx += 1;
        }
        groups.push(IncomingSocketGroup {
            source_id: first.source_id,
            target_id: first.target_id,
            socket_idx: first.socket_idx,
            socket_pos: first.socket_pos,
            weight: weight_sum.max(1),
            raw_count: (idx - start) as u32,
        });
    }

    let group_ranges = build_ranges_by_target(&groups, n, |group| group.target_id);
    let mut outgoing_groups = vec![Vec::<IncomingSocketGroup>::new(); n];
    for group in &groups {
        outgoing_groups[group.source_id as usize].push(*group);
    }
    for source_groups in &mut outgoing_groups {
        source_groups.sort_unstable_by_key(|group| (group.target_id, group.socket_idx));
    }

    IncomingBuild {
        raw,
        ranges,
        groups,
        group_ranges,
        outgoing_groups,
        duplicate_targets,
        self_targets,
    }
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

#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    len(sub(a, b))
}

/// Host-local axon-tree node (morphology-branching-tree step 2). NEVER uploaded;
/// flattened to `MorphSegment`s after build. Index-keyed `Vec` (not HashMap) so
/// iteration order is deterministic — the locked ordered-structures rule.
#[derive(Clone, Debug)]
struct ArborNode {
    pos: [f32; 3],
    parent: Option<usize>,
    children: Vec<usize>,
    /// Real target neuron id for a leaf (socket) node; `None` for internal/root.
    target_id: Option<u32>,
    /// Leaf synaptic weight (clamped positive); 0 for internal/root.
    weight: i64,
    /// Bottom-up subtree weight sum (filled in the width pass).
    subtree_weight: i64,
    /// Area-preserving radius (filled in the width pass).
    radius: f32,
    /// Depth from the root (root = 0).
    depth: u32,
}

#[derive(Clone, Copy, Debug)]
struct AxonLeaf {
    target_id: u32,
    socket_pos: [f32; 3],
    weight: i64,
}

#[derive(Clone, Copy, Debug)]
struct DendriteAssignment {
    group_idx: usize,
    dir: [f32; 3],
    socket_dist: f32,
    cluster_idx: usize,
}

#[allow(clippy::too_many_arguments)]
fn emit_incoming_dendrites(
    segments: &mut Vec<MorphSegment>,
    cap: usize,
    dropped: &mut usize,
    incoming_dropped: &mut usize,
    seed_lo: u32,
    target_id: u32,
    soma: [f32; 3],
    groups: &[IncomingSocketGroup],
    params: &MorphologyParams,
    decor_group_budget: usize,
) {
    if groups.is_empty() {
        return;
    }

    let mut axis = [0.0f32; 3];
    let mut assignments = Vec::<DendriteAssignment>::with_capacity(groups.len());
    for (idx, group) in groups.iter().enumerate() {
        let offset = sub(group.socket_pos, soma);
        let socket_dist = len(offset).max(params.base_radius * 1.02);
        let dir = if socket_dist > 1e-6 {
            norm(offset)
        } else {
            dir_from_hashes(
                mix_key(seed_lo, target_id, group.socket_idx, salt::DENDRITE_DIR),
                mix_key(seed_lo, target_id, group.source_id, salt::DENDRITE_CURL),
            )
        };
        axis = add(axis, scale(dir, group.weight.max(1) as f32));
        assignments.push(DendriteAssignment {
            group_idx: idx,
            dir,
            socket_dist,
            cluster_idx: 0,
        });
    }

    let axis = if len(axis) > 1e-6 {
        norm(axis)
    } else {
        dir_from_hashes(
            mix_key(seed_lo, target_id, groups.len() as u32, salt::DENDRITE_DIR),
            mix_key(seed_lo, target_id, groups.len() as u32, salt::DENDRITE_CURL),
        )
    };
    let mut tangent_u = cross(
        axis,
        dir_from_hashes(
            mix_key(seed_lo, target_id, 0, salt::DENDRITE_CURL ^ 0x8f13_51a5),
            mix_key(seed_lo, target_id, 1, salt::DENDRITE_DIR ^ 0x2c1b_f00d),
        ),
    );
    if len(tangent_u) < 1e-5 {
        tangent_u = perp(axis, mix_key(seed_lo, target_id, 2, salt::DENDRITE_CURL));
    }
    tangent_u = norm(tangent_u);
    let tangent_v = norm(cross(axis, tangent_u));

    assignments.sort_unstable_by(|a, b| {
        let aa = dot(a.dir, tangent_v).atan2(dot(a.dir, tangent_u));
        let bb = dot(b.dir, tangent_v).atan2(dot(b.dir, tangent_u));
        aa.total_cmp(&bb).then_with(|| {
            let ga = groups[a.group_idx];
            let gb = groups[b.group_idx];
            (ga.socket_idx, ga.source_id).cmp(&(gb.socket_idx, gb.source_id))
        })
    });

    let root_count = params
        .dendrite_primary_root_count
        .clamp(1, 6)
        .min(assignments.len().max(1));
    let assignment_len = assignments.len();
    for (rank, assignment) in assignments.iter_mut().enumerate() {
        assignment.cluster_idx = rank * root_count / assignment_len;
    }

    let r_root = (params.base_radius * params.dendrite_mid_radius_fraction).max(1e-4);
    let r_child = lerp(
        r_root,
        params.base_radius * params.dendrite_tip_radius_fraction,
        0.46,
    )
    .max(1e-4);
    let r_tip = (params.base_radius * params.dendrite_tip_radius_fraction).max(1e-4);
    let root_surface = params.base_radius * 1.04;
    let curve_tightness = params.dendrite_curve_tightness.clamp(0.0, 1.25);
    let group_spacing = params.dendrite_group_spacing.clamp(0.0, 1.5);

    // Bounded decorative-branch budget for this neuron's whole dendrite arbor. The
    // bushy local grammar (secondary branchlets + terminal twigs) is sprouted only
    // on the first `dendrite_decor_group_max` groups in cluster order, so the
    // extra-segment growth stays bounded regardless of in-degree. These decorations
    // carry NO synapse — owner is the neuron's own id (`target_id == target_id`).
    let decor_group_budget = decor_group_budget.min(DENDRITE_DECOR_GROUP_MAX);
    let branchlet_count = params.dendrite_branchlet_count.min(DENDRITE_BRANCHLET_MAX);
    let twig_count = params.dendrite_twig_count.min(DENDRITE_TWIG_MAX);
    let r_branchlet = (params.base_radius * params.dendrite_branchlet_radius_fraction).max(1e-4);
    let r_twig = (params.base_radius * params.dendrite_twig_radius_fraction).max(1e-4);
    let twig_curl = params.dendrite_twig_curl.max(0.0);
    let mut decorated_groups = 0usize;

    for cluster_idx in 0..root_count {
        let cluster_start = assignments
            .iter()
            .position(|a| a.cluster_idx == cluster_idx)
            .expect("root has at least one assignment");
        let cluster_end = assignments
            .iter()
            .rposition(|a| a.cluster_idx == cluster_idx)
            .map(|idx| idx + 1)
            .expect("root has at least one assignment");
        let cluster = &assignments[cluster_start..cluster_end];
        let mut weighted_dir = [0.0f32; 3];
        let mut weighted_socket_dist = 0.0f32;
        let mut total_weight = 0i64;
        for assignment in cluster {
            let group = groups[assignment.group_idx];
            total_weight += group.weight.max(1);
            weighted_socket_dist += assignment.socket_dist * group.weight.max(1) as f32;
            weighted_dir = add(
                weighted_dir,
                scale(assignment.dir, group.weight.max(1) as f32),
            );
        }
        let total_weight_f = total_weight.max(1) as f32;
        let avg_socket_dist = weighted_socket_dist / total_weight_f;
        let root_dir = if len(weighted_dir) > 1e-6 {
            norm(weighted_dir)
        } else {
            dir_from_hashes(
                mix_key(seed_lo, target_id, cluster_idx as u32, salt::DENDRITE_DIR),
                mix_key(seed_lo, target_id, cluster_idx as u32, salt::DENDRITE_CURL),
            )
        };
        let collar = add(soma, scale(root_dir, root_surface));
        let fork_distance = (params.base_radius * params.dendrite_fork_distance)
            .max(root_surface + params.base_radius * 0.18)
            .min(avg_socket_dist.max(root_surface + params.base_radius * 0.22) * 1.05);
        let first_fork = add(soma, scale(root_dir, fork_distance));
        let root_len = dist(collar, first_fork).max(1e-5);
        let root_bend = bend_vector(
            root_dir,
            mix_key(
                seed_lo,
                target_id,
                cluster_idx as u32,
                salt::DENDRITE_CURL ^ 0x4c8b_6d35,
            ),
            root_len * curve_tightness,
        );
        let root_p1 = add(
            add(collar, scale(root_dir, root_len * 0.34)),
            scale(root_bend, 0.34),
        );
        let root_p2 = add(
            add(first_fork, scale(root_dir, -root_len * 0.24)),
            scale(root_bend, -0.18),
        );
        let (root_path_end, _, _, root_dropped) = emit_bezier_path(
            segments,
            cap,
            dropped,
            target_id,
            target_id,
            0,
            collar,
            root_p1,
            root_p2,
            first_fork,
            r_root,
            r_child,
            DENDRITE_ROOT_SAMPLES,
            0.0,
            params.taper_curve * 0.72,
        );
        *incoming_dropped += root_dropped;

        let fan = cluster.len().max(1) as f32;
        for (local_idx, assignment) in cluster.iter().enumerate() {
            let group = groups[assignment.group_idx];
            let socket = group.socket_pos;
            let socket_dir = assignment.dir;
            let lateral_seed = mix_key(
                seed_lo,
                group.source_id,
                target_id ^ group.socket_idx,
                salt::DENDRITE_DIR ^ 0x6a09_e667,
            );
            let mut lateral = cross(root_dir, socket_dir);
            if len(lateral) < 1e-5 {
                lateral = perp(root_dir, lateral_seed);
            }
            let lateral = norm(lateral);
            let centered = local_idx as f32 - (fan - 1.0) * 0.5;
            let normalized_offset = if fan <= 1.0 {
                unit(lateral_seed) - 0.5
            } else {
                centered / ((fan - 1.0) * 0.5).max(1.0)
            };
            let child_distance = (fork_distance * 1.34).max(params.base_radius * 1.72).min(
                (assignment.socket_dist * 0.94).max(fork_distance + params.base_radius * 0.08),
            );
            let child_axis = norm(add(scale(root_dir, 0.58), scale(socket_dir, 0.42)));
            let group_fork = add(
                add(soma, scale(child_axis, child_distance)),
                scale(
                    lateral,
                    normalized_offset * group_spacing * params.base_radius,
                ),
            );

            let fork_dir = norm(sub(group_fork, first_fork));
            let fork_len = dist(first_fork, group_fork).max(1e-5);
            let fork_bend = bend_vector(
                fork_dir,
                mix_key(
                    seed_lo,
                    target_id,
                    group.source_id ^ group.socket_idx,
                    salt::DENDRITE_CURL ^ 0x510e_527f,
                ),
                fork_len * curve_tightness * 0.72,
            );
            let fork_p1 = add(
                add(first_fork, scale(fork_dir, fork_len * 0.38)),
                scale(fork_bend, 0.30),
            );
            let fork_p2 = add(
                add(group_fork, scale(fork_dir, -fork_len * 0.24)),
                scale(fork_bend, -0.14),
            );
            let weight_scale =
                ((group.weight.max(1) as f32 / total_weight_f).sqrt() * 1.65).clamp(0.75, 1.25);
            let (fork_path_end, _, _, fork_dropped) = emit_bezier_path(
                segments,
                cap,
                dropped,
                target_id,
                target_id,
                0,
                first_fork,
                fork_p1,
                fork_p2,
                group_fork,
                r_child,
                (r_child * 0.88 * weight_scale).max(r_tip),
                DENDRITE_FORK_SAMPLES,
                root_path_end,
                params.taper_curve * 0.82,
            );
            *incoming_dropped += fork_dropped;

            let leaf_dir = norm(sub(group_fork, socket));
            let leaf_len = dist(socket, group_fork).max(1e-5);
            let leaf_bend = bend_vector(
                leaf_dir,
                mix_key(
                    seed_lo,
                    group.source_id,
                    target_id ^ group.socket_idx,
                    salt::DENDRITE_CURL ^ 0xa13f_91d1,
                ),
                leaf_len * curve_tightness * 0.62,
            );
            let leaf_p1 = add(
                add(socket, scale(leaf_dir, leaf_len * 0.38)),
                scale(leaf_bend, 0.26),
            );
            let leaf_p2 = add(
                add(group_fork, scale(leaf_dir, -leaf_len * 0.22)),
                scale(leaf_bend, -0.12),
            );
            let (_, _, _, leaf_dropped) = emit_bezier_path(
                segments,
                cap,
                dropped,
                target_id,
                group.source_id,
                0,
                socket,
                leaf_p1,
                leaf_p2,
                group_fork,
                (r_tip * weight_scale).max(1e-4),
                (r_child * 0.78 * weight_scale).max(r_tip),
                DENDRITE_TWIG_SAMPLES,
                0.0,
                params.taper_curve * 0.72,
            );
            *incoming_dropped += leaf_dropped;

            // ── Bushy local grammar (Stream D) ──────────────────────────────────
            // Sprout decorative secondary branchlets and terminal twigs off the
            // group tip (`group_fork`, a real branch vertex where the fork edge
            // ends at cumulative path `fork_path_end`), so the dendrite reads as a
            // LOCAL, BUSHY arbor instead of a generic radial star-burst. These are
            // NOT synapses: owner is the neuron's own id (`target_id`), so they
            // light WITH the neuron and never invent a fake presynaptic source. The
            // real presynaptic leaf above (kind 0, `target_id == group.source_id`,
            // path_len 0, a == socket) is untouched. Rooting every decoration at the
            // existing `group_fork` vertex with `path_len = fork_path_end` keeps the
            // tree path-length continuity invariant intact. Bounded to the first
            // `decor_group_budget` groups in deterministic cluster order.
            if decorated_groups < decor_group_budget && (branchlet_count > 0 || twig_count > 0) {
                decorated_groups += 1;
                let decor_basis = norm(sub(group_fork, first_fork));
                let decor_side = {
                    let mut s = cross(decor_basis, lateral);
                    if len(s) < 1e-5 {
                        s = perp(decor_basis, lateral_seed ^ 0x1234_5678);
                    }
                    norm(s)
                };
                let decor_side2 = norm(cross(decor_basis, decor_side));
                // Secondary branchlets: longer outward processes peeling off the tip
                // with extra curl. Trunk → twig taper: branchlet base picks up the
                // mid radius, tip thins to the twig floor.
                for b in 0..branchlet_count {
                    let bseed = mix_key(
                        seed_lo,
                        group.source_id,
                        target_id ^ group.socket_idx ^ ((b as u32 + 1) << 8),
                        salt::DENDRITE_BRANCHLET,
                    );
                    let sign = if unit(bseed ^ 0x9e37_79b9) < 0.5 {
                        -1.0
                    } else {
                        1.0
                    };
                    let bdir = norm(add(
                        scale(decor_basis, 0.6),
                        add(
                            scale(decor_side, sign * (0.5 + 0.35 * unit(bseed ^ 0x85eb_ca6b))),
                            scale(decor_side2, (unit(bseed ^ 0xc2b2_ae35) - 0.5) * 0.7),
                        ),
                    ));
                    let blen = (fork_len * params.dendrite_branchlet_length_fraction)
                        .clamp(params.base_radius * 0.6, params.base_radius * 4.0);
                    let btip = add(group_fork, scale(bdir, blen));
                    let bbend = bend_vector(
                        bdir,
                        bseed ^ 0x27d4_eb2f,
                        blen * curve_tightness * twig_curl,
                    );
                    let bp1 = add(
                        add(group_fork, scale(bdir, blen * 0.36)),
                        scale(bbend, 0.42),
                    );
                    let bp2 = add(add(btip, scale(bdir, -blen * 0.22)), scale(bbend, -0.20));
                    let bcurv = (len(bbend) / blen.max(1e-5)).min(2.0);
                    let bsamples = adaptive_subsegments(blen, bcurv, false, params)
                        .min(DENDRITE_DECOR_SAMPLES_MAX);
                    let (_, _, _, bdrop) = emit_bezier_path(
                        segments,
                        cap,
                        dropped,
                        target_id,
                        target_id, // decorative — owner is the neuron itself
                        0,
                        group_fork,
                        bp1,
                        bp2,
                        btip,
                        (r_branchlet * weight_scale).max(r_twig),
                        r_twig,
                        bsamples,
                        fork_path_end,
                        params.taper_curve,
                    );
                    *incoming_dropped += bdrop;
                }
                // Terminal twigs: splay a small brush of very thin processes off the
                // group tip, each with its own curl direction (curvature VARIATION).
                for w in 0..twig_count {
                    let wseed = mix_key(
                        seed_lo,
                        group.source_id,
                        target_id ^ group.socket_idx ^ ((w as u32 + 1) << 16),
                        salt::DENDRITE_TWIG,
                    );
                    let a0 = unit(wseed ^ 0x165e_3a1f) * 2.0 - 1.0;
                    let a1 = unit(wseed ^ 0x9e37_79b1) * 2.0 - 1.0;
                    let wdir = norm(add(
                        scale(decor_basis, 0.62),
                        add(scale(decor_side, a0 * 0.7), scale(decor_side2, a1 * 0.7)),
                    ));
                    let wlen = (params.base_radius * params.dendrite_twig_length_fraction)
                        .clamp(params.base_radius * 0.5, params.base_radius * 3.0)
                        * (0.7 + 0.6 * unit(wseed ^ 0x452f_9e1d));
                    let wtip = add(group_fork, scale(wdir, wlen));
                    let wbend = bend_vector(
                        wdir,
                        mix_key(
                            seed_lo,
                            group.source_id,
                            target_id ^ (w as u32 + 1),
                            salt::DENDRITE_TWIG_CURL,
                        ),
                        wlen * curve_tightness * twig_curl,
                    );
                    let wp1 = add(add(group_fork, scale(wdir, wlen * 0.34)), scale(wbend, 0.5));
                    let wp2 = add(add(wtip, scale(wdir, -wlen * 0.2)), scale(wbend, -0.24));
                    let wcurv = (len(wbend) / wlen.max(1e-5)).min(2.0);
                    let wsamples = adaptive_subsegments(wlen, wcurv, false, params)
                        .min(DENDRITE_DECOR_SAMPLES_MAX);
                    let (_, _, _, wdrop) = emit_bezier_path(
                        segments,
                        cap,
                        dropped,
                        target_id,
                        target_id, // decorative — owner is the neuron itself
                        0,
                        group_fork,
                        wp1,
                        wp2,
                        wtip,
                        (r_twig * 1.1).max(1e-4),
                        (r_twig * 0.55).max(1e-4),
                        wsamples,
                        // Roots at the group_fork vertex (path = fork_path_end) so the
                        // tree path-length continuity invariant holds; never 0 (that
                        // is reserved for soma collars / presynaptic leaves).
                        fork_path_end,
                        params.taper_curve,
                    );
                    *incoming_dropped += wdrop;
                }
            }
        }
    }
}

fn build_process_root(
    seed_lo: u32,
    source_id: u32,
    soma: [f32; 3],
    leaves: &[AxonLeaf],
    params: &MorphologyParams,
    root_radius: f32,
) -> ProcessRoot {
    let mut axis = [0.0f32; 3];
    let mut avg_distance = 0.0f32;
    let mut root_weight = 0u64;
    for leaf in leaves {
        axis = add(axis, norm(sub(leaf.socket_pos, soma)));
        avg_distance += dist(leaf.socket_pos, soma);
        root_weight = root_weight.saturating_add(leaf.weight.max(0) as u64);
    }

    let unique_count = leaves.len();
    let direction = if len(axis) < 1e-6 {
        dir_from_hashes(
            mix_key(seed_lo, source_id, unique_count as u32, salt::AXON_BOW),
            mix_key(seed_lo, source_id, unique_count as u32, salt::DENDRITE_DIR),
        )
    } else {
        norm(axis)
    };
    let trunk_len = if unique_count == 0 {
        params.base_radius.max(0.001)
    } else {
        avg_distance /= unique_count as f32;
        (avg_distance * params.trunk_length_fraction.max(0.05)).max(0.02)
    };

    ProcessRoot {
        neuron_id: source_id,
        soma_center: soma,
        direction,
        soma_root: add(soma, scale(direction, params.base_radius.max(0.0))),
        first_fork: add(soma, scale(direction, trunk_len)),
        root_radius,
        root_weight: root_weight.min(u32::MAX as u64) as u32,
        unique_target_count: unique_count as u32,
    }
}

/// Deterministic intermediate waypoints for a VISUALLY LONG axon leaf edge.
///
/// Heuristic: connectivity exposes no per-synapse long-range flag (the heavy-tail
/// coin is baked into the resulting target id), so "long" is decided purely by
/// world distance — the chord `start → socket` exceeds
/// `long_range_chord_cells × cell_size`. Returns `[]` for local edges, otherwise
/// 1..=`long_range_max_waypoints` intermediate points (terminal socket NOT
/// included) so the caller emits `waypoints.len() + 1` bounded hops instead of one
/// giant span.
///
/// Placement is salt-seeded and bias-curved: each waypoint sits on the straight
/// chord, then (a) bows away from `brain_center` so the fiber curves around the
/// volume rather than crossing straight through it, and (b) takes a deterministic
/// lateral detour from the morphology salts. Count grows with chord length
/// (`round(chord / long_range_waypoint_span)`), clamped to the bound. Depends only
/// on positions + salts — no runtime/camera state.
fn long_range_waypoints(
    seed_lo: u32,
    source_id: u32,
    target_id: u32,
    start: [f32; 3],
    socket: [f32; 3],
    brain_center: [f32; 3],
    cell_size: f32,
    params: &MorphologyParams,
    brain_bounds: &BrainBounds,
) -> Vec<[f32; 3]> {
    let chord = dist(start, socket);
    let threshold = (params.long_range_chord_cells.max(0.0) * cell_size).max(1e-4);
    if chord < threshold || params.long_range_max_waypoints == 0 {
        return Vec::new();
    }
    let span = params.long_range_waypoint_span.max(1e-3);
    // Hops ≈ chord / span; waypoints = hops - 1, clamped to [1, max].
    let hops = (chord / span).round().max(2.0) as usize;
    let count = (hops.saturating_sub(1)).clamp(1, params.long_range_max_waypoints);

    let axis = norm(sub(socket, start));
    // Lateral basis seeded from the salts (deterministic, decorrelated per edge).
    let lateral = perp(
        axis,
        mix_key(seed_lo, source_id, target_id, salt::TREE_BEND ^ 0x51ed_270f),
    );
    let lateral2 = norm(cross(axis, lateral));
    let offset_mag = params.long_range_lateral_offset.max(0.0);

    let mut out = Vec::with_capacity(count);
    for w in 0..count {
        let t = (w + 1) as f32 / (count + 1) as f32;
        let on_chord = add(start, scale(sub(socket, start), t));
        // Bow magnitude peaks mid-edge (sin-like via 4·t·(1-t)), tapering to 0 at
        // the endpoints so the route stays anchored at the fork and socket.
        let bow = 4.0 * t * (1.0 - t);
        // Curve around the brain volume: push away from the centroid.
        let outward = norm(sub(on_chord, brain_center));
        let around = scale(outward, offset_mag * bow);
        // Deterministic lateral detour (salt-seeded sign/magnitude per waypoint).
        let s0 = unit(mix_key(
            seed_lo,
            source_id,
            target_id ^ (w as u32 + 1),
            salt::TREE_BEND,
        )) * 2.0
            - 1.0;
        let s1 = unit(mix_key(
            seed_lo,
            source_id,
            target_id ^ (w as u32 + 0x9e37),
            salt::TREE_SPLIT,
        )) * 2.0
            - 1.0;
        let detour = add(
            scale(lateral, s0 * offset_mag * bow),
            scale(lateral2, s1 * offset_mag * bow * 0.5),
        );
        out.push(brain_bounds.clamp_point(add(add(on_chord, around), detour)));
    }
    out
}

/// Prim attach score for placing a new leaf at `cand_pos` under `parent_idx`:
/// `distance + curvature·curv + density·dens + degree·deg`. Lower is better.
/// Curvature = `1 - dot` of the parent's incoming direction vs the new edge
/// (trig-free, deterministic). Density = inverse proximity to existing nodes.
/// Degree = `children.len().saturating_sub(1)` (1st child free, soft fork).
fn attach_score(
    tree: &[ArborNode],
    parent_idx: usize,
    cand_pos: [f32; 3],
    leaf_pos: [f32; 3],
    curv_w: f32,
    dens_w: f32,
    deg_w: f32,
) -> f32 {
    let new_dir = norm(sub(leaf_pos, cand_pos));
    let base = dist(cand_pos, leaf_pos);

    // Curvature: angle between the parent's incoming edge and the new edge.
    let curvature = if let Some(gp) = tree[parent_idx].parent {
        let incoming = norm(sub(tree[parent_idx].pos, tree[gp].pos));
        (1.0 - dot(incoming, new_dir)).max(0.0)
    } else {
        0.0
    };

    // Density: penalize crowding near already-placed nodes (linear scan over the
    // small per-arbor node Vec — ordered, deterministic).
    let mut density = 0.0f32;
    for (ni, node) in tree.iter().enumerate() {
        if ni == parent_idx {
            continue;
        }
        let d = dist(node.pos, cand_pos);
        density += 1.0 / (1.0 + d * 40.0);
    }

    let degree = tree[parent_idx].children.len().saturating_sub(1) as f32;

    base + curv_w * curvature * base + dens_w * density * base + deg_w * degree * base * 0.25
}

/// Record a direct-node attach candidate if it beats the current best, breaking
/// ties deterministically by `(leaf target_id, node index)` ascending.
#[allow(clippy::too_many_arguments)]
fn consider(
    best: &mut Option<(usize, usize, [f32; 3], u32, f32)>,
    ui: usize,
    node_idx: usize,
    pos: [f32; 3],
    leaf_tid: u32,
    score: f32,
    eps: f32,
) {
    match best {
        None => *best = Some((ui, node_idx, pos, leaf_tid, score)),
        Some((_, b_node, _, b_tid, b_score)) => {
            if score < *b_score - eps
                || ((score - *b_score).abs() <= eps && (leaf_tid, node_idx) < (*b_tid, *b_node))
            {
                *best = Some((ui, node_idx, pos, leaf_tid, score));
            }
        }
    }
}

/// Like `consider` but for a mid-edge split candidate (the attach node is the
/// child whose incoming edge is split). Same deterministic tie-break.
#[allow(clippy::too_many_arguments)]
fn consider_split(
    best: &mut Option<(usize, usize, [f32; 3], u32, f32)>,
    ui: usize,
    node_idx: usize,
    split_pos: [f32; 3],
    leaf_tid: u32,
    score: f32,
    eps: f32,
) {
    consider(best, ui, node_idx, split_pos, leaf_tid, score, eps);
}

/// Re-derive depths for the subtree rooted at `root` from its parent's depth.
/// Iterative, ascending — deterministic.
fn refresh_depths(tree: &mut [ArborNode], root: usize) {
    let mut stack = vec![root];
    while let Some(idx) = stack.pop() {
        let base = tree[idx].parent.map(|p| tree[p].depth + 1).unwrap_or(0);
        tree[idx].depth = base;
        // Clone children indices to avoid borrow conflict.
        let kids = tree[idx].children.clone();
        for c in kids {
            stack.push(c);
        }
    }
}

/// Local relaxation: pull each INTERNAL node in the just-touched ancestor window
/// toward the mean of (parent + children) by `lerp`, then repel from nearby
/// nodes by `repel`. Node 0 (axon root), node 1 (descriptor first fork), and all
/// leaf nodes are held fixed.
/// Processes nodes in ascending index order; neighbour set is the ordered node
/// Vec — no spatial-hash iteration, no float-nondeterministic reduction.
fn relax_window(
    tree: &mut [ArborNode],
    start: usize,
    window: usize,
    lerp_amt: f32,
    repel_amt: f32,
) {
    // Collect the ancestor window (start and up to `window` ancestors), ascending.
    let mut chain = Vec::with_capacity(window + 1);
    let mut cur = Some(start);
    for _ in 0..=window {
        match cur {
            Some(idx) => {
                chain.push(idx);
                cur = tree[idx].parent;
            }
            None => break,
        }
    }
    chain.sort_unstable();

    for &idx in &chain {
        // Held-fixed: root (no parent), descriptor trunk/fork, and leaves.
        if tree[idx].parent.is_none() || tree[idx].depth <= 1 || tree[idx].target_id.is_some() {
            continue;
        }
        let parent = tree[idx].parent.expect("internal node has a parent");
        // Mean of parent + children.
        let mut sum = tree[parent].pos;
        let mut count = 1u32;
        let kids = tree[idx].children.clone();
        for c in &kids {
            sum = add(sum, tree[*c].pos);
            count += 1;
        }
        let mean = scale(sum, 1.0 / count as f32);
        let cur_pos = tree[idx].pos;
        let mut new_pos = add(cur_pos, scale(sub(mean, cur_pos), lerp_amt.clamp(0.0, 1.0)));

        // Repel from nearby nodes (ascending order, fixed reduction).
        let mut push = [0.0f32; 3];
        for (ni, node) in tree.iter().enumerate() {
            if ni == idx {
                continue;
            }
            let off = sub(new_pos, node.pos);
            let d = len(off).max(1e-4);
            if d < 0.05 {
                push = add(push, scale(norm(off), (0.05 - d) / 0.05));
            }
        }
        new_pos = add(new_pos, scale(push, repel_amt.clamp(0.0, 1.0) * 0.02));
        tree[idx].pos = new_pos;
    }
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
    reach: connectivity::ReachParams,
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
    let mut process_roots: Vec<ProcessRoot> = Vec::with_capacity(n);
    let mut dropped = 0usize;
    let mut incoming_dropped = 0usize;
    let mut unique_targets_expected = 0usize;
    let mut unique_targets_emitted = 0usize;
    let mut all_k_coverage = true;
    let mut source_type_excitatory = 0usize;
    let mut source_type_inhibitory = 0usize;
    let mut cluster_count_histogram = [0u32; 6];
    let mut terminal_socket_distance_bands = [0u32; 4];
    let mut socket_reuse_bands = [0u32; 4];
    let mut radius_bands = [0u32; 4];
    let mut tree_depth_max = 0u32;
    let mut tree_depth_sum = 0u64;
    let mut tree_depth_count = 0u64;

    // Scratch buffers hoisted out of the per-neuron loop to avoid repeated heap
    // allocation/deallocation at high N.  Each is cleared at the start of the
    // neuron that uses it; capacity is retained across neurons.
    let mut leaves: Vec<AxonLeaf> = Vec::with_capacity(k);

    // Precompute each neuron's grid cell once (O(N)) so the axon-arbor loop below
    // can use the hot-path `target_with_cell` entry. The uncached
    // `connectivity::target` re-derives the cell with an O(N) `cell_of_index`
    // scan per call, which made morphology generation O(N²·K) and dominated
    // network-rebuild time at high N. The CPU/GPU paths already cache this map.
    let cell_of_neuron = grid.cell_of_neuron_map();

    // World-space anchors for long-range waypoint routing (deterministic from the
    // grid bounds). `brain_center` is the bounding-box centroid; `cell_size` is the
    // characteristic length used to classify a leaf chord as visually long.
    let cell_size = grid.cell_size;
    let half_extent = grid.cell_size * grid.dim as f32 * 0.5;
    let brain_center = [
        grid.min[0] + half_extent,
        grid.min[1] + half_extent,
        grid.min[2] + half_extent,
    ];
    let brain_bounds = BrainBounds::from_positions(positions, params.base_radius);

    let setup_ms = setup_start.elapsed_ms();
    let incoming_start = MorphTimer::start();
    let incoming = build_incoming_view(
        positions,
        grid,
        k,
        seed_lo,
        params,
        source_types,
        reach,
        &cell_of_neuron,
    );
    let incoming_ms = incoming_start.elapsed_ms();
    let mut dendrite_ms = 0.0f32;
    let mut axon_ms = 0.0f32;

    // Bounded bushy-decoration allowance. High-N storage binding limits are
    // handled by GPU segment chunking, so the budget no longer changes with N.
    let decor_group_budget = effective_decor_group_max(n, params.dendrite_decor_group_max);

    for i in 0..n {
        let soma = positions[i];
        let id = i as u32;

        // ── Dendrites (kind 0): target-owned real incoming socket aggregation.
        let dendrite_start = MorphTimer::start();
        let src_type = source_types[i];
        if src_type & 0x01 == 0 {
            source_type_excitatory += 1;
        } else {
            source_type_inhibitory += 1;
        }
        let group_range = incoming.group_ranges[i];
        let incoming_groups =
            &incoming.groups[group_range.start..group_range.start + group_range.len];
        emit_incoming_dendrites(
            &mut segments,
            cap,
            &mut dropped,
            &mut incoming_dropped,
            seed_lo,
            id,
            soma,
            incoming_groups,
            params,
            decor_group_budget,
        );
        dendrite_ms += dendrite_start.elapsed_ms();

        // ── Axon arbor (kind 1): Prim-like shared tree → relax → width → spline.
        let axon_start = MorphTimer::start();
        let outgoing_groups = &incoming.outgoing_groups[i];
        let unique_count = outgoing_groups.len();
        unique_targets_expected += unique_count;

        let r_trunk = params.base_radius * params.axon_root_radius_fraction;
        let r_floor = (r_trunk * params.twig_radius_fraction).max(1e-4);

        // Leaf sockets (held fixed), tagged with synaptic weight. Each leaf is a
        // detached node until attached by the Prim loop below.
        // (leaves is pre-allocated before the neuron loop; cleared here per neuron.)
        leaves.clear();
        for group in outgoing_groups {
            let tgt_id = group.target_id;
            let target_pos = positions[tgt_id as usize];
            let socket_distance = dist(group.socket_pos, target_pos);
            terminal_socket_distance_bands[if socket_distance < params.socket_radius_lo * 0.5 {
                0
            } else if socket_distance < params.socket_radius_lo * 0.9 {
                1
            } else if socket_distance < params.socket_radius_hi * 1.1 {
                2
            } else {
                3
            }] += 1;
            socket_reuse_bands[(group.socket_idx as usize).min(3)] += 1;
            leaves.push(AxonLeaf {
                target_id: tgt_id,
                socket_pos: group.socket_pos,
                weight: group.weight,
            });
        }

        let process_root = build_process_root(seed_lo, id, soma, &leaves, params, r_trunk);
        process_roots.push(process_root);
        if unique_count == 0 {
            cluster_count_histogram[0] += 1;
            axon_ms += axon_start.elapsed_ms();
            continue;
        }

        // ── Build the per-arbor tree (host-local; never uploaded). ───────────
        let mut tree: Vec<ArborNode> = Vec::with_capacity(2 * unique_count + 1);
        // Node 0 = soma-surface axon root, held fixed.
        tree.push(ArborNode {
            pos: process_root.soma_root,
            parent: None,
            children: Vec::new(),
            target_id: None,
            weight: 0,
            subtree_weight: 0,
            radius: 0.0,
            depth: 0,
        });

        // Seed a shared trunk node from the descriptor so all axons, including
        // single-target arbors, use the same root → first-fork convention.
        tree.push(ArborNode {
            pos: process_root.first_fork,
            parent: Some(0),
            children: Vec::new(),
            target_id: None,
            weight: 0,
            subtree_weight: 0,
            radius: 0.0,
            depth: 1,
        });
        tree[0].children.push(1);

        if unique_count == 1 {
            tree.push(ArborNode {
                pos: leaves[0].socket_pos,
                parent: Some(1),
                children: Vec::new(),
                target_id: Some(leaves[0].target_id),
                weight: leaves[0].weight,
                subtree_weight: 0,
                radius: 0.0,
                depth: 2,
            });
            tree[1].children.push(2);
        } else {
            // Prim-like greedy attach: one edge per iteration, picking the
            // globally-best (leaf, attach-point) pair over existing nodes AND
            // along existing edges. Attach points can split an edge (→ shared
            // trunks/forks). Deterministic tie-break by (leaf target_id, node idx).
            let mut unattached: Vec<usize> = (0..leaves.len()).collect(); // ordered by target_id
            let curv_w = params.tree_score_curvature.max(0.0);
            let dens_w = params.tree_score_density.max(0.0);
            let deg_w = params.tree_score_degree.max(0.0);
            const EPS: f32 = 1e-5;

            while !unattached.is_empty() {
                let mut best: Option<(usize, usize, [f32; 3], u32, f32)> = None;
                // (unattached_idx, parent_node, split_pos, leaf_target_id, score)
                for (ui, &li) in unattached.iter().enumerate() {
                    let leaf_pos = leaves[li].socket_pos;
                    let leaf_tid = leaves[li].target_id;
                    for node_idx in 0..tree.len() {
                        // The descriptor root edge (soma_root → first_fork) is
                        // the primary axon trunk. Never attach leaves directly
                        // to the soma root; all real branches must start at or
                        // beyond the fixed first-fork point.
                        if node_idx == 0 {
                            continue;
                        }
                        // Candidate 1: attach directly to this node.
                        let cand_pos = tree[node_idx].pos;
                        let score = attach_score(
                            &tree, node_idx, cand_pos, leaf_pos, curv_w, dens_w, deg_w,
                        );
                        consider(&mut best, ui, node_idx, cand_pos, leaf_tid, score, EPS);

                        // Candidate 2: split the edge (parent→node) at the
                        // projection of the leaf, if this node has a parent.
                        if let Some(parent) = tree[node_idx].parent {
                            if parent == 0 {
                                continue;
                            }
                            let a = tree[parent].pos;
                            let b = tree[node_idx].pos;
                            let ab = sub(b, a);
                            let ab_len2 = dot(ab, ab).max(1e-12);
                            let t = (dot(sub(leaf_pos, a), ab) / ab_len2).clamp(0.0, 1.0);
                            // Deterministic split-point jitter (kept tiny so it
                            // stays on the edge); uses the new TREE_SPLIT salt.
                            let jitter = (unit(mix_key(
                                seed_lo,
                                id,
                                leaf_tid ^ (node_idx as u32),
                                salt::TREE_SPLIT,
                            )) - 0.5)
                                * 0.04;
                            let t = (t + jitter).clamp(0.05, 0.95);
                            let split_pos = add(a, scale(ab, t));
                            let score = attach_score(
                                &tree, parent, split_pos, leaf_pos, curv_w, dens_w, deg_w,
                            ) + 0.001; // mild bias toward existing nodes on ties
                                       // Encode "split of edge into node_idx" as parent index
                                       // with the split position; resolved at attach.
                            consider_split(
                                &mut best, ui, node_idx, split_pos, leaf_tid, score, EPS,
                            );
                        }
                    }
                }

                let (ui, attach_target, split_pos, _tid, _score) =
                    best.expect("at least one candidate per non-empty unattached set");
                let li = unattached[ui];

                // Resolve the attach point. If `split_pos == node.pos` we attach to
                // the node directly; otherwise we split the (parent→node) edge.
                let parent_for_leaf = if dist(split_pos, tree[attach_target].pos) < 1e-9 {
                    attach_target
                } else {
                    // Insert an internal split node between node's parent and node.
                    let node = attach_target;
                    let parent = tree[node].parent.expect("split target has a parent");
                    let split_idx = tree.len();
                    let depth = tree[parent].depth + 1;
                    tree.push(ArborNode {
                        pos: split_pos,
                        parent: Some(parent),
                        children: vec![node],
                        target_id: None,
                        weight: 0,
                        subtree_weight: 0,
                        radius: 0.0,
                        depth,
                    });
                    // Re-parent `node` under the split node.
                    if let Some(slot) = tree[parent].children.iter().position(|&c| c == node) {
                        tree[parent].children[slot] = split_idx;
                    }
                    tree[node].parent = Some(split_idx);
                    // `node` and its subtree depths shift by +1; fix lazily below.
                    refresh_depths(&mut tree, split_idx);
                    split_idx
                };

                // Append the leaf node.
                let leaf_idx = tree.len();
                let depth = tree[parent_for_leaf].depth + 1;
                tree.push(ArborNode {
                    pos: leaves[li].socket_pos,
                    parent: Some(parent_for_leaf),
                    children: Vec::new(),
                    target_id: Some(leaves[li].target_id),
                    weight: leaves[li].weight,
                    subtree_weight: 0,
                    radius: 0.0,
                    depth,
                });
                tree[parent_for_leaf].children.push(leaf_idx);

                unattached.remove(ui);

                // ── Relaxation: local ancestor-window pass (fixed root + leaves).
                relax_window(
                    &mut tree,
                    parent_for_leaf,
                    params.relax_window,
                    params.relax_lerp,
                    params.relax_repel,
                );
            }
        }

        // ── Width pass (bottom-up, area-preserving √ rule). ──────────────────
        // Edge-splitting can make a parent's index exceed its child's, so a plain
        // reverse-index pass is NOT reliably bottom-up. Walk by descending depth
        // (deepest first) with a deterministic (depth, index) order so every
        // child is summed before its parent.
        let total_weight: i64 = leaves.iter().map(|l| l.weight).sum::<i64>().max(1);
        let mut order: Vec<usize> = (0..tree.len()).collect();
        order.sort_unstable_by(|&a, &b| tree[b].depth.cmp(&tree[a].depth).then_with(|| a.cmp(&b)));
        for &idx in &order {
            let own = tree[idx].weight;
            let child_sum: i64 = tree[idx]
                .children
                .iter()
                .map(|&c| tree[c].subtree_weight)
                .sum();
            tree[idx].subtree_weight = own + child_sum;
            tree[idx].radius = if tree[idx].target_id.is_some() {
                r_floor
            } else {
                let frac = tree[idx].subtree_weight as f32 / total_weight as f32;
                (r_trunk * frac.max(0.0).sqrt()).max(r_floor)
            };
        }

        // ── Stats: fork-degree histogram (internal nodes), depth, width bands.
        let mut neuron_depth_max = 0u32;
        for node in &tree {
            neuron_depth_max = neuron_depth_max.max(node.depth);
            if node.target_id.is_none() && node.parent.is_some() {
                // internal (non-root, non-leaf) fork node
                cluster_count_histogram[node.children.len().min(5)] += 1;
            }
        }
        tree_depth_max = tree_depth_max.max(neuron_depth_max);
        tree_depth_sum += neuron_depth_max as u64;
        tree_depth_count += 1;

        // ── Spline emission: each edge → sampled Bézier → MorphSegment list. ──
        // Carry cumulative path length forward per node so child edges start at
        // the parent edge's end path (path_lengths_match_parent_branch_endpoints).
        let mut node_path_end = vec![0.0f32; tree.len()];
        // Emit edges parent-before-child so node_path_end[parent] is ready when the
        // child edge starts. Edge-splitting can break index ordering, so order by
        // ascending depth (then index) — a deterministic topological order.
        let mut emit_order: Vec<usize> = (1..tree.len()).collect();
        emit_order
            .sort_unstable_by(|&a, &b| tree[a].depth.cmp(&tree[b].depth).then_with(|| a.cmp(&b)));
        for &child in &emit_order {
            let parent = tree[child].parent.expect("non-root has a parent");
            let p_start = tree[parent].pos;
            let p_end = tree[child].pos;
            let is_leaf = tree[child].target_id.is_some();
            // LOCKED lighting rule: leaf edges carry the real target id; internal
            // (trunk/fork) edges carry the SOURCE neuron id. Waypoints are visual
            // route geometry only: the leaf target id / weight are UNCHANGED.
            let seg_target = if is_leaf {
                tree[child].target_id.expect("leaf has a target id")
            } else {
                id
            };

            // Build the route polyline. Local edges are a single hop
            // [p_start, p_end]; a visually-long leaf edge routes through
            // deterministic intermediate waypoints (Stream C).
            let waypoints = if is_leaf {
                long_range_waypoints(
                    seed_lo,
                    id,
                    seg_target,
                    p_start,
                    p_end,
                    brain_center,
                    cell_size,
                    params,
                    &brain_bounds,
                )
            } else {
                Vec::new()
            };
            let is_long_range = !waypoints.is_empty();
            let mut route: Vec<[f32; 3]> = Vec::with_capacity(waypoints.len() + 2);
            route.push(if is_long_range {
                brain_bounds.clamp_point(p_start)
            } else {
                p_start
            });
            route.extend_from_slice(&waypoints);
            route.push(if is_long_range {
                brain_bounds.clamp_point(p_end)
            } else {
                p_end
            });

            let r_parent = tree[parent].radius.max(r_floor);
            let r_child = tree[child].radius;
            let total_route_len: f32 = route
                .windows(2)
                .map(|w| dist(w[0], w[1]).max(1e-4))
                .sum::<f32>()
                .max(1e-4);

            let mut cur_path = node_path_end[parent];
            let mut covered = 0.0f32;
            let mut complete = true;
            for (hop, w) in route.windows(2).enumerate() {
                let h_start = w[0];
                let h_end = w[1];
                let dir = norm(sub(h_end, h_start));
                let edge_len = dist(h_end, h_start).max(1e-4);
                // Radii interpolate from parent → child across the whole route by
                // cumulative arc fraction (continuous taper through waypoints).
                let f0 = covered / total_route_len;
                covered += edge_len;
                let f1 = covered / total_route_len;
                let r0 = lerp(r_parent, r_child, f0).max(r_floor);
                let r1 = lerp(r_parent, r_child, f1).max(r_floor);
                let bend = bend_vector(
                    dir,
                    mix_key(
                        seed_lo,
                        id,
                        seg_target ^ (child as u32) ^ ((hop as u32) << 24),
                        salt::TREE_BEND,
                    ),
                    edge_len * params.axon_curve_lift.max(0.0),
                );
                let p1 = add(add(h_start, scale(dir, edge_len * 0.33)), scale(bend, 0.30));
                let p2 = add(add(h_end, scale(dir, -edge_len * 0.27)), scale(bend, -0.16));
                let p1 = if is_long_range {
                    brain_bounds.clamp_point(p1)
                } else {
                    p1
                };
                let p2 = if is_long_range {
                    brain_bounds.clamp_point(p2)
                } else {
                    p2
                };
                // Adaptive (length + curvature aware) subsegment count, smaller
                // max length on long-range hops for readable pulse motion.
                let curvature = (len(bend) / edge_len).min(4.0);
                let subs = adaptive_subsegments(edge_len, curvature, is_long_range, params);
                let (next_path, hop_complete, _, _) = emit_bezier_path(
                    &mut segments,
                    cap,
                    &mut dropped,
                    id,
                    seg_target,
                    1,
                    h_start,
                    p1,
                    p2,
                    h_end,
                    r0,
                    r1,
                    subs,
                    cur_path,
                    params.taper_curve,
                );
                cur_path = next_path;
                complete &= hop_complete;
            }
            node_path_end[child] = cur_path;
            if !complete {
                all_k_coverage = false;
            } else if is_leaf {
                unique_targets_emitted += 1;
            }
            // Width-band stat on the edge's thicker (parent) radius.
            let rb = tree[parent].radius / r_trunk;
            radius_bands[if rb < 0.25 {
                0
            } else if rb < 0.5 {
                1
            } else if rb < 0.75 {
                2
            } else {
                3
            }] += 1;
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
    let total_ms = setup_ms + incoming_ms + dendrite_ms + axon_ms + finalize_ms;
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
    let mut segments_per_neuron = vec![0usize; n];
    for segment in &segments {
        if let Some(count) = segments_per_neuron.get_mut(segment.neuron_id as usize) {
            *count += 1;
        }
    }
    let segments_per_neuron_max = segments_per_neuron.iter().copied().max().unwrap_or(0);
    segments_per_neuron.sort_unstable();
    let segments_per_neuron_p99 = if segments_per_neuron.is_empty() {
        0
    } else {
        let idx = ((segments_per_neuron.len() as f32 * 0.99).ceil() as usize)
            .saturating_sub(1)
            .min(segments_per_neuron.len() - 1);
        segments_per_neuron[idx]
    };
    let incoming_degrees: Vec<usize> = incoming.ranges.iter().map(|range| range.len).collect();
    let incoming_visible_groups: Vec<usize> = incoming
        .group_ranges
        .iter()
        .map(|range| range.len)
        .collect();
    let incoming_in_degree_mean = mean_usize(&incoming_degrees);
    let incoming_in_degree_p99 = percentile_p99(&incoming_degrees);
    let incoming_in_degree_max = incoming_degrees.iter().copied().max().unwrap_or(0);
    let incoming_visible_groups_mean = mean_usize(&incoming_visible_groups);
    let incoming_visible_groups_p99 = percentile_p99(&incoming_visible_groups);
    let incoming_visible_groups_max = incoming_visible_groups.iter().copied().max().unwrap_or(0);
    let incoming_raw_bytes = incoming.raw.len() * std::mem::size_of::<IncomingSynapse>();
    let incoming_range_bytes = incoming.ranges.len() * std::mem::size_of::<IncomingRange>()
        + incoming.group_ranges.len() * std::mem::size_of::<IncomingRange>();
    let incoming_group_bytes = incoming.groups.len() * std::mem::size_of::<IncomingSocketGroup>();
    let incoming_storage_bytes = incoming_raw_bytes + incoming_range_bytes + incoming_group_bytes;
    let incoming_raw_count = incoming.raw.len();
    let incoming_socket_group_count = incoming.groups.len();
    let duplicate_targets = incoming.duplicate_targets;
    let self_targets = incoming.self_targets;
    let incoming_synapses = incoming.raw;
    let incoming_ranges = incoming.ranges;
    let incoming_socket_groups = incoming.groups;
    let incoming_socket_group_ranges = incoming.group_ranges;
    Morphology {
        segments,
        process_roots,
        incoming_synapses,
        incoming_ranges,
        incoming_socket_groups,
        incoming_socket_group_ranges,
        dropped,
        stats: MorphologyStats {
            neuron_count: n,
            fanout_k: k,
            segment_count,
            dropped_count: dropped,
            segment_cap_per_neuron: params.segment_cap(k),
            segment_cap: cap,
            segment_cap_bytes,
            segment_buffer_bytes,
            segments_per_neuron_p99,
            segments_per_neuron_max,
            cap_utilization,
            duplicate_targets,
            self_targets,
            incoming_raw_count,
            incoming_socket_group_count,
            incoming_in_degree_mean,
            incoming_in_degree_p99,
            incoming_in_degree_max,
            incoming_visible_groups_mean,
            incoming_visible_groups_p99,
            incoming_visible_groups_max,
            incoming_capped_count: 0,
            incoming_dropped_count: incoming_dropped,
            incoming_raw_bytes,
            incoming_range_bytes,
            incoming_group_bytes,
            incoming_storage_bytes,
            unique_targets_expected,
            unique_targets_emitted,
            unique_target_coverage,
            all_k_coverage,
            timings: MorphologyTimings {
                setup_ms,
                incoming_ms,
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
            tree_depth_max,
            tree_depth_mean: if tree_depth_count == 0 {
                0.0
            } else {
                tree_depth_sum as f32 / tree_depth_count as f32
            },
            radius_bands,
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

    fn point_bits(p: [f32; 3]) -> [u32; 3] {
        [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
    }

    #[test]
    fn locked_default_matches_current_constants() {
        let p = MorphologyParams::locked_default();
        assert_eq!(p.base_radius, params::R0);
        assert_eq!(p.axon_stop_fraction, params::AXON_STOP_FRACTION);
        assert_eq!(p.axon_root_radius_fraction, params::AXON_R0_FRACTION);
        assert_eq!(p.axon_curve_lift, 0.15);
        assert_eq!(p.socket_count_min, 2);
        assert_eq!(p.socket_count_max, 4);
        assert_eq!(p.dendrite_primary_root_count, 4);
        assert_eq!(p.dendrite_fork_distance, 1.45);
        assert_eq!(p.dendrite_curve_tightness, 0.55);
        assert_eq!(p.dendrite_mid_radius_fraction, 0.78);
        assert_eq!(p.dendrite_tip_radius_fraction, 0.42);
        assert_eq!(p.dendrite_group_spacing, 0.55);
        // Stream D bushy local-branching defaults (protected — not web-exposed).
        assert_eq!(p.dendrite_branchlet_count, 1);
        assert_eq!(p.dendrite_branchlet_length_fraction, 0.6);
        assert_eq!(p.dendrite_branchlet_radius_fraction, 0.30);
        assert_eq!(p.dendrite_twig_count, 1);
        assert_eq!(p.dendrite_twig_length_fraction, 1.1);
        assert_eq!(p.dendrite_twig_radius_fraction, 0.18);
        assert_eq!(p.dendrite_twig_curl, 1.6);
        assert_eq!(p.dendrite_decor_group_max, 12);
        assert_eq!(p.dendrite_budget, DENDRITE_MAX);
        assert_eq!(
            DENDRITE_MAX,
            160 + DENDRITE_DECOR_GROUP_MAX * DENDRITE_DECOR_PER_GROUP_MAX
        );
        assert_eq!(
            p.terminal_twig_budget,
            (LONG_RANGE_MAX_WAYPOINTS + 1) * EDGE_SUBSEGMENTS_MAX + EDGE_SUBSEGMENTS_MAX
        );
        assert_eq!(p.trunk_cluster_budget, EDGE_SUBSEGMENTS_MAX);
        assert_eq!(p.cap_slack, 4);
        // New Prim-tree generator knobs.
        assert_eq!(p.tree_score_curvature, 0.5);
        assert_eq!(p.tree_score_density, 0.5);
        assert_eq!(p.tree_score_degree, 0.7);
        assert_eq!(p.relax_lerp, 0.25);
        assert_eq!(p.relax_repel, 0.15);
        assert_eq!(p.relax_window, 3);
        assert_eq!(p.edge_subsegments, 3);
        // Adaptive subdivision + waypoint routing defaults (Stream B/C).
        assert_eq!(p.max_segment_length, 0.05);
        assert_eq!(p.long_range_max_segment_length, 0.025);
        assert_eq!(p.curvature_subsegment_boost, 2.0);
        assert_eq!(p.edge_subsegments_max, EDGE_SUBSEGMENTS_MAX);
        assert_eq!(p.min_subsegments, 1);
        assert_eq!(p.long_range_chord_cells, 3.0);
        assert_eq!(p.long_range_max_waypoints, LONG_RANGE_MAX_WAYPOINTS);
        assert_eq!(p.long_range_waypoint_span, 0.20);
        assert_eq!(p.long_range_lateral_offset, 0.12);
        // The long-range hop max segment length must be SMALLER than local so long
        // fibers carry more spatial samples for readable pulse motion.
        assert!(p.long_range_max_segment_length < p.max_segment_length);
    }

    #[test]
    fn segment_layout_is_48_bytes() {
        assert_eq!(std::mem::size_of::<MorphSegment>(), 48);
        assert_eq!(std::mem::size_of::<MorphSegment>() % 16, 0);
    }

    #[test]
    fn sphere_instance_layout_is_48_bytes() {
        assert_eq!(std::mem::size_of::<MorphSphereInstance>(), 48);
        assert_eq!(std::mem::size_of::<MorphSphereInstance>() % 16, 0);
    }

    #[test]
    fn soma_spheres_consume_process_root_descriptor() {
        let params = MorphologyParams::locked_default();
        let positions = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let source_types = [0u8, 1u8];
        let roots = [
            ProcessRoot {
                neuron_id: 0,
                soma_center: positions[0],
                direction: [0.0, 1.0, 0.0],
                soma_root: [1.0, 2.0 + params.base_radius, 3.0],
                first_fork: [1.0, 3.0, 3.0],
                root_radius: params.base_radius * params.axon_root_radius_fraction,
                root_weight: 4,
                unique_target_count: 4,
            },
            ProcessRoot {
                neuron_id: 1,
                soma_center: positions[1],
                direction: [1.0, 0.0, 0.0],
                soma_root: [4.0 + params.base_radius, 5.0, 6.0],
                first_fork: [5.0, 5.0, 6.0],
                root_radius: params.base_radius * params.axon_root_radius_fraction,
                root_weight: 0,
                unique_target_count: 0,
            },
        ];

        let spheres = emit_soma_spheres(&positions, &source_types, &params, &roots);
        assert_eq!(spheres.len(), 2);
        assert_eq!(spheres[0].root_dir, [0.0, 1.0, 0.0]);
        assert!(spheres[0].root_pull > 0.0);
        assert_eq!(spheres[1].root_dir, [0.0; 3]);
        assert_eq!(spheres[1].root_pull, 0.0);
    }

    #[test]
    fn generates_segments_for_every_neuron() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(1234, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            16,
            1234,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        // Every neuron contributes at least one dendrite + (usually) axon segment.
        assert!(!m.segments.is_empty());
        assert_eq!(m.dropped, 0, "should not hit the cap at this size");
        assert_eq!(m.process_roots.len(), pos.len());
        assert_eq!(m.stats.segment_count, m.segments.len());
        assert_eq!(m.stats.neuron_count, pos.len());
        assert_eq!(m.stats.fanout_k, 16);
        assert_eq!(m.stats.segment_cap_per_neuron, params.segment_cap(16));
        assert!(m.stats.segments_per_neuron_p99 <= m.stats.segment_cap_per_neuron);
        assert!(m.stats.segments_per_neuron_max <= m.stats.segment_cap_per_neuron);
        assert!(m.stats.all_k_coverage, "expected current all-K coverage");
        assert_eq!(m.stats.unique_target_coverage, 1.0);
        assert_eq!(m.stats.source_type_bytes, pos.len());
        assert_eq!(
            m.stats.source_type_excitatory + m.stats.source_type_inhibitory,
            pos.len()
        );
        // Fork-degree histogram now counts internal fork nodes (plus index 0 for
        // arbor-less neurons), so it no longer sums to neuron count — just assert
        // it has signal and that some forks (≥2 children) exist.
        assert!(m.stats.cluster_count_histogram.iter().sum::<u32>() > 0);
        assert!(
            m.stats.cluster_count_histogram[2..].iter().sum::<u32>() > 0,
            "expected at least one ≥2-child fork node"
        );
        assert!(
            m.stats.tree_depth_max >= 2,
            "expected shared-trunk depth ≥2"
        );
        assert!(m.stats.radius_bands.iter().sum::<u32>() > 0);
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
                // Dendrites are geometrically target-owned. Shared stems carry
                // self; source-specific leaves carry the presynaptic source id.
                assert!((s.neuron_id as usize) < pos.len());
            }
        }
        for (i, root) in m.process_roots.iter().enumerate() {
            assert_eq!(root.neuron_id, i as u32);
            assert_eq!(root.soma_center, pos[i]);
            assert!(len(root.direction) > 0.999 && len(root.direction) < 1.001);
            assert_eq!(
                root.root_radius,
                params.base_radius * params.axon_root_radius_fraction
            );
            assert!((dist(root.soma_root, root.soma_center) - params.base_radius).abs() < 1e-6);
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
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        let probe = (0..pos.len() as u32)
            .find(|&nid| {
                let src_type = source_types[nid as usize];
                let src_cell = g.unpack(g.cell_of_index(nid));
                let mut expected: Vec<u32> = (0..k as u32)
                    .map(|j| {
                        connectivity::target_with_cell(
                            nid,
                            j,
                            &g,
                            k,
                            seed,
                            src_type,
                            src_cell,
                            connectivity::ReachParams::LOCAL_ONLY,
                        )
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
            .map(|j| {
                connectivity::target_with_cell(
                    probe,
                    j,
                    &g,
                    k,
                    seed,
                    probe_type,
                    probe_cell,
                    connectivity::ReachParams::LOCAL_ONLY,
                )
            })
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
    fn single_target_path_uses_process_root_descriptor() {
        let (pos, g) = small_grid();
        let seed = 2468u32;
        let k = 1usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        let probe = (0..pos.len() as u32)
            .find(|&nid| {
                let src_type = source_types[nid as usize];
                let src_cell = g.unpack(g.cell_of_index(nid));
                connectivity::target_with_cell(
                    nid,
                    0,
                    &g,
                    k,
                    seed,
                    src_type,
                    src_cell,
                    connectivity::ReachParams::LOCAL_ONLY,
                ) != nid
            })
            .expect("need a single-target probe");
        let src_type = source_types[probe as usize];
        let src_cell = g.unpack(g.cell_of_index(probe));
        let expected = connectivity::target_with_cell(
            probe,
            0,
            &g,
            k,
            seed,
            src_type,
            src_cell,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        assert_ne!(expected, probe);
        let root = m.process_roots[probe as usize];
        assert_eq!(root.unique_target_count, 1);
        assert!(root.root_weight > 0);
        assert_eq!(root.soma_center, pos[probe as usize]);
        assert!((dist(root.soma_root, root.soma_center) - params.base_radius).abs() < 1e-6);
        assert!(dot(norm(sub(root.first_fork, root.soma_center)), root.direction) > 0.999);
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
        let mut got_real_targets: Vec<u32> = got.iter().copied().filter(|&t| t != probe).collect();
        got_real_targets.sort_unstable();
        got_real_targets.dedup();
        assert_eq!(got_real_targets, vec![expected]);
        assert!(
            m.segments
                .iter()
                .any(|s| s.kind == 1 && s.neuron_id == probe && s.target_id == probe),
            "single-target path should emit the descriptor-backed shared root"
        );
    }

    #[test]
    fn axon_branches_start_after_descriptor_first_fork() {
        let (pos, g) = small_grid();
        let seed = 6060u32;
        let k = 12usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );

        for root in m
            .process_roots
            .iter()
            .filter(|root| root.unique_target_count > 0)
        {
            let id = root.neuron_id;
            let trunk_chord = dist(root.soma_root, root.first_fork);
            assert!(trunk_chord > params.base_radius);

            let starts_at_root: Vec<&MorphSegment> = m
                .segments
                .iter()
                .filter(|s| s.kind == 1 && s.neuron_id == id && s.a == root.soma_root)
                .collect();
            assert_eq!(
                starts_at_root.len(),
                1,
                "neuron {id} should have exactly one axon segment leaving the soma root"
            );
            assert_eq!(starts_at_root[0].target_id, id);
            assert_eq!(starts_at_root[0].path_len, 0.0);
            assert!((starts_at_root[0].radius_a - root.root_radius).abs() < 1e-6);

            assert!(
                m.segments.iter().any(|s| {
                    s.kind == 1 && s.neuron_id == id && s.target_id == id && s.b == root.first_fork
                }),
                "neuron {id} should finish the source-lit trunk at ProcessRoot::first_fork"
            );

            for s in m
                .segments
                .iter()
                .filter(|s| s.kind == 1 && s.neuron_id == id && s.target_id != id)
            {
                assert!(
                    s.path_len >= trunk_chord * 0.99,
                    "real target segment for neuron {id} starts before the descriptor trunk ends"
                );
            }
        }
    }

    #[test]
    fn terminal_axon_tips_taper_to_twig_floor() {
        let (pos, g) = small_grid();
        let seed = 7070u32;
        let k = 12usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        let r_floor =
            params.base_radius * params.axon_root_radius_fraction * params.twig_radius_fraction;
        let terminal_floor_endpoint_count = m
            .segments
            .iter()
            .filter(|s| {
                s.kind == 1 && s.target_id != s.neuron_id && (s.radius_b - r_floor).abs() < 1e-6
            })
            .count();
        let below_floor_count = m
            .segments
            .iter()
            .filter(|s| s.kind == 1 && s.target_id != s.neuron_id && s.radius_b < r_floor - 1e-6)
            .count();

        assert_eq!(below_floor_count, 0, "terminal axons should not underflow");
        assert!(
            terminal_floor_endpoint_count >= m.stats.unique_targets_emitted,
            "each emitted terminal axon should have a twig-floor endpoint"
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
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );

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
                .map(|j| {
                    connectivity::target_with_cell(
                        probe,
                        j,
                        &g,
                        k,
                        seed,
                        src_type,
                        src_cell,
                        connectivity::ReachParams::LOCAL_ONLY,
                    )
                })
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
        let a = generate(
            &pos,
            &g,
            16,
            99,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        let b = generate(
            &pos,
            &g,
            16,
            99,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
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
        assert_eq!(a.process_roots, b.process_roots);
        for (x, y) in a.segments.iter().zip(b.segments.iter()) {
            assert_eq!(x.a, y.a);
            assert_eq!(x.b, y.b);
            assert_eq!(x.path_len, y.path_len);
        }
    }

    /// Stream B/C: with long-range reach enabled, two generations with the same
    /// seed/config must produce a bit-identical segment buffer (waypoint routing +
    /// adaptive subdivision included). Guards against any runtime/float nondeterminism.
    #[test]
    fn deterministic_with_long_range_waypoints() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(424242, &regions);
        let params = MorphologyParams::locked_default();
        let reach = connectivity::ReachParams {
            long_range_frac: 1024, // ~ heavy long-range share
            max_reach: 4,
        };
        let gen = || generate(&pos, &g, 16, 424242, &params, &source_types, reach);
        let a = gen();
        let b = gen();
        assert_eq!(a.segments.len(), b.segments.len());
        assert_eq!(a.dropped, b.dropped);
        for (x, y) in a.segments.iter().zip(b.segments.iter()) {
            assert_eq!(point_bits(x.a), point_bits(y.a));
            assert_eq!(point_bits(x.b), point_bits(y.b));
            assert_eq!(x.radius_a.to_bits(), y.radius_a.to_bits());
            assert_eq!(x.radius_b.to_bits(), y.radius_b.to_bits());
            assert_eq!(x.path_len.to_bits(), y.path_len.to_bits());
            assert_eq!(x.neuron_id, y.neuron_id);
            assert_eq!(x.kind, y.kind);
            assert_eq!(x.target_id, y.target_id);
        }
    }

    /// Stream C: waypoint routing must NOT change which target ids the axon leaves
    /// land on. The set of leaf (kind==1, target_id != source) terminal targets
    /// emitted with waypoints must equal the set emitted with waypoints disabled.
    #[test]
    fn waypoints_preserve_leaf_target_identity() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(7, &regions);
        let reach = connectivity::ReachParams {
            long_range_frac: 1024,
            max_reach: 4,
        };

        let with_wp = MorphologyParams::locked_default();
        let mut no_wp = MorphologyParams::locked_default();
        no_wp.long_range_max_waypoints = 0; // disable routing, keep everything else

        let routed = generate(&pos, &g, 16, 7, &with_wp, &source_types, reach);
        let straight = generate(&pos, &g, 16, 7, &no_wp, &source_types, reach);

        // Leaf terminal targets per neuron: kind==1 segments whose target_id is the
        // real target (different from the source neuron_id).
        let leaf_targets = |m: &Morphology| -> std::collections::BTreeSet<(u32, u32)> {
            m.segments
                .iter()
                .filter(|s| s.kind == 1 && s.target_id != s.neuron_id)
                .map(|s| (s.neuron_id, s.target_id))
                .collect()
        };
        assert_eq!(
            leaf_targets(&routed),
            leaf_targets(&straight),
            "waypoint routing must not change axon leaf target identity"
        );

        // Sanity: routing actually fired (some long-range edges were detected) →
        // the routed buffer has strictly more segments than the straight one.
        assert!(
            routed.segments.len() > straight.segments.len(),
            "expected waypoint routing to add geometry (routed={}, straight={})",
            routed.segments.len(),
            straight.segments.len()
        );
        assert_eq!(routed.dropped, 0, "waypoint routing must stay within cap");
    }

    #[test]
    fn long_range_waypoints_stay_inside_brain_bounds() {
        let (pos, g) = small_grid();
        let params = MorphologyParams::locked_default();
        let bounds = BrainBounds::from_positions(&pos, params.base_radius);
        let waypoints = long_range_waypoints(
            8675309,
            0,
            (pos.len() - 1) as u32,
            pos[0],
            pos[pos.len() - 1],
            [0.375, 0.375, 0.375],
            g.cell_size,
            &params,
            &bounds,
        );

        assert!(!waypoints.is_empty());
        for waypoint in waypoints {
            assert!(
                bounds.contains(waypoint),
                "waypoint escaped brain bounds: {waypoint:?}"
            );
        }
    }

    #[test]
    fn incoming_synapses_drive_target_owned_dendrites() {
        let (pos, g) = small_grid();
        let seed = 31337u32;
        let k = 16usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        assert_eq!(m.dropped, 0);
        assert_eq!(m.stats.incoming_capped_count, 0);
        assert_eq!(m.stats.incoming_dropped_count, 0);
        assert_eq!(m.incoming_ranges.len(), pos.len());
        assert_eq!(m.incoming_socket_group_ranges.len(), pos.len());
        assert_eq!(m.stats.incoming_raw_count, m.incoming_synapses.len());
        assert_eq!(
            m.stats.incoming_socket_group_count,
            m.incoming_socket_groups.len()
        );
        assert!(m.stats.incoming_socket_group_count <= m.stats.incoming_raw_count);

        let cell_of_neuron = g.cell_of_neuron_map();
        let mut expected = Vec::<(u32, u32, u32)>::new();
        let mut expected_self = 0usize;
        for source in 0..pos.len() as u32 {
            let src_type = source_types[source as usize];
            let src_cell = g.unpack(cell_of_neuron[source as usize]);
            for synapse_index in 0..k as u32 {
                let target = connectivity::target_with_cell(
                    source,
                    synapse_index,
                    &g,
                    k,
                    seed,
                    src_type,
                    src_cell,
                    connectivity::ReachParams::LOCAL_ONLY,
                );
                if target == source {
                    expected_self += 1;
                } else {
                    expected.push((target, source, synapse_index));
                }
            }
        }
        expected.sort_unstable();
        let got: Vec<(u32, u32, u32)> = m
            .incoming_synapses
            .iter()
            .map(|record| (record.target_id, record.source_id, record.synapse_index))
            .collect();
        assert_eq!(got, expected);
        assert_eq!(m.stats.self_targets, expected_self);

        for (target, range) in m.incoming_ranges.iter().enumerate() {
            for record in &m.incoming_synapses[range.start..range.start + range.len] {
                assert_eq!(record.target_id, target as u32);
                assert_ne!(record.source_id, record.target_id);
            }
        }

        for group in &m.incoming_socket_groups {
            let raw_matches: Vec<&IncomingSynapse> = m
                .incoming_synapses
                .iter()
                .filter(|record| {
                    record.source_id == group.source_id
                        && record.target_id == group.target_id
                        && record.socket_idx == group.socket_idx
                })
                .collect();
            assert_eq!(
                raw_matches.len(),
                group.raw_count as usize,
                "group raw_count must aggregate duplicate source/target/socket records"
            );
            let expected_weight: i64 = raw_matches
                .iter()
                .map(|record| record.weight.unsigned_abs().max(1) as i64)
                .sum();
            assert_eq!(group.weight, expected_weight);
            assert!(
                m.segments.iter().any(|segment| {
                    segment.kind == 0
                        && segment.neuron_id == group.target_id
                        && segment.target_id == group.source_id
                        && segment.path_len == 0.0
                        && point_bits(segment.a) == point_bits(group.socket_pos)
                        && segment.radius_a
                            >= params.base_radius * params.dendrite_tip_radius_fraction * 0.75
                }),
                "missing source-specific dendrite leaf for group {group:?}"
            );
        }

        let probe_target = m
            .incoming_socket_groups
            .first()
            .map(|group| group.target_id)
            .expect("small grid should have incoming groups");
        assert!(
            m.segments.iter().any(|segment| segment.kind == 0
                && segment.neuron_id == probe_target
                && segment.target_id == probe_target),
            "target with incoming groups should have shared aggregate stems"
        );
        let root_collars: Vec<&MorphSegment> = m
            .segments
            .iter()
            .filter(|segment| {
                segment.kind == 0
                    && segment.target_id == segment.neuron_id
                    && segment.path_len == 0.0
            })
            .collect();
        assert!(
            !root_collars.is_empty(),
            "incoming arbors should emit target-owned root collars"
        );
        for stem in root_collars {
            assert!(
                dist(stem.a, pos[stem.neuron_id as usize]) >= params.base_radius * 1.02,
                "dendrite collar should start on the soma surface: {stem:?}"
            );
            assert!(
                dist(stem.a, pos[stem.neuron_id as usize]) <= params.base_radius * 1.08,
                "dendrite collar should not enter from a long barrel: {stem:?}"
            );
            assert!(
                stem.radius_a >= params.base_radius * params.dendrite_mid_radius_fraction * 0.99,
                "dendrite collar radius should use visible branch thickness"
            );
        }
    }

    /// Stream D: the bushy local grammar must (a) emit decorative branchlets/twigs,
    /// (b) those decorations carry the neuron's OWN id as owner — never a fake
    /// presynaptic target id, and (c) the real presynaptic leaves still carry
    /// `kind==0` with `target_id == source_id` so the compaction/render shaders
    /// derive presynaptic activity from the SOURCE. Also confirms the trunk→twig
    /// radius taper: decorative twigs are thinner than the soma-proximal collars.
    #[test]
    fn bushy_dendrite_decorations_preserve_presynaptic_owner_rule() {
        let (pos, g) = small_grid();
        let seed = 31337u32;
        let k = 16usize;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        assert_eq!(
            m.dropped, 0,
            "bushy dendrites should fit the cap at this size"
        );

        // Every real presynaptic dendrite leaf still keeps kind==0 and a real,
        // distinct source as target_id (the activity owner the shaders read).
        let valid_sources: std::collections::HashSet<(u32, u32)> = m
            .incoming_socket_groups
            .iter()
            .map(|grp| (grp.target_id, grp.source_id))
            .collect();
        let mut presynaptic_leaves = 0usize;
        for s in &m.segments {
            if s.kind == 0 && s.target_id != s.neuron_id {
                // A target-owned dendrite whose owner != self MUST be a real
                // presynaptic leaf: (neuron_id == target neuron, target_id == a real
                // source for that neuron). No fabricated target ids.
                assert!(
                    valid_sources.contains(&(s.neuron_id, s.target_id)),
                    "dendrite leaf carries a non-source owner (fabricated target): {s:?}"
                );
                presynaptic_leaves += 1;
            }
        }
        assert!(
            presynaptic_leaves > 0,
            "expected real presynaptic dendrite leaves (kind==0, target_id==source)"
        );

        // Decorative branchlets/twigs exist and are self-owned (light with the
        // neuron). Detect them as kind==0, self-owned, non-zero path_len, and
        // rooted away from the soma surface (they sprout off the group tip).
        let r_twig = params.base_radius * params.dendrite_twig_radius_fraction;
        let decorations: Vec<&MorphSegment> = m
            .segments
            .iter()
            .filter(|s| {
                s.kind == 0
                    && s.target_id == s.neuron_id
                    && s.path_len > 0.0
                    && s.radius_a <= params.base_radius * params.dendrite_mid_radius_fraction
            })
            .collect();
        assert!(
            !decorations.is_empty(),
            "Stream D should emit bushy decorative branchlets/twigs"
        );
        // At least one very-thin terminal twig (trunk→twig taper end of the scale).
        assert!(
            m.segments.iter().any(|s| {
                s.kind == 0 && s.target_id == s.neuron_id && s.radius_b <= r_twig * 1.5 + 1e-6
            }),
            "expected thin terminal twigs (radius taper toward the tips)"
        );
    }

    /// Decoration is a configured per-neuron product budget, not a hidden
    /// storage-binding workaround. The value is deterministic and clamped by the
    /// protected hard cap, but does not fall to zero just because N grows.
    #[test]
    fn decoration_budget_is_not_neuron_count_throttled() {
        let configured = DENDRITE_DECOR_GROUP_MAX;
        assert_eq!(effective_decor_group_max(1200, configured), configured);
        assert_eq!(effective_decor_group_max(8_000, configured), configured);
        assert_eq!(effective_decor_group_max(12_000, configured), configured);
        assert_eq!(effective_decor_group_max(20_000, configured), configured);
        // A configured value is never exceeded by the clamp.
        assert!(effective_decor_group_max(1, 999) <= DENDRITE_DECOR_GROUP_MAX);
    }

    #[test]
    fn path_lengths_match_parent_branch_endpoints() {
        let (pos, g) = small_grid();
        let seed = 8080u32;
        let regions = small_regions(pos.len());
        let source_types = build_source_types(seed, &regions);
        let params = MorphologyParams::locked_default();
        let m = generate(
            &pos,
            &g,
            16,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );

        let mut branch_end_paths: std::collections::HashMap<(u32, u32, [u32; 3]), Vec<f32>> =
            std::collections::HashMap::new();
        for s in &m.segments {
            let end_path = s.path_len + len(sub(s.b, s.a));
            branch_end_paths
                .entry((s.neuron_id, s.kind, point_bits(s.b)))
                .or_default()
                .push(end_path);
        }

        for s in &m.segments {
            if s.path_len == 0.0 {
                if s.kind == 1 {
                    let expected_root = m.process_roots[s.neuron_id as usize].soma_root;
                    assert_eq!(s.a, expected_root, "zero path_len starts at branch root");
                } else {
                    assert_eq!(s.neuron_id.min((pos.len() - 1) as u32), s.neuron_id);
                }
                continue;
            }

            let parent_key = (s.neuron_id, s.kind, point_bits(s.a));
            let Some(candidates) = branch_end_paths.get(&parent_key) else {
                panic!("missing parent path for segment {s:?}");
            };
            assert!(
                candidates
                    .iter()
                    .any(|end_path| (end_path - s.path_len).abs() < 1e-5),
                "segment path_len does not match any parent end path: {s:?}"
            );
        }
    }

    #[test]
    fn seed_changes_morphology() {
        let (pos, g) = small_grid();
        let regions = small_regions(pos.len());
        let source_types = build_source_types(1, &regions);
        let source_types_b = build_source_types(2, &regions);
        let params = MorphologyParams::locked_default();
        let a = generate(
            &pos,
            &g,
            16,
            1,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        let b = generate(
            &pos,
            &g,
            16,
            2,
            &params,
            &source_types_b,
            connectivity::ReachParams::LOCAL_ONLY,
        );
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
        let m = generate(
            &pos,
            &g,
            16,
            7,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
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
        let _m = generate(
            &pos,
            &g,
            k,
            seed,
            &params,
            &source_types,
            connectivity::ReachParams::LOCAL_ONLY,
        );
        let probe = (0..pos.len() as u32)
            .find(|&nid| {
                let src_type = source_types[nid as usize];
                let src_cell = g.unpack(g.cell_of_index(nid));
                let mut expected: Vec<u32> = (0..k as u32)
                    .map(|j| {
                        connectivity::target_with_cell(
                            nid,
                            j,
                            &g,
                            k,
                            seed,
                            src_type,
                            src_cell,
                            connectivity::ReachParams::LOCAL_ONLY,
                        )
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
            .map(|j| {
                connectivity::target_with_cell(
                    probe,
                    j,
                    &g,
                    k,
                    seed,
                    src_type,
                    src_cell,
                    connectivity::ReachParams::LOCAL_ONLY,
                )
            })
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
            neuron_count: 2,
            fanout_k: 4,
            segment_count: 10,
            dropped_count: 1,
            segment_cap_per_neuron: 6,
            segment_cap: 12,
            segment_cap_bytes: 576,
            segment_buffer_bytes: 480,
            segments_per_neuron_p99: 5,
            segments_per_neuron_max: 6,
            cap_utilization: 0.8333333,
            duplicate_targets: 2,
            self_targets: 3,
            incoming_raw_count: 8,
            incoming_socket_group_count: 7,
            incoming_in_degree_mean: 4.0,
            incoming_in_degree_p99: 5,
            incoming_in_degree_max: 5,
            incoming_visible_groups_mean: 3.5,
            incoming_visible_groups_p99: 4,
            incoming_visible_groups_max: 4,
            incoming_capped_count: 0,
            incoming_dropped_count: 0,
            incoming_raw_bytes: 384,
            incoming_range_bytes: 64,
            incoming_group_bytes: 336,
            incoming_storage_bytes: 784,
            source_type_bytes: 4,
            source_type_excitatory: 3,
            source_type_inhibitory: 1,
            cluster_count_histogram: [0, 1, 2, 3, 4, 5],
            terminal_socket_distance_bands: [1, 2, 3, 4],
            socket_reuse_bands: [5, 6, 7, 8],
            tree_depth_max: 4,
            tree_depth_mean: 2.5,
            radius_bands: [9, 8, 7, 6],
            unique_targets_expected: 7,
            unique_targets_emitted: 7,
            unique_target_coverage: 1.0,
            all_k_coverage: true,
            timings: MorphologyTimings {
                setup_ms: 0.1,
                incoming_ms: 0.15,
                dendrite_ms: 0.2,
                axon_ms: 0.3,
                finalize_ms: 0.0,
                total_ms: 0.6,
            },
        };
        let json = stats.to_json();
        for field in [
            "\"neuron_count\":2",
            "\"fanout_k\":4",
            "\"segment_count\":10",
            "\"dropped_count\":1",
            "\"segment_cap_per_neuron\":6",
            "\"segment_cap\":12",
            "\"segment_buffer_bytes\":480",
            "\"segments_per_neuron_p99\":5",
            "\"segments_per_neuron_max\":6",
            "\"incoming_raw_count\":8",
            "\"incoming_socket_group_count\":7",
            "\"incoming_in_degree_mean\":4.000000",
            "\"incoming_visible_groups_p99\":4",
            "\"incoming_storage_bytes\":784",
            "\"cluster_count_histogram\":[0,1,2,3,4,5]",
            "\"terminal_socket_distance_bands\":[1,2,3,4]",
            "\"socket_reuse_bands\":[5,6,7,8]",
            "\"tree_depth_max\":4",
            "\"radius_bands\":[9,8,7,6]",
            "\"all_k_coverage\":true",
            "\"setup_ms\":0.100",
            "\"incoming_ms\":0.150",
            "\"total_ms\":0.600",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }
    }

    #[test]
    fn lighting_config_default_matches_product_default() {
        assert_eq!(LightingConfig::default().resting_brightness, 0.0);
    }

    #[test]
    fn generator_config_exposes_bounded_subdivision_controls() {
        let base = MorphologyParams::locked_default();
        let cfg = GeneratorConfig {
            max_segment_length: 0.02,
            long_range_max_segment_length: 0.014,
            curvature_subsegment_boost: 3.5,
            edge_subsegments_max: EDGE_SUBSEGMENTS_MAX,
            min_subsegments: 2,
            ..GeneratorConfig::from_params(&base)
        };
        let params = cfg.apply_to(&base);
        assert_eq!(params.max_segment_length, 0.02);
        assert_eq!(params.long_range_max_segment_length, 0.014);
        assert_eq!(params.curvature_subsegment_boost, 3.5);
        assert_eq!(params.edge_subsegments_max, EDGE_SUBSEGMENTS_MAX);
        assert_eq!(params.min_subsegments, 2);

        let too_high = GeneratorConfig {
            edge_subsegments_max: EDGE_SUBSEGMENTS_MAX + 20,
            min_subsegments: EDGE_SUBSEGMENTS_MAX + 20,
            curvature_subsegment_boost: 9.0,
            ..cfg
        }
        .apply_to(&base);
        assert_eq!(too_high.edge_subsegments_max, EDGE_SUBSEGMENTS_MAX);
        assert_eq!(too_high.min_subsegments, EDGE_SUBSEGMENTS_MAX);
        assert_eq!(too_high.curvature_subsegment_boost, 4.0);
    }

    #[test]
    fn morphology_config_missing_lighting_field_uses_product_default() {
        let cfg = MorphologyConfig::from_json(r#"{"lighting":{"ambient":0.7}}"#)
            .expect("partial config should deserialize with defaults");
        assert_eq!(cfg.lighting.ambient, 0.7);
        assert_eq!(cfg.lighting.resting_brightness, 0.0);
    }
}
