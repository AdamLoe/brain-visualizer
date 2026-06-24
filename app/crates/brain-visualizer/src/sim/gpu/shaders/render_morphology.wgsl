// render_morphology.wgsl — V2 Beauty-First procedural neuron morphology + soma spheres.
//
// Instanced, fully GPU-generated tube geometry. One INSTANCE per MorphSegment
// in the currently bound segment chunk;
// TUBE_SIDES * (TUBE_RINGS - 1) * 2 * 3 verts per instance form a tapered,
// gently bowed tube from endpoint `a` (radius radius_a · width_scale) to endpoint
// `b` (radius radius_b · width_scale). Ring centers are curved in-shader from
// existing segment fields so the 48 B MorphSegment contract stays unchanged.
//
// Lighting model (Stage 0 / v0.3.0): one directional light + rim term.
//   ambient + diffuse·max(dot(N,L),0) + rim·pow(1-max(dot(N,V),0), rim_power)
// The lighting multiplier modulates the existing glow/brightness so the additive
// glow aesthetic is preserved (lit-active branches shine through).
//
// Each segment carries its SOURCE neuron_id and cumulative `path_len`. The first
// pulse pass keeps the shared buffer layouts untouched and derives a traveling
// packet directly from the source neuron's packed `last_spike` word plus the
// local path interpolation along the segment.
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
// (TUBE_SIDES * (TUBE_RINGS - 1) * 2 * 3) from the same runtime value, so the two sites stay in sync.
// Default 6 matches the inherited v0.3.0 value.
override TUBE_SIDES: u32 = 6u;
const TUBE_RINGS: u32 = 4u;
const TUBE_SPANS: u32 = TUBE_RINGS - 1u;
const ORGANIC_TUBE_BEND_SALT: u32 = 0x00B10001u;

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
// 128:   color_by u32 | arrival_hold_ticks f32 | reveal_on_arrival u32 | _pad_c u32
// 144:   light_dir     vec3<f32>      (12 B) | ambient         f32 (4 B)
// 160:   diffuse_intensity f32 | rim_intensity f32 | rim_power f32 | _pad3 u32
// 176:   resting_brightness f32 | active_boost f32 | active_opacity f32 | inactive_opacity_floor f32
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
    arrival_hold_ticks: f32, // until-arrival fade duration; mirrors CompactUniforms (was _pad_a)
    reveal_on_arrival: u32,  // 1 = hard front-gate mode-2 segments until the front arrives (was _pad_b)
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
    // True-opacity active layer (read only by fs_main_active / fs_sphere_active).
    active_opacity: f32,          // active-opacity ceiling (was _pad4)
    inactive_opacity_floor: f32,  // inactive-opacity floor (was _pad5)
}

// ── Tube pass bindings (group 0, bindings 0/1/2 + 6) ──────────────────────────
@group(0) @binding(0) var<storage, read> segments: array<MorphSegment>;
@group(0) @binding(1) var<storage, read> last_spike: array<u32>;
@group(0) @binding(2) var<uniform> u: MorphUniforms;
// Active/recent compaction: instance_index → original segment index map. Filled
// by compact_morph_segments.wgsl; the tube passes draw the chunk-local
// `active_segment_count` instances and fetch segments[active_segment_indices[inst]].
// Binding 6 is used
// (not 3/4/5) so it does not collide with the soma pass bindings in this shared
// module.
@group(0) @binding(6) var<storage, read> active_segment_indices: array<u32>;
// Visual-only spike clock for morphology tube impulses. `last_spike` above stays
// the physics/type source; this buffer avoids restarting a packet near the soma
// while an older impulse is still traversing the generated fanout.
@group(0) @binding(7) var<storage, read> visual_spike: array<u32>;

// ── Soma sphere pass bindings (group 0, bindings 3/4/5) ──────────────────────
// The sphere pipeline uses its OWN bind group layout (render_soma_spheres_bgl)
// with entries at binding slots 3/4/5 so both entry-point sets can live in the
// same WGSL module without a slot collision. WebGPU validates only reachable
// bindings per entry point, so the tube pipeline ignores 3/4/5 and vice versa.
//
//   binding 3: sphere_instances (STORAGE, read)  ← MorphSphereInstance array
//   binding 4: sphere_last_spike (STORAGE, read) ← same last_spike buffer
//   binding 5: su (UNIFORM)                      ← same morph_uniform buffer (192 B)

/// Per-soma sphere instance. 48 B, 16-aligned. Field order MUST match
/// `MorphSphereInstance` in src/sim/morphology.rs.
struct SphereInstance {
    center: vec3<f32>,
    radius: f32,
    neuron_id: u32,
    kind: u32,
    _pad0: u32,
    _pad1: u32,
    root_dir: vec3<f32>,
    root_pull: f32,
}

@group(0) @binding(3) var<storage, read> sphere_instances: array<SphereInstance>;
@group(0) @binding(4) var<storage, read> sphere_last_spike: array<u32>;
@group(0) @binding(5) var<uniform> su: MorphUniforms;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;
const IDENTITY_SALT: u32 = 0x9f3ab7c2u;
const IDENTITY_MORPH_BLEND: f32 = 0.62;
const SOMA_FLASH_RATIO: f32 = 0.18;
const SOMA_CORE_TICKS: f32 = 2.2;
const SOMA_RADIUS_GLOW: f32 = 0.08;
const SOMA_RADIUS_FLASH: f32 = 0.16;
const LEGACY_WHOLE_GLOW: f32 = 0.10;
// ── Travelling-packet timing ──────────────────────────────────────────────────
// A single moving Gaussian "packet" sweeps each lit path at `*_SPEED` path-units
// per tick, with a Gaussian half-width of `*_WIDTH` path-units. Two regimes:
//   • LOCAL arbors (path span < LONG_RANGE_PATH): tight, slow packet so the pulse
//     reads inside a dense local arbor without smearing across sibling twigs.
//   • LONG-RANGE projections (path span ≥ LONG_RANGE_PATH): a FASTER, WIDER packet
//     so the eye can follow one blue bolus travelling across the brain instead of
//     the whole fiber blinking. Long axons route through waypoints (path_len up to
//     ~0.8+), so the local speed/width would make the packet crawl and read as a
//     glow rather than motion.
// Classification is per-segment and deterministic: `seg.path_len >= LONG_RANGE_PATH`
// (cumulative path position from the soma). It is MIRRORED verbatim in
// compact_morph_segments.wgsl so the selection window uses the same speed/width.
const AXON_IMPULSE_SPEED: f32 = 0.018;
const DENDRITE_ECHO_SPEED: f32 = 0.006;
const IMPULSE_WIDTH: f32 = 0.028;
// Long-range axon packet (faster + wider; see note above).
const LONG_RANGE_IMPULSE_SPEED: f32 = 0.045;
const LONG_RANGE_IMPULSE_WIDTH: f32 = 0.060;
// Path-position threshold (cumulative path-units from soma) above which a segment
// is treated as a long-range projection segment. Local arbor/trunk segments stay
// well below this; waypoint-routed axons cross it. MUST match the mirror in
// compact_morph_segments.wgsl.
const LONG_RANGE_PATH: f32 = 0.18;
const IMPULSE_TAIL_STRENGTH: f32 = 0.28;
const DENDRITE_ECHO_STRENGTH: f32 = 0.28;
const DENDRITE_ECHO_RANGE: f32 = 0.075;
const ACTIVE_OPACITY_SOFT_MIN: f32 = 0.10;
const ARRIVAL_MODE_REST_BRIGHTNESS: f32 = 0.11;
// Mirror of compact_morph_segments.wgsl ARRIVAL_MODE_MAX_TRAVEL_TICKS: in mode 2
// compaction keeps a segment selected while age <= 28 + arrival_hold_ticks. The
// render fade starts here so the [28 .. 28+hold] ramp window matches selection.
const ARRIVAL_MODE_MAX_TRAVEL_TICKS: f32 = 28.0;
// Axon branch radii are generated from downstream subtree synaptic weight:
// internal radius = root_radius * sqrt(subtree_weight / total_weight), with
// terminal leaves at the configured twig floor. The renderer uses that baked
// radius as a layout-free impulse-flow signal so split branches carry smaller,
// dimmer packets without binding live per-neuron current buffers.
const AXON_FLOW_ROOT_RADIUS: f32 = 0.0054;
const AXON_FLOW_MIN: f32 = 0.22;
const AXON_FLOW_POWER: f32 = 1.35;
const BRAIN_REST_PINK: vec3<f32> = vec3<f32>(1.0, 0.18, 0.54);
const BRAIN_ACTIVE_BLUE: vec3<f32> = vec3<f32>(0.08, 0.56, 1.0);
const BRAIN_SOFT_BLUE: vec3<f32> = vec3<f32>(0.30, 0.68, 1.0);
// Brain 2 (color_by == 7u): resting neuron reads blue, firing region reads red.
const BRAIN2_RESTING_BLUE: vec3<f32> = vec3<f32>(0.0, 0.15, 1.0);
const BRAIN2_FIRING_RED: vec3<f32> = vec3<f32>(1.0, 0.0, 0.0);

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

fn safe_tau(tau: f32) -> f32 {
    return max(tau, 1.0);
}

fn spike_age(now: u32, packed: u32) -> f32 {
    return f32(tick_diff(now, packed & TICK_MASK));
}

fn spike_glow(now: u32, packed: u32, tau: f32) -> f32 {
    if !has_spiked(packed) {
        return 0.0;
    }
    return exp(-spike_age(now, packed) / safe_tau(tau));
}

fn soma_flash(age: f32, tau: f32) -> f32 {
    return exp(-age / max(safe_tau(tau) * SOMA_FLASH_RATIO, 1.0));
}

fn soma_core(age: f32, glow: f32) -> f32 {
    return (1.0 - smoothstep(0.0, SOMA_CORE_TICKS, age)) * glow;
}

fn soma_radius_scale(glow: f32, flash: f32) -> f32 {
    return 1.0 + glow * SOMA_RADIUS_GLOW + flash * SOMA_RADIUS_FLASH;
}

fn deform_soma_dir(dir: vec3<f32>, pull_dir: vec3<f32>, pull: f32) -> vec3<f32> {
    let strength = clamp(pull, 0.0, 0.55);
    let facing = dot(dir, pull_dir);
    let forward = max(facing, 0.0);
    let back = max(-facing, 0.0);
    let front = forward * forward;
    let shoulder = pow(max(1.0 - abs(facing), 0.0), 2.0);

    // Trunk-dominant fairing: elongate toward the dominant root, add a small
    // shoulder around the root, and compress the opposite side just enough that
    // the body reads pulled rather than uniformly scaled.
    let radial = max(0.75, 1.0 + strength * (0.28 * front + 0.08 * shoulder - 0.05 * back));
    let axial = strength * (0.34 * front + 0.08 * forward - 0.04 * back);
    return dir * radial + pull_dir * axial;
}

fn material_hash(seed: u32) -> f32 {
    return f32(hash32(seed)) / 4294967295.0;
}

fn material_noise3(p: vec3<f32>, seed: u32) -> f32 {
    let phase = material_hash(seed) * 17.0;
    let q = vec3<f32>(
        dot(p, vec3<f32>(0.83, 1.31, 0.47)) + phase,
        dot(p, vec3<f32>(1.11, 0.57, 0.89)) + phase * 1.37,
        dot(p, vec3<f32>(0.61, 0.73, 1.27)) + phase * 1.79,
    );
    return fract(sin(dot(q, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

fn branch_base_color(kind: u32, region: u32, ei: u32, color_by: u32, neuron_id: u32) -> vec3<f32> {
    if color_by == 6u {
        return BRAIN_REST_PINK;
    }
    if color_by == 7u {
        return BRAIN2_RESTING_BLUE;
    }
    var color: vec3<f32>;
    if kind == 0u {
        color = vec3<f32>(0.22, 0.34, 0.5);
    } else if color_by == 0u {
        if region == 0u {
            color = vec3<f32>(0.30, 0.55, 1.0);
        } else if region == 1u {
            color = vec3<f32>(0.34, 0.9, 0.5);
        } else {
            color = vec3<f32>(1.0, 0.55, 0.2);
        }
    } else {
        color = select(vec3<f32>(0.55, 0.72, 1.0), vec3<f32>(1.0, 0.34, 0.28), ei == 1u);
    }
    if color_by == 5u {
        color = mix(color, identity_color(neuron_id), IDENTITY_MORPH_BLEND);
    }
    return color;
}

fn soma_base_color(region: u32, ei: u32, color_by: u32, neuron_id: u32) -> vec3<f32> {
    if color_by == 6u {
        return BRAIN_REST_PINK;
    }
    if color_by == 7u {
        return BRAIN2_RESTING_BLUE;
    }
    var color: vec3<f32>;
    if color_by == 0u {
        if region == 0u {
            color = vec3<f32>(0.30, 0.55, 1.0);
        } else if region == 1u {
            color = vec3<f32>(0.34, 0.9, 0.5);
        } else {
            color = vec3<f32>(1.0, 0.55, 0.2);
        }
    } else {
        color = select(vec3<f32>(0.55, 0.72, 1.0), vec3<f32>(1.0, 0.34, 0.28), ei == 1u);
    }
    if color_by == 5u {
        color = mix(color, identity_color(neuron_id), IDENTITY_MORPH_BLEND);
    }
    return color;
}

fn brain_tube_tint(material: vec3<f32>, legacy: f32, packet: f32) -> vec3<f32> {
    let halo_t = clamp(legacy * 0.35, 0.0, 0.25);
    let packet_t = clamp(packet * 1.6, 0.0, 1.0);
    return mix(mix(material, BRAIN_SOFT_BLUE, halo_t), BRAIN_ACTIVE_BLUE, packet_t);
}

// Brain 2 tube tint: blue at rest, saturating to red where the fragment is firing.
// `activity` is the per-fragment legacy + packet_flow signal; k is chosen so a
// passing impulse clearly pushes the segment to full red.
fn brain2_tube_tint(activity: f32) -> vec3<f32> {
    let firing_t = clamp(activity * 6.0, 0.0, 1.0);
    return mix(BRAIN2_RESTING_BLUE, BRAIN2_FIRING_RED, firing_t);
}

fn brain_soma_material(material: vec3<f32>, glow: f32, flash: f32) -> vec3<f32> {
    let active_t = clamp(glow * 0.25 + flash * 0.75, 0.0, 1.0);
    return mix(material, BRAIN_SOFT_BLUE, active_t);
}

// Brain 2 soma body: blue at rest, saturating to red as the soma fires (glow/flash).
fn brain2_soma_material(glow: f32, flash: f32) -> vec3<f32> {
    let firing_t = clamp(glow * 0.5 + flash * 1.0, 0.0, 1.0);
    return mix(BRAIN2_RESTING_BLUE, BRAIN2_FIRING_RED, firing_t);
}

fn tube_material(
    base: vec3<f32>,
    n: vec3<f32>,
    world: vec3<f32>,
    path: f32,
    neuron_id: u32,
    kind: u32,
) -> vec3<f32> {
    let seed = mix_key(0u, neuron_id, kind, IDENTITY_SALT ^ 0x2c79d31bu);
    let striation = material_noise3(
        vec3<f32>(
            path * 10.0,
            dot(world, vec3<f32>(0.42, 0.71, 0.33)) * 3.0,
            dot(n, vec3<f32>(0.62, 0.21, 0.75)) * 2.0,
        ),
        seed,
    );
    let sheath = material_noise3(world * 7.0 + n * 1.1, seed ^ 0x68bc21ebu);
    let neuron_shift = material_hash(seed ^ 0x9e3779b9u) - 0.5;
    let shade = 0.93 + (striation - 0.5) * 0.14 + (sheath - 0.5) * 0.10 + neuron_shift * 0.05;
    let kind_tint = select(vec3<f32>(0.95, 0.99, 1.04), vec3<f32>(1.02, 1.0, 0.98), kind == 1u);
    return base * shade * kind_tint;
}

fn soma_material(
    base: vec3<f32>,
    n: vec3<f32>,
    world: vec3<f32>,
    neuron_id: u32,
    glow: f32,
    flash: f32,
) -> vec3<f32> {
    let seed = mix_key(0u, neuron_id, 1u, IDENTITY_SALT ^ 0x51a3f27du);
    let membrane = material_noise3(world * 2.4 + n * 0.8, seed);
    let mottle = material_noise3(world * 6.5 + n * 1.4, seed ^ 0x7f4a7c15u);
    let speck = material_noise3(world * 18.0, seed ^ 0xa511e9b3u);
    let speckle = smoothstep(0.86, 1.0, speck) * 0.08;
    let shade = 0.92 + (membrane - 0.5) * 0.18 + (mottle - 0.5) * 0.12 + speckle;
    return base * shade + vec3<f32>(flash * 0.10 + glow * 0.04);
}

// Per-segment packet speed. Long-range axon segments sweep faster so one bolus
// travels visibly across the brain; local arbors and all dendrites keep the
// tight local timing. `long_range` is true only for axon segments past
// LONG_RANGE_PATH (kind is folded into the caller's flag).
fn impulse_speed(kind: u32, long_range: bool) -> f32 {
    let axon_speed = select(AXON_IMPULSE_SPEED, LONG_RANGE_IMPULSE_SPEED, long_range);
    return select(DENDRITE_ECHO_SPEED, axon_speed, kind == 1u);
}

fn impulse_width(kind: u32, long_range: bool) -> f32 {
    return select(IMPULSE_WIDTH, LONG_RANGE_IMPULSE_WIDTH, long_range && kind == 1u);
}

fn impulse_travel(age: f32, kind: u32, long_range: bool) -> f32 {
    return age * impulse_speed(kind, long_range);
}

fn impulse_packet(path_pos: f32, age: f32, packet_gate: f32, kind: u32, long_range: bool) -> f32 {
    if packet_gate <= 0.0 {
        return 0.0;
    }
    let travel = impulse_travel(age, kind, long_range);
    let width = impulse_width(kind, long_range);
    let delta = path_pos - travel;
    let head = exp(-(delta * delta) / max(width * width, 1e-4));
    let behind = max(travel - path_pos, 0.0);
    let tail = exp(-behind / max(width * 2.6, 1e-4)) * select(0.0, 1.0, path_pos <= travel);
    var packet = (head + tail * IMPULSE_TAIL_STRENGTH) * packet_gate;
    if kind == 0u {
        packet = packet * DENDRITE_ECHO_STRENGTH * exp(-path_pos / DENDRITE_ECHO_RANGE);
    }
    return packet;
}

fn impulse_segment_activity(seg_start: f32, seg_end: f32, age: f32, packet_gate: f32, kind: u32, long_range: bool) -> f32 {
    if packet_gate <= 0.0 {
        return 0.0;
    }
    let travel = impulse_travel(age, kind, long_range);
    let width = impulse_width(kind, long_range);
    let a = min(seg_start, seg_end);
    let b = max(seg_start, seg_end);
    let inside = travel >= a && travel <= b;
    let distance_to_segment = select(min(abs(travel - a), abs(travel - b)), 0.0, inside);
    let proximity = 1.0 - smoothstep(width, width * 3.0, distance_to_segment);
    return clamp(proximity * packet_gate, 0.0, 1.0);
}

fn impulse_flow_strength(radius: f32, kind: u32) -> f32 {
    if kind != 1u {
        return 1.0;
    }
    let radius_ratio = clamp(radius / AXON_FLOW_ROOT_RADIUS, 0.0, 1.0);
    return mix(AXON_FLOW_MIN, 1.0, pow(radius_ratio, AXON_FLOW_POWER));
}

fn active_opacity_ceiling(active_opacity: f32, inactive_floor: f32) -> f32 {
    let floor = clamp(inactive_floor, 0.0, 1.0);
    let requested = clamp(active_opacity, 0.0, 1.0);
    return max(floor, mix(ACTIVE_OPACITY_SOFT_MIN, 1.0, requested));
}

fn tube_resting_brightness(connection_layer: u32, configured: f32) -> f32 {
    return select(configured, max(configured, ARRIVAL_MODE_REST_BRIGHTNESS), connection_layer >= 2u);
}

// Mode-2 fade factor over the [28 .. 28+hold] window, matching the compaction
// selection lifetime (28 + arrival_hold_ticks). Returns 1.0 at/below age 28
// (unchanged subdued rest value), ramps 1.0→0.0 across the hold window, and 0.0
// at/after the compaction drop point. `denom = max(hold, 1.0)` guards hold == 0
// (where compaction drops the segment at age 28, leaving no real fade window).
fn arrival_fade_factor(arrival_age: f32, hold_ticks: f32) -> f32 {
    let hold = max(hold_ticks, 0.0);
    let denom = max(hold, 1.0);
    return 1.0 - clamp((arrival_age - ARRIVAL_MODE_MAX_TRAVEL_TICKS) / denom, 0.0, 1.0);
}

// Reveal-on-arrival gate (until-arrival sub-option). Returns false only when the
// reveal_on_arrival mode is on, the segment is mode-2 eligible, AND the impulse
// front has not yet reached the segment's start. A hard front-gate (reveal as the
// front is drawn), not a soft fade-in: `travel = impulse_travel(arrival_age)` and
// the segment reveals when `travel >= segment_start`. Modes 0/1 and the off case
// are unconditionally revealed (the caller still keys the whole effect on layer >= 2u).
fn reveal_gated(
    reveal_on: u32,
    connection_layer: u32,
    arrival_age: f32,
    segment_start: f32,
    kind: u32,
    long_range: bool,
) -> bool {
    if reveal_on != 1u || connection_layer < 2u {
        return true;
    }
    let travel = impulse_travel(arrival_age, kind, long_range);
    return travel >= segment_start;
}

struct TubeVertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) base_color: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) view_dir: vec3<f32>,
    @location(3) world_pos: vec3<f32>,
    @location(4) path_pos: f32,
    @location(5) spike_age: f32,
    @location(6) glow: f32,
    @location(7) @interpolate(flat) kind: u32,
    @location(8) @interpolate(flat) neuron_id: u32,
    @location(9) @interpolate(flat) segment_start: f32,
    @location(10) @interpolate(flat) segment_end: f32,
    @location(11) @interpolate(flat) long_range: u32,
    @location(12) packet_gate: f32,
    @location(13) flow_strength: f32,
    // Ungated spike age (ticks) on the same visual_spike word compaction uses.
    // Unlike spike_age (gated by spike_enabled), this is always the real age so
    // the mode-2 fade can ramp the subdued resting branch out over the hold window.
    @location(14) arrival_age: f32,
}

struct SphereVertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) base_color: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) view_dir: vec3<f32>,
    @location(3) world_pos: vec3<f32>,
    @location(4) glow: f32,
    @location(5) flash: f32,
    @location(6) core: f32,
    @location(7) @interpolate(flat) neuron_id: u32,
}

// ── Tube vertex generation ────────────────────────────────────────────────────
//
// Vertices per instance: TUBE_SIDES * (TUBE_RINGS - 1) * 2 * 3
// Layout: for each axial span and side, emit 2 triangles (6 verts).
// Quad (span, s) connects ring-side s and ring-side (s+1) % TUBE_SIDES across
// rings `span` and `span + 1`.
//
// Triangle 0: (ring0,s), (ring0,s+1), (ring1,s)
// Triangle 1: (ring1,s), (ring0,s+1), (ring1,s+1)
//
// Within each block of 6 verts (vid 0..5 for side s):
//   vid 0 → (ring=span,     s)
//   vid 1 → (ring=span,     s+1)
//   vid 2 → (ring=span + 1, s)
//   vid 3 → (ring=span + 1, s)
//   vid 4 → (ring=span,     s+1)
//   vid 5 → (ring=span + 1, s+1)

fn tube_curve_bend(seg: MorphSegment, seg_len: f32, u_vec: vec3<f32>, v_vec: vec3<f32>) -> vec3<f32> {
    let path_key = u32(abs(seg.path_len) * 4096.0);
    let h0 = f32(mix_key(seg.neuron_id, seg.target_id, path_key, ORGANIC_TUBE_BEND_SALT)) / 4294967295.0;
    let h1 = f32(mix_key(seg.target_id, seg.neuron_id, path_key ^ seg.kind, ORGANIC_TUBE_BEND_SALT ^ 0x6a09e667u)) / 4294967295.0;
    var bend_raw = u_vec * (h0 * 2.0 - 1.0) + v_vec * (h1 * 2.0 - 1.0);
    if length(bend_raw) < 1e-5 {
        bend_raw = u_vec;
    }
    let bend_dir = normalize(bend_raw);
    let local_strength = select(0.18, 0.13, seg.kind == 1u);
    let long_strength = select(local_strength, local_strength * 0.55, seg.path_len >= LONG_RANGE_PATH);
    return bend_dir * seg_len * long_strength;
}

fn tube_ring_basis(tangent: vec3<f32>, fallback_hint: vec3<f32>) -> mat2x3<f32> {
    var fallback = fallback_hint;
    if abs(dot(normalize(tangent), normalize(fallback))) > 0.94 {
        fallback = vec3<f32>(1.0, 0.0, 0.0);
        if abs(dot(normalize(tangent), fallback)) > 0.94 {
            fallback = vec3<f32>(0.0, 0.0, 1.0);
        }
    }
    let ring_u = normalize(cross(normalize(tangent), fallback));
    let ring_v = cross(normalize(tangent), ring_u);
    return mat2x3<f32>(ring_u, ring_v);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) inst: u32,
) -> TubeVertOut {
    let seg_index = active_segment_indices[inst];
    let seg = segments[seg_index];
    let a = seg.a;
    let b = seg.b;

    // ── Build tube basis ─────────────────────────────────────────────────────
    let axis = b - a;
    let seg_len = length(axis);
    var tube_axis = normalize(axis);
    // Guard near-zero-length segments: output degenerate (at-a) rather than NaN.
    if seg_len < 1e-9 {
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
    // 6 verts per quad; quads are grouped by axial span then side.
    let quad = vid / 6u;
    let span = quad / TUBE_SIDES;
    let side = quad % TUBE_SIDES;
    let local = vid % 6u;

    // The two column indices in the ring for this triangle pair.
    let s0 = side;
    let s1 = (side + 1u) % TUBE_SIDES;

    // Map local vertex to (ring, column).
    // Triangle 0 (local 0,1,2): (ring0,s0), (ring0,s1), (ring1,s0)
    // Triangle 1 (local 3,4,5): (ring1,s0), (ring0,s1), (ring1,s1)
    var ring: u32;
    var col: u32;
    switch local {
        case 0u: { ring = span; col = s0; }
        case 1u: { ring = span; col = s1; }
        case 2u: { ring = span + 1u; col = s0; }
        case 3u: { ring = span + 1u; col = s0; }
        case 4u: { ring = span; col = s1; }
        default: { ring = span + 1u; col = s1; }
    }

    // ── Ring position ────────────────────────────────────────────────────────
    let theta = 6.283185307 * f32(col) / f32(TUBE_SIDES); // 2π * col / TUBE_SIDES
    let cos_t = cos(theta);
    let sin_t = sin(theta);
    let t = f32(ring) / f32(TUBE_SPANS);
    let bend = tube_curve_bend(seg, seg_len, u_vec, v_vec);
    let bend_weight = 4.0 * t * (1.0 - t);
    let center = a + axis * t + bend * bend_weight;
    let tangent = normalize(axis + bend * (4.0 - 8.0 * t));
    let ring_basis = tube_ring_basis(tangent, fallback);
    let radial = cos_t * ring_basis[0] + sin_t * ring_basis[1];

    let unscaled_radius = mix(seg.radius_a, seg.radius_b, t);
    let radius = unscaled_radius * u.width_scale;
    let world = center + radial * radius;
    let path_pos = seg.path_len + seg_len * t;

    // ── Normal and view direction for lighting ────────────────────────────────
    let N = radial; // unit outward normal = radial direction
    let world_to_cam = u.camera_pos - world;
    let V = normalize(world_to_cam);

    // ── Source timing + structural color ─────────────────────────────────────
    let owner_packed = last_spike[seg.neuron_id];
    let presynaptic_dendrite = seg.kind == 0u && seg.target_id != seg.neuron_id;
    let activity_id = select(seg.neuron_id, seg.target_id, presynaptic_dendrite);
    let activity_packed = visual_spike[activity_id];
    let ty = neuron_type(owner_packed);
    let ei = ty & 1u;
    let region = (ty >> 2u) & 0x3u;
    let light_enabled = u.light_next == 1u || (presynaptic_dendrite && u.light_past == 1u);
    let spike_enabled = u.connection_layer >= 1u && light_enabled && has_spiked(activity_packed);
    let glow = select(0.0, spike_glow(u.tick, activity_packed, u.glow_tau), spike_enabled);
    let age = select(0.0, spike_age(u.tick, activity_packed), spike_enabled);
    let color = branch_base_color(seg.kind, region, ei, u.color_by, seg.neuron_id);

    var out: TubeVertOut;
    out.pos = u.mvp * vec4<f32>(world, 1.0);
    out.base_color = color;
    out.normal = N;
    out.view_dir = V;
    out.world_pos = world;
    out.path_pos = path_pos;
    out.spike_age = age;
    out.glow = glow;
    out.kind = seg.kind;
    out.neuron_id = seg.neuron_id;
    out.segment_start = seg.path_len;
    out.segment_end = seg.path_len + seg_len;
    // Long-range classification (deterministic, mirrored in compaction): a segment
    // whose cumulative path position has crossed LONG_RANGE_PATH is on a waypoint-
    // routed projection. Only axons use the long-range packet regime; dendrites
    // keep the local echo regardless, but the flag is set uniformly here and the
    // axon-only gate lives in impulse_speed/impulse_width.
    out.long_range = select(0u, 1u, seg.path_len >= LONG_RANGE_PATH);
    out.packet_gate = select(0.0, 1.0, spike_enabled);
    out.flow_strength = impulse_flow_strength(unscaled_radius, seg.kind);
    // Mode-2 fade age: ungated, real spike age on the compaction word. A never-fired
    // segment (not selected by compaction in mode 2) gets a large age so it reads as
    // fully faded if a boundary fragment ever samples it. Used only when layer >= 2u.
    out.arrival_age = select(1e9, spike_age(u.tick, activity_packed), has_spiked(activity_packed));
    return out;
}

@fragment
fn fs_main(in: TubeVertOut) -> @location(0) vec4<f32> {
    // ── Lighting model ────────────────────────────────────────────────────────
    // Simple ambient + half-Lambert diffuse + Fresnel-approximation rim.
    // Modulates the glow brightness so active/lit tubes punch through.
    let N = normalize(in.normal);
    let V = normalize(in.view_dir);
    let L = normalize(u.light_dir); // pre-normalised in CPU but normalize again for safety

    let long_range = in.long_range == 1u;
    let packet = impulse_packet(in.path_pos, in.spike_age, in.packet_gate, in.kind, long_range);
    let packet_flow = packet * in.flow_strength;
    let legacy = in.glow * select(0.04, LEGACY_WHOLE_GLOW, in.kind == 1u) * in.flow_strength;
    let activity = legacy + packet_flow;
    let material = tube_material(in.base_color, N, in.world_pos, in.path_pos, in.neuron_id, in.kind);
    let tint = select(
        select(
            mix(material, vec3<f32>(1.0), clamp(packet_flow * 0.18, 0.0, 0.18)),
            brain_tube_tint(material, legacy, packet_flow),
            u.color_by == 6u,
        ),
        brain2_tube_tint(activity),
        u.color_by == 7u,
    );
    let resting_base = tube_resting_brightness(u.connection_layer, u.resting_brightness);
    // Mode-2 only: fade the subdued resting term to nothing over the hold window so
    // the until-arrival branch ramps out instead of popping. The packet/active term
    // (`activity * active_boost`) is left unfaded — a still-traveling pulse should
    // punch through. Modes 0/1 are byte-identical (fade factor selected off).
    let arrival_fade = arrival_fade_factor(in.arrival_age, u.arrival_hold_ticks);
    // Reveal-on-arrival: hard front-gate the subdued resting term to nothing until
    // the impulse front has reached this segment's start (mode-2 only). The packet
    // term is left intact — it is zero ahead of the front anyway.
    let revealed = reveal_gated(
        u.reveal_on_arrival, u.connection_layer, in.arrival_age, in.segment_start, in.kind, long_range,
    );
    let resting_fade = select(0.0, arrival_fade, revealed);
    let resting_brightness = select(resting_base, resting_base * resting_fade, u.connection_layer >= 2u);
    let brightness = resting_brightness + activity * u.active_boost;

    let lambert = max(dot(N, L), 0.0);
    let nv = max(dot(N, V), 0.0);
    let rim = pow(1.0 - nv, u.rim_power) * u.rim_intensity * (1.0 + clamp(activity, 0.0, 1.0) * 0.25);
    let lighting = u.ambient + u.diffuse_intensity * lambert + rim;

    let c = tint * brightness * lighting;
    if c.r + c.g + c.b < 0.002 { discard; }
    return vec4<f32>(c, 1.0);
}

// True-opacity active tube pass (active-opacity-render-pass). Same color as the
// additive fs_main, but uses a continuous segment-level packet-proximity factor
// for straight alpha and is rendered depth-tested + alpha-blended so a firing
// tube genuinely occludes the additive background behind it. Brightness stays
// fragment-local (`activity` = legacy + packet) so the impulse still travels
// through the temporarily opaque segment.
@fragment
fn fs_main_active(in: TubeVertOut) -> @location(0) vec4<f32> {
    let N = normalize(in.normal);
    let V = normalize(in.view_dir);
    let L = normalize(u.light_dir);

    let long_range = in.long_range == 1u;
    let packet = impulse_packet(in.path_pos, in.spike_age, in.packet_gate, in.kind, long_range);
    let packet_flow = packet * in.flow_strength;
    let legacy = in.glow * select(0.04, LEGACY_WHOLE_GLOW, in.kind == 1u) * in.flow_strength;
    let activity = legacy + packet_flow;
    let material = tube_material(in.base_color, N, in.world_pos, in.path_pos, in.neuron_id, in.kind);
    let tint = select(
        select(
            mix(material, vec3<f32>(1.0), clamp(packet_flow * 0.18, 0.0, 0.18)),
            brain_tube_tint(material, legacy, packet_flow),
            u.color_by == 6u,
        ),
        brain2_tube_tint(activity),
        u.color_by == 7u,
    );
    let resting_base = tube_resting_brightness(u.connection_layer, u.resting_brightness);
    // Same mode-2 resting fade as fs_main (see there). Resting-only; packet unfaded.
    let arrival_fade = arrival_fade_factor(in.arrival_age, u.arrival_hold_ticks);
    // Reveal-on-arrival: hard front-gate as in fs_main. Zeroes the resting term AND
    // the selection alpha floor (below) so the segment is fully hidden pre-arrival.
    let revealed = reveal_gated(
        u.reveal_on_arrival, u.connection_layer, in.arrival_age, in.segment_start, in.kind, long_range,
    );
    let resting_fade = select(0.0, arrival_fade, revealed);
    let resting_brightness = select(resting_base, resting_base * resting_fade, u.connection_layer >= 2u);
    let brightness = resting_brightness + activity * u.active_boost;

    let lambert = max(dot(N, L), 0.0);
    let nv = max(dot(N, V), 0.0);
    let rim = pow(1.0 - nv, u.rim_power) * u.rim_intensity * (1.0 + clamp(activity, 0.0, 1.0) * 0.25);
    let lighting = u.ambient + u.diffuse_intensity * lambert + rim;

    let c = tint * brightness * lighting;
    let segment_activity = impulse_segment_activity(
        in.segment_start,
        in.segment_end,
        in.spike_age,
        in.packet_gate,
        in.kind,
        long_range,
    ) * in.flow_strength;
    // Reveal-on-arrival zeroes the inactive coverage floor pre-arrival too, so an
    // unrevealed segment contributes no alpha (no resting floor, no selection floor)
    // and hits the discard below — a true hard front-gate, not a dimmed segment.
    let inactive_floor = select(0.0, clamp(u.inactive_opacity_floor, 0.0, 1.0), revealed);
    let active_ceiling = active_opacity_ceiling(u.active_opacity, inactive_floor);
    // Packet proximity drives opacity continuously from the inactive floor to
    // the active ceiling. active_opacity=0 maps to a soft low-end ceiling so the
    // active pass still damps additive blowout instead of disappearing.
    // Mode-2: the selection floor was a constant 1.0 (segment fully opaque until
    // compaction dropped it). `resting_fade` ramps it with arrival_fade so opacity
    // fades to 0 over the hold window AND (with reveal-on-arrival) holds at 0 until
    // the front arrives; the active packet term still drives alpha up via the mix.
    // As resting_fade → 0 the floor reaches 0 and the discard below cleanly drops it.
    let visible_selected = select(0.0, resting_fade, u.connection_layer >= 2u);
    let active_alpha = max(mix(inactive_floor, active_ceiling, segment_activity), visible_selected);
    // Below epsilon, write neither color nor depth (in-shader inactive skip).
    if active_alpha < 0.004 { discard; }
    return vec4<f32>(c, active_alpha);
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

@vertex
fn vs_sphere(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) inst: u32,
) -> SphereVertOut {
    let sph = sphere_instances[inst];

    // Unit outward normal = radial direction on the unit sphere.
    let dir = decode_sphere_vertex(vid);
    var pull_dir = vec3<f32>(0.0, 0.0, 1.0);
    if length(sph.root_dir) > 1e-5 {
        pull_dir = normalize(sph.root_dir);
    }
    let deformed_dir = deform_soma_dir(dir, pull_dir, sph.root_pull);

    // ── Source timing + sphere radius pulse ──────────────────────────────────
    let packed = sphere_last_spike[sph.neuron_id];
    let ty = neuron_type(packed);
    let ei = ty & 1u;
    let region = (ty >> 2u) & 0x3u;
    let glow = select(0.0, spike_glow(su.tick, packed, su.glow_tau), su.connection_layer >= 1u && su.light_next == 1u);
    let age = select(0.0, spike_age(su.tick, packed), glow > 0.0);
    let flash = select(0.0, soma_flash(age, su.glow_tau), glow > 0.0);
    let core = soma_core(age, glow);
    let world = sph.center + deformed_dir * (sph.radius * su.width_scale * soma_radius_scale(glow, flash));
    let N = normalize(deformed_dir);
    let V = normalize(su.camera_pos - world);

    var out: SphereVertOut;
    out.pos = su.mvp * vec4<f32>(world, 1.0);
    out.base_color = soma_base_color(region, ei, su.color_by, sph.neuron_id);
    out.normal = N;
    out.view_dir = V;
    out.world_pos = world;
    out.glow = glow;
    out.flash = flash;
    out.core = core;
    out.neuron_id = sph.neuron_id;
    return out;
}

@fragment
fn fs_sphere(in: SphereVertOut) -> @location(0) vec4<f32> {
    // Reuse the IDENTICAL lighting model from fs_main (ambient + diffuse + rim).
    let N = normalize(in.normal);
    let V = normalize(in.view_dir);
    let L = normalize(su.light_dir);
    let raw_material = soma_material(in.base_color, N, in.world_pos, in.neuron_id, in.glow, in.flash);
    let material = select(
        select(raw_material, brain_soma_material(raw_material, in.glow, in.flash), su.color_by == 6u),
        brain2_soma_material(in.glow, in.flash),
        su.color_by == 7u,
    );
    let brightness = su.resting_brightness + (in.glow * 0.55 + in.flash * 1.15) * su.active_boost;

    let lambert = max(dot(N, L), 0.0);
    let nv = max(dot(N, V), 0.0);
    let rim = pow(1.0 - nv, su.rim_power) * su.rim_intensity * (1.0 + in.flash * 0.45);
    let lighting = su.ambient + su.diffuse_intensity * lambert + rim;
    let core_color = select(
        select(mix(material, vec3<f32>(1.0), 0.70), BRAIN_ACTIVE_BLUE, su.color_by == 6u),
        BRAIN2_FIRING_RED,
        su.color_by == 7u,
    );
    let core = core_color * in.core * 0.85;

    let c = (material * brightness + core) * lighting;
    if c.r + c.g + c.b < 0.002 { discard; }
    return vec4<f32>(c, 1.0);
}

// True-opacity active soma pass (active-opacity-render-pass). Same color as the
// additive fs_sphere, but returns a firing-driven straight alpha and is rendered
// depth-tested + alpha-blended so a firing soma genuinely occludes the additive
// background behind it. The firing signal is the soma's glow/flash/core energy
// (the same "active = firing" source the additive path lights from), and shares
// the tube pass's soft active_opacity=0 low end.
@fragment
fn fs_sphere_active(in: SphereVertOut) -> @location(0) vec4<f32> {
    let N = normalize(in.normal);
    let V = normalize(in.view_dir);
    let L = normalize(su.light_dir);
    let raw_material = soma_material(in.base_color, N, in.world_pos, in.neuron_id, in.glow, in.flash);
    let material = select(
        select(raw_material, brain_soma_material(raw_material, in.glow, in.flash), su.color_by == 6u),
        brain2_soma_material(in.glow, in.flash),
        su.color_by == 7u,
    );
    let brightness = su.resting_brightness + (in.glow * 0.55 + in.flash * 1.15) * su.active_boost;

    let lambert = max(dot(N, L), 0.0);
    let nv = max(dot(N, V), 0.0);
    let rim = pow(1.0 - nv, su.rim_power) * su.rim_intensity * (1.0 + in.flash * 0.45);
    let lighting = su.ambient + su.diffuse_intensity * lambert + rim;
    let core_color = select(
        select(mix(material, vec3<f32>(1.0), 0.70), BRAIN_ACTIVE_BLUE, su.color_by == 6u),
        BRAIN2_FIRING_RED,
        su.color_by == 7u,
    );
    let core = core_color * in.core * 0.85;

    let c = (material * brightness + core) * lighting;
    let activity = clamp(in.glow + in.flash + in.core, 0.0, 1.0);
    let inactive_floor = clamp(su.inactive_opacity_floor, 0.0, 1.0);
    let active_ceiling = active_opacity_ceiling(su.active_opacity, inactive_floor);
    let active_alpha = mix(inactive_floor, active_ceiling, activity);
    if active_alpha < 0.004 { discard; }
    return vec4<f32>(c, active_alpha);
}
