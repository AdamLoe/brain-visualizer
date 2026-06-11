// render_manifold.wgsl — Static dark mesh pass for the folded brain surface
// (Phase 3, architecture §6 / phase-3-gpu-render.md "Manifold surface
// visualization"). Rendered BEFORE the glow pass so the brain shape reads
// through neuron glows.
//
// No lighting model — flat dark fill is enough.  The neuron glow provides
// all the illumination.
//
// V2 Phase E: re-enabled as an OPTIONAL dim/translucent context surface gated by
// the `surface` setting (0=off ⇒ pass skipped on the CPU side; 1=dim; 2=normal).
// `surface_opacity`, `surface_mode`, and `color_by` modulate the fill.

struct Uniforms {
    mvp: mat4x4<f32>,
    // V2 Phase E (offset 64): surface controls.
    surface_opacity: f32,
    surface_mode: u32, // 1=dim, 2=normal
    color_by: u32,
    _pad1: u32,
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
    // Base dark surface tint. mode 1 (dim) = half brightness, mode 2 (normal) = full.
    let brain_rest_pink = vec3<f32>(1.0, 0.18, 0.54);
    let base = select(vec3<f32>(0.09, 0.09, 0.14), brain_rest_pink, u.color_by == 6u);
    let mode_scale = select(0.5, 1.0, u.surface_mode == 2u);
    let rgb = base * mode_scale * u.surface_opacity;
    return vec4(rgb, u.surface_opacity);
}
