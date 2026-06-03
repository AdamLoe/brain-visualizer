//! GPU backend (WebGPU compute, clock-driven) — STUB in phase 1 (BV4).
//!
//! `tick()` returns zeroed stats and allocates nothing. The resource/pipeline
//! ownership boundaries are real (`resources.rs`, `pipelines.rs`); only the sim
//! kernels are deferred to phase 2.

pub mod pipelines;
pub mod resources;

use crate::sim::backend::{RenderState, SimBackend, SimConfig, TickStats};
use pipelines::GpuPipelines;
use resources::{GpuLayouts, GpuResources};

/// Clock-driven, data-parallel GPU simulation backend.
pub struct GpuBackend {
    config: SimConfig,
    resources: GpuResources,
    pipelines: GpuPipelines,
    layouts: GpuLayouts,
    // TODO(phase 2): wgpu::Device / Queue handles owned here.
}

impl GpuBackend {
    /// Construct the backend scaffolding (no device work in phase 1).
    pub fn new(config: SimConfig) -> Self {
        Self {
            config,
            resources: GpuResources::new(),
            pipelines: GpuPipelines::new(),
            layouts: GpuLayouts {},
        }
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn resources(&self) -> &GpuResources {
        &self.resources
    }
}

impl SimBackend for GpuBackend {
    fn tick(&mut self, _ticks: u32, _excitability: f32) -> TickStats {
        // Phase 1 stub: allocate nothing, run nothing.
        TickStats::default()
    }

    fn stimulate(&mut self, _pos: [f32; 3], _radius: f32, _current: f32) {
        // Phase 1 stub.
    }

    fn render_state(&self) -> RenderState<'_> {
        // No buffers allocated yet.
        RenderState::Empty
    }

    fn resize(&mut self, config: &SimConfig) {
        self.config = config.clone();
        // Rare-path: rebuild layouts and mark bind groups dirty. Without a real
        // device we only update the layout structs (phase 2 passes the device).
        self.resources.neuron_buffers =
            Some(resources::NeuronBuffers::new(config.n));
        self.resources.bind_groups_dirty = true;
        let _ = &self.layouts;
        let _ = &mut self.pipelines;
    }

    fn destroy(&mut self) {
        self.resources.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_returns_zeros_and_allocates_nothing() {
        let mut b = GpuBackend::new(SimConfig::default());
        let stats = b.tick(4, 0.5);
        assert_eq!(stats, TickStats::default());
        assert!(b.resources().neuron_buffers.is_none());
    }

    #[test]
    fn resize_builds_layouts_and_dirties_bind_groups() {
        let mut b = GpuBackend::new(SimConfig::default());
        let cfg = SimConfig {
            n: 200_000,
            ..SimConfig::default()
        };
        b.resize(&cfg);
        assert!(b.resources().bind_groups_dirty);
        assert_eq!(
            b.resources()
                .neuron_buffers
                .as_ref()
                .unwrap()
                .v
                .total(),
            200_000
        );
    }

    #[test]
    fn destroy_releases_resources() {
        let mut b = GpuBackend::new(SimConfig::default());
        b.resize(&SimConfig::default());
        b.destroy();
        assert!(b.resources().neuron_buffers.is_none());
    }
}
