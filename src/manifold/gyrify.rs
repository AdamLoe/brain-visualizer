//! Noise-based gyrification — turns the smooth icosphere into a folded cortex.
//!
//! Two octaves of OpenSimplex noise displace each vertex along its outward
//! normal (BV13, phase-1 doc step 2):
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
    pub gyri_freq: f64,
    pub gyri_amp: f32,
    pub sulci_freq: f64,
    pub sulci_amp: f32,
}

impl Default for GyrifyParams {
    fn default() -> Self {
        Self {
            radius: 1.0,
            gyri_freq: 1.5,
            gyri_amp: 0.15,
            sulci_freq: 4.0,
            sulci_amp: 0.05,
        }
    }
}

/// Displace unit-sphere vertices along their normal (which equals the position
/// for a unit sphere) by the two-octave noise field. Returns folded vertices at
/// roughly `radius`. Deterministic in `seed`.
pub fn gyrify(unit_vertices: &[[f32; 3]], params: &GyrifyParams, seed: u32) -> Vec<[f32; 3]> {
    // Two independent noise fields (different seeds) so octaves decorrelate.
    let gyri = OpenSimplex::new(seed);
    let sulci = OpenSimplex::new(seed.wrapping_add(0x9e37_79b9));

    unit_vertices
        .iter()
        .map(|&p| {
            let n = normalize(p); // outward normal == direction on unit sphere
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
            // Noise in [-1, 1] (OpenSimplex range is roughly [-1,1]).
            let g = gyri.get(gp) as f32;
            let s = sulci.get(sp) as f32;
            let displacement = params.radius
                + g * params.gyri_amp * params.radius
                + s * params.sulci_amp * params.radius;
            [n[0] * displacement, n[1] * displacement, n[2] * displacement]
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
        assert!(max - min > 0.05, "surface is too smooth: range {}", max - min);
        // Within [1 - (0.15+0.05) - eps, 1 + (0.15+0.05) + eps].
        assert!(min > 0.70 && max < 1.30, "folds out of expected band");
    }
}
