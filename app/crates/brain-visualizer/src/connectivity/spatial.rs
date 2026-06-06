//! Integer spatial hash grid over the cortical manifold.
//!
//! Serves three jobs across phases (architecture §"Spatial structures"):
//! procedural local connectivity, cursor-stimulation lookup, and near-LOD
//! culling. Phase 1 builds it once at startup (geometry is static).
//!
//! Hot-path hygiene (architecture §10.1): cells are addressed by a **packed
//! `u32` linear id**, never by string keys. Neuron membership is stored CSR-
//! style (`cell_start` offsets into a flat `cell_neurons` array) so a cell's
//! occupants are a contiguous slice with zero per-query allocation.

/// A uniform grid over an axis-aligned bounding box. `dim` cells per axis.
#[derive(Debug, Clone)]
pub struct SpatialGrid {
    /// Minimum corner of the bounding box (world space).
    pub min: [f32; 3],
    /// Cell edge length (world units).
    pub cell_size: f32,
    /// Number of cells along each axis. Packed id = x + y*dim + z*dim*dim.
    pub dim: u32,
    /// CSR offsets: `cell_start[c]..cell_start[c+1]` indexes `cell_neurons`.
    /// Length = dim^3 + 1.
    pub cell_start: Vec<u32>,
    /// Neuron ids grouped by cell, contiguous per cell.
    pub cell_neurons: Vec<u32>,
}

impl SpatialGrid {
    /// Total number of cells (`dim^3`).
    #[inline]
    pub fn cell_count(&self) -> u32 {
        self.dim * self.dim * self.dim
    }

    /// Clamp a single world coordinate to a valid integer cell coordinate on
    /// one axis. Integer-only after the initial quantization.
    #[inline]
    fn axis_coord(&self, world: f32, axis: usize) -> u32 {
        let c = ((world - self.min[axis]) / self.cell_size).floor();
        // Clamp into [0, dim-1]; NaN floors to 0 via the unsigned cast guard.
        if c < 0.0 {
            0
        } else {
            let ci = c as u32;
            ci.min(self.dim - 1)
        }
    }

    /// Integer cell coordinate `(cx, cy, cz)` for a world position.
    #[inline]
    pub fn cell_coord(&self, pos: [f32; 3]) -> [u32; 3] {
        [
            self.axis_coord(pos[0], 0),
            self.axis_coord(pos[1], 1),
            self.axis_coord(pos[2], 2),
        ]
    }

    /// Pack integer cell coordinate into a linear `u32` id.
    #[inline]
    pub fn pack(&self, c: [u32; 3]) -> u32 {
        c[0] + c[1] * self.dim + c[2] * self.dim * self.dim
    }

    /// Unpack a linear cell id back into integer coordinates.
    #[inline]
    pub fn unpack(&self, id: u32) -> [u32; 3] {
        let x = id % self.dim;
        let y = (id / self.dim) % self.dim;
        let z = id / (self.dim * self.dim);
        [x, y, z]
    }

    /// Packed cell id for a world position (cell of a neuron).
    #[inline]
    pub fn cell_of(&self, pos: [f32; 3]) -> u32 {
        self.pack(self.cell_coord(pos))
    }

    /// Neurons resident in cell `id` (contiguous slice; empty if none).
    #[inline]
    pub fn neurons_in_cell(&self, id: u32) -> &[u32] {
        let id = id as usize;
        let lo = self.cell_start[id] as usize;
        let hi = self.cell_start[id + 1] as usize;
        &self.cell_neurons[lo..hi]
    }

    /// Build a per-neuron packed-cell-id map (`cell_of_neuron[i]` = packed cell
    /// id of neuron `i`). O(N) — inverts the CSR layout in one pass, unlike the
    /// O(N²) `cell_of_index` scan. Used to upload `cell_of_neuron` to the GPU.
    pub fn cell_of_neuron_map(&self) -> Vec<u32> {
        let n = self.cell_neurons.len();
        let mut out = vec![0u32; n];
        let cells = self.cell_count() as usize;
        for cell in 0..cells {
            let lo = self.cell_start[cell] as usize;
            let hi = self.cell_start[cell + 1] as usize;
            for slot in lo..hi {
                let neuron = self.cell_neurons[slot] as usize;
                out[neuron] = cell as u32;
            }
        }
        out
    }

    /// Build the grid from neuron positions. `dim` chosen so the grid is dense
    /// enough for local connectivity without exploding cell count.
    pub fn build(positions: &[[f32; 3]], dim: u32) -> Self {
        assert!(dim >= 1, "grid dim must be >= 1");

        // Bounding box (with a tiny epsilon so the max corner lands inside).
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in positions {
            for a in 0..3 {
                min[a] = min[a].min(p[a]);
                max[a] = max[a].max(p[a]);
            }
        }
        if positions.is_empty() {
            min = [0.0; 3];
            max = [1.0; 3];
        }

        let mut extent = 0.0f32;
        for a in 0..3 {
            extent = extent.max(max[a] - min[a]);
        }
        // Guard degenerate extent.
        let extent = if extent <= 0.0 { 1.0 } else { extent };
        let cell_size = extent / dim as f32;

        let mut grid = SpatialGrid {
            min,
            cell_size,
            dim,
            cell_start: Vec::new(),
            cell_neurons: Vec::new(),
        };

        let cell_count = grid.cell_count() as usize;

        // Counting sort: count per cell, prefix-sum to offsets, scatter.
        let mut counts = vec![0u32; cell_count];
        for p in positions {
            counts[grid.cell_of(*p) as usize] += 1;
        }

        let mut cell_start = vec![0u32; cell_count + 1];
        let mut acc = 0u32;
        for c in 0..cell_count {
            cell_start[c] = acc;
            acc += counts[c];
        }
        cell_start[cell_count] = acc;

        let mut cursor = cell_start.clone();
        let mut cell_neurons = vec![0u32; positions.len()];
        for (i, p) in positions.iter().enumerate() {
            let c = grid.cell_of(*p) as usize;
            let slot = cursor[c];
            cell_neurons[slot as usize] = i as u32;
            cursor[c] = slot + 1;
        }

        grid.cell_start = cell_start;
        grid.cell_neurons = cell_neurons;
        grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring() -> Vec<[f32; 3]> {
        // 8 points spread across a cube so multiple cells are populated.
        let mut v = Vec::new();
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    v.push([x as f32, y as f32, z as f32]);
                }
            }
        }
        v
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let g = SpatialGrid::build(&ring(), 4);
        for id in 0..g.cell_count() {
            assert_eq!(g.pack(g.unpack(id)), id);
        }
    }

    #[test]
    fn csr_partitions_all_neurons() {
        let pos = ring();
        let g = SpatialGrid::build(&pos, 4);
        // Every neuron appears exactly once across all cells.
        let total: usize = (0..g.cell_count())
            .map(|c| g.neurons_in_cell(c).len())
            .sum();
        assert_eq!(total, pos.len());
        assert_eq!(*g.cell_start.last().unwrap() as usize, pos.len());
    }

    #[test]
    fn neuron_found_in_its_own_cell() {
        let pos = ring();
        let g = SpatialGrid::build(&pos, 4);
        for (i, p) in pos.iter().enumerate() {
            let cell = g.cell_of(*p);
            assert!(
                g.neurons_in_cell(cell).contains(&(i as u32)),
                "neuron {i} missing from its cell {cell}"
            );
        }
    }

    #[test]
    fn cell_of_neuron_map_matches_scan() {
        let pos = ring();
        let g = SpatialGrid::build(&pos, 4);
        let map = g.cell_of_neuron_map();
        assert_eq!(map.len(), pos.len());
        // O(N) inverse must agree with the membership scan for every neuron.
        for i in 0..pos.len() as u32 {
            assert_eq!(map[i as usize], g.cell_of_index(i));
            // And the neuron must be resident in the cell the map names.
            assert!(g.neurons_in_cell(map[i as usize]).contains(&i));
        }
    }

    #[test]
    fn coords_clamped_in_range() {
        let g = SpatialGrid::build(&ring(), 4);
        // Way outside the box clamps, never panics / overflows.
        let c = g.cell_coord([1e9, -1e9, f32::NAN]);
        for a in 0..3 {
            assert!(c[a] < g.dim);
        }
    }
}
