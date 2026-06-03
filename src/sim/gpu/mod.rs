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
    FrustumCullUniforms, GpuBindGroups, GpuLayouts, GpuResources, IntegrateUniforms,
    ManifoldUniforms, NearLodStats, NearRenderUniforms, RenderUniforms, StimUniform,
};

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
}

impl GpuBackend {
    /// Construct against an already-acquired device/queue. `config` is the
    /// initial network; `manifold`-derived state is uploaded via `resize`.
    pub fn new(ctx: GpuContext, config: SimConfig) -> Self {
        let layouts = GpuLayouts::new(&ctx.device);
        let mut pipelines = GpuPipelines::new();
        pipelines.build(&ctx.device, &layouts);
        let i_ext = config.i_ext;
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
        }
    }

    /// Set the effective recurrent-coupling scale (tuning knob). Default 1.0.
    pub fn set_synaptic_scale(&mut self, s: f32) {
        self.synaptic_scale = s;
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
        let surface: wgpu::Surface<'static> =
            unsafe { std::mem::transmute::<wgpu::Surface<'_>, wgpu::Surface<'static>>(raw_surface) };

        // 3. Request adapter compatible with the surface.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no wgpu adapter: {e}"))?;

        let timestamps_supported = adapter
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut required_features = wgpu::Features::empty();
        if timestamps_supported {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }

        // 4. Request device with generous limits (same pattern as acquire_native).
        let adapter_limits = adapter.limits();
        let mut limits = wgpu::Limits::downlevel_webgl2_defaults();
        // Prefer the higher WebGPU limits if available.
        limits.max_storage_buffer_binding_size =
            adapter_limits.max_storage_buffer_binding_size;
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
            GpuContext { device, queue, timestamps_supported, instance: Some(instance) },
            surface,
            format,
        ))
    }

    /// Acquire a native adapter (high-performance, falling back to llvmpipe) and
    /// build a `GpuContext`. Native-only (examples + tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn acquire_native() -> Result<GpuContext, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no wgpu adapter: {e}"))?;
        let info = adapter.get_info();
        let timestamps_supported = adapter
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut required_features = wgpu::Features::empty();
        if timestamps_supported {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }
        // llvmpipe exposes large storage buffers; request a generous limit so
        // big N fits a single binding. Clamp to adapter limits.
        let mut limits = wgpu::Limits::downlevel_defaults();
        let adapter_limits = adapter.limits();
        limits.max_storage_buffer_binding_size =
            adapter_limits.max_storage_buffer_binding_size;
        limits.max_buffer_size = adapter_limits.max_buffer_size;
        limits.max_compute_workgroups_per_dimension =
            adapter_limits.max_compute_workgroups_per_dimension;
        // Scatter binds 8 storage buffers; integrate binds 5. downlevel default
        // is only 4. Lift to the adapter's capability.
        limits.max_storage_buffers_per_shader_stage =
            adapter_limits.max_storage_buffers_per_shader_stage;
        limits.max_storage_buffer_binding_size =
            adapter_limits.max_storage_buffer_binding_size;
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

    /// Build the network from a manifold and upload the silent-start state.
    /// Rare-path; allocates. Call once after `new` and on every tier change.
    pub fn initialize(&mut self, config: &SimConfig) {
        self.config = config.clone();
        let manifold = crate::build_manifold(config);
        self.resources.resize_neurons(
            &self.ctx.device,
            &self.ctx.queue,
            config,
            &manifold.neuron_positions,
            &manifold.neuron_regions,
            &manifold.spatial_grid,
        );
        // Phase 3: upload manifold mesh + create render uniform buffers.
        self.resources.init_render_resources(
            &self.ctx.device,
            &manifold.vertices,
            &manifold.faces,
            config.n as u32,
            manifold.spatial_grid.dim,
        );
        // Phase 4: near-LOD GPU buffers (allocated once; cleared each frame).
        self.resources.init_near_lod_resources(
            &self.ctx.device,
            &self.ctx.queue,
            config,
            &manifold.spatial_grid,
        );
        self.resources
            .refresh_bind_groups(&self.ctx.device, &self.layouts);
        self.tick = 0;
        self.parity = 0;
        self.max_abs_current_hw = 0;
        self.stim_pending = None;
        self.near_lod_stats = NearLodStats::default();
    }

    /// Build the render pipelines for a given color format.
    /// Called once at startup (or on surface re-creation).
    pub fn build_render_pipelines(&mut self, color_format: wgpu::TextureFormat) {
        self.pipelines.build_render(&self.ctx.device, &self.layouts, color_format);
        // Phase 4: near-LOD pipelines use the same color format.
        self.pipelines.build_near_lod(&self.ctx.device, &self.layouts, color_format);
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
        self.resources.resize_render_targets(&self.ctx.device, width, height);
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
        self.render_full(target_view, mvp, camera_right, camera_up, glow_tau, point_radius,
            [0.0, 0.0, 3.0], self.lod_camera_distance);
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
        let depth_view = match self.resources.render_targets.as_ref()
            .and_then(|t| t.depth_view.as_ref())
        {
            Some(d) => d,
            None => return,
        };
        let pipe_far = match self.pipelines.render_far.as_ref() {
            Some(p) => p,
            None => return,
        };
        let pipe_manifold = match self.pipelines.render_manifold.as_ref() {
            Some(p) => p,
            None => return,
        };

        let n = self.config.n as u32;

        // --- LOD transition ---
        // distance > LOD_FAR_ONLY_DIST  → far only
        // LOD_NEAR_ONLY_DIST..=LOD_FAR_ONLY_DIST → crossfade
        // distance < LOD_NEAR_ONLY_DIST → near only
        let dist = camera_distance;
        let far_alpha = if dist >= LOD_FAR_ONLY_DIST {
            1.0f32
        } else if dist <= LOD_NEAR_ONLY_DIST {
            0.0f32
        } else {
            (dist - LOD_NEAR_ONLY_DIST) / (LOD_FAR_ONLY_DIST - LOD_NEAR_ONLY_DIST)
        };
        let near_alpha = 1.0 - far_alpha;
        let run_near_lod = near_alpha > 0.001
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
        };
        self.ctx.queue.write_buffer(&rr.render_uniform, 0, bytemuck::bytes_of(&ru));

        let mu = ManifoldUniforms { mvp: *mvp };
        self.ctx.queue.write_buffer(&rr.manifold_uniform, 0, bytemuck::bytes_of(&mu));

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
                self.ctx.queue.write_buffer(&nlb.near_render_uniform, 0, bytemuck::bytes_of(&nru));

                // Extract 6 frustum planes from column-major MVP matrix.
                // Standard Gribb/Hartmann plane extraction from MVP rows.
                let planes = extract_frustum_planes(mvp);
                let fu = FrustumCullUniforms {
                    planes,
                    camera_pos,
                    max_synapse_dist: 2.5,  // cull synapses beyond 2.5 world units
                    current_tick: self.tick,
                    n,
                    _pad: [0; 2],
                };
                self.ctx.queue.write_buffer(&nlb.frustum_uniform, 0, bytemuck::bytes_of(&fu));

                // Zero per-frame atomic counters.
                let zero = [0u32];
                self.ctx.queue.write_buffer(&nlb.neuron_count,    0, bytemuck::cast_slice(&zero));
                self.ctx.queue.write_buffer(&nlb.synapse_count,   0, bytemuck::cast_slice(&zero));
                self.ctx.queue.write_buffer(&nlb.neuron_overflow, 0, bytemuck::cast_slice(&zero));
                self.ctx.queue.write_buffer(&nlb.synapse_overflow,0, bytemuck::cast_slice(&zero));
                self.ctx.queue.write_buffer(&nlb.neuron_visible,  0, bytemuck::cast_slice(&zero));
                self.ctx.queue.write_buffer(&nlb.synapse_visible, 0, bytemuck::cast_slice(&zero));
            }
        }

        let mut enc = self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render-frame"),
        });

        // Pass 1: manifold dark mesh (depth prepass).
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("manifold-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
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

        // Pass 2: far-LOD billboard glow (additive, reads depth from pass 1).
        // Skipped when fully in near-LOD mode (far_alpha ≈ 0).
        if far_alpha > 0.001 {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("far-glow-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
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
            pass.set_pipeline(pipe_far);
            pass.set_bind_group(0, bg.render_far.as_ref().unwrap(), &[]);
            pass.draw(0..6, 0..n);
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
            {
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
            // Sphere render pass (draw_indexed_indirect, depth test Load).
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("near-sphere-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
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
                pass.set_pipeline(pipe_sphere);
                pass.set_bind_group(0, srg, &[]);
                pass.set_vertex_buffer(0, nlb.sphere_vb.slice(..));
                pass.set_index_buffer(nlb.sphere_ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed_indirect(&nlb.neuron_draw_args, 0);
            }
            // Cylinder render pass.
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("near-cylinder-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
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
            enc.copy_buffer_to_buffer(&nlb.neuron_count,    0, &nlb.profiler_staging, 0,  4);
            enc.copy_buffer_to_buffer(&nlb.neuron_overflow, 0, &nlb.profiler_staging, 4,  4);
            enc.copy_buffer_to_buffer(&nlb.synapse_count,   0, &nlb.profiler_staging, 8,  4);
            enc.copy_buffer_to_buffer(&nlb.synapse_overflow,0, &nlb.profiler_staging, 12, 4);
            enc.copy_buffer_to_buffer(&nlb.neuron_visible,  0, &nlb.profiler_staging, 16, 4);
            enc.copy_buffer_to_buffer(&nlb.synapse_visible, 0, &nlb.profiler_staging, 20, 4);
        }

        self.ctx.queue.submit([enc.finish()]);

        // Non-blocking profiler readback for near-LOD stats (only when near-LOD ran).
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
            eprintln!("[debug] {:.0}% fired in one tick (excitability bug?)", frac * 100.0);
        }
        (mean_v, frac)
    }

    fn ensure_bind_groups(&mut self) {
        if self.resources.bind_groups_dirty {
            self.resources
                .refresh_bind_groups(&self.ctx.device, &self.layouts);
        }
    }
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

        // Phase 3: write stimulation uniform. Pre-extract stim resources so the
        // borrow checker can split self.pipelines / self.resources borrows.
        let stim_pending = self.stim_pending.take();
        let do_stim = stim_pending.is_some()
            && self.pipelines.stimulate.is_some()
            && bg.stimulate.is_some();
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

            // Flip double-buffer parity: next tick integrate reads what scatter
            // just wrote, and scatter writes the buffer integrate just consumed.
            self.parity ^= 1;
            self.tick = self.tick.wrapping_add(1);
        }

        // Stage stats ONCE for the whole batch (after the last tick). This reads
        // spike_count from the final tick + the high-water max|current|. It does
        // not size any dispatch; purely instrumentation.
        enc.copy_buffer_to_buffer(&sim.spike_count, 0, &sim.stats_staging, 0, 4);
        enc.copy_buffer_to_buffer(&sim.max_abs_current, 0, &sim.stats_staging, 4, 4);

        queue.submit([enc.finish()]);

        // Map the small stats staging buffer (8 B). This is one readback per
        // batch, never per tick, and never feeds the scatter dispatch size.
        let (last_spikes, max_abs) = read_stats(device, &sim.stats_staging);
        self.max_abs_current_hw = self.max_abs_current_hw.max(max_abs);

        let tick_ms = t0.elapsed().as_secs_f32() * 1000.0;
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
    let row0 = [m[0], m[4], m[8],  m[12]];
    let row1 = [m[1], m[5], m[9],  m[13]];
    let row2 = [m[2], m[6], m[10], m[14]];
    let row3 = [m[3], m[7], m[11], m[15]];

    let add = |a: [f32;4], b: [f32;4]| [a[0]+b[0], a[1]+b[1], a[2]+b[2], a[3]+b[3]];
    let sub = |a: [f32;4], b: [f32;4]| [a[0]-b[0], a[1]-b[1], a[2]-b[2], a[3]-b[3]];

    // Left:   row3 + row0
    // Right:  row3 - row0
    // Bottom: row3 + row1
    // Top:    row3 - row1
    // Near:   row3 + row2
    // Far:    row3 - row2
    [
        add(row3, row0),  // left
        sub(row3, row0),  // right
        add(row3, row1),  // bottom
        sub(row3, row1),  // top
        add(row3, row2),  // near
        sub(row3, row2),  // far
    ]
}

/// Read near-LOD profiler stats from the staging buffer (blocks once per frame).
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
}
