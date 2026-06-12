//! Fixed-point current overflow gate for worst-case synchronized scatter.
//!
//! The production shader still uses plain i32 atomicAdd plus max-current
//! high-water instrumentation. This gate makes that policy executable by forcing
//! a full-network synchronous spike and requiring a large margin below i32::MAX.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use brain_visualizer::sim::backend::{SimBackend, SimConfig, PRODUCT_MAX_N};
use brain_visualizer::sim::gpu::GpuBackend;

const N: usize = PRODUCT_MAX_N;
const K: usize = 128;
const FIRING_CURRENT: f32 = 2.0;
const OVERFLOW_MARGIN: u32 = (i32::MAX as u32) / 16;

#[test]
fn synchronized_scatter_current_stays_below_i32_margin() {
    let Some(ctx) = pollster::block_on(common::acquire_native_context_or_skip(
        "gpu_current_overflow",
    )) else {
        return;
    };
    let cfg = SimConfig {
        n: N,
        k: K,
        ..SimConfig::default()
    };
    let mut backend = GpuBackend::new(ctx, cfg.clone());
    let state = backend.begin_initialize(&cfg);
    backend.initialize_neuron_buffers(&state);
    backend.finish_initialize();
    backend.set_i_ext(0.0);
    backend.set_input_mode(5);
    backend.set_heterogeneity(0.0);
    backend.set_weight_normalization(0);
    backend.set_synaptic_scale(1.0);

    let current_fp = (FIRING_CURRENT * cfg.fixed_point_scale as f32) as i32;
    let current = vec![current_fp; N];
    let i_current = &backend
        .resources()
        .neuron_buffers
        .as_ref()
        .expect("neuron buffers")
        .i_current
        .chunks[0];
    backend
        .queue()
        .write_buffer(i_current, 0, bytemuck::cast_slice(&current));

    let stats = backend.tick(1, 1.0);
    let high_water = backend.max_abs_current_hw();

    eprintln!(
        "[current-overflow] n={N} k={K} spikes={} max|I|={} margin={} i32_max={}",
        stats.spikes,
        high_water,
        OVERFLOW_MARGIN,
        i32::MAX
    );

    assert!(
        stats.spikes >= (N as u64 * 95) / 100,
        "synchronized stress did not fire enough neurons: {}/{}",
        stats.spikes,
        N
    );
    assert!(
        high_water > (K as u32 * 4096),
        "stress did not accumulate enough current to exercise scatter: {high_water}"
    );
    assert!(
        high_water < OVERFLOW_MARGIN,
        "fixed-point current high-water {high_water} exceeds margin {OVERFLOW_MARGIN}"
    );
}
