// draw_indirect.wgsl — Phase 4 indirect draw argument writer.
//
// Single-thread compute shader that copies the atomically-accumulated
// neuron/synapse counts into DrawIndexedIndirect argument buffers, clamped
// to buffer capacity to guarantee no overrun.  Writes unclamped counts to
// profiler counters.
//
// DrawIndexedIndirectArgs layout (wgpu / Vulkan / WebGPU):
//   [0] index_count     u32
//   [1] instance_count  u32
//   [2] first_index     u32
//   [3] base_vertex     i32
//   [4] first_instance  u32
// Total: 5 x u32 = 20 bytes.
//
// group(0) bindings match NearLodIndirectLayouts in resources.rs.

// The append counters (written by frustum_cull.wgsl).
@group(0) @binding(0) var<storage, read_write> neuron_count: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> synapse_count: atomic<u32>;

// DrawIndexedIndirect argument buffers — 5 x u32 each.
@group(0) @binding(2) var<storage, read_write> neuron_draw_args: array<u32>;
@group(0) @binding(3) var<storage, read_write> synapse_draw_args: array<u32>;

// Profiler counters: unclamped (total appended) counts.
@group(0) @binding(4) var<storage, read_write> neuron_visible_count: atomic<u32>;
@group(0) @binding(5) var<storage, read_write> synapse_visible_count: atomic<u32>;

struct IndirectUniforms {
    sphere_index_count: u32,
    cylinder_index_count: u32,
    max_near_instances: u32,
    max_synapse_instances: u32,
}
@group(0) @binding(6) var<uniform> params: IndirectUniforms;

@compute @workgroup_size(1)
fn write_indirect() {
    let raw_n = atomicLoad(&neuron_count);
    let raw_s = atomicLoad(&synapse_count);

    // Write unclamped counts to profiler slots.
    atomicStore(&neuron_visible_count, raw_n);
    atomicStore(&synapse_visible_count, raw_s);

    // Clamped instance counts — guarantee no buffer overrun.
    let clamped_n = min(raw_n, params.max_near_instances);
    let clamped_s = min(raw_s, params.max_synapse_instances);

    // Neuron sphere draw: index_count=sphere_index_count, instance_count=clamped_n.
    neuron_draw_args[0] = params.sphere_index_count;
    neuron_draw_args[1] = clamped_n;
    neuron_draw_args[2] = 0u;   // first_index
    neuron_draw_args[3] = 0u;   // base_vertex (u32 storage; sign-extend is irrelevant)
    neuron_draw_args[4] = 0u;   // first_instance

    // Synapse cylinder draw.
    synapse_draw_args[0] = params.cylinder_index_count;
    synapse_draw_args[1] = clamped_s;
    synapse_draw_args[2] = 0u;
    synapse_draw_args[3] = 0u;
    synapse_draw_args[4] = 0u;
}
