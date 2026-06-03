// render_far.wgsl — Far-LOD billboard glow pass (Phase 3, architecture §6).
//
// Reads pos_x/pos_y/pos_z/last_spike/v storage buffers and a Uniforms struct
// (mvp, camera_right, camera_up, tick, glow_tau, point_radius, n).
//
// Glow = has_spiked ? exp(-tick_diff / glow_tau) : 0, plus a faint sub-threshold
// v glow. Region color from type bits. Additive blend. Draw(6, N).
//
// Do NOT use @builtin(point_size) — not portable in WGSL/WebGPU (architecture §6).

struct Uniforms {
    mvp: mat4x4<f32>,
    camera_right: vec3<f32>,
    _pad0: f32,
    camera_up: vec3<f32>,
    _pad1: f32,
    tick: u32,
    glow_tau: f32,      // decay constant in ticks (~100 ticks = 100ms bio)
    point_radius: f32,  // base radius in world units
    n: u32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> pos_x: array<f32>;
@group(0) @binding(2) var<storage, read> pos_y: array<f32>;
@group(0) @binding(3) var<storage, read> pos_z: array<f32>;
@group(0) @binding(4) var<storage, read> last_spike: array<u32>;
@group(0) @binding(5) var<storage, read> v: array<f32>;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}

fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) glow: f32,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

fn region_color(neuron_type: u32) -> vec3<f32> {
    // type byte layout: bits[3:2] = region (Input=0/Assoc=1/Output=2),
    // bit0 = EI flag (architecture §2 / BV21).
    let region = (neuron_type >> 2u) & 0x3u;
    switch region {
        case 0u: { return vec3(0.2, 0.6, 1.0); }   // input: cool blue
        case 1u: { return vec3(0.4, 0.9, 0.4); }   // association: green
        case 2u: { return vec3(1.0, 0.5, 0.2); }   // output: warm orange
        default: { return vec3(0.8, 0.8, 0.8); }
    }
}

@vertex
fn vs_main(
    @builtin(vertex_index) quad_vertex: u32,
    @builtin(instance_index) neuron_id: u32,
) -> VertOut {
    let packed      = last_spike[neuron_id];
    let neuron_type = (packed >> 24u) & 0x7Fu;
    let last_tick   = packed & TICK_MASK;

    let ticks_since = tick_diff(u.tick, last_tick);
    let glow = select(0.0, exp(-f32(ticks_since) / u.glow_tau), has_spiked(packed));

    // Sub-threshold voltage contributes a faint background glow.
    let v_glow = clamp(v[neuron_id] * 0.15, 0.0, 0.15);

    // Two-triangle quad (triangle-list, 6 vertices per instance).
    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2( 1.0, -1.0), vec2(-1.0,  1.0),
        vec2(-1.0,  1.0), vec2( 1.0, -1.0), vec2( 1.0,  1.0),
    );
    let corner = corners[quad_vertex];
    let radius = u.point_radius * (1.0 + glow * 2.0);
    let center = vec3<f32>(pos_x[neuron_id], pos_y[neuron_id], pos_z[neuron_id]);
    let world_pos = center
        + u.camera_right * corner.x * radius
        + u.camera_up    * corner.y * radius;

    var out: VertOut;
    out.pos   = u.mvp * vec4(world_pos, 1.0);
    out.glow  = glow + v_glow;
    out.color = region_color(neuron_type);
    out.uv    = corner;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    if d > 1.0 { discard; }
    let falloff = exp(-d * d * 3.0);
    let alpha = (in.glow * 0.9 + 0.05) * falloff;
    return vec4(in.color * in.glow * falloff, alpha);
}
