// bloom.wgsl — V2 Phase E post-process bloom (OPT-IN; only run when
// bloom_strength > 0). Pipeline:
//   scene (rgba16float HDR) → bright_pass (threshold) → blur_h → blur_v
//   (half-res ping-pong) → composite (scene + blur*strength, tonemap) → surface.
//
// All passes are a single fullscreen triangle (vs_fullscreen). Additive glow
// already comes from the scene passes; bloom adds the soft blurred halo.

struct BloomUniforms {
    // texel size of the INPUT texture (1/w, 1/h) for blur tap spacing.
    inv_texel: vec2<f32>,
    // blur direction: (1,0) horizontal, (0,1) vertical. Unused by bright/composite.
    direction: vec2<f32>,
    // bright-pass luminance threshold (knee).
    threshold: f32,
    // composite bloom intensity (= bloom_strength).
    bloom_strength: f32,
    // composite exposure (tonemap), hardcoded BACKGROUND_EXPOSURE from the CPU.
    exposure: f32,
    _pad: f32,
}

@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex_a: texture_2d<f32>;      // bright/blur input; composite scene
@group(0) @binding(2) var<uniform> u: BloomUniforms;
// Composite-only: the blurred bloom texture (bound for the composite pipeline,
// which uses a layout that includes this binding).
@group(0) @binding(3) var tex_b: texture_2d<f32>;      // composite: blurred bloom

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// Fullscreen triangle (3 verts cover the screen). No vertex buffer.
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VOut {
    var out: VOut;
    // (0,0),(2,0),(0,2) in UV → clip covers [-1,1].
    let uv = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    out.uv = uv;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    // Flip Y so UV origin is top-left (texture sample space).
    out.pos.y = -out.pos.y;
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Bright-pass: keep only the part of the scene above the threshold knee.
@fragment
fn fs_bright(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex_a, samp, in.uv).rgb;
    let l = luma(c);
    let keep = max(l - u.threshold, 0.0);
    // Scale the color by how far over the knee it is (soft).
    let scale = select(0.0, keep / max(l, 1e-4), l > u.threshold);
    return vec4<f32>(c * scale, 1.0);
}

// Separable 9-tap Gaussian (direction from the uniform).
@fragment
fn fs_blur(in: VOut) -> @location(0) vec4<f32> {
    let step = u.inv_texel * u.direction;
    // Normalized Gaussian weights (sigma ~2).
    let w0 = 0.227027;
    let w1 = 0.1945946;
    let w2 = 0.1216216;
    let w3 = 0.054054;
    let w4 = 0.016216;
    var acc = textureSample(tex_a, samp, in.uv).rgb * w0;
    acc += textureSample(tex_a, samp, in.uv + step * 1.0).rgb * w1;
    acc += textureSample(tex_a, samp, in.uv - step * 1.0).rgb * w1;
    acc += textureSample(tex_a, samp, in.uv + step * 2.0).rgb * w2;
    acc += textureSample(tex_a, samp, in.uv - step * 2.0).rgb * w2;
    acc += textureSample(tex_a, samp, in.uv + step * 3.0).rgb * w3;
    acc += textureSample(tex_a, samp, in.uv - step * 3.0).rgb * w3;
    acc += textureSample(tex_a, samp, in.uv + step * 4.0).rgb * w4;
    acc += textureSample(tex_a, samp, in.uv - step * 4.0).rgb * w4;
    return vec4<f32>(acc, 1.0);
}

// Composite: scene (tex_a) preserved, plus the blurred bloom halo (tex_b) added
// with a soft rolloff so the additive glow can't blow past 1.0 (preserves the
// Part-1 look in dark areas; only bright tendrils gain a halo). `exposure`
// scales the scene gently; default 1.0 ≈ pass-through.
@fragment
fn fs_composite(in: VOut) -> @location(0) vec4<f32> {
    let scene = textureSample(tex_a, samp, in.uv).rgb * u.exposure;
    let bloom = textureSample(tex_b, samp, in.uv).rgb * u.bloom_strength;
    // Soft-add: 1 - exp(-x) rolls the bloom contribution off smoothly toward 1.0
    // without darkening the base scene the way a global Reinhard tonemap does.
    let halo = vec3<f32>(1.0) - exp(-bloom);
    let result = clamp(scene + halo, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(result, 1.0);
}
