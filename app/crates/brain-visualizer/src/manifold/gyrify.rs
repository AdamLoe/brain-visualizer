//! Structured gyrification — turns the smooth icosphere into a folded cortex.
//!
//! Three OpenSimplex noise fields plus deterministic anatomical groove masks
//! displace each vertex along its outward normal:
//! - coarse lumps (freq ~0.8): large slow bulges, amplitude ~12% of radius;
//! - large scale (freq ~1.5): gyri (ridges), amplitude ~15% of radius;
//! - small scale (freq ~4.0): sulci (fine folds), amplitude ~5% of radius.
//! - major grooves: longitudinal fissure, central sulcus, lateral sulcus, and
//!   parieto-occipital grooves, all derived from normalized coordinates.
//!
//! Pure & host-testable: `noise` is cross-platform. Deterministic for a given
//! seed.

use noise::{NoiseFn, OpenSimplex};

/// Gyrification parameters. Defaults match the phase-1 doc.
#[derive(Debug, Clone, Copy)]
pub struct GyrifyParams {
    pub radius: f32,
    pub lump_freq: f64,
    pub lump_amp: f32,
    pub gyri_freq: f64,
    pub gyri_amp: f32,
    pub sulci_freq: f64,
    pub sulci_amp: f32,
}

impl Default for GyrifyParams {
    fn default() -> Self {
        Self {
            radius: 1.0,
            lump_freq: 0.8,
            lump_amp: 0.12,
            gyri_freq: 1.5,
            gyri_amp: 0.15,
            sulci_freq: 4.0,
            sulci_amp: 0.05,
        }
    }
}

/// Deterministic fold field shared by surface generation and folded-shell
/// neuron placement.
pub struct FoldField {
    params: GyrifyParams,
    gyri: OpenSimplex,
    sulci: OpenSimplex,
    lumps: OpenSimplex,
}

impl FoldField {
    pub fn new(params: GyrifyParams, seed: u32) -> Self {
        Self {
            params,
            gyri: OpenSimplex::new(seed),
            sulci: OpenSimplex::new(seed.wrapping_add(0x9e37_79b9)),
            lumps: OpenSimplex::new(seed.wrapping_add(0x85eb_ca6b)),
        }
    }

    /// Radius multiplier for a normalized direction. Values are clamped to a
    /// conservative band so structured grooves improve readability without
    /// escaping existing interaction/framing bounds.
    pub fn radius_scale(&self, dir: [f32; 3]) -> f32 {
        let n = normalize(dir);
        let [x, y, z] = n;
        let ax = x.abs();

        let gp = [
            (x as f64) * self.params.gyri_freq,
            (y as f64) * self.params.gyri_freq,
            (z as f64) * self.params.gyri_freq,
        ];
        let sp = [
            (x as f64) * self.params.sulci_freq,
            (y as f64) * self.params.sulci_freq,
            (z as f64) * self.params.sulci_freq,
        ];
        let lp = [
            (x as f64) * self.params.lump_freq,
            (y as f64) * self.params.lump_freq,
            (z as f64) * self.params.lump_freq,
        ];

        let cortical_mask = smoothstep(-0.72, 0.18, y);
        let dorsal_mask = smoothstep(-0.12, 0.82, y);
        let lateral_mask = smoothstep(0.18, 0.72, ax);
        let fissure_mask = gaussian(ax, 0.0, 0.08) * dorsal_mask * gaussian(z, 0.04, 0.82);

        let random_relief = self.lumps.get(lp) as f32 * self.params.lump_amp * 0.42
            + self.gyri.get(gp) as f32 * self.params.gyri_amp * 0.42 * cortical_mask
            + self.sulci.get(sp) as f32 * self.params.sulci_amp * 0.55 * cortical_mask;

        let central_sulcus =
            gaussian(z + y * 0.20, 0.02, 0.08) * lateral_mask * smoothstep(-0.22, 0.70, y);
        let lateral_sulcus =
            gaussian(y, -0.23, 0.07) * smoothstep(0.36, 0.90, ax) * gaussian(z, -0.03, 0.58);
        let parieto_occipital = gaussian(z, 0.55, 0.09) * lateral_mask * smoothstep(0.02, 0.80, y);
        let temporal_groove =
            gaussian(y, -0.42, 0.10) * smoothstep(0.54, 0.96, ax) * gaussian(z, -0.02, 0.48);
        let band_phase = z * 17.0 + y * 6.5 + ax * 4.0;
        let shallow_bands = (0.5 - 0.5 * band_phase.sin()).powf(1.7) * lateral_mask * cortical_mask;

        let major_grooves = 0.125 * fissure_mask
            + 0.052 * central_sulcus
            + 0.058 * lateral_sulcus
            + 0.034 * parieto_occipital
            + 0.032 * temporal_groove
            + 0.022 * shallow_bands;

        let scale = self.params.radius * (1.0 + random_relief - major_grooves);
        scale.clamp(self.params.radius * 0.72, self.params.radius * 1.12)
    }

    #[inline]
    pub fn folded_radius(&self, base_radius: f32, dir: [f32; 3]) -> f32 {
        base_radius * self.radius_scale(dir)
    }
}

/// Displace base-envelope vertices along their outward direction by the shared
/// fold field. The local envelope radius is preserved and fold amplitudes are
/// applied as a fraction of that radius, so the same field can place neurons
/// just under the folded cortical surface.
pub fn gyrify(base_vertices: &[[f32; 3]], params: &GyrifyParams, seed: u32) -> Vec<[f32; 3]> {
    let field = FoldField::new(*params, seed);
    gyrify_with_field(base_vertices, &field)
}

pub fn gyrify_with_field(base_vertices: &[[f32; 3]], field: &FoldField) -> Vec<[f32; 3]> {
    base_vertices
        .iter()
        .map(|&p| {
            let n = normalize(p);
            let base_radius = length(p).max(1e-5);
            let radius = field.folded_radius(base_radius, n);
            [n[0] * radius, n[1] * radius, n[2] * radius]
        })
        .collect()
}

#[inline]
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

#[inline]
fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifold::icosphere::icosphere;

    #[test]
    fn deterministic_for_seed() {
        let m = icosphere(3);
        let p = GyrifyParams::default();
        let a = gyrify(&m.vertices, &p, 42);
        let b = gyrify(&m.vertices, &p, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn produces_folds_not_a_smooth_sphere() {
        let m = icosphere(4);
        let p = GyrifyParams::default();
        let folded = gyrify(&m.vertices, &p, 7);
        // Radii should vary (folds), bounded near the configured amplitudes.
        let radii: Vec<f32> = folded
            .iter()
            .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
            .collect();
        let min = radii.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = radii.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 0.05,
            "surface is too smooth: range {}",
            max - min
        );
        // Bounded by the structured fold-field clamp.
        assert!(min > 0.66 && max < 1.34, "folds out of expected band");
    }

    #[test]
    fn structured_grooves_indent_fissure_and_lateral_sulcus() {
        let p = GyrifyParams::default();
        let field = FoldField::new(p, 11);
        let lateral_top = field.radius_scale(normalize([0.60, 0.78, 0.05]));
        let fissure = field.radius_scale(normalize([0.02, 0.99, 0.05]));
        let lateral_sulcus = field.radius_scale(normalize([0.82, -0.24, -0.02]));
        let temporal_ridge = field.radius_scale(normalize([0.82, -0.05, -0.02]));

        assert!(
            fissure < lateral_top - 0.05,
            "fissure not visibly indented: fissure={fissure} lateral_top={lateral_top}"
        );
        assert!(
            lateral_sulcus < temporal_ridge - 0.03,
            "lateral sulcus not visibly indented: sulcus={lateral_sulcus} ridge={temporal_ridge}"
        );
    }
}
