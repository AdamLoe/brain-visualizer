// stimulate.wgsl — Cursor stimulation compute pass (Phase 3, BV10).
//
// Finds neurons within a sphere (pos, radius) and adds a fixed-point current
// bump to i_current. Dispatched once at the START of each tick when active=1;
// skipped entirely when active=0.
//
// Spatial lookup strategy: bounded brute-force over the spatial grid CSR.
// We query only cells overlapping the bounding box of the sphere, limiting
// work to a small neighborhood (radius ~0.15 world units).
// This stays proportional to the queried grid neighborhood, not full N.
// No separate per-neuron cell-id upload needed: we use the existing grid CSR.

struct StimUniforms {
    pos: vec3<f32>,
    radius: f32,
    current_fp: i32,   // already scaled to fixed-point (S=4096)
    is_active: u32,
    _pad: vec2<u32>,
}

struct GridUniforms {
    grid_min: vec3<f32>,
    cell_size: f32,
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

fn world_to_cell(p: f32, axis: u32, dim: u32) -> i32 {
    let coord = floor((p - grid_u.grid_min[axis]) / grid_u.cell_size);
    return i32(clamp(coord, 0.0, f32(dim) - 1.0));
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
    let cx0 = max(world_to_cell(stim.pos.x - r, 0u, dim), 0);
    let cy0 = max(world_to_cell(stim.pos.y - r, 1u, dim), 0);
    let cz0 = max(world_to_cell(stim.pos.z - r, 2u, dim), 0);
    let cx1 = min(world_to_cell(stim.pos.x + r, 0u, dim), i32(dim) - 1);
    let cy1 = min(world_to_cell(stim.pos.y + r, 1u, dim), i32(dim) - 1);
    let cz1 = min(world_to_cell(stim.pos.z + r, 2u, dim), i32(dim) - 1);

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
