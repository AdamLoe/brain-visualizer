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

/// Phase 3: far-LOD billboard glow pass.
pub const RENDER_FAR_WGSL: &str = include_str!("shaders/render_far.wgsl");

/// Phase 3: static dark manifold mesh pass.
pub const RENDER_MANIFOLD_WGSL: &str = include_str!("shaders/render_manifold.wgsl");

/// Phase 3: cursor stimulation compute pass.
pub const STIMULATE_WGSL: &str = include_str!("shaders/stimulate.wgsl");

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
        }
    }

    /// Build all sim pipelines from the embedded WGSL against the given layouts.
    pub fn build(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
        self.build_sim(device, layouts);
    }

    /// Build the sim compute pipelines (phase 2).
    fn build_sim(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
        let integrate_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("integrate.wgsl"),
            source: wgpu::ShaderSource::Wgsl(INTEGRATE_WGSL.into()),
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
        self.integrate = Some(device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("integrate"),
            layout: Some(&integrate_pl),
            module: &integrate_module,
            entry_point: Some("integrate"),
            compilation_options: Default::default(),
            cache: None,
        }));

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
        self.scatter = Some(device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("scatter"),
            layout: Some(&scatter_pl),
            module: &scatter_module,
            entry_point: Some("scatter"),
            compilation_options: Default::default(),
            cache: None,
        }));
    }

    /// Build the Phase 3 render + stimulate pipelines.
    /// Called ONCE after `build()` with the canvas surface format.
    /// `color_format` must match the swap-chain / offscreen texture format.
    pub fn build_render(
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
        self.stimulate = Some(device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("stimulate"),
            layout: Some(&stim_pl),
            module: &stim_module,
            entry_point: Some("stimulate"),
            compilation_options: Default::default(),
            cache: None,
        }));

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
                        blend: Some(wgpu::BlendState::REPLACE),
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
        let far_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render_far.wgsl"),
            source: wgpu::ShaderSource::Wgsl(RENDER_FAR_WGSL.into()),
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
        self.render_far = Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
            // Depth test reads depth written by manifold pass but does NOT write
            // depth for additive overlapping glows.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        }));
    }

    pub fn is_built(&self) -> bool {
        self.integrate.is_some() && self.write_dispatch.is_some() && self.scatter.is_some()
    }

    pub fn is_render_built(&self) -> bool {
        self.render_far.is_some()
            && self.render_manifold.is_some()
            && self.stimulate.is_some()
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
        assert!(RENDER_MANIFOLD_WGSL.contains("0.05"));
        assert!(STIMULATE_WGSL.contains("@compute"));
        assert!(STIMULATE_WGSL.contains("fn stimulate"));
        assert!(STIMULATE_WGSL.contains("atomicAdd"));
    }
}
