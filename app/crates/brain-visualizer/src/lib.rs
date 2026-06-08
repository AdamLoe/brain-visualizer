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

    // ── Consolidation: GPU backend browser bridge (OD11 closed) ──────────────
    //
    // WasmGpuBackend wraps GpuBackend with browser surface management.
    // The rAF loop calls:
    //   app.tick(ticks, excitability)              — advance simulation
    //   app.set_lod_camera_distance(d)             — LOD blend distance
    //   app.render_frame(mvp[16], right[3], up[3], — render to canvas surface
    //                   eye[3], dist, glow_tau, point_radius)
    //   app.stimulate(x, y, z, radius, current)   — cursor excitation
    //   app.resize(w, h)                           — on canvas resize
    //   app.destroy()                              — on backend teardown
    //
    // Created via: `const app = await WasmGpuBackend.create(canvas, n, k, seed, i_ext, syn)`
    // Returns a JS Promise<WasmGpuBackend>.

    use crate::sim::gpu::{GpuBackend, VisualSettings};
    use wasm_bindgen_futures::future_to_promise;

    /// Browser GPU backend. Own the wgpu surface; delegates all sim/render to
    /// the native-tested GpuBackend.  Created by the async `WasmGpuBackend.create()`.
    #[wasm_bindgen]
    pub struct WasmGpuBackend {
        inner:          GpuBackend,
        surface:        wgpu::Surface<'static>,
        surface_format: wgpu::TextureFormat,
        width:          u32,
        height:         u32,
    }

    #[wasm_bindgen]
    impl WasmGpuBackend {
        /// Async factory. Returns `Promise<WasmGpuBackend>`.
        ///
        /// Call from TypeScript:
        /// ```ts
        /// const app = await WasmGpuBackend.create(canvas, n, k, seed, iExt, synScale);
        /// ```
        pub fn create(
            canvas:         web_sys::HtmlCanvasElement,
            n:              usize,
            k:              usize,
            seed:           u32,
            i_ext:          f32,
            synaptic_scale: f32,
        ) -> js_sys::Promise {
            future_to_promise(async move {
                // Acquire WebGPU device + configure canvas surface.
                let (ctx, surface, fmt) =
                    GpuBackend::acquire_web(canvas)
                        .await
                        .map_err(|e| JsValue::from_str(&format!("[gpu] acquire_web: {e}")))?;

                // Retrieve surface dimensions from the already-committed config.
                let surf_config = surface.get_configuration();
                let (w, h) = surf_config
                    .map(|c| (c.width, c.height))
                    .unwrap_or((800, 600));

                let config = SimConfig {
                    n,
                    k,
                    seed: seed as u64,
                    i_ext,
                    backend: crate::sim::backend::BackendKind::Gpu,
                    ..SimConfig::default()
                };

                // Build GpuBackend (pipelines, layouts) — same path as native.
                let mut inner = GpuBackend::new(ctx, config.clone());
                inner.set_i_ext(i_ext);
                inner.set_synaptic_scale(synaptic_scale);

                // Upload manifold + allocate GPU buffers.
                inner.initialize(&config);

                // Build render pipelines for the surface format.
                inner.build_render_pipelines(fmt);

                // Size the depth texture to match the surface.
                inner.resize_render_targets(w, h);

                web_sys::console::log_1(
                    &format!("[gpu] WasmGpuBackend ready: N={n} K={k} size={w}×{h}")
                        .into(),
                );

                let backend = WasmGpuBackend {
                    inner,
                    surface,
                    surface_format: fmt,
                    width: w,
                    height: h,
                };

                // wasm-bindgen requires JsValue; wrap the struct.
                Ok(JsValue::from(backend))
            })
        }

        // ── Per-frame API ────────────────────────────────────────────────────

        /// Advance `ticks` simulation sub-steps at the given `excitability`.
        /// Returns spike count for the batch (f64 for JS number compat).
        pub fn tick(&mut self, ticks: u32, excitability: f32) -> f64 {
            self.inner.tick(ticks, excitability).spikes as f64
        }

        /// Set the camera-to-surface distance for LOD blend.
        pub fn set_lod_camera_distance(&mut self, d: f32) {
            self.inner.set_lod_camera_distance(d);
        }

        // ── V2 Phase 0: settings push ────────────────────────────────────────

        /// Push a full settings snapshot from JS.  Called once after backend
        /// creation and again whenever the settings store changes.  The
        /// Float32Array layout matches the canonical 24-element contract in
        /// web/settings.ts `toFloat32Array`.
        pub fn update_settings(&mut self, data: &[f32]) {
            let v = VisualSettings::from_slice(data);
            self.inner.set_visual_settings(v);
        }

        /// v0.3.1: push a morphology-config JSON blob from the dev panel. Shape is
        /// `{ generator: {...}, renderQuality: {...}, lighting: {...} }` per the
        /// Config Contract. The Rust side diffs vs the current config and runs the
        /// narrowest update (uniform-only lighting, generator regenerate, and/or
        /// render-pipeline rebuild). This is a SEPARATE path from update_settings —
        /// it does NOT touch the VisualSettings Float32Array contract. Logs and
        /// ignores malformed JSON (no panic across the WASM boundary).
        pub fn set_morphology_config(&mut self, json: &str) {
            if let Err(e) = self.inner.set_morphology_config(json) {
                web_sys::console::warn_1(
                    &format!("[gpu] set_morphology_config: ignoring bad config: {e}").into(),
                );
            }
        }

        /// Return the length-33 metrics Vec (Float32Array on the JS side).
        /// Layout matches METRICS_LAYOUT + the 16-bin voltage histogram in
        /// web/settings.ts.  V2 Phase A: sourced from the GPU reduction pass +
        /// non-blocking async readback (see GpuBackend::metrics_snapshot).
        pub fn metrics(&self) -> Vec<f32> {
            self.inner.metrics_snapshot().to_vec()
        }

        /// Render one frame to the canvas surface.
        ///
        /// `mvp` — column-major 4×4 MVP float array (length 16)
        /// `right_x/y/z` — camera-right unit vector
        /// `up_x/y/z`    — camera-up unit vector
        /// `eye_x/y/z`   — camera eye position (world space)
        /// `camera_dist` — camera distance from origin
        ///
        /// glow_tau and point_radius are sourced from the VisualSettings pushed
        /// via update_settings() (V2 Phase 0 — removed from per-frame args).
        ///
        /// No-op if surface texture acquisition fails (e.g. window is minimized).
        #[allow(clippy::too_many_arguments)]
        pub fn render_frame(
            &mut self,
            mvp:          &[f32],
            right_x: f32, right_y: f32, right_z: f32,
            up_x:    f32, up_y:    f32, up_z:    f32,
            eye_x:   f32, eye_y:   f32, eye_z:   f32,
            camera_dist:  f32,
        ) {
            // Acquire surface texture.
            let surface_tex = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) |
                wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                _ => return, // surface lost / timeout
            };
            let view = surface_tex.texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Copy the MVP slice into a fixed-size array.
            if mvp.len() < 16 { return; }
            let mut mvp_arr = [0f32; 16];
            mvp_arr.copy_from_slice(&mvp[..16]);

            // V2 Phase 0: source glow_tau and point_radius from VisualSettings
            // (pushed via update_settings) rather than per-frame scalar args.
            let glow_tau     = self.inner.visual().glow_tau;
            let point_radius = self.inner.visual().point_radius;

            self.inner.render_full(
                &view,
                &mvp_arr,
                [right_x, right_y, right_z],
                [up_x,    up_y,    up_z   ],
                glow_tau,
                point_radius,
                [eye_x, eye_y, eye_z],
                camera_dist,
            );

            surface_tex.present();
        }

        /// Inject cursor excitation near world-space position `(x,y,z)`.
        pub fn stimulate(&mut self, x: f32, y: f32, z: f32, radius: f32, current: f32) {
            self.inner.stimulate([x, y, z], radius, current);
        }

        /// Resize the depth texture + reconfigure the surface on canvas resize.
        pub fn resize(&mut self, width: u32, height: u32) {
            let w = width.max(1);
            let h = height.max(1);
            if w == self.width && h == self.height { return; }
            self.width  = w;
            self.height = h;

            // Reconfigure the surface at the new size.
            self.surface.configure(
                self.inner.device(),
                &wgpu::SurfaceConfiguration {
                    usage:   wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format:  self.surface_format,
                    width:   w,
                    height:  h,
                    present_mode: wgpu::PresentMode::Fifo,
                    alpha_mode:   wgpu::CompositeAlphaMode::Auto,
                    view_formats: vec![],
                    desired_maximum_frame_latency: 2,
                },
            );

            // Resize depth texture to match.
            self.inner.resize_render_targets(w, h);
        }

        /// Reinitialize with a new neuron/connectivity count (adaptive scaler).
        /// Keeps the same surface; rebuilds all GPU buffers.
        pub fn reinitialize(&mut self, n: usize, k: usize, seed: u32, i_ext: f32, synaptic_scale: f32) {
            let config = SimConfig {
                n,
                k,
                seed: seed as u64,
                i_ext,
                backend: crate::sim::backend::BackendKind::Gpu,
                ..SimConfig::default()
            };
            self.inner.set_i_ext(i_ext);
            self.inner.set_synaptic_scale(synaptic_scale);
            self.inner.initialize(&config);
            self.inner.build_render_pipelines(self.surface_format);
            self.inner.resize_render_targets(self.width, self.height);
        }

        /// Release all GPU resources (BV16 teardown before a backend/tier restart).
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
