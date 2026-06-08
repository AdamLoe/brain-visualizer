// render_morphology.wgsl — V2 Beauty-First procedural neuron morphology + soma spheres.
//
// Instanced, fully GPU-generated tube geometry. One INSTANCE per MorphSegment;
// TUBE_SIDES * 2 * 3 verts per instance form a tapered cylinder (tube) from
// endpoint `a` (radius radius_a · width_scale) to endpoint `b` (radius
// radius_b · width_scale). Two rings of TUBE_SIDES vertices each, triangulated
// as quads (2 tris per side), with a stable perpendicular basis built from the
// segment axis.
//
// Lighting model (Stage 0 / v0.3.0): one directional light + rim term.
//   ambient + diffuse·max(dot(N,L),0) + rim·pow(1-max(dot(N,V),0), rim_power)
// The lighting multiplier modulates the existing glow/brightness so the additive
// glow aesthetic is preserved (lit-active branches shine through).
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

// ── Tube geometry constant ────────────────────────────────────────────────────
// v0.3.1: pipeline-overridable constant. The Rust side (gpu/mod.rs) sets this via
// `compilation_options.constants` AND computes the matching draw vert-count
// (TUBE_SIDES * 2 * 3) from the same runtime value, so the two sites stay in sync.
// Default 6 matches the inherited v0.3.0 value.
override TUBE_SIDES: u32 = 6u;

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

// MorphUniforms layout — MUST match Rust MorphUniforms in resources.rs exactly.
// 192 B total: mat4=64 + 8×16-B blocks.
//
// Byte offsets:
//   0:   mvp           mat4x4<f32>    (64 B)
//  64:   camera_right  vec3<f32>      (12 B) | tick  u32       (4 B)
//  80:   camera_up     vec3<f32>      (12 B) | width_scale f32 (4 B)
//  96:   camera_pos    vec3<f32>      (12 B) | light_next  u32 (4 B)
// 112:   light_past u32 | glow_tau f32 | base_brightness f32 | connection_layer u32
// 128:   color_by u32 | _pad_a u32 | _pad_b u32 | _pad_c u32
// 144:   light_dir     vec3<f32>      (12 B) | ambient         f32 (4 B)
// 160:   diffuse_intensity f32 | rim_intensity f32 | rim_power f32 | _pad3 u32
// 176:   resting_brightness f32 | active_boost f32 | _pad4 u32 | _pad5 u32
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
    _pad_a: u32,
    _pad_b: u32,
    _pad_c: u32,
    // Stage 0 lighting fields (v0.3.0 defaults; dev-panel exposure in v0.3.1)
    light_dir: vec3<f32>,
    ambient: f32,
    diffuse_intensity: f32,
    rim_intensity: f32,
    rim_power: f32,
    _pad3: u32,
    // v0.3.1 active/resting brightness split (morph-config owned).
    resting_brightness: f32, // resting structure brightness (config source)
    active_boost: f32,       // multiplier on the lit/spiking contribution (was const BOOST)
    _pad4: u32,
    _pad5: u32,
}

// ── Tube pass bindings (group 0, bindings 0/1/2) ──────────────────────────────
@group(0) @binding(0) var<storage, read> segments: array<MorphSegment>;
@group(0) @binding(1) var<storage, read> last_spike: array<u32>;
@group(0) @binding(2) var<uniform> u: MorphUniforms;

// ── Soma sphere pass bindings (group 0, bindings 3/4/5) ──────────────────────
// The sphere pipeline uses its OWN bind group layout (render_soma_spheres_bgl)
// with entries at binding slots 3/4/5 so both entry-point sets can live in the
// same WGSL module without a slot collision. WebGPU validates only reachable
// bindings per entry point, so the tube pipeline ignores 3/4/5 and vice versa.
//
//   binding 3: sphere_instances (STORAGE, read)  ← MorphSphereInstance array
//   binding 4: sphere_last_spike (STORAGE, read) ← same last_spike buffer
//   binding 5: su (UNIFORM)                      ← same morph_uniform buffer (176 B)

/// Per-soma sphere instance. 32 B, 16-aligned. Field order MUST match
/// `MorphSphereInstance` in src/sim/morphology.rs.
struct SphereInstance {
    center: vec3<f32>,
    radius: f32,
    neuron_id: u32,
    kind: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(3) var<storage, read> sphere_instances: array<SphereInstance>;
@group(0) @binding(4) var<storage, read> sphere_last_spike: array<u32>;
@group(0) @binding(5) var<uniform> su: MorphUniforms;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;
const IDENTITY_SALT: u32 = 0x9f3ab7c2u;
const IDENTITY_MORPH_BLEND: f32 = 0.62;

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}
fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}
fn neuron_type(packed: u32) -> u32 {
    return (packed >> 24u) & 0x7Fu;
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let hp = fract(h) * 6.0;
    let hp_mod2 = hp - floor(hp * 0.5) * 2.0;
    let x = c * (1.0 - abs(hp_mod2 - 1.0));
    var rgb: vec3<f32>;
    if hp < 1.0 {
        rgb = vec3<f32>(c, x, 0.0);
    } else if hp < 2.0 {
        rgb = vec3<f32>(x, c, 0.0);
    } else if hp < 3.0 {
        rgb = vec3<f32>(0.0, c, x);
    } else if hp < 4.0 {
        rgb = vec3<f32>(0.0, x, c);
    } else if hp < 5.0 {
        rgb = vec3<f32>(x, 0.0, c);
    } else {
        rgb = vec3<f32>(c, 0.0, x);
    }
    return rgb + vec3<f32>(l - 0.5 * c);
}

fn identity_color(id: u32) -> vec3<f32> {
    let h = f32(mix_key(0u, id, 0u, IDENTITY_SALT)) / 4294967295.0;
    return hsl_to_rgb(h, 0.75, 0.60);
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) normal: vec3<f32>, // world-space tube normal (ring radial direction)
    @location(2) view_dir: vec3<f32>, // world-space unit vector toward camera
}

// ── Tube vertex generation ────────────────────────────────────────────────────
//
// Vertices per instance: TUBE_SIDES * 2 * 3
// Layout: for each of TUBE_SIDES quads, emit 2 triangles (6 verts).
// Quad s connects ring-side s and ring-side (s+1) % TUBE_SIDES across the two
// rings (ring 0 at endpoint a/radius_a, ring 1 at endpoint b/radius_b).
//
// Triangle 0: (ring0,s), (ring0,s+1), (ring1,s)
// Triangle 1: (ring1,s), (ring0,s+1), (ring1,s+1)
//
// Within each block of 6 verts (vid 0..5 for side s):
//   vid 0 → (ring0, s)
//   vid 1 → (ring0, s+1)
//   vid 2 → (ring1, s)
//   vid 3 → (ring1, s)
//   vid 4 → (ring0, s+1)
//   vid 5 → (ring1, s+1)

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) inst: u32,
) -> VertOut {
    let seg = segments[inst];
    let a = seg.a;
    let b = seg.b;

    // ── Build tube basis ─────────────────────────────────────────────────────
    let axis = b - a;
    var tube_axis = normalize(axis);
    // Guard near-zero-length segments: output degenerate (at-a) rather than NaN.
    if length(axis) < 1e-9 {
        tube_axis = vec3<f32>(0.0, 1.0, 0.0);
    }

    // Stable perpendicular: avoid cross product with a parallel fallback.
    var fallback: vec3<f32>;
    if abs(tube_axis.y) < 0.9 {
        fallback = vec3<f32>(0.0, 1.0, 0.0);
    } else {
        fallback = vec3<f32>(1.0, 0.0, 0.0);
    }
    let u_vec = normalize(cross(tube_axis, fallback)); // first radial basis
    let v_vec = cross(tube_axis, u_vec);               // second radial basis (already unit)

    // ── Decode vertex within the quad strip ─────────────────────────────────
    // 6 verts per side; which side (s) and local index within that side.
    let side = vid / 6u;
    let local = vid % 6u;

    // The two column indices in the ring for this triangle pair.
    let s0 = side;
    let s1 = (side + 1u) % TUBE_SIDES;

    // Map local vertex to (ring, column) — ring 0 = endpoint a, ring 1 = endpoint b.
    // Triangle 0 (local 0,1,2): (ring0,s0), (ring0,s1), (ring1,s0)
    // Triangle 1 (local 3,4,5): (ring1,s0), (ring0,s1), (ring1,s1)
    var ring: u32;
    var col: u32;
    switch local {
        case 0u: { ring = 0u; col = s0; }
        case 1u: { ring = 0u; col = s1; }
        case 2u: { ring = 1u; col = s0; }
        case 3u: { ring = 1u; col = s0; }
        case 4u: { ring = 0u; col = s1; }
        default: { ring = 1u; col = s1; }
    }

    // ── Ring position ────────────────────────────────────────────────────────
    let theta = 6.283185307 * f32(col) / f32(TUBE_SIDES); // 2π * col / TUBE_SIDES
    let cos_t = cos(theta);
    let sin_t = sin(theta);
    // Radial direction in world space — also the outward normal.
    let radial = cos_t * u_vec + sin_t * v_vec;

    let endpoint = select(a, b, ring == 1u);
    let radius = select(seg.radius_a, seg.radius_b, ring == 1u) * u.width_scale;
    let world = endpoint + radial * radius;

    // ── Normal and view direction for lighting ────────────────────────────────
    let N = radial; // unit outward normal = radial direction
    let world_to_cam = u.camera_pos - world;
    let V = normalize(world_to_cam);

    // ── Spike brightness / glow calculation (unchanged from billboard pass) ──
    let packed = last_spike[seg.neuron_id];
    let ty = neuron_type(packed);
    let ei = ty & 1u;            // 0 = excitatory, 1 = inhibitory
    let region = (ty >> 2u) & 0x3u;

    // Resting structure brightness (always visible). Morph-config owned (v0.3.1).
    var brightness = u.resting_brightness;

    // Morphology controls: whole-connection lighting (connection_layer >= 1).
    let BOOST: f32 = u.active_boost;
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

    // ── Color ────────────────────────────────────────────────────────────────
    // Dendrites: cool dim tint. Axons: E/I tinted (exc cool blue-white, inh warm
    // red). color_by overrides axon tint with region colors when requested, or
    // blends per-neuron identity color into the structural tint.
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
    if u.color_by == 5u {
        color = mix(color, identity_color(seg.neuron_id), IDENTITY_MORPH_BLEND);
    }

    var out: VertOut;
    out.pos = u.mvp * vec4<f32>(world, 1.0);
    out.color = color * brightness;
    out.normal = N;
    out.view_dir = V;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // ── Lighting model ────────────────────────────────────────────────────────
    // Simple ambient + half-Lambert diffuse + Fresnel-approximation rim.
    // Modulates the glow brightness so active/lit tubes punch through.
    let N = normalize(in.normal);
    let V = normalize(in.view_dir);
    let L = normalize(u.light_dir); // pre-normalised in CPU but normalize again for safety

    let lambert = max(dot(N, L), 0.0);
    let nv = max(dot(N, V), 0.0);
    let rim = pow(1.0 - nv, u.rim_power) * u.rim_intensity;
    let lighting = u.ambient + u.diffuse_intensity * lambert + rim;

    let c = in.color * lighting;
    if c.r + c.g + c.b < 0.002 { discard; }
    return vec4<f32>(c, 1.0);
}

// ════════════════════════════════════════════════════════════════════════════
// Stream 2 — Soma sphere pass (vs_sphere / fs_sphere)
// ════════════════════════════════════════════════════════════════════════════
//
// Shader-generated UV sphere. One INSTANCE per soma (MorphSphereInstance).
// SPHERE_VERTS vertices per instance: 8 slices × 6 stacks.
//
// Layout: top cap (SPHERE_SLICES triangles) +
//         body quads ((SPHERE_STACKS-1) × SPHERE_SLICES × 2 tris) +
//         bottom cap (SPHERE_SLICES triangles).
// Total tris = SPHERE_SLICES*(2 + (SPHERE_STACKS-1)*2) = SPHERE_SLICES*SPHERE_STACKS*2
// Total verts = SPHERE_SLICES*SPHERE_STACKS*2 * 3
//             = 8 * 6 * 2 * 3 = 288
//
// MUST match Rust const SPHERE_VERTS in gpu/mod.rs.

// ── Sphere geometry constants ─────────────────────────────────────────────────
// v0.3.1: pipeline-overridable constants (set via compilation_options.constants).
// SPHERE_VERTS = SPHERE_SLICES * SPHERE_STACKS * 2 * 3; the Rust side computes the
// matching draw vert-count from the same runtime values.
override SPHERE_SLICES: u32 = 8u;
override SPHERE_STACKS: u32 = 6u;

// ── Helper: decode vertex_index into a sphere surface point ──────────────────
//
// Triangulation scheme:
//   [0, SPHERE_SLICES*3) → top cap: SPHERE_SLICES triangles (pole → stack 0)
//   [SPHERE_SLICES*3, SPHERE_SLICES*3 + body_verts) → body quads
//   last SPHERE_SLICES*3 → bottom cap: SPHERE_SLICES triangles
//
// Stack ring latitude: theta = π * stack / SPHERE_STACKS (0=top, π=bottom)
// Slice longitude:     phi   = 2π * slice / SPHERE_SLICES

fn sphere_point(stack_i: u32, slice_i: u32) -> vec3<f32> {
    let pi = 3.14159265358979;
    let theta = pi * f32(stack_i) / f32(SPHERE_STACKS);
    let phi   = 2.0 * pi * f32(slice_i) / f32(SPHERE_SLICES);
    let sin_t = sin(theta);
    let cos_t = cos(theta);
    let sin_p = sin(phi);
    let cos_p = cos(phi);
    return vec3<f32>(sin_t * cos_p, cos_t, sin_t * sin_p);
}

// Decode vertex_index into a world-space direction on the unit sphere.
// Returns the unit radial direction (also the outward normal).
fn decode_sphere_vertex(vid: u32) -> vec3<f32> {
    let top_cap_verts = SPHERE_SLICES * 3u;
    let body_quad_verts = (SPHERE_STACKS - 1u) * SPHERE_SLICES * 6u;
    // bottom cap starts after top + body.

    if vid < top_cap_verts {
        // Top cap: SPHERE_SLICES triangles, 3 verts each.
        // Triangle t: (north_pole, stack0[t], stack0[t+1])
        let tri = vid / 3u;
        let local = vid % 3u;
        let north = vec3<f32>(0.0, 1.0, 0.0);
        let s0 = sphere_point(1u, tri);
        let s1 = sphere_point(1u, (tri + 1u) % SPHERE_SLICES);
        if local == 0u { return north; }
        else if local == 1u { return s0; }
        else { return s1; }
    } else if vid < top_cap_verts + body_quad_verts {
        // Body quads: (SPHERE_STACKS-1) rows, SPHERE_SLICES quads, 2 tris each.
        // Each quad = 6 verts.
        let body_vid = vid - top_cap_verts;
        let quad = body_vid / 6u;
        let local = body_vid % 6u;
        let row = quad / SPHERE_SLICES;    // 0 .. SPHERE_STACKS-2
        let col = quad % SPHERE_SLICES;
        // Ring indices: row 0 = stack 1, row SPHERE_STACKS-2 = stack SPHERE_STACKS-1.
        let stk0 = row + 1u;
        let stk1 = row + 2u;
        let sl0 = col;
        let sl1 = (col + 1u) % SPHERE_SLICES;
        // Quad: (stk0,sl0), (stk0,sl1), (stk1,sl0), (stk1,sl1)
        // Tri 0: (stk0,sl0), (stk1,sl0), (stk0,sl1)
        // Tri 1: (stk0,sl1), (stk1,sl0), (stk1,sl1)
        switch local {
            case 0u: { return sphere_point(stk0, sl0); }
            case 1u: { return sphere_point(stk1, sl0); }
            case 2u: { return sphere_point(stk0, sl1); }
            case 3u: { return sphere_point(stk0, sl1); }
            case 4u: { return sphere_point(stk1, sl0); }
            default: { return sphere_point(stk1, sl1); }
        }
    } else {
        // Bottom cap: SPHERE_SLICES triangles.
        // Triangle t: (south_pole, stack[SPHERE_STACKS-1][t+1], stack[SPHERE_STACKS-1][t])
        let bot_vid = vid - (top_cap_verts + body_quad_verts);
        let tri = bot_vid / 3u;
        let local = bot_vid % 3u;
        let south = vec3<f32>(0.0, -1.0, 0.0);
        let s0 = sphere_point(SPHERE_STACKS - 1u, tri);
        let s1 = sphere_point(SPHERE_STACKS - 1u, (tri + 1u) % SPHERE_SLICES);
        // Reverse winding vs top cap so normal faces outward.
        if local == 0u { return south; }
        else if local == 1u { return s1; }
        else { return s0; }
    }
}

// ── Sphere vertex output (reuses VertOut) ─────────────────────────────────────

@vertex
fn vs_sphere(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) inst: u32,
) -> VertOut {
    let sph = sphere_instances[inst];

    // Unit outward normal = radial direction on the unit sphere.
    let dir = decode_sphere_vertex(vid);

    // World position: soma center + dir * scaled radius.
    let world = sph.center + dir * (sph.radius * su.width_scale);

    let N = dir;
    let V = normalize(su.camera_pos - world);

    // ── Spike brightness — same logic as tube pass (keyed by neuron_id) ──────
    let packed = sphere_last_spike[sph.neuron_id];
    let ty = neuron_type(packed);
    let ei = ty & 1u;
    let region = (ty >> 2u) & 0x3u;

    var brightness = su.resting_brightness;
    let SOMA_BOOST: f32 = su.active_boost;
    if su.connection_layer >= 1u {
        let src_packed = sphere_last_spike[sph.neuron_id];
        if su.light_next == 1u && has_spiked(src_packed) {
            let glow = exp(-f32(tick_diff(su.tick, src_packed & TICK_MASK)) / max(su.glow_tau, 1.0));
            brightness = brightness + glow * SOMA_BOOST;
        }
    }

    // ── Soma color — E/I and region, same scheme as axon tubes ───────────────
    var color: vec3<f32>;
    if su.color_by == 0u {
        if region == 0u      { color = vec3<f32>(0.30, 0.55, 1.0); }
        else if region == 1u { color = vec3<f32>(0.34, 0.9, 0.5); }
        else                 { color = vec3<f32>(1.0, 0.55, 0.2); }
    } else {
        color = select(vec3<f32>(0.55, 0.72, 1.0), vec3<f32>(1.0, 0.34, 0.28), ei == 1u);
    }
    if su.color_by == 5u {
        color = mix(color, identity_color(sph.neuron_id), IDENTITY_MORPH_BLEND);
    }

    var out: VertOut;
    out.pos = su.mvp * vec4<f32>(world, 1.0);
    out.color = color * brightness;
    out.normal = N;
    out.view_dir = V;
    return out;
}

@fragment
fn fs_sphere(in: VertOut) -> @location(0) vec4<f32> {
    // Reuse the IDENTICAL lighting model from fs_main (ambient + diffuse + rim).
    let N = normalize(in.normal);
    let V = normalize(in.view_dir);
    let L = normalize(su.light_dir);

    let lambert = max(dot(N, L), 0.0);
    let nv = max(dot(N, V), 0.0);
    let rim = pow(1.0 - nv, su.rim_power) * su.rim_intensity;
    let lighting = su.ambient + su.diffuse_intensity * lambert + rim;

    let c = in.color * lighting;
    if c.r + c.g + c.b < 0.002 { discard; }
    return vec4<f32>(c, 1.0);
}
