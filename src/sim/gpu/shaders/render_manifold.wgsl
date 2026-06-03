// render_manifold.wgsl — Static dark mesh pass for the folded brain surface
// (Phase 3, architecture §6 / phase-3-gpu-render.md "Manifold surface
// visualization"). Rendered BEFORE the glow pass so the brain shape reads
// through neuron glows.
//
// No lighting model — flat dark fill is enough.  The neuron glow provides
// all the illumination (spec: vec3(0.05, 0.05, 0.08), depth test enabled).

struct Uniforms {
    mvp: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertOut {
    @builtin(position) pos: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
) -> VertOut {
    var out: VertOut;
    out.pos = u.mvp * vec4(position, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return vec4(0.05, 0.05, 0.08, 1.0);
}
