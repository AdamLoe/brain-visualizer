//! Compute + render pipeline creation + shader module loading (architecture §5).
//!
//! Phase 2 builds the real `integrate`, `write_scatter_dispatch`, and `scatter`
//! compute pipelines from the embedded WGSL. The BV22 `hash.wgsl` is prepended
//! to the scatter + dispatch shaders so the locked hash is reused, never
//! re-authored (phase-1 note).
//!
//! Phase 3 adds:
//!   - `render_far` — instanced billboard glow render pipeline (additive blend).
//!   - `render_manifold` — static dark mesh render pipeline (depth test, opaque).
//!   - `stimulate` — cursor stimulation compute pipeline.

use super::resources::GpuLayouts;

/// BV22 hash WGSL — prepended to any shader that derives synapse targets.
/// MUST match `connectivity::hash` (see that module's tests + the native
/// determinism test).
pub const HASH_WGSL: &str = include_str!("shaders/hash.wgsl");

/// Integrate+threshold pass.
pub const INTEGRATE_WGSL: &str = include_str!("shaders/integrate.wgsl");

/// Scatter pass (prepended with HASH_WGSL at module-creation time).
pub const SCATTER_WGSL: &str = include_str!("shaders/scatter.wgsl");

/// Indirect-dispatch writer (prepended with HASH_WGSL for uniform struct parity;
/// it does not call the hash but shares the WGSL build path).
pub const WRITE_SCATTER_DISPATCH_WGSL: &str = include_str!("shaders/write_scatter_dispatch.wgsl");

/// Phase 3: far-LOD billboard glow pass (prepended with HASH_WGSL for identity color).
pub const RENDER_FAR_WGSL: &str = include_str!("shaders/render_far.wgsl");

/// Phase 3: static dark manifold mesh pass.
pub const RENDER_MANIFOLD_WGSL: &str = include_str!("shaders/render_manifold.wgsl");

/// Phase 3: cursor stimulation compute pass.
pub const STIMULATE_WGSL: &str = include_str!("shaders/stimulate.wgsl");

/// V2 Phase A: metrics reduction compute pass (read-only over neuron state).
pub const METRICS_WGSL: &str = include_str!("shaders/metrics.wgsl");

/// V2 Phase E: bloom post-process (bright/blur/composite fullscreen passes).
pub const BLOOM_WGSL: &str = include_str!("shaders/bloom.wgsl");

/// Morphology: procedural per-neuron geometry render (tapered tubes + soma spheres).
/// Prepended with HASH_WGSL for identity color.
pub const RENDER_MORPHOLOGY_WGSL: &str = include_str!("shaders/render_morphology.wgsl");

/// Morphology: active/recent segment compaction compute (reset/compact/write_args).
pub const COMPACT_MORPH_WGSL: &str = include_str!("shaders/compact_morph_segments.wgsl");

/// Holds compiled compute pipelines for the per-tick sim passes.
pub struct GpuPipelines {
    pub integrate: Option<wgpu::ComputePipeline>,
    pub write_dispatch: Option<wgpu::ComputePipeline>,
    pub scatter: Option<wgpu::ComputePipeline>,
    /// Phase 3: far-LOD billboard glow render pipeline.
    pub render_far: Option<wgpu::RenderPipeline>,
    /// Phase 3: manifold dark mesh render pipeline.
    pub render_manifold: Option<wgpu::RenderPipeline>,
    /// Phase 3: stimulation compute pipeline.
    pub stimulate: Option<wgpu::ComputePipeline>,
    /// V2 Phase A: metrics reduction compute pipeline.
    pub metrics: Option<wgpu::ComputePipeline>,
    /// Morphology: procedural neuron geometry render pipeline (tubes).
    pub render_morphology: Option<wgpu::RenderPipeline>,
    /// Morphology: active/recent compaction compute pipelines (reset → compact →
    /// write_args). Built alongside the morph render pipelines.
    pub compact_morph_reset: Option<wgpu::ComputePipeline>,
    pub compact_morph: Option<wgpu::ComputePipeline>,
    pub compact_morph_write_args: Option<wgpu::ComputePipeline>,
    /// Morphology: soma sphere render pipeline (Wave 2).
    pub render_soma_spheres: Option<wgpu::RenderPipeline>,
    /// Morphology: true-opacity active tube pipeline — depth-tested, alpha-blended
    /// (`fs_main_active`). Renders firing tubes opaque so they occlude background.
    pub render_morphology_active: Option<wgpu::RenderPipeline>,
    /// Morphology: true-opacity active soma pipeline — depth-tested, alpha-blended
    /// (`fs_sphere_active`). Renders firing somas opaque so they occlude background.
    pub render_soma_spheres_active: Option<wgpu::RenderPipeline>,
    // ─── V2 Phase E: bloom post-process pipelines ─────────────────────────────
    /// Bright-pass (threshold) → rgba16float.
    pub bloom_bright: Option<wgpu::RenderPipeline>,
    /// Separable Gaussian blur → rgba16float (used for both H and V passes).
    pub bloom_blur: Option<wgpu::RenderPipeline>,
    /// Composite (scene + bloom, tonemap) → surface color format.
    pub bloom_composite: Option<wgpu::RenderPipeline>,
}

impl Default for GpuPipelines {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuPipelines {
    pub fn new() -> Self {
        Self {
            integrate: None,
            write_dispatch: None,
            scatter: None,
            render_far: None,
            render_manifold: None,
            stimulate: None,
            // V2 Phase A
            metrics: None,
            // Morphology
            render_morphology: None,
            compact_morph_reset: None,
            compact_morph: None,
            compact_morph_write_args: None,
            render_soma_spheres: None,
            render_morphology_active: None,
            render_soma_spheres_active: None,
            // V2 Phase E
            bloom_bright: None,
            bloom_blur: None,
            bloom_composite: None,
        }
    }

    /// Build all sim pipelines from the embedded WGSL against the given layouts.
    pub fn build(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
        self.build_sim(device, layouts);
    }

    /// Build the sim compute pipelines (phase 2).
    fn build_sim(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
        // V2 Phase C: prepend HASH_WGSL so integrate can draw per-neuron /
        // per-tick randomness (heterogeneity, poisson input) from the locked
        // BV22 hash — same pattern as scatter.
        let integrate_src = format!("{HASH_WGSL}\n{INTEGRATE_WGSL}");
        let integrate_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("integrate.wgsl"),
            source: wgpu::ShaderSource::Wgsl(integrate_src.into()),
        });
        let dispatch_src = format!("{HASH_WGSL}\n{WRITE_SCATTER_DISPATCH_WGSL}");
        let dispatch_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("write_scatter_dispatch.wgsl"),
            source: wgpu::ShaderSource::Wgsl(dispatch_src.into()),
        });
        let scatter_src = format!("{HASH_WGSL}\n{SCATTER_WGSL}");
        let scatter_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scatter.wgsl"),
            source: wgpu::ShaderSource::Wgsl(scatter_src.into()),
        });

        let integrate_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("integrate-pl"),
            bind_group_layouts: &[
                Some(&layouts.integrate_bgl),
                Some(&layouts.integrate_uniform_bgl),
            ],
            immediate_size: 0,
        });
        self.integrate = Some(
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("integrate"),
                layout: Some(&integrate_pl),
                module: &integrate_module,
                entry_point: Some("integrate"),
                compilation_options: Default::default(),
                cache: None,
            }),
        );

        let dispatch_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("write-dispatch-pl"),
            bind_group_layouts: &[
                Some(&layouts.write_dispatch_bgl),
                Some(&layouts.connect_uniform_bgl),
            ],
            immediate_size: 0,
        });
        self.write_dispatch = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("write_scatter_dispatch"),
                layout: Some(&dispatch_pl),
                module: &dispatch_module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            },
        ));

        let scatter_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scatter-pl"),
            bind_group_layouts: &[
                Some(&layouts.scatter_bgl),
                Some(&layouts.connect_uniform_bgl),
            ],
            immediate_size: 0,
        });
        self.scatter = Some(
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("scatter"),
                layout: Some(&scatter_pl),
                module: &scatter_module,
                entry_point: Some("scatter"),
                compilation_options: Default::default(),
                cache: None,
            }),
        );

        // ─── V2 Phase A: metrics reduction compute pipeline ───────────────────
        let metrics_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("metrics.wgsl"),
            source: wgpu::ShaderSource::Wgsl(METRICS_WGSL.into()),
        });
        let metrics_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("metrics-pl"),
            bind_group_layouts: &[
                Some(&layouts.metrics_bgl),
                Some(&layouts.metrics_uniform_bgl),
            ],
            immediate_size: 0,
        });
        self.metrics = Some(
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("reduce_metrics"),
                layout: Some(&metrics_pl),
                module: &metrics_module,
                entry_point: Some("reduce_metrics"),
                compilation_options: Default::default(),
                cache: None,
            }),
        );

    }

    /// Build the Phase 3 render + stimulate pipelines.
    /// Called ONCE after `build()` with the canvas surface format.
    /// `color_format` must match the swap-chain / offscreen texture format.
    ///
    /// Boot-load overhaul (2026-06-12): this is now `build_render_core` +
    /// `build_render_deferred` split so boot only compiles the pipelines the
    /// first frame actually draws. Kept as a convenience wrapper for the
    /// surface-recreation / render-quality-rebuild paths that want everything
    /// at once.
    pub fn build_render(
        &mut self,
        device: &wgpu::Device,
        layouts: &GpuLayouts,
        color_format: wgpu::TextureFormat,
    ) {
        let rq = crate::sim::morphology::RenderQualityConfig::default();
        self.build_render_core(device, layouts, color_format);
        self.build_render_deferred(device, layouts, color_format, rq);
    }

    /// Build the render pipelines required to paint the FIRST frame: stimulate
    /// compute, manifold mesh, far billboards, the additive morphology tube +
    /// soma passes, and the active/recent compaction compute pipelines.
    ///
    /// The bloom post-process pipelines and the true-opacity `*_active`
    /// morphology variants are NOT built here — they are compiled lazily by
    /// `build_render_deferred` one frame after `ready`. `render_full` guards
    /// every bloom/active access with `is_some()`, so a frame that runs before
    /// the deferred build simply skips those passes (bloom is opt-in and
    /// default-off; the active layer briefly falls back to the additive look).
    pub fn build_render_core(
        &mut self,
        device: &wgpu::Device,
        layouts: &GpuLayouts,
        color_format: wgpu::TextureFormat,
    ) {
        // --- Stimulate compute ---
        let stim_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stimulate.wgsl"),
            source: wgpu::ShaderSource::Wgsl(STIMULATE_WGSL.into()),
        });
        let stim_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stimulate-pl"),
            bind_group_layouts: &[Some(&layouts.stimulate_bgl)],
            immediate_size: 0,
        });
        self.stimulate = Some(
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("stimulate"),
                layout: Some(&stim_pl),
                module: &stim_module,
                entry_point: Some("stimulate"),
                compilation_options: Default::default(),
                cache: None,
            }),
        );

        // --- Manifold dark mesh ---
        let manifold_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render_manifold.wgsl"),
            source: wgpu::ShaderSource::Wgsl(RENDER_MANIFOLD_WGSL.into()),
        });
        let manifold_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render-manifold-pl"),
            bind_group_layouts: &[Some(&layouts.render_manifold_bgl)],
            immediate_size: 0,
        });
        self.render_manifold = Some(device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("render_manifold"),
                layout: Some(&manifold_pl),
                vertex: wgpu::VertexState {
                    module: &manifold_module,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 12, // 3 × f32
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        }],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &manifold_module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        // V2 Phase E: alpha-blend so surface_opacity controls how
                        // translucent the dim context surface is over black.
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None, // no culling: see brain from both sides
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        ));

        // --- Far-LOD billboard glow (additive blend, no depth write) ---
        let far_src = format!("{HASH_WGSL}\n{RENDER_FAR_WGSL}");
        let far_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render_far.wgsl"),
            source: wgpu::ShaderSource::Wgsl(far_src.into()),
        });
        let far_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render-far-pl"),
            bind_group_layouts: &[Some(&layouts.render_far_bgl)],
            immediate_size: 0,
        });
        // Additive blend: src=One, dst=One. Multiple overlapping glows sum.
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        self.render_far = Some(
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("render_far"),
                layout: Some(&far_pl),
                vertex: wgpu::VertexState {
                    module: &far_module,
                    entry_point: Some("vs_main"),
                    buffers: &[], // no vertex buffers; all data from storage bindings
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &far_module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(additive_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                // No depth attachment — all neurons are visible from any angle.
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            }),
        );

        // ─── Morphology: tube + soma-sphere render pipelines ───────────────────
        // v0.3.1: tessellation (TUBE_SIDES / SPHERE_SLICES / SPHERE_STACKS) is now
        // driven by WGSL override constants, so the pipelines are built in a
        // dedicated method that the backend can also re-invoke (pipeline-rebuild)
        // when the render-quality config changes. Defaults match v0.3.0.
        self.build_morph_pipelines(
            device,
            layouts,
            color_format,
            crate::sim::morphology::RenderQualityConfig::default(),
        );
    }

    /// Build the render pipelines that are NOT needed for the first painted
    /// frame and can therefore be compiled one frame after `ready`:
    ///   - the 3 bloom post-process pipelines (bright/blur/composite),
    ///   - the true-opacity `*_active` morphology tube + soma variants.
    ///
    /// Idempotent: safe to call again after a render-quality / surface rebuild
    /// (it simply overwrites the pipelines). `render_full` guards every access
    /// to these with `is_some()`, so a frame between core and deferred builds
    /// renders correctly without them.
    pub fn build_render_deferred(
        &mut self,
        device: &wgpu::Device,
        layouts: &GpuLayouts,
        color_format: wgpu::TextureFormat,
        rq: crate::sim::morphology::RenderQualityConfig,
    ) {
        self.build_morph_active_pipelines(device, layouts, color_format, rq);

        // ─── V2 Phase E: bloom post-process pipelines ──────────────────────────
        // Fullscreen-triangle passes. bright/blur write rgba16float (HDR), the
        // composite writes the surface color_format. Built unconditionally; only
        // ENCODED when bloom_strength > 0 (default off ⇒ never touched).
        let bloom_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom.wgsl"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_WGSL.into()),
        });
        let hdr_format = wgpu::TextureFormat::Rgba16Float;

        // bright + blur share the 3-binding pass layout.
        let bloom_pass_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom-pass-pl"),
            bind_group_layouts: &[Some(&layouts.bloom_pass_bgl)],
            immediate_size: 0,
        });
        let composite_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom-composite-pl"),
            bind_group_layouts: &[Some(&layouts.bloom_composite_bgl)],
            immediate_size: 0,
        });

        let make_fullscreen =
            |label: &str, pl: &wgpu::PipelineLayout, fs_entry: &str, fmt: wgpu::TextureFormat| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(pl),
                    vertex: wgpu::VertexState {
                        module: &bloom_module,
                        entry_point: Some("vs_fullscreen"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &bloom_module,
                        entry_point: Some(fs_entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: fmt,
                            blend: Some(wgpu::BlendState::REPLACE),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        unclipped_depth: false,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };

        self.bloom_bright = Some(make_fullscreen(
            "bloom_bright",
            &bloom_pass_pl,
            "fs_bright",
            hdr_format,
        ));
        self.bloom_blur = Some(make_fullscreen(
            "bloom_blur",
            &bloom_pass_pl,
            "fs_blur",
            hdr_format,
        ));
        self.bloom_composite = Some(make_fullscreen(
            "bloom_composite",
            &composite_pl,
            "fs_composite",
            color_format,
        ));
    }

    /// True if the deferred render pipelines (bloom + `*_active` morphology)
    /// have all been built. Used by the backend to fire `build_render_deferred`
    /// exactly once after the first frame.
    pub fn is_render_deferred_built(&self) -> bool {
        self.bloom_bright.is_some()
            && self.bloom_blur.is_some()
            && self.bloom_composite.is_some()
            && self.render_morphology_active.is_some()
            && self.render_soma_spheres_active.is_some()
    }

    /// Morphology tube + soma-sphere render pipelines (additive, no depth).
    /// v0.3.1: tessellation comes in via WGSL override constants set through
    /// `compilation_options.constants` (TUBE_SIDES / SPHERE_SLICES / SPHERE_STACKS).
    /// The Rust draw vert-counts must be derived from the SAME `RenderQualityConfig`
    /// (see `GpuBackend`). Re-invokable for a render-quality pipeline rebuild.
    pub fn build_morph_pipelines(
        &mut self,
        device: &wgpu::Device,
        layouts: &GpuLayouts,
        color_format: wgpu::TextureFormat,
        rq: crate::sim::morphology::RenderQualityConfig,
    ) {
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let morph_src = format!("{HASH_WGSL}\n{RENDER_MORPHOLOGY_WGSL}");
        let morph_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render_morphology.wgsl"),
            source: wgpu::ShaderSource::Wgsl(morph_src.into()),
        });

        // WGSL override constants. Keyed by the WGSL identifier; values are f64
        // (wgpu casts to the override's declared type, u32 here).
        let tube_consts: &[(&str, f64)] = &[("TUBE_SIDES", rq.tube_sides as f64)];
        let sphere_consts: &[(&str, f64)] = &[
            ("SPHERE_SLICES", rq.sphere_slices as f64),
            ("SPHERE_STACKS", rq.sphere_stacks as f64),
        ];

        // ── Tube pipeline ──────────────────────────────────────────────────────
        let morph_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render-morphology-pl"),
            bind_group_layouts: &[Some(&layouts.render_morphology_bgl)],
            immediate_size: 0,
        });
        self.render_morphology = Some(device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("render_morphology"),
                layout: Some(&morph_pl),
                vertex: wgpu::VertexState {
                    module: &morph_module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: tube_consts,
                        ..Default::default()
                    },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &morph_module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(additive_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: tube_consts,
                        ..Default::default()
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        ));

        // ── Soma sphere pipeline (same module, vs_sphere/fs_sphere) ─────────────
        let soma_sphere_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render-soma-spheres-pl"),
            bind_group_layouts: &[Some(&layouts.render_soma_spheres_bgl)],
            immediate_size: 0,
        });
        self.render_soma_spheres = Some(device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("render_soma_spheres"),
                layout: Some(&soma_sphere_pl),
                vertex: wgpu::VertexState {
                    module: &morph_module,
                    entry_point: Some("vs_sphere"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: sphere_consts,
                        ..Default::default()
                    },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &morph_module,
                    entry_point: Some("fs_sphere"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(additive_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: sphere_consts,
                        ..Default::default()
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        ));

        // ── Active/recent compaction compute pipelines ──────────────────────────
        // One module, three entry points (reset → compact → write_args). Reads
        // segments + last_spike; writes active_segment_indices + draw args. Built
        // here so the set_morphology_config render-quality rebuild keeps them in
        // sync (they carry no override constants, so rebuild is harmless).
        let compact_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compact_morph_segments.wgsl"),
            source: wgpu::ShaderSource::Wgsl(COMPACT_MORPH_WGSL.into()),
        });
        let compact_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compact-morph-pl"),
            bind_group_layouts: &[Some(&layouts.compact_morph_bgl)],
            immediate_size: 0,
        });
        let make_compact = |entry: &str, label: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&compact_pl),
                module: &compact_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        self.compact_morph_reset = Some(make_compact("reset", "compact_morph_reset"));
        self.compact_morph = Some(make_compact("compact", "compact_morph"));
        self.compact_morph_write_args =
            Some(make_compact("write_args", "compact_morph_write_args"));
    }

    /// True-opacity active morphology pipelines (depth-tested, alpha-blended):
    /// `render_morphology_active` (`fs_main_active`) + `render_soma_spheres_active`
    /// (`fs_sphere_active`). Same module / layout / override constants as their
    /// additive siblings in `build_morph_pipelines` — only the fragment entry
    /// point, blend mode, and depth_stencil differ.
    ///
    /// Boot-load overhaul: split out of `build_morph_pipelines` so boot's core
    /// render-pipeline stage skips them; `build_render_deferred` compiles them
    /// one frame after `ready`. The set_morphology_config render-quality rebuild
    /// re-invokes `build_morph_pipelines` (additive) AND this (active) so both
    /// stay in sync.
    pub fn build_morph_active_pipelines(
        &mut self,
        device: &wgpu::Device,
        layouts: &GpuLayouts,
        color_format: wgpu::TextureFormat,
        rq: crate::sim::morphology::RenderQualityConfig,
    ) {
        let morph_src = format!("{HASH_WGSL}\n{RENDER_MORPHOLOGY_WGSL}");
        let morph_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render_morphology.wgsl"),
            source: wgpu::ShaderSource::Wgsl(morph_src.into()),
        });
        let tube_consts: &[(&str, f64)] = &[("TUBE_SIDES", rq.tube_sides as f64)];
        let sphere_consts: &[(&str, f64)] = &[
            ("SPHERE_SLICES", rq.sphere_slices as f64),
            ("SPHERE_STACKS", rq.sphere_stacks as f64),
        ];
        let morph_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render-morphology-pl"),
            bind_group_layouts: &[Some(&layouts.render_morphology_bgl)],
            immediate_size: 0,
        });
        let soma_sphere_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render-soma-spheres-pl"),
            bind_group_layouts: &[Some(&layouts.render_soma_spheres_bgl)],
            immediate_size: 0,
        });

        // Depth state copied from the manifold pipeline.
        let active_depth = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };

        self.render_morphology_active = Some(device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("render_morphology_active"),
                layout: Some(&morph_pl),
                vertex: wgpu::VertexState {
                    module: &morph_module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: tube_consts,
                        ..Default::default()
                    },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &morph_module,
                    entry_point: Some("fs_main_active"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: tube_consts,
                        ..Default::default()
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(active_depth.clone()),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        ));

        self.render_soma_spheres_active = Some(device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("render_soma_spheres_active"),
                layout: Some(&soma_sphere_pl),
                vertex: wgpu::VertexState {
                    module: &morph_module,
                    entry_point: Some("vs_sphere"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: sphere_consts,
                        ..Default::default()
                    },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &morph_module,
                    entry_point: Some("fs_sphere_active"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: sphere_consts,
                        ..Default::default()
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(active_depth),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        ));
    }

    pub fn is_built(&self) -> bool {
        self.integrate.is_some() && self.write_dispatch.is_some() && self.scatter.is_some()
    }

    pub fn is_render_built(&self) -> bool {
        self.render_far.is_some() && self.render_manifold.is_some() && self.stimulate.is_some()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_wgsl_embedded_and_locked() {
        assert!(HASH_WGSL.contains("0x7feb352du"));
        assert!(HASH_WGSL.contains("0x846ca68bu"));
        assert!(HASH_WGSL.contains("fn mix_key"));
        assert!(HASH_WGSL.contains("0x9e3779b1u"));
    }

    #[test]
    fn pass_shaders_present() {
        assert!(INTEGRATE_WGSL.contains("@compute"));
        assert!(INTEGRATE_WGSL.contains("fn integrate"));
        assert!(SCATTER_WGSL.contains("@compute"));
        assert!(SCATTER_WGSL.contains("fn scatter"));
        assert!(WRITE_SCATTER_DISPATCH_WGSL.contains("dispatch_args"));
    }

    #[test]
    fn scatter_uses_spatial_rule_not_modulo_n_in_production() {
        // Production target_neuron must use the spatial grid; modulo-N is only
        // in the explicitly-named debug fallback.
        assert!(SCATTER_WGSL.contains("fn target_neuron("));
        assert!(SCATTER_WGSL.contains("nearest_occupied"));
        assert!(SCATTER_WGSL.contains("cell_of_neuron"));
        // The only modulo-N target lives in the debug fallback.
        assert!(SCATTER_WGSL.contains("fn target_neuron_debug"));
    }

    #[test]
    fn render_shaders_present() {
        assert!(RENDER_FAR_WGSL.contains("@vertex"));
        assert!(RENDER_FAR_WGSL.contains("@fragment"));
        assert!(RENDER_FAR_WGSL.contains("fn vs_main"));
        assert!(RENDER_FAR_WGSL.contains("fn fs_main"));
        assert!(RENDER_FAR_WGSL.contains("HAS_SPIKED_MASK"));
        assert!(RENDER_MANIFOLD_WGSL.contains("@vertex"));
        assert!(RENDER_MANIFOLD_WGSL.contains("@fragment"));
        // V2 Phase E: surface pass now driven by surface_opacity/mode.
        assert!(RENDER_MANIFOLD_WGSL.contains("surface_opacity"));
        assert!(STIMULATE_WGSL.contains("@compute"));
        assert!(STIMULATE_WGSL.contains("fn stimulate"));
        assert!(STIMULATE_WGSL.contains("atomicAdd"));
        // V2 Phase E: bloom post-process shader present (bright/blur/composite).
        assert!(BLOOM_WGSL.contains("fn vs_fullscreen"));
        assert!(BLOOM_WGSL.contains("fn fs_bright"));
        assert!(BLOOM_WGSL.contains("fn fs_blur"));
        assert!(BLOOM_WGSL.contains("fn fs_composite"));
    }

    #[test]
    fn morphology_compaction_shaders_present() {
        // compact_morph_segments.wgsl — active/recent compaction entry points +
        // mirrored owner rule + impulse constants must stay present.
        assert!(COMPACT_MORPH_WGSL.contains("fn compact"));
        assert!(COMPACT_MORPH_WGSL.contains("fn reset"));
        assert!(COMPACT_MORPH_WGSL.contains("fn write_args"));
        assert!(COMPACT_MORPH_WGSL.contains("presynaptic_dendrite"));
        assert!(COMPACT_MORPH_WGSL.contains("AXON_IMPULSE_SPEED"));
        // render_morphology.wgsl — compacted instance remap present.
        assert!(RENDER_MORPHOLOGY_WGSL.contains("active_segment_indices"));
        assert!(RENDER_MORPHOLOGY_WGSL.contains("const TUBE_RINGS: u32 = 4u"));
        assert!(RENDER_MORPHOLOGY_WGSL.contains("tube_curve_bend"));
    }
}
