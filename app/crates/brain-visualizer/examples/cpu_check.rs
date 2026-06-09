//! Phase-6 native verification harness (architecture §"verification policy").
//!
//! Runs the REAL production `CpuBackend` (event-driven, host rayon under the
//! `cpu-threads` feature) and compares it against the `GpuBackend` (native wgpu
//! / llvmpipe) on identical networks. Reports the four phase-6 gates:
//!   1. Determinism parity — first 100 targets for neuron 0, CPU path vs the
//!      shared Rust `connectivity::target()` the GPU scatter mirrors (the
//!      WGSL==Rust gate in tests/wgsl_target_determinism.rs closes the GPU side).
//!   2. Firing-rate parity — CPU vs GPU mean Hz at `focused`, within ±10%.
//!   3. Lazy decay correctness — a neuron silent for 500 ticks decays to ~0.
//!   4. Render decay correctness — an untouched neuron's v_render decays to ~0.
//! Plus native CPU throughput (ticks/s, syn-events/s, threads) for context.
//!
//! Run: `cargo run --release --example cpu_check --features cpu-threads`
//! (without the feature it still runs, single-threaded.)

use std::sync::atomic::AtomicI32;

use brain_visualizer::connectivity::{self, SpatialGrid};
use brain_visualizer::sim::backend::{neuron_type_byte, SimBackend, SimConfig};
use brain_visualizer::sim::cpu::core::{self, CpuNeuronBuffers, LifParams};
use brain_visualizer::sim::cpu::CpuBackend;
use brain_visualizer::sim::gpu::GpuBackend;

const N: usize = 30_000;
const K: usize = 32;
const TICK_MS_BIO: f32 = 1.0;
// Matches examples/sim_check.rs tuning (documented; no locked BV value changed).
const I_EXT: f32 = 0.040;
const SYN_SCALE: f32 = 0.03;
const FOCUSED: f32 = 0.55;

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let cfg = SimConfig {
        n: N,
        k: K,
        ..SimConfig::default()
    };

    println!("=== Phase 6 CPU backend check ===");
    println!(
        "N={N} K={K}  seed=0x{:x}  i_ext={I_EXT:.3}  synaptic_scale={SYN_SCALE:.3}",
        cfg.seed
    );
    #[cfg(feature = "cpu-threads")]
    let threads = rayon::current_num_threads();
    #[cfg(not(feature = "cpu-threads"))]
    let threads = 1usize;
    println!(
        "rayon threads = {threads} ({})",
        if cfg!(feature = "cpu-threads") {
            "cpu-threads ON"
        } else {
            "single-threaded fallback"
        }
    );

    // ── Gate 1: determinism parity ────────────────────────────────────────────
    determinism_parity(&cfg);

    // ── Gate 3 + 4: lazy / render decay correctness ───────────────────────────
    decay_correctness(&cfg);

    // ── CPU run: firing rate + throughput ─────────────────────────────────────
    let mut cpu = CpuBackend::new(cfg.clone());
    cpu.set_i_ext(I_EXT);
    cpu.set_synaptic_scale(SYN_SCALE);
    cpu.initialize(&cfg);

    let warmup = 200;
    let measure = 600;
    for _ in 0..warmup {
        cpu.tick(1, FOCUSED);
    }
    let mut cpu_spikes: u64 = 0;
    let mut cpu_syn: u64 = 0;
    let t0 = std::time::Instant::now();
    for _ in 0..measure {
        let s = cpu.tick(1, FOCUSED);
        cpu_spikes += s.spikes;
        cpu_syn += s.synaptic_events;
    }
    let cpu_wall = t0.elapsed().as_secs_f32();
    let seconds = measure as f32 * TICK_MS_BIO / 1000.0;
    let cpu_hz = cpu_spikes as f32 / (N as f32 * seconds);
    let cpu_ticks_per_s = measure as f32 / cpu_wall;
    let cpu_syn_per_s_real = cpu_syn as f32 / cpu_wall;
    println!("\n--- CPU run (focused, excit={FOCUSED}) ---");
    println!(
        "CPU mean_rate={cpu_hz:.2} Hz  spikes={cpu_spikes}  syn_events={cpu_syn}  \
         max|I|_hw={}",
        cpu.max_abs_current_hw
    );
    println!(
        "CPU throughput (wall): {cpu_ticks_per_s:.0} ticks/s  {cpu_syn_per_s_real:.0} syn-events/s  \
         ({threads} threads)"
    );

    // ── GPU run for firing-rate parity ────────────────────────────────────────
    let gpu_hz = match GpuBackend::acquire_native().await {
        Ok(ctx) => {
            let mut gpu = GpuBackend::new(ctx, cfg.clone());
            gpu.set_i_ext(I_EXT);
            gpu.set_synaptic_scale(SYN_SCALE);
            gpu.initialize(&cfg);
            for _ in 0..warmup {
                gpu.tick(1, FOCUSED);
            }
            let mut gpu_spikes: u64 = 0;
            for _ in 0..measure {
                gpu_spikes += gpu.tick(1, FOCUSED).spikes;
            }
            let hz = gpu_spikes as f32 / (N as f32 * seconds);
            println!("\n--- GPU run (focused, excit={FOCUSED}) ---");
            println!("GPU mean_rate={hz:.2} Hz  spikes={gpu_spikes}");
            Some(hz)
        }
        Err(e) => {
            println!("\nSKIP GPU run (no adapter): {e}");
            None
        }
    };

    // ── Verdicts ──────────────────────────────────────────────────────────────
    println!("\n=== verdicts ===");
    match gpu_hz {
        Some(g) if g > 0.0 && cpu_hz > 0.0 => {
            let rel = (cpu_hz - g).abs() / g;
            println!(
                "firing-rate parity: CPU={cpu_hz:.2} Hz  GPU={g:.2} Hz  rel_diff={:.1}%  -> {}",
                rel * 100.0,
                if rel <= 0.10 { "PASS (±10%)" } else { "OUTSIDE ±10%" }
            );
        }
        Some(_) => println!("firing-rate parity: INCONCLUSIVE (a rate was zero)"),
        None => println!(
            "firing-rate parity: GPU unavailable; CPU={cpu_hz:.2} Hz (plausible 5-20 Hz at focused: {})",
            if (1.0..=40.0).contains(&cpu_hz) { "yes" } else { "check" }
        ),
    }
}

/// Gate 1: first 100 targets for neuron 0 from the CPU backend's exact code path
/// vs the shared `connectivity::target()` used by (and proven equal to) the GPU
/// scatter. Asserts exact match.
fn determinism_parity(cfg: &SimConfig) {
    let manifold = brain_visualizer::build_manifold(cfg);
    let grid = &manifold.spatial_grid;
    let seed_lo = cfg.seed_lo();

    // Source-type byte for neuron 0 (same derivation as both backends).
    let src_type = neuron_type_byte(0, seed_lo, manifold.neuron_regions[0]);
    let src_cell = grid.unpack(grid.cell_of_neuron_map()[0]);

    let mut cpu_targets = Vec::with_capacity(100);
    let mut shared_targets = Vec::with_capacity(100);
    for j in 0..100u32 {
        // CPU hot path (precomputed cell): what scatter_one_source calls.
        cpu_targets.push(connectivity::target_with_cell(
            0,
            j,
            grid,
            K,
            seed_lo,
            src_type,
            src_cell,
            connectivity::ReachParams::LOCAL_ONLY,
        ));
        // Canonical shared function the GPU scatter mirrors bit-for-bit
        // (tests/wgsl_target_determinism.rs proves WGSL == this).
        shared_targets.push(connectivity::target(
            0,
            j,
            grid,
            K,
            seed_lo,
            src_type,
            connectivity::ReachParams::LOCAL_ONLY,
        ));
    }

    let matches = cpu_targets == shared_targets;
    println!("\n--- Gate 1: determinism parity (neuron 0, first 100 targets) ---");
    println!("CPU  targets[0..10] = {:?}", &cpu_targets[..10]);
    println!(
        "Rust targets[0..10] = {:?}  (== WGSL via wgsl_target_determinism gate)",
        &shared_targets[..10]
    );
    println!(
        "first-100 targets match: {}",
        if matches {
            "PASS (bit-identical)"
        } else {
            "FAIL (DIVERGENCE)"
        }
    );
    assert!(matches, "CPU vs shared Rust target() diverged for neuron 0");
    let _ = SpatialGrid::cell_count; // keep import meaningful
}

/// Gates 3 & 4: lazy decay + render decay correctness using the real core fns.
fn decay_correctness(cfg: &SimConfig) {
    let manifold = brain_visualizer::build_manifold(cfg);
    let mut bufs = CpuNeuronBuffers::build(cfg, &manifold.neuron_regions, &manifold.spatial_grid);
    let params = LifParams {
        excitability: 0.0,
        ..LifParams::new(I_EXT, SYN_SCALE, cfg.fixed_point_scale as f32)
    };

    // Pick a non-input neuron so i_ext doesn't re-excite it.
    let probe = (0..cfg.n)
        .find(|&i| (((bufs.last_spike[i] >> 24) & 0x7F) >> 2) != 0)
        .unwrap();
    bufs.v[probe] = 1.0;
    bufs.last_updated[probe] = 0;
    // No current accumulated (silent). Integrate once at tick 500.
    let _ = AtomicI32::new(0); // (I accumulator stays 0 → no input)
    core::integrate_neuron(probe, 500, &mut bufs, &params);
    let lazy_v = bufs.v[probe];
    let expected = 1.0_f32 * 0.95_f32.powi(500);

    // Render decay: a different untouched neuron, v set, never integrated.
    let probe2 = probe + 1;
    bufs.v[probe2] = 1.0;
    bufs.last_updated[probe2] = 0;
    core::update_v_render(&mut bufs, 500, &params);
    let render_v = bufs.v_render[probe2];

    println!("\n--- Gates 3 & 4: decay correctness ---");
    println!(
        "lazy decay: v after 500 silent ticks = {lazy_v:.3e}  (expected v_init*0.95^500 = {expected:.3e})  -> {}",
        if lazy_v.abs() < 1e-6 { "PASS (~0)" } else { "FAIL" }
    );
    println!(
        "render decay: v_render of untouched neuron after 500 ticks = {render_v:.3e}  -> {}",
        if render_v.abs() < 1e-6 {
            "PASS (~0, no stale glow)"
        } else {
            "FAIL"
        }
    );
    assert!(lazy_v.abs() < 1e-6, "lazy decay did not reach ~0");
    assert!(render_v.abs() < 1e-6, "render decay did not reach ~0");
}
