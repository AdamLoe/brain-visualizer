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

use crate::manifold::{Manifold, ManifoldParams, RegionAssignmentMode, RegionKind};
use crate::sim::backend::SimConfig;

/// Build the manifold for a config. Pure, host-callable; the wasm entry point
/// and `cargo test` both go through this so there is one code path.
pub fn build_manifold(config: &SimConfig) -> Manifold {
    build_manifold_with_region_assignment(config, RegionAssignmentMode::HashRandom)
}

/// Build the manifold with an explicit region-assignment mode.
pub fn build_manifold_with_region_assignment(
    config: &SimConfig,
    region_assignment: RegionAssignmentMode,
) -> Manifold {
    let params =
        ManifoldParams::new(config.n, config.seed_lo()).with_region_assignment(region_assignment);
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
    use crate::sim::backend::{clamp_neuron_count, SimBackend};
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
        let n = clamp_neuron_count(n);
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

    use crate::sim::gpu::{
        morph_params_from_config_and_visual, reach_from_visual_settings, GpuBackend,
        NetworkBuildState, PreparedNetworkBuild, VisualSettings, PREPARED_NETWORK_VERSION,
    };
    use wasm_bindgen_futures::future_to_promise;

    #[wasm_bindgen]
    pub struct WasmPreparedNetwork {
        inner: PreparedNetworkBuild,
    }

    #[wasm_bindgen]
    impl WasmPreparedNetwork {
        pub fn version(&self) -> u32 {
            PREPARED_NETWORK_VERSION
        }

        pub fn n(&self) -> usize {
            self.inner.config().n
        }

        pub fn k(&self) -> usize {
            self.inner.config().k
        }

        pub fn seed(&self) -> u32 {
            self.inner.config().seed_lo()
        }

        pub fn grid_dim(&self) -> u32 {
            self.inner.grid_dim()
        }

        pub fn grid_cell_size(&self) -> f32 {
            self.inner.grid_cell_size()
        }

        pub fn dropped_count(&self) -> usize {
            self.inner.dropped_count()
        }

        pub fn positions(&self) -> Vec<f32> {
            self.inner.positions_f32()
        }

        pub fn region_codes(&self) -> Vec<u8> {
            self.inner.region_codes()
        }

        pub fn vertices(&self) -> Vec<f32> {
            self.inner.vertices_f32()
        }

        pub fn faces(&self) -> Vec<u32> {
            self.inner.faces_u32()
        }

        pub fn grid_min(&self) -> Vec<f32> {
            self.inner.grid_min_f32()
        }

        pub fn grid_cell_start(&self) -> Vec<u32> {
            self.inner.grid_cell_start_u32()
        }

        pub fn grid_cell_neurons(&self) -> Vec<u32> {
            self.inner.grid_cell_neurons_u32()
        }

        pub fn segment_endpoints(&self) -> Vec<f32> {
            self.inner.segment_endpoints_f32()
        }

        pub fn segment_path_len(&self) -> Vec<f32> {
            self.inner.segment_path_len_f32()
        }

        pub fn segment_neuron_ids(&self) -> Vec<u32> {
            self.inner.segment_neuron_ids_u32()
        }

        pub fn segment_kinds(&self) -> Vec<u32> {
            self.inner.segment_kinds_u32()
        }

        pub fn segment_target_ids(&self) -> Vec<u32> {
            self.inner.segment_target_ids_u32()
        }

        pub fn sphere_geometry(&self) -> Vec<f32> {
            self.inner.sphere_geometry_f32()
        }

        pub fn sphere_neuron_ids(&self) -> Vec<u32> {
            self.inner.sphere_neuron_ids_u32()
        }

        pub fn sphere_kinds(&self) -> Vec<u32> {
            self.inner.sphere_kinds_u32()
        }

        pub fn stats_json(&self) -> String {
            self.inner.stats_json()
        }

        pub fn params_json(&self) -> String {
            self.inner.params_json()
        }
    }

    #[wasm_bindgen]
    pub fn prepare_network_payload(
        n: usize,
        k: usize,
        seed: u32,
        visual_settings: &[f32],
        morph_config_json: &str,
        region_assignment_mode: &str,
        // Boot-load overhaul: optional `(phase_label, fraction 0..1)` progress
        // callback fired at each payload-build phase boundary. The worker
        // `postMessage`s each tick to the (non-blocked) main thread so the
        // "Prepare network payload" overlay percent climbs with real work
        // instead of a synthetic creep. Optional so an older web layer (or a
        // stale `.d.ts`) that passes nothing still works.
        progress: Option<js_sys::Function>,
    ) -> Result<WasmPreparedNetwork, JsValue> {
        let n = clamp_neuron_count(n);
        let visual = VisualSettings::from_slice(visual_settings);
        let morph_config =
            crate::sim::morphology::MorphologyConfig::from_json(morph_config_json)
                .map_err(|e| JsValue::from_str(&format!("[gpu-build] bad morph config: {e}")))?;
        let params = morph_params_from_config_and_visual(&morph_config, &visual);
        let reach = reach_from_visual_settings(&visual);
        let region_assignment = region_assignment_mode_from_str(region_assignment_mode);
        let config = SimConfig {
            n,
            k,
            seed: seed as u64,
            i_ext: visual.i_ext,
            backend: crate::sim::backend::BackendKind::Gpu,
            ..SimConfig::default()
        };
        let bridge = progress.map(|cb| {
            move |label: &str, frac: f32| {
                let _ = cb.call2(
                    &JsValue::NULL,
                    &JsValue::from_str(label),
                    &JsValue::from_f64(frac as f64),
                );
            }
        });
        let inner = PreparedNetworkBuild::prepare_with_progress(
            config,
            params,
            reach,
            region_assignment,
            bridge.as_ref().map(|f| f as &dyn Fn(&str, f32)),
        );
        Ok(WasmPreparedNetwork { inner })
    }

    /// Browser GPU backend. Own the wgpu surface; delegates all sim/render to
    /// the native-tested GpuBackend.  Created by the async `WasmGpuBackend.create()`.
    #[wasm_bindgen]
    pub struct WasmGpuBackend {
        inner: GpuBackend,
        surface: wgpu::Surface<'static>,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        pending_network: Option<NetworkBuildState>,
        /// Boot-load overhaul (Workstream B): optional one-way `(label, fraction)`
        /// progress callback for the compile-heavy startup stages. Set by the web
        /// layer via `set_progress_callback` after `create_staged`. Always
        /// optional so a stale-vs-regenerated `.d.ts` mismatch can't break boot.
        progress_cb: Option<js_sys::Function>,
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
            canvas: web_sys::HtmlCanvasElement,
            n: usize,
            k: usize,
            seed: u32,
            i_ext: f32,
            synaptic_scale: f32,
        ) -> js_sys::Promise {
            future_to_promise(async move {
                // Acquire WebGPU device + configure canvas surface.
                let (ctx, surface, fmt) = GpuBackend::acquire_web(canvas, None)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("[gpu] acquire_web: {e}")))?;

                // Retrieve surface dimensions from the already-committed config.
                let surf_config = surface.get_configuration();
                let (w, h) = surf_config
                    .map(|c| (c.width, c.height))
                    .unwrap_or((800, 600));

                let n = clamp_neuron_count(n);
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
                    &format!("[gpu] WasmGpuBackend ready: N={n} K={k} size={w}×{h}").into(),
                );

                let backend = WasmGpuBackend {
                    inner,
                    surface,
                    surface_format: fmt,
                    width: w,
                    height: h,
                    pending_network: None,
                    progress_cb: None,
                };

                // wasm-bindgen requires JsValue; wrap the struct.
                Ok(JsValue::from(backend))
            })
        }

        /// Async staged factory. This acquires WebGPU and builds the core
        /// compute backend, but intentionally leaves network/resource
        /// initialization to the explicit `startup_*` methods below so JS can
        /// yield between expensive stages and update real progress.
        pub fn create_staged(
            canvas: web_sys::HtmlCanvasElement,
            n: usize,
            k: usize,
            seed: u32,
            i_ext: f32,
            synaptic_scale: f32,
            // Boot-load overhaul (Workstream B): optional `(label, fraction)`
            // progress callback for the GPU-acquire sub-stages. Optional so an
            // older web layer that passes nothing still works.
            progress: Option<js_sys::Function>,
        ) -> js_sys::Promise {
            future_to_promise(async move {
                let (ctx, surface, fmt) = GpuBackend::acquire_web(canvas, progress.as_ref())
                    .await
                    .map_err(|e| JsValue::from_str(&format!("[gpu] acquire_web: {e}")))?;

                let surf_config = surface.get_configuration();
                let (w, h) = surf_config
                    .map(|c| (c.width, c.height))
                    .unwrap_or((800, 600));

                let n = clamp_neuron_count(n);
                let config = SimConfig {
                    n,
                    k,
                    seed: seed as u64,
                    i_ext,
                    backend: crate::sim::backend::BackendKind::Gpu,
                    ..SimConfig::default()
                };

                let mut inner = GpuBackend::new(ctx, config);
                inner.set_i_ext(i_ext);
                inner.set_synaptic_scale(synaptic_scale);

                web_sys::console::log_1(
                    &format!("[gpu] staged backend acquired: N={n} K={k} size={w}×{h}").into(),
                );

                Ok(JsValue::from(WasmGpuBackend {
                    inner,
                    surface,
                    surface_format: fmt,
                    width: w,
                    height: h,
                    pending_network: None,
                    progress_cb: None,
                }))
            })
        }

        /// Boot-load overhaul (Workstream B): install an optional one-way
        /// `(label: string, fraction: f32)` progress callback. `fraction` is
        /// 0..1 progress within the current startup stage. Called from the
        /// compile-heavy `startup_build_render_pipelines` stage so the loading
        /// bar advances with an honest label instead of freezing. Optional; the
        /// `create()` fallback path never sets it.
        pub fn set_progress_callback(&mut self, cb: js_sys::Function) {
            self.progress_cb = Some(cb);
        }

        /// Emit a `(label, fraction)` event if a progress callback is installed.
        fn emit_progress(&self, label: &str, fraction: f64) {
            if let Some(cb) = self.progress_cb.as_ref() {
                let _ = cb.call2(
                    &JsValue::NULL,
                    &JsValue::from_str(label),
                    &JsValue::from_f64(fraction),
                );
            }
        }

        /// Staged startup: build the manifold and retain it for upload.
        pub fn startup_build_manifold(&mut self) {
            let config = self.inner.config().clone();
            self.pending_network = Some(self.inner.begin_initialize(&config));
        }

        /// Staged startup: accept a worker-prepared network payload and
        /// retain it for the normal main-thread WebGPU upload stages.
        #[allow(clippy::too_many_arguments)]
        pub fn startup_begin_prepared_network(
            &mut self,
            version: u32,
            n: usize,
            k: usize,
            seed: u32,
            visual_settings: &[f32],
            morph_config_json: &str,
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
            dropped_count: usize,
        ) -> Result<(), JsValue> {
            if version != PREPARED_NETWORK_VERSION {
                return Err(JsValue::from_str(&format!(
                    "[gpu] prepared network version {version} != {PREPARED_NETWORK_VERSION}"
                )));
            }
            let n = clamp_neuron_count(n);
            let visual = VisualSettings::from_slice(visual_settings);
            let morph_config =
                crate::sim::morphology::MorphologyConfig::from_json(morph_config_json)
                    .map_err(|e| JsValue::from_str(&format!("[gpu] bad morph config: {e}")))?;
            let params = morph_params_from_config_and_visual(&morph_config, &visual);
            let config = SimConfig {
                n,
                k,
                seed: seed as u64,
                i_ext: visual.i_ext,
                backend: crate::sim::backend::BackendKind::Gpu,
                ..SimConfig::default()
            };
            let mut stats = crate::sim::morphology::MorphologyStats::default();
            stats.neuron_count = n;
            stats.fanout_k = k;
            stats.segment_count = segment_path_len.len();
            stats.dropped_count = dropped_count;
            let prepared = PreparedNetworkBuild::from_flat_payload(
                config,
                positions,
                region_codes,
                grid_min,
                grid_cell_size,
                grid_dim,
                grid_cell_start,
                grid_cell_neurons,
                vertices,
                faces,
                segment_endpoints,
                segment_path_len,
                segment_neuron_ids,
                segment_kinds,
                segment_target_ids,
                sphere_geometry,
                sphere_neuron_ids,
                sphere_kinds,
                params,
                stats,
                dropped_count,
            )
            .map_err(|e| JsValue::from_str(&format!("[gpu] bad prepared network: {e}")))?;

            self.pending_network = Some(self.inner.begin_initialize_prepared_with_settings(
                prepared,
                visual,
                morph_config,
            ));
            Ok(())
        }

        /// Staged startup: upload neuron, grid, and sim scratch buffers.
        pub fn startup_upload_neuron_buffers(&mut self) -> Result<(), JsValue> {
            let state = self.pending_network.as_ref().ok_or_else(|| {
                JsValue::from_str("[gpu] startup_upload_neuron_buffers before manifold build")
            })?;
            self.inner.initialize_neuron_buffers(state);
            Ok(())
        }

        /// Staged startup: upload the manifold render mesh and render uniforms.
        pub fn startup_upload_render_resources(&mut self) -> Result<(), JsValue> {
            let state = self.pending_network.as_ref().ok_or_else(|| {
                JsValue::from_str("[gpu] startup_upload_render_resources before manifold build")
            })?;
            self.inner.initialize_render_resources(state);
            Ok(())
        }

        /// Staged startup compatibility stage; retired resource allocators are gone.
        pub fn startup_allocate_lod_edge_resources(&mut self) -> Result<(), JsValue> {
            let state = self.pending_network.as_ref().ok_or_else(|| {
                JsValue::from_str("[gpu] startup_allocate_lod_edge_resources before manifold build")
            })?;
            self.inner.initialize_lod_edge_resources(state);
            Ok(())
        }

        /// Staged startup: generate/upload morphology geometry.
        pub fn startup_upload_morphology(&mut self) -> Result<(), JsValue> {
            let state = self.pending_network.as_ref().ok_or_else(|| {
                JsValue::from_str("[gpu] startup_upload_morphology before manifold build")
            })?;
            self.inner.initialize_morph_resources(state);
            Ok(())
        }

        /// Staged startup: refresh bind groups and reset per-network runtime state.
        pub fn startup_finish_network(&mut self) {
            self.inner.finish_initialize();
            self.pending_network = None;
        }

        /// Staged startup: build the CORE render pipelines for the configured
        /// surface (everything the first frame draws). Boot-load overhaul
        /// (2026-06-12): the bloom + true-opacity `*_active` pipelines are
        /// deferred to `build_render_deferred_pipelines`, called from the rAF
        /// loop one frame after the first frame renders.
        pub fn startup_build_render_pipelines(&mut self) {
            self.emit_progress("Compiling render shaders…", 0.1);
            self.inner.build_render_core_pipelines(self.surface_format);
            self.emit_progress("Compiling render shaders…", 1.0);
        }

        /// Boot-load overhaul: compile the deferred render pipelines (bloom +
        /// `*_active` morphology variants). The web layer calls this from the
        /// rAF loop one frame after the first GPU frame renders. Idempotent.
        pub fn build_deferred_render_pipelines(&mut self) {
            self.inner.build_render_deferred_pipelines();
        }

        /// Staged startup: create depth/bloom render targets for the surface size.
        pub fn startup_resize_render_targets(&mut self) {
            self.inner.resize_render_targets(self.width, self.height);
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
        /// Float32Array layout matches the canonical 26-element contract in
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
            mvp: &[f32],
            right_x: f32,
            right_y: f32,
            right_z: f32,
            up_x: f32,
            up_y: f32,
            up_z: f32,
            eye_x: f32,
            eye_y: f32,
            eye_z: f32,
            camera_dist: f32,
        ) {
            // Acquire surface texture.
            let surface_tex = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                _ => return, // surface lost / timeout
            };
            let view = surface_tex
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            // Copy the MVP slice into a fixed-size array.
            if mvp.len() < 16 {
                return;
            }
            let mut mvp_arr = [0f32; 16];
            mvp_arr.copy_from_slice(&mvp[..16]);

            // V2 Phase 0: source glow_tau and point_radius from VisualSettings
            // (pushed via update_settings) rather than per-frame scalar args.
            let glow_tau = self.inner.visual().glow_tau;
            let point_radius = self.inner.visual().point_radius;

            self.inner.render_full(
                &view,
                &mvp_arr,
                [right_x, right_y, right_z],
                [up_x, up_y, up_z],
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
            if w == self.width && h == self.height {
                return;
            }
            self.width = w;
            self.height = h;

            // Reconfigure the surface at the new size.
            self.surface.configure(
                self.inner.device(),
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: self.surface_format,
                    width: w,
                    height: h,
                    present_mode: wgpu::PresentMode::Fifo,
                    alpha_mode: wgpu::CompositeAlphaMode::Auto,
                    view_formats: vec![],
                    desired_maximum_frame_latency: 2,
                },
            );

            // Resize depth texture to match.
            self.inner.resize_render_targets(w, h);
        }

        /// Reinitialize with a new neuron/connectivity count (adaptive scaler).
        /// Keeps the same surface; rebuilds all GPU buffers.
        pub fn reinitialize(
            &mut self,
            n: usize,
            k: usize,
            seed: u32,
            i_ext: f32,
            synaptic_scale: f32,
        ) {
            let n = clamp_neuron_count(n);
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

        #[allow(clippy::too_many_arguments)]
        pub fn apply_prepared_network(
            &mut self,
            version: u32,
            n: usize,
            k: usize,
            seed: u32,
            visual_settings: &[f32],
            morph_config_json: &str,
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
            dropped_count: usize,
        ) -> Result<(), JsValue> {
            if version != PREPARED_NETWORK_VERSION {
                return Err(JsValue::from_str(&format!(
                    "[gpu] prepared network version {version} != {PREPARED_NETWORK_VERSION}"
                )));
            }
            let n = clamp_neuron_count(n);
            let visual = VisualSettings::from_slice(visual_settings);
            let morph_config =
                crate::sim::morphology::MorphologyConfig::from_json(morph_config_json)
                    .map_err(|e| JsValue::from_str(&format!("[gpu] bad morph config: {e}")))?;
            let params = morph_params_from_config_and_visual(&morph_config, &visual);
            let config = SimConfig {
                n,
                k,
                seed: seed as u64,
                i_ext: visual.i_ext,
                backend: crate::sim::backend::BackendKind::Gpu,
                ..SimConfig::default()
            };
            let mut stats = crate::sim::morphology::MorphologyStats::default();
            stats.neuron_count = n;
            stats.fanout_k = k;
            stats.segment_count = segment_path_len.len();
            stats.dropped_count = dropped_count;
            let prepared = PreparedNetworkBuild::from_flat_payload(
                config,
                positions,
                region_codes,
                grid_min,
                grid_cell_size,
                grid_dim,
                grid_cell_start,
                grid_cell_neurons,
                vertices,
                faces,
                segment_endpoints,
                segment_path_len,
                segment_neuron_ids,
                segment_kinds,
                segment_target_ids,
                sphere_geometry,
                sphere_neuron_ids,
                sphere_kinds,
                params,
                stats,
                dropped_count,
            )
            .map_err(|e| JsValue::from_str(&format!("[gpu] bad prepared network: {e}")))?;

            self.inner
                .initialize_prepared(prepared, visual, morph_config);
            self.inner.build_render_pipelines(self.surface_format);
            self.inner.resize_render_targets(self.width, self.height);
            Ok(())
        }

        /// Release all GPU resources before a tier restart or page teardown.
        pub fn destroy(&mut self) {
            self.inner.destroy();
        }
    }

    /// Report cross-origin isolation status to the console.
    #[wasm_bindgen]
    pub fn log_cross_origin_isolation(isolated: bool) {
        web_sys::console::log_1(&format!("[coi] crossOriginIsolated={isolated}").into());
    }

    fn region_assignment_mode_from_str(value: &str) -> RegionAssignmentMode {
        match value {
            "anterior-posterior-prototype" => RegionAssignmentMode::AnteriorPosteriorPrototype,
            _ => RegionAssignmentMode::HashRandom,
        }
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

    #[test]
    fn build_manifold_with_explicit_region_mode_preserves_default_path() {
        let c = SimConfig {
            n: 4000,
            seed: 17,
            ..SimConfig::default()
        };
        let default = build_manifold(&c);
        let explicit = build_manifold_with_region_assignment(&c, RegionAssignmentMode::HashRandom);
        assert_eq!(explicit.neuron_regions, default.neuron_regions);
    }

    #[test]
    fn build_manifold_with_prototype_region_mode_is_opt_in() {
        let c = SimConfig {
            n: 4000,
            seed: 17,
            ..SimConfig::default()
        };
        let default = build_manifold(&c);
        let prototype = build_manifold_with_region_assignment(
            &c,
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );
        assert_ne!(prototype.neuron_regions, default.neuron_regions);
        let (i, a, o) = region_split(&prototype);
        assert_eq!(i + a + o, 4000);
    }
}
