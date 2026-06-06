// emit_edges.wgsl — V2 Phase D active-edge event emission.
//
// One thread per FIRING NEURON (sized by spike_count). Each firing neuron emits
// ONE EdgeEvent for a sampled synapse into a persistent, monotonically-indexed
// ring buffer (edge_buffer). The active modulus (= min(max_active_visual_edges,
// EDGE_CAP)) keeps only the most-recent `modulus` edges alive — the documented
// "overwrite ring-style" culling. No silent caps: edge_emitted counts every emit
// this frame and is read back via the GpuBackend metrics path.
//
// HASH_WGSL (mix_key/hash32) is PREPENDED at module build (pipelines.rs), as for
// scatter. The grid + target derivation below MIRRORS scatter.wgsl exactly so a
// visual edge lands on the same target neuron the scatter pass drove current to.

@group(0) @binding(0) var<storage, read> spike_list: array<u32>;
@group(0) @binding(1) var<storage, read> spike_count_buf: array<u32>; // [0] = count
@group(0) @binding(2) var<storage, read> last_spike: array<u32>;      // src type/weight_sign
// Spatial grid (CSR), matches connectivity::spatial::SpatialGrid.
@group(0) @binding(3) var<storage, read> cell_of_neuron: array<u32>;
@group(0) @binding(4) var<storage, read> cell_start: array<u32>;
@group(0) @binding(5) var<storage, read> cell_neurons: array<u32>;
// Neuron positions (SoA) — captured into the EdgeEvent so render needs no neuron bufs.
@group(0) @binding(6) var<storage, read> pos_x: array<f32>;
@group(0) @binding(7) var<storage, read> pos_y: array<f32>;
@group(0) @binding(8) var<storage, read> pos_z: array<f32>;
// Edge ring buffer + monotonic write index + per-frame emit counter.
@group(0) @binding(9)  var<storage, read_write> edge_buffer: array<EdgeEvent>;
@group(0) @binding(10) var<storage, read_write> edge_write_index: atomic<u32>;
@group(0) @binding(11) var<storage, read_write> edge_emitted: atomic<u32>;

// EdgeEvent — 48 bytes, std430. MUST match the Rust `EdgeEvent` (resources.rs):
//   src_pos: vec3<f32>, birth_tick: u32,   (16 B)
//   tgt_pos: vec3<f32>, weight_sign: f32,  (16 B)
//   curve_seed, _pad0, _pad1, _pad2: u32   (16 B)
struct EdgeEvent {
    src_pos: vec3<f32>,
    birth_tick: u32,
    tgt_pos: vec3<f32>,
    weight_sign: f32,
    curve_seed: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

// Edge emit uniform (group 1). Self-contained: does not reuse ConnectUniforms so
// it can also carry the ring modulus + sample stride.
struct EdgeUniforms {
    tick: u32,
    n: u32,
    k: u32,
    seed_lo: u32,
    grid_dim: u32,
    modulus: u32,       // active ring size = min(max_active_visual_edges, EDGE_CAP)
    sample_stride: u32, // emit only when (spike_idx % stride)==0 (1 = every firing neuron)
    _pad: u32,
}
@group(1) @binding(0) var<uniform> eu: EdgeUniforms;

// ── MIRRORS scatter.wgsl — keep in sync (BV22 locked) ─────────────────────────
const LOCAL_D: i32 = 1;
const AXIS_SPAN: u32 = 3u;
const ANTERIOR_BIAS_NUM: u32 = 5u;
const ANTERIOR_BIAS_DEN: u32 = 16u;
const SALT_CELL_OFFSET: u32 = 0x00000001u;
const SALT_IN_CELL_PICK: u32 = 0x00000002u;
const SALT_ANTERIOR_BIAS: u32 = 0x00000004u;
// Edge-only salts (do not collide with scatter's salt space; only used here).
const SALT_EDGE_SYNAPSE: u32 = 0x00000101u;
const SALT_EDGE_CURVE: u32 = 0x00000102u;

const TYPE_MASK: u32 = 0x7F000000u;

fn neuron_type(packed: u32) -> u32 {
    return (packed & TYPE_MASK) >> 24u;
}

// MIRRORS scatter.wgsl is_excitatory.
fn is_excitatory(src_type: u32) -> bool {
    return (src_type & 1u) == 0u;
}

fn grid_unpack(id: u32) -> vec3<u32> {
    let d = eu.grid_dim;
    let x = id % d;
    let y = (id / d) % d;
    let z = id / (d * d);
    return vec3<u32>(x, y, z);
}

fn grid_pack(c: vec3<u32>) -> u32 {
    let d = eu.grid_dim;
    return c.x + c.y * d + c.z * d * d;
}

fn clamp_cell(base: vec3<u32>, delta: vec3<i32>) -> vec3<u32> {
    let d = i32(eu.grid_dim);
    var out: vec3<i32>;
    out.x = clamp(i32(base.x) + delta.x, 0, d - 1);
    out.y = clamp(i32(base.y) + delta.y, 0, d - 1);
    out.z = clamp(i32(base.z) + delta.z, 0, d - 1);
    return vec3<u32>(u32(out.x), u32(out.y), u32(out.z));
}

fn cell_occupancy(cell_id: u32) -> u32 {
    return cell_start[cell_id + 1u] - cell_start[cell_id];
}

fn nearest_occupied(cell: vec3<u32>) -> u32 {
    let id = grid_pack(cell);
    if cell_occupancy(id) > 0u {
        return id;
    }
    let dim = i32(eu.grid_dim);
    for (var r: i32 = 1; r < dim; r = r + 1) {
        for (var dz: i32 = -r; dz <= r; dz = dz + 1) {
            for (var dy: i32 = -r; dy <= r; dy = dy + 1) {
                for (var dx: i32 = -r; dx <= r; dx = dx + 1) {
                    if abs(dx) != r && abs(dy) != r && abs(dz) != r {
                        continue;
                    }
                    let c = clamp_cell(cell, vec3<i32>(dx, dy, dz));
                    let cid = grid_pack(c);
                    if cell_occupancy(cid) > 0u {
                        return cid;
                    }
                }
            }
        }
    }
    return id;
}

fn offset_component(h: u32) -> i32 {
    return i32(h % AXIS_SPAN) - LOCAL_D;
}

// MIRRORS scatter.wgsl target_neuron — identical algorithm.
fn target_neuron(src: u32, synapse_j: u32, src_type: u32) -> u32 {
    let src_cell = grid_unpack(cell_of_neuron[src]);
    let h = mix_key(eu.seed_lo, src, synapse_j, SALT_CELL_OFFSET);

    let dx = offset_component(h & 0x3ffu);
    let dy = offset_component((h >> 10u) & 0x3ffu);
    var dz = offset_component((h >> 20u) & 0x3ffu);

    if is_excitatory(src_type) {
        let bias_draw = mix_key(eu.seed_lo, src, synapse_j, SALT_ANTERIOR_BIAS) % ANTERIOR_BIAS_DEN;
        if bias_draw < ANTERIOR_BIAS_NUM {
            dz = LOCAL_D;
        }
    }

    let target_cell = clamp_cell(src_cell, vec3<i32>(dx, dy, dz));
    let cell_id = nearest_occupied(target_cell);

    let occ = cell_occupancy(cell_id);
    if occ == 0u {
        return src;
    }
    let pick = mix_key(eu.seed_lo, src, synapse_j, SALT_IN_CELL_PICK) % occ;
    return cell_neurons[cell_start[cell_id] + pick];
}

fn pos_of(id: u32) -> vec3<f32> {
    return vec3<f32>(pos_x[id], pos_y[id], pos_z[id]);
}

@compute @workgroup_size(64)
fn emit_edges(@builtin(global_invocation_id) gid: vec3<u32>) {
    let spike_idx = gid.x;
    let count = spike_count_buf[0];
    if spike_idx >= count { return; }
    if eu.modulus == 0u { return; }

    // Optional sampling gate when spikes >> modulus: explicit, surfaced drops.
    let stride = max(eu.sample_stride, 1u);
    if (spike_idx % stride) != 0u { return; }

    let src = spike_list[spike_idx];
    if src >= eu.n { return; }
    let src_type = neuron_type(last_spike[src]);

    // Sample one synapse for this firing neuron.
    let synapse_j = mix_key(eu.seed_lo, src, eu.tick, SALT_EDGE_SYNAPSE) % max(eu.k, 1u);
    let tgt = target_neuron(src, synapse_j, src_type);

    let weight_sign = select(-1.0, 1.0, is_excitatory(src_type));
    let curve_seed = mix_key(eu.seed_lo, src, synapse_j, SALT_EDGE_CURVE);

    // Monotonic write index; ring modulus keeps the most-recent `modulus` edges.
    let slot = atomicAdd(&edge_write_index, 1u) % eu.modulus;

    var e: EdgeEvent;
    e.src_pos = pos_of(src);
    e.birth_tick = eu.tick;
    e.tgt_pos = pos_of(tgt);
    e.weight_sign = weight_sign;
    e.curve_seed = curve_seed;
    e._pad0 = 0u;
    e._pad1 = 0u;
    e._pad2 = 0u;
    edge_buffer[slot] = e;

    atomicAdd(&edge_emitted, 1u);
}
