// Integrate + threshold pass — STUB (phase 2 fills this in).
// Phase 1 only needs the file to exist so pipeline scaffolding can reference
// it. The real LIF update (architecture §4/§5) lands in phase 2.

// Placeholder so the module parses if ever loaded.
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    // no-op stub
}
