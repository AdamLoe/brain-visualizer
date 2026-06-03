// Scatter pass — STUB (phase 2 fills this in).
// The real version (architecture §5) includes hash.wgsl, evaluates mix_key to
// derive targets, and does fixed-point atomicAdd into I_next. Phase 1 only
// needs the file to exist.

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    // no-op stub
}
