---
status:        active
owner:         adamg
last_updated:  2026-06-06
---

# GPU Rendering

The rendering subsystem turns the live neuron state (positions, `last_spike` packed words, membrane voltages) into a frame. Its one job is visual fidelity at GPU-only data rates — no CPU readback of per-instance data in the render loop.

## What it owns

- Far-LOD additive billboard glow pass — `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl → vs_main / fs_main`
- GPU compute frustum-cull → `drawIndexedIndirect` path for near-LOD — `crates/brain-visualizer/src/sim/gpu/shaders/frustum_cull.wgsl → cull_neurons / cull_synapses`
- Near-LOD icosphere body pass (present-but-disabled) — `crates/brain-visualizer/src/sim/gpu/shaders/render_sphere.wgsl → vs_main`
- Legacy near-LOD cylinder synapse pass (present-but-disabled) — `crates/brain-visualizer/src/sim/gpu/shaders/render_cylinder.wgsl → vs_main`
- Manifold surface pass — `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl → vs_main / fs_main`
- Procedural morphology tube pass (dendrite + axon branches as shader-generated tubes, spike-keyed lighting) — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main / fs_main`
- Procedural morphology soma sphere pass (one UV-sphere per neuron, same lighting model) — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_sphere / fs_sphere`
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
| `color_by` | 0=region, 1=E/I, 2=spike-age, 3=voltage-debug, 4=activity, 5=identity |
| `neuron_visibility` | 0=all, 1=active-emphasis, 2=active-only |
| `signal_source` | 0=spike, 1=voltage, 2=activity |
| `surface` | 0=off, 1=dim, 2=normal |

`color_by` and `neuron_visibility` are the only two that materially affect the shader output per-frame. `color_by = 5` derives a stable per-neuron hue from the shared BV22 hash (`IDENTITY_SALT` is reserved in `render_far.wgsl` and `render_morphology.wgsl`); far glow uses that hue directly, while morphology blends it with the structural dendrite/axon/soma tint. `signal_source` and `surface` gate entire passes on the CPU side (`draw_surface` guard; morphology pass consults `connection_layer` not `signal_source` directly). Integer values are frozen — the `VisualSettings::from_slice` contract in `crates/brain-visualizer/src/sim/gpu/mod.rs → from_slice` maps them from a flat `Float32Array` shared with TypeScript.

## Render pass order

Each call to `GpuBackend::render_full` encodes passes in this order into a single command encoder:

1. **Manifold surface pass** (optional — skipped when `surface == 0`): clears color + depth, draws the static folded brain mesh as a dim fill so the brain shape reads through the glow. `render_manifold.wgsl → fs_main` emits `base * mode_scale * surface_opacity`.
2. **Far-glow pass**: clears (or loads if surface pass ran). Additive blend, no depth write, `Draw(6, N)` — one instance per neuron.
3. **Morphology tube pass** (skipped when `connection_layer == 0`): additive, no depth. Draws procedural dendrite + axon branches as shader-generated tubes via `render_morphology.wgsl → vs_main / fs_main`. `MorphSegment` storage buffer generated at network build time (`crates/brain-visualizer/src/sim/morphology.rs`). See Morphology pass below.
4. **Morphology soma sphere pass** (skipped when `connection_layer == 0`): additive, no depth. Draws one UV-sphere per neuron via `render_morphology.wgsl → vs_sphere / fs_sphere`. Uses the same `last_spike` and `morph_uniform` buffers as the tube pass (`render_soma_spheres` pipeline, `crates/brain-visualizer/src/sim/gpu/pipelines.rs`).
5. **Near-LOD passes** (skipped when `DRAW_LEGACY_NEAR_SPHERES = false`): cull_neurons compute → (cull_synapses if `DRAW_LEGACY_CYLINDERS`) → write_indirect → sphere render → cylinder render.
6. **Bloom post-process** (skipped when `bloom_strength == 0`): scene is in an HDR offscreen target; bright-pass → separable 9-tap Gaussian blur (half-res ping-pong) → composite with soft-add `1 - exp(-bloom)` to avoid hard clipping. When bloom is off, `scene_view` IS `target_view` — no offscreen indirection whatsoever.

## Morphology pass

Procedural neuron geometry (soma body + dendrite tree branches + shared root/cluster branches + terminal twigs) is generated once at network build time. Two separate draw sub-passes cover branches and soma bodies; both are additive, no depth write, and share the same `MorphUniforms` buffer.

**Tube sub-pass (branches).** Branch segments are uploaded as a flat `MorphSegment` array (48 B per segment, branch-only). `render_morphology.wgsl → vs_main` builds a shader-generated tube: `TUBE_SIDES` sides (default 6), two rings (ring 0 at endpoint `a` with `radius_a`, ring 1 at `b` with `radius_b`), triangulated as `TUBE_SIDES * 2 * 3` vertices per instance. A stable per-vertex basis is built from the segment axis with the fallback `abs(axis.y) < 0.9 ? (0,1,0) : (1,0,0)` to avoid degenerate cross products. Open tubes (no end caps) at this scale. `TUBE_SIDES` is a WGSL `override` constant set at pipeline build from the render-quality config (see Render quality below), so the draw vertex-count is runtime-driven: `render_full` draws `pass.draw(0..morph_tube_verts, 0..segs)`, where `morph_tube_verts` is recomputed from the same tube-sides value (`crates/brain-visualizer/src/sim/gpu/mod.rs → tube_verts`) to stay in lockstep with the override. Tube pass bind group bindings: 0 = `segment_buffer`, 1 = `last_spike`, 2 = `morph_uniform`.

**Soma sphere sub-pass.** A separate `render_soma_spheres` pass draws one `MorphSphereInstance` per neuron (emitted by `crates/brain-visualizer/src/sim/morphology.rs → emit_soma_spheres`, called from `crates/brain-visualizer/src/sim/gpu/resources.rs → init_morph_resources`). Sphere geometry is a low-res UV sphere via `render_morphology.wgsl → vs_sphere / fs_sphere`: `SPHERE_SLICES` × `SPHERE_STACKS` (defaults 8 × 6) → `slices * stacks * 2 * 3` vertices per instance (288 at default), drawn `pass.draw(0..morph_sphere_verts, 0..n_spheres)`. Both are WGSL `override` constants set at pipeline build from the render-quality config (see Render quality below); `morph_sphere_verts` is recomputed from the same values (`crates/brain-visualizer/src/sim/gpu/mod.rs → sphere_verts`) to keep the draw count in lockstep. Soma radius = `params::R0` (the `MorphologyParams` base radius). Sphere pass bind group bindings: 3 = `sphere_instances`, 4 = `last_spike` (same buffer), 5 = `morph_uniform` (same buffer). Binding slots 3/4/5 avoid WGSL name clashes with tube slots 0/1/2 in the shared shader module — WebGPU validates only bindings reachable per entry point. See [`manifold.md`](manifold.md) for the `MorphSphereInstance` layout and generation contract.

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

`MorphUniforms` (**192 B**, Rust ↔ WGSL layout locked) carries the lighting fields after `color_by`: `light_dir: vec3<f32>`, `ambient`, `diffuse_intensity`, `rim_intensity`, `rim_power`, and the v0.3.1 additions `resting_brightness` and `active_boost` (the latter replaced the former hardcoded WGSL `const BOOST = 1.8`), plus padding. Size asserts: `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_layouts_locked`. See `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms` for the full field list.

**Spike lighting.** There is no traveling pulse. When a neuron fires, its connections light *instantly* and fade with the **same** `exp(-tick_diff/glow_tau)` curve as the far-glow neuron dot. One active toggle fed via `MorphUniforms`: `light_next` (downstream, default ON). `light_past` is **removed from the settings surface** (Float32Array index 9 = `reserved_zero`, uniform field always `0`) — upstream lighting on shared arbors was misleading because shared root/cluster segments are source-owned; target-keyed lighting only resolved correctly at terminal twigs. See [`../decisions/rendering.md`](../decisions/rendering.md) for rationale.

The shader reads `last_spike[neuron_id]` each frame; `path_len` is retained in `MorphSegment` but no longer drives timing. See `render_morphology.wgsl → vs_main` for the brightness model.

**Layout contracts.** `MorphSegment` (48 B, branch-only) field order must stay byte-identical between `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment` and `render_morphology.wgsl`. `MorphSphereInstance` (32 B, soma-only) must stay byte-identical with its WGSL counterpart. `MorphUniforms` (192 B) must match `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms`. Reordering fields in any of these is the primary corruption source; size asserts guard all three — `crates/brain-visualizer/src/sim/morphology.rs → segment_layout_is_48_bytes / sphere_instance_layout_is_32_bytes` and `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_layouts_locked`.

## Legacy near-LOD cylinder pass

Permanently off (`DRAW_LEGACY_CYLINDERS = false`); the morphology pass is the live connection visual. See `render_cylinder.wgsl → vs_main` to revive.

## Bloom / HDR path

When `bloom_strength > 0` the scene renders into an `rgba16float` offscreen texture (`hdr_view`). The bloom pipeline is: bright-pass (luminance threshold `BLOOM_THRESHOLD = 0.55`) → horizontal blur → vertical blur at half resolution → composite. The composite uses `1 - exp(-bloom * strength)` rather than plain addition to roll off smoothly near 1.0. The additive scene passes cooperate naturally with bloom because additive energy accumulates in the HDR buffer without clamping before the post pass.

When `bloom_strength == 0`, `scene_view = target_view` — the validated direct path — with zero overhead and bit-for-bit identical output to the pre-bloom baseline. The shipped v0.2.1 default is `0.4`, so the HDR path is now enabled unless a caller explicitly turns bloom off.

## Key gotcha: no portable point size in WebGPU

`@builtin(point_size)` is not portable in WGSL/WebGPU. Do not use it. All neuron "glow dots" are camera-facing quad billboards (`Draw(6, N)`) with a Gaussian falloff in the fragment shader. This is the only way to get a variable-radius soft glow that works across all WebGPU backends. The `render_far.wgsl` comment documents this constraint explicitly.

## Offline validation

`crates/brain-visualizer/examples/render_check.rs` drives the full production GPU pipeline on llvmpipe (offscreen 512×512), warms the sim for 300 ticks, and asserts pixel-level correctness: not all-black, distinct region colors present, stimulate produces measurable activity increase. `crates/brain-visualizer/examples/near_lod_check.rs` exercises the frustum-cull → indirect draw path at close camera distance. Both validate all shaders via Naga at pipeline-build time (panics on any WGSL error).

## Update when

- A new render pass is added or the pass order changes.
- `VisualSettings` adds a new field that the render shaders consume (update the mode tables above).
- `MorphSegment` layout changes in `crates/brain-visualizer/src/sim/morphology.rs` (immediately breaks `render_morphology.wgsl`).
- `MorphSphereInstance` layout changes in `crates/brain-visualizer/src/sim/morphology.rs` (breaks the sphere sub-pass).
- `MorphUniforms` layout changes in `crates/brain-visualizer/src/sim/gpu/resources.rs` (breaks both sub-passes; update Rust + WGSL atomically — currently 192 B).
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
