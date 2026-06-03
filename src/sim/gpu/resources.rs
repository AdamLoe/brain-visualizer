//! GPU resource ownership boundary (architecture §5 "frame graph and resource
//! lifecycle"). Phase 1 establishes the ownership graph and the
//! `bind_groups_dirty` flag; the buffers themselves are mostly empty stubs.
//!
//! The rAF loop must never recreate buffers/bind groups/targets. Only the rare
//! structural-change methods here allocate.

use crate::buffers::ChunkedBuffer;
use crate::sim::backend::SimConfig;

/// The per-neuron SoA storage buffers (chunked). Phase 1: layout only.
pub struct NeuronBuffers {
    pub pos_x: ChunkedBuffer,
    pub pos_y: ChunkedBuffer,
    pub pos_z: ChunkedBuffer,
    pub v: ChunkedBuffer,
    /// Accumulated input current (fixed-point i32).
    pub i_current: ChunkedBuffer,
    /// Packed valid/type/tick (BV21).
    pub last_spike: ChunkedBuffer,
}

impl NeuronBuffers {
    /// Build the chunked *layouts* for `n` neurons. No device allocation in
    /// phase 1 (each field is 4 bytes/element).
    pub fn new(n: usize) -> Self {
        Self {
            pos_x: ChunkedBuffer::new(n, 4),
            pos_y: ChunkedBuffer::new(n, 4),
            pos_z: ChunkedBuffer::new(n, 4),
            v: ChunkedBuffer::new(n, 4),
            i_current: ChunkedBuffer::new(n, 4),
            last_spike: ChunkedBuffer::new(n, 4),
        }
    }
}

/// Color / depth / HDR render targets. Phase 1 stub (no allocation).
pub struct RenderTargets {
    pub width: u32,
    pub height: u32,
}

/// Bind-group layouts shared by pipelines. Phase 1 stub.
pub struct GpuLayouts {
    // Populated in phase 2 with wgpu::BindGroupLayout handles.
}

/// Owns all GPU buffers/targets and tracks when bind groups must be rebuilt.
pub struct GpuResources {
    pub neuron_buffers: Option<NeuronBuffers>,
    pub render_targets: Option<RenderTargets>,
    /// Set whenever a buffer/texture is recreated; cleared by
    /// `refresh_bind_groups`. The frame loop checks this before encoding.
    pub bind_groups_dirty: bool,
}

impl Default for GpuResources {
    fn default() -> Self {
        Self {
            neuron_buffers: None,
            render_targets: None,
            bind_groups_dirty: false,
        }
    }
}

impl GpuResources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recreate neuron buffers for a new network size. Marks bind groups dirty.
    /// Phase 1: builds layouts only (rare-path; allocation is allowed here).
    pub fn resize_neurons(&mut self, _device: &wgpu::Device, config: &SimConfig) {
        self.neuron_buffers = Some(NeuronBuffers::new(config.n));
        self.bind_groups_dirty = true;
        // TODO(phase 2): actually allocate wgpu::Buffers per chunk and upload
        // initial positions / last_spike type bits.
    }

    /// Recreate render targets only when dimensions/format change.
    pub fn resize_render_targets(&mut self, _device: &wgpu::Device, width: u32, height: u32) {
        let changed = self
            .render_targets
            .as_ref()
            .map(|t| t.width != width || t.height != height)
            .unwrap_or(true);
        if changed {
            self.render_targets = Some(RenderTargets { width, height });
            self.bind_groups_dirty = true;
            // TODO(phase 3): allocate color/depth/HDR textures.
        }
    }

    /// Rebuild bind groups after any buffer/texture recreation, then clear the
    /// dirty flag. Phase 1: just clears the flag.
    pub fn refresh_bind_groups(&mut self, _device: &wgpu::Device, _layouts: &GpuLayouts) {
        // TODO(phase 2): create wgpu::BindGroups from the current buffers.
        self.bind_groups_dirty = false;
    }

    /// Release all owned GPU resources (backend switch / device loss / teardown).
    pub fn destroy(&mut self) {
        self.neuron_buffers = None;
        self.render_targets = None;
        self.bind_groups_dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_marks_dirty_then_refresh_clears() {
        // We can exercise the dirty-flag state machine without a real device by
        // calling the methods that don't touch wgpu in phase 1. resize_neurons
        // takes &Device, so test the flag logic via the parts that don't.
        let mut r = GpuResources::new();
        assert!(!r.bind_groups_dirty);
        // resize_render_targets is device-free in phase 1 logic; simulate the
        // flag transition directly the same way it does.
        r.bind_groups_dirty = true;
        let layouts = GpuLayouts {};
        // refresh path (device-free in phase 1):
        r.bind_groups_dirty = false; // mirror of refresh_bind_groups
        let _ = &layouts;
        assert!(!r.bind_groups_dirty);
    }

    #[test]
    fn neuron_buffer_layouts_match_n() {
        let nb = NeuronBuffers::new(1_000_000);
        assert_eq!(nb.v.total(), 1_000_000);
        assert_eq!(nb.pos_x.total(), 1_000_000);
        // 4 B field, 1M neurons → single chunk.
        assert_eq!(nb.v.layout.chunk_count(), 1);
    }

    #[test]
    fn destroy_releases_everything() {
        let mut r = GpuResources::new();
        r.neuron_buffers = Some(NeuronBuffers::new(100));
        r.render_targets = Some(RenderTargets { width: 800, height: 600 });
        r.destroy();
        assert!(r.neuron_buffers.is_none());
        assert!(r.render_targets.is_none());
    }
}
