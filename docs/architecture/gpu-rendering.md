---
status:        active
owner:         adamg
last_updated:  2026-06-15
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
   dim brain shell.
2. **Far billboard glow** for every neuron; additive, no depth, with the
   close-camera radius ramping in `render_far.wgsl`.
3. **Active/recent compaction** when `connection_layer != 0`; each morphology
   segment chunk writes `active_segment_indices` and `active_draw_args`.
4. **Morphology tubes** when `connection_layer != 0`; additive, no depth,
   drawn only through each chunk's GPU-written indirect args.
5. **Active tube redraw** when the active pipelines exist; depth-tested alpha
   over the additive tube pass.
6. **Soma spheres** when `connection_layer != 0`; additive, one shader-built
   sphere per neuron.
7. **Active soma redraw**; depth-tested alpha, loading the active-tube depth.
8. **Bloom** only when `bloom_strength > 0`; the app settings tombstone that
   Float32Array slot to zero, but examples can still call
   `GpuBackend::set_bloom_strength` to validate the retained path.

If compaction is unavailable, tube passes do not fall back to a full-segment
draw. The shipped tube path is the compacted active/recent path only.

## Modes And Settings

`connection_layer` has two active meanings: off, or active/recent morphology.
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

## Morphology Rendering

Morphology geometry is generated at network build time and uploaded as chunked
`MorphSegment` storage plus one `MorphSphereInstance` per neuron. The tube pass
uses `active_segment_indices[instance_index]` to map compacted instances back to
chunk-local segments; there is no `instance_index == segment_index` debug path.

The CPU generation of this geometry (`morphology::generate_with_progress`) is the
heavy "Prepare network payload" boot phase and now reports continuous
sub-progress + `MorphologyTimings` to the boot overlay — see
[`manifold.md`](manifold.md#neuron-morphology-geometry) and
[`web-frontend.md`](web-frontend.md) for the per-phase ms and the
`window.__bvBootTimings` / stall-watchdog observability.

The compaction predicate mirrors the shader's traveling-packet activity. It
keeps only segments whose packet band is about to light, lit, or recently lit,
then writes a chunk-local `DrawIndirectArgs`. Both additive and active tube
passes use the same indirect args, so per-frame selected counts stay GPU-side.
`GpuBackend::read_active_segment_count` is diagnostics-only.

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

Tessellation is controlled by WGSL override constants (`TUBE_SIDES`,
`SPHERE_SLICES`, `SPHERE_STACKS`) supplied from `RenderQualityConfig` in
`crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines`.
The Rust draw vertex counts are derived from the same config in
`GpuBackend`.

## Active Opacity

Active tubes and somas redraw the same geometry with alpha blending and depth
testing. "Active" means spike-keyed firing, not click selection. The active
tube pass clears depth; the active soma pass loads that depth so active tubes
and somas mutually occlude. The additive resting layer remains additive/no-depth.

`active_opacity` and `inactive_opacity_floor` live in
`crates/brain-visualizer/src/sim/morphology.rs → LightingConfig` and ride the
existing 192 B `MorphUniforms` layout. `active_opacity = 0` still encodes a
soft low-emphasis active layer; it does not skip the active redraw.

## Bloom

Bloom is retained as an internal render path. When enabled through
`GpuBackend::set_bloom_strength`, the scene renders into an HDR target, then
bright-pass, horizontal blur, vertical blur, and composite passes run. Normal
web settings write bloom strength as zero, so product frames use the direct
target path.

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
