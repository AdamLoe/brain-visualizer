// render_morphology.wgsl — V2 Beauty-First procedural neuron morphology.
//
// Instanced, fully GPU-generated camera-facing tapered tubes. One INSTANCE per
// MorphSegment; 6 verts (two triangles) per instance form a quad from endpoint
// `a` (half-width radius_a·width_scale) to `b` (half-width radius_b·width_scale),
// the quad's screen-facing side derived from the segment direction × view ray.
//
// Each axon segment carries its SOURCE neuron_id and its synaptic TARGET
// (target_id). Whole-connection lighting: when a neuron fires, its actual
// connections light up INSTANTLY and fade with the SAME exp(-tick_diff/glow_tau)
// curve the far-glow neuron dot uses — no spatial pulse travel. Two toggles:
//   light_next (downstream): a segment lights when its SOURCE neuron fires.
//   light_past (upstream, AXON ONLY): a segment lights when its TARGET fires.
// Both may be on → take the max of the two contributions. Upstream lighting is
// inter-neuron, so it applies only to axon segments (kind 1); dendrites
// (kind 0) carry target_id = self and only respond to their own neuron's spike
// via light_next. path_len is retained in the struct but no longer drives timing.
//
// kind 0 = dendrite (cool, dim), kind 1 = axon (E/I tinted). E/I comes from the
// SOURCE neuron's packed type bit (type & 1). Additive, bloom-friendly, no depth
// write.
//
// MorphSegment field order + size (48 B) MUST match `MorphSegment` in
// src/sim/morphology.rs verbatim (#1 corruption source — do not reorder).

struct MorphSegment {
    a: vec3<f32>,
    radius_a: f32,
    b: vec3<f32>,
    radius_b: f32,
    neuron_id: u32,
    path_len: f32,
    kind: u32,
    target_id: u32,
}

struct MorphUniforms {
    mvp: mat4x4<f32>,
    camera_right: vec3<f32>,
    tick: u32,
    camera_up: vec3<f32>,
    width_scale: f32,
    camera_pos: vec3<f32>,
    light_next: u32,       // Morphology controls: light downstream (outgoing) connections (0/1)
    light_past: u32,       // Morphology controls: light upstream (incoming) connections (0/1)
    glow_tau: f32,         // Morphology controls: τ for exp(-Δt/τ) fade (matches far-glow dot)
    base_brightness: f32,  // Morphology controls: resting structure brightness (morph_resting_opacity)
    connection_layer: u32, // Morphology controls: 0 = off, 1 = on (structure + signal lighting)
    color_by: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> segments: array<MorphSegment>;
@group(0) @binding(1) var<storage, read> last_spike: array<u32>;
@group(0) @binding(2) var<uniform> u: MorphUniforms;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}
fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}
fn neuron_type(packed: u32) -> u32 {
    return (packed >> 24u) & 0x7Fu;
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) cross_u: f32, // -1..1 across the tube width (round cross-section)
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) inst: u32,
) -> VertOut {
    let seg = segments[inst];
    let a = seg.a;
    let b = seg.b;
    let dir = b - a;

    // 6 verts → quad: tri A (a-, b-, a+); tri B (a+, b-, b+).
    // `along` 0 = endpoint a, 1 = endpoint b; `side` -1/+1 across width.
    var along = 0u;
    var side = -1.0;
    switch vid {
        case 0u: { along = 0u; side = -1.0; }
        case 1u: { along = 1u; side = -1.0; }
        case 2u: { along = 0u; side =  1.0; }
        case 3u: { along = 0u; side =  1.0; }
        case 4u: { along = 1u; side = -1.0; }
        default: { along = 1u; side =  1.0; }
    }

    let endpoint = select(a, b, along == 1u);
    let radius = select(seg.radius_a, seg.radius_b, along == 1u) * u.width_scale;

    // Screen-facing perpendicular: side = normalize(cross(dir, eye - midpoint)).
    let mid = (a + b) * 0.5;
    let eye = u.camera_pos - mid;
    var perp = cross(dir, eye);
    if length(perp) < 1e-9 {
        // dir parallel to view ray — fall back to camera_right.
        perp = u.camera_right;
    }
    perp = normalize(perp);
    let world = endpoint + perp * (side * radius);

    // ── Lighting / brightness ───────────────────────────────────────────────
    // COLOR is always keyed off the SOURCE neuron (E/I + region tint).
    let packed = last_spike[seg.neuron_id];
    let ty = neuron_type(packed);
    let ei = ty & 1u;            // 0 = excitatory, 1 = inhibitory
    let region = (ty >> 2u) & 0x3u;

    // Resting structure brightness (always visible).
    var brightness = u.base_brightness;

    // Morphology controls: whole-connection lighting (connection_layer >= 1).
    // A connection lights INSTANTLY when its keyed neuron fires and fades with
    // the SAME exp(-Δt/glow_tau) curve as the far-glow neuron dot:
    //   light_next (downstream): keyed off the SOURCE neuron (this neuron_id).
    //   light_past (upstream, axon only): keyed off the TARGET neuron.
    // Both may be on → take the max. Dendrites get no spike lighting.
    const BOOST: f32 = 1.8;
    if u.connection_layer >= 1u {
        var lit = 0.0;
        let src_packed = last_spike[seg.neuron_id];
        if u.light_next == 1u && has_spiked(src_packed) {
            lit = max(lit, exp(-f32(tick_diff(u.tick, src_packed & TICK_MASK)) / max(u.glow_tau, 1.0)));
        }
        if u.light_past == 1u && seg.kind == 1u {
            let tgt_packed = last_spike[seg.target_id];
            if has_spiked(tgt_packed) {
                lit = max(lit, exp(-f32(tick_diff(u.tick, tgt_packed & TICK_MASK)) / max(u.glow_tau, 1.0)));
            }
        }
        brightness = brightness + lit * BOOST;
    }

    // ── Color ───────────────────────────────────────────────────────────────
    // Dendrites: cool dim tint. Axons: E/I tinted (exc cool blue-white, inh warm
    // red). color_by overrides axon tint with region colors when requested.
    var color: vec3<f32>;
    if seg.kind == 0u {
        color = vec3<f32>(0.22, 0.34, 0.5); // cool, dim dendrite
    } else {
        if u.color_by == 0u {
            // region: Input cool-blue, Assoc green, Output warm-orange.
            if region == 0u { color = vec3<f32>(0.30, 0.55, 1.0); }
            else if region == 1u { color = vec3<f32>(0.34, 0.9, 0.5); }
            else { color = vec3<f32>(1.0, 0.55, 0.2); }
        } else {
            // E/I tint (default axon look): excitatory cool blue-white,
            // inhibitory warm red.
            color = select(vec3<f32>(0.55, 0.72, 1.0), vec3<f32>(1.0, 0.34, 0.28), ei == 1u);
        }
    }

    var out: VertOut;
    out.pos = u.mvp * vec4<f32>(world, 1.0);
    out.color = color * brightness;
    out.cross_u = side;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // Soft gaussian across the tube width → round cross-section look.
    let falloff = exp(-in.cross_u * in.cross_u * 2.2);
    let c = in.color * falloff;
    if c.r + c.g + c.b < 0.002 { discard; }
    return vec4<f32>(c, 1.0);
}
