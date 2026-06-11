//! Phase-2 native dynamics test: drive the REAL `GpuBackend` on a native device
//! (llvmpipe under WSL2) and assert the qualitative dynamics contract:
//!   - deep_sleep -> near silent,
//!   - focused -> non-zero, biologically plausible firing,
//!   - seizure -> strictly higher rate than focused,
//!   - no NaN membrane, no i32 overflow.
//!
//! Skips (does not fail) if no wgpu adapter is available. Native-only.

#![cfg(not(target_arch = "wasm32"))]

use brain_visualizer::sim::backend::{SimBackend, SimConfig};
use brain_visualizer::sim::gpu::GpuBackend;

const N: usize = 12_000;
const K: usize = 32;
// Tuning matching examples/sim_check.rs (documented in the phase-2 closeout).
const I_EXT: f32 = 0.040;
const SYN_SCALE: f32 = 0.03;
const HETEROGENEITY: f32 = 0.0;

fn mean_rate_hz(backend: &mut GpuBackend, excit: f32, warmup: u32, measure: u32) -> f32 {
    backend.set_i_ext(I_EXT);
    backend.set_synaptic_scale(SYN_SCALE);
    // This test guards the original dynamics envelope, independent of product
    // visual defaults. Heterogeneity 0.0 is the documented pre-V2 baseline.
    backend.set_heterogeneity(HETEROGENEITY);
    let cfg = SimConfig {
        n: N,
        k: K,
        ..SimConfig::default()
    };
    backend.initialize(&cfg);
    for _ in 0..warmup {
        backend.tick(1, excit);
    }
    let mut spikes: u64 = 0;
    for _ in 0..measure {
        spikes += backend.tick(1, excit).spikes;
    }
    let seconds = measure as f32 / 1000.0; // 1 ms/tick
    spikes as f32 / (N as f32 * seconds)
}

#[test]
fn gpu_excitability_sweep_and_no_overflow() {
    let ctx = match pollster::block_on(GpuBackend::acquire_native()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP gpu_sim_dynamics: {e}");
            return;
        }
    };
    let mut backend = GpuBackend::new(
        ctx,
        SimConfig {
            n: N,
            k: K,
            ..SimConfig::default()
        },
    );

    let sleep = mean_rate_hz(&mut backend, 0.10, 150, 300);
    let focused = mean_rate_hz(&mut backend, 0.55, 200, 400);
    let seizure = mean_rate_hz(&mut backend, 1.00, 200, 400);
    eprintln!(
        "[gpu-dynamics] deep_sleep={sleep:.2}Hz focused={focused:.2}Hz seizure={seizure:.2}Hz \
         max|I|_hw={}",
        backend.max_abs_current_hw
    );

    assert!(sleep < 1.0, "deep_sleep not near-silent: {sleep:.2} Hz");
    assert!(focused > 0.0, "focused produced no spikes");
    assert!(
        (1.0..40.0).contains(&focused),
        "focused rate implausible: {focused:.2} Hz"
    );
    assert!(
        seizure > focused,
        "seizure ({seizure:.2}) not above focused ({focused:.2})"
    );

    // Overflow guard (BV19): high-water max|current| must stay within i32.
    assert!(
        backend.max_abs_current_hw < i32::MAX as u32,
        "i32 current overflow at seizure: {}",
        backend.max_abs_current_hw
    );

    // NaN guard: debug snapshot reads back v and asserts no NaN internally.
    let (mean_v, frac) = backend.debug_dynamics_snapshot();
    assert!(mean_v.is_finite(), "mean_v not finite: {mean_v}");
    assert!(frac <= 1.0);
}
