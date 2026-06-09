//! CPU simulation core (phase 6) — pure Rust, native-testable.
//!
//! Event-driven LIF with lazy decay (architecture §5, phase-6 spec). The same
//! procedural connectivity (`connectivity::target`/`weight`) and the same
//! fixed-point current scale (S = 4096, BV19) as the GPU path, so CPU≡GPU
//! networks at a given seed and the dynamics are directly comparable.
//!
//! This module is `#![cfg(not(wasm32))]`-independent: it is plain Rust and runs
//! on the host (native rayon, 20 cores) and on wasm (single-thread by default;
//! threaded under the `cpu-threads` feature). The hot loops avoid per-tick
//! allocation by reusing caller-owned scratch vectors.

use std::sync::atomic::{AtomicI32, Ordering};

use crate::connectivity::{self, SpatialGrid};
use crate::manifold::RegionKind;
use crate::sim::backend::{neuron_type_byte, SimConfig, HAS_SPIKED_MASK, TICK_MASK};

/// LIF parameters — identical to the GPU `integrate.wgsl` constants so the two
/// backends share dynamics. (`GpuBackend` keeps these as private consts; we
/// re-declare the same locked values here.)
#[derive(Debug, Clone, Copy)]
pub struct LifParams {
    pub leak_decay: f32,
    pub threshold: f32,
    pub reset_potential: f32,
    pub refractory_ticks: u32,
    pub i_ext: f32,
    pub excitability: f32,
    pub fixed_point_scale: f32,
    /// Effective recurrent-coupling scale (tuning knob; matches GpuBackend).
    pub synaptic_scale: f32,
}

impl LifParams {
    /// Locked LIF constants (matches `src/sim/gpu/mod.rs`).
    pub const LEAK_DECAY: f32 = 0.95;
    pub const THRESHOLD: f32 = 1.0;
    pub const RESET_POTENTIAL: f32 = 0.0;
    pub const REFRACTORY_TICKS: u32 = 5;

    pub fn new(i_ext: f32, synaptic_scale: f32, fixed_point_scale: f32) -> Self {
        Self {
            leak_decay: Self::LEAK_DECAY,
            threshold: Self::THRESHOLD,
            reset_potential: Self::RESET_POTENTIAL,
            refractory_ticks: Self::REFRACTORY_TICKS,
            i_ext,
            excitability: 0.0,
            fixed_point_scale,
            synaptic_scale,
        }
    }
}

/// Connectivity parameters shared with the scatter pass.
#[derive(Debug, Clone, Copy)]
pub struct ConnParams {
    pub k: usize,
    pub seed_lo: u32,
    /// Heavy-tailed reach knobs (CPU backend has no dev panel; defaults to
    /// `ReachParams::LOCAL_ONLY`, keeping CPU↔GPU bit-identical at frac=0).
    pub reach: connectivity::ReachParams,
}

/// Per-neuron CPU state, structure-of-arrays (phase-6 spec). On the browser
/// these `Vec`s are backed by SharedArrayBuffer slices; natively they are plain
/// `Vec`s. `I` is a fixed-point current accumulator (S = 4096) updated by the
/// parallel scatter via atomics — no per-thread partial buffers (BV20).
pub struct CpuNeuronBuffers {
    /// Membrane potential.
    pub v: Vec<f32>,
    /// Packed spike word: bit31 valid, [30:24] type, [23:0] tick (BV21).
    pub last_spike: Vec<u32>,
    /// Fixed-point current accumulator (S = 4096). Atomic for parallel scatter.
    pub i: Vec<AtomicI32>,
    /// Tick of last `integrate_neuron` call (drives lazy decay).
    pub last_updated: Vec<u32>,
    /// Decayed-voltage snapshot uploaded to WebGL each frame.
    pub v_render: Vec<f32>,
    /// Neuron ids in input regions — receive ambient `i_ext` every tick.
    pub input_neurons: Vec<u32>,
    /// Per-neuron integer cell coordinate (precomputed once; hot-path target()).
    pub cell_coord: Vec<[u32; 3]>,
}

impl CpuNeuronBuffers {
    pub fn len(&self) -> usize {
        self.v.len()
    }

    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }

    /// Build silent-start buffers for `config.n` neurons from the manifold's
    /// region assignment and spatial grid. Same silent-start packing as the GPU
    /// path (`initial_last_spike`): HAS_SPIKED = 0, type bits set, tick = 0.
    pub fn build(config: &SimConfig, regions: &[RegionKind], grid: &SpatialGrid) -> Self {
        let n = config.n;
        let seed_lo = config.seed_lo();
        let cell_of_neuron = grid.cell_of_neuron_map();
        debug_assert_eq!(cell_of_neuron.len(), n);

        let mut last_spike = Vec::with_capacity(n);
        let mut input_neurons = Vec::new();
        let mut cell_coord = Vec::with_capacity(n);
        for id in 0..n {
            let region = regions[id];
            // Type byte = (region<<2)|ei. Input region encodes as 0 so the
            // integrate test `(type>>2)==0` selects input-region neurons —
            // identical to the GPU integrate shader.
            let tbyte = neuron_type_byte(id as u32, seed_lo, region);
            last_spike.push(((tbyte as u32) << 24) & crate::sim::backend::TYPE_MASK);
            if region == RegionKind::Input {
                input_neurons.push(id as u32);
            }
            cell_coord.push(grid.unpack(cell_of_neuron[id]));
        }

        let mut i = Vec::with_capacity(n);
        i.resize_with(n, || AtomicI32::new(0));

        Self {
            v: vec![0.0; n],
            last_spike,
            i,
            last_updated: vec![0; n],
            v_render: vec![0.0; n],
            input_neurons,
            cell_coord,
        }
    }
}

/// Integrate a single neuron at `current_tick`, applying lazy decay for the
/// ticks it was dormant, the swap-0 fixed-point current, ambient drive for
/// input-region neurons, the excitability gain, and the threshold + refractory
/// rule. Returns `true` if the neuron fired (and packs `last_spike`).
///
/// Mirrors `integrate.wgsl` exactly, including the `synaptic_scale` knob applied
/// to recurrent current only (not to `i_ext`), so CPU and GPU dynamics match.
#[inline]
pub fn integrate_neuron(
    i: usize,
    current_tick: u32,
    buffers: &mut CpuNeuronBuffers,
    params: &LifParams,
) -> bool {
    // Lazy decay: catch up the leak for ticks skipped since this neuron was
    // last integrated. The +1 the per-tick step adds below handles this tick.
    let ticks_dormant = current_tick.wrapping_sub(buffers.last_updated[i]);
    if ticks_dormant > 1 {
        let decay = params.leak_decay.powi(ticks_dormant as i32 - 1);
        buffers.v[i] *= decay;
    }
    buffers.last_updated[i] = current_tick;

    let packed = buffers.last_spike[i];
    let ntype = (packed >> 24) & 0x7F;
    let last_fire = packed & TICK_MASK;
    // Input region encoded as 0 → upper type bits zero (matches GPU shader).
    let is_input = (ntype >> 2) == 0;

    // Swap-0 the fixed-point accumulator, scale to f32, apply synaptic_scale to
    // recurrent current; add ambient drive for input-region neurons.
    let raw = buffers.i[i].swap(0, Ordering::AcqRel) as f32;
    let mut current = (raw / params.fixed_point_scale) * params.synaptic_scale;
    if is_input {
        current += params.i_ext;
    }

    let gain = 0.5 + params.excitability * 1.5;
    let new_v = buffers.v[i] * params.leak_decay + current * gain;
    buffers.v[i] = new_v;

    let refractory_ok = (packed & HAS_SPIKED_MASK) == 0
        || tick_diff(current_tick, last_fire) > params.refractory_ticks;
    if new_v >= params.threshold && refractory_ok {
        buffers.v[i] = params.reset_potential;
        buffers.last_spike[i] = HAS_SPIKED_MASK | (ntype << 24) | (current_tick & TICK_MASK);
        return true;
    }
    false
}

#[inline]
fn tick_diff(now: u32, then: u32) -> u32 {
    now.wrapping_sub(then) & TICK_MASK
}

/// Integrate every neuron in the active list, collecting fired ids into
/// `fired_next`. Scalar correct path (SIMD128 is a documented follow-up).
pub fn integrate_active(
    active: &[u32],
    fired_next: &mut Vec<u32>,
    buffers: &mut CpuNeuronBuffers,
    current_tick: u32,
    params: &LifParams,
) {
    fired_next.clear();
    for &idx in active {
        if integrate_neuron(idx as usize, current_tick, buffers, params) {
            fired_next.push(idx);
        }
    }
}

/// Parallel scatter: for each fired source, derive its K targets from the SHARED
/// `connectivity::target`/`weight` and `fetch_add` the fixed-point weight into
/// the target's atomic accumulator (BV20 — no per-thread partial buffers).
/// Collects the touched target ids + the always-active input neurons, sorted &
/// deduped, into `touched_out`. Returns the total synaptic events scattered.
///
/// Uses `target_with_cell` with the precomputed per-neuron cell coordinate so
/// each call is O(1) (no `cell_of_index` scan), while remaining bit-identical to
/// the GPU scatter (which reads its precomputed `cell_of_neuron` buffer).
pub fn scatter_tick(
    fired: &[u32],
    buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    params: &ConnParams,
    touched_out: &mut Vec<u32>,
) -> u64 {
    let k = params.k;
    let seed_lo = params.seed_lo;
    let reach = params.reach;

    let per_thread: Vec<Vec<u32>> = scatter_map(fired, buffers, grid, k, seed_lo, reach);

    touched_out.clear();
    for local in &per_thread {
        touched_out.extend_from_slice(local);
    }
    let events = touched_out.len() as u64;
    touched_out.extend_from_slice(&buffers.input_neurons);
    touched_out.sort_unstable();
    touched_out.dedup();
    events
}

/// The scatter body factored so the rayon (`cpu-threads` / native) and the
/// single-threaded fallback share the exact same per-source logic.
#[inline]
fn scatter_one_source(
    src: u32,
    buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    k: usize,
    seed_lo: u32,
    reach: connectivity::ReachParams,
    local_touched: &mut Vec<u32>,
) {
    let s = src as usize;
    let src_type = ((buffers.last_spike[s] >> 24) & 0x7F) as u8;
    let src_cell = buffers.cell_coord[s];
    for j in 0..k {
        let tgt = connectivity::target_with_cell(
            src, j as u32, grid, k, seed_lo, src_type, src_cell, reach,
        ) as usize;
        let w = connectivity::weight(src, j as u32, src_type);
        buffers.i[tgt].fetch_add(w, Ordering::Relaxed);
        local_touched.push(tgt as u32);
    }
}

#[cfg(feature = "cpu-threads")]
fn scatter_map(
    fired: &[u32],
    buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    k: usize,
    seed_lo: u32,
    reach: connectivity::ReachParams,
) -> Vec<Vec<u32>> {
    use rayon::prelude::*;
    fired
        .par_chunks(256)
        .map(|chunk| {
            let mut local = Vec::with_capacity(chunk.len() * k);
            for &src in chunk {
                scatter_one_source(src, buffers, grid, k, seed_lo, reach, &mut local);
            }
            local
        })
        .collect()
}

#[cfg(not(feature = "cpu-threads"))]
fn scatter_map(
    fired: &[u32],
    buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    k: usize,
    seed_lo: u32,
    reach: connectivity::ReachParams,
) -> Vec<Vec<u32>> {
    let mut local = Vec::with_capacity(fired.len() * k);
    for &src in fired {
        scatter_one_source(src, buffers, grid, k, seed_lo, reach, &mut local);
    }
    vec![local]
}

/// Decay the membrane snapshot for the uploaded range so silent neurons do not
/// keep stale subthreshold glow: `v_render[i] = v[i]*leak_decay^(tick-last_updated[i])`.
/// Matches the GPU render path (the WebGL shader reads `v_render` directly).
pub fn update_v_render(buffers: &mut CpuNeuronBuffers, current_tick: u32, params: &LifParams) {
    for i in 0..buffers.v.len() {
        let dormant = current_tick.wrapping_sub(buffers.last_updated[i]);
        let decay = if dormant == 0 {
            1.0
        } else {
            params.leak_decay.powi(dormant as i32)
        };
        buffers.v_render[i] = buffers.v[i] * decay;
    }
}
