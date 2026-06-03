// frustum_cull.wgsl — Phase 4 Near-LOD frustum culling + instance generation.
//
// Two compute entry points:
//   cull_neurons  — iterates all neurons, appends those in-frustum to neuron_instances.
//   cull_synapses — iterates all neurons, appends K_NEAR synapses per visible neuron.
//
// The BV22 hash (hash32/mix_key) is PREPENDED from pipelines::HASH_WGSL.
// target_neuron MUST be identical to scatter.wgsl's production rule.
// Do NOT re-author the hash or target rule here — only reuse.
//
// Layout matches FrustumCullUniforms in resources.rs (16-B aligned).

struct FrustumUniforms {
    // 6 frustum planes, each vec4 = (normal.xyz, d). Point P is inside if
    // for all planes: dot(plane.xyz, P) + plane.w >= -EPSILON.
    planes: array<vec4<f32>, 6>,
    camera_pos: vec3<f32>,
    max_synapse_dist: f32,   // cull synapses beyond this distance from camera
    current_tick: u32,
    n: u32,                  // total neuron count
    _pad0: u32,
    _pad1: u32,
}

struct NeuronInstance {
    pos: vec3<f32>,
    glow: f32,
    color: vec3<f32>,
    _pad: f32,
}

struct SynapseInstance {
    src_pos: vec3<f32>,
    weight_sign: f32,   // +1.0 excitatory, -1.0 inhibitory
    tgt_pos: vec3<f32>,
    activity: f32,      // 0..1 normalized (future use; 0.0 for now)
}

// --- Bindings (group 0) ---
@group(0) @binding(0) var<uniform> frustum: FrustumUniforms;
@group(0) @binding(1) var<storage, read> pos_x: array<f32>;
@group(0) @binding(2) var<storage, read> pos_y: array<f32>;
@group(0) @binding(3) var<storage, read> pos_z: array<f32>;
@group(0) @binding(4) var<storage, read> last_spike: array<u32>;
@group(0) @binding(5) var<storage, read> v: array<f32>;
@group(0) @binding(6) var<storage, read_write> neuron_instances: array<NeuronInstance>;
@group(0) @binding(7) var<storage, read_write> synapse_instances: array<SynapseInstance>;
@group(0) @binding(8) var<storage, read_write> neuron_count: atomic<u32>;
@group(0) @binding(9) var<storage, read_write> synapse_count: atomic<u32>;
// Overflow / clamped counters for profiler (unclamped append counts).
@group(0) @binding(10) var<storage, read_write> neuron_overflow: atomic<u32>;
@group(0) @binding(11) var<storage, read_write> synapse_overflow: atomic<u32>;

// Spatial grid (CSR) — same as scatter.wgsl, for target_neuron reuse.
@group(1) @binding(0) var<storage, read> cell_of_neuron: array<u32>;
@group(1) @binding(1) var<storage, read> cell_start: array<u32>;
@group(1) @binding(2) var<storage, read> cell_neurons: array<u32>;
@group(1) @binding(3) var<uniform> cu: NearConnectUniforms;

struct NearConnectUniforms {
    n: u32,
    k_near: u32,           // K_NEAR synapses per neuron (typically 8)
    seed_lo: u32,
    grid_dim: u32,
    max_near_instances: u32,
    max_synapse_instances: u32,
    _pad0: u32,
    _pad1: u32,
}

// --- Constants ---
const GLOW_TAU: f32 = 100.0;    // decay ticks, same as far-LOD default
const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TYPE_MASK: u32 = 0x7F000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;

// --- Spatial grid helpers (identical to scatter.wgsl) ---
const LOCAL_D: i32 = 1;
const AXIS_SPAN: u32 = 3u;
const ANTERIOR_BIAS_NUM: u32 = 5u;
const ANTERIOR_BIAS_DEN: u32 = 16u;
const SALT_CELL_OFFSET: u32 = 0x00000001u;
const SALT_IN_CELL_PICK: u32 = 0x00000002u;
const SALT_ANTERIOR_BIAS: u32 = 0x00000004u;

fn grid_unpack(id: u32) -> vec3<u32> {
    let d = cu.grid_dim;
    return vec3<u32>(id % d, (id / d) % d, id / (d * d));
}

fn grid_pack(c: vec3<u32>) -> u32 {
    let d = cu.grid_dim;
    return c.x + c.y * d + c.z * d * d;
}

fn clamp_cell(base: vec3<u32>, delta: vec3<i32>) -> vec3<u32> {
    let d = i32(cu.grid_dim);
    let ox = clamp(i32(base.x) + delta.x, 0, d - 1);
    let oy = clamp(i32(base.y) + delta.y, 0, d - 1);
    let oz = clamp(i32(base.z) + delta.z, 0, d - 1);
    return vec3<u32>(u32(ox), u32(oy), u32(oz));
}

fn cell_occupancy(cell_id: u32) -> u32 {
    return cell_start[cell_id + 1u] - cell_start[cell_id];
}

fn nearest_occupied(cell: vec3<u32>) -> u32 {
    let id = grid_pack(cell);
    if cell_occupancy(id) > 0u { return id; }
    let dim = i32(cu.grid_dim);
    for (var r: i32 = 1; r < dim; r++) {
        for (var dz: i32 = -r; dz <= r; dz++) {
            for (var dy: i32 = -r; dy <= r; dy++) {
                for (var dx: i32 = -r; dx <= r; dx++) {
                    if abs(dx) != r && abs(dy) != r && abs(dz) != r { continue; }
                    let c = clamp_cell(cell, vec3<i32>(dx, dy, dz));
                    let cid = grid_pack(c);
                    if cell_occupancy(cid) > 0u { return cid; }
                }
            }
        }
    }
    return id;
}

fn offset_component(h: u32) -> i32 {
    return i32(h % AXIS_SPAN) - LOCAL_D;
}

fn is_excitatory(src_type: u32) -> bool {
    return (src_type & 1u) == 0u;
}

// Identical production target rule as scatter.wgsl — uses BV22 hash prepended.
fn target_neuron(src: u32, synapse_j: u32, src_type: u32) -> u32 {
    let src_cell = grid_unpack(cell_of_neuron[src]);
    let h = mix_key(cu.seed_lo, src, synapse_j, SALT_CELL_OFFSET);

    let dx = offset_component(h & 0x3ffu);
    let dy = offset_component((h >> 10u) & 0x3ffu);
    var dz = offset_component((h >> 20u) & 0x3ffu);

    if is_excitatory(src_type) {
        let bias_draw = mix_key(cu.seed_lo, src, synapse_j, SALT_ANTERIOR_BIAS) % ANTERIOR_BIAS_DEN;
        if bias_draw < ANTERIOR_BIAS_NUM { dz = LOCAL_D; }
    }

    let target_cell = clamp_cell(src_cell, vec3<i32>(dx, dy, dz));
    let cell_id = nearest_occupied(target_cell);
    let occ = cell_occupancy(cell_id);
    if occ == 0u { return src; }
    let pick = mix_key(cu.seed_lo, src, synapse_j, SALT_IN_CELL_PICK) % occ;
    return cell_neurons[cell_start[cell_id] + pick];
}

// --- Helpers ---

// Tight frustum test for neurons (primary visibility).
fn in_frustum(pos: vec3<f32>) -> bool {
    for (var p: u32 = 0u; p < 6u; p++) {
        let plane = frustum.planes[p];
        if dot(plane.xyz, pos) + plane.w < -0.05 { return false; }
    }
    return true;
}

// Looser frustum test for synapse TARGETS — allows targets slightly outside the
// visible region so connections still appear when zoomed close into the brain.
fn in_frustum_loose(pos: vec3<f32>) -> bool {
    for (var p: u32 = 0u; p < 6u; p++) {
        let plane = frustum.planes[p];
        if dot(plane.xyz, pos) + plane.w < -0.5 { return false; }
    }
    return true;
}

fn position(i: u32) -> vec3<f32> {
    return vec3<f32>(pos_x[i], pos_y[i], pos_z[i]);
}

fn neuron_type(packed: u32) -> u32 {
    return (packed >> 24u) & 0x7Fu;
}

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}

fn tick_diff_local(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

fn region_color(ntype: u32) -> vec3<f32> {
    let region = (ntype >> 2u) & 0x3u;
    switch region {
        case 0u: { return vec3(0.2, 0.6, 1.0); }   // input: blue
        case 1u: { return vec3(0.4, 0.9, 0.4); }   // association: green
        case 2u: { return vec3(1.0, 0.5, 0.2); }   // output: orange
        default: { return vec3(0.8, 0.8, 0.8); }
    }
}

// --- cull_neurons entry point ---
@compute @workgroup_size(256)
fn cull_neurons(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= frustum.n { return; }

    let pos = position(i);
    if !in_frustum(pos) { return; }

    let packed = last_spike[i];
    let ticks_since = tick_diff_local(frustum.current_tick, packed & TICK_MASK);
    let glow = select(0.0, exp(-f32(ticks_since) / GLOW_TAU), has_spiked(packed));

    // Atomic append. Increment always; only write if within capacity.
    let idx = atomicAdd(&neuron_count, 1u);
    if idx >= cu.max_near_instances {
        // Count the overflow separately for the profiler.
        atomicAdd(&neuron_overflow, 1u);
        return;
    }
    neuron_instances[idx] = NeuronInstance(
        pos, glow, region_color(neuron_type(packed)), 0.0
    );
}

// --- cull_synapses entry point ---
@compute @workgroup_size(256)
fn cull_synapses(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= frustum.n { return; }

    let src_pos = position(i);
    if !in_frustum(src_pos) { return; }
    if distance(src_pos, frustum.camera_pos) > frustum.max_synapse_dist { return; }

    let packed = last_spike[i];
    let src_type = neuron_type(packed);

    for (var j: u32 = 0u; j < cu.k_near; j++) {
        let tgt = target_neuron(i, j, src_type);
        let tgt_pos = position(tgt);
        // Both endpoints roughly in view. Use a loose frustum for targets so
        // synapses appear even when the camera is inside the brain.
        if !in_frustum_loose(tgt_pos) { continue; }

        let w_sign = select(1.0, -1.0, !is_excitatory(src_type));
        let idx = atomicAdd(&synapse_count, 1u);
        if idx >= cu.max_synapse_instances {
            atomicAdd(&synapse_overflow, 1u);
            return;
        }
        synapse_instances[idx] = SynapseInstance(src_pos, w_sign, tgt_pos, 0.0);
    }
}
