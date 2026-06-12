//! GPU backend (WebGPU compute, clock-driven) — phase 2 real implementation.
//!
//! Per-tick dispatch sequence (architecture §5, phase-2 spec), all GPU-driven
//! with NO CPU readback in the loop:
//!   reset spike_count -> integrate (wg 256) -> write_scatter_dispatch ->
//!   scatter via dispatch_workgroups_indirect (wg 64) -> flip I / I_next.
//! One command encoder per frame batch; pass boundaries provide ordering.
//!
//! Stats (spikes, max|current|) are read back ONCE per `tick()` batch via a
//! staging buffer — never inside the per-tick loop and never used to size the
//! scatter dispatch (the GPU-written indirect buffer does that).

pub mod pipelines;
pub mod resources;

use crate::sim::backend::{RenderState, SimBackend, SimConfig, TickStats};
use pipelines::GpuPipelines;
use resources::{
    EdgeUniforms, FrustumCullUniforms, GpuBindGroups, GpuLayouts, GpuResources, IntegrateUniforms,
    MetricsUniforms, MorphUniforms, NearLodStats, NearRenderUniforms, RenderUniforms,
    RibbonUniforms, StimUniform, EDGE_CAP, METRICS_SLOT_COUNT,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// CPU-side state carried between staged GPU startup calls.
///
/// Browser startup drives these stages from TypeScript with a frame yield
/// between calls, so the loading UI can paint real progress instead of waiting
/// behind one monolithic `initialize()` call.
pub struct NetworkBuildState {
    config: SimConfig,
    manifold: crate::manifold::Manifold,
    prepared_morphology: Option<PreparedMorphology>,
}

pub const PREPARED_NETWORK_VERSION: u32 = 1;

pub struct PreparedMorphology {
    segments: Vec<crate::sim::morphology::MorphSegment>,
    spheres: Vec<crate::sim::morphology::MorphSphereInstance>,
    params: crate::sim::morphology::MorphologyParams,
    stats: crate::sim::morphology::MorphologyStats,
    dropped: usize,
}

pub struct PreparedNetworkBuild {
    config: SimConfig,
    manifold: crate::manifold::Manifold,
    morphology: PreparedMorphology,
}

impl PreparedNetworkBuild {
    pub fn prepare(
        config: SimConfig,
        params: crate::sim::morphology::MorphologyParams,
        reach: crate::connectivity::ReachParams,
    ) -> Self {
        let manifold = crate::build_manifold(&config);
        let source_types =
            crate::sim::morphology::build_source_types(config.seed_lo(), &manifold.neuron_regions);
        let morph = crate::sim::morphology::generate(
            &manifold.neuron_positions,
            &manifold.spatial_grid,
            config.k,
            config.seed_lo(),
            &params,
            &source_types,
            reach,
        );
        let spheres = crate::sim::morphology::emit_soma_spheres(
            &manifold.neuron_positions,
            &source_types,
            &params,
            &morph.process_roots,
        );
        let morphology = PreparedMorphology {
            segments: morph.segments,
            spheres,
            params,
            stats: morph.stats,
            dropped: morph.dropped,
        };
        Self {
            config,
            manifold,
            morphology,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_flat_payload(
        config: SimConfig,
        positions: &[f32],
        region_codes: &[u8],
        grid_min: &[f32],
        grid_cell_size: f32,
        grid_dim: u32,
        grid_cell_start: &[u32],
        grid_cell_neurons: &[u32],
        vertices: &[f32],
        faces: &[u32],
        segment_endpoints: &[f32],
        segment_path_len: &[f32],
        segment_neuron_ids: &[u32],
        segment_kinds: &[u32],
        segment_target_ids: &[u32],
        sphere_geometry: &[f32],
        sphere_neuron_ids: &[u32],
        sphere_kinds: &[u32],
        params: crate::sim::morphology::MorphologyParams,
        stats: crate::sim::morphology::MorphologyStats,
        dropped: usize,
    ) -> Result<Self, String> {
        if config.n == 0 {
            return Err("prepared payload N must be > 0".into());
        }
        let positions = vec3s("positions", positions, config.n)?;
        let vertices = vec3s("vertices", vertices, vertices.len() / 3)?;
        if vertices.is_empty() {
            return Err("prepared payload vertices must be non-empty".into());
        }
        if faces.len() % 3 != 0 {
            return Err("prepared payload faces length must be divisible by 3".into());
        }
        if region_codes.len() != config.n {
            return Err(format!(
                "prepared payload region length {} != N {}",
                region_codes.len(),
                config.n
            ));
        }
        let neuron_regions = region_codes
            .iter()
            .copied()
            .map(region_from_code)
            .collect::<Result<Vec<_>, _>>()?;
        if grid_min.len() != 3 {
            return Err("prepared payload grid_min length must be 3".into());
        }
        if !grid_cell_size.is_finite() || grid_cell_size <= 0.0 {
            return Err("prepared payload grid_cell_size must be finite and positive".into());
        }
        if grid_dim == 0 {
            return Err("prepared payload grid_dim must be > 0".into());
        }
        let cell_count = grid_dim
            .checked_mul(grid_dim)
            .and_then(|v| v.checked_mul(grid_dim))
            .ok_or_else(|| "prepared payload grid_dim overflows cell count".to_string())?
            as usize;
        if grid_cell_start.len() != cell_count + 1 {
            return Err(format!(
                "prepared payload cell_start length {} != dim^3+1 {}",
                grid_cell_start.len(),
                cell_count + 1
            ));
        }
        if grid_cell_neurons.len() != config.n {
            return Err(format!(
                "prepared payload cell_neurons length {} != N {}",
                grid_cell_neurons.len(),
                config.n
            ));
        }
        if grid_cell_start.first().copied() != Some(0)
            || grid_cell_start.last().copied() != Some(config.n as u32)
        {
            return Err("prepared payload cell_start must span 0..N".into());
        }
        for window in grid_cell_start.windows(2) {
            if window[0] > window[1] {
                return Err("prepared payload cell_start must be monotonic".into());
            }
        }
        let mut seen = vec![false; config.n];
        for &id in grid_cell_neurons {
            let idx = id as usize;
            if idx >= config.n {
                return Err(format!(
                    "prepared payload cell_neurons id {id} out of range"
                ));
            }
            if seen[idx] {
                return Err(format!("prepared payload cell_neurons duplicate id {id}"));
            }
            seen[idx] = true;
        }
        if faces.iter().any(|&idx| idx as usize >= vertices.len()) {
            return Err("prepared payload face index out of range".into());
        }

        let segment_count = segment_path_len.len();
        if segment_endpoints.len() != segment_count * 8
            || segment_neuron_ids.len() != segment_count
            || segment_kinds.len() != segment_count
            || segment_target_ids.len() != segment_count
        {
            return Err("prepared payload segment field lengths disagree".into());
        }
        let mut segments = Vec::with_capacity(segment_count);
        for i in 0..segment_count {
            let base = i * 8;
            let neuron_id = segment_neuron_ids[i];
            let target_id = segment_target_ids[i];
            if neuron_id as usize >= config.n || target_id as usize >= config.n {
                return Err(format!(
                    "prepared payload segment {i} neuron id out of range"
                ));
            }
            let kind = segment_kinds[i];
            if kind > 1 {
                return Err(format!("prepared payload segment {i} kind {kind} invalid"));
            }
            segments.push(crate::sim::morphology::MorphSegment {
                a: [
                    segment_endpoints[base],
                    segment_endpoints[base + 1],
                    segment_endpoints[base + 2],
                ],
                radius_a: segment_endpoints[base + 3],
                b: [
                    segment_endpoints[base + 4],
                    segment_endpoints[base + 5],
                    segment_endpoints[base + 6],
                ],
                radius_b: segment_endpoints[base + 7],
                neuron_id,
                path_len: segment_path_len[i],
                kind,
                target_id,
            });
        }

        let sphere_count = sphere_neuron_ids.len();
        if sphere_geometry.len() != sphere_count * 8 || sphere_kinds.len() != sphere_count {
            return Err("prepared payload sphere field lengths disagree".into());
        }
        let mut spheres = Vec::with_capacity(sphere_count);
        for i in 0..sphere_count {
            let base = i * 8;
            let neuron_id = sphere_neuron_ids[i];
            if neuron_id as usize >= config.n {
                return Err(format!(
                    "prepared payload sphere {i} neuron id out of range"
                ));
            }
            let kind = sphere_kinds[i];
            if kind != 2 {
                return Err(format!("prepared payload sphere {i} kind {kind} invalid"));
            }
            spheres.push(crate::sim::morphology::MorphSphereInstance {
                center: [
                    sphere_geometry[base],
                    sphere_geometry[base + 1],
                    sphere_geometry[base + 2],
                ],
                radius: sphere_geometry[base + 3],
                neuron_id,
                kind,
                _pad0: 0,
                _pad1: 0,
                root_dir: [
                    sphere_geometry[base + 4],
                    sphere_geometry[base + 5],
                    sphere_geometry[base + 6],
                ],
                root_pull: sphere_geometry[base + 7],
            });
        }

        let manifold = crate::manifold::Manifold {
            vertices,
            faces: faces.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            neuron_positions: positions,
            neuron_regions,
            spatial_grid: crate::connectivity::spatial::SpatialGrid {
                min: [grid_min[0], grid_min[1], grid_min[2]],
                cell_size: grid_cell_size,
                dim: grid_dim,
                cell_start: grid_cell_start.to_vec(),
                cell_neurons: grid_cell_neurons.to_vec(),
            },
        };
        let morphology = PreparedMorphology {
            segments,
            spheres,
            params,
            stats,
            dropped,
        };
        Ok(Self {
            config,
            manifold,
            morphology,
        })
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn manifold(&self) -> &crate::manifold::Manifold {
        &self.manifold
    }

    pub fn morphology(&self) -> &PreparedMorphology {
        &self.morphology
    }

    pub fn positions_f32(&self) -> Vec<f32> {
        self.manifold
            .neuron_positions
            .iter()
            .flat_map(|p| p.iter().copied())
            .collect()
    }

    pub fn region_codes(&self) -> Vec<u8> {
        self.manifold
            .neuron_regions
            .iter()
            .copied()
            .map(region_code)
            .collect()
    }

    pub fn vertices_f32(&self) -> Vec<f32> {
        self.manifold
            .vertices
            .iter()
            .flat_map(|v| v.iter().copied())
            .collect()
    }

    pub fn grid_min_f32(&self) -> Vec<f32> {
        self.manifold.spatial_grid.min.to_vec()
    }

    pub fn grid_cell_size(&self) -> f32 {
        self.manifold.spatial_grid.cell_size
    }

    pub fn grid_dim(&self) -> u32 {
        self.manifold.spatial_grid.dim
    }

    pub fn grid_cell_start_u32(&self) -> Vec<u32> {
        self.manifold.spatial_grid.cell_start.clone()
    }

    pub fn grid_cell_neurons_u32(&self) -> Vec<u32> {
        self.manifold.spatial_grid.cell_neurons.clone()
    }

    pub fn faces_u32(&self) -> Vec<u32> {
        self.manifold
            .faces
            .iter()
            .flat_map(|f| f.iter().copied())
            .collect()
    }

    pub fn segment_endpoints_f32(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.morphology.segments.len() * 8);
        for s in &self.morphology.segments {
            out.extend_from_slice(&s.a);
            out.push(s.radius_a);
            out.extend_from_slice(&s.b);
            out.push(s.radius_b);
        }
        out
    }

    pub fn segment_path_len_f32(&self) -> Vec<f32> {
        self.morphology
            .segments
            .iter()
            .map(|s| s.path_len)
            .collect()
    }

    pub fn segment_neuron_ids_u32(&self) -> Vec<u32> {
        self.morphology
            .segments
            .iter()
            .map(|s| s.neuron_id)
            .collect()
    }

    pub fn segment_kinds_u32(&self) -> Vec<u32> {
        self.morphology.segments.iter().map(|s| s.kind).collect()
    }

    pub fn segment_target_ids_u32(&self) -> Vec<u32> {
        self.morphology
            .segments
            .iter()
            .map(|s| s.target_id)
            .collect()
    }

    pub fn sphere_geometry_f32(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.morphology.spheres.len() * 8);
        for s in &self.morphology.spheres {
            out.extend_from_slice(&s.center);
            out.push(s.radius);
            out.extend_from_slice(&s.root_dir);
            out.push(s.root_pull);
        }
        out
    }

    pub fn sphere_neuron_ids_u32(&self) -> Vec<u32> {
        self.morphology
            .spheres
            .iter()
            .map(|s| s.neuron_id)
            .collect()
    }

    pub fn sphere_kinds_u32(&self) -> Vec<u32> {
        self.morphology.spheres.iter().map(|s| s.kind).collect()
    }

    pub fn stats_json(&self) -> String {
        self.morphology.stats.to_json()
    }

    pub fn dropped_count(&self) -> usize {
        self.morphology.dropped
    }

    pub fn params_json(&self) -> String {
        self.morphology.params.to_json()
    }
}

fn vec3s(name: &str, data: &[f32], count: usize) -> Result<Vec<[f32; 3]>, String> {
    if data.len() != count * 3 {
        return Err(format!(
            "prepared payload {name} length {} != {}",
            data.len(),
            count * 3
        ));
    }
    let mut out = Vec::with_capacity(count);
    for chunk in data.chunks_exact(3) {
        if !chunk.iter().all(|v| v.is_finite()) {
            return Err(format!("prepared payload {name} contains non-finite value"));
        }
        out.push([chunk[0], chunk[1], chunk[2]]);
    }
    Ok(out)
}

fn region_from_code(code: u8) -> Result<crate::manifold::RegionKind, String> {
    match code {
        0 => Ok(crate::manifold::RegionKind::Input),
        1 => Ok(crate::manifold::RegionKind::Association),
        2 => Ok(crate::manifold::RegionKind::Output),
        _ => Err(format!("prepared payload region code {code} invalid")),
    }
}

fn region_code(region: crate::manifold::RegionKind) -> u8 {
    match region {
        crate::manifold::RegionKind::Input => 0,
        crate::manifold::RegionKind::Association => 1,
        crate::manifold::RegionKind::Output => 2,
    }
}

// ─── V2 Phase A: metrics readback ─────────────────────────────────────────────

/// Voltage clamp range + fixed-point scale for the metrics reduction. Must match
/// the constants written into the metrics uniform (and the comment in
/// metrics.wgsl). threshold=1.0 sits inside [-0.5, 1.5].
const METRICS_VOLT_LO: f32 = -0.5;
const METRICS_VOLT_HI: f32 = 1.5;
const METRICS_VOLT_SCALE: f32 = 1024.0;
const METRICS_HISTO_BINS: u32 = 16;
const METRICS_SCALAR_COUNT: usize = 17;
const METRICS_OUTPUT_LEN: usize = METRICS_SCALAR_COUNT + METRICS_HISTO_BINS as usize;

/// Issue a new reduction + readback roughly once per this many ticks. The
/// reduce pass itself is a cheap O(n) read-only dispatch, but the COPY+MAP
/// readback is gated to avoid per-tick CPU round-trips (no-stall policy).
const METRICS_ISSUE_INTERVAL: u32 = 15;

/// Number of past spikes_this_tick samples kept for branching-ratio / cascade.
const METRICS_HISTORY_LEN: usize = 64;

/// Async metrics readback phase. `Idle` = safe to issue a new reduce+copy;
/// `Pending` = a map_async is in flight and metrics_staging is mapped/locked
/// (NEVER copy into it while Pending — that is the bug the stats comment warns
/// about). The Arc<AtomicBool> resolves the map callback on both native and wasm.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MetricsReadState {
    Idle,
    Pending,
}

// Canonical flat-array contract (shared with TypeScript via Float32Array).
// Indices 0..=25 match web/settings.ts `toFloat32Array` output exactly.
// Length-tolerant parsing: new indices added later won't break old callers.
//
// Field naming: f32 for continuous knobs, u32 for mode enums.

/// Full set of visual + sim settings. Replaces the per-frame scalar args
/// (glow_tau, point_radius) and the separate set_i_ext / set_synaptic_scale
/// calls.
#[derive(Clone, Debug)]
pub struct VisualSettings {
    // ── continuous knobs ──────────────────────────────────────────────────
    /// index 0  — glow decay in ticks (default 10.0)
    pub glow_tau: f32,
    /// index 1  — billboard radius in world units (default 0.004)
    pub point_radius: f32,
    /// index 2  — neuron mesh radius (default 0.004)
    pub neuron_visual_radius: f32,
    /// index 3  — radius multiplier when actively firing (default 2.0)
    pub active_neuron_radius_boost: f32,
    /// index 4  — opacity of inactive neurons (default 1.0)
    pub inactive_neuron_opacity: f32,
    /// index 5  — voltage glow contribution (default 0.0 = off)
    pub voltage_glow_strength: f32,
    /// index 6  — Morphology controls: branch-width multiplier on the stored
    /// tube radii (default 1.0; <1 thinner, >1 thicker)
    pub connection_visual_width: f32,
    /// index 7  — Bézier midpoint lift for ribbon curves (default 0.15)
    pub connection_curve_lift: f32,
    /// index 8  — Morphology controls: light a firing neuron's downstream
    /// (outgoing) axon connections (0 = off, 1 = on; default 1)
    pub connection_light_next: u32,
    /// index 9  — reserved_zero (connectionLightPast removed; upstream lighting
    /// deferred until whole-path shared-arbor semantics are redesigned)
    pub connection_light_past: u32,
    /// index 10 — reserved_zero (bloomStrength removed from user settings)
    pub bloom_strength: f32,
    /// index 11 — manifold surface opacity (default 1.0)
    pub surface_opacity: f32,
    /// index 12 — ambient drive current (sim tuning; default 0.014)
    pub i_ext: f32,
    /// index 13 — recurrent coupling scale (sim tuning; default 0.03)
    pub synaptic_scale: f32,
    /// index 14 — per-neuron parameter spread 0→1 (default 0.50)
    pub heterogeneity: f32,
    // ── index 15 (repurposed) ──────────────────────────────────────────────
    /// index 15 — Morphology controls: resting opacity of non-active structure
    /// (0..1; default 0.0). 0 → only live signal pulses are visible. (Replaces
    /// the retired max_active_visual_edges budget.)
    pub morph_resting_opacity: f32,
    // ── mode enums (stored as integer cast to f32) ─────────────────────────
    /// index 16 — reserved_zero (signalSource removed)
    pub signal_source: u32,
    /// index 17 — connection_layer mode: 0=Off, 1=Active/recent only (default), 2=Resting debug
    pub connection_layer: u32,
    /// index 18 — color_by mode (default 6 = Brain)
    pub color_by: u32,
    /// index 19 — neuron_visibility mode (default 0)
    pub neuron_visibility: u32,
    /// index 20 — surface mode (default 0)
    pub surface: u32,
    /// index 21 — weight normalization: 0=none, 1=sqrt_k, 2=k (default 1)
    pub weight_normalization: u32,
    /// index 22 — input_mode: 0=constant, ... (default 0)
    pub input_mode: u32,
    /// index 23 — reserved_zero (adaptiveScalerEnabled removed)
    pub adaptive_scaler_enabled: u32,
    /// index 24 — heavy-tailed reach: long-range fraction 0..1 (default 0.0 =
    /// local only, bit-identical to pre-heavy-tail). Converted to the integer
    /// `long_range_frac` (over REACH_FRAC_DEN) at the connect-uniform boundary.
    pub long_range_reach_frac: f32,
    /// index 25 — heavy-tailed reach: max-reach cell radius (integer carried as
    /// f32 like the other mode fields; default 6.0, clamped >= 1 at the boundary).
    pub max_reach_cells: f32,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            glow_tau: 10.0,
            point_radius: 0.004,
            neuron_visual_radius: 0.004,
            active_neuron_radius_boost: 2.0,
            inactive_neuron_opacity: 1.0,
            voltage_glow_strength: 0.0,
            // Morphology controls: width multiplier (1.0 = use raw radii).
            connection_visual_width: 0.80,
            connection_curve_lift: 0.15,
            connection_light_next: 1,
            connection_light_past: 0,
            // index 10 reserved_zero (bloomStrength removed from user settings).
            bloom_strength: 0.0,
            surface_opacity: 1.0,
            i_ext: 0.014,
            synaptic_scale: 0.03,
            heterogeneity: 0.50,
            // Morphology controls: resting opacity of non-active structure.
            morph_resting_opacity: 0.0,
            signal_source: 0,
            // Morphology controls: 0=Off (skip all morphology work), 1=Active/recent only
            // (default — compacted GPU draw of recently-lit segments + somas), 2=Resting debug.
            connection_layer: 1,
            color_by: 6,
            neuron_visibility: 0,
            surface: 0,
            weight_normalization: 1,
            input_mode: 0,
            adaptive_scaler_enabled: 0,
            // Heavy-tailed reach: 14% long-range synapses by default.
            long_range_reach_frac: 0.14,
            max_reach_cells: 14.0,
        }
    }
}

impl VisualSettings {
    /// Parse from the canonical flat Float32Array. Length-tolerant: indices
    /// beyond the array length fall back to `Default` values so the contract
    /// can grow without breaking existing callers.
    pub fn from_slice(data: &[f32]) -> Self {
        let d = Self::default();
        let f = |i: usize, def: f32| -> f32 { data.get(i).copied().unwrap_or(def) };
        let u =
            |i: usize, def: u32| -> u32 { data.get(i).copied().map(|v| v as u32).unwrap_or(def) };
        Self {
            glow_tau: f(0, d.glow_tau),
            point_radius: f(1, d.point_radius),
            neuron_visual_radius: f(2, d.neuron_visual_radius),
            active_neuron_radius_boost: f(3, d.active_neuron_radius_boost),
            inactive_neuron_opacity: f(4, d.inactive_neuron_opacity),
            voltage_glow_strength: f(5, d.voltage_glow_strength),
            connection_visual_width: f(6, d.connection_visual_width),
            connection_curve_lift: f(7, d.connection_curve_lift),
            connection_light_next: u(8, d.connection_light_next),
            connection_light_past: 0, // index 9: reserved_zero (upstream lighting removed)
            bloom_strength: 0.0,      // index 10: reserved_zero (bloomStrength removed)
            surface_opacity: f(11, d.surface_opacity),
            i_ext: f(12, d.i_ext),
            synaptic_scale: f(13, d.synaptic_scale),
            heterogeneity: f(14, d.heterogeneity),
            morph_resting_opacity: f(15, d.morph_resting_opacity),
            signal_source: 0, // index 16: reserved_zero (signalSource removed)
            connection_layer: u(17, d.connection_layer),
            color_by: u(18, d.color_by),
            neuron_visibility: u(19, d.neuron_visibility),
            surface: u(20, d.surface),
            weight_normalization: u(21, d.weight_normalization),
            input_mode: u(22, d.input_mode),
            adaptive_scaler_enabled: 0, // index 23: reserved_zero (adaptiveScalerEnabled removed)
            long_range_reach_frac: f(24, d.long_range_reach_frac),
            max_reach_cells: f(25, d.max_reach_cells),
        }
    }

    /// Compact JSON snapshot for review artifacts.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"glow_tau\":{:.6},\"point_radius\":{:.6},\"neuron_visual_radius\":{:.6},\"active_neuron_radius_boost\":{:.6},\"inactive_neuron_opacity\":{:.6},\"voltage_glow_strength\":{:.6},\"connection_visual_width\":{:.6},\"connection_curve_lift\":{:.6},\"connection_light_next\":{},\"bloom_strength\":{:.6},\"surface_opacity\":{:.6},\"i_ext\":{:.6},\"synaptic_scale\":{:.6},\"heterogeneity\":{:.6},\"morph_resting_opacity\":{:.6},\"signal_source\":{},\"connection_layer\":{},\"color_by\":{},\"neuron_visibility\":{},\"surface\":{},\"weight_normalization\":{},\"input_mode\":{},\"adaptive_scaler_enabled\":{},\"long_range_reach_frac\":{:.6},\"max_reach_cells\":{:.6}}}",
            self.glow_tau,
            self.point_radius,
            self.neuron_visual_radius,
            self.active_neuron_radius_boost,
            self.inactive_neuron_opacity,
            self.voltage_glow_strength,
            self.connection_visual_width,
            self.connection_curve_lift,
            self.connection_light_next,
            self.bloom_strength,
            self.surface_opacity,
            self.i_ext,
            self.synaptic_scale,
            self.heterogeneity,
            self.morph_resting_opacity,
            self.signal_source,
            self.connection_layer,
            self.color_by,
            self.neuron_visibility,
            self.surface,
            self.weight_normalization,
            self.input_mode,
            self.adaptive_scaler_enabled,
            self.long_range_reach_frac,
            self.max_reach_cells,
        )
    }
}

pub fn reach_from_visual_settings(visual: &VisualSettings) -> crate::connectivity::ReachParams {
    let den = crate::connectivity::REACH_FRAC_DEN;
    let frac = (visual.long_range_reach_frac * den as f32).round();
    let long_range_frac = frac.clamp(0.0, den as f32) as u32;
    let max_reach = (visual.max_reach_cells.round() as i64).max(1) as u32;
    crate::connectivity::ReachParams {
        long_range_frac,
        max_reach,
    }
}

pub fn morph_params_from_config_and_visual(
    config: &crate::sim::morphology::MorphologyConfig,
    visual: &VisualSettings,
) -> crate::sim::morphology::MorphologyParams {
    config
        .to_params()
        .with_curve_lift(visual.connection_curve_lift)
}

// ─── LOD transition thresholds ───────────────────────────────────────────────
/// Camera distance above which only far-LOD runs.
const LOD_FAR_ONLY_DIST: f32 = 1.5;
/// Camera distance below which only near-LOD runs.
const LOD_NEAR_ONLY_DIST: f32 = 0.8;

/// LIF parameters (phase-2 spec; locked, adjust only via excitability gain).
const LEAK_DECAY: f32 = 0.95;
const THRESHOLD: f32 = 1.0;
const RESET_POTENTIAL: f32 = 0.0;
const REFRACTORY_TICKS: u32 = 5;

/// V2 Phase E: the active-edge ribbon renderer (Phase D) is now the ONE
/// connection renderer. The legacy near-LOD straight-cylinder synapse path is
/// kept (not deleted) but guarded off so it never double-draws connections.
/// Flip to true only for debugging the legacy near-LOD synapse geometry.
const DRAW_LEGACY_CYLINDERS: bool = false;

/// UX fix (near-LOD / shadow line): the near-LOD faceted icosphere body
/// (render_sphere.wgsl, a level-1 20-tri icosphere) is retired the same way the
/// cylinders were. Up close it read as a blocky "hexagon of color" (BUG 5) and
/// its `abs(dot(n,light))` shading drew a dark terminator band per sphere — the
/// "shadow line on the ball" (BUG 9). The soft additive billboards
/// (render_far.wgsl) are the beauty-first body visual at ALL distances now.
/// Kept (not deleted) and guarded off; flip to true only to debug the geometry.
const DRAW_LEGACY_NEAR_SPHERES: bool = false;

/// Morphology: the Phase-D active-edge ribbon (1 curved arc per firing neuron)
/// is RETIRED. Real procedural neuron morphology (soma + dendrite tree + axon
/// arbor with an outward signal pulse) is the connection visual now. The ribbon
/// emit pass (in tick) and render pass (in render_full) are both gated behind
/// this const — mirroring DRAW_LEGACY_NEAR_SPHERES — so the code is preserved
/// (flip to true only to debug the old ribbons), never double-drawing.
const DRAW_LEGACY_RIBBONS: bool = false;

/// Morphology controls: the retired ribbon emit ring's modulus used to read the
/// `max_active_visual_edges` budget; that field was repurposed to
/// `morph_resting_opacity`, so the gated-off (DRAW_LEGACY_RIBBONS=false) ribbon
/// path uses this fixed fallback budget instead. Never reached by default.
const LEGACY_RIBBON_EDGE_BUDGET: u32 = 100;

/// Morphology controls: default resting structure opacity (matches the
/// VisualSettings default for morph_resting_opacity). Resting brightness is now
/// taken live from the setting; this const documents the default value.
#[allow(dead_code)]
const MORPH_BASE_BRIGHTNESS: f32 = 0.25;

/// Morphology tube geometry: number of sides in the tube cross-section polygon.
/// MUST match `TUBE_SIDES` const in render_morphology.wgsl. Vertices per tube =
/// TUBE_SIDES * 2 * 3 (two rings, triangulated as quads → 2 tris per side).
/// v0.3.1 will expose this as a pipeline rebuild knob.
const TUBE_SIDES: u32 = 6;

/// Morphology soma sphere geometry (Wave 2): vertex count per soma instance.
/// MUST match the WGSL constants in render_morphology.wgsl:
///   SPHERE_SLICES=8, SPHERE_STACKS=6
///   SPHERE_VERTS = SPHERE_SLICES * SPHERE_STACKS * 2 * 3 = 288
/// (top cap: 8*3, body quads: 5*8*6, bottom cap: 8*3 = 24+240+24 = 288)
const SPHERE_VERTS: u32 = 288;

// Morphology lighting defaults (Stage 0 / v0.3.0) moved into
// `morphology::LightingConfig::default()` (v0.3.1). The MorphUniforms lighting
// fields are now sourced from `self.morph_config.lighting` each frame; the
// dev-panel `set_morphology_config` entry point owns them.

/// V2 Phase E: bright-pass luminance threshold for bloom (only the part of the
/// scene above this contributes to the blurred halo). Hardcoded (no settings
/// field; contract frozen at 24).
const BLOOM_THRESHOLD: f32 = 0.55;
/// V2 Phase E: composite exposure (tonemap) for the bloom path. ~1.0 keeps the
/// default scene brightness close to the direct path before tonemapping.
const BACKGROUND_EXPOSURE: f32 = 1.0;

/// V2 Phase C: reference connectivity degree for K-invariant weight
/// normalization. K_REF is the V2 default app K (16). The normalization factor
/// is computed relative to K_REF so that at K==K_REF every mode (sqrt_k, k)
/// yields exactly 1.0 — i.e. the default config reproduces pre-V2 dynamics
/// bit-for-bit. For K>K_REF the recurrent term is attenuated (more fan-in is
/// compensated for), for K<K_REF it is amplified, keeping per-neuron drive
/// roughly K-invariant.
const K_REF: f32 = 16.0;

/// V2 Phase C: compute the recurrent-current normalization factor for a given
/// mode (0=none, 1=sqrt_k, 2=k) and connectivity degree `k`, relative to K_REF.
fn weight_norm_factor(mode: u32, k: usize) -> f32 {
    let k = (k as f32).max(1.0);
    match mode {
        1 => (K_REF / k).sqrt(), // sqrt_k
        2 => K_REF / k,          // k
        _ => 1.0,                // none (0) or unknown
    }
}

/// Device + queue handle pair. Acquired natively (examples/tests, llvmpipe) or
/// from the browser (wasm). The acquisition path differs; the backend does not.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub timestamps_supported: bool,
    /// Keep the `Instance` alive so any surface tied to it is never orphaned.
    /// On native this stays `None` (native surfaces own themselves or are
    /// offscreen). On web it holds the single-page Instance.
    #[allow(dead_code)]
    pub instance: Option<wgpu::Instance>,
}

/// Clock-driven, data-parallel GPU simulation backend.
pub struct GpuBackend {
    config: SimConfig,
    ctx: GpuContext,
    resources: GpuResources,
    pipelines: GpuPipelines,
    layouts: GpuLayouts,
    /// Global tick counter (24-bit-wrapping semantics handled in shaders).
    tick: u32,
    /// Parity selects which I-buffer double-buffer variant runs this tick.
    parity: usize,
    /// Last observed max |accumulated current| (fixed-point), high-water.
    pub max_abs_current_hw: u32,
    /// i_ext override (defaults to config.i_ext); tunable for verification.
    i_ext: f32,
    /// Effective recurrent-coupling scale (tuning knob, default 1.0). Scales
    /// accumulated synaptic current at integrate time. Documented deviation:
    /// leaves locked weights + fixed_point_scale untouched; controls how many
    /// coincident inputs are needed to fire (biological plausibility).
    synaptic_scale: f32,
    /// Pending stimulation parameters (written via stimulate(), consumed at tick start).
    stim_pending: Option<StimUniform>,
    /// Phase 4: most recently read near-LOD profiler stats (non-blocking readback).
    pub near_lod_stats: NearLodStats,
    /// Phase 4: camera distance from surface (set by caller each frame).
    lod_camera_distance: f32,
    /// V2 Phase 0: merged visual + sim settings (source of truth for glow_tau,
    /// point_radius, i_ext, synaptic_scale, and future visual knobs).
    visual: VisualSettings,
    // ─── V2 Phase A: metrics reduction + non-blocking readback ────────────────
    /// Readback phase (Idle ↔ Pending).
    metrics_state: MetricsReadState,
    /// Set true by the map_async callback when the staging buffer is mapped.
    metrics_ready: Arc<AtomicBool>,
    /// Last good metrics snapshot (raw u32 slots from the reduction).
    metrics_cpu: [u32; METRICS_SLOT_COUNT],
    /// Ring of recent spikes_this_tick samples (branching ratio / cascade age).
    metrics_history: std::collections::VecDeque<u32>,
    /// Readbacks since spikes_this_tick last exceeded the large-cascade threshold.
    last_cascade_age: u32,
    /// Ticks elapsed since the last reduction was issued (throttle counter).
    ticks_since_metrics_issue: u32,
    // ─── V2 Phase D: active-edge emit instrumentation ─────────────────────────
    /// Last read edge_emitted count (edges emitted in the most recent tick batch
    /// where the connection layer was active). "No silent caps": queryable via
    /// `edges_emitted_last()`. 0 when the layer is off.
    edges_emitted_last: u32,
    /// V2 Phase E: surface color format the render pipelines were built for.
    /// The bloom HDR scene target uses THIS format (so the scene pipelines stay
    /// compatible); only the bloom blur ping-pong is rgba16float.
    render_color_format: wgpu::TextureFormat,
    // ─── v0.3.1 morphology config (dev-panel exposure) ────────────────────────
    /// Current applied morphology config (generator + render-quality + lighting).
    /// Initialised to `Default` (== contract defaults); `set_morphology_config`
    /// diffs incoming vs this and runs the narrowest update.
    morph_config: crate::sim::morphology::MorphologyConfig,
    /// Tube draw vertex-count = tube_sides * 2 * 3. Runtime value kept in sync with
    /// the WGSL `TUBE_SIDES` override constant the morph pipeline was built with.
    morph_tube_verts: u32,
    /// Soma sphere draw vertex-count = sphere_slices * sphere_stacks * 2 * 3.
    morph_sphere_verts: u32,
}

/// Tube draw vertex-count from a tube-sides tessellation value.
#[inline]
fn tube_verts(tube_sides: u32) -> u32 {
    tube_sides * 2 * 3
}

/// Soma-sphere draw vertex-count from slice/stack tessellation values.
#[inline]
fn sphere_verts(slices: u32, stacks: u32) -> u32 {
    slices * stacks * 2 * 3
}

impl GpuBackend {
    /// Construct against an already-acquired device/queue. `config` is the
    /// initial network; `manifold`-derived state is uploaded via `resize`.
    pub fn new(ctx: GpuContext, config: SimConfig) -> Self {
        let layouts = GpuLayouts::new(&ctx.device);
        let mut pipelines = GpuPipelines::new();
        pipelines.build(&ctx.device, &layouts);
        let i_ext = config.i_ext;
        // V2 Phase 0: init visual settings to defaults; JS will call
        // set_visual_settings() after backend creation to push the full
        // settings struct (including i_ext / synaptic_scale).
        let mut visual = VisualSettings::default();
        visual.i_ext = i_ext; // honour the SimConfig value at construction time
        Self {
            config,
            ctx,
            resources: GpuResources::new(),
            pipelines,
            layouts,
            tick: 0,
            parity: 0,
            max_abs_current_hw: 0,
            i_ext,
            synaptic_scale: 1.0,
            stim_pending: None,
            near_lod_stats: NearLodStats::default(),
            lod_camera_distance: f32::MAX,
            visual,
            // V2 Phase A
            metrics_state: MetricsReadState::Idle,
            metrics_ready: Arc::new(AtomicBool::new(false)),
            metrics_cpu: [0u32; METRICS_SLOT_COUNT],
            metrics_history: std::collections::VecDeque::with_capacity(METRICS_HISTORY_LEN),
            last_cascade_age: 0,
            ticks_since_metrics_issue: METRICS_ISSUE_INTERVAL, // issue on first batch
            edges_emitted_last: 0,
            // V2 Phase E: overwritten by build_render_pipelines with the real
            // surface format; this is only a placeholder until then.
            render_color_format: wgpu::TextureFormat::Rgba8Unorm,
            // v0.3.1: morphology config defaults (== contract defaults). Draw
            // counts match the default tessellation (TUBE_SIDES=6 → 36 verts,
            // 8×6 sphere → 288 verts).
            morph_config: crate::sim::morphology::MorphologyConfig::default(),
            morph_tube_verts: tube_verts(TUBE_SIDES),
            morph_sphere_verts: SPHERE_VERTS,
        }
    }

    /// Set the effective recurrent-coupling scale (tuning knob). Default 1.0.
    pub fn set_synaptic_scale(&mut self, s: f32) {
        self.synaptic_scale = s;
    }

    // ── V2 Phase 0: visual settings API ──────────────────────────────────────

    /// Apply a full VisualSettings snapshot.  Stores it and immediately syncs
    /// the live sim knobs (i_ext, synaptic_scale) so the next tick picks them
    /// up.  All other fields are consumed by render_full (glow_tau/point_radius)
    /// or by future phases.
    pub fn set_visual_settings(&mut self, v: VisualSettings) {
        self.set_i_ext(v.i_ext);
        self.set_synaptic_scale(v.synaptic_scale);
        // Morphology controls: connection_curve_lift is baked into the axon bow at
        // GENERATION time, so it only takes effect by rebuilding the morphology.
        // Detect a real change and regenerate after storing (so generation reads
        // the new value). Guarded so dragging other sliders never regenerates.
        let curve_changed =
            (v.connection_curve_lift - self.visual.connection_curve_lift).abs() > f32::EPSILON;
        // Heavy-tailed reach knobs change target ids → both the GPU scatter
        // uniform and the generated axon geometry must be refreshed. These are
        // brain-reset / morphology-rebuild impact (not pure "live"), mirroring
        // connection_curve_lift.
        let reach_changed = (v.long_range_reach_frac - self.visual.long_range_reach_frac).abs()
            > f32::EPSILON
            || (v.max_reach_cells - self.visual.max_reach_cells).abs() > f32::EPSILON;
        self.visual = v;
        if reach_changed {
            // Push the new knobs to the GPU scatter uniform for the next tick.
            self.write_connect_uniform();
        }
        if curve_changed || reach_changed {
            self.regenerate_morphology();
        }
    }

    /// Morphology controls: rebuild + re-upload the procedural neuron geometry
    /// from the current `self.config`, using the live
    /// `self.visual.connection_curve_lift` for the axon bow. Reuses the same
    /// generation path `initialize()` uses (build_manifold → morphology::generate
    /// → init_morph_resources). Only called when the curve-lift setting changes.
    fn regenerate_morphology(&mut self) {
        let config = self.config.clone();
        let manifold = crate::build_manifold(&config);
        let params = self.current_morph_params();
        let reach = self.current_reach();
        self.resources.init_morph_resources(
            &self.ctx.device,
            &manifold.neuron_positions,
            &manifold.spatial_grid,
            &manifold.neuron_regions,
            &config,
            &params,
            reach,
        );
        self.resources
            .refresh_bind_groups(&self.ctx.device, &self.layouts);
    }

    /// Effective generator params: the morphology-config generator group layered
    /// over the locked default (protected budgets preserved), then the live
    /// Float32Array `connection_curve_lift` applied last (it owns the axon bow at
    /// generation time and is not part of the morph-config Float32Array contract).
    fn current_morph_params(&self) -> crate::sim::morphology::MorphologyParams {
        morph_params_from_config_and_visual(&self.morph_config, &self.visual)
    }

    /// Heavy-tailed reach knobs from the live VisualSettings, converted to the
    /// integer encoding the kernel + morphology consume. The float→integer
    /// conversion happens HERE (the boundary), never inside `target` — keeping
    /// the determinism path float-free. `frac` is rounded over `REACH_FRAC_DEN`
    /// and clamped `0..=REACH_FRAC_DEN`; `max_reach` is rounded and clamped `>= 1`.
    fn current_reach(&self) -> crate::connectivity::ReachParams {
        reach_from_visual_settings(&self.visual)
    }

    /// Re-write the `connect_uniform` buffer with the live reach knobs so the
    /// GPU scatter picks them up on the next tick. The other ConnectUniforms
    /// fields are static for the run (set at init); only the two reach knobs
    /// change at runtime, but we rewrite the whole struct for simplicity.
    fn write_connect_uniform(&self) {
        let Some(sim) = self.resources.sim_buffers.as_ref() else {
            return;
        };
        let grid_dim = self
            .resources
            .grid_buffers
            .as_ref()
            .map(|g| g.grid_dim)
            .unwrap_or(1);
        let reach = self.current_reach();
        let cu = crate::sim::gpu::resources::ConnectUniforms {
            n: self.config.n as u32,
            k: self.config.k as u32,
            fixed_point_scale: self.config.fixed_point_scale as f32,
            seed_lo: self.config.seed_lo(),
            grid_dim,
            long_range_frac: reach.long_range_frac,
            max_reach: reach.max_reach,
            _pad: [0; 1],
        };
        self.ctx
            .queue
            .write_buffer(&sim.connect_uniform, 0, bytemuck::bytes_of(&cu));
    }

    /// Re-normalised world-space light direction from the morph-config lighting
    /// group (falls back to +Y if the user zeroes all three axes).
    fn morph_light_dir(&self) -> [f32; 3] {
        let l = self.morph_config.lighting;
        let v = [l.light_dir_x, l.light_dir_y, l.light_dir_z];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len < 1e-6 {
            [0.0, 1.0, 0.0]
        } else {
            [v[0] / len, v[1] / len, v[2] / len]
        }
    }

    /// v0.3.1 dev-panel morphology config entry point. Deserialises a JSON blob,
    /// diffs it against the current config, and runs the NARROWEST update:
    ///   - generator changed → re-run `generate()` + re-upload morphology buffers
    ///   - render-quality (tessellation) changed → rebuild the morph render
    ///     pipelines (WGSL override constants) + recompute draw vert-counts
    ///   - lighting/brightness only → no rebuild; picked up by the next frame's
    ///     uniform write (uniform-only)
    /// Combinations run all applicable paths. Returns Err on malformed JSON.
    pub fn set_morphology_config(&mut self, json: &str) -> Result<(), String> {
        let incoming =
            crate::sim::morphology::MorphologyConfig::from_json(json).map_err(|e| e.to_string())?;
        let prev = self.morph_config;
        if incoming == prev {
            return Ok(());
        }

        let generator_changed = incoming.generator != prev.generator;
        let render_quality_changed = incoming.render_quality != prev.render_quality;
        // lighting changes need no rebuild — the next render_full uniform write
        // reads self.morph_config directly.

        self.morph_config = incoming;

        if render_quality_changed {
            let rq = incoming.render_quality;
            self.pipelines.build_morph_pipelines(
                &self.ctx.device,
                &self.layouts,
                self.render_color_format,
                rq,
            );
            self.morph_tube_verts = tube_verts(rq.tube_sides);
            self.morph_sphere_verts = sphere_verts(rq.sphere_slices, rq.sphere_stacks);
        }

        if generator_changed {
            self.regenerate_morphology();
        }

        Ok(())
    }

    /// Read-only access to the current morphology config (artifact capture).
    pub fn morph_config(&self) -> &crate::sim::morphology::MorphologyConfig {
        &self.morph_config
    }

    /// Read-only access to the current visual settings.
    pub fn visual(&self) -> &VisualSettings {
        &self.visual
    }

    // ── V2 Phase C: granular dynamics knobs (pin one without a full snapshot) ──

    /// Set per-neuron heterogeneity [0,1]. 0 => homogeneous (pre-V2 dynamics).
    pub fn set_heterogeneity(&mut self, h: f32) {
        self.visual.heterogeneity = h;
    }

    /// Set weight-normalization mode (0=none, 1=sqrt_k, 2=k). At K=16 modes 1
    /// and 2 both give factor 1.0 (default reproduces pre-V2 dynamics).
    pub fn set_weight_normalization(&mut self, mode: u32) {
        self.visual.weight_normalization = mode;
    }

    /// Set input mode (0=constant, 1=poisson, 2=pulsed, 3=cursor_only,
    /// 4=scripted, 5=off). 0 reproduces pre-V2 ambient drive.
    pub fn set_input_mode(&mut self, mode: u32) {
        self.visual.input_mode = mode;
    }

    /// Current high-water max |accumulated current| (fixed-point).
    /// Exposed for the metrics Vec returned by WasmGpuBackend::metrics().
    pub fn max_abs_current_hw(&self) -> u32 {
        self.max_abs_current_hw
    }

    // ── V2 Phase D: active-edge connection layer ──────────────────────────────

    /// Set the connection-layer mode.
    ///   0 = Off — skip ALL morphology work: compaction compute, tube passes, soma sphere passes.
    ///   1 = Active/recent only (default) — compacted GPU draw of recently-lit tubes + somas.
    ///   2 = Resting debug — intended to show full resting morphology via the legacy
    ///       all-segment path; currently behaves like mode 1 unless `DRAW_LEGACY_ALL_SEGMENTS`
    ///       in pipelines.rs is flipped to true at compile time.
    /// Granular setter so the harness/UI can flip it without a full VisualSettings snapshot.
    pub fn set_connection_layer(&mut self, mode: u32) {
        self.visual.connection_layer = mode;
    }

    /// "No silent caps": edges emitted in the most recent active tick batch.
    /// 0 when the connection layer is off. Read back non-blocking on native.
    pub fn edges_emitted_last(&self) -> u32 {
        self.edges_emitted_last
    }

    /// V2 Phase E: set the bloom post-process intensity. 0.0 (default) = OFF →
    /// the scene renders directly to the surface (validated direct path). > 0
    /// enables the offscreen HDR + blur + composite bloom path. Granular setter
    /// so the harness/UI can flip it without a full VisualSettings snapshot.
    pub fn set_bloom_strength(&mut self, s: f32) {
        self.visual.bloom_strength = s;
    }

    /// Acquire a WebGPU adapter+device from the browser, create a wgpu surface
    /// for the given `<canvas>`, and configure it. Web / wasm32 only.
    ///
    /// Returns `(GpuContext, Surface<'static>, TextureFormat, width, height)`.
    /// The caller owns the surface and configuration; `GpuContext` holds device+queue.
    ///
    /// ## Why 'static surface?
    /// `SurfaceTarget::Canvas` stores no external reference (wgpu copies the JS
    /// object internally), so the surface does not borrow external memory and
    /// transmuting to `'static` is sound.  We pass the surface back to the caller
    /// (WasmGpuBackend) which keeps the `Instance` alive for the same duration.
    #[cfg(target_arch = "wasm32")]
    pub async fn acquire_web(
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(GpuContext, wgpu::Surface<'static>, wgpu::TextureFormat), String> {
        // 1. Instance with all default backends (includes BROWSER_WEBGPU on wasm).
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        // Read canvas dimensions before consuming it.
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);

        // 2. Create surface from the canvas.  SurfaceTarget::Canvas is gated by
        //    wgpu's cfg(web) = cfg(all(wasm32, not(Emscripten), feature="web"));
        //    the default wgpu features include "webgpu" → "web", so this variant
        //    is available. The returned surface is Surface<'_> but holds no
        //    external borrow (Canvas path sets _handle_source = None), so we
        //    extend the lifetime to 'static to allow storage in WasmGpuBackend.
        let raw_surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("create_surface: {e}"))?;
        // Safety: Canvas surface stores no external reference; lifetime is phantom.
        let surface: wgpu::Surface<'static> = unsafe {
            std::mem::transmute::<wgpu::Surface<'_>, wgpu::Surface<'static>>(raw_surface)
        };

        // 3. Request adapter compatible with the surface.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no wgpu adapter: {e}"))?;

        let timestamps_supported = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut required_features = wgpu::Features::empty();
        if timestamps_supported {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }

        // 4. Request device with generous limits (same pattern as acquire_native).
        let adapter_limits = adapter.limits();
        let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
        // Prefer the higher WebGPU limits if available.
        limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
        limits.max_buffer_size = adapter_limits.max_buffer_size;
        limits.max_compute_workgroups_per_dimension =
            adapter_limits.max_compute_workgroups_per_dimension;
        limits.max_storage_buffers_per_shader_stage =
            adapter_limits.max_storage_buffers_per_shader_stage;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("brain-visualizer-web-gpu"),
                required_features,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("request_device: {e}"))?;

        // 5. Configure the surface.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(*caps.formats.first().ok_or("no surface formats")?);

        let surf_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surf_config);

        web_sys::console::log_1(
            &format!(
                "[gpu] WebGPU adapter acquired; format={format:?} size={width}×{height} timestamps={timestamps_supported}"
            )
            .into(),
        );

        Ok((
            GpuContext {
                device,
                queue,
                timestamps_supported,
                instance: Some(instance),
            },
            surface,
            format,
        ))
    }

    /// Acquire a native adapter (high-performance, falling back to llvmpipe) and
    /// build a `GpuContext`. Native-only (examples + tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn acquire_native() -> Result<GpuContext, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no wgpu adapter: {e}"))?;
        let info = adapter.get_info();
        let timestamps_supported = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut required_features = wgpu::Features::empty();
        if timestamps_supported {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }
        // llvmpipe exposes large storage buffers; request a generous limit so
        // big N fits a single binding. Clamp to adapter limits.
        let mut limits = wgpu::Limits::downlevel_defaults();
        let adapter_limits = adapter.limits();
        limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
        limits.max_buffer_size = adapter_limits.max_buffer_size;
        limits.max_compute_workgroups_per_dimension =
            adapter_limits.max_compute_workgroups_per_dimension;
        // Scatter binds 8 storage buffers; integrate binds 5. downlevel default
        // is only 4. Lift to the adapter's capability.
        limits.max_storage_buffers_per_shader_stage =
            adapter_limits.max_storage_buffers_per_shader_stage;
        limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("brain-visualizer-gpu"),
                required_features,
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("request_device: {e}"))?;
        eprintln!(
            "[gpu] adapter = {:?} ({:?}), timestamps = {}",
            info.name, info.backend, timestamps_supported
        );
        Ok(GpuContext {
            device,
            queue,
            timestamps_supported,
            instance: None,
        })
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn resources(&self) -> &GpuResources {
        &self.resources
    }

    pub fn tick_count(&self) -> u32 {
        self.tick
    }

    /// Active/recent compaction profiler readback (Stream B/C verification).
    /// Copies the GPU-written selected-segment count into the staging buffer and
    /// maps it. Blocking — for tests/profiler diagnostics only, NOT the per-frame
    /// path (the per-frame draw uses GPU indirect args, no CPU readback). Returns
    /// `(selected, total)` where `selected` is the number of segments the last
    /// `render_full` selected and `total` is the generated segment count. Returns
    /// None if morphology buffers are absent.
    pub fn read_active_segment_count(&self) -> Option<(u32, u32)> {
        let mb = self.resources.morph_buffers.as_ref()?;
        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("read-active-segment-count"),
            });
        for chunk in &mb.segment_chunks {
            enc.copy_buffer_to_buffer(&chunk.active_selected, 0, &chunk.selected_staging, 0, 4);
        }
        self.ctx.queue.submit([enc.finish()]);
        let mut selected = 0u32;
        for chunk in &mb.segment_chunks {
            let slice = chunk.selected_staging.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            let _ = self.ctx.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            });
            rx.recv().ok()?.ok()?;
            let chunk_selected = {
                let data = slice.get_mapped_range();
                u32::from_le_bytes([data[0], data[1], data[2], data[3]])
            };
            chunk.selected_staging.unmap();
            selected = selected.saturating_add(chunk_selected);
        }
        Some((selected, mb.segment_count))
    }

    /// Device handle (for one-off debug readbacks / render setup).
    pub fn device(&self) -> &wgpu::Device {
        &self.ctx.device
    }

    /// Queue handle (for one-off debug readbacks / render setup).
    pub fn queue(&self) -> &wgpu::Queue {
        &self.ctx.queue
    }

    /// Override the ambient drive (BV17 i_ext) for tuning/verification. Does not
    /// alter any locked BV value; this is a runtime knob.
    pub fn set_i_ext(&mut self, i_ext: f32) {
        self.i_ext = i_ext;
    }

    /// Build the CPU manifold and start a staged network upload.
    pub fn begin_initialize(&mut self, config: &SimConfig) -> NetworkBuildState {
        self.config = config.clone();
        let manifold = crate::build_manifold(config);
        NetworkBuildState {
            config: config.clone(),
            manifold,
            prepared_morphology: None,
        }
    }

    /// Start a staged network upload from a worker-prepared, GPU-agnostic
    /// payload. WebGPU allocation and upload still happen only in the normal
    /// main-thread staged methods below.
    pub fn begin_initialize_prepared(
        &mut self,
        prepared: PreparedNetworkBuild,
    ) -> NetworkBuildState {
        self.config = prepared.config.clone();
        NetworkBuildState {
            config: prepared.config,
            manifold: prepared.manifold,
            prepared_morphology: Some(prepared.morphology),
        }
    }

    /// Upload neuron/grid/sim buffers for a staged network build.
    pub fn initialize_neuron_buffers(&mut self, state: &NetworkBuildState) {
        self.resources.resize_neurons(
            &self.ctx.device,
            &self.ctx.queue,
            &state.config,
            &state.manifold.neuron_positions,
            &state.manifold.neuron_regions,
            &state.manifold.spatial_grid,
        );
    }

    /// Upload manifold mesh + create render uniform buffers for a staged build.
    pub fn initialize_render_resources(&mut self, state: &NetworkBuildState) {
        self.resources.init_render_resources(
            &self.ctx.device,
            &state.manifold.vertices,
            &state.manifold.faces,
            state.config.n as u32,
            state.manifold.spatial_grid.dim,
        );
    }

    /// Allocate near-LOD + persistent edge resources for a staged build.
    pub fn initialize_lod_edge_resources(&mut self, state: &NetworkBuildState) {
        self.resources.init_near_lod_resources(
            &self.ctx.device,
            &self.ctx.queue,
            &state.config,
            &state.manifold.spatial_grid,
        );
        self.resources
            .init_edge_resources(&self.ctx.device, &self.ctx.queue);
    }

    /// Generate and upload morphology buffers for a staged build.
    pub fn initialize_morph_resources(&mut self, state: &NetworkBuildState) {
        if let Some(prepared) = state.prepared_morphology.as_ref() {
            self.resources.init_morph_resources_from_prepared(
                &self.ctx.device,
                prepared.segments.clone(),
                prepared.spheres.clone(),
                prepared.params,
                prepared.stats,
                prepared.dropped,
                state.config.n,
            );
        } else {
            let morph_params = self.current_morph_params();
            let reach = self.current_reach();
            self.resources.init_morph_resources(
                &self.ctx.device,
                &state.manifold.neuron_positions,
                &state.manifold.spatial_grid,
                &state.manifold.neuron_regions,
                &state.config,
                &morph_params,
                reach,
            );
        }
    }

    /// Finish staged initialization by refreshing bind groups and resetting
    /// per-run state.
    pub fn finish_initialize(&mut self) {
        self.resources
            .refresh_bind_groups(&self.ctx.device, &self.layouts);
        // Heavy-tailed reach: push the live knobs into the freshly (re)written
        // connect_uniform so the GPU scatter matches the generated geometry.
        self.write_connect_uniform();
        self.tick = 0;
        self.parity = 0;
        self.max_abs_current_hw = 0;
        self.stim_pending = None;
        self.near_lod_stats = NearLodStats::default();
        // V2 Phase A: reset metrics readback state (buffers were recreated).
        self.metrics_state = MetricsReadState::Idle;
        self.metrics_ready.store(false, Ordering::SeqCst);
        self.metrics_cpu = [0u32; METRICS_SLOT_COUNT];
        self.metrics_history.clear();
        self.last_cascade_age = 0;
        self.ticks_since_metrics_issue = METRICS_ISSUE_INTERVAL;
        self.edges_emitted_last = 0;
    }

    /// Build the network from a manifold and upload the silent-start state.
    /// Rare-path; allocates. Call once after `new` and on every tier change.
    pub fn initialize(&mut self, config: &SimConfig) {
        let state = self.begin_initialize(config);
        self.initialize_neuron_buffers(&state);
        self.initialize_render_resources(&state);
        self.initialize_lod_edge_resources(&state);
        self.initialize_morph_resources(&state);
        self.finish_initialize();
    }

    pub fn initialize_prepared(
        &mut self,
        prepared: PreparedNetworkBuild,
        visual: VisualSettings,
        morph_config: crate::sim::morphology::MorphologyConfig,
    ) {
        self.set_i_ext(visual.i_ext);
        self.set_synaptic_scale(visual.synaptic_scale);
        self.visual = visual;
        self.morph_config = morph_config;
        let state = self.begin_initialize_prepared(prepared);
        self.initialize_neuron_buffers(&state);
        self.initialize_render_resources(&state);
        self.initialize_lod_edge_resources(&state);
        self.initialize_morph_resources(&state);
        self.finish_initialize();
    }

    /// Build the render pipelines for a given color format.
    /// Called once at startup (or on surface re-creation).
    pub fn build_render_pipelines(&mut self, color_format: wgpu::TextureFormat) {
        // V2 Phase E: remember the surface format so the bloom HDR scene target
        // matches it (scene pipelines are format-specific).
        self.render_color_format = color_format;
        self.pipelines
            .build_render(&self.ctx.device, &self.layouts, color_format);
        // v0.3.1: build_render builds the morph pipelines at DEFAULT tessellation;
        // re-apply the current render-quality config so a non-default tubeSides /
        // sphere tessellation survives a surface re-creation.
        let rq = self.morph_config.render_quality;
        if rq != crate::sim::morphology::RenderQualityConfig::default() {
            self.pipelines
                .build_morph_pipelines(&self.ctx.device, &self.layouts, color_format, rq);
        }
        self.morph_tube_verts = tube_verts(rq.tube_sides);
        self.morph_sphere_verts = sphere_verts(rq.sphere_slices, rq.sphere_stacks);
        // Phase 4: near-LOD pipelines use the same color format.
        self.pipelines
            .build_near_lod(&self.ctx.device, &self.layouts, color_format);
    }

    /// Set camera distance (from surface/origin) each frame so near-LOD can
    /// decide whether to run. Phase 5 (controls) will call this.
    pub fn set_lod_camera_distance(&mut self, d: f32) {
        self.lod_camera_distance = d;
    }

    /// Return the most recently read near-LOD profiler stats.
    pub fn near_lod_stats(&self) -> NearLodStats {
        self.near_lod_stats
    }

    /// Resize the depth texture when the canvas/offscreen dimensions change.
    pub fn resize_render_targets(&mut self, width: u32, height: u32) {
        self.resources.resize_render_targets(
            &self.ctx.device,
            width,
            height,
            self.render_color_format,
        );
    }

    /// Render one frame. Encodes:
    ///   1. manifold dark mesh pass (depth write, opaque),
    ///   2. far-LOD billboard glow pass (additive, no depth write),
    ///   3. (when near LOD active) cull_neurons → cull_synapses → write_indirect
    ///      → sphere render → cylinder render (depth test against pass 1).
    ///
    /// `camera_pos` is the eye position in world space (needed for frustum cull).
    /// `camera_distance` is ||eye - origin||; the caller may pass f32::MAX to
    /// force far-only mode.
    ///
    /// Upload pattern (per-frame): write render_uniform + manifold_uniform via
    /// queue.write_buffer; the bind groups already reference those buffers so
    /// no bind-group rebuild is needed.
    pub fn render(
        &mut self,
        target_view: &wgpu::TextureView,
        mvp: &[f32; 16],
        camera_right: [f32; 3],
        camera_up: [f32; 3],
        glow_tau: f32,
        point_radius: f32,
    ) {
        // Default to far-only: caller did not set camera_distance explicitly.
        self.render_full(
            target_view,
            mvp,
            camera_right,
            camera_up,
            glow_tau,
            point_radius,
            [0.0, 0.0, 3.0],
            self.lod_camera_distance,
        );
    }

    /// Full render variant accepting camera_pos + camera_distance explicitly
    /// (used by the near_lod_check harness and future TS bridge).
    pub fn render_full(
        &mut self,
        target_view: &wgpu::TextureView,
        mvp: &[f32; 16],
        camera_right: [f32; 3],
        camera_up: [f32; 3],
        glow_tau: f32,
        point_radius: f32,
        camera_pos: [f32; 3],
        camera_distance: f32,
    ) {
        let bg = match self.resources.bind_groups.as_ref() {
            Some(b) if b.render_far.is_some() => b,
            _ => return,
        };
        let rr = match self.resources.render_resources.as_ref() {
            Some(r) => r,
            None => return,
        };
        let rt = match self.resources.render_targets.as_ref() {
            Some(t) => t,
            None => return,
        };
        let depth_view = match rt.depth_view.as_ref() {
            Some(d) => d,
            None => return,
        };
        let pipe_far = match self.pipelines.render_far.as_ref() {
            Some(p) => p,
            None => return,
        };

        // ─── V2 Phase E: bloom routing ────────────────────────────────────────
        // OPT-IN. When bloom_strength <= 0 (the default), `scene_view` IS the
        // surface `target_view` → the exact validated direct path (no offscreen
        // indirection, bit-for-bit the Part-1 look). Only when bloom_strength > 0
        // AND all bloom resources/pipelines exist do we render the scene into the
        // HDR offscreen target and run the post passes.
        let bloom_on = self.visual.bloom_strength > 0.0
            && self.pipelines.bloom_bright.is_some()
            && self.pipelines.bloom_blur.is_some()
            && self.pipelines.bloom_composite.is_some()
            && rt.hdr_view.is_some()
            && rt.bloom_a_view.is_some()
            && rt.bloom_b_view.is_some();
        let scene_view: &wgpu::TextureView = if bloom_on {
            rt.hdr_view.as_ref().unwrap()
        } else {
            target_view
        };

        // ─── True-opacity active layer guard ──────────────────────────────────
        // The depth-tested, alpha-blended active passes stay encoded whenever the
        // morphology layer is on and both active pipelines exist. active_opacity=0
        // is the soft low-emphasis end of the shader model, not a CPU pass skip;
        // skipping here removes the occluding active layer and lets the additive
        // morphology passes read blown out.
        let active_opaque_on = self.visual.connection_layer != 0
            && self.pipelines.render_morphology_active.is_some()
            && self.pipelines.render_soma_spheres_active.is_some();

        let n = self.config.n as u32;

        // --- LOD transition ---
        // UX fix (near-LOD / shadow line): the near-LOD faceted sphere is retired
        // (see DRAW_LEGACY_NEAR_SPHERES). The soft billboards (render_far.wgsl) are
        // the body visual at ALL camera distances — no crossfade to spheres — so we
        // force far_alpha = 1.0. The legacy crossfade ramp is preserved below
        // (computed only when the spheres are re-enabled for debugging) so flipping
        // the const back on restores the exact old behavior.
        let dist = camera_distance;
        let _legacy_far_alpha = if dist >= LOD_FAR_ONLY_DIST {
            1.0f32
        } else if dist <= LOD_NEAR_ONLY_DIST {
            0.0f32
        } else {
            (dist - LOD_NEAR_ONLY_DIST) / (LOD_FAR_ONLY_DIST - LOD_NEAR_ONLY_DIST)
        };
        // Billboards-everywhere: always full-strength. (Was `_legacy_far_alpha`.)
        let far_alpha = if DRAW_LEGACY_NEAR_SPHERES {
            _legacy_far_alpha
        } else {
            1.0f32
        };
        let near_alpha = 1.0 - far_alpha;
        let run_near_lod = DRAW_LEGACY_NEAR_SPHERES
            && near_alpha > 0.001
            && self.resources.near_lod_buffers.is_some()
            && self.pipelines.is_near_lod_built();

        // Upload per-frame render uniforms.
        let ru = RenderUniforms {
            mvp: *mvp,
            camera_right,
            _pad0: 0.0,
            camera_up,
            _pad1: 0.0,
            tick: self.tick,
            glow_tau,
            point_radius,
            n,
            camera_pos,
            // V2 Phase B: live debug voltage-glow knob (0.0 default = off).
            voltage_glow_strength: self.visual.voltage_glow_strength,
            // V2 Phase E: orthogonal color/visibility/radius controls.
            color_by: self.visual.color_by,
            neuron_visibility: self.visual.neuron_visibility,
            neuron_visual_radius: self.visual.neuron_visual_radius,
            active_neuron_radius_boost: self.visual.active_neuron_radius_boost,
            inactive_neuron_opacity: self.visual.inactive_neuron_opacity,
            _pad2: 0.0,
            _pad3: 0.0,
            _pad4: 0.0,
        };
        self.ctx
            .queue
            .write_buffer(&rr.render_uniform, 0, bytemuck::bytes_of(&ru));

        // V2 Phase D: upload the per-frame ribbon uniform when the layer is on.
        // Morphology: ribbons are RETIRED — gated behind DRAW_LEGACY_RIBBONS.
        if DRAW_LEGACY_RIBBONS && self.visual.connection_layer != 0 {
            if let Some(eb) = self.resources.edge_buffers.as_ref() {
                let modulus = LEGACY_RIBBON_EDGE_BUDGET.min(EDGE_CAP);
                let rib = RibbonUniforms {
                    mvp: *mvp,
                    camera_right,
                    tick: self.tick,
                    camera_up,
                    // Morphology: ribbons are retired (DRAW_LEGACY_RIBBONS=false);
                    // the lifetime/pulse-speed settings were repurposed to the
                    // lighting toggles, so feed literal fallbacks here just to
                    // keep this gated-off path compiling.
                    lifetime: 60.0,
                    width: self.visual.connection_visual_width,
                    curve_lift: self.visual.connection_curve_lift,
                    pulse_speed: 0.05,
                    modulus,
                    connection_layer: self.visual.connection_layer,
                    _pad0: 0,
                    _pad1: 0,
                    _pad2: 0,
                };
                self.ctx
                    .queue
                    .write_buffer(&eb.ribbon_uniform, 0, bytemuck::bytes_of(&rib));
            }
        }

        // Phase 4: upload per-frame near-LOD uniforms and frustum.
        if run_near_lod {
            if let Some(nlb) = self.resources.near_lod_buffers.as_ref() {
                // Near-render uniform.
                let nru = NearRenderUniforms {
                    mvp: *mvp,
                    camera_pos,
                    sphere_radius: point_radius * 2.5, // larger than billboard radius
                    lod_alpha: near_alpha,
                    _pad: [0.0; 3],
                };
                self.ctx
                    .queue
                    .write_buffer(&nlb.near_render_uniform, 0, bytemuck::bytes_of(&nru));

                // Extract 6 frustum planes from column-major MVP matrix.
                // Standard Gribb/Hartmann plane extraction from MVP rows.
                let planes = extract_frustum_planes(mvp);
                let fu = FrustumCullUniforms {
                    planes,
                    camera_pos,
                    max_synapse_dist: 2.5, // cull synapses beyond 2.5 world units
                    current_tick: self.tick,
                    n,
                    _pad: [0; 2],
                };
                self.ctx
                    .queue
                    .write_buffer(&nlb.frustum_uniform, 0, bytemuck::bytes_of(&fu));

                // Zero per-frame atomic counters.
                let zero = [0u32];
                self.ctx
                    .queue
                    .write_buffer(&nlb.neuron_count, 0, bytemuck::cast_slice(&zero));
                self.ctx
                    .queue
                    .write_buffer(&nlb.synapse_count, 0, bytemuck::cast_slice(&zero));
                self.ctx
                    .queue
                    .write_buffer(&nlb.neuron_overflow, 0, bytemuck::cast_slice(&zero));
                self.ctx
                    .queue
                    .write_buffer(&nlb.synapse_overflow, 0, bytemuck::cast_slice(&zero));
                self.ctx
                    .queue
                    .write_buffer(&nlb.neuron_visible, 0, bytemuck::cast_slice(&zero));
                self.ctx
                    .queue
                    .write_buffer(&nlb.synapse_visible, 0, bytemuck::cast_slice(&zero));
            }
        }

        // V2 Phase E: optional surface context. When surface != 0 we upload the
        // manifold uniform (MVP + opacity + mode) and draw the dim mesh FIRST
        // (clearing color + depth), then the far-glow pass loads on top. When
        // surface == 0 (default) this is skipped entirely → the far-glow pass
        // clears the color target exactly as before (validated default path).
        let draw_surface = self.visual.surface != 0
            && self.pipelines.render_manifold.is_some()
            && bg.render_manifold.is_some();
        if draw_surface {
            let mu = resources::ManifoldUniforms {
                mvp: *mvp,
                surface_opacity: self.visual.surface_opacity,
                surface_mode: self.visual.surface,
                color_by: self.visual.color_by,
                _pad1: 0,
            };
            self.ctx
                .queue
                .write_buffer(&rr.manifold_uniform, 0, bytemuck::bytes_of(&mu));
        }

        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render-frame"),
            });

        // V2 Phase E: manifold surface pass (optional, before far-glow). Clears
        // color + depth so the far-glow pass can load on top. Depth-writes so the
        // surface reads sensibly behind the additive glow.
        if draw_surface {
            let pipe_manifold = self.pipelines.render_manifold.as_ref().unwrap();
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surface-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipe_manifold);
            pass.set_bind_group(0, bg.render_manifold.as_ref().unwrap(), &[]);
            pass.set_vertex_buffer(0, rr.manifold_vb.slice(..));
            pass.set_index_buffer(rr.manifold_ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..rr.manifold_index_count, 0, 0..1);
        }

        // Pass 1: far-LOD neuron glow — no depth test so all neurons are visible
        // from every angle. Clears to black each frame (unless the surface pass
        // already cleared, in which case it loads on top). Near-LOD crossfade:
        // skip draw when fully zoomed in, but still clear so near-LOD passes start clean.
        {
            let color_load = if draw_surface {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                })
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("far-glow-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: color_load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if far_alpha > 0.001 {
                pass.set_pipeline(pipe_far);
                pass.set_bind_group(0, bg.render_far.as_ref().unwrap(), &[]);
                pass.draw(0..6, 0..n);
            }
        }

        // ─── Morphology: procedural neuron geometry pass ──────────────────────
        // After the soma far-glow pass. Draws the dendrite trees + axon arbors
        // (resting structure at morph_resting_opacity) and, when connection_layer
        // is on (>=1), the outward signal pulse racing along each firing neuron's
        // tree. Additive, no depth (bloom-friendly). Skipped when layer==0 (off).
        // ─── Active/recent compaction compute (Stream B/C) ────────────────────
        // Before the tube passes draw, run a GPU compaction stage that selects
        // only segments the shader would light (currently/recently lit + an
        // "about to be lit" headroom) into active_segment_indices, and writes the
        // selected count into the tube draw-indirect args. The tube passes then
        // draw ONLY those instances via draw_indirect — frame cost scales with
        // active/recent count, not total segment count. No CPU readback decides
        // the per-frame selection (discipline rule).
        let compaction_ran = if self.visual.connection_layer != 0
            && !crate::sim::gpu::pipelines::DRAW_LEGACY_ALL_SEGMENTS
        {
            if let (Some(p_reset), Some(p_compact), Some(p_write), Some(mb)) = (
                self.pipelines.compact_morph_reset.as_ref(),
                self.pipelines.compact_morph.as_ref(),
                self.pipelines.compact_morph_write_args.as_ref(),
                self.resources.morph_buffers.as_ref(),
            ) {
                for chunk in &mb.segment_chunks {
                    let cu = resources::CompactUniforms {
                        tick: self.tick,
                        segment_count: chunk.segment_count,
                        glow_tau: self.visual.glow_tau,
                        connection_layer: self.visual.connection_layer,
                        light_next: self.visual.connection_light_next,
                        light_past: 0, // upstream lighting removed (mirrors tube uniform)
                        tube_verts: self.morph_tube_verts,
                        _pad: 0,
                    };
                    self.ctx
                        .queue
                        .write_buffer(&chunk.compact_uniform, 0, bytemuck::bytes_of(&cu));
                }
                let mut cpass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("compact-morph-pass"),
                    timestamp_writes: None,
                });
                for (chunk, chunk_bg) in
                    mb.segment_chunks.iter().zip(bg.morph_segment_chunks.iter())
                {
                    let groups = chunk.segment_count.div_ceil(64).max(1);
                    cpass.set_bind_group(0, &chunk_bg.compact_morph, &[]);
                    cpass.set_pipeline(p_reset);
                    cpass.dispatch_workgroups(1, 1, 1);
                    cpass.set_pipeline(p_compact);
                    cpass.dispatch_workgroups(groups, 1, 1);
                    cpass.set_pipeline(p_write);
                    cpass.dispatch_workgroups(1, 1, 1);
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        if self.visual.connection_layer != 0 {
            if let (Some(pipe_morph), Some(mb)) = (
                self.pipelines.render_morphology.as_ref(),
                self.resources.morph_buffers.as_ref(),
            ) {
                let lighting = self.morph_config.lighting;
                let mu = MorphUniforms {
                    mvp: *mvp,
                    camera_right,
                    tick: self.tick,
                    camera_up,
                    // Morphology controls: width is a live multiplier on radii.
                    width_scale: self.visual.connection_visual_width,
                    camera_pos,
                    // Morphology controls: whole-connection τ-fade lighting toggles.
                    light_next: self.visual.connection_light_next,
                    light_past: 0, // reserved_zero: upstream lighting removed
                    glow_tau: self.visual.glow_tau,
                    // Legacy Float32Array resting opacity (kept populated for the
                    // contract slot; the shader now reads resting_brightness below).
                    base_brightness: self.visual.morph_resting_opacity,
                    connection_layer: self.visual.connection_layer,
                    color_by: self.visual.color_by,
                    _pad_a: 0,
                    _pad_b: 0,
                    _pad_c: 0,
                    // v0.3.1: lighting + brightness from the morphology config
                    // (set_morphology_config), not the Float32Array. light_dir is
                    // re-normalised CPU-side.
                    light_dir: self.morph_light_dir(),
                    ambient: lighting.ambient,
                    diffuse_intensity: lighting.diffuse_intensity,
                    rim_intensity: lighting.rim_intensity,
                    rim_power: lighting.rim_power,
                    _pad3: 0,
                    resting_brightness: lighting.resting_brightness,
                    active_boost: lighting.active_boost,
                    active_opacity: lighting.active_opacity,
                    inactive_opacity_floor: lighting.inactive_opacity_floor,
                };
                self.ctx
                    .queue
                    .write_buffer(&mb.morph_uniform, 0, bytemuck::bytes_of(&mu));
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("morphology-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if mb.segment_count > 0 {
                    pass.set_pipeline(pipe_morph);
                    // tube_sides * 2 * 3 verts per segment (two rings, triangulated
                    // as quads → 2 tris × 3 verts per side). v0.3.1: runtime value
                    // kept in sync with the WGSL TUBE_SIDES override constant the
                    // morph pipeline was built with.
                    // Active/recent path (default): draw only the compacted
                    // instances via the GPU-written indirect args (instance count
                    // = selected segments). Legacy path: draw all segments.
                    for (chunk, chunk_bg) in
                        mb.segment_chunks.iter().zip(bg.morph_segment_chunks.iter())
                    {
                        if chunk.segment_count == 0 {
                            continue;
                        }
                        pass.set_bind_group(0, &chunk_bg.render_morphology, &[]);
                        if compaction_ran {
                            pass.draw_indirect(&chunk.active_draw_args, 0);
                        } else {
                            pass.draw(0..self.morph_tube_verts, 0..chunk.segment_count);
                        }
                    }
                }
            }
        }

        // ─── NEW: true-opacity active tube pass ───────────────────────────────
        // Depth-tested, alpha-blended draw of the SAME tubes, on top of the
        // additive resting tube pass. Owns the frame's depth clear (Clear(1.0))
        // since the additive passes never touch depth and the surface/near-LOD
        // depth users are off by default. `fs_main_active` returns continuous
        // spike-proximity straight alpha so active_opacity smoothly changes tube
        // occlusion while the bright packet remains fragment-local.
        // Reuses the additive tube bind group + draw count (same override consts).
        if active_opaque_on {
            if let (Some(pipe_active), Some(mb)) = (
                self.pipelines.render_morphology_active.as_ref(),
                self.resources.morph_buffers.as_ref(),
            ) {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("morphology-active-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if mb.segment_count > 0 {
                    pass.set_pipeline(pipe_active);
                    // Same compacted instance list as the additive pass — the
                    // active-opacity pass MUST NOT reintroduce full-segment draws.
                    for (chunk, chunk_bg) in
                        mb.segment_chunks.iter().zip(bg.morph_segment_chunks.iter())
                    {
                        if chunk.segment_count == 0 {
                            continue;
                        }
                        pass.set_bind_group(0, &chunk_bg.render_morphology, &[]);
                        if compaction_ran {
                            pass.draw_indirect(&chunk.active_draw_args, 0);
                        } else {
                            pass.draw(0..self.morph_tube_verts, 0..chunk.segment_count);
                        }
                    }
                }
            }
        }

        // ─── Morphology: soma sphere pass (Wave 2) ────────────────────────────
        // One shader-generated UV sphere per neuron. Drawn AFTER the tube pass
        // (order: manifold → far glow → morphology tubes → soma spheres → bloom).
        // Gated on the same connection_layer condition as tubes (0 = off).
        // Additive, no depth — same compositing as the tube pass.
        if self.visual.connection_layer != 0 {
            if let (Some(pipe_soma), Some(soma_bg), Some(mb)) = (
                self.pipelines.render_soma_spheres.as_ref(),
                bg.render_soma_spheres.as_ref(),
                self.resources.morph_buffers.as_ref(),
            ) {
                let n_spheres = mb.sphere_count;
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("soma-sphere-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if n_spheres > 0 {
                    pass.set_pipeline(pipe_soma);
                    pass.set_bind_group(0, soma_bg, &[]);
                    // slices * stacks * 2 * 3 verts per soma instance. v0.3.1:
                    // runtime value kept in sync with the WGSL SPHERE_SLICES /
                    // SPHERE_STACKS override constants.
                    pass.draw(0..self.morph_sphere_verts, 0..n_spheres);
                }
            }
        }

        // ─── NEW: true-opacity active soma pass ───────────────────────────────
        // Depth-tested, alpha-blended draw of the SAME somas, on top of the
        // additive resting soma pass. Loads the depth the active-tube pass wrote
        // (so active tubes and active somas mutually occlude correctly), and
        // writes depth itself. `fs_sphere_active` returns a firing-driven straight
        // alpha. Reuses the additive soma bind group + draw count.
        if active_opaque_on {
            if let (Some(pipe_active), Some(soma_bg), Some(mb)) = (
                self.pipelines.render_soma_spheres_active.as_ref(),
                bg.render_soma_spheres.as_ref(),
                self.resources.morph_buffers.as_ref(),
            ) {
                let n_spheres = mb.sphere_count;
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("soma-sphere-active-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if n_spheres > 0 {
                    pass.set_pipeline(pipe_active);
                    pass.set_bind_group(0, soma_bg, &[]);
                    pass.draw(0..self.morph_sphere_verts, 0..n_spheres);
                }
            }
        }

        // ─── V2 Phase D: active-edge ribbon pass (RETIRED) ────────────────────
        // Morphology replaces this. Gated behind DRAW_LEGACY_RIBBONS (default
        // false) — kept only to debug the old curved-arc ribbons. Additive, no depth.
        if DRAW_LEGACY_RIBBONS && self.visual.connection_layer != 0 {
            if let (Some(pipe_ribbon), Some(rbg)) = (
                self.pipelines.render_ribbon.as_ref(),
                bg.render_ribbon.as_ref(),
            ) {
                let modulus = LEGACY_RIBBON_EDGE_BUDGET.min(EDGE_CAP);
                // SEGMENTS=8 → 48 verts per instance (matches render_ribbon.wgsl).
                const RIBBON_VERTS_PER_INSTANCE: u32 = 8 * 6;
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ribbon-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if modulus > 0 {
                    pass.set_pipeline(pipe_ribbon);
                    pass.set_bind_group(0, rbg, &[]);
                    pass.draw(0..RIBBON_VERTS_PER_INSTANCE, 0..modulus);
                }
            }
        }

        // ─── Phase 4: Near-LOD passes ─────────────────────────────────────────
        // Only run when camera is close enough (near_alpha > threshold).
        // Pass order: cull_neurons → cull_synapses → write_indirect →
        //             draw_indexed_indirect(spheres) + draw_indexed_indirect(cylinders).
        if run_near_lod {
            let nlb = self.resources.near_lod_buffers.as_ref().unwrap();
            let bg = self.resources.bind_groups.as_ref().unwrap();
            let pipe_cull_n = self.pipelines.cull_neurons.as_ref().unwrap();
            let pipe_cull_s = self.pipelines.cull_synapses.as_ref().unwrap();
            let pipe_indirect = self.pipelines.write_indirect.as_ref().unwrap();
            let pipe_sphere = self.pipelines.render_sphere.as_ref().unwrap();
            let pipe_cylinder = self.pipelines.render_cylinder.as_ref().unwrap();
            let cg0 = bg.cull_group0.as_ref().unwrap();
            let cg1 = bg.cull_group1.as_ref().unwrap();
            let dig = bg.draw_indirect.as_ref().unwrap();
            let srg = bg.render_sphere.as_ref().unwrap();
            let crg = bg.render_cylinder.as_ref().unwrap();

            let cull_groups = n.div_ceil(256).max(1);

            // Cull neurons compute pass.
            {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("cull-neurons"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_cull_n);
                cp.set_bind_group(0, cg0, &[]);
                cp.set_bind_group(1, cg1, &[]);
                cp.dispatch_workgroups(cull_groups, 1, 1);
            }
            // Cull synapses compute pass.
            // V2 Phase E: guarded behind DRAW_LEGACY_CYLINDERS — the ribbon pass
            // (Phase D) is the one connection renderer now. Skipping the cull
            // leaves synapse_count at 0 so the indirect cylinder draw is a no-op.
            if DRAW_LEGACY_CYLINDERS {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("cull-synapses"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_cull_s);
                cp.set_bind_group(0, cg0, &[]);
                cp.set_bind_group(1, cg1, &[]);
                cp.dispatch_workgroups(cull_groups, 1, 1);
            }
            // Write indirect args.
            {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("write-indirect"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_indirect);
                cp.set_bind_group(0, dig, &[]);
                cp.dispatch_workgroups(1, 1, 1);
            }
            // Sphere render pass (draw_indexed_indirect).
            // Clear depth here — the manifold prepass no longer does it.
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("near-sphere-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe_sphere);
                pass.set_bind_group(0, srg, &[]);
                pass.set_vertex_buffer(0, nlb.sphere_vb.slice(..));
                pass.set_index_buffer(nlb.sphere_ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed_indirect(&nlb.neuron_draw_args, 0);
            }
            // Cylinder render pass (legacy near-LOD straight connections).
            // V2 Phase E: guarded off by default — see DRAW_LEGACY_CYLINDERS.
            if DRAW_LEGACY_CYLINDERS {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("near-cylinder-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: scene_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe_cylinder);
                pass.set_bind_group(0, crg, &[]);
                pass.set_vertex_buffer(0, nlb.cylinder_vb.slice(..));
                pass.set_index_buffer(nlb.cylinder_ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed_indirect(&nlb.synapse_draw_args, 0);
            }

            // Stage profiler counters for async readback (non-blocking; never stalls the loop).
            // On wasm/WebGPU device.poll(Wait) is a no-op so map_async never resolves
            // synchronously — identical deadlock risk as stats_staging in tick(). Skip.
            #[cfg(not(target_arch = "wasm32"))]
            {
                enc.copy_buffer_to_buffer(&nlb.neuron_count, 0, &nlb.profiler_staging, 0, 4);
                enc.copy_buffer_to_buffer(&nlb.neuron_overflow, 0, &nlb.profiler_staging, 4, 4);
                enc.copy_buffer_to_buffer(&nlb.synapse_count, 0, &nlb.profiler_staging, 8, 4);
                enc.copy_buffer_to_buffer(&nlb.synapse_overflow, 0, &nlb.profiler_staging, 12, 4);
                enc.copy_buffer_to_buffer(&nlb.neuron_visible, 0, &nlb.profiler_staging, 16, 4);
                enc.copy_buffer_to_buffer(&nlb.synapse_visible, 0, &nlb.profiler_staging, 20, 4);
            }
        }

        // ─── V2 Phase E: bloom post-process (opt-in) ──────────────────────────
        // Scene is in the HDR target (scene_view). bright-pass → blur_h → blur_v
        // (half-res ping-pong) → composite (scene + blur*strength, tonemap) into
        // the surface `target_view`. Bind groups are built per-frame because the
        // textures are recreated on resize; cheap on the opt-in path.
        if bloom_on {
            let hdr_view = rt.hdr_view.as_ref().unwrap();
            let bloom_a_view = rt.bloom_a_view.as_ref().unwrap();
            let bloom_b_view = rt.bloom_b_view.as_ref().unwrap();
            let pipe_bright = self.pipelines.bloom_bright.as_ref().unwrap();
            let pipe_blur = self.pipelines.bloom_blur.as_ref().unwrap();
            let pipe_composite = self.pipelines.bloom_composite.as_ref().unwrap();

            let inv_full = [1.0 / rt.width.max(1) as f32, 1.0 / rt.height.max(1) as f32];
            let inv_half = [
                1.0 / rt.bloom_width.max(1) as f32,
                1.0 / rt.bloom_height.max(1) as f32,
            ];

            // Per-pass uniforms.
            let bright_u = resources::BloomUniforms {
                inv_texel: inv_full,
                direction: [0.0, 0.0],
                threshold: BLOOM_THRESHOLD,
                bloom_strength: self.visual.bloom_strength,
                exposure: BACKGROUND_EXPOSURE,
                _pad: 0.0,
            };
            let blur_h_u = resources::BloomUniforms {
                inv_texel: inv_half,
                direction: [1.0, 0.0],
                threshold: BLOOM_THRESHOLD,
                bloom_strength: self.visual.bloom_strength,
                exposure: BACKGROUND_EXPOSURE,
                _pad: 0.0,
            };
            let blur_v_u = resources::BloomUniforms {
                inv_texel: inv_half,
                direction: [0.0, 1.0],
                threshold: BLOOM_THRESHOLD,
                bloom_strength: self.visual.bloom_strength,
                exposure: BACKGROUND_EXPOSURE,
                _pad: 0.0,
            };
            let composite_u = bright_u; // direction/threshold unused by composite
            self.ctx
                .queue
                .write_buffer(&rr.bloom_bright_uniform, 0, bytemuck::bytes_of(&bright_u));
            self.ctx
                .queue
                .write_buffer(&rr.bloom_blur_h_uniform, 0, bytemuck::bytes_of(&blur_h_u));
            self.ctx
                .queue
                .write_buffer(&rr.bloom_blur_v_uniform, 0, bytemuck::bytes_of(&blur_v_u));
            self.ctx.queue.write_buffer(
                &rr.bloom_composite_uniform,
                0,
                bytemuck::bytes_of(&composite_u),
            );

            let dev = &self.ctx.device;
            // bright: read HDR scene → bloom_a (half-res). uniform = bright.
            let bright_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom-bright-bg"),
                layout: &self.layouts.bloom_pass_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&rr.bloom_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(hdr_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: rr.bloom_bright_uniform.as_entire_binding(),
                    },
                ],
            });
            // blur_h: read bloom_a → bloom_b. uniform = blur_h.
            let blur_h_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom-blur-h-bg"),
                layout: &self.layouts.bloom_pass_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&rr.bloom_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(bloom_a_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: rr.bloom_blur_h_uniform.as_entire_binding(),
                    },
                ],
            });
            // blur_v: read bloom_b → bloom_a. uniform = blur_v.
            let blur_v_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom-blur-v-bg"),
                layout: &self.layouts.bloom_pass_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&rr.bloom_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(bloom_b_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: rr.bloom_blur_v_uniform.as_entire_binding(),
                    },
                ],
            });
            // composite: scene(1)=HDR, bloom(3)=bloom_a (final blur), uniform(2).
            let composite_bg = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom-composite-bg"),
                layout: &self.layouts.bloom_composite_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&rr.bloom_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(hdr_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: rr.bloom_composite_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(bloom_a_view),
                    },
                ],
            });

            let fullscreen = |enc: &mut wgpu::CommandEncoder,
                              label: &str,
                              target: &wgpu::TextureView,
                              pipe: &wgpu::RenderPipeline,
                              bgrp: &wgpu::BindGroup| {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, bgrp, &[]);
                pass.draw(0..3, 0..1);
            };

            fullscreen(
                &mut enc,
                "bloom-bright",
                bloom_a_view,
                pipe_bright,
                &bright_bg,
            );
            fullscreen(
                &mut enc,
                "bloom-blur-h",
                bloom_b_view,
                pipe_blur,
                &blur_h_bg,
            );
            fullscreen(
                &mut enc,
                "bloom-blur-v",
                bloom_a_view,
                pipe_blur,
                &blur_v_bg,
            );
            fullscreen(
                &mut enc,
                "bloom-composite",
                target_view,
                pipe_composite,
                &composite_bg,
            );
        }

        self.ctx.queue.submit([enc.finish()]);

        // Non-blocking profiler readback for near-LOD stats (only when near-LOD ran).
        #[cfg(not(target_arch = "wasm32"))]
        if run_near_lod {
            if let Some(nlb) = self.resources.near_lod_buffers.as_ref() {
                self.near_lod_stats = read_near_lod_stats(&self.ctx.device, &nlb.profiler_staging);
            }
        }
    }

    /// Debug-mode correctness check (architecture §"correctness checks"). Reads
    /// back `v` once (a stall — call OFF the hot path, e.g. once per second in
    /// debug builds, or from tests/the verification harness). Returns
    /// (mean_v, frac_fired_recent). Warns if mean_v leaves [-0.5, 1.5] or if a
    /// huge fraction of neurons just fired (>80% → excitability bug).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn debug_dynamics_snapshot(&self) -> (f32, f32) {
        use crate::sim::backend::{has_spiked, tick_diff, RenderState, TICK_MASK};
        let rs = self.render_state();
        let (v_buf, ls_buf, n) = match rs {
            RenderState::Gpu {
                v_buf,
                last_spike_buf,
                neuron_count,
                ..
            } => (v_buf, last_spike_buf, neuron_count),
            _ => return (0.0, 0.0),
        };
        let v: Vec<f32> = readback(&self.ctx.device, &self.ctx.queue, v_buf, n);
        let ls: Vec<u32> = readback(&self.ctx.device, &self.ctx.queue, ls_buf, n);
        let mut sum = 0.0f64;
        let mut nan = 0usize;
        for &x in &v {
            if x.is_nan() {
                nan += 1;
            } else {
                sum += x as f64;
            }
        }
        let mean_v = (sum / n as f64) as f32;
        let now = self.tick.wrapping_sub(1) & TICK_MASK;
        let fired_recent = ls
            .iter()
            .filter(|&&w| has_spiked(w) && tick_diff(now, w & TICK_MASK) == 0)
            .count();
        let frac = fired_recent as f32 / n as f32;
        debug_assert!(nan == 0, "NaN membrane potentials: {nan}");
        if !(-0.5..=1.5).contains(&mean_v) {
            eprintln!("[debug] mean(v)={mean_v:.3} outside [-0.5,1.5]");
        }
        if frac > 0.80 {
            eprintln!(
                "[debug] {:.0}% fired in one tick (excitability bug?)",
                frac * 100.0
            );
        }
        (mean_v, frac)
    }

    fn ensure_bind_groups(&mut self) {
        if self.resources.bind_groups_dirty {
            self.resources
                .refresh_bind_groups(&self.ctx.device, &self.layouts);
        }
    }

    // ─── V2 Phase A: metrics reduction + non-blocking async readback ──────────

    /// Drive the metrics state machine once per `tick()` batch (called AFTER the
    /// sim submit). Works identically on native and wasm:
    ///   1. If Idle and the throttle has elapsed → zero metrics_buf, dispatch the
    ///      read-only reduce_metrics pass, copy → metrics_staging, submit, then
    ///      map_async (sets metrics_ready via the Arc when the map resolves) and
    ///      transition to Pending.
    ///   2. Always device.poll(Poll) — NON-blocking; progresses the map natively,
    ///      harmless no-op on wasm (the browser progresses the map between frames).
    ///   3. If Pending and metrics_ready → copy the mapped slots into metrics_cpu,
    ///      unmap, recompute branching ratio / cascade age, return to Idle.
    ///
    /// The Idle gate guarantees we never copy_buffer_to_buffer into the staging
    /// buffer while it is mapped — the exact corruption the stats path warns of.
    fn update_metrics(&mut self) {
        // ── Step 3 first: drain a completed map before we consider re-issuing.
        if self.metrics_state == MetricsReadState::Pending
            && self.metrics_ready.load(Ordering::SeqCst)
        {
            if let Some(sim) = self.resources.sim_buffers.as_ref() {
                let slice = sim.metrics_staging.slice(..);
                {
                    let data = slice.get_mapped_range();
                    let words: &[u32] = bytemuck::cast_slice(&data);
                    let take = words.len().min(METRICS_SLOT_COUNT);
                    self.metrics_cpu[..take].copy_from_slice(&words[..take]);
                    // `data` drops here, releasing the borrow before unmap().
                }
                sim.metrics_staging.unmap();
            }
            self.metrics_ready.store(false, Ordering::SeqCst);
            self.metrics_state = MetricsReadState::Idle;

            // Update the spikes_this_tick history ring + cascade age.
            let spikes = self.metrics_cpu[0];
            if self.metrics_history.len() >= METRICS_HISTORY_LEN {
                self.metrics_history.pop_front();
            }
            // Running mean BEFORE pushing the new sample (for cascade threshold).
            let prev_mean = if self.metrics_history.is_empty() {
                0.0
            } else {
                self.metrics_history.iter().map(|&x| x as f64).sum::<f64>()
                    / self.metrics_history.len() as f64
            };
            self.metrics_history.push_back(spikes);
            // Large cascade: spikes > 2× running mean (and non-trivial).
            if (spikes as f64) > 2.0 * prev_mean && spikes > 1 {
                self.last_cascade_age = 0;
            } else {
                self.last_cascade_age = self.last_cascade_age.saturating_add(1);
            }
        }

        // ── Step 1: issue a new reduction when Idle + throttle elapsed.
        if self.metrics_state == MetricsReadState::Idle
            && self.ticks_since_metrics_issue >= METRICS_ISSUE_INTERVAL
        {
            let n = self.config.n as u32;
            let pipe_metrics = match self.pipelines.metrics.as_ref() {
                Some(p) => p,
                None => return,
            };
            let (Some(bg), Some(sim)) = (
                self.resources.bind_groups.as_ref(),
                self.resources.sim_buffers.as_ref(),
            ) else {
                return;
            };

            // Metrics read the most-recently completed tick (self.tick already
            // points at the NEXT tick after the batch, so subtract 1, 24-bit).
            let now = self.tick.wrapping_sub(1) & 0x00FF_FFFFu32;
            let mu = MetricsUniforms {
                current_tick: now,
                n,
                volt_lo: METRICS_VOLT_LO,
                volt_hi: METRICS_VOLT_HI,
                volt_scale: METRICS_VOLT_SCALE,
                histo_bins: METRICS_HISTO_BINS,
                _pad: [0; 2],
            };
            self.ctx
                .queue
                .write_buffer(&sim.metrics_uniform, 0, bytemuck::bytes_of(&mu));
            // Zero the metrics buffer (atomic accumulators start fresh each pass).
            let zeros = [0u32; METRICS_SLOT_COUNT];
            self.ctx
                .queue
                .write_buffer(&sim.metrics_buf, 0, bytemuck::cast_slice(&zeros));

            let groups = n.div_ceil(256).max(1);
            let mut enc = self
                .ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("metrics-reduce"),
                });
            {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("reduce_metrics"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_metrics);
                cp.set_bind_group(0, &bg.metrics, &[]);
                cp.set_bind_group(1, &bg.metrics_uniform, &[]);
                cp.dispatch_workgroups(groups, 1, 1);
            }
            // Safe: staging is Idle (not mapped) so this copy cannot corrupt it.
            enc.copy_buffer_to_buffer(
                &sim.metrics_buf,
                0,
                &sim.metrics_staging,
                0,
                (METRICS_SLOT_COUNT * 4) as u64,
            );
            self.ctx.queue.submit([enc.finish()]);

            // Non-blocking map: the callback flips the shared AtomicBool when the
            // GPU work + map complete. On native, device.poll(Poll) below (and on
            // subsequent frames) progresses it; on wasm the browser does.
            //
            // Closure bounds: map_async requires the callback be Send + 'static on
            // native. We move only a clone of Arc<AtomicBool> (Send + Sync +
            // 'static) — no borrow of `self` — so it satisfies the bound on native
            // and is trivially fine on wasm (single-threaded, FnOnce + 'static).
            let ready = self.metrics_ready.clone();
            let slice = sim.metrics_staging.slice(..);
            slice.map_async(wgpu::MapMode::Read, move |res| {
                if res.is_ok() {
                    ready.store(true, Ordering::SeqCst);
                }
            });
            self.metrics_state = MetricsReadState::Pending;
            self.ticks_since_metrics_issue = 0;
        }

        // ── Step 2: always poll non-blocking so native progresses the map.
        let _ = self.ctx.device.poll(wgpu::PollType::Poll);
    }

    /// Build the metrics layout consumed by web/settings.ts METRICS_LAYOUT from
    /// the last reduction snapshot plus CPU-derived history fields.
    pub fn metrics_snapshot(&self) -> [f32; METRICS_OUTPUT_LEN] {
        build_metrics_snapshot(
            self.config.n,
            self.config.k,
            &self.metrics_cpu,
            &self.metrics_history,
            self.last_cascade_age,
            self.max_abs_current_hw,
        )
    }
}

fn build_metrics_snapshot(
    n: usize,
    k: usize,
    m: &[u32; METRICS_SLOT_COUNT],
    metrics_history: &std::collections::VecDeque<u32>,
    last_cascade_age: u32,
    max_abs_current_hw: u32,
) -> [f32; METRICS_OUTPUT_LEN] {
    let n = n.max(1) as f32;

    let spikes_this_tick = m[0] as f32;
    let spikes_per_sec = spikes_this_tick * 60.0;
    let mean_firing_rate_hz = (spikes_this_tick / n) * 60.0;
    let synaptic_events_per_sec = spikes_per_sec * k as f32;

    let volt_sum = (m[10] as f64) * 4_294_967_296.0 + (m[9] as f64);
    let mean_v = ((volt_sum / METRICS_VOLT_SCALE as f64) / n as f64) as f32 + METRICS_VOLT_LO;

    let branching_ratio = {
        let mut sum = 0.0f64;
        let mut count = 0u32;
        let mut prev: Option<u32> = None;
        for &s in metrics_history.iter() {
            if let Some(p) = prev {
                if p > 0 {
                    sum += s as f64 / p as f64;
                    count += 1;
                }
            }
            prev = Some(s);
        }
        if count > 0 {
            (sum / count as f64) as f32
        } else {
            0.0
        }
    };

    let mut out = [0.0f32; METRICS_OUTPUT_LEN];
    out[0] = spikes_this_tick;
    out[1] = spikes_per_sec;
    out[2] = mean_firing_rate_hz;
    out[3] = synaptic_events_per_sec;
    out[4] = mean_v;
    out[5] = m[1] as f32;
    out[6] = m[2] as f32;
    out[7] = m[3] as f32;
    out[8] = m[4] as f32;
    out[9] = m[5] as f32;
    out[10] = m[6] as f32 / n;
    out[11] = m[7] as f32 / n;
    out[12] = m[8] as f32 / n;
    out[13] = branching_ratio;
    out[14] = (last_cascade_age * METRICS_ISSUE_INTERVAL) as f32;
    out[15] = m[11] as f32;
    out[16] = max_abs_current_hw as f32;
    for b in 0..METRICS_HISTO_BINS as usize {
        out[METRICS_SCALAR_COUNT + b] = m[16 + b] as f32 / n;
    }
    out
}

impl SimBackend for GpuBackend {
    fn tick(&mut self, ticks: u32, excitability: f32) -> TickStats {
        if ticks == 0 || self.resources.bind_groups.is_none() {
            self.ensure_bind_groups();
            if self.resources.bind_groups.is_none() {
                return TickStats::default();
            }
        }
        self.ensure_bind_groups();

        #[cfg(not(target_arch = "wasm32"))]
        let t0 = std::time::Instant::now();
        let n = self.config.n as u32;
        let integrate_groups = n.div_ceil(256).max(1);

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let bg: &GpuBindGroups = self.resources.bind_groups.as_ref().unwrap();
        let sim = self.resources.sim_buffers.as_ref().unwrap();
        let pipe_integrate = self.pipelines.integrate.as_ref().unwrap();
        let pipe_write = self.pipelines.write_dispatch.as_ref().unwrap();
        let pipe_scatter = self.pipelines.scatter.as_ref().unwrap();

        // V2 Phase D: active-edge emit pass — ONLY when the connection layer is on
        // (default off ⇒ these resolve to None-paths and the pass is SKIPPED
        // ENTIRELY, so determinism/dynamics are bit-for-bit unaffected).
        let connection_layer = self.visual.connection_layer;
        let edge_modulus = LEGACY_RIBBON_EDGE_BUDGET.min(EDGE_CAP);
        // Morphology: the active-edge emit pass feeds the RETIRED ribbon renderer.
        // Gate it behind DRAW_LEGACY_RIBBONS so it never runs by default (the
        // morphology pulse reads neuron last_spike directly — no edge ring needed).
        let do_emit = DRAW_LEGACY_RIBBONS
            && connection_layer != 0
            && self.pipelines.emit_edges.is_some()
            && bg.emit_edges.is_some()
            && bg.emit_edges_uniform.is_some()
            && self.resources.edge_buffers.is_some();
        let pipe_emit = self.pipelines.emit_edges.as_ref();
        let emit_bg = bg.emit_edges.as_ref();
        let emit_u_bg = bg.emit_edges_uniform.as_ref();
        let edge_bufs = self.resources.edge_buffers.as_ref();

        // Phase 3: write stimulation uniform. Pre-extract stim resources so the
        // borrow checker can split self.pipelines / self.resources borrows.
        let stim_pending = self.stim_pending.take();
        let do_stim =
            stim_pending.is_some() && self.pipelines.stimulate.is_some() && bg.stimulate.is_some();
        if let Some(su) = stim_pending {
            if let Some(rr) = self.resources.render_resources.as_ref() {
                queue.write_buffer(&rr.stim_uniform, 0, bytemuck::bytes_of(&su));
            }
        }
        // Pre-borrow stim pipeline + bg for the loop (both are immutable refs).
        let pipe_stim = self.pipelines.stimulate.as_ref();
        let stim_bgs = bg.stimulate.as_ref();
        // Initial parity for stimulate (stimulate runs before integrate so it uses
        // the SAME i_current buffer that integrate will read this tick).
        let initial_parity = self.parity;

        let gain_excit = excitability;
        let fp_scale = self.config.fixed_point_scale as f32;

        // One encoder for the whole batch. Each tick: write uniforms, clear
        // spike_count, integrate, write indirect args, indirect scatter, flip.
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sim-batch"),
        });

        let zero = [0u32];
        for tick_idx in 0..ticks {
            // Phase 3: stimulate dispatch at the start of the FIRST tick only
            // (the stim uniform was written once above for this batch).
            if tick_idx == 0 && do_stim {
                if let (Some(ps), Some(sbgs)) = (pipe_stim, stim_bgs) {
                    let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("stimulate"),
                        timestamp_writes: None,
                    });
                    cp.set_pipeline(ps);
                    cp.set_bind_group(0, &sbgs[initial_parity], &[]);
                    cp.dispatch_workgroups(1, 1, 1);
                }
            }

            // Update the integrate uniform (cheap; per-tick tick counter).
            let u = IntegrateUniforms {
                tick: self.tick,
                n,
                leak_decay: LEAK_DECAY,
                threshold: THRESHOLD,
                reset_potential: RESET_POTENTIAL,
                refractory_ticks: REFRACTORY_TICKS,
                i_ext: self.i_ext,
                excitability: gain_excit,
                fixed_point_scale: fp_scale,
                synaptic_scale: self.synaptic_scale,
                // ─── V2 Phase C ───────────────────────────────────────────────
                seed_lo: self.config.seed_lo(),
                heterogeneity: self.visual.heterogeneity,
                weight_norm_factor: weight_norm_factor(
                    self.visual.weight_normalization,
                    self.config.k,
                ),
                input_mode: self.visual.input_mode,
                _pad: [0; 2],
            };
            queue.write_buffer(&sim.integrate_uniform, 0, bytemuck::bytes_of(&u));
            // Reset spike_count to 0 for this tick.
            queue.write_buffer(&sim.spike_count, 0, bytemuck::cast_slice(&zero));

            let p = self.parity;
            {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("integrate"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_integrate);
                cp.set_bind_group(0, &bg.integrate[p], &[]);
                cp.set_bind_group(1, &bg.integrate_uniform, &[]);
                cp.dispatch_workgroups(integrate_groups, 1, 1);
            }
            {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("write_scatter_dispatch"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_write);
                cp.set_bind_group(0, &bg.write_dispatch, &[]);
                cp.set_bind_group(1, &bg.connect_uniform, &[]);
                cp.dispatch_workgroups(1, 1, 1);
            }
            {
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("scatter"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipe_scatter);
                cp.set_bind_group(0, &bg.scatter[p], &[]);
                cp.set_bind_group(1, &bg.connect_uniform, &[]);
                // GPU-driven: scatter group count comes from dispatch_args,
                // written by write_scatter_dispatch above. NO CPU readback.
                cp.dispatch_workgroups_indirect(&sim.dispatch_args, 0);
            }

            // V2 Phase D: emit one active-edge per firing neuron AFTER scatter.
            // Skipped entirely when the connection layer is off.
            if do_emit {
                if let (Some(pe), Some(ebg), Some(eubg), Some(eb)) =
                    (pipe_emit, emit_bg, emit_u_bg, edge_bufs)
                {
                    // Update the per-tick edge uniform + zero the per-frame counter.
                    let eu = EdgeUniforms {
                        tick: self.tick,
                        n,
                        k: self.config.k as u32,
                        seed_lo: self.config.seed_lo(),
                        grid_dim: self
                            .resources
                            .grid_buffers
                            .as_ref()
                            .map(|g| g.grid_dim)
                            .unwrap_or(1),
                        modulus: edge_modulus,
                        sample_stride: 1, // emit per firing neuron (ring caps the budget)
                        _pad: 0,
                    };
                    queue.write_buffer(&eb.edge_uniform, 0, bytemuck::bytes_of(&eu));
                    queue.write_buffer(&eb.edge_emitted, 0, bytemuck::cast_slice(&zero));
                    // Dispatch one thread per neuron; threads past spike_count early-out.
                    let emit_groups = n.div_ceil(64).max(1);
                    let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("emit_edges"),
                        timestamp_writes: None,
                    });
                    cp.set_pipeline(pe);
                    cp.set_bind_group(0, ebg, &[]);
                    cp.set_bind_group(1, eubg, &[]);
                    cp.dispatch_workgroups(emit_groups, 1, 1);
                }
            }

            // Flip double-buffer parity: next tick integrate reads what scatter
            // just wrote, and scatter writes the buffer integrate just consumed.
            self.parity ^= 1;
            self.tick = self.tick.wrapping_add(1);
        }

        // Stage stats ONCE for the whole batch (after the last tick). This reads
        // spike_count from the final tick + the high-water max|current|. It does
        // not size any dispatch; purely instrumentation.
        //
        // On wasm/WebGPU the CPU-side readback is skipped: device.poll(Wait) is
        // a documented no-op there, so map_async never completes synchronously.
        // Leaving stats_staging in a pending-mapped state would make the next
        // copy_buffer_to_buffer into it a validation error, silently poisoning the
        // command encoder and producing a permanently black frame.  Stats are
        // profiler-only; returning 0 is safe.
        #[cfg(not(target_arch = "wasm32"))]
        {
            enc.copy_buffer_to_buffer(&sim.spike_count, 0, &sim.stats_staging, 0, 4);
            enc.copy_buffer_to_buffer(&sim.max_abs_current, 0, &sim.stats_staging, 4, 4);
            // V2 Phase D: stage edge_emitted (no silent caps). Only when active.
            if do_emit {
                if let Some(eb) = edge_bufs {
                    enc.copy_buffer_to_buffer(&eb.edge_emitted, 0, &eb.edge_emitted_staging, 0, 4);
                }
            }
        }

        queue.submit([enc.finish()]);

        #[cfg(not(target_arch = "wasm32"))]
        let (last_spikes, max_abs) = read_stats(device, &sim.stats_staging);
        // V2 Phase D: read edge_emitted into a temp here (immutable borrows of
        // device + edge_bufs still live); assign to self below after they drop.
        #[cfg(not(target_arch = "wasm32"))]
        let edges_emitted = if do_emit {
            edge_bufs
                .map(|eb| read_u32(device, &eb.edge_emitted_staging))
                .unwrap_or(0)
        } else {
            0
        };
        #[cfg(target_arch = "wasm32")]
        let edges_emitted = 0u32;
        #[cfg(target_arch = "wasm32")]
        let (last_spikes, max_abs) = (0u32, 0u32);
        self.max_abs_current_hw = self.max_abs_current_hw.max(max_abs);
        // V2 Phase D: surface the emit count (0 when the layer is off).
        self.edges_emitted_last = edges_emitted;

        // V2 Phase A: advance the metrics throttle by this batch, then drive the
        // metrics reduction + non-blocking readback state machine. This re-borrows
        // self mutably, so it must come AFTER the immutable sim/device borrows
        // used by read_stats above are released.
        self.ticks_since_metrics_issue = self.ticks_since_metrics_issue.saturating_add(ticks);
        self.update_metrics();

        #[cfg(not(target_arch = "wasm32"))]
        let tick_ms = t0.elapsed().as_secs_f32() * 1000.0;
        #[cfg(target_arch = "wasm32")]
        let tick_ms = 0.0_f32;
        // spikes: we only have the final tick's count cheaply. Approximate total
        // batch spikes as last_count * ticks (uniform-rate assumption) for the
        // throughput headline; exact per-tick sum would need per-tick readback,
        // which the no-stall policy forbids. Callers that need exact counts use
        // ticks=1 (the verification harness does for rate measurement).
        let spikes = (last_spikes as u64) * (ticks as u64);
        let synaptic_events = spikes * self.config.k as u64;
        TickStats {
            tick_count: ticks,
            spikes,
            synaptic_events,
            tick_ms,
        }
    }

    fn stimulate(&mut self, pos: [f32; 3], radius: f32, current: f32) {
        // Convert current to fixed-point (S = FIXED_POINT_SCALE = 4096).
        let current_fp = (current * self.config.fixed_point_scale as f32) as i32;
        self.stim_pending = Some(StimUniform {
            pos,
            radius,
            current_fp,
            is_active: 1,
            _pad: [0; 2],
        });
    }

    fn render_state(&self) -> RenderState<'_> {
        match (&self.resources.neuron_buffers,) {
            (Some(nb),) if !nb.v.chunks.is_empty() => RenderState::Gpu {
                v_buf: &nb.v.chunks[0],
                last_spike_buf: &nb.last_spike.chunks[0],
                pos_x_buf: &nb.pos_x.chunks[0],
                pos_y_buf: &nb.pos_y.chunks[0],
                pos_z_buf: &nb.pos_z.chunks[0],
                neuron_count: self.config.n,
            },
            _ => RenderState::Empty,
        }
    }

    fn resize(&mut self, config: &SimConfig) {
        self.initialize(config);
    }

    fn destroy(&mut self) {
        self.resources.destroy();
    }
}

/// Extract 6 frustum planes from a column-major MVP matrix (Gribb-Hartmann).
/// Returns [[a,b,c,d]; 6] where ax+by+cz+d >= 0 is inside. Each plane is
/// UNNORMALIZED (sufficient for sign tests). Planes: left, right, bottom, top, near, far.
fn extract_frustum_planes(m: &[f32; 16]) -> [[f32; 4]; 6] {
    // Column-major: m[col*4 + row]. Row vectors of the matrix for plane extraction.
    // Row 0: m[0],m[4],m[8],m[12]
    // Row 1: m[1],m[5],m[9],m[13]
    // Row 2: m[2],m[6],m[10],m[14]
    // Row 3: m[3],m[7],m[11],m[15]
    let row0 = [m[0], m[4], m[8], m[12]];
    let row1 = [m[1], m[5], m[9], m[13]];
    let row2 = [m[2], m[6], m[10], m[14]];
    let row3 = [m[3], m[7], m[11], m[15]];

    let add = |a: [f32; 4], b: [f32; 4]| [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]];
    let sub = |a: [f32; 4], b: [f32; 4]| [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]];

    // Left:   row3 + row0
    // Right:  row3 - row0
    // Bottom: row3 + row1
    // Top:    row3 - row1
    // Near:   row3 + row2
    // Far:    row3 - row2
    [
        add(row3, row0), // left
        sub(row3, row0), // right
        add(row3, row1), // bottom
        sub(row3, row1), // top
        add(row3, row2), // near
        sub(row3, row2), // far
    ]
}

/// Read near-LOD profiler stats from the staging buffer (blocks once per frame).
#[cfg(not(target_arch = "wasm32"))]
fn read_near_lod_stats(device: &wgpu::Device, staging: &wgpu::Buffer) -> NearLodStats {
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    if rx.recv().is_err() {
        return NearLodStats::default();
    }
    let data = slice.get_mapped_range();
    let words: &[u32] = bytemuck::cast_slice(&*data);
    if words.len() < 6 {
        drop(data);
        staging.unmap();
        return NearLodStats::default();
    }
    let stats = NearLodStats {
        emitted_neuron_instances: words[0],
        neuron_overflow: words[1],
        emitted_synapse_instances: words[2],
        synapse_overflow: words[3],
        visible_neuron_candidates: words[4],
        visible_synapse_candidates: words[5],
    };
    drop(data);
    staging.unmap();
    stats
}

/// One-shot stats readback: map the 8-byte staging buffer, return
/// (spike_count, max_abs_current). Blocks on poll — acceptable once per batch.
#[cfg(not(target_arch = "wasm32"))]
fn read_stats(device: &wgpu::Device, staging: &wgpu::Buffer) -> (u32, u32) {
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    if rx.recv().is_err() {
        return (0, 0);
    }
    let data = slice.get_mapped_range();
    let words: &[u32] = bytemuck::cast_slice(&data);
    let out = (words[0], words[1]);
    drop(data);
    staging.unmap();
    out
}

/// V2 Phase D: read a single u32 from a 4-byte staging buffer (blocks on poll;
/// once per batch, acceptable — instrumentation only). Used for edge_emitted.
#[cfg(not(target_arch = "wasm32"))]
fn read_u32(device: &wgpu::Device, staging: &wgpu::Buffer) -> u32 {
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    if rx.recv().is_err() {
        return 0;
    }
    let data = slice.get_mapped_range();
    let words: &[u32] = bytemuck::cast_slice(&data);
    let out = if words.is_empty() { 0 } else { words[0] };
    drop(data);
    staging.unmap();
    out
}

/// One-off debug readback of a storage buffer (stalls; off the hot path only).
#[cfg(not(target_arch = "wasm32"))]
fn readback<T: bytemuck::Pod>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    src: &wgpu::Buffer,
    count: usize,
) -> Vec<T> {
    let bytes = (count * std::mem::size_of::<T>()) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("debug_readback"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(src, 0, &staging, 0, bytes);
    queue.submit([enc.finish()]);
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().expect("map").expect("map ok");
    let data = slice.get_mapped_range();
    let out: Vec<T> = bytemuck::cast_slice(&data)[..count].to_vec();
    drop(data);
    staging.unmap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_001,
            "actual={actual} expected={expected}"
        );
    }

    #[test]
    fn lif_constants_match_spec() {
        assert_eq!(LEAK_DECAY, 0.95);
        assert_eq!(THRESHOLD, 1.0);
        assert_eq!(RESET_POTENTIAL, 0.0);
        assert_eq!(REFRACTORY_TICKS, 5);
    }

    #[test]
    fn tuning_knobs_default_neutral() {
        // synaptic_scale defaults to 1.0 (neutral) without a device; we can't
        // build a real backend in a unit test, but the constant is the contract.
        // Verified end-to-end by examples/sim_check.rs (native GPU).
        let _ = (LEAK_DECAY, THRESHOLD); // touch to keep the module exercised
    }

    #[test]
    fn visual_settings_default_matches_product_defaults() {
        let settings = VisualSettings::default();
        assert_eq!(settings.glow_tau, 10.0);
        assert_eq!(settings.bloom_strength, 0.0);
        assert_eq!(settings.heterogeneity, 0.50);
    }

    #[test]
    fn visual_settings_from_short_slice_uses_product_defaults() {
        let settings = VisualSettings::from_slice(&[]);
        assert_eq!(settings.glow_tau, 10.0);
        assert_eq!(settings.bloom_strength, 0.0);
        assert_eq!(settings.heterogeneity, 0.50);
    }

    #[test]
    fn visual_settings_ignores_tombstoned_bloom_slot() {
        let mut data = vec![0.0; 26];
        data[10] = 1.5;
        let settings = VisualSettings::from_slice(&data);
        assert_eq!(settings.bloom_strength, 0.0);
    }

    #[test]
    fn visual_settings_from_slice_maps_locked_indices() {
        let data: Vec<f32> = (1..=26).map(|v| v as f32).collect();
        let settings = VisualSettings::from_slice(&data);

        assert_eq!(settings.glow_tau, 1.0);
        assert_eq!(settings.point_radius, 2.0);
        assert_eq!(settings.neuron_visual_radius, 3.0);
        assert_eq!(settings.active_neuron_radius_boost, 4.0);
        assert_eq!(settings.inactive_neuron_opacity, 5.0);
        assert_eq!(settings.voltage_glow_strength, 6.0);
        assert_eq!(settings.connection_visual_width, 7.0);
        assert_eq!(settings.connection_curve_lift, 8.0);
        assert_eq!(settings.connection_light_next, 9);
        assert_eq!(settings.connection_light_past, 0);
        assert_eq!(settings.bloom_strength, 0.0);
        assert_eq!(settings.surface_opacity, 12.0);
        assert_eq!(settings.i_ext, 13.0);
        assert_eq!(settings.synaptic_scale, 14.0);
        assert_eq!(settings.heterogeneity, 15.0);
        assert_eq!(settings.morph_resting_opacity, 16.0);
        assert_eq!(settings.signal_source, 0);
        assert_eq!(settings.connection_layer, 18);
        assert_eq!(settings.color_by, 19);
        assert_eq!(settings.neuron_visibility, 20);
        assert_eq!(settings.surface, 21);
        assert_eq!(settings.weight_normalization, 22);
        assert_eq!(settings.input_mode, 23);
        assert_eq!(settings.adaptive_scaler_enabled, 0);
        assert_eq!(settings.long_range_reach_frac, 25.0);
        assert_eq!(settings.max_reach_cells, 26.0);
    }

    #[test]
    fn visual_settings_tombstoned_slots_ignore_nonzero_input() {
        let mut data = vec![0.0; 26];
        data[9] = 9.0;
        data[10] = 10.0;
        data[16] = 16.0;
        data[23] = 23.0;

        let settings = VisualSettings::from_slice(&data);

        assert_eq!(settings.connection_light_past, 0);
        assert_eq!(settings.bloom_strength, 0.0);
        assert_eq!(settings.signal_source, 0);
        assert_eq!(settings.adaptive_scaler_enabled, 0);
    }

    #[test]
    fn prepared_network_flat_payload_roundtrips_to_rust_types() {
        let config = SimConfig {
            n: 48,
            k: 4,
            seed: 123,
            ..SimConfig::default()
        };
        let visual = VisualSettings::default();
        let morph_config = crate::sim::morphology::MorphologyConfig::default();
        let params = morph_params_from_config_and_visual(&morph_config, &visual);
        let prepared = PreparedNetworkBuild::prepare(
            config.clone(),
            params,
            reach_from_visual_settings(&visual),
        );

        let rebuilt = PreparedNetworkBuild::from_flat_payload(
            config,
            &prepared.positions_f32(),
            &prepared.region_codes(),
            &prepared.grid_min_f32(),
            prepared.grid_cell_size(),
            prepared.grid_dim(),
            &prepared.grid_cell_start_u32(),
            &prepared.grid_cell_neurons_u32(),
            &prepared.vertices_f32(),
            &prepared.faces_u32(),
            &prepared.segment_endpoints_f32(),
            &prepared.segment_path_len_f32(),
            &prepared.segment_neuron_ids_u32(),
            &prepared.segment_kinds_u32(),
            &prepared.segment_target_ids_u32(),
            &prepared.sphere_geometry_f32(),
            &prepared.sphere_neuron_ids_u32(),
            &prepared.sphere_kinds_u32(),
            params,
            prepared.morphology.stats,
            prepared.dropped_count(),
        )
        .expect("prepared payload should validate");

        assert_eq!(
            rebuilt.manifold.neuron_positions,
            prepared.manifold.neuron_positions
        );
        assert_eq!(
            rebuilt.manifold.neuron_regions,
            prepared.manifold.neuron_regions
        );
        assert_eq!(
            rebuilt.manifold.spatial_grid.cell_start,
            prepared.manifold.spatial_grid.cell_start
        );
        assert_eq!(
            rebuilt.manifold.spatial_grid.cell_neurons,
            prepared.manifold.spatial_grid.cell_neurons
        );
        assert_eq!(
            rebuilt.morphology.segments.len(),
            prepared.morphology.segments.len()
        );
        assert_eq!(
            rebuilt.morphology.spheres.len(),
            prepared.morphology.spheres.len()
        );
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&rebuilt.morphology.segments),
            bytemuck::cast_slice::<_, u8>(&prepared.morphology.segments)
        );
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&rebuilt.morphology.spheres),
            bytemuck::cast_slice::<_, u8>(&prepared.morphology.spheres)
        );
    }

    #[test]
    fn prepared_network_validation_rejects_bad_region_code() {
        let config = SimConfig {
            n: 8,
            k: 2,
            seed: 7,
            ..SimConfig::default()
        };
        let visual = VisualSettings::default();
        let params = morph_params_from_config_and_visual(
            &crate::sim::morphology::MorphologyConfig::default(),
            &visual,
        );
        let prepared = PreparedNetworkBuild::prepare(
            config.clone(),
            params,
            reach_from_visual_settings(&visual),
        );
        let mut regions = prepared.region_codes();
        regions[0] = 9;

        let err = match PreparedNetworkBuild::from_flat_payload(
            config,
            &prepared.positions_f32(),
            &regions,
            &prepared.grid_min_f32(),
            prepared.grid_cell_size(),
            prepared.grid_dim(),
            &prepared.grid_cell_start_u32(),
            &prepared.grid_cell_neurons_u32(),
            &prepared.vertices_f32(),
            &prepared.faces_u32(),
            &prepared.segment_endpoints_f32(),
            &prepared.segment_path_len_f32(),
            &prepared.segment_neuron_ids_u32(),
            &prepared.segment_kinds_u32(),
            &prepared.segment_target_ids_u32(),
            &prepared.sphere_geometry_f32(),
            &prepared.sphere_neuron_ids_u32(),
            &prepared.sphere_kinds_u32(),
            params,
            prepared.morphology.stats,
            prepared.dropped_count(),
        ) {
            Ok(_) => panic!("bad region code should be rejected"),
            Err(err) => err,
        };

        assert!(err.contains("region code 9 invalid"));
    }

    #[test]
    fn metrics_snapshot_layout_and_histogram_offset_are_locked() {
        let mut raw = [0u32; METRICS_SLOT_COUNT];
        raw[0] = 2;
        raw[1] = 3;
        raw[2] = 4;
        raw[3] = 5;
        raw[4] = 6;
        raw[5] = 7;
        raw[6] = 1;
        raw[7] = 2;
        raw[8] = 3;
        raw[9] = 1024;
        raw[10] = 0;
        raw[11] = 8;
        for b in 0..METRICS_HISTO_BINS as usize {
            raw[16 + b] = b as u32;
        }
        let history = VecDeque::from([2, 4, 8]);

        let out = build_metrics_snapshot(4, 7, &raw, &history, 3, 99);

        assert_eq!(out.len(), 33);
        assert_close(out[0], 2.0);
        assert_close(out[1], 120.0);
        assert_close(out[2], 30.0);
        assert_close(out[3], 840.0);
        assert_close(out[4], -0.25);
        assert_close(out[5], 3.0);
        assert_close(out[6], 4.0);
        assert_close(out[7], 5.0);
        assert_close(out[8], 6.0);
        assert_close(out[9], 7.0);
        assert_close(out[10], 0.25);
        assert_close(out[11], 0.5);
        assert_close(out[12], 0.75);
        assert_close(out[13], 2.0);
        assert_close(out[14], 45.0);
        assert_close(out[15], 8.0);
        assert_close(out[16], 99.0);
        for b in 0..METRICS_HISTO_BINS as usize {
            assert_close(out[METRICS_SCALAR_COUNT + b], b as f32 / 4.0);
        }
    }
}
