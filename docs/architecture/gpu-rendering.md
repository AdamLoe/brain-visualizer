---
status:        active
owner:         adamg
last_updated:  2026-06-04
---

# GPU Rendering

The rendering subsystem turns the live neuron state (positions, `last_spike` packed words, membrane voltages) into a frame. Its one job is visual fidelity at GPU-only data rates — no CPU readback of per-instance data in the render loop.

## What it owns

- Far-LOD additive billboard glow pass — `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl → vs_main / fs_main`
- GPU compute frustum-cull → `drawIndexedIndirect` path for near-LOD — `crates/brain-visualizer/src/sim/gpu/shaders/frustum_cull.wgsl → cull_neurons / cull_synapses`
- Near-LOD icosphere body pass (present-but-disabled) — `crates/brain-visualizer/src/sim/gpu/shaders/render_sphere.wgsl → vs_main`
- Legacy near-LOD cylinder synapse pass (present-but-disabled) — `crates/brain-visualizer/src/sim/gpu/shaders/render_cylinder.wgsl → vs_main`
- Manifold surface pass — `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl → vs_main / fs_main`
- Procedural morphology pass (soma + dendrite tree + axon arbor with spike-keyed connection lighting) — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main / fs_main`
- HDR scene buffer + bloom post-process pass — `crates/brain-visualizer/src/sim/gpu/shaders/bloom.wgsl → fs_bright / fs_blur / fs_composite`
- LOD thresholds — `crates/brain-visualizer/src/sim/gpu/mod.rs → LOD_FAR_ONLY_DIST / LOD_NEAR_ONLY_DIST`
- Guards for disabled passes — `crates/brain-visualizer/src/sim/gpu/mod.rs → DRAW_LEGACY_CYLINDERS / DRAW_LEGACY_NEAR_SPHERES / DRAW_LEGACY_RIBBONS`
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
| `color_by` | 0=region, 1=E/I, 2=spike-age, 3=voltage-debug, 4=activity |
| `neuron_visibility` | 0=all, 1=active-emphasis, 2=active-only |
| `signal_source` | 0=spike, 1=voltage, 2=activity |
| `surface` | 0=off, 1=dim, 2=normal |

`color_by` and `neuron_visibility` are the only two that materially affect the shader output per-frame. `signal_source` and `surface` gate entire passes on the CPU side (`draw_surface` guard; morphology pass consults `connection_layer` not `signal_source` directly). Integer values are frozen — the `VisualSettings::from_slice` contract in `crates/brain-visualizer/src/sim/gpu/mod.rs → from_slice` maps them from a flat `Float32Array` shared with TypeScript.

## Render pass order

Each call to `GpuBackend::render_full` encodes passes in this order into a single command encoder:

1. **Manifold surface pass** (optional — skipped when `surface == 0`): clears color + depth, draws the static folded brain mesh as a dim fill so the brain shape reads through the glow. `render_manifold.wgsl → fs_main` emits `base * mode_scale * surface_opacity`.
2. **Far-glow pass**: clears (or loads if surface pass ran). Additive blend, no depth write, `Draw(6, N)` — one instance per neuron.
3. **Morphology pass** (skipped when `connection_layer == 0`): additive, no depth. Draws procedural dendrite + axon segments per neuron; when a neuron fires, its connections light instantly and fade with the same `exp(-tick_diff/glow_tau)` curve as the far-glow dot (see Morphology pass below). `render_morphology.wgsl → vs_main` reads `MorphSegment` from a storage buffer generated at network build time (`crates/brain-visualizer/src/sim/morphology.rs`).
4. **Near-LOD passes** (skipped when `DRAW_LEGACY_NEAR_SPHERES = false`): cull_neurons compute → (cull_synapses if `DRAW_LEGACY_CYLINDERS`) → write_indirect → sphere render → cylinder render.
5. **Bloom post-process** (skipped when `bloom_strength == 0`): scene is in an HDR offscreen target; bright-pass → separable 9-tap Gaussian blur (half-res ping-pong) → composite with soft-add `1 - exp(-bloom)` to avoid hard clipping. When bloom is off, `scene_view` IS `target_view` — no offscreen indirection whatsoever.

## Morphology pass

Procedural neuron geometry (soma radius + dendrite tree branches + axon arbor with a baked bow) is generated once at network build time and uploaded as a flat `MorphSegment` array. Each axon segment carries both its SOURCE `neuron_id` and its synaptic `target_id` (the destination neuron); dendrites set `target_id = neuron_id` (self, unused). `kind 0` = dendrite (cool dim tint); `kind 1` = axon (E/I or region tinted). `connection_curve_lift` is baked into the axon bow geometry at generation time and triggers `regenerate_morphology` on change — see `crates/brain-visualizer/src/sim/gpu/mod.rs → set_visual_settings`. All K outgoing connections are drawn per neuron (one axon arbor per synaptic target), so the lit segments match real synapses — see [`manifold.md`](manifold.md) for the generation rule.

**Whole-connection spike lighting.** There is no traveling pulse. When a neuron fires its connections light *instantly* and fade with the **same** `exp(-tick_diff/glow_tau)` curve as the far-glow neuron dot. Two independent toggles, both fed via `MorphUniforms` (`light_next`, `light_past`, `glow_tau`) and combined as a `max`:

- `light_next` (downstream, default ON): a segment lights when its SOURCE neuron (`neuron_id`) fires — a firing neuron's own structure and outgoing connections.
- `light_past` (upstream, default OFF, **axon only**): a segment lights when its TARGET neuron (`target_id`) fires — an axon glows when the neuron it drives spikes. Dendrites carry `target_id = self` and never respond to `light_past`.

The shader reads `last_spike[neuron_id]` (and `last_spike[target_id]` for upstream) each frame; `path_len` is retained in the struct but no longer drives timing. See `render_morphology.wgsl → vs_main` for the brightness model and [`../decisions/rendering.md`](../decisions/rendering.md) for the rationale.

The critical layout contracts: `MorphSegment` (48 B) field order in the WGSL struct must stay byte-identical to `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment`, and `MorphUniforms` (144 B) must match `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms`. Reordering fields in either is the primary corruption source; size asserts guard both.

## Legacy near-LOD cylinder pass

Permanently off (`DRAW_LEGACY_CYLINDERS = false`); the morphology pass is the live connection visual. See `render_cylinder.wgsl → vs_main` to revive.

## Bloom / HDR path

When `bloom_strength > 0` the scene renders into an `rgba16float` offscreen texture (`hdr_view`). The bloom pipeline is: bright-pass (luminance threshold `BLOOM_THRESHOLD = 0.55`) → horizontal blur → vertical blur at half resolution → composite. The composite uses `1 - exp(-bloom * strength)` rather than plain addition to roll off smoothly near 1.0. The additive scene passes cooperate naturally with bloom because additive energy accumulates in the HDR buffer without clamping before the post pass.

When `bloom_strength == 0` (the default), `scene_view = target_view` — the validated direct path — with zero overhead and bit-for-bit identical output to the pre-bloom baseline.

## Key gotcha: no portable point size in WebGPU

`@builtin(point_size)` is not portable in WGSL/WebGPU. Do not use it. All neuron "glow dots" are camera-facing quad billboards (`Draw(6, N)`) with a Gaussian falloff in the fragment shader. This is the only way to get a variable-radius soft glow that works across all WebGPU backends. The `render_far.wgsl` comment documents this constraint explicitly.

## Offline validation

`crates/brain-visualizer/examples/render_check.rs` drives the full production GPU pipeline on llvmpipe (offscreen 512×512), warms the sim for 300 ticks, and asserts pixel-level correctness: not all-black, distinct region colors present, stimulate produces measurable activity increase. `crates/brain-visualizer/examples/near_lod_check.rs` exercises the frustum-cull → indirect draw path at close camera distance. Both validate all shaders via Naga at pipeline-build time (panics on any WGSL error).

## Update when

- A new render pass is added or the pass order changes.
- `VisualSettings` adds a new field that the render shaders consume (update the mode tables above).
- `MorphSegment` layout changes in `crates/brain-visualizer/src/sim/morphology.rs` (immediately breaks `render_morphology.wgsl`).
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
