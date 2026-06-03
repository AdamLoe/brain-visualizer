// render_cylinder.wgsl — Phase 4 Near-LOD synapse cylinder render pass.
//
// Renders instanced 6-sided prism cylinders (12 triangles).
// Instance data provides src_pos, tgt_pos, weight_sign (+1 excitatory, -1 inhibitory).
//
// The vertex shader transforms a unit cylinder (radius 1, height 1, centre at origin,
// axis along +Y) to span from src_pos to tgt_pos in world space.
//
// Color: excitatory = faint blue-white (0.3,0.5,1.0), inhibitory = faint red (1.0,0.2,0.2).
// Activity intensity is reserved (weight_sign field) for future accumulation.

struct NearUniforms {
    mvp: mat4x4<f32>,
    camera_pos: vec3<f32>,
    sphere_radius: f32,
    lod_alpha: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

struct SynapseInstance {
    src_pos: vec3<f32>,
    weight_sign: f32,   // +1.0 excitatory, -1.0 inhibitory
    tgt_pos: vec3<f32>,
    activity: f32,
}

@group(0) @binding(0) var<uniform> u: NearUniforms;
@group(0) @binding(1) var<storage, read> synapse_instances: array<SynapseInstance>;

struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) alpha: f32,
}

// Cylinder radius (world units). Fixed thin line for synapses.
const CYLINDER_RADIUS: f32 = 0.003;

// Build an orthonormal basis where Y_axis aligns with the given direction.
fn build_basis(y_dir: vec3<f32>) -> mat3x3<f32> {
    // Pick a reference vector not parallel to y_dir.
    let ref_v = select(vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), abs(y_dir.z) > 0.9);
    let x_axis = normalize(cross(ref_v, y_dir));
    let z_axis = cross(y_dir, x_axis);
    return mat3x3<f32>(x_axis, y_dir, z_axis);
}

@vertex
fn vs_main(
    @location(0) local_pos: vec3<f32>,    // unit cylinder vertex (R=1, H=1, centred at 0)
    @builtin(instance_index) inst_idx: u32,
) -> VOut {
    let inst = synapse_instances[inst_idx];
    let diff = inst.tgt_pos - inst.src_pos;
    let length_v = length(diff);

    var world: vec3<f32>;
    if length_v < 0.0001 {
        // Degenerate zero-length synapse: place at src
        world = inst.src_pos;
    } else {
        let y_dir = diff / length_v;
        let basis = build_basis(y_dir);

        // local_pos: x,z = radial in [-1,1]; y = axial in [0,1] (bottom at 0, top at 1).
        // Scale: radial by CYLINDER_RADIUS, axial by length_v.
        // Translate: bottom at src_pos.
        let scaled = vec3<f32>(
            local_pos.x * CYLINDER_RADIUS,
            local_pos.y * length_v,       // axial
            local_pos.z * CYLINDER_RADIUS
        );
        world = inst.src_pos + basis * scaled;
    }

    // Color by weight sign.
    var color: vec3<f32>;
    if inst.weight_sign > 0.0 {
        color = vec3(0.3, 0.5, 1.0);   // excitatory: faint blue-white
    } else {
        color = vec3(1.0, 0.2, 0.2);   // inhibitory: faint red
    }

    var out: VOut;
    out.clip_pos = u.mvp * vec4(world, 1.0);
    out.color    = color;
    out.alpha    = u.lod_alpha * (0.25 + inst.activity * 0.5);
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4(in.color * in.alpha, in.alpha);
}
