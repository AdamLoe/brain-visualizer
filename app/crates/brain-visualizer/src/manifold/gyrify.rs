//! Noise-based gyrification — turns the smooth icosphere into a folded cortex.
//!
//! Three octaves of OpenSimplex noise displace each vertex along its outward
//! normal (BV13, phase-1 doc step 2):
//! - coarse lumps (freq ~0.8): large slow bulges, amplitude ~12% of radius;
//! - large scale (freq ~1.5): gyri (ridges), amplitude ~15% of radius;
//! - small scale (freq ~4.0): sulci (fine folds), amplitude ~5% of radius.
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

/// Displace base-envelope vertices along their outward direction by the
/// two-octave noise field. The local envelope radius is preserved and the fold
/// amplitudes are applied as a fraction of that radius, so the same noise
/// params work for both the old sphere and the shaped brain shell.
pub fn gyrify(base_vertices: &[[f32; 3]], params: &GyrifyParams, seed: u32) -> Vec<[f32; 3]> {
    // Three independent noise fields (distinct seed offsets) so octaves decorrelate.
    let gyri = OpenSimplex::new(seed);
    let sulci = OpenSimplex::new(seed.wrapping_add(0x9e37_79b9));
    let lumps = OpenSimplex::new(seed.wrapping_add(0x85eb_ca6b));

    base_vertices
        .iter()
        .map(|&p| {
            let n = normalize(p);
            let base_radius = length(p).max(1e-5);
            let gp = [
                (n[0] as f64) * params.gyri_freq,
                (n[1] as f64) * params.gyri_freq,
                (n[2] as f64) * params.gyri_freq,
            ];
            let sp = [
                (n[0] as f64) * params.sulci_freq,
                (n[1] as f64) * params.sulci_freq,
                (n[2] as f64) * params.sulci_freq,
            ];
            let lp = [
                (n[0] as f64) * params.lump_freq,
                (n[1] as f64) * params.lump_freq,
                (n[2] as f64) * params.lump_freq,
            ];
            // Noise in [-1, 1] (OpenSimplex range is roughly [-1,1]).
            let g = gyri.get(gp) as f32;
            let s = sulci.get(sp) as f32;
            let l = lumps.get(lp) as f32;
            let radius = base_radius
                * (params.radius
                    + l * params.lump_amp * params.radius
                    + g * params.gyri_amp * params.radius
                    + s * params.sulci_amp * params.radius);
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
        // Within [1 - (0.12+0.15+0.05) - eps, 1 + (0.12+0.15+0.05) + eps].
        assert!(min > 0.66 && max < 1.34, "folds out of expected band");
    }
}
