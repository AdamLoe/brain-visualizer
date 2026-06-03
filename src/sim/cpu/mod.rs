//! CPU backend — event-driven, active-list LIF on rayon (BV4, BV24, phase 6).
//!
//! The CPU backend runs the SAME network as the GPU backend (shared
//! `connectivity::target`/`weight`, same BV21 packing, same fixed-point current
//! scale) but event-driven: only neurons that received current this tick (plus
//! the always-driven input-region neurons) are integrated; silent neurons decay
//! lazily the next time they appear in the active list (`core::integrate_neuron`).
//!
//! Native (examples/tests, this verification environment) rayon runs directly on
//! the host. In the browser the sim runs on a dedicated coordinator Web Worker
//! that owns a `wasm-bindgen-rayon` pool and writes the SoA into a
//! SharedArrayBuffer (gated behind the `cpu-threads` feature); the main thread
//! renders via WebGL2. The browser glue is compile-only here (no headless
//! browser) — see `web/cpu-worker.ts` and `web/cpu-renderer.ts`.

pub mod core;

use crate::sim::backend::{RenderState, SimBackend, SimConfig, TickStats};
use core::{ConnParams, CpuNeuronBuffers, LifParams};

use crate::connectivity::SpatialGrid;

/// Event-driven CPU simulation backend.
pub struct CpuBackend {
    config: SimConfig,
    /// Per-neuron SoA state (membrane, packed spike, current accumulator, …).
    buffers: Option<CpuNeuronBuffers>,
    /// Static spatial grid (shared connectivity reads this each scatter).
    grid: Option<SpatialGrid>,
    /// Neuron world positions for the WebGL2 renderer.
    positions: Vec<[f32; 3]>,
    /// Global tick counter (24-bit-wrapping semantics in `last_spike`).
    tick: u32,
    /// Tuning knobs (match GpuBackend defaults / sim_check).
    i_ext: f32,
    synaptic_scale: f32,
    /// Active list for the next tick (neurons to integrate). Reused across ticks
    /// to avoid per-tick allocation in the hot loop.
    active: Vec<u32>,
    /// Scratch: fired neurons this tick.
    fired: Vec<u32>,
    /// Scratch: touched targets (sorted/deduped) — becomes next `active`.
    touched: Vec<u32>,
    /// High-water max |accumulated current| (fixed-point) for overflow checks.
    pub max_abs_current_hw: u32,
    /// Pending stimulation (world pos, radius, fixed-point current).
    stim_pending: Option<([f32; 3], f32, i32)>,
}

impl CpuBackend {
    pub fn new(config: SimConfig) -> Self {
        let i_ext = config.i_ext;
        Self {
            config,
            buffers: None,
            grid: None,
            positions: Vec::new(),
            tick: 0,
            i_ext,
            synaptic_scale: 1.0,
            active: Vec::new(),
            fired: Vec::new(),
            touched: Vec::new(),
            max_abs_current_hw: 0,
            stim_pending: None,
        }
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn tick_count(&self) -> u32 {
        self.tick
    }

    /// Override ambient drive (BV17 i_ext) — tuning knob, no locked value changes.
    pub fn set_i_ext(&mut self, i_ext: f32) {
        self.i_ext = i_ext;
    }

    /// Set the effective recurrent-coupling scale (matches GpuBackend). Default 1.0.
    pub fn set_synaptic_scale(&mut self, s: f32) {
        self.synaptic_scale = s;
    }

    /// Read-only access to the SoA buffers (verification / debug).
    pub fn buffers(&self) -> Option<&CpuNeuronBuffers> {
        self.buffers.as_ref()
    }

    pub fn grid(&self) -> Option<&SpatialGrid> {
        self.grid.as_ref()
    }

    /// Build the network from the manifold and upload the silent-start state.
    /// Rare-path; allocates. Mirrors `GpuBackend::initialize` so a backend switch
    /// at the same seed produces the identical network (BV16).
    pub fn initialize(&mut self, config: &SimConfig) {
        self.config = config.clone();
        let manifold = crate::build_manifold(config);
        let buffers = CpuNeuronBuffers::build(
            config,
            &manifold.neuron_regions,
            &manifold.spatial_grid,
        );
        // Seed the active list with the input neurons so activity starts from
        // ambient drive (everything else is silent and decays lazily).
        self.active.clear();
        self.active.extend_from_slice(&buffers.input_neurons);
        self.positions = manifold.neuron_positions;
        self.grid = Some(manifold.spatial_grid);
        self.buffers = Some(buffers);
        self.tick = 0;
        self.max_abs_current_hw = 0;
        self.stim_pending = None;
        self.fired.clear();
        self.touched.clear();
    }

    fn lif_params(&self, excitability: f32) -> LifParams {
        LifParams {
            excitability,
            ..LifParams::new(self.i_ext, self.synaptic_scale, self.config.fixed_point_scale as f32)
        }
    }
}

impl SimBackend for CpuBackend {
    fn tick(&mut self, ticks: u32, excitability: f32) -> TickStats {
        if self.buffers.is_none() || self.grid.is_none() {
            return TickStats::default();
        }

        let t0 = std::time::Instant::now();
        let params = self.lif_params(excitability);
        let conn = ConnParams {
            k: self.config.k,
            seed_lo: self.config.seed_lo(),
        };

        let mut max_hw = self.max_abs_current_hw;

        // Split-borrow self into disjoint field references so the scratch lists
        // (active/fired/touched) can be mutated alongside the buffers + grid.
        let Self {
            buffers,
            grid,
            active,
            fired,
            touched,
            stim_pending,
            tick,
            ..
        } = self;
        let grid = grid.as_ref().unwrap();
        let buffers = buffers.as_mut().unwrap();

        // Apply any pending stimulation once before the batch (fixed-point add
        // to neurons within the radius, via the spatial grid — bounded lookup).
        if let Some((pos, radius, current_fp)) = stim_pending.take() {
            apply_stimulation(buffers, grid, pos, radius, current_fp);
            // Ensure stimulated neurons get integrated this batch (the first
            // integrate needs them in the active set; afterwards they're carried
            // by the touched list).
            stimulated_into_active(buffers, grid, pos, radius, active);
        }

        let mut total_spikes: u64 = 0;
        let mut total_syn: u64 = 0;

        for _ in 0..ticks {
            *tick = tick.wrapping_add(1);
            let current_tick = *tick;

            // Integrate the active list → fired ids.
            core::integrate_active(active, fired, buffers, current_tick, &params);
            total_spikes += fired.len() as u64;

            // Scatter fired neurons' current into targets → touched list.
            let events = core::scatter_tick(fired, buffers, grid, &conn, touched);
            total_syn += events;

            // Track high-water |accumulated current| over the touched targets.
            for &t in touched.iter() {
                let mag = buffers.i[t as usize]
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .unsigned_abs();
                if mag > max_hw {
                    max_hw = mag;
                }
            }

            // The touched list (targets that received current this tick + the
            // always-driven input neurons) is the next active list.
            std::mem::swap(active, touched);
        }
        let final_tick = *tick;

        // Refresh the decayed render snapshot for the whole array before the
        // caller uploads it to WebGL (so silent neurons don't keep stale glow).
        core::update_v_render(buffers, final_tick, &params);

        self.max_abs_current_hw = max_hw;
        let tick_ms = t0.elapsed().as_secs_f32() * 1000.0;
        TickStats {
            tick_count: ticks,
            spikes: total_spikes,
            synaptic_events: total_syn,
            tick_ms,
        }
    }

    fn stimulate(&mut self, pos: [f32; 3], radius: f32, current: f32) {
        let current_fp = (current * self.config.fixed_point_scale as f32) as i32;
        self.stim_pending = Some((pos, radius, current_fp));
    }

    fn render_state(&self) -> RenderState<'_> {
        match &self.buffers {
            Some(b) if !b.is_empty() => RenderState::Cpu {
                v_render: &b.v_render,
                last_spike: &b.last_spike,
                positions: &self.positions,
            },
            _ => RenderState::Empty,
        }
    }

    fn resize(&mut self, config: &SimConfig) {
        self.initialize(config);
    }

    fn destroy(&mut self) {
        // Release all SoA state + the grid (browser: terminate the coordinator
        // worker + rayon pool — handled in the TS layer).
        self.buffers = None;
        self.grid = None;
        self.positions = Vec::new();
        self.active = Vec::new();
        self.fired = Vec::new();
        self.touched = Vec::new();
    }
}

/// Add fixed-point current to neurons near `pos` (cursor stimulation) AND push
/// them into the active list so they're integrated next tick. The candidate set
/// is the grid cells overlapping the sphere bounding box — same bounded lookup
/// as the GPU `stimulate.wgsl`. The radius footprint is approximated in cell
/// space (Chebyshev distance in cells), which is exact enough for the cursor.
fn apply_stimulation(
    buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    pos: [f32; 3],
    radius: f32,
    current_fp: i32,
) {
    use std::sync::atomic::Ordering;
    for_each_neuron_in_radius(grid, pos, radius, |idx| {
        buffers.i[idx].fetch_add(current_fp, Ordering::Relaxed);
    });
}

/// Push stimulated neurons into the active list so they're integrated next tick.
fn stimulated_into_active(
    _buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    pos: [f32; 3],
    radius: f32,
    active: &mut Vec<u32>,
) {
    for_each_neuron_in_radius(grid, pos, radius, |idx| active.push(idx as u32));
    active.sort_unstable();
    active.dedup();
}

/// Visit every neuron in the grid cells overlapping the stimulation sphere.
fn for_each_neuron_in_radius(
    grid: &SpatialGrid,
    pos: [f32; 3],
    radius: f32,
    mut f: impl FnMut(usize),
) {
    let center = grid.cell_coord(pos);
    let cells_r = ((radius / grid.cell_size).ceil() as i32).max(1);
    let dim = grid.dim as i32;
    for dz in -cells_r..=cells_r {
        for dy in -cells_r..=cells_r {
            for dx in -cells_r..=cells_r {
                let cx = center[0] as i32 + dx;
                let cy = center[1] as i32 + dy;
                let cz = center[2] as i32 + dz;
                if cx < 0 || cy < 0 || cz < 0 || cx >= dim || cy >= dim || cz >= dim {
                    continue;
                }
                let cid = grid.pack([cx as u32, cy as u32, cz as u32]);
                for &n in grid.neurons_in_cell(cid) {
                    f(n as usize);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::backend::has_spiked;

    fn small_config(n: usize) -> SimConfig {
        SimConfig {
            n,
            k: 16,
            ..SimConfig::default()
        }
    }

    #[test]
    fn initialize_builds_state() {
        let mut b = CpuBackend::new(small_config(2000));
        b.initialize(&small_config(2000));
        let bufs = b.buffers().unwrap();
        assert_eq!(bufs.len(), 2000);
        assert!(!bufs.input_neurons.is_empty(), "input region must be non-empty");
        // Silent start: nothing has spiked yet.
        assert!(bufs.last_spike.iter().all(|&w| !has_spiked(w)));
    }

    #[test]
    fn focused_run_produces_spikes() {
        let cfg = small_config(5000);
        let mut b = CpuBackend::new(cfg.clone());
        b.set_i_ext(0.040);
        b.set_synaptic_scale(0.03);
        b.initialize(&cfg);
        // Warm up + measure.
        for _ in 0..200 {
            b.tick(1, 0.55);
        }
        let mut spikes = 0u64;
        for _ in 0..200 {
            spikes += b.tick(1, 0.55).spikes;
        }
        assert!(spikes > 0, "focused run produced no spikes");
    }

    #[test]
    fn lazy_decay_zeroes_silent_neuron() {
        // A neuron given an initial v and never re-stimulated should decay to ~0
        // after 500 ticks via lazy decay (v_init * 0.95^500).
        let cfg = small_config(1000);
        let mut b = CpuBackend::new(cfg.clone());
        b.initialize(&cfg);
        let params = b.lif_params(0.0);
        let bufs = b.buffers.as_mut().unwrap();
        // Pick a non-input neuron (so i_ext doesn't re-excite it): type>>2 != 0.
        let probe = (0..1000)
            .find(|&i| (((bufs.last_spike[i] >> 24) & 0x7F) >> 2) != 0)
            .unwrap_or(0);
        bufs.v[probe] = 1.0;
        bufs.last_updated[probe] = 0;
        // Integrate once at tick 500 → lazy decay applies 0.95^499 then one more.
        core::integrate_neuron(probe, 500, bufs, &params);
        assert!(bufs.v[probe].abs() < 1e-6, "v not decayed: {}", bufs.v[probe]);
    }

    #[test]
    fn render_decay_zeroes_untouched_neuron() {
        let cfg = small_config(1000);
        let mut b = CpuBackend::new(cfg.clone());
        b.initialize(&cfg);
        let params = b.lif_params(0.0);
        let bufs = b.buffers.as_mut().unwrap();
        let probe = 0usize;
        bufs.v[probe] = 1.0;
        bufs.last_updated[probe] = 0;
        core::update_v_render(bufs, 500, &params);
        assert!(
            bufs.v_render[probe].abs() < 1e-6,
            "v_render not decayed: {}",
            bufs.v_render[probe]
        );
    }

    #[test]
    fn destroy_releases_state() {
        let cfg = small_config(1000);
        let mut b = CpuBackend::new(cfg.clone());
        b.initialize(&cfg);
        b.destroy();
        assert!(b.buffers.is_none());
        matches!(b.render_state(), RenderState::Empty);
    }
}
