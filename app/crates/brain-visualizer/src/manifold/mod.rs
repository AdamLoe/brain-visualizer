//! Cortical manifold generation (BV13, BV17).
//!
//! Pipeline (phase-1 doc):
//! 1. icosphere subdivision (`icosphere.rs`),
//! 2. two-octave simplex gyrification along normals (`gyrify.rs`),
//! 3. sample N neuron positions on faces via random barycentric coords,
//! 4. assign Input/Association/Output regions by anterior–posterior rank
//!    (`regions.rs`),
//! 5. build the integer spatial grid for connectivity & stimulation lookup.
//!
//! Entirely host-testable; no GPU / wasm dependency.

pub mod gyrify;
pub mod icosphere;
pub mod regions;

pub use regions::RegionKind;

use crate::connectivity::hash::hash32;
use crate::connectivity::spatial::SpatialGrid;
use gyrify::GyrifyParams;

/// Fixed anterior–posterior axis used for region assignment. +Z = posterior
/// (Input); -Z = anterior (Output). Connectivity's forward bias is +Z to match.
pub const ANTERIOR_POSTERIOR_AXIS: [f32; 3] = [0.0, 0.0, 1.0];

/// Default spatial-grid resolution (cells per axis). dim=16 → 4096 cells, a
/// reasonable density for ~10k–50k neurons on the folded surface.
pub const DEFAULT_GRID_DIM: u32 = 16;

/// The generated cortical surface plus neuron placement and spatial index.
pub struct Manifold {
    /// Folded surface vertices.
    pub vertices: Vec<[f32; 3]>,
    /// Surface triangle indices.
    pub faces: Vec<[u32; 3]>,
    /// N neuron positions on the surface.
    pub neuron_positions: Vec<[f32; 3]>,
    /// Per-neuron region class.
    pub neuron_regions: Vec<RegionKind>,
    /// Integer spatial grid for connectivity + stimulation lookup.
    pub spatial_grid: SpatialGrid,
}

/// Manifold generation parameters.
#[derive(Debug, Clone, Copy)]
pub struct ManifoldParams {
    /// Icosphere subdivision level (4–5 → ~2.5k–10k verts).
    pub subdivisions: u32,
    /// Number of neurons to place.
    pub n: usize,
    /// Network seed (drives both gyrification and placement, deterministically).
    pub seed: u32,
    /// Spatial grid resolution (cells per axis).
    pub grid_dim: u32,
    /// Gyrification controls.
    pub gyrify: GyrifyParams,
}

impl ManifoldParams {
    pub fn new(n: usize, seed: u32) -> Self {
        Self {
            subdivisions: 5,
            n,
            seed,
            grid_dim: DEFAULT_GRID_DIM,
            gyrify: GyrifyParams::default(),
        }
    }
}

impl Manifold {
    /// Generate the full manifold. Deterministic for a given `ManifoldParams`.
    pub fn generate(params: &ManifoldParams) -> Self {
        // 1 + 2 + 3: folded surface.
        let ico = icosphere::icosphere(params.subdivisions);
        let vertices = gyrify::gyrify(&ico.vertices, &params.gyrify, params.seed);
        let faces = ico.faces;

        // 4: place N neurons via random barycentric coords on random faces.
        let neuron_positions = place_neurons(&vertices, &faces, params.n, params.seed);

        // 5: regions by anterior–posterior rank.
        let neuron_regions = regions::assign_regions(&neuron_positions, ANTERIOR_POSTERIOR_AXIS);

        // 6: integer spatial grid.
        let spatial_grid = SpatialGrid::build(&neuron_positions, params.grid_dim);

        Manifold {
            vertices,
            faces,
            neuron_positions,
            neuron_regions,
            spatial_grid,
        }
    }
}

/// Radius of the neuron volume sphere (matches the gyrified manifold scale).
const SPHERE_RADIUS: f32 = 1.0;

/// Sample `n` points uniformly inside a sphere using spherical coordinates.
/// cos(θ) uniform in [-1,1] + φ uniform in [0,2π] gives uniform direction;
/// r = R * cbrt(uniform) gives uniform volume density.
///
/// Deterministic & integer-seeded; does not use the surface mesh at all.
fn place_neurons(
    _vertices: &[[f32; 3]],
    _faces: &[[u32; 3]],
    n: usize,
    seed: u32,
) -> Vec<[f32; 3]> {
    use std::f32::consts::TAU;
    let mut out = Vec::with_capacity(n);
    let scale = 1.0 / (u32::MAX as f32 + 1.0);
    for i in 0..n as u32 {
        let hc = hash32(seed ^ i.wrapping_mul(0x9e37_79b1));
        let hp = hash32(seed ^ i.wrapping_mul(0x85eb_ca6b));
        let hr = hash32(seed ^ i.wrapping_mul(0xc2b2_ae35));

        let cos_theta = (hc as f32) * scale * 2.0 - 1.0;          // [-1, 1]
        let phi       = (hp as f32) * scale * TAU;                  // [0, 2π]
        let r         = (hr as f32 * scale).cbrt() * SPHERE_RADIUS; // uniform volume

        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
        out.push([
            r * sin_theta * phi.cos(),
            r * sin_theta * phi.sin(),
            r * cos_theta,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_requested_neuron_count() {
        let p = ManifoldParams::new(5000, 1);
        let m = Manifold::generate(&p);
        assert_eq!(m.neuron_positions.len(), 5000);
        assert_eq!(m.neuron_regions.len(), 5000);
    }

    #[test]
    fn region_split_approx_30_40_30() {
        let p = ManifoldParams::new(10_000, 7);
        let m = Manifold::generate(&p);
        let count = |k: RegionKind| m.neuron_regions.iter().filter(|&&r| r == k).count();
        let input = count(RegionKind::Input);
        let assoc = count(RegionKind::Association);
        let output = count(RegionKind::Output);
        let n = m.neuron_positions.len() as f32;
        assert!((input as f32 / n - 0.30).abs() < 0.03, "input frac off");
        assert!((output as f32 / n - 0.30).abs() < 0.03, "output frac off");
        assert!((assoc as f32 / n - 0.40).abs() < 0.05, "assoc frac off");
    }

    #[test]
    fn deterministic() {
        let p = ManifoldParams::new(2000, 99);
        let a = Manifold::generate(&p);
        let b = Manifold::generate(&p);
        assert_eq!(a.neuron_positions, b.neuron_positions);
    }

    #[test]
    fn neurons_fill_sphere() {
        let p = ManifoldParams::new(3000, 3);
        let m = Manifold::generate(&p);
        for pos in &m.neuron_positions {
            let r = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt();
            assert!(r <= SPHERE_RADIUS + 1e-4, "neuron outside sphere r={r}");
        }
    }
}
