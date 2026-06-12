// compact_morph_segments.wgsl — active/recent morphology segment compaction.
//
// Replaces the all-segment morphology tube draw with a GPU compaction stage:
// a compute pass scans every MorphSegment in the currently bound segment chunk,
// derives the segment's ACTIVITY OWNER
// using the EXACT SAME rule as render_morphology.wgsl::vs_main, reads
// last_spike[owner], and — mirroring the shader's impulse decode — appends
// the chunk-local segment index to `active_segment_indices` ONLY when the segment is
// currently lit, recently lit, or ABOUT to be lit (a headroom window so a
// long-range packet is submitted slightly before it arrives). A second
// single-thread entry point copies the atomic counter into a DrawIndirectArgs
// buffer so the tube passes draw exactly the selected instances.
//
// Discipline: this is intentionally CONSERVATIVE — it is correct to select a few
// extra segments, wrong to drop a segment the shader would light. The owner rule
// + impulse travel constants below MUST stay in lockstep with
// render_morphology.wgsl (the authoritative source of truth).
//
// MorphSegment field order + size (48 B) MUST match render_morphology.wgsl /
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

// CompactUniforms — layout MUST match `CompactUniforms` in resources.rs.
struct CompactUniforms {
    tick: u32,
    segment_count: u32,
    glow_tau: f32,
    connection_layer: u32,
    light_next: u32,
    light_past: u32,
    tube_verts: u32, // vertex_count written into the indirect draw args
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> segments: array<MorphSegment>;
@group(0) @binding(1) var<storage, read> last_spike: array<u32>;
@group(0) @binding(2) var<uniform> u: CompactUniforms;
@group(0) @binding(3) var<storage, read_write> active_indices: array<u32>;
@group(0) @binding(4) var<storage, read_write> active_count: atomic<u32>;
// DrawIndirectArgs (non-indexed): [vertex_count, instance_count, first_vertex, first_instance].
@group(0) @binding(5) var<storage, read_write> draw_args: array<u32>;
// Profiler: last selected count (mirror of active_count after the dispatch).
@group(0) @binding(6) var<storage, read_write> selected_count: atomic<u32>;

// ── Mirror of render_morphology.wgsl decode constants ────────────────────────
// These MUST stay byte-for-byte in lockstep with render_morphology.wgsl: speed,
// width, AND the long-range split (LONG_RANGE_* + LONG_RANGE_PATH). If the render
// shader's packet uses a speed/width this selection window does not match, packets
// pop in late or get culled mid-flight.
const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;
const AXON_IMPULSE_SPEED: f32 = 0.018;
const DENDRITE_ECHO_SPEED: f32 = 0.006;
const IMPULSE_WIDTH: f32 = 0.028;
// Long-range axon packet regime (mirror of render_morphology.wgsl). Faster +
// wider so one bolus sweeps a waypoint-routed projection.
const LONG_RANGE_IMPULSE_SPEED: f32 = 0.045;
const LONG_RANGE_IMPULSE_WIDTH: f32 = 0.060;
// Per-segment long-range classification threshold (cumulative path-units). Must
// match render_morphology.wgsl::LONG_RANGE_PATH and its `seg.path_len >= …` test.
const LONG_RANGE_PATH: f32 = 0.18;

// Window factors expressed in MULTIPLES of the (per-segment) packet width, so the
// selection window scales automatically with the wider long-range packet:
//   • PACKET head reach AHEAD of the front the shader still lights: width*3 (the
//     smoothstep edge in impulse_segment_activity).
//   • TAIL reach BEHIND the front: the tail term decays as exp(-behind/(width*2.6));
//     beyond ~4 of those it is sub-perceptual. → width*2.6*4.
//   • HEAD_HEADROOM AHEAD of the front so a segment is submitted slightly BEFORE
//     the packet arrives (no late pop-in): width*4. At the long-range speed
//     (0.045/tick) this is 0.06*4 = 0.24 path-units ≈ 5 ticks of lead — ample.
const PACKET_REACH_MUL: f32 = 3.0;
const TAIL_REACH_MUL: f32 = 2.6 * 4.0;
const HEAD_HEADROOM_MUL: f32 = 4.0;

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}
fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

@compute @workgroup_size(64)
fn compact(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= u.segment_count {
        return;
    }
    // Off mode (0) selects nothing; resting structure is hidden by default.
    if u.connection_layer < 1u {
        return;
    }

    let seg = segments[idx];

    // ── Activity owner rule (EXACT mirror of vs_main lines ~488-490) ──────────
    let presynaptic_dendrite = seg.kind == 0u && seg.target_id != seg.neuron_id;
    let activity_id = select(seg.neuron_id, seg.target_id, presynaptic_dendrite);

    // ── Lighting enable mirror (vs_main): glow only when the layer is on AND the
    // relevant directional toggle is set. Presynaptic incoming dendrites light
    // from light_past; everything else from light_next. ──────────────────────
    let light_enabled = u.light_next == 1u || (presynaptic_dendrite && u.light_past == 1u);
    if !light_enabled {
        return;
    }

    let packed = last_spike[activity_id];
    if !has_spiked(packed) {
        return;
    }

    // ── Spike age for packet travel. Glow tau intentionally does NOT cull packet
    // travel; it only controls soma/legacy afterglow in render_morphology.wgsl.
    let age = f32(tick_diff(u.tick, packed & TICK_MASK));

    // ── Packet-localized travel window (mirror of impulse_segment_activity) ───
    // The packet front travels travel = age*speed along the cumulative path. The
    // shader lights a segment only where the head (near the front, ±PACKET_REACH)
    // or the tail (a bounded region behind the front) overlaps the segment's path
    // span [seg_start, seg_end]. We select a segment iff the front is within
    //   [seg_start - HEAD_HEADROOM,  seg_end + TAIL_REACH]
    // i.e. the front is about to reach the segment (headroom ahead), is inside it,
    // or has just passed it (tail behind). This makes selection LOCAL to the
    // moving pulse rather than the whole tree, so it scales with active/recent
    // pulses. Long, fast pulses also self-terminate: once the front overruns the
    // far endpoint by more than TAIL_REACH the segment drops.
    // Per-segment long-range split (EXACT mirror of vs_main: seg.path_len ≥
    // LONG_RANGE_PATH, long-range packet only on axons). Speed AND width pick the
    // long-range regime so the selection window matches the render shader exactly.
    let long_range = seg.kind == 1u && seg.path_len >= LONG_RANGE_PATH;
    let speed = select(
        DENDRITE_ECHO_SPEED,
        select(AXON_IMPULSE_SPEED, LONG_RANGE_IMPULSE_SPEED, long_range),
        seg.kind == 1u,
    );
    let width = select(IMPULSE_WIDTH, LONG_RANGE_IMPULSE_WIDTH, long_range);
    let head_headroom = width * HEAD_HEADROOM_MUL;
    let tail_reach = width * TAIL_REACH_MUL;
    let travel = age * speed;
    let seg_start = seg.path_len;
    let seg_end = seg.path_len + length(seg.b - seg.a);
    if travel < seg_start - head_headroom {
        return; // front has not yet reached this segment (beyond headroom lead)
    }
    if travel > seg_end + tail_reach {
        return; // front passed; tail behind it is sub-perceptual here
    }

    // Selected. Append the chunk-local index (hard cap = segment_count, so no
    // overflow clamp needed — the buffer is sized to this chunk's segments).
    let slot = atomicAdd(&active_count, 1u);
    active_indices[slot] = idx;
}

// Single-thread reset: zero the counter + profiler and prime draw args. Run
// BEFORE `compact` each frame.
@compute @workgroup_size(1)
fn reset() {
    atomicStore(&active_count, 0u);
    atomicStore(&selected_count, 0u);
    draw_args[0] = u.tube_verts; // vertex_count
    draw_args[1] = 0u;           // instance_count (finalized in write_args)
    draw_args[2] = 0u;           // first_vertex
    draw_args[3] = 0u;           // first_instance
}

// Single-thread finalize: copy the atomic counter into the indirect draw args
// instance_count + profiler slot. Run AFTER `compact`.
@compute @workgroup_size(1)
fn write_args() {
    let count = atomicLoad(&active_count);
    draw_args[0] = u.tube_verts;
    draw_args[1] = count;
    draw_args[2] = 0u;
    draw_args[3] = 0u;
    atomicStore(&selected_count, count);
}
