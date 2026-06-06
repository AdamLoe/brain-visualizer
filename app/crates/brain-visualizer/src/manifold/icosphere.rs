//! Subdivided icosahedron → near-uniform unit sphere mesh.
//!
//! Pure geometry, host-testable. Level 4 → ~2.5k verts, level 5 → ~10k verts
//! (phase-1 doc target ~5k–20k). Vertices are unit-length; gyrification
//! (`gyrify.rs`) then displaces them along their normals.

use std::collections::HashMap;

/// A triangle mesh on the unit sphere.
#[derive(Debug, Clone)]
pub struct IcoMesh {
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<[u32; 3]>,
}

#[inline]
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Build the 12-vertex / 20-face base icosahedron (unit radius).
fn base_icosahedron() -> IcoMesh {
    // Golden ratio.
    let t = (1.0 + 5.0_f32.sqrt()) / 2.0;
    let raw = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    let vertices = raw.into_iter().map(normalize).collect();
    let faces = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    IcoMesh { vertices, faces }
}

/// Subdivide `mesh` once: each triangle → 4, midpoints projected to the sphere.
/// A midpoint cache keyed on the undirected edge keeps the mesh watertight and
/// vertex count minimal.
fn subdivide(mesh: &IcoMesh) -> IcoMesh {
    let mut vertices = mesh.vertices.clone();
    let mut faces = Vec::with_capacity(mesh.faces.len() * 4);
    let mut cache: HashMap<(u32, u32), u32> = HashMap::new();

    let mut midpoint = |a: u32, b: u32, verts: &mut Vec<[f32; 3]>| -> u32 {
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(&idx) = cache.get(&key) {
            return idx;
        }
        let va = verts[a as usize];
        let vb = verts[b as usize];
        let mid = normalize([
            (va[0] + vb[0]) * 0.5,
            (va[1] + vb[1]) * 0.5,
            (va[2] + vb[2]) * 0.5,
        ]);
        let idx = verts.len() as u32;
        verts.push(mid);
        cache.insert(key, idx);
        idx
    };

    for &[a, b, c] in &mesh.faces {
        let ab = midpoint(a, b, &mut vertices);
        let bc = midpoint(b, c, &mut vertices);
        let ca = midpoint(c, a, &mut vertices);
        faces.push([a, ab, ca]);
        faces.push([b, bc, ab]);
        faces.push([c, ca, bc]);
        faces.push([ab, bc, ca]);
    }

    IcoMesh { vertices, faces }
}

/// Build an icosphere subdivided `levels` times (4–5 recommended).
pub fn icosphere(levels: u32) -> IcoMesh {
    let mut mesh = base_icosahedron();
    for _ in 0..levels {
        mesh = subdivide(&mesh);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_count_grows_as_expected() {
        // V = 10 * 4^level + 2 for an icosphere.
        for level in 0..=5u32 {
            let m = icosphere(level);
            let expected = 10 * 4u32.pow(level) + 2;
            assert_eq!(m.vertices.len() as u32, expected, "level {level}");
            assert_eq!(m.faces.len() as u32, 20 * 4u32.pow(level));
        }
    }

    #[test]
    fn level5_in_target_range() {
        let m = icosphere(5);
        assert!(
            (5_000..=20_000).contains(&m.vertices.len()),
            "level5 verts {} out of doc range",
            m.vertices.len()
        );
    }

    #[test]
    fn all_vertices_unit_length() {
        let m = icosphere(3);
        for v in &m.vertices {
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-4, "vertex not on unit sphere");
        }
    }

    #[test]
    fn face_indices_valid() {
        let m = icosphere(2);
        for f in &m.faces {
            for &idx in f {
                assert!((idx as usize) < m.vertices.len());
            }
        }
    }
}
