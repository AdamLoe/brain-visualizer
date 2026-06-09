// Scatter pass (phase 2, architecture §5, phase-2 spec).
//
// One thread per (spike x synapse) event. Derives the synapse target and weight
// from the procedural BV6/BV22 rule (identical to Rust connectivity::target /
// weight) and does a fixed-point i32 atomicAdd into I_next. The BV22 hash
// (hash32/mix_key) is PREPENDED from pipelines::HASH_WGSL — do not re-author it
// here.
//
// `target_neuron` uses the phase-1 integer spatial rule (cell offset within the
// local neighbourhood + anterior bias + nearest-occupied spiral + in-cell pick),
// reading the same CSR spatial grid the Rust side builds. Modulo-N is provided
// only as a clearly labelled debug fallback (target_neuron_debug), never used by
// production dispatch.

@group(0) @binding(0) var<storage, read> spike_list: array<u32>;
@group(0) @binding(1) var<storage, read> spike_count_buf: array<u32>; // [0] = count
@group(0) @binding(2) var<storage, read_write> I_next: array<atomic<i32>>;
@group(0) @binding(3) var<storage, read> last_spike: array<u32>; // for source type
// Spatial grid (CSR), matches connectivity::spatial::SpatialGrid.
@group(0) @binding(4) var<storage, read> cell_of_neuron: array<u32>;  // packed cell id per neuron
@group(0) @binding(5) var<storage, read> cell_start: array<u32>;      // CSR offsets, len = dim^3 + 1
@group(0) @binding(6) var<storage, read> cell_neurons: array<u32>;    // neuron ids grouped by cell
// High-water instrumentation (BV19): track max |accumulated current| seen.
@group(0) @binding(7) var<storage, read_write> max_abs_current: atomic<u32>;

struct ConnectUniforms {
    n: u32,
    k: u32,
    fixed_point_scale: f32,
    seed_lo: u32,    // BV22 seed; matches SimConfig.seed_lo()
    grid_dim: u32,   // cells per axis
    long_range_frac: u32, // heavy-tailed reach: numerator over REACH_FRAC_DEN
    max_reach: u32,       // heavy-tailed reach: long-range cell radius (>=1)
}
@group(1) @binding(0) var<uniform> cu: ConnectUniforms;

// --- Constants mirrored from connectivity::mod.rs --------------------------
const LOCAL_D: i32 = 1;
const AXIS_SPAN: u32 = 3u;            // 2*LOCAL_D + 1
const ANTERIOR_BIAS_NUM: u32 = 5u;
const ANTERIOR_BIAS_DEN: u32 = 16u;
const REACH_FRAC_DEN: u32 = 256u;
// Salt constants (connectivity::salt).
const SALT_CELL_OFFSET: u32 = 0x00000001u;
const SALT_IN_CELL_PICK: u32 = 0x00000002u;
const SALT_WEIGHT: u32 = 0x00000003u;
const SALT_ANTERIOR_BIAS: u32 = 0x00000004u;
const SALT_REACH_COIN: u32 = 0x00000005u;
const SALT_REACH_OFFSET: u32 = 0x00000006u;

const TYPE_MASK: u32 = 0x7F000000u;

fn neuron_type(packed: u32) -> u32 {
    return (packed & TYPE_MASK) >> 24u;
}

// E/I flag is bit 0 of the type byte: 0 = excitatory (connectivity::is_excitatory).
fn is_excitatory(src_type: u32) -> bool {
    return (src_type & 1u) == 0u;
}

// --- Spatial grid helpers (mirror SpatialGrid pack/unpack) -----------------
fn grid_unpack(id: u32) -> vec3<u32> {
    let d = cu.grid_dim;
    let x = id % d;
    let y = (id / d) % d;
    let z = id / (d * d);
    return vec3<u32>(x, y, z);
}

fn grid_pack(c: vec3<u32>) -> u32 {
    let d = cu.grid_dim;
    return c.x + c.y * d + c.z * d * d;
}

fn clamp_cell(base: vec3<u32>, delta: vec3<i32>) -> vec3<u32> {
    let d = i32(cu.grid_dim);
    var out: vec3<i32>;
    out.x = clamp(i32(base.x) + delta.x, 0, d - 1);
    out.y = clamp(i32(base.y) + delta.y, 0, d - 1);
    out.z = clamp(i32(base.z) + delta.z, 0, d - 1);
    return vec3<u32>(u32(out.x), u32(out.y), u32(out.z));
}

fn cell_occupancy(cell_id: u32) -> u32 {
    return cell_start[cell_id + 1u] - cell_start[cell_id];
}

// Deterministic walk to the nearest occupied cell (Chebyshev shells), mirroring
// connectivity::nearest_occupied.
fn nearest_occupied(cell: vec3<u32>) -> u32 {
    let id = grid_pack(cell);
    if cell_occupancy(id) > 0u {
        return id;
    }
    let dim = i32(cu.grid_dim);
    for (var r: i32 = 1; r < dim; r = r + 1) {
        for (var dz: i32 = -r; dz <= r; dz = dz + 1) {
            for (var dy: i32 = -r; dy <= r; dy = dy + 1) {
                for (var dx: i32 = -r; dx <= r; dx = dx + 1) {
                    // Only the shell at Chebyshev radius r.
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

// Long-range offset in [-max_reach, +max_reach] (mirrors Rust long_offset_component).
fn long_offset_component(bits: u32, max_reach: u32) -> i32 {
    return i32(bits % (2u * max_reach + 1u)) - i32(max_reach);
}

// Production target rule: identical algorithm to Rust connectivity::target().
fn target_neuron(src: u32, synapse_j: u32, src_type: u32) -> u32 {
    let src_cell = grid_unpack(cell_of_neuron[src]);
    let h = mix_key(cu.seed_lo, src, synapse_j, SALT_CELL_OFFSET);

    var dx = offset_component(h & 0x3ffu);
    var dy = offset_component((h >> 10u) & 0x3ffu);
    var dz = offset_component((h >> 20u) & 0x3ffu);

    // Mild anterior (+Z) feed-forward bias for a fraction of excitatory synapses.
    if is_excitatory(src_type) {
        let bias_draw = mix_key(cu.seed_lo, src, synapse_j, SALT_ANTERIOR_BIAS) % ANTERIOR_BIAS_DEN;
        if bias_draw < ANTERIOR_BIAS_NUM {
            dz = LOCAL_D;
        }
    }

    // Heavy-tailed reach: integer coin flip; when long, OVERWRITE the local
    // (biased) offset with a wider draw bounded by max_reach. At
    // long_range_frac == 0 the compare is always false (bit-identical to local).
    let coin = mix_key(cu.seed_lo, src, synapse_j, SALT_REACH_COIN) % REACH_FRAC_DEN;
    if coin < cu.long_range_frac {
        let h2 = mix_key(cu.seed_lo, src, synapse_j, SALT_REACH_OFFSET);
        dx = long_offset_component(h2 & 0x3ffu, cu.max_reach);
        dy = long_offset_component((h2 >> 10u) & 0x3ffu, cu.max_reach);
        dz = long_offset_component((h2 >> 20u) & 0x3ffu, cu.max_reach);
    }

    let target_cell = clamp_cell(src_cell, vec3<i32>(dx, dy, dz));
    let cell_id = nearest_occupied(target_cell);

    let occ = cell_occupancy(cell_id);
    if occ == 0u {
        return src; // degenerate empty grid: self-connect (matches Rust)
    }
    let pick = mix_key(cu.seed_lo, src, synapse_j, SALT_IN_CELL_PICK) % occ;
    return cell_neurons[cell_start[cell_id] + pick];
}

// Debug-only fallback (NOT used by production dispatch). Modulo-N.
fn target_neuron_debug(src: u32, synapse_j: u32) -> u32 {
    let h = mix_key(cu.seed_lo, src, synapse_j, SALT_CELL_OFFSET);
    return h % cu.n;
}

// Fixed-point synaptic weight, identical to Rust connectivity::weight() (which
// is seed-independent: it hashes with seed_lo = 0).
fn synapse_weight(src: u32, synapse_j: u32, src_type: u32) -> i32 {
    let h = mix_key(0u, src, synapse_j, SALT_WEIGHT);
    if is_excitatory(src_type) {
        let span = u32(cu.fixed_point_scale) - 1000u; // 3096
        return 1000 + i32(h % span);
    } else {
        let span = 1000u;
        return -2000 + i32(h % span);
    }
}

@compute @workgroup_size(64)
fn scatter(@builtin(global_invocation_id) gid: vec3<u32>) {
    let event_idx = gid.x;
    let count = spike_count_buf[0];
    let total_events = count * cu.k;
    if event_idx >= total_events { return; }

    let spike_idx = event_idx / cu.k;
    let synapse_j = event_idx - spike_idx * cu.k;
    let src = spike_list[spike_idx];
    let src_type = neuron_type(last_spike[src]);
    let tgt = target_neuron(src, synapse_j, src_type);
    let w = synapse_weight(src, synapse_j, src_type);

    // Phase 2: plain atomicAdd + high-water overflow instrumentation (BV19).
    let prev = atomicAdd(&I_next[tgt], w);
    let mag = u32(abs(prev + w));
    atomicMax(&max_abs_current, mag);
}
