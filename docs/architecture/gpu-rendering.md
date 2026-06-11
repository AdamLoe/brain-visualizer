---
status:        active
owner:         adamg
last_updated:  2026-06-11
---

# GPU Rendering

The rendering subsystem turns the live neuron state (positions, `last_spike` packed words, membrane voltages) into a frame. Its one job is visual fidelity at GPU-only data rates — no CPU readback of per-instance data in the render loop.

## What it owns

- Far-LOD additive billboard glow pass — `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl → vs_main / fs_main`
- GPU compute frustum-cull → `drawIndexedIndirect` path for near-LOD — `crates/brain-visualizer/src/sim/gpu/shaders/frustum_cull.wgsl → cull_neurons / cull_synapses`
- Near-LOD icosphere body pass (present-but-disabled) — `crates/brain-visualizer/src/sim/gpu/shaders/render_sphere.wgsl → vs_main`
- Legacy near-LOD cylinder synapse pass (present-but-disabled) — `crates/brain-visualizer/src/sim/gpu/shaders/render_cylinder.wgsl → vs_main`
- Manifold surface pass — `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl → vs_main / fs_main`
- Active/recent morphology compaction compute (selects only about-to-be-lit / lit / recently-lit segments so the tube passes draw a small subset) — `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl → reset / compact / write_args`
- Procedural morphology tube pass (dendrite + axon branches as shader-generated tubes, spike-keyed lighting, drawn indirect over the compacted set) — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main / fs_main`
- Procedural morphology soma sphere pass (one UV-sphere per neuron, same lighting model) — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_sphere / fs_sphere`
- True-opacity active layer (depth-tested alpha redraw of firing tubes + somas, occludes the additive resting layer) — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → fs_main_active / fs_sphere_active`
- HDR scene buffer + bloom post-process pass — `crates/brain-visualizer/src/sim/gpu/shaders/bloom.wgsl → fs_bright / fs_blur / fs_composite`
- LOD thresholds — `crates/brain-visualizer/src/sim/gpu/mod.rs → LOD_FAR_ONLY_DIST / LOD_NEAR_ONLY_DIST`
- Guards for disabled passes — `crates/brain-visualizer/src/sim/gpu/mod.rs → DRAW_LEGACY_CYLINDERS / DRAW_LEGACY_NEAR_SPHERES / DRAW_LEGACY_RIBBONS`
- Legacy all-segment morphology draw guard (bypasses compaction, draws every generated segment) — `crates/brain-visualizer/src/sim/gpu/pipelines.rs → DRAW_LEGACY_ALL_SEGMENTS` (Rust const + matching WGSL `override` in both `compact_morph_segments.wgsl` and `render_morphology.wgsl`)
- Visual mode enums consumed by the render shaders — `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`
- Render pass order + frame routing logic — `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full`

## What it does NOT own

- Active-edge ribbon emit and render subsystem → [`active-edges.md`](active-edges.md)
- Frame-graph orchestration, pipeline construction, buffer layout — [`gpu-backend.md`](gpu-backend.md)
- What the visual modes mean dynamically (how the sim drives them) → [`simulation.md`](simulation.md)
- Manifold mesh generation (gyri/sulci folding) → [`manifold.md`](manifold.md)

## LOD scheme

Two LOD levels share the screen, with the near path currently disabled in favor of the billboard-everywhere policy (see below).

**Far LOD — additive billboards (always on).** Every neuron is a camera-facing two-triangle quad. Glow is:

```
glow = has_spiked ? exp(-tick_diff / glow_tau) : 0
```

`has_spiked` and `tick_diff` come from the packed `last_spike` word (bit 31 = has-spiked flag, bits 23–0 = tick). The quad radius grows by `active_neuron_radius_boost` scaled by `glow` for active neurons and applies a near-camera scale ramp (`NEAR_RADIUS_DIST` / `NEAR_RADIUS_MAX`) so close-up neurons remain large soft orbs rather than shrinking dots. A Gaussian falloff (`exp(-d²×6)`) in the fragment shader produces the round soft orb shape without `@builtin(point_size)` — see the gotcha below.

Inactive neurons are dimmed or hidden via `inactive_neuron_opacity` and `neuron_visibility`. A `dist_fade` smoothstep (0.05 → 0.6 world units from camera) fades the resting-gray contribution to zero near the camera, preventing a fogged interior when flying inside the cloud.

**Near LOD — icospheres via GPU cull (present, disabled).** Gated behind `DRAW_LEGACY_NEAR_SPHERES = false`. Would render frustum-culled icosphere bodies for close-up neurons using `frustum_cull.wgsl → cull_neurons` and `render_sphere.wgsl → vs_main`. Retired because blocky faceted geometry and the shading terminator band were visually inferior to the billboard-everywhere approach.

**Billboard-everywhere policy.** `DRAW_LEGACY_NEAR_SPHERES = false` keeps `far_alpha = 1.0` always and `run_near_lod = false` always. The billboard near-camera scale ramp compensates for the zoom use case.

## Visual mode enums

These four enums are carried in `VisualSettings` (crates/brain-visualizer/src/sim/gpu/mod.rs), packed into the render uniform, and consumed by `render_far.wgsl → color_for` and `render_morphology.wgsl`:

| Field | Integer values |
|---|---|
| `color_by` | 0=region, 1=E/I, 2=spike-age, 3=voltage-debug, 4=activity, 5=identity, 6=brain |
| `neuron_visibility` | 0=all, 1=active-emphasis, 2=active-only |
| `surface` | 0=off, 1=dim, 2=normal |

`color_by` and `neuron_visibility` are the only two mode fields that materially affect shader output per-frame. `color_by = 5` derives a stable per-neuron hue from the shared BV22 hash (`IDENTITY_SALT` is reserved in `render_far.wgsl` and `render_morphology.wgsl`); far glow uses that hue directly, while morphology blends it with the structural dendrite/axon/soma tint. The old signal-source slot is tombstoned in the TypeScript `Float32Array` as index 16 and is written as `0`; shaders do not consume it. `surface` still gates the optional manifold pass on the CPU side (`draw_surface` guard). Integer values are frozen — the `VisualSettings::from_slice` contract in `crates/brain-visualizer/src/sim/gpu/mod.rs → from_slice` maps them from a flat `Float32Array` shared with TypeScript.

`color_by = 6` is the Brain mode and the clean default. It is a unified product
color branch across the live shaders: resting/inactive visible neurons,
morphology tubes, soma spheres, and optional manifold surface are pink
(`vec3(1.0, 0.18, 0.54)`); firing neuron cores and active morphology packets are
blue (`vec3(0.08, 0.56, 1.0)`) with softer bluish halos where the existing
activity math calls for a halo. Brain mode does not force hidden layers on:
`surface = 0` still skips the optional surface pass, `connection_layer = 0`
still skips morphology, and `neuron_visibility` still controls neuron body
visibility.

## Render pass order

Each call to `GpuBackend::render_full` encodes passes in this order into a single command encoder:

1. **Manifold surface pass** (optional — skipped when `surface == 0`): clears color + depth, draws the static procedural brain shell as a dim fill so the brain shape reads through the glow. `render_manifold.wgsl → fs_main` emits `base * mode_scale * surface_opacity`.
2. **Far-glow pass**: clears (or loads if surface pass ran). Additive blend, no depth write, `Draw(6, N)` — one instance per neuron.
3. **Active/recent compaction compute + morphology tube pass** (both skipped when `connection_layer == 0`): when `connection_layer != 0`, a compute pass first selects the about-to-be-lit / lit / recently-lit segments (`compact_morph_segments.wgsl`), then the tube pass draws **only that compacted subset** indirect via `render_morphology.wgsl → vs_main / fs_main` — additive, no depth. `MorphSegment` storage buffer generated at network build time (`crates/brain-visualizer/src/sim/morphology.rs`). See Morphology pass and Active/recent compaction below.
4. **Active tube pass** (skipped unless `active_opaque_on`): the *same* tubes redrawn alpha-blended + depth-tested via `render_morphology.wgsl → fs_main_active`, so firing tubes occlude. **Owns the frame's depth clear** (`LoadOp::Clear(1.0)`). See Active-opacity layer below.
5. **Morphology soma sphere pass** (skipped when `connection_layer == 0`): additive, no depth. Draws one UV-sphere per neuron via `render_morphology.wgsl → vs_sphere / fs_sphere`. Uses the same `last_spike` and `morph_uniform` buffers as the tube pass (`render_soma_spheres` pipeline, `crates/brain-visualizer/src/sim/gpu/pipelines.rs`).
6. **Active soma pass** (skipped unless `active_opaque_on`): the *same* somas redrawn alpha-blended + depth-tested via `render_morphology.wgsl → fs_sphere_active`. **Loads** (does not clear) the depth the active tube pass wrote, so active tubes and active somas mutually occlude. See Active-opacity layer below.
7. **Near-LOD passes** (skipped when `DRAW_LEGACY_NEAR_SPHERES = false`): cull_neurons compute → (cull_synapses if `DRAW_LEGACY_CYLINDERS`) → write_indirect → sphere render → cylinder render.
8. **Bloom post-process** (skipped when `bloom_strength == 0`): scene is in an HDR offscreen target; bright-pass → separable 9-tap Gaussian blur (half-res ping-pong) → composite with soft-add `1 - exp(-bloom)` to avoid hard clipping. When bloom is off, `scene_view` IS `target_view` — no offscreen indirection whatsoever. Bloom reads color only, never depth, so it composes over the active layer with no bloom-path change.

The two active passes are the **only** writers/readers of depth in the morphology path (the additive passes 2/3/5 use `depth_stencil_attachment: None`). Both are placed immediately after their additive sibling so the additive layer lays down the soft resting glow and the opaque active layer punches solid geometry on top — into the same `scene_view` color target with `LoadOp::Load`, so bloom composes over them unchanged.

## Morphology pass

Procedural neuron geometry (soma body + dendrite tree branches + shared root/cluster branches + terminal twigs) is generated once at network build time. Two separate draw sub-passes cover branches and soma bodies; both are additive, no depth write, and share the same `MorphUniforms` buffer. Each is optionally followed by a depth-tested alpha redraw of the same geometry — see Active-opacity layer below.

The tube sub-passes do **not** draw every generated segment — they draw only the segments that are about-to-be-lit / lit / recently-lit, chosen each frame by the active/recent compaction compute below. The soma sub-passes are per-neuron (not per-segment) and are unaffected by compaction; they are gated only on `connection_layer != 0`.

### Active/recent compaction

When `connection_layer != 0`, a compute pass (`compact_morph_segments.wgsl`, entry points `reset` → `compact` → `write_args`) runs **before** the tube passes and writes the indices of the segments worth drawing this frame into `active_segment_indices`, the count into the atomic `active_segment_count`, and a `DrawIndirectArgs` record into `active_draw_args`. Both tube passes then `draw_indirect` over that set, so the instance count flows through GPU indirect args and the per-frame selection **never touches the CPU**. `render_full` encodes it as reset (1 workgroup) → compact (⌈`segment_count`/64⌉ workgroups) → write_args (1 workgroup), then sets `active_draw_args` on both `pass.draw_indirect` calls (`crates/brain-visualizer/src/sim/gpu/mod.rs → render_full`).

**Selection predicate (mirrors the render shader exactly).** Per segment, the activity owner is `select(neuron_id, target_id, kind==0 && target_id!=neuron_id)` — a source-specific incoming dendrite leaf lights from its presynaptic source (`target_id`), everything else from `neuron_id`. The pass reads `last_spike[owner]`, then keeps the segment only if the moving impulse **packet band** along `path_len` overlaps it: a `HEAD_HEADROOM` lead ahead of the front (`front = age * speed`) so a segment is submitted slightly before it lights, plus a `TAIL_REACH` window behind the front. It is **not** the whole-tree glow lifetime — only the segments under the traveling packet survive. At the low-firing default (N=6000) roughly 0.6% of segments are selected, rising with firing activity. The five pulse-timing constants (`AXON_IMPULSE_SPEED`, `IMPULSE_WIDTH`, `DENDRITE_ECHO_SPEED`, `LONG_RANGE_IMPULSE_SPEED`, `LONG_RANGE_IMPULSE_WIDTH`) and the `LONG_RANGE_PATH` split are mirrored per-segment from `render_morphology.wgsl` so the selection window tracks the wider/faster long-range packet (`HEAD_HEADROOM_MUL = 4`, `TAIL_REACH_MUL = 2.6 * 4` × width).

**Resources** live in `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphBuffers`: `active_segment_indices` (capacity = total segment count), `active_segment_count` (atomic), `active_draw_args` (INDIRECT usage), `compact_uniform` (`CompactUniforms`, 32 B), plus `active_selected` / `selected_staging` for the profiler-only selected-count readback (see [`profiling.md`](profiling.md)). The compute bind group / layout is `compact_morph` / `compact_morph_bgl`; the tube render bind group gained binding 6 (`active_segment_indices`). Pipelines `compact_morph_reset` / `compact_morph` / `compact_morph_write_args` are built in `crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines`.

**Legacy all-segment path.** Setting `crates/brain-visualizer/src/sim/gpu/pipelines.rs → DRAW_LEGACY_ALL_SEGMENTS = true` (Rust const + matching WGSL `override`, default `false`) skips the compaction compute and draws every generated segment via the old `instance_index`-as-segment path.

**connection_layer modes** (`crates/brain-visualizer/src/sim/gpu/mod.rs → render_full` / `set_connection_layer`): `0` = Off — skips compaction, both tube passes, and both soma passes (no morphology work); `1` = Active/recent only (default) — compaction selects the lit packet band; `2` = Resting debug — intended to draw the full resting morphology, which currently requires rebuilding with `DRAW_LEGACY_ALL_SEGMENTS`, else it behaves like mode 1.

**Tube sub-pass (branches).** Branch segments are uploaded as a flat `MorphSegment` array (48 B per segment, branch-only). `render_morphology.wgsl → vs_main` builds a shader-generated tube: `TUBE_SIDES` sides (default 6), two rings (ring 0 at endpoint `a` with `radius_a`, ring 1 at `b` with `radius_b`), triangulated as `TUBE_SIDES * 2 * 3` vertices per instance. A stable per-vertex basis is built from the segment axis with the fallback `abs(axis.y) < 0.9 ? (0,1,0) : (1,0,0)` to avoid degenerate cross products. Open tubes (no end caps) at this scale. `TUBE_SIDES` is a WGSL `override` constant set at pipeline build from the render-quality config (see Render quality below), so the per-instance vertex-count is runtime-driven (`morph_tube_verts`, recomputed from the same tube-sides value via `crates/brain-visualizer/src/sim/gpu/mod.rs → tube_verts` to stay in lockstep with the override). The **instance count is GPU-decided**: both tube passes draw via `pass.draw_indirect(&mb.active_draw_args, 0)` over the compacted active/recent set (see Active/recent compaction below), not one instance per generated segment. `vs_main` remaps `instance_index` through `active_segment_indices[inst]` to fetch the real `MorphSegment` (the legacy `inst`-as-segment path is gated behind `DRAW_LEGACY_ALL_SEGMENTS`). Tube pass bind group bindings: 0 = `segment_buffer`, 1 = `last_spike`, 2 = `morph_uniform`, 6 = `active_segment_indices` (the compacted instance→segment map).

**Soma sphere sub-pass.** A separate `render_soma_spheres` pass draws one `MorphSphereInstance` per neuron (emitted by `crates/brain-visualizer/src/sim/morphology.rs → emit_soma_spheres`, called from `crates/brain-visualizer/src/sim/gpu/resources.rs → init_morph_resources`). Sphere geometry is a low-res UV sphere via `render_morphology.wgsl → vs_sphere / fs_sphere`: `SPHERE_SLICES` × `SPHERE_STACKS` (defaults 8 × 6) → `slices * stacks * 2 * 3` vertices per instance (288 at default), drawn `pass.draw(0..morph_sphere_verts, 0..n_spheres)`. Both are WGSL `override` constants set at pipeline build from the render-quality config (see Render quality below); `morph_sphere_verts` is recomputed from the same values (`crates/brain-visualizer/src/sim/gpu/mod.rs → sphere_verts`) to keep the draw count in lockstep. Soma radius = `params::R0` (the `MorphologyParams` base radius). The instance also carries `root_dir/root_pull` baked from the host-side `ProcessRoot`; `vs_sphere` uses those fields to stretch and shoulder the UV sphere toward the dominant axon root before applying the normal spike pulse scale. Sphere pass bind group bindings: 3 = `sphere_instances`, 4 = `last_spike` (same buffer), 5 = `morph_uniform` (same buffer). Binding slots 3/4/5 avoid WGSL name clashes with tube slots 0/1/2 in the shared shader module — WebGPU validates only bindings reachable per entry point. See [`manifold.md`](manifold.md) for the `MorphSphereInstance` layout and generation contract.

**Render quality.** Tube and sphere tessellation are not baked into the shader as
`const`s — they are WGSL `override` constants (`TUBE_SIDES`, `SPHERE_SLICES`,
`SPHERE_STACKS`) supplied at pipeline-build time from a `RenderQualityConfig`
(`crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines`, via
`compilation_options.constants`). The matching Rust draw vert-counts
(`morph_tube_verts` / `morph_sphere_verts`) are derived from the **same**
`RenderQualityConfig`, so a render-quality change rebuilds the morph pipelines and
recomputes draw counts together — the override and the draw call can never drift.
Render-quality is exposed in the dev panel as a `renderer-rebuild` control group
(see [`dev-panel.md`](dev-panel.md)).

**Lighting.** Both sub-passes share a single lighting model in the fragment stage (implemented by `fs_main` for tubes and `fs_sphere` for somas):

```
lighting = ambient + diffuse_intensity * max(dot(N, L), 0)
         + pow(1 - max(dot(N, V), 0), rim_power) * rim_intensity
```

`N` is the radial ring direction (tubes) or surface direction (spheres); `L` and `V` are light direction and view direction. The lit contribution is scaled by `active_boost` and added on top of a `resting_brightness` floor, giving a tunable resting-vs-active split (resting structure stays subtle; firing structure reveals curvature/lighting without blowing to white). All lighting/brightness values are sourced each frame from the dev-panel-owned `MorphologyConfig` lighting group, whose defaults live in `crates/brain-visualizer/src/sim/morphology.rs → LightingConfig` (NOT hardcoded shader/`mod.rs` constants); `render_full` reads `morph_config.lighting` and re-normalizes the light direction CPU-side (`crates/brain-visualizer/src/sim/gpu/mod.rs → morph_light_dir`). These are `live`/uniform-only controls.

`MorphUniforms` (**192 B**, Rust ↔ WGSL layout locked) carries the lighting fields after `color_by`: `light_dir: vec3<f32>`, `ambient`, `diffuse_intensity`, `rim_intensity`, `rim_power`, the v0.3.1 additions `resting_brightness` and `active_boost` (the latter replaced the former hardcoded WGSL `const BOOST = 1.8`), and the active-layer `active_opacity` / `inactive_opacity_floor` (repurposed in place from the former trailing `_pad4`/`_pad5` reserved slots — type/name change only, no reorder, so the size asserts stay green; read only by the active fragment stages). Size asserts: `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_layouts_locked`. See `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms` for the full field list. The first shared v0.3.2/v0.3.3 pass deliberately kept `MorphUniforms` unchanged; the pulse/material defaults live in shader constants and derive from the existing `tick`, `glow_tau`, `resting_brightness`, and `active_boost` fields.

**Procedural material.** `render_morphology.wgsl` now applies deterministic shader-only material helpers (`tube_material`, `soma_material`) before the lighting multiplier. The inputs are world position, normal, `path_pos`, `kind`, and `neuron_id`; there are no texture assets, no sampler bindings, and no time-varying noise. Tube material uses low-amplitude longitudinal striation plus sheath variation; soma material uses low-frequency membrane mottling plus sparse restrained speckle. The amplitude stays intentionally low so region/E-I/identity colors remain legible and pulse motion still dominates.

**Spike lighting.** Far billboards and morphology soma spheres share the same packed-`last_spike` timing model: `render_far.wgsl` and `render_morphology.wgsl → vs_sphere / fs_sphere` derive a slower `glow` envelope from `exp(-tick_diff/glow_tau)` plus a shorter `flash` term and a very young white-core lift. The far pass uses that timing for body brightness/radius; the morphology sphere pass uses the same timing for soma brightness/radius.

The tube pass is now a traveling packet rather than whole-arbor instant
lighting. `render_morphology.wgsl → vs_main` exports
`path_pos = seg.path_len + t * length(seg.b - seg.a)` (`t = 0/1` at the two tube
rings, then interpolated per fragment). `fs_main` combines that `path_pos` with
the segment's activity owner to build a moving impulse. Axons and shared
dendrite stems read `last_spike[neuron_id]`; source-specific incoming dendrite
leaves are the v1 exception and read `last_spike[target_id]`, where `target_id`
stores the presynaptic source id. Those leaves are emitted socket-to-soma, so
their weaker dendrite packet travels inward from the synapse. Shared aggregate
stems keep `target_id = neuron_id`, so they do not presynaptically pulse in v1.
`light_past` is still **removed from the settings surface** (Float32Array index
9 = `reserved_zero`), so source-specific dendrite leaves use the normal
connection-light enable while selecting the presynaptic activity owner. See
[`../decisions/rendering.md`](../decisions/rendering.md) for the original
upstream-lighting rationale.

**Local vs long-range pulse split.** Pulse timing keys off `MorphSegment.path_len`
(cumulative path distance), so a packet's position is `front = age * speed`. Local
axon segments use `AXON_IMPULSE_SPEED = 0.018` / `IMPULSE_WIDTH = 0.028`; the
dendrite echo uses `DENDRITE_ECHO_SPEED = 0.006`. Axon segments whose cumulative
path passes `LONG_RANGE_PATH = 0.18` switch to a faster, wider regime —
`LONG_RANGE_IMPULSE_SPEED = 0.045` (~2.5×) and `LONG_RANGE_IMPULSE_WIDTH = 0.060`
(~2.1×) — so a single blue packet visibly sweeps a long waypoint-routed projection
instead of the whole fiber blinking. Classification is per-segment and
deterministic (`seg.path_len >= LONG_RANGE_PATH`, axon only). All six constants
live in `render_morphology.wgsl` and are mirrored byte-for-meaning in
`compact_morph_segments.wgsl` so the compaction selection window tracks the same
packet. No new uniform field was needed — `MorphUniforms` stays 192 B.

In Brain mode, the same activity scalars drive hue rather than changing the
activity contract: inactive material starts pink, the traveling packet mixes
toward active blue, and soma firing cores go blue while non-firing soma material
stays pink. No new firing-state buffer or layout field was added for this mode.

**Layout contracts.** `MorphSegment` (48 B, branch-only) field order must stay byte-identical between `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment` and `render_morphology.wgsl`. `MorphSphereInstance` (48 B, soma-only) must stay byte-identical with its WGSL counterpart. `MorphUniforms` (192 B) must match `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms`. Reordering fields in any of these is the primary corruption source; size asserts guard all three — `crates/brain-visualizer/src/sim/morphology.rs → segment_layout_is_48_bytes / sphere_instance_layout_is_48_bytes` and `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_layouts_locked`.

## Active-opacity layer

The additive morphology passes can only ever make geometry *brighter*, never solid — additive blend cannot occlude. To let **firing** geometry read as genuinely opaque, two extra depth-tested, alpha-blended passes redraw the same tubes and somas on top of their additive siblings: the active tube pass (`render_morphology.wgsl → fs_main_active`, `render_morphology_active` pipeline) and the active soma pass (`fs_sphere_active`, `render_soma_spheres_active` pipeline). "Active" here means **firing** — alpha is keyed off the same `last_spike` recency the additive passes use, NOT click-picking. See [`../decisions/rendering.md`](../decisions/rendering.md) for why active-rather-than-selected and true-opacity-rather-than-brighter-additive.

Both active pipelines use `wgpu::BlendState::ALPHA_BLENDING` with `depth_write_enabled` + `depth_compare: Less` (`crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines`). They reuse the additive sibling's bind group, draw count, and override constants verbatim — no new buffers or bind groups. They render into the same `scene_view` with `LoadOp::Load`, so bloom composes over them unchanged.

**Depth ownership.** The active passes are the only depth users in the morphology path, so the active tube pass clears depth (`LoadOp::Clear(1.0)`) and the active soma pass loads it — giving correct active-tube/active-soma mutual occlusion. They cannot borrow the surface pass's depth clear: the surface pass is off by default (`surface == 0`) and near-LOD is permanently off, so the active layer owns its own clear. Self-occlusion within the additive resting layer stays deferred — only the active layer is depth-correct, which is the intent.

**Opacity model.** `fs_main_active` shades color from the same fragment-local
traveling-packet activity as the additive tube pass, but its alpha is driven by
a continuous packet-proximity factor computed over the segment's path interval.
Alpha moves smoothly from `inactive_opacity_floor` to an active ceiling as the
packet approaches and crosses the segment; brightness remains fragment-local, so
the bright packet still travels through a temporarily more opaque segment rather
than turning the whole arbor into a flash. `fs_sphere_active` uses the same
ceiling/floor model with soma-local activity (`glow + flash + core`). Fragments
below `active_alpha < 0.004` `discard`, so fully inactive fragments write
neither color nor depth. `active_opacity` (requested ceiling, default `1.0`) and
`inactive_opacity_floor` (floor, default `0.0` = resting structure fully hidden
in the opaque layer, still shown softly by the additive layer) live in the
dev-panel-owned `LightingConfig`
(`crates/brain-visualizer/src/sim/morphology.rs → LightingConfig`), fed through
the two repurposed trailing `MorphUniforms` fields (see Morphology pass) — NOT
the `VisualSettings` Float32Array.

**Low-end opacity.** `crates/brain-visualizer/src/sim/gpu/mod.rs → render_full`
keeps `active_opaque_on` tied to the morphology layer and active-pipeline
availability, not to the requested opacity value. The shader maps
`active_opacity = 0` to a soft low-emphasis ceiling of `0.10` (clamped above
`inactive_opacity_floor`) for both tubes and somas, so the active layer still
contributes depth-tested damping at the slider's low end instead of dropping the
occluding redraw.

## Legacy near-LOD cylinder pass

Permanently off (`DRAW_LEGACY_CYLINDERS = false`); the morphology pass is the live connection visual. See `render_cylinder.wgsl → vs_main` to revive.

## Bloom / HDR path

When `bloom_strength > 0` the scene renders into an `rgba16float` offscreen texture (`hdr_view`). The bloom pipeline is: bright-pass (luminance threshold `BLOOM_THRESHOLD = 0.55`) → horizontal blur → vertical blur at half resolution → composite. The composite uses `1 - exp(-bloom * strength)` rather than plain addition to roll off smoothly near 1.0. The additive scene passes cooperate naturally with bloom because additive energy accumulates in the HDR buffer without clamping before the post pass.

When `bloom_strength == 0`, `scene_view = target_view` — the validated direct path — with zero overhead and bit-for-bit identical output to the pre-bloom baseline. The shipped v0.2.1 default is `0.4`, so the HDR path is now enabled unless a caller explicitly turns bloom off.

## Key gotcha: no portable point size in WebGPU

`@builtin(point_size)` is not portable in WGSL/WebGPU. Do not use it. All neuron "glow dots" are camera-facing quad billboards (`Draw(6, N)`) with a Gaussian falloff in the fragment shader. This is the only way to get a variable-radius soft glow that works across all WebGPU backends. The `render_far.wgsl` comment documents this constraint explicitly.

## Offline validation

`crates/brain-visualizer/examples/render_check.rs` drives the full production GPU pipeline on llvmpipe (offscreen 512×512), warms the sim for 300 ticks, and asserts pixel-level correctness: not all-black, distinct region colors present, stimulate produces measurable activity increase. It also proves the active-opacity layer: low, small-positive, and high active-opacity frames differ measurably, showing that the active redraw remains encoded at the low end and that shader alpha is continuous rather than a binary cliff. `crates/brain-visualizer/examples/near_lod_check.rs` exercises the frustum-cull → indirect draw path at close camera distance. Both validate all shaders via Naga at pipeline-build time (panics on any WGSL error).

The visual-product-polish consolidated gate used `morph_view` and
`render_check` as offline render evidence because the browser environment could
not obtain a real WebGPU adapter: Chromium `requestAdapter()` returned `null`
and the app fell back to the clear-only WebGL2/black-canvas path. The offline
artifacts were nonblank and showed Brain mode (`color_by = 6`, `surface = 0`),
pink resting structure, blue/cyan active segments, close dendrite branching,
and zero morphology drops. A real-WebGPU browser nonblank smoke remains the
environment-dependent verification gap.

## Update when

- A new render pass is added or the pass order changes.
- The active/recent compaction predicate or its mirrored pulse constants change (`compact_morph_segments.wgsl` must stay in lockstep with `render_morphology.wgsl`), or `DRAW_LEGACY_ALL_SEGMENTS` is flipped default-on, or a `connection_layer` mode changes meaning.
- The local-vs-long-range pulse split constants change (`AXON_IMPULSE_SPEED` / `IMPULSE_WIDTH` / `DENDRITE_ECHO_SPEED` / `LONG_RANGE_IMPULSE_SPEED` / `LONG_RANGE_IMPULSE_WIDTH` / `LONG_RANGE_PATH`) in either shader.
- `VisualSettings` adds a new field that the render shaders consume (update the mode tables above).
- `MorphSegment` layout changes in `crates/brain-visualizer/src/sim/morphology.rs` (immediately breaks `render_morphology.wgsl`).
- `MorphSphereInstance` layout changes in `crates/brain-visualizer/src/sim/morphology.rs` (breaks the sphere sub-pass).
- `MorphUniforms` layout changes in `crates/brain-visualizer/src/sim/gpu/resources.rs` (breaks both sub-passes; update Rust + WGSL atomically — currently 192 B; the trailing `active_opacity` / `inactive_opacity_floor` are repurposed pads, free for any future no-size-change knob).
- The active-opacity layer changes (`active_opaque_on` guard, the two active passes' depth attachments / clear-vs-load, the `fs_*_active` alpha model, or the `LightingConfig` opacity knobs).
- The render-quality override mechanism changes (`TUBE_SIDES` / `SPHERE_SLICES` / `SPHERE_STACKS` override consts, `build_morph_pipelines`, or the `tube_verts` / `sphere_verts` draw-count derivation), or the lighting source moves off `MorphologyConfig`/`LightingConfig`.
- The bloom pipeline changes (bright-pass threshold, blur kernel, composite formula).
- `DRAW_LEGACY_NEAR_SPHERES` or `DRAW_LEGACY_CYLINDERS` is flipped to `true` permanently.
- The LOD threshold constants change.

## See also

- [`active-edges.md`](active-edges.md) — active-edge ribbon emit and render (the retired predecessor; code gated behind `DRAW_LEGACY_RIBBONS`)
- [`gpu-backend.md`](gpu-backend.md) — pipeline construction, bind group layout, frame-graph orchestration
- [`simulation.md`](simulation.md) — what the visual modes mean in terms of sim dynamics
- [`manifold.md`](manifold.md) — manifold mesh generation (gyri/sulci)
- [`profiling.md`](profiling.md) — near-LOD profiler counters, bloom timing, metrics readback
- [`../decisions/rendering.md`](../decisions/rendering.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
