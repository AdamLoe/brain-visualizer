//! Compute pipeline creation + shader module loading (architecture §5).
//!
//! Phase 2 builds the real `integrate`, `write_scatter_dispatch`, and `scatter`
//! compute pipelines from the embedded WGSL. The BV22 `hash.wgsl` is prepended
//! to the scatter + dispatch shaders so the locked hash is reused, never
//! re-authored (phase-1 note).

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

/// Holds compiled compute pipelines for the per-tick sim passes.
pub struct GpuPipelines {
    pub integrate: Option<wgpu::ComputePipeline>,
    pub write_dispatch: Option<wgpu::ComputePipeline>,
    pub scatter: Option<wgpu::ComputePipeline>,
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
        }
    }

    /// Build all sim pipelines from the embedded WGSL against the given layouts.
    pub fn build(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
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

    pub fn is_built(&self) -> bool {
        self.integrate.is_some() && self.write_dispatch.is_some() && self.scatter.is_some()
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
}
