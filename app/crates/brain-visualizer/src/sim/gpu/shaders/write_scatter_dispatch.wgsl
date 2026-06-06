// Write indirect scatter dispatch args from the GPU-side spike count (phase 2).
//
// Runs after integrate. Computes ceil(spike_count * K / 64) into a
// DispatchIndirect buffer so the CPU never reads spike_count to size the
// scatter dispatch (architecture §5 "Avoid CPU readbacks in the rAF loop").

@group(0) @binding(0) var<storage, read> spike_count_buf: array<u32>;
@group(0) @binding(1) var<storage, read_write> dispatch_args: array<u32>;

struct ConnectUniforms {
    n: u32,
    k: u32,
    fixed_point_scale: f32,
    seed_lo: u32,
    grid_dim: u32,
}
@group(1) @binding(0) var<uniform> cu: ConnectUniforms;

@compute @workgroup_size(1)
fn main() {
    let total_events = spike_count_buf[0] * cu.k;
    let groups = (total_events + 63u) / 64u;
    dispatch_args[0] = groups;
    dispatch_args[1] = 1u;
    dispatch_args[2] = 1u;
}
