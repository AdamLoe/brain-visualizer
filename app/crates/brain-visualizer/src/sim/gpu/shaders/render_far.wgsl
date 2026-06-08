// render_far.wgsl — Far-LOD billboard glow pass (Phase 3, architecture §6).
//
// Reads pos_x/pos_y/pos_z/last_spike/v storage buffers and a Uniforms struct
// (mvp, camera_right, camera_up, tick, glow_tau, point_radius, n).
//
// Glow = has_spiked ? exp(-tick_diff / glow_tau) : 0, plus a faint sub-threshold
// v glow. Region color from type bits. Additive blend. Draw(6, N).
//
// V2 Phase E: rainbow hash replaced with orthogonal `color_for(mode, …)` modes;
// `neuron_visual_radius` + `active_neuron_radius_boost` drive size; inactive
// neurons are dimmed (or hidden) via `inactive_neuron_opacity` + the
// `neuron_visibility` mode. Defaults (color_by=0, visibility=0, radius=0.004,
// boost=2.0, opacity=1.0) reproduce the pre-E look (all neurons, active bigger).
//
// Do NOT use @builtin(point_size) — not portable in WGSL/WebGPU (architecture §6).

struct Uniforms {
    mvp: mat4x4<f32>,
    camera_right: vec3<f32>,
    _pad0: f32,
    camera_up: vec3<f32>,
    _pad1: f32,
    tick: u32,
    glow_tau: f32,
    point_radius: f32,
    n: u32,
    camera_pos: vec3<f32>,
    voltage_glow_strength: f32,  // V2 Phase B: debug glow on subthreshold |v| (0=off)
    // ─── V2 Phase E (offset 128): orthogonal color/visibility/radius ─────────
    color_by: u32,                    // 0=region 1=E/I 2=spike-age 3=voltage 4=activity 5=identity
    neuron_visibility: u32,           // 0=all 1=active-emphasis 2=active-only
    neuron_visual_radius: f32,        // base radius (world units)
    active_neuron_radius_boost: f32,  // radius mult at full glow
    // ─── V2 Phase E (offset 144) ─────────────────────────────────────────────
    inactive_neuron_opacity: f32,     // brightness mult for inactive neurons
    _pad2: f32,
    _pad3: f32,
    _pad4: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> pos_x: array<f32>;
@group(0) @binding(2) var<storage, read> pos_y: array<f32>;
@group(0) @binding(3) var<storage, read> pos_z: array<f32>;
@group(0) @binding(4) var<storage, read> last_spike: array<u32>;
@group(0) @binding(5) var<storage, read> v: array<f32>;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;

// V2 Phase E: glow at/above this counts a neuron as "active" (for visibility
// modes + radius boost). Hardcoded; not a settings field (contract frozen at 24).
const ACTIVE_GLOW_THRESHOLD: f32 = 0.06;
// V2 Phase E: optional sparse-stride for inactive neurons. 1 = render all (the
// default look). Raise to thin the inactive cloud when visibility==1.
const INACTIVE_STRIDE: u32 = 1u;
const IDENTITY_SALT: u32 = 0x9f3ab7c2u;
const SOMA_FLASH_RATIO: f32 = 0.18;
const SOMA_CORE_TICKS: f32 = 2.2;
const SOMA_RADIUS_GLOW: f32 = 0.08;
const SOMA_RADIUS_FLASH: f32 = 0.16;

// UX fix (near-LOD / shadow line): close-up billboard size ramp. Below
// NEAR_RADIUS_DIST world units from the camera a neuron's billboard radius grows
// smoothly toward NEAR_RADIUS_MAX× so it reads as a soft round orb when zoomed
// in, instead of shrinking to a dot. At/above NEAR_RADIUS_DIST the scale is 1.0
// (no change to the validated mid/far look). The gaussian falloff (fs_main) is
// resolution-independent, so the orb stays smooth at any on-screen size.
const NEAR_RADIUS_DIST: f32 = 0.8;
const NEAR_RADIUS_MAX: f32 = 6.0;

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}

fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

// 7-bit neuron type from the packed last_spike word (BV21 packing, see
// backend::neuron_type): bits [30..24]. type = (region_code << 2) | ei, so
// region = type>>2 (0=Input,1=Assoc,2=Output) and ei = type&1 (0=exc,1=inh).
fn neuron_type(packed: u32) -> u32 {
    return (packed >> 24u) & 0x7Fu;
}

// HSV-ish hue→RGB (smooth, branchless). h in [0,1].
fn hue(h: f32) -> vec3<f32> {
    let r = clamp(abs(h * 6.0 - 3.0) - 1.0, 0.0, 1.0);
    let g = clamp(2.0 - abs(h * 6.0 - 2.0), 0.0, 1.0);
    let b = clamp(2.0 - abs(h * 6.0 - 4.0), 0.0, 1.0);
    return vec3(r, g, b);
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

fn soma_flash(age: f32, tau: f32) -> f32 {
    return exp(-age / max(safe_tau(tau) * SOMA_FLASH_RATIO, 1.0));
}

fn soma_core(age: f32, glow: f32) -> f32 {
    return (1.0 - smoothstep(0.0, SOMA_CORE_TICKS, age)) * glow;
}

// V2 Phase E: orthogonal color modes. `id` neuron id, `packed` last_spike word,
// `vv` clamped membrane voltage, `glow` recency [0,1].
fn color_for(mode: u32, id: u32, packed: u32, vv: f32, glow: f32) -> vec3<f32> {
    let ty = neuron_type(packed);
    let region = (ty >> 2u) & 0x3u; // 0=Input 1=Assoc 2=Output
    let ei = ty & 1u;               // 0=exc 1=inh
    if mode == 1u {
        // E/I: excitatory cool-blue, inhibitory warm-red.
        return select(vec3(0.30, 0.55, 1.0), vec3(1.0, 0.32, 0.28), ei == 1u);
    } else if mode == 2u {
        // spike-age: warm (fresh) → cool (old) via glow recency.
        return hue(mix(0.62, 0.05, glow)); // 0.62≈blue (old), 0.05≈orange (fresh)
    } else if mode == 3u {
        // voltage (debug): blue (low) → red (high).
        return hue(mix(0.66, 0.0, clamp(vv, 0.0, 1.0)));
    } else if mode == 4u {
        // activity: dim teal (inactive) → bright yellow-white (active).
        return mix(vec3(0.12, 0.20, 0.22), vec3(1.0, 0.95, 0.6), clamp(glow, 0.0, 1.0));
    } else if mode == 5u {
        // identity: stable per-neuron hue from the locked BV22 hash.
        return identity_color(id);
    }
    // mode 0 (default) region: Input cool-blue, Assoc green, Output warm-orange.
    if region == 0u { return vec3(0.28, 0.55, 1.0); }
    if region == 1u { return vec3(0.30, 0.90, 0.45); }
    return vec3(1.0, 0.55, 0.18);
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) glow: f32,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) dist_fade: f32,
    @location(4) vglow: f32,
    @location(5) opacity: f32,  // V2 Phase E: inactive dimming / hide
    @location(6) flash: f32,
    @location(7) core: f32,
}

@vertex
fn vs_main(
    @builtin(vertex_index) quad_vertex: u32,
    @builtin(instance_index) neuron_id: u32,
) -> VertOut {
    let packed    = last_spike[neuron_id];
    let last_tick = packed & TICK_MASK;

    let ticks_since = f32(tick_diff(u.tick, last_tick));
    let glow = select(0.0, exp(-ticks_since / safe_tau(u.glow_tau)), has_spiked(packed));
    let flash = select(0.0, soma_flash(ticks_since, u.glow_tau), has_spiked(packed));
    let core = select(0.0, soma_core(ticks_since, glow), has_spiked(packed));
    let vv = clamp(v[neuron_id], 0.0, 1.0);

    let is_active = max(glow, flash) >= ACTIVE_GLOW_THRESHOLD;

    // V2 Phase E: visibility. 0=all visible; 1=active-emphasis (inactive dimmed
    // by opacity + optional sparse stride); 2=active-only (inactive hidden).
    var opacity = 1.0;
    if !is_active {
        if u.neuron_visibility == 2u {
            opacity = 0.0; // hide
        } else if u.neuron_visibility == 1u {
            opacity = u.inactive_neuron_opacity;
            // Optional sparse stride for the inactive cloud.
            if INACTIVE_STRIDE > 1u && (neuron_id % INACTIVE_STRIDE) != 0u {
                opacity = 0.0;
            }
        } else {
            // mode 0 (all): still honour inactive_neuron_opacity (default 1.0 →
            // no change). Lets the knob dim the resting cloud without hiding.
            opacity = u.inactive_neuron_opacity;
        }
    }

    // Two-triangle quad (triangle-list, 6 vertices per instance).
    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2( 1.0, -1.0), vec2(-1.0,  1.0),
        vec2(-1.0,  1.0), vec2( 1.0, -1.0), vec2( 1.0,  1.0),
    );
    let corner = corners[quad_vertex];
    // V2 Phase E: base radius from neuron_visual_radius; active neurons grow by
    // the boost factor scaled by glow. Inactive stay at base.
    let base = u.neuron_visual_radius;
    let center = vec3<f32>(pos_x[neuron_id], pos_y[neuron_id], pos_z[neuron_id]);

    // UX fix (near-LOD / shadow line): the soft billboards are now the body visual
    // at ALL camera distances (the faceted near-LOD sphere is retired). Without a
    // near boost a world-unit radius shrinks to a tiny dot when the camera dives
    // into the cloud. Gently grow the radius as the camera approaches so zoomed-in
    // neurons read as large soft round orbs (not pinpricks), while staying flat
    // (no boost) at normal/far distances so the validated look is unchanged.
    // near_scale ramps 1→NEAR_RADIUS_MAX as cam_dist falls below NEAR_RADIUS_DIST.
    let cam_dist_c = length(center - u.camera_pos);
    let near_t = clamp((NEAR_RADIUS_DIST - cam_dist_c) / NEAR_RADIUS_DIST, 0.0, 1.0);
    let near_scale = 1.0 + near_t * (NEAR_RADIUS_MAX - 1.0);
    let pulse_scale = 1.0 + glow * (u.active_neuron_radius_boost - 1.0)
        + glow * SOMA_RADIUS_GLOW
        + flash * SOMA_RADIUS_FLASH;
    let radius = base * pulse_scale * near_scale;
    let world_pos = center
        + u.camera_right * corner.x * radius
        + u.camera_up    * corner.y * radius;

    // Fade gray contribution to zero for neurons close to the camera so they
    // don't accumulate into a foggy background when the camera is inside.
    // UX fix (near-LOD / shadow line): widen + smooth the ramp. The old sharp
    // 0.05..0.20 linear band put a hard-edged "bubble" of suppressed resting glow
    // around the camera that read as a faint ring/line when flying through the
    // cloud. Use a wider range and smoothstep so the resting glow fades in
    // gradually with no visible boundary circle.
    let cam_dist = length(center - u.camera_pos);
    let dist_fade = smoothstep(0.05, 0.6, cam_dist);

    // V2 Phase B: debug subthreshold-voltage glow. Default strength 0 → no
    // contribution (pre-V2 look). When enabled, resting neurons glow by |v|.
    let vglow = vv * u.voltage_glow_strength;

    var out: VertOut;
    out.pos       = u.mvp * vec4(world_pos, 1.0);
    out.glow      = glow;
    out.color     = color_for(u.color_by, neuron_id, packed, vv, glow);
    out.uv        = corner;
    out.dist_fade = dist_fade;
    out.vglow     = vglow;
    out.opacity   = opacity;
    out.flash     = flash;
    out.core      = core;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    if d > 1.0 { discard; }
    let falloff = exp(-d * d * 6.0);
    // Gray for resting neurons (fades to zero near camera to avoid fog inside).
    // Colored flash when firing, with a short bright core on the youngest spikes.
    let gray  = vec3(0.18) * in.dist_fade * falloff;
    let spike = in.color * (in.glow * 0.42 + in.flash * 1.08) * falloff;
    let core = mix(in.color, vec3(1.0), 0.65) * in.core * falloff * 0.85;
    // V2 Phase B: voltage glow (debug) — adds brightness from membrane |v|.
    let vglow = in.color * in.vglow * falloff;
    // V2 Phase E: scale the resting/gray contribution by inactive opacity so the
    // active flash always reads at full strength, but resting fog can be dimmed.
    let contrib = (gray * in.opacity) + spike + core + (vglow * in.opacity);
    if contrib.r + contrib.g + contrib.b < 0.002 { discard; }
    return vec4(contrib, 1.0);
}
