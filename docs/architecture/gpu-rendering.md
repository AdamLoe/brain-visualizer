---
status:        active
owner:         adamg
last_updated:  2026-06-20
---

# GPU Rendering

The renderer turns GPU-resident neuron state into a frame with no CPU readback
for per-instance draw sizing. The live visual stack is: optional manifold
surface, additive neuron billboards, active/recent procedural morphology,
depth-tested active morphology, and optional internal bloom.

## What It Owns

- Far additive billboard pass —
  `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl → vs_main / fs_main`
- Manifold surface pass —
  `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl → vs_main / fs_main`
- Active/recent morphology compaction —
  `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl → reset / compact / write_args`
- Procedural morphology tubes, soma spheres, and active-opacity shaders —
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- Internal bloom post-process —
  `crates/brain-visualizer/src/sim/gpu/shaders/bloom.wgsl → fs_bright / fs_blur / fs_composite`
- Render pass order and settings consumption —
  `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full, VisualSettings`

The retired ribbon and close-body branches are gone. Git history is the archive;
current docs must not describe those as dormant runtime surfaces.

## Pass Order

`GpuBackend::render_full` encodes these passes into one command encoder:

1. **Manifold surface** when `surface != 0`; clears color/depth and draws the
   dim brain shell. Accepted defaults write `surface = 1`, so the first healthy
   frame is visibly brain-shaped even before the calm default dynamics produce
   much spike activity.
2. **Far billboard glow** for every neuron; additive, no depth, with the
   close-camera radius ramping in `render_far.wgsl`.
3. **Active/recent compaction** when `connection_layer != 0`; each morphology
   segment chunk writes `active_segment_indices` and `active_draw_args`.
4. **Morphology tubes** when `connection_layer != 0`; additive, no depth,
   drawn only through each chunk's GPU-written indirect args.
5. **Soma spheres** when `connection_layer != 0`; additive, one shader-built
   sphere per neuron. The sphere is a bold cell body (`emit_soma_spheres` scales
   the radius by `params::SOMA_RADIUS_FRACTION`), and `vs_sphere` adds the firing
   pulse on top so a firing soma visibly swells. A firing soma reads as a glowing
   light source, not a flat white disc: `render_morphology.wgsl →
   soma_firing_emission` adds a two-lobe radial Gaussian (tight near-white-hot
   core + wider firing-coloured halo) keyed to the screen-radial distance from the
   sphere centre, with the core emitted in HDR above the bloom knee so pass 8
   blurs it into a halo (see `decisions/rendering.md`).
6. **Active tube redraw** when the active pipelines exist; depth-tested alpha
   over both additive morphology passes.
7. **Active soma redraw**; depth-tested alpha, loading the active-tube depth.
8. **Bloom** only when `bloom_strength > 0`; the app settings tombstone that
   Float32Array slot to zero, but examples can still call
   `GpuBackend::set_bloom_strength` to validate the retained path.

If compaction is unavailable, tube passes do not fall back to a full-segment
draw. The shipped tube path is the compacted active/recent path only.

## Modes And Settings

`connection_layer` has three meanings: off, active/recent morphology, or
visible-until-arrival morphology.
Persisted and direct values normalize at both boundaries through
`web/src/core/settings.ts → normalizeConnectionLayer, toFloat32Array` and
`crates/brain-visualizer/src/sim/gpu/mod.rs → normalize_connection_layer,
VisualSettings::from_slice`. The locked index contract is gated by `npm test`
(`web/src/core/settings-contract.test.ts`) and `cargo test`
(`visual_settings_from_slice_maps_locked_indices`); do not renumber settings
slots.

Other render mode fields are carried in `VisualSettings`. The authoritative
option lists/defaults live in `web/src/core/settings.ts → DEFAULT_SETTINGS` and
`web/src/ui/dev-panel.ts → COLOR_BY_OPTIONS`; Rust consumes the packed snapshot
through `VisualSettings::from_slice`. Tombstoned Float32Array slots stay in
place and are written by the web settings boundary.

`color_by` is read UNCLAMPED from Float32Array index 18, so new modes are purely
additive (no `SETTINGS_LENGTH` change, no uniform repad). **Brain** (6) and
**Brain 2** (7) are the themed activity languages spanning all three color
shaders; Brain 2 reads near-black background, blue at rest, red where the
per-fragment `activity = legacy + packet_flow` signal (and soma glow/flash/core)
fires, reusing that existing signal rather than a new buffer. See
[`../decisions/rendering.md`](../decisions/rendering.md) for the themed-mode
rationale.

## Morphology Rendering

Morphology geometry is generated at network build time and uploaded as chunked
`MorphSegment` storage plus one `MorphSphereInstance` per neuron. The tube pass
uses `active_segment_indices[instance_index]` to map compacted instances back to
chunk-local segments; there is no `instance_index == segment_index` debug path.
Each selected segment is expanded by `render_morphology.wgsl → vs_main` into a
curved multi-ring tube. The ring-count bend is derived deterministically from
existing segment fields, so it changes only the render primitive and draw vertex
count, not the Rust/WGSL storage layout or compaction predicate.

Axon impulse emphasis is weighted by the same downstream synaptic-flow signal
that shapes the baked axon tree: generator radii encode subtree synaptic weight
(`sqrt(subtree_weight / total_weight)` for internal branches, terminal twig
floor for leaves, full `r_trunk` pinned on the soma-root/first-fork trunk), and
the shader derives `flow_strength` from the interpolated
unscaled radius. As an impulse splits, child branches are already physically
smaller and their packet brightness/tint/active opacity fade with the carried
flow. The renderer does not bind `i_current` / `I_next` for this because those
buffers are target-neuron accumulated current, not per-branch flow.

The CPU generation of this geometry (`morphology::generate_with_progress`) is the
heavy "Prepare network payload" boot phase and now reports continuous
sub-progress + `MorphologyTimings` to the boot overlay — see
[`manifold.md`](manifold.md#neuron-morphology-geometry) and
[`web-frontend.md`](web-frontend.md) for the per-phase ms and the
`window.__bvBootTimings` / stall-watchdog observability.

The compaction predicate mirrors the shader's traveling-packet activity. Tube
impulse age comes from the morphology-only `visual_spike` buffer, not directly
from the physics `last_spike` buffer. `integrate.wgsl` updates `last_spike` on
every real firing for simulation/metrics, but only starts a new `visual_spike`
packet when the previous visual packet has had enough ticks to traverse the
generated axon fanout. This prevents high-frequency source neurons from
constantly resetting the visible packet near the soma before it reaches the
generated outgoing leaves. In the default active/recent mode, compaction keeps only
segments whose packet band is about to light, lit, or recently lit. In
visible-until-arrival mode (the fresh-state default), every segment owned by a
recent visual spike stays selected for the whole `28 + arrival_hold_ticks`
lifetime; non-packet fragments render as subdued resting structure rather than
lit signal. That subdued branch does not pop out at the compaction drop point —
the render shader **fades it out** over the `[28 .. 28+hold]` window
(`render_morphology.wgsl → arrival_fade_factor`): the mode-2 resting brightness
(in both `fs_main` and `fs_main_active`) and the mode-2 opacity floor (in
`fs_main_active`) ramp from the subdued rest value to zero, then the segment
drops as compaction stops selecting it. The fade is render-only (compaction
selection is unchanged) and applies only when `connection_layer >= 2`, so the
off / active-recent paths are byte-identical. `arrival_hold_ticks` reaches render
through the repurposed `_pad_a` slot of `MorphUniforms` (the same value
`CompactUniforms` carries).

The `reveal_on_arrival` boolean is an opt-in mode-2 sub-option (default off,
ignored in modes 0/1). When on, a segment is **hard front-gated**: hidden until
the impulse front reaches its start
(`impulse_travel(arrival_age) >= segment_start`), so the fired arbor draws in
along the travelling pulse instead of appearing at once. The gate
(`render_morphology.wgsl → reveal_gated`, in both `fs_main` and `fs_main_active`)
zeroes the resting brightness, selection alpha floor, and inactive opacity floor
before arrival; after reveal the segment follows the arrival-hold + fade path
unchanged. It is render-only — compaction still selects the whole fired arbor and
has no `reveal_on_arrival` field, so the geometry is held ready and revealed
without a recompute. Both modes write chunk-local `DrawIndirectArgs`, and
both additive and active tube passes use the same indirect args, so per-frame
selected counts stay GPU-side. `GpuBackend::read_active_segment_count` is
diagnostics-only.

The layout contracts are the corruption-sensitive part:

- `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment` ↔
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  MorphSegment` is branch-only and 48 B, gated by `cargo test` through
  `segment_layout_is_48_bytes`.
- `crates/brain-visualizer/src/sim/morphology.rs → MorphSphereInstance` ↔
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  SphereInstance` is soma-only and 48 B, gated by `cargo test` through
  `sphere_instance_layout_is_48_bytes`.
- `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms` ↔
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  MorphUniforms` is 192 B, gated by `cargo test` through `morph_layouts_locked`.
- Morphology tube timing binds both `last_spike` (type/color metadata) and
  `visual_spike` (packet age). `compact_morph_segments.wgsl` and
  `render_morphology.wgsl` must use the same visual clock for `activity_id`.

Tessellation is controlled by WGSL override constants (`TUBE_SIDES`,
`SPHERE_SLICES`, `SPHERE_STACKS`) supplied from `RenderQualityConfig` in
`crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines`,
plus the fixed tube ring count in `render_morphology.wgsl`. The Rust draw vertex
counts are derived from the same side count and fixed ring count in `GpuBackend`,
then written into the compaction draw args.

## Active Opacity

Active tubes and somas redraw the same geometry with alpha blending and depth
testing. "Active" means spike-keyed firing, not click selection. The additive
tube and soma passes run first; the active tube pass then clears the active
depth target, and the active soma pass loads that depth so active tubes and
somas mutually occlude. Tube alpha is continuous from the configured inactive
floor toward the active ceiling using the same spike-packet proximity that
drives lighting. The subdued/inactive look is carried by brightness, tint, and
low alpha while the additive resting layer remains additive/no-depth behind the
active redraw.

The legacy `active_opacity` and `inactive_opacity_floor` field names live in
`crates/brain-visualizer/src/sim/morphology.rs → LightingConfig` and ride the
existing 192 B `MorphUniforms` layout (repurposed from the former trailing
`_pad4`/`_pad5`). They now act as coverage/emphasis inputs for the solid redraw
rather than allowing see-through tube fragments. `arrival_hold_ticks` (the
until-arrival fade duration) rides the same trick — it repurposes the former
`_pad_a` slot (`u32`→`f32` in place). `reveal_on_arrival` likewise repurposes the
former `_pad_b` slot (kept as `u32`), so the 192 B layout and its
`morph_layouts_locked` assert are unchanged. See
`crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms` for the field
order.

## Bloom

Bloom is retained as an internal render path. Normal web settings write bloom
strength as zero, so product frames use the direct target path and do not
allocate the HDR/blur textures. When enabled through
`GpuBackend::set_bloom_strength`, the first bloom-ready render allocates the
HDR scene target and half-resolution blur ping-pong targets, then bright-pass,
horizontal blur, vertical blur, and composite passes run.

## Validation

`crates/brain-visualizer/examples/render_check.rs` is the offline production
render smoke. It validates nonblack output, region colors, stimulation,
morphology, active opacity, bloom-on, bloom-off, and chunked compaction under
llvmpipe.

## Update When

- Render pass order or render resources change.
- The compaction predicate or packet timing constants change in either
  `compact_morph_segments.wgsl` or `render_morphology.wgsl`.
- `VisualSettings`, `MorphSegment`, `MorphSphereInstance`, or `MorphUniforms`
  layout contracts change.
- The active-opacity depth/alpha model changes.
- Bloom routing or shader behavior changes.

## See Also

- [`active-edges.md`](active-edges.md) — retired ribbon subsystem status
- [`gpu-backend.md`](gpu-backend.md) — frame graph and resource lifecycle
- [`manifold.md`](manifold.md) — morphology generation and layouts
- [`profiling.md`](profiling.md) — metrics and diagnostics readback
- [`../decisions/rendering.md`](../decisions/rendering.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
