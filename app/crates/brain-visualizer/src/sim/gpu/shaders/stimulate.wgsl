// stimulate.wgsl — Cursor stimulation compute pass (Phase 3, BV10).
//
// Finds neurons within a sphere (pos, radius) and adds a fixed-point current
// bump to i_current. Dispatched once at the START of each tick when active=1;
// skipped entirely when active=0.
//
// Spatial lookup strategy: bounded brute-force over the spatial grid CSR.
// We query only cells overlapping the bounding box of the sphere, limiting
// work to a small neighborhood (radius ~0.15 world units, cell_size ~0.125 at
// dim=16 → ~(2*radius/cell_size+1)^3 ≈ 27 cells, each with ~N/4096 neurons).
// This is O(27 * (N/4096)) ≈ O(N/150) per dispatch — cheap and correct.
// No separate per-neuron cell-id upload needed: we use the existing grid CSR.

struct StimUniforms {
    pos: vec3<f32>,
    radius: f32,
    current_fp: i32,   // already scaled to fixed-point (S=4096)
    is_active: u32,
    _pad: vec2<u32>,
}

struct GridUniforms {
    grid_dim: u32,
    n: u32,
    _pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform>          stim: StimUniforms;
@group(0) @binding(1) var<uniform>          grid_u: GridUniforms;
@group(0) @binding(2) var<storage, read>    pos_x: array<f32>;
@group(0) @binding(3) var<storage, read>    pos_y: array<f32>;
@group(0) @binding(4) var<storage, read>    pos_z: array<f32>;
@group(0) @binding(5) var<storage, read>    cell_of_neuron: array<u32>;
@group(0) @binding(6) var<storage, read>    cell_start: array<u32>;
@group(0) @binding(7) var<storage, read>    cell_neurons: array<u32>;
@group(0) @binding(8) var<storage, read_write> i_current: array<atomic<i32>>;

// World-space bounding box of the grid (must match SpatialGrid build).
// Neurons lie near the unit sphere surface (r ~ 0.7..1.3 after gyrification).
// The grid covers [-1.5, 1.5]^3 (a safe margin around the folded surface).
const GRID_MIN: f32 = -1.5;
const GRID_MAX: f32 =  1.5;

fn world_to_cell(p: f32, dim: u32) -> i32 {
    let frac = (p - GRID_MIN) / (GRID_MAX - GRID_MIN);
    return i32(clamp(frac * f32(dim), 0.0, f32(dim) - 1.0));
}

fn cell_id(cx: i32, cy: i32, cz: i32, dim: u32) -> u32 {
    let d = i32(dim);
    return u32(cx) + u32(cy) * dim + u32(cz) * dim * dim;
}

@compute @workgroup_size(1, 1, 1)
fn stimulate() {
    // Guard: skip when no hover this frame.
    if stim.is_active == 0u { return; }

    let dim = grid_u.grid_dim;
    let r   = stim.radius;

    // Bounding box in cell coordinates.
    let cx0 = max(world_to_cell(stim.pos.x - r, dim), 0);
    let cy0 = max(world_to_cell(stim.pos.y - r, dim), 0);
    let cz0 = max(world_to_cell(stim.pos.z - r, dim), 0);
    let cx1 = min(world_to_cell(stim.pos.x + r, dim), i32(dim) - 1);
    let cy1 = min(world_to_cell(stim.pos.y + r, dim), i32(dim) - 1);
    let cz1 = min(world_to_cell(stim.pos.z + r, dim), i32(dim) - 1);

    let r2 = r * r;
    let n  = grid_u.n;

    for (var iz = cz0; iz <= cz1; iz++) {
        for (var iy = cy0; iy <= cy1; iy++) {
            for (var ix = cx0; ix <= cx1; ix++) {
                let cid   = cell_id(ix, iy, iz, dim);
                let start = cell_start[cid];
                // cell_start has dim^3 + 1 entries; next-cell start is end.
                let end   = cell_start[cid + 1u];
                for (var k = start; k < end; k++) {
                    let nid = cell_neurons[k];
                    if nid >= n { continue; }
                    let dx = pos_x[nid] - stim.pos.x;
                    let dy = pos_y[nid] - stim.pos.y;
                    let dz = pos_z[nid] - stim.pos.z;
                    if dx*dx + dy*dy + dz*dz <= r2 {
                        atomicAdd(&i_current[nid], stim.current_fp);
                    }
                }
            }
        }
    }
}
