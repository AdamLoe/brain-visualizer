//! Brain Visualizer — Rust crate (compiles to WASM for the browser; pure logic
//! also compiles & unit-tests on the host).
//!
//! Module layout follows architecture §10.1: sim, connectivity, manifold,
//! buffers, profiler, and gpu_limits are separate; nothing is a god-object.
//!
//! Dual-target: all pure logic builds on the host (`cargo build` / `cargo
//! test`). The wasm-only glue (`#[wasm_bindgen]` entry points, DOM/Worker code)
//! is gated behind `#[cfg(target_arch = "wasm32")]`.

pub mod buffers;
pub mod connectivity;
pub mod gpu_limits;
pub mod manifold;
pub mod profiler;
pub mod sim;

use crate::manifold::{Manifold, ManifoldParams, RegionKind};
use crate::sim::backend::SimConfig;

/// Build the manifold for a config. Pure, host-callable; the wasm entry point
/// and `cargo test` both go through this so there is one code path.
pub fn build_manifold(config: &SimConfig) -> Manifold {
    let params = ManifoldParams::new(config.n, config.seed_lo());
    Manifold::generate(&params)
}

/// Region split summary (Input, Association, Output) — used for startup logging
/// and as a host-testable sanity surface.
pub fn region_split(manifold: &Manifold) -> (usize, usize, usize) {
    let mut input = 0;
    let mut assoc = 0;
    let mut output = 0;
    for r in &manifold.neuron_regions {
        match r {
            RegionKind::Input => input += 1,
            RegionKind::Association => assoc += 1,
            RegionKind::Output => output += 1,
        }
    }
    (input, assoc, output)
}

// ---------------------------------------------------------------------------
// wasm entry points (browser only)
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
mod wasm_entry {
    use super::*;
    use wasm_bindgen::prelude::*;

    /// Install the panic hook so Rust panics surface in the browser console.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        web_sys::console::log_1(&"[brain-visualizer] wasm module loaded".into());
    }

    /// Generate the manifold and log neuron count + region split. Returns the
    /// neuron count so `main.ts` can confirm geometry exists (phase-1 "done
    /// when": neuron positions exist even though nothing draws them).
    #[wasm_bindgen]
    pub fn init_manifold(n: usize, seed: u32) -> usize {
        let config = SimConfig {
            n,
            seed: seed as u64,
            ..SimConfig::default()
        };
        let manifold = build_manifold(&config);
        let (input, assoc, output) = region_split(&manifold);
        web_sys::console::log_1(
            &format!(
                "[manifold] verts={} faces={} neurons={} regions(I/A/O)={}/{}/{}",
                manifold.vertices.len(),
                manifold.faces.len(),
                manifold.neuron_positions.len(),
                input,
                assoc,
                output
            )
            .into(),
        );
        manifold.neuron_positions.len()
    }

    // ── Phase 6: CPU backend (event-driven LIF on the coordinator worker) ────
    //
    // The coordinator Web Worker (web/cpu-worker.ts) owns this instance + the
    // rayon pool (under `cpu-threads`) and writes the SoA into the wasm linear
    // memory (a SharedArrayBuffer when threads are enabled). The main thread
    // reads `v_render` + `last_spike` views (by pointer/len) each frame and
    // uploads them to WebGL2 (web/cpu-renderer.ts). BV24 ownership boundary.
    use crate::sim::backend::SimBackend;
    use crate::sim::cpu::CpuBackend;

    /// Thin wasm wrapper around the native-tested `CpuBackend`.
    #[wasm_bindgen]
    pub struct WasmCpuBackend {
        inner: CpuBackend,
    }

    #[wasm_bindgen]
    impl WasmCpuBackend {
        /// Create + initialize the CPU backend for `n`/`k`/`seed`. Same seed as
        /// the GPU backend → identical network (BV16 restart semantics).
        #[wasm_bindgen(constructor)]
        pub fn new(n: usize, k: usize, seed: u32, i_ext: f32, synaptic_scale: f32) -> WasmCpuBackend {
            let config = SimConfig {
                n,
                k,
                seed: seed as u64,
                i_ext,
                backend: crate::sim::backend::BackendKind::Cpu,
                ..SimConfig::default()
            };
            let mut inner = CpuBackend::new(config.clone());
            inner.set_i_ext(i_ext);
            inner.set_synaptic_scale(synaptic_scale);
            inner.initialize(&config);
            WasmCpuBackend { inner }
        }

        /// Advance `ticks` sub-ticks at `excitability`. Returns spikes this batch
        /// (the worker posts a richer stats object separately if needed).
        pub fn tick(&mut self, ticks: u32, excitability: f32) -> f64 {
            self.inner.tick(ticks, excitability).spikes as f64
        }

        /// Inject current near `pos` (world space) within `radius`.
        pub fn stimulate(&mut self, x: f32, y: f32, z: f32, radius: f32, current: f32) {
            self.inner.stimulate([x, y, z], radius, current);
        }

        pub fn neuron_count(&self) -> usize {
            self.inner.config().n
        }

        pub fn tick_count(&self) -> u32 {
            self.inner.tick_count()
        }

        /// Pointer to the decayed render-voltage array (`f32`, len = neuron_count).
        /// JS builds a `Float32Array(memory.buffer, ptr, n)` view for WebGL upload.
        pub fn v_render_ptr(&self) -> *const f32 {
            match self.inner.render_state() {
                crate::sim::backend::RenderState::Cpu { v_render, .. } => v_render.as_ptr(),
                _ => std::ptr::null(),
            }
        }

        /// Pointer to the packed `last_spike` array (`u32`, len = neuron_count).
        pub fn last_spike_ptr(&self) -> *const u32 {
            match self.inner.render_state() {
                crate::sim::backend::RenderState::Cpu { last_spike, .. } => last_spike.as_ptr(),
                _ => std::ptr::null(),
            }
        }

        /// Pointer to neuron positions (`f32` xyz triples, len = 3*neuron_count).
        pub fn positions_ptr(&self) -> *const f32 {
            match self.inner.render_state() {
                crate::sim::backend::RenderState::Cpu { positions, .. } => positions.as_ptr().cast(),
                _ => std::ptr::null(),
            }
        }

        /// Release all SoA state (BV16 teardown before a backend/tier restart).
        pub fn destroy(&mut self) {
            self.inner.destroy();
        }
    }

    /// Initialize the `wasm-bindgen-rayon` thread pool (threaded wasm build only).
    /// Returns a JS `Promise` the worker awaits before running the sim. Only
    /// available under the `cpu-threads` feature + a threaded wasm build (nightly
    /// + `+atomics,+bulk-memory` + `build-std`); otherwise the CPU backend runs
    /// single-threaded (still correct) and this symbol is absent.
    #[cfg(feature = "cpu-threads")]
    #[wasm_bindgen]
    pub fn init_cpu_thread_pool(num_threads: usize) -> js_sys::Promise {
        wasm_bindgen_rayon::init_thread_pool(num_threads)
    }

    /// Report cross-origin isolation status (SharedArrayBuffer availability) to
    /// the console — part of the phase-1 startup log (COOP/COEP check).
    #[wasm_bindgen]
    pub fn log_cross_origin_isolation(isolated: bool) {
        web_sys::console::log_1(
            &format!("[coi] crossOriginIsolated={isolated} (SharedArrayBuffer {})",
                if isolated { "available" } else { "UNAVAILABLE" })
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_manifold_via_config() {
        let c = SimConfig {
            n: 4000,
            ..SimConfig::default()
        };
        let m = build_manifold(&c);
        assert_eq!(m.neuron_positions.len(), 4000);
        let (i, a, o) = region_split(&m);
        assert_eq!(i + a + o, 4000);
    }
}
