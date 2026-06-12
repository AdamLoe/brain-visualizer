//! Cortical manifold generation (BV13, BV17).
//!
//! Pipeline:
//! 1. icosphere subdivision (`icosphere.rs`),
//! 2. apply a deterministic brain-shaped envelope to the sphere directions,
//! 3. apply structured procedural gyrification along outward directions
//!    (`gyrify.rs`),
//! 4. sample N neuron positions inside the same folded envelope with
//!    cortical-shell bias,
//! 5. assign Input/Association/Output regions with the selected deterministic
//!    assignment mode (`regions.rs`),
//! 6. build the integer spatial grid for connectivity & stimulation lookup.
//!
//! Entirely host-testable; no GPU / wasm dependency.

pub mod gyrify;
pub mod icosphere;
pub mod regions;

pub use regions::{RegionAssignmentMode, RegionKind};

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
const BRAIN_AXES: [f32; 3] = [0.94, 0.74, 1.18];

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
    /// Region assignment strategy. Defaults to the production hash-random split.
    pub region_assignment: RegionAssignmentMode,
}

impl ManifoldParams {
    pub fn new(n: usize, seed: u32) -> Self {
        Self {
            subdivisions: 5,
            n,
            seed,
            grid_dim: DEFAULT_GRID_DIM,
            gyrify: GyrifyParams::default(),
            region_assignment: RegionAssignmentMode::HashRandom,
        }
    }

    pub fn with_region_assignment(mut self, mode: RegionAssignmentMode) -> Self {
        self.region_assignment = mode;
        self
    }
}

impl Manifold {
    /// Generate the full manifold. Deterministic for a given `ManifoldParams`.
    pub fn generate(params: &ManifoldParams) -> Self {
        let ico = icosphere::icosphere(params.subdivisions);
        let fold_field = gyrify::FoldField::new(params.gyrify, params.seed);
        let base_vertices: Vec<[f32; 3]> = ico
            .vertices
            .iter()
            .copied()
            .map(brain_surface_point)
            .collect();
        let vertices = gyrify::gyrify_with_field(&base_vertices, &fold_field);
        let faces = ico.faces;

        let neuron_positions = place_neurons(params.n, params.seed, &fold_field);
        let neuron_regions = regions::assign_regions_with_mode(
            &neuron_positions,
            ANTERIOR_POSTERIOR_AXIS,
            params.region_assignment,
        );
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
    let ax = x.abs();
    let ellipsoid = ellipsoid_radius(d, BRAIN_AXES);

    let dorsal_mask = smoothstep(-0.15, 0.82, y);
    let ventral_mask = smoothstep(0.03, 0.95, -y);
    let frontal_fullness = 0.075 * gaussian(z, -0.62, 0.32);
    let parietal_dome = 0.085 * gaussian(z, 0.12, 0.48) * dorsal_mask;
    let occipital_rounding = 0.075 * gaussian(z, 0.77, 0.26);
    let hemisphere_shoulder =
        0.070 * gaussian(ax, 0.56, 0.34) * gaussian(z, 0.04, 0.78) * smoothstep(-0.38, 0.72, y);
    let temporal_lobe =
        0.165 * gaussian(ax, 0.84, 0.22) * gaussian(y, -0.24, 0.34) * gaussian(z, -0.04, 0.56);
    let ventral_flatten = 0.150 * ventral_mask;
    let lower_rear_taper = 0.075 * gaussian(z, 0.90, 0.22) * ventral_mask;
    let midline_fissure = 0.390 * gaussian(ax, 0.00, 0.125) * dorsal_mask * gaussian(z, 0.06, 0.78);
    let medial_waist =
        0.045 * gaussian(ax, 0.00, 0.20) * gaussian(z, 0.00, 0.95) * smoothstep(-0.04, 0.70, y);

    let scale = (1.0
        + frontal_fullness
        + parietal_dome
        + occipital_rounding
        + hemisphere_shoulder
        + temporal_lobe
        - ventral_flatten
        - lower_rear_taper
        - midline_fissure
        - medial_waist)
        .clamp(0.55, 1.35);

    ellipsoid * scale
}

#[inline]
fn brain_surface_point(dir: [f32; 3]) -> [f32; 3] {
    scale(normalize(dir), brain_outer_radius(dir))
}

fn folded_outer_radius(dir: [f32; 3], fold_field: &gyrify::FoldField) -> f32 {
    fold_field.folded_radius(brain_outer_radius(dir), dir)
}

/// Shell-biased neuron placement inside the folded envelope that defines the
/// manifold surface. Most neurons sit in the outer cortical band, with a small
/// deterministic interior fill so the cloud still has some depth.
fn place_neurons(n: usize, seed: u32, fold_field: &gyrify::FoldField) -> Vec<[f32; 3]> {
    use std::f32::consts::TAU;

    let mut out = Vec::with_capacity(n);
    for i in 0..n as u32 {
        let cos_theta = hash_to_unit(mix_key(seed, i, 0, placement_salt::DIR_COS)) * 2.0 - 1.0;
        let phi = hash_to_unit(mix_key(seed, i, 0, placement_salt::DIR_PHI)) * TAU;
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
        let dir = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
        let outer_radius = folded_outer_radius(dir, fold_field);

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

#[cfg(test)]
#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
#[inline]
fn shell_depth_ratio(pos: [f32; 3], fold_field: &gyrify::FoldField) -> f32 {
    let r = length(pos);
    if r <= 1e-6 {
        0.0
    } else {
        r / folded_outer_radius(scale(pos, 1.0 / r), fold_field)
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
    fn default_region_assignment_mode_is_hash_random() {
        let p = ManifoldParams::new(128, 11);
        assert_eq!(p.region_assignment, RegionAssignmentMode::HashRandom);
        let m = Manifold::generate(&p);
        let expected = regions::assign_regions(&m.neuron_positions, ANTERIOR_POSTERIOR_AXIS);
        assert_eq!(m.neuron_regions, expected);
    }

    #[test]
    fn prototype_region_assignment_mode_is_opt_in() {
        let p = ManifoldParams::new(2000, 11)
            .with_region_assignment(RegionAssignmentMode::AnteriorPosteriorPrototype);
        let m = Manifold::generate(&p);
        let expected = regions::assign_regions_with_mode(
            &m.neuron_positions,
            ANTERIOR_POSTERIOR_AXIS,
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );
        assert_eq!(m.neuron_regions, expected);
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
        let ventral = brain_outer_radius([0.0, -1.0, 0.0]);
        let temporal = brain_outer_radius(normalize([0.90, -0.24, -0.05]));
        let lower_lateral = brain_outer_radius(normalize([0.55, -0.24, -0.05]));

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
        assert!(
            lateral_top > ventral + 0.08,
            "ventral side should read flatter/shallower than the lateral dorsal crown: lateral_top={lateral_top} ventral={ventral}"
        );
        assert!(
            temporal > lower_lateral + 0.04,
            "temporal lobe should add lower lateral fullness: temporal={temporal} lower_lateral={lower_lateral}"
        );
    }

    #[test]
    fn neurons_stay_inside_folded_brain_envelope() {
        let p = ManifoldParams::new(3000, 3);
        let m = Manifold::generate(&p);
        let fold_field = gyrify::FoldField::new(p.gyrify, p.seed);
        for pos in &m.neuron_positions {
            let shell_ratio = shell_depth_ratio(*pos, &fold_field);
            assert!(
                shell_ratio <= 1.0 + 1e-4,
                "neuron escaped folded brain envelope depth={shell_ratio}"
            );
        }
    }

    #[test]
    fn placement_is_cortical_shell_biased() {
        let p = ManifoldParams::new(12_000, 17);
        let m = Manifold::generate(&p);
        let fold_field = gyrify::FoldField::new(p.gyrify, p.seed);
        let mut outer_shell = 0usize;
        let mut interior = 0usize;
        for &pos in &m.neuron_positions {
            let depth = shell_depth_ratio(pos, &fold_field);
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
    fn folded_surface_stays_inside_interaction_radius() {
        for seed in [1, 7, 31] {
            let p = ManifoldParams::new(8000, seed);
            let m = Manifold::generate(&p);
            let max_surface = m.vertices.iter().copied().map(length).fold(0.0, f32::max);
            let max_neuron = m
                .neuron_positions
                .iter()
                .copied()
                .map(length)
                .fold(0.0, f32::max);
            assert!(
                max_surface <= 1.40,
                "surface escaped interaction radius seed={seed}: {max_surface}"
            );
            assert!(
                max_neuron <= 1.40,
                "neuron escaped interaction radius seed={seed}: {max_neuron}"
            );
        }
    }

    #[test]
    fn stimulation_radius_samples_surface_population() {
        const STIM_RADIUS: f32 = 0.15;
        let p = ManifoldParams::new(12_000, 41);
        let m = Manifold::generate(&p);
        let fold_field = gyrify::FoldField::new(p.gyrify, p.seed);

        for dir in [
            normalize([0.55, 0.78, 0.05]),
            normalize([-0.55, 0.78, 0.05]),
            normalize([0.82, -0.20, -0.05]),
            normalize([-0.82, -0.20, -0.05]),
            normalize([0.00, 0.10, 1.00]),
            normalize([0.00, 0.10, -1.00]),
        ] {
            let center = scale(dir, folded_outer_radius(dir, &fold_field));
            let count = m
                .neuron_positions
                .iter()
                .filter(|&&pos| length(sub(pos, center)) <= STIM_RADIUS)
                .count();
            assert!(
                count >= 8,
                "surface stimulation sample too sparse dir={dir:?}: {count}"
            );
        }
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

    #[test]
    fn directional_radius_metrics_are_within_bounds() {
        let p = ManifoldParams::new(10_000, 1);
        let m = Manifold::generate(&p);
        let fold_field = gyrify::FoldField::new(p.gyrify, p.seed);
        let samples = [
            ("anterior", [0.0, 0.0, -1.0]),
            ("posterior", [0.0, 0.0, 1.0]),
            ("lateral_r", [1.0, 0.0, 0.0]),
            ("dorsal_mid", [0.0, 1.0, 0.0]),
            ("ventral_mid", [0.0, -1.0, 0.0]),
            ("temporal_r", [0.90, -0.24, -0.05]),
            ("fissure_mid", [0.04, 0.99, 0.02]),
            ("lateral_top", [0.55, 0.82, 0.0]),
        ];

        eprintln!("direction smooth_radius folded_radius fold_scale");
        for (name, dir) in samples {
            let dir = normalize(dir);
            let smooth = brain_outer_radius(dir);
            let folded = folded_outer_radius(dir, &fold_field);
            eprintln!(
                "{name:>11} {smooth:>13.4} {folded:>13.4} {:>10.4}",
                folded / smooth
            );
        }

        let max_surface = m.vertices.iter().copied().map(length).fold(0.0, f32::max);
        let max_neuron = m
            .neuron_positions
            .iter()
            .copied()
            .map(length)
            .fold(0.0, f32::max);
        let occupied = (0..m.spatial_grid.cell_count())
            .filter(|&cell| !m.spatial_grid.neurons_in_cell(cell).is_empty())
            .count();
        let max_occupancy = (0..m.spatial_grid.cell_count())
            .map(|cell| m.spatial_grid.neurons_in_cell(cell).len())
            .max()
            .unwrap_or(0);
        eprintln!(
            "bounds max_surface={max_surface:.4} max_neuron={max_neuron:.4} occupied_cells={occupied} max_cell_occupancy={max_occupancy}"
        );

        assert!(max_surface <= 1.40, "surface max radius {max_surface}");
        assert!(max_neuron <= 1.40, "neuron max radius {max_neuron}");
        assert!(occupied > 250, "occupied cells {occupied}");
        assert!(max_occupancy < 220, "max cell occupancy {max_occupancy}");
    }
}
