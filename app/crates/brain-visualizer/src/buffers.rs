//! Chunked Structure-of-Arrays buffer layout (architecture §2, phase-1 doc).
//!
//! One logical SoA field (e.g. `v`, `pos_x`) may exceed a single WebGPU
//! storage binding (`maxStorageBufferBindingSize`, often 128 MiB). When it
//! does we split it across multiple `wgpu::Buffer`s. Shaders index via:
//!   `chunk = neuron_id / chunk_size`, `local = neuron_id % chunk_size`.
//!
//! The **layout math is fully host-testable without a GPU device** — that is
//! the part phase 1 must get right. The `wgpu::Buffer` handles are optional and
//! only populated on a real device (phase 2+). Positions remain three
//! independent 4-byte fields (`pos_x/y/z`), never `array<vec3<f32>>`, so the
//! stride stays 4 bytes and the memory budget holds.

/// Conservative per-chunk byte budget: 64 MiB. Fits even integrated GPUs and
/// stays well under the 128 MiB default binding size.
pub const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;

/// Layout description for one chunked SoA field. Pure data + math; no device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLayout {
    /// Neurons per chunk (a power-of-two-friendly count chosen from the budget).
    pub chunk_size: usize,
    /// Bytes per element (4 for f32/i32/u32 fields).
    pub element_bytes: usize,
    /// Total neurons across all chunks.
    pub total: usize,
}

impl ChunkLayout {
    /// Derive a layout that keeps each chunk ≤ `MAX_CHUNK_BYTES`.
    pub fn new(total: usize, element_bytes: usize) -> Self {
        Self::with_budget(total, element_bytes, MAX_CHUNK_BYTES)
    }

    /// Same as [`ChunkLayout::new`] with an explicit byte budget (testable).
    pub fn with_budget(total: usize, element_bytes: usize, max_chunk_bytes: usize) -> Self {
        assert!(element_bytes > 0, "element_bytes must be > 0");
        assert!(
            max_chunk_bytes >= element_bytes,
            "budget smaller than one element"
        );
        let chunk_size = (max_chunk_bytes / element_bytes).max(1);
        Self {
            chunk_size,
            element_bytes,
            total,
        }
    }

    /// Number of chunks needed to hold `total` neurons.
    #[inline]
    pub fn chunk_count(&self) -> usize {
        if self.total == 0 {
            0
        } else {
            self.total.div_ceil(self.chunk_size)
        }
    }

    /// Chunk index holding `neuron_id`.
    #[inline]
    pub fn chunk_for(&self, neuron_id: usize) -> usize {
        neuron_id / self.chunk_size
    }

    /// Local index of `neuron_id` within its chunk.
    #[inline]
    pub fn local_index(&self, neuron_id: usize) -> usize {
        neuron_id % self.chunk_size
    }

    /// Byte size of chunk `chunk` (the last chunk may be partial).
    #[inline]
    pub fn chunk_bytes(&self, chunk: usize) -> usize {
        let elems = if chunk + 1 < self.chunk_count() {
            self.chunk_size
        } else if chunk + 1 == self.chunk_count() {
            self.total - chunk * self.chunk_size
        } else {
            0
        };
        elems * self.element_bytes
    }
}

/// A chunked SoA field. Holds the layout math plus the (optional) device
/// buffers. In phase 1 `chunks` is typically empty (no device); the layout
/// math is exercised by unit tests and used by the GPU resource scaffolding.
pub struct ChunkedBuffer {
    pub layout: ChunkLayout,
    /// One `wgpu::Buffer` per chunk. Empty until a device allocates them.
    pub chunks: Vec<wgpu::Buffer>,
}

impl ChunkedBuffer {
    /// Create the layout only (no allocation). Allocation happens in phase 2
    /// via `GpuResources::resize_neurons`.
    pub fn new(total: usize, element_bytes: usize) -> Self {
        Self {
            layout: ChunkLayout::new(total, element_bytes),
            chunks: Vec::new(),
        }
    }

    #[inline]
    pub fn chunk_size(&self) -> usize {
        self.layout.chunk_size
    }

    #[inline]
    pub fn total(&self) -> usize {
        self.layout.total
    }

    #[inline]
    pub fn chunk_for(&self, neuron_id: usize) -> usize {
        self.layout.chunk_for(neuron_id)
    }

    #[inline]
    pub fn local_index(&self, neuron_id: usize) -> usize {
        self.layout.local_index(neuron_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_chunk_for_typical_sizes() {
        // 1M neurons × 4 B = 4 MiB → one chunk.
        let l = ChunkLayout::new(1_000_000, 4);
        assert_eq!(l.chunk_count(), 1);
        // 64 MiB / 4 B = 16M per chunk.
        assert_eq!(l.chunk_size, 16 * 1024 * 1024);
    }

    #[test]
    fn splits_when_over_budget() {
        // 40M neurons × 4 B = 160 MiB → 3 chunks (16M each).
        let l = ChunkLayout::new(40_000_000, 4);
        assert_eq!(l.chunk_size, 16 * 1024 * 1024);
        assert_eq!(l.chunk_count(), 3);
    }

    #[test]
    fn chunk_for_and_local_index_consistent() {
        // Small budget to force multiple chunks: 16 B / 4 B = 4 per chunk.
        let l = ChunkLayout::with_budget(10, 4, 16);
        assert_eq!(l.chunk_size, 4);
        assert_eq!(l.chunk_count(), 3);
        for id in 0..10usize {
            assert_eq!(l.chunk_for(id), id / 4);
            assert_eq!(l.local_index(id), id % 4);
            // Reconstruct id from (chunk, local).
            assert_eq!(l.chunk_for(id) * l.chunk_size + l.local_index(id), id);
        }
    }

    #[test]
    fn chunk_bytes_last_is_partial() {
        let l = ChunkLayout::with_budget(10, 4, 16); // 4 per chunk, 3 chunks
        assert_eq!(l.chunk_bytes(0), 16);
        assert_eq!(l.chunk_bytes(1), 16);
        assert_eq!(l.chunk_bytes(2), 2 * 4); // last chunk holds 2 elems
        assert_eq!(l.chunk_bytes(3), 0);
    }

    #[test]
    fn empty_total() {
        let l = ChunkLayout::new(0, 4);
        assert_eq!(l.chunk_count(), 0);
    }
}
