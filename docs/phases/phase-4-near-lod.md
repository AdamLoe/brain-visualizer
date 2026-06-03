# Phase 4 — Near LOD (Zoom-In Rendering)

_Zoom in close and individual neurons become spheres; synapses become visible
cylinders. All via GPU indirect rendering — no CPU readback per frame._

## Done when
- At close zoom distance (< 0.8 world units from surface), neurons render as
  low-poly spheres sized by glow intensity.
- Visible synaptic connections between nearby neurons render as thin cylinders
  (impostor or geometry) matching the procedural connectivity rule.
- Switching smoothly between far LOD (billboard glow) and near LOD (spheres)
  based on camera distance — no pop.
- Near LOD does not degrade far-LOD frame rate; near LOD is only computed when
  zoomed in.
- Frame profiler reports near-LOD instance count per frame.
- Near-LOD buffers are fixed-capacity, allocated once, cleared/reused each
  frame, and clamped safely if the visible set exceeds capacity.

## LOD transition
```
distance > 1.5 world units  → far LOD only (billboards)
0.8 < distance < 1.5        → blend / crossfade
distance < 0.8              → near LOD only (spheres + cylinders)
```
Implement as an alpha blend of the two passes during the crossfade range.

## Near LOD: GPU indirect rendering pipeline

The CPU must not iterate neurons or synapses per frame. All culling and
instance generation happens in a GPU compute shader.

Pass sequence when near LOD is active:

1. clear `neuron_count` and `synapse_count`;
2. cull/materialize visible neurons into append buffer;
3. cull/materialize visible synapses into append buffer;
4. write indirect draw args from append counters;
5. render spheres/cylinders with `draw_indexed_indirect`.

All buffers are persistent. Do not allocate instance buffers or indirect buffers
inside the frame loop.

### Frustum culling compute shader
`src/sim/gpu/shaders/frustum_cull.wgsl`

```wgsl
struct FrustumPlanes {
    planes: array<vec4<f32>, 6>,  // each vec4 = (normal.xyz, d)
    camera_pos: vec3<f32>,
    max_synapse_dist: f32,        // cull synapses beyond this world distance
    current_tick: u32,
}
@group(0) @binding(0) var<uniform> frustum: FrustumPlanes;
@group(0) @binding(1) var<storage, read> pos_x: array<f32>;
@group(0) @binding(2) var<storage, read> pos_y: array<f32>;
@group(0) @binding(3) var<storage, read> pos_z: array<f32>;
@group(0) @binding(4) var<storage, read> last_spike: array<u32>;
@group(0) @binding(5) var<storage, read> v: array<f32>;
@group(0) @binding(6) var<storage, read_write> neuron_instances: array<NeuronInstance>;
@group(0) @binding(7) var<storage, read_write> synapse_instances: array<SynapseInstance>;
@group(0) @binding(8) var<storage, read_write> neuron_count: atomic<u32>;
@group(0) @binding(9) var<storage, read_write> synapse_count: atomic<u32>;

struct NeuronInstance {
    pos: vec3<f32>,
    glow: f32,
    color: vec3<f32>,
    _pad: f32,
}

struct SynapseInstance {
    src_pos: vec3<f32>,
    tgt_pos: vec3<f32>,
    weight_sign: f32,   // +1.0 excitatory, -1.0 inhibitory
    activity: f32,      // 0..1 normalized recent spike activity
}

fn in_frustum(pos: vec3<f32>) -> bool {
    for (var p: u32 = 0u; p < 6u; p++) {
        let plane = frustum.planes[p];
        if dot(plane.xyz, pos) + plane.w < -0.05 { return false; }
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
    return (packed & 0x80000000u) != 0u;
}

fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & 0x00FFFFFFu;
}

@compute @workgroup_size(256)
fn cull_neurons(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= arrayLength(&pos_x) { return; }
    let pos = position(i);
    if !in_frustum(pos) { return; }

    let packed = last_spike[i];
    let ticks_since = tick_diff(frustum.current_tick, packed & 0x00FFFFFFu);
    let glow = select(0.0, exp(-f32(ticks_since) / GLOW_TAU), has_spiked(packed));

    let idx = atomicAdd(&neuron_count, 1u);
    if idx >= MAX_NEAR_INSTANCES { return; }
    neuron_instances[idx] = NeuronInstance(
        pos, glow, region_color(neuron_type(packed)), 0.0
    );
}

@compute @workgroup_size(256)
fn cull_synapses(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= arrayLength(&pos_x) { return; }
    let src_pos = position(i);
    if !in_frustum(src_pos) { return; }
    if distance(src_pos, frustum.camera_pos) > frustum.max_synapse_dist { return; }

    let src_type = neuron_type(last_spike[i]);

    // Materialize K synapses using the same BV22 hash rule as the sim
    for (var j: u32 = 0u; j < K_NEAR; j++) {
        let tgt = target_neuron(i, j);   // same function as scatter.wgsl
        let tgt_pos = position(tgt);
        // Skip synapses pointing outside frustum (both ends must be roughly in view)
        if !in_frustum(tgt_pos) { continue; }

        let w_sign = select(1.0, -1.0, (src_type & 1u) == 1u);
        let idx = atomicAdd(&synapse_count, 1u);
        if idx >= MAX_SYNAPSE_INSTANCES { return; }
        synapse_instances[idx] = SynapseInstance(src_pos, tgt_pos, w_sign, 0.0);
    }
}
```

Use the integer spatial grid to avoid scanning the full neuron array when close
zoom makes the visible region small enough to query cells directly. Full-array
frustum culling is acceptable for the first working near-LOD pass; the cell-query
version is the performance target once correctness is proven.

### Indirect draw buffers
After the cull dispatches, write `neuron_count` and `synapse_count` into
`DrawIndexedIndirect` buffers via a tiny shader, then:
```wgsl
// draw_indirect.wgsl — one thread
@compute @workgroup_size(1)
fn write_indirect() {
    neuron_draw_indirect.instance_count = atomicLoad(&neuron_count);
    synapse_draw_indirect.instance_count = atomicLoad(&synapse_count);
    // vertex_count = sphere_index_count / synapse_index_count
    // first_vertex, first_instance = 0
}
```
Then `render_pass.draw_indexed_indirect(neuron_buf, 0)` and
`render_pass.draw_indexed_indirect(synapse_buf, 0)`.

The indirect writer clamps instance counts to buffer capacity. It should also
write the unclamped count to a debug/profiler counter so we can see when the
near-LOD cap is too low without risking buffer overrun.

## Sphere geometry
Low-poly sphere: icosphere subdivision level 1 → 20 triangles, 12 vertices.
Sufficient for neurons at near zoom distances. Generate at startup, upload once
to a static vertex/index buffer. Instance data provides position, glow, color.

Sphere vertex shader uses `instance_pos + sphere_vertex * radius * (0.5 + glow)`.

## Synapse impostor cylinders
Two options (pick one):
1. **Geometry cylinder:** 6-sided prism, 2 ends → 12 triangles. Instance data
   provides src_pos and tgt_pos; vertex shader transforms the unit cylinder
   to span from src to tgt. Simple but more geometry.
2. **Quad impostor:** draw a quad billboard, reconstruct cylinder shape in
   fragment shader from src/tgt direction. Fewer vertices, more shader math.

Use option 1 for phase 4 (simpler). Option 2 is a future optimization.

Color: excitatory synapses = faint blue-white, inhibitory = faint red.
Activity intensity (future: accumulate per-edge spike count lazily, phase 7+).

## Limits
```
MAX_NEAR_INSTANCES    = 32_768   // neurons in near LOD
MAX_SYNAPSE_INSTANCES = 262_144  // synapses in near LOD
K_NEAR                = 8        // synapses materialized per neuron in near LOD
                                 // (subset of full K; enough to show connectivity)
```
These buffers are allocated once at startup. The atomics guarantee no overrun.

Derive final caps from adapter limits and memory budget during startup:
`MAX_NEAR_INSTANCES` and `MAX_SYNAPSE_INSTANCES` are defaults, not unconditional
truth. If the adapter cannot support the default buffers comfortably, reduce
near-LOD caps or disable near LOD for that tier/device.

## Debug and profiling

Near-LOD profiler fields:
- visible neuron candidates;
- emitted neuron instances;
- visible synapse candidates;
- emitted synapse instances;
- clamped/overflow counts;
- cull ms and render ms when timestamp queries are available.

Debug overlays for frustum planes, cell bounds, and emitted-instance caps are
optional and default off. They must not force near-LOD computation when the
camera is in far-LOD mode.

## What is still stubbed
- Speed controls UI — phase 5.
- Brain states button group — phase 5.
- Backend toggle UI — phase 5.
