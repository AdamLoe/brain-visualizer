//! Cortical manifold generation (BV13, BV17).
//!
//! Pipeline:
//! 1. icosphere subdivision (`icosphere.rs`),
//! 2. apply a deterministic brain-shaped envelope to the sphere directions,
//! 3. two-octave simplex gyrification along outward directions (`gyrify.rs`),
//! 4. sample N neuron positions inside the same envelope with cortical-shell
//!    bias,
//! 5. assign Input/Association/Output regions by deterministic hash shuffle
//!    (`regions.rs`),
//! 6. build the integer spatial grid for connectivity & stimulation lookup.
//!
//! Entirely host-testable; no GPU / wasm dependency.

pub mod gyrify;
pub mod icosphere;
pub mod regions;

pub use regions::RegionKind;

use crate::connectivity::hash::mix_key;
use crate::connectivity::spatial::SpatialGrid;
use gyrify::GyrifyParams;

/// Fixed anterior–posterior axis used for region assignment. +Z = posterior
/// (Input); -Z = anterior (Output). Connectivity's forward bias is +Z to match.
pub const ANTERIOR_POSTERIOR_AXIS: [f32; 3] = [0.0, 0.0, 1.0];

/// Default spatial-grid resolution (cells per axis). dim=16 → 4096 cells, a
/// reasonable density for ~10k–50k neurons on the folded surface.
pub const DEFAULT_GRID_DIM: u32 = 16;

/// Base ellipsoid semiaxes for the coarse brain envelope.
const BRAIN_AXES: [f32; 3] = [0.92, 0.78, 1.30];

mod placement_salt {
    pub const DIR_COS: u32 = 0x0003_0001;
    pub const DIR_PHI: u32 = 0x0003_0002;
    pub const SHELL_DEPTH: u32 = 0x0003_0003;
    pub const INTERIOR_MIX: u32 = 0x0003_0004;
    pub const INTERIOR_DEPTH: u32 = 0x0003_0005;
}

/// The generated cortical surface plus neuron placement and spatial index.
pub struct Manifold {
    /// Folded surface vertices.
    pub vertices: Vec<[f32; 3]>,
    /// Surface triangle indices.
    pub faces: Vec<[u32; 3]>,
    /// N neuron positions inside the cortical volume.
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
        let ico = icosphere::icosphere(params.subdivisions);
        let base_vertices: Vec<[f32; 3]> = ico
            .vertices
            .iter()
            .copied()
            .map(brain_surface_point)
            .collect();
        let vertices = gyrify::gyrify(&base_vertices, &params.gyrify, params.seed);
        let faces = ico.faces;

        let neuron_positions = place_neurons(params.n, params.seed);
        let neuron_regions = regions::assign_regions(&neuron_positions, ANTERIOR_POSTERIOR_AXIS);
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

/// Deterministic brain-shaped envelope shared by shell generation and neuron
/// placement. The shape is intentionally approximate: elongated
/// anterior–posterior, compressed superior–inferior, bilaterally fuller through
/// the temporal sides, and indented at the dorsal midline for the longitudinal
/// fissure.
fn brain_outer_radius(dir: [f32; 3]) -> f32 {
    let d = normalize(dir);
    let [x, y, z] = d;
    let ellipsoid = ellipsoid_radius(d, BRAIN_AXES);

    let frontal = 0.08 * gaussian(z, -0.55, 0.32);
    let parietal = 0.05 * gaussian(z, 0.10, 0.40);
    let occipital = 0.10 * gaussian(z, 0.80, 0.22);
    let temporal =
        0.09 * gaussian(x.abs(), 0.82, 0.24) * gaussian(y, -0.20, 0.42) * gaussian(z, 0.00, 0.60);
    let dorsal_fullness = 0.04 * smoothstep(-0.10, 0.75, y);
    let ventral_flatten = 0.14 * smoothstep(0.10, 0.95, -y);
    let fissure =
        0.32 * gaussian(x.abs(), 0.00, 0.15) * smoothstep(-0.05, 0.85, y) * gaussian(z, 0.10, 0.78);
    let lower_rear_taper = 0.06 * gaussian(z, 0.95, 0.20) * smoothstep(0.05, 0.95, -y);

    let scale = (1.0 + frontal + parietal + occipital + temporal + dorsal_fullness
        - ventral_flatten
        - fissure
        - lower_rear_taper)
        .clamp(0.55, 1.35);

    ellipsoid * scale
}

#[inline]
fn brain_surface_point(dir: [f32; 3]) -> [f32; 3] {
    scale(normalize(dir), brain_outer_radius(dir))
}

/// Shell-biased neuron placement inside the same envelope that defines the
/// manifold surface. Most neurons sit in the outer cortical band, with a small
/// deterministic interior fill so the cloud still has some depth.
fn place_neurons(n: usize, seed: u32) -> Vec<[f32; 3]> {
    use std::f32::consts::TAU;

    let mut out = Vec::with_capacity(n);
    for i in 0..n as u32 {
        let cos_theta = hash_to_unit(mix_key(seed, i, 0, placement_salt::DIR_COS)) * 2.0 - 1.0;
        let phi = hash_to_unit(mix_key(seed, i, 0, placement_salt::DIR_PHI)) * TAU;
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
        let dir = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
        let outer_radius = brain_outer_radius(dir);

        let shell_u = hash_to_unit(mix_key(seed, i, 0, placement_salt::SHELL_DEPTH));
        let interior_mix = hash_to_unit(mix_key(seed, i, 0, placement_salt::INTERIOR_MIX));
        let depth = if interior_mix < 0.08 {
            0.25 + 0.45
                * hash_to_unit(mix_key(seed, i, 0, placement_salt::INTERIOR_DEPTH)).powf(0.8)
        } else {
            1.0 - 0.28 * shell_u.powf(2.4)
        };

        out.push(scale(dir, outer_radius * depth));
    }
    out
}

#[inline]
fn hash_to_unit(h: u32) -> f32 {
    (h as f32) / (u32::MAX as f32 + 1.0)
}

#[inline]
fn ellipsoid_radius(dir: [f32; 3], axes: [f32; 3]) -> f32 {
    let denom = (dir[0] * dir[0] / (axes[0] * axes[0])
        + dir[1] * dir[1] / (axes[1] * axes[1])
        + dir[2] * dir[2] / (axes[2] * axes[2]))
        .sqrt()
        .max(1e-6);
    1.0 / denom
}

#[inline]
fn gaussian(x: f32, center: f32, sigma: f32) -> f32 {
    let sigma = sigma.max(1e-4);
    let t = (x - center) / sigma;
    (-0.5 * t * t).exp()
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = length(v);
    if len <= 1e-6 {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

#[inline]
fn scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

#[inline]
fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
#[inline]
fn shell_depth_ratio(pos: [f32; 3]) -> f32 {
    let r = length(pos);
    if r <= 1e-6 {
        0.0
    } else {
        r / brain_outer_radius(scale(pos, 1.0 / r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectivity;
    use crate::sim::backend::neuron_type_byte;

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
        assert_eq!(a.vertices, b.vertices);
    }

    #[test]
    fn envelope_is_elongated_with_midline_fissure() {
        let lateral_top = brain_outer_radius(normalize([0.55, 0.82, 0.0]));
        let midline_top = brain_outer_radius(normalize([0.04, 0.99, 0.0]));
        let ap = brain_outer_radius([0.0, 0.0, 1.0]);
        let dorsal = brain_outer_radius([0.0, 1.0, 0.0]);
        let lateral = brain_outer_radius([1.0, 0.0, 0.0]);

        assert!(
            ap > dorsal,
            "brain should be longer anterior-posterior than tall"
        );
        assert!(
            ap > lateral,
            "brain should be longer anterior-posterior than wide"
        );
        assert!(
            lateral_top > midline_top + 0.12,
            "dorsal fissure should indent the top midline"
        );
    }

    #[test]
    fn neurons_stay_inside_brain_envelope() {
        let p = ManifoldParams::new(3000, 3);
        let m = Manifold::generate(&p);
        for pos in &m.neuron_positions {
            let shell_ratio = shell_depth_ratio(*pos);
            assert!(
                shell_ratio <= 1.0 + 1e-4,
                "neuron escaped brain envelope depth={shell_ratio}"
            );
        }
    }

    #[test]
    fn placement_is_cortical_shell_biased() {
        let p = ManifoldParams::new(12_000, 17);
        let m = Manifold::generate(&p);
        let mut outer_shell = 0usize;
        let mut interior = 0usize;
        for &pos in &m.neuron_positions {
            let depth = shell_depth_ratio(pos);
            if depth >= 0.72 {
                outer_shell += 1;
            }
            if depth <= 0.70 {
                interior += 1;
            }
        }

        let n = m.neuron_positions.len() as f32;
        assert!(
            outer_shell as f32 / n > 0.88,
            "too few neurons in cortical shell: {outer_shell}/{n}"
        );
        assert!(
            interior as f32 / n < 0.12,
            "interior fill is too dense: {interior}/{n}"
        );
    }

    #[test]
    fn shell_bias_still_populates_spatial_grid() {
        let p = ManifoldParams::new(10_000, 23);
        let m = Manifold::generate(&p);
        let occupied = (0..m.spatial_grid.cell_count())
            .filter(|&cell| !m.spatial_grid.neurons_in_cell(cell).is_empty())
            .count();
        let max_occupancy = (0..m.spatial_grid.cell_count())
            .map(|cell| m.spatial_grid.neurons_in_cell(cell).len())
            .max()
            .unwrap_or(0);

        assert!(occupied > 250, "too few occupied cells: {occupied}");
        assert!(
            max_occupancy < 220,
            "cell occupancy too clumped for default grid: {max_occupancy}"
        );
    }

    #[test]
    fn connectivity_rule_remains_deterministic_and_in_range() {
        let p = ManifoldParams::new(4000, 31);
        let m = Manifold::generate(&p);
        let seed = p.seed;
        let k = 16usize;
        let cell_of_neuron = m.spatial_grid.cell_of_neuron_map();

        for i in (0..m.neuron_positions.len()).step_by(137).take(24) {
            let src_type = neuron_type_byte(i as u32, seed, m.neuron_regions[i]);
            let src_cell = m.spatial_grid.unpack(cell_of_neuron[i]);
            for j in 0..k as u32 {
                let a = connectivity::target_with_cell(
                    i as u32,
                    j,
                    &m.spatial_grid,
                    k,
                    seed,
                    src_type,
                    src_cell,
                    connectivity::ReachParams::LOCAL_ONLY,
                );
                let b = connectivity::target_with_cell(
                    i as u32,
                    j,
                    &m.spatial_grid,
                    k,
                    seed,
                    src_type,
                    src_cell,
                    connectivity::ReachParams::LOCAL_ONLY,
                );
                assert_eq!(a, b, "target drifted for neuron {i}, synapse {j}");
                assert!(
                    (a as usize) < m.neuron_positions.len(),
                    "target out of range for neuron {i}, synapse {j}: {a}"
                );
            }
        }
    }
}
