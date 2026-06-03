// Phase 0 — Browser WebGPU/WASM microbench harness (STUB)
//
// This is a minimal compile-only stub. It is NOT run in CI (no browser).
// TODO (manual): Run `wasm-pack build --target web` and serve index.html
// to collect real browser WebGPU numbers for shipped tier cap validation.
//
// Shape when complete:
//   - WASM initializes WebGPU via wgpu/wasm path
//   - Allocates the same hot SoA buffers as the native bench
//   - Runs 300–1000 ticks per N/K point
//   - Reports ticks/sec, synaptic_events/sec, adapter limits,
//     timestamp support, and COOP/COEP / WASM threads status
//   - Output: JSON to browser console, parsed by test harness
//
// See phase-0-benchmark.md for the full spec.

use wasm_bindgen::prelude::*;

// BV22 hash — must match the WGSL and Rust native versions exactly.
#[inline(always)]
fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

#[inline(always)]
fn mix_key(neuron_id: u32, synapse_index: u32, salt: u32) -> u32 {
    let k = neuron_id
        .wrapping_add(synapse_index.wrapping_mul(0x9e3779b9))
        .wrapping_add(salt.wrapping_mul(0x6c62272e));
    hash32(k)
}

/// Minimal CPU-only microbench exposed to JS as a smoke test that the WASM
/// build is functional. Returns a JSON string with result.
///
/// The real WebGPU bench is a manual TODO — requires a browser with WebGPU.
#[wasm_bindgen]
pub fn run_cpu_microbench(n: u32, k: u32, ticks: u32) -> String {
    let n = n as usize;
    let k_u = k as usize;

    let mut v: Vec<f32> = vec![0.0; n];
    let mut i_buf: Vec<i32> = vec![0; n];

    // Seed
    for i in (0..n).step_by(20) {
        i_buf[i] = 245 * 20; // pre-charge with drive
    }

    let mut total_events: u64 = 0;

    for _tick in 0..ticks {
        // Drive injection
        for i in (0..n).step_by(20) {
            i_buf[i] += 245;
        }

        let mut fired: Vec<u32> = Vec::new();
        for idx in 0..n {
            let cur = i_buf[idx] as f32 / 4096.0;
            i_buf[idx] = 0;
            let new_v = v[idx] * 0.95 + cur;
            if new_v >= 1.0 {
                v[idx] = 0.0;
                fired.push(idx as u32);
            } else {
                v[idx] = new_v;
            }
        }

        total_events += fired.len() as u64 * k as u64;
        for &src in &fired {
            for j in 0..k_u {
                let tgt = (mix_key(src, j as u32, 1) as usize) % n;
                i_buf[tgt] += 205; // WEIGHT_FP
            }
        }
    }

    let avg_fired = if ticks > 0 { total_events / ticks as u64 / k as u64 } else { 0 };
    format!(
        r#"{{"status":"cpu_only_stub","n":{},"k":{},"ticks":{},"total_synaptic_events":{},"avg_fired_per_tick":{},"note":"WebGPU bench is manual TODO — no browser in build env"}}"#,
        n, k, ticks, total_events, avg_fired
    )
}

/// Placeholder for the real WebGPU bench entry point.
/// In production: request WebGPU adapter via web-sys, run GPU bench,
/// return JSON with adapter name, limits, and throughput numbers.
#[wasm_bindgen]
pub async fn run_webgpu_bench(_n: u32, _k: u32, _ticks: u32) -> String {
    // TODO: implement using wgpu with WebGPU backend
    // Steps:
    //   1. wgpu::Instance::new(InstanceDescriptor for WebGPU backend)
    //   2. request_adapter(HighPerformance)
    //   3. Allocate hot SoA buffers (v: f32, I: i32, last_spike: u32)
    //   4. Compile and run integrate + scatter shaders (same WGSL as native)
    //   5. Record wall time across ticks, derive ticks/s + synaptic_events/s
    //   6. Query adapter limits + timestamp_query availability
    //   7. Return JSON matching native bench output format
    r#"{"status":"not_implemented","reason":"manual TODO — run in a real browser with WebGPU support"}"#.to_string()
}
