// render_ribbon.wgsl — V2 Phase D active-edge ribbon renderer.
//
// Instanced, fully GPU-generated camera-facing ribbons. instances = modulus,
// SEGMENTS=8 segments per ribbon, each segment a quad (2 tris = 6 verts), so
// SEGMENTS*6 = 48 vertices per instance, triangle-LIST. NO neuron buffers — all
// geometry comes from the EdgeEvent (src_pos/tgt_pos captured at emit time).
//
// A cubic Bézier curls each edge between source and target with a seeded
// perpendicular lift. A traveling pulse band runs p0→p3. E/I tint: cool
// blue-white for excitatory, warm red for inhibitory. Additive, bloom-friendly,
// no depth write. Degenerate (clipped) verts emitted for dead/expired slots.

struct EdgeEvent {
    src_pos: vec3<f32>,
    birth_tick: u32,
    tgt_pos: vec3<f32>,
    weight_sign: f32,
    curve_seed: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct RibbonUniforms {
    mvp: mat4x4<f32>,
    camera_right: vec3<f32>,
    tick: u32,
    camera_up: vec3<f32>,
    lifetime: f32,
    width: f32,
    curve_lift: f32,
    pulse_speed: f32,
    modulus: u32,
    connection_layer: u32, // 1 = active_only, 2 = active+recent_fade
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> edge_buffer: array<EdgeEvent>;
@group(0) @binding(1) var<uniform> u: RibbonUniforms;

const SEGMENTS: u32 = 8u;
const TICK_MASK: u32 = 0x00FFFFFFu;
const PI: f32 = 3.14159265;

fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

// Seeded unit vector perpendicular to `dir`, derived from the edge's curve_seed.
fn perp_dir(seed: u32, dir: vec3<f32>) -> vec3<f32> {
    // Random direction from the seed (three independent hash draws → [-1,1]).
    let rx = f32(hash32(seed ^ 0x11111111u) & 0xffffu) / 32767.5 - 1.0;
    let ry = f32(hash32(seed ^ 0x22222222u) & 0xffffu) / 32767.5 - 1.0;
    let rz = f32(hash32(seed ^ 0x33333333u) & 0xffffu) / 32767.5 - 1.0;
    var rnd = vec3<f32>(rx, ry, rz);
    let dl = length(dir);
    if dl < 1e-8 {
        return normalize(vec3<f32>(0.0, 1.0, 0.0) + rnd * 0.01);
    }
    let d = dir / dl;
    // Remove the component along dir → perpendicular; fall back if near-parallel.
    var p = rnd - d * dot(rnd, d);
    if length(p) < 1e-5 {
        p = cross(d, vec3<f32>(0.0, 1.0, 0.0));
        if length(p) < 1e-5 {
            p = cross(d, vec3<f32>(1.0, 0.0, 0.0));
        }
    }
    return normalize(p);
}

fn bezier(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, t: f32) -> vec3<f32> {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    return a * p0 + b * p1 + c * p2 + d * p3;
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) edge_t: f32, // 0..1 along the ribbon (for soft cross-section glow)
    @location(2) cross_u: f32, // -1..1 across the ribbon width
}

// Degenerate output: clip everything (w=0 → nothing rasterizes).
fn clipped() -> VertOut {
    var o: VertOut;
    o.pos = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    o.color = vec3<f32>(0.0);
    o.edge_t = 0.0;
    o.cross_u = 0.0;
    return o;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) inst: u32,
) -> VertOut {
    if inst >= u.modulus {
        return clipped();
    }
    let e = edge_buffer[inst];

    // Dead slot: never written (birth_tick 0 with zero positions) → clip.
    let src = e.src_pos;
    let tgt = e.tgt_pos;
    let degenerate = (length(tgt - src) < 1e-9);
    if degenerate {
        return clipped();
    }

    let age = tick_diff(u.tick, e.birth_tick);
    let life = max(u.lifetime, 1.0);
    if f32(age) > life {
        return clipped();
    }

    // Cubic Bézier control points with seeded perpendicular curl.
    let dir = tgt - src;
    let mid = (src + tgt) * 0.5;
    let nrm = perp_dir(e.curve_seed, dir);
    let lift = u.curve_lift * length(dir);
    let p1 = mix(src, mid, 0.66) + nrm * lift;
    let p2 = mix(tgt, mid, 0.66) + nrm * lift;

    // Decode this vertex: which segment + which of the 6 quad corners.
    let seg = vid / 6u;
    let corner = vid % 6u;
    // Quad corners (triangle-LIST): two triangles spanning [seg, seg+1] × [-1,+1].
    // tri A: (s,-1) (s+1,-1) (s,+1) ; tri B: (s,+1) (s+1,-1) (s+1,+1)
    var along = 0u; // 0 = seg edge, 1 = seg+1 edge
    var side = -1.0; // -1 / +1 across width
    switch corner {
        case 0u: { along = 0u; side = -1.0; }
        case 1u: { along = 1u; side = -1.0; }
        case 2u: { along = 0u; side =  1.0; }
        case 3u: { along = 0u; side =  1.0; }
        case 4u: { along = 1u; side = -1.0; }
        default: { along = 1u; side =  1.0; }
    }
    let t = f32(seg + along) / f32(SEGMENTS);

    let center = bezier(src, p1, p2, tgt, t);
    // Taper width toward both ends (sin profile → 0 at t=0 and t=1).
    let taper = sin(PI * t);
    let half_w = u.width * taper;
    // Camera-facing offset across the ribbon.
    let offset = u.camera_right * (side * half_w);
    let world = center + offset;

    // ── Brightness & pulse ───────────────────────────────────────────────────
    // active_only (1): near-constant within lifetime. active+recent_fade (2): full fade.
    let af = f32(age) / life;
    let fade = select(max(0.0, 1.0 - af * 0.3), max(0.0, 1.0 - af), u.connection_layer == 2u);

    // Traveling pulse band p0→p3.
    let pt = fract(f32(age) * u.pulse_speed);
    let pulse_boost = select(0.0, 1.2, abs(t - pt) < 0.12);

    // E/I tint: excitatory cool blue-white, inhibitory warm red.
    let tint = select(vec3<f32>(1.0, 0.3, 0.25), vec3<f32>(0.4, 0.7, 1.0), e.weight_sign > 0.0);
    let base = 0.45;
    let color = tint * (base + pulse_boost) * fade;

    var o: VertOut;
    o.pos = u.mvp * vec4<f32>(world, 1.0);
    o.color = color;
    o.edge_t = t;
    o.cross_u = side;
    return o;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // Soft cross-section falloff so the ribbon edges glow rather than hard-cut.
    let falloff = 1.0 - in.cross_u * in.cross_u; // 1 at center, 0 at edges
    let c = in.color * falloff;
    if c.r + c.g + c.b < 0.002 { discard; }
    return vec4<f32>(c, 1.0);
}
