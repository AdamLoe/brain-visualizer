# Rendering decisions

## Billboard neuron bodies at all camera distances

- **Decision.** Additive-blended instanced billboard quads with Gaussian falloff and recency-based glow are the neuron-body visual at all camera distances. No near-body icosphere path is retained.
- **Why.** Additive point-glow matches the real brain-viz aesthetic, and the close-camera billboard ramp reads better than faceted low-poly bodies without carrying a second geometry path.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl → NEAR_RADIUS_DIST / NEAR_RADIUS_MAX`

## Active connections only by default

- **Decision.** The full connectome is never drawn. When `connection_layer == 0`, morphology compaction and morphology draw passes are skipped entirely. When it is on, only active/recent morphology segments are drawn; inactive full-forest debug drawing is not a runtime mode.
- **Why.** At product scale the full generated morphology forest is visually a dense fog, not information. Active/recent morphology makes causality visible and keeps frame cost proportional to visible activity.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full` (morphology pass gated on `connection_layer != 0`)

## Compile bloom + active pipelines lazily, one frame after first render

- **Decision.** Boot compiles only the render pipelines the first frame actually
  draws. Bloom and true-opacity `*_active` morphology variants are compiled from
  the first post-ready rAF frame via
  `build_render_deferred_pipelines` called from the web rAF loop. `render_full`
  guards every bloom/active access with `is_some()`, and bloom HDR/blur textures
  are allocated only on the first bloom-enabled render, so the first frame paints
  correctly without them.
- **Why.** Synchronous WebGPU shader compilation blocks the calling thread; the
  bloom + active compiles are the largest avoidable cost on the boot critical
  path. Bloom is opt-in/default-off, and the active layer can fall back to the
  additive look until the deferred pipelines exist. Async pipeline creation is
  not available in the repo's current `wgpu` safe API, so this defer-on-first-use
  approach is the win without dropping below wgpu.
- **Applies to.** [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/pipelines.rs →
  build_render_core / build_render_deferred / build_morph_active_pipelines /
  is_render_deferred_built`; `crates/brain-visualizer/src/sim/gpu/mod.rs →
  build_render_core_pipelines / build_render_deferred_pipelines`;
  `crates/brain-visualizer/src/sim/gpu/resources.rs →
  ensure_bloom_render_targets`;
  `crates/brain-visualizer/src/lib.rs → startup_build_render_pipelines /
  build_deferred_render_pipelines`; `web/src/main.ts → rafLoop`.
- **Revisit when.** Hardware testing shows the bloom/active gap is multiple
  frames or causes a visible flash (fallback: compile bloom in the core stage but
  still defer the `*_active` variants), or if a future wgpu exposes safe async
  pipeline creation.

## Per-neuron identity color

- **Decision.** The identity color mode assigns each neuron a stable identity hue from the locked BV22 hash; far glow uses it directly, and morphology blends it with the existing structural tint.
- **Why.** Identity color makes individual cells easier to trace by eye without adding a buffer, changing the settings Float32Array, or weakening the dendrite/axon visual language. HSL hue from the hash is simple shader math and good enough for the default scale.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl → identity_color`; `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → identity_color`; `crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_render / build_morph_pipelines`; `web/src/ui/dev-panel.ts → _buildAppearanceTab`
- **Revisit when.** Visual review shows too many near-duplicate hues or colourblind-safe tracing becomes a requirement.

## Brain color mode as the default activity language

- **Decision.** The Brain color mode is the clean default color mode. Resting
  visible structure is pink; firing neuron cores, active morphology packets, and
  active-adjacent highlights are blue. Brain mode is a color branch only: it
  respects `surface = off`, `connection_layer = off`, and `neuron_visibility`
  instead of forcing hidden layers on.
- **Why.** The product goal is a coherent brain-themed activity view rather than
  separate debug encodings. Pink resting structure keeps the whole sculpture
  readable, while blue current activity gives spikes and traveling packets a
  single clear focal language. Reusing the existing `color_by` slot avoids a
  settings contract migration.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md),
  [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/core/settings.ts → DEFAULT_SETTINGS.colorBy`;
  `web/src/ui/dev-panel.ts → COLOR_BY_OPTIONS`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`.
  `crates/brain-visualizer/src/sim/gpu/resources.rs → ManifoldUniforms` threads
  `color_by` to the optional surface without increasing the uniform size.

## Morphology pass supersedes and deletes ribbon/cylinder connection visuals

- **Decision.** Procedural neuron morphology (soma + dendrite tree + shared
  arbor + terminal twigs) is the live connection visual. Straight-cylinder
  synapse rendering and curved-arc ribbon rendering are not retained behind
  disabled guards.
- **Why.** The morphology pass gives each neuron a distinct anatomical tree and
  makes connectivity visible as physically motivated arbors rather than arcs
  between abstracted points. Keeping disabled render paths increased build,
  docs, and settings-contract complexity without serving the current product;
  git history is the archive.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`;
  `crates/brain-visualizer/src/sim/morphology.rs`
- **Tradeoffs.** Morphology geometry is generated once at network build time;
  `connection_curve_lift` changes force a full `regenerate_morphology` call.
  The cylinders and ribbon required no such bake step.

## Source-owned soma pulse + traveling morphology impulse

- **Decision.** When a neuron fires, the body visual now reads as a short soma
  pulse first and a traveling morphology impulse second. `render_far.wgsl` and
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  vs_sphere / fs_sphere` derive a slower `glow` envelope plus a faster `flash`
  and brief white-core lift from the existing packed `last_spike` word and
  `tick`. Morphology tubes use a separate `visual_spike` clock for packet age:
  `integrate.wgsl` keeps the first unresolved visual tick until the previous
  packet has had time to reach the generated axon leaves, while physics,
  metrics, refractory checks, and scatter continue to use the latest
  `last_spike`. `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  vs_main / fs_main` derives a traveling packet from `visual_spike` plus
  `MorphSegment.path_len + t * length(b-a)`; axons carry the full outward
  packet, dendrites carry only a weak near-soma echo. This keeps
  `MorphSegment`, `MorphUniforms`, `RenderUniforms`, and the
  `VisualSettings` Float32Array unchanged; pulse defaults live in shader
  constants, the visual packet hold, and existing `glow_tau` /
  `resting_brightness` / `active_boost`.
  `light_past` stays tombstoned in the settings surface.
- **Decision.** Axon packets are attenuated by branch flow using the baked
  morphology radius as the source. The generator already derives internal axon
  radii from downstream subtree synaptic weight, so `render_morphology.wgsl`
  maps interpolated unscaled radius to `flow_strength` and applies it to packet
  brightness, active tint, and active-pass opacity. Trunks stay bright; split
  branches get smaller through their existing tube radius and dimmer through the
  flow multiplier.
- **Why.** The whole-arbor instant glow made firing read as a state change
  rather than a causal event, but using only latest `last_spike` made frequently
  firing neurons restart their packet near the soma before the signal reached
  most outgoing leaves. The visual clock keeps the effect fully GPU-side and
  simulation-honest without changing synaptic timing: it only changes what the
  morphology renderer remembers. Using `path_len` restores branch-local motion
  without forcing a segment layout migration. Keeping dendrites to a local echo
  avoids implying false outgoing signaling on source-owned dendrite geometry.
  Using baked radii makes split intensity track real downstream synaptic weight
  without new per-edge flow buffers; live `i_current` / `I_next` is target
  accumulation, not per-edge flow. Upstream lighting remains deferred because
  shared arbors are still source-owned structure.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), [`../architecture/manifold.md`](../architecture/manifold.md), [`../architecture/data-model.md`](../architecture/data-model.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`;
  `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms,
  NeuronBuffers::visual_spike`.

## Morphology material stays procedural, deterministic, and asset-free

- **Decision.** The morphology material stays entirely in
  `render_morphology.wgsl`: no external textures, samplers, new bind groups, or
  public beauty controls. Soma spheres and branch tubes use deterministic hash /
  noise helpers keyed from world position, normal, `path_len`, `kind`, and
  `neuron_id`. It keeps the shared uniform/layout surface unchanged; material
  strengths are shader constants until review proves that extra config fields
  are necessary.
- **Why.** The beauty target was restrained surface life, not a new asset
  system. Procedural material variation improves close-up anatomy and still
  preserves the existing region/E-I/identity color modes, while avoiding a
  second shared-layout migration on the same implementation surface as the pulse
  work.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  material_hash / material_noise3 / tube_material / soma_material`.

## Draw all K outgoing connections per neuron

- **Decision.** The morphology generator emits one axon arbor per configured synaptic target, not a fixed branch subset; the per-neuron segment cap is derived from K (`max_segs_per_neuron`).
- **Why.** With spike lighting keyed off real synapses, drawing only a subset of configured connections would light paths that do not match where current actually flows. No point simulating connections you never draw — the lit set should equal the real synapse set.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md), [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate, max_segs_per_neuron`.
- **Revisit when.** All-K coverage reads as a hairball at default scale — then lower default K (a runtime knob already) rather than re-capping branches.

## Bloom is retained internally, not exposed as a user setting

- **Decision.** The bloom pipeline remains in the renderer and
  `bloom_strength > 0` still enables the HDR offscreen render + bright-pass +
  separable blur + composite path, but the user-facing `VisualSettings` index is
  tombstoned and zero-written. Normal app settings therefore use the direct
  `target_view` path and do not allocate HDR/blur textures; internal
  examples/tests can still call `GpuBackend::set_bloom_strength` to validate the
  retained pipeline and its first-use target allocation.
- **Why.** Bloom strength was not needed as a user control. Keeping the pipeline
  avoids a broad render-resource deletion while removing the settings and
  persistence surface.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full, VisualSettings::from_slice, GpuBackend::set_bloom_strength`; `crates/brain-visualizer/src/sim/gpu/resources.rs → GpuResources::ensure_bloom_render_targets`; `crates/brain-visualizer/src/sim/gpu/shaders/bloom.wgsl → fs_bright / fs_blur / fs_composite`; `web/src/core/settings.ts → toFloat32Array`.

## Morphology as shader-generated curved tubes + soma spheres

- **Decision.** Branch segments are drawn as shader-generated tapered, curved
  multi-ring tubes; soma bodies are a separate morphology-owned UV-sphere
  sub-pass. The tube centerline bow is derived deterministically from existing
  `MorphSegment` fields, so organic close-up curvature does not add a buffer,
  widen the 48 B segment layout, or alter activity ownership. Resting tube and
  soma sub-passes keep additive blend and no depth write; firing geometry also
  gets the active depth-tested redraw described below. The soma sphere is
  procedurally deformed in `vs_sphere` toward the dominant axon root carried by
  `MorphSphereInstance::root_dir/root_pull`, then lit by the same soma material
  and spike pulse path. A simple ambient + half-Lambert diffuse + rim lighting
  model in `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  fs_main / fs_sphere` makes curvature readable without abandoning the glow
  aesthetic. Cross-section tessellation (`TUBE_SIDES`, `SPHERE_SLICES`,
  `SPHERE_STACKS`) and lighting/brightness values are runtime-configurable;
  the fixed tube ring count is shader-owned and mirrored in Rust draw counts.
- **Why.** Billboard quads give no sense of volume or curvature, and straight
  two-ring cylinders still read as angular generated linework at close camera
  distances. Shader-generated curved tubes require no new storage contract, are
  fully GPU-side, and immediately read as cylindrical branching processes.
  Keeping the resting layer additive/no-depth avoids a broad compositing rework,
  while the active redraw supplies true occlusion only for firing geometry. Rim
  lighting keeps the SNN glow aesthetic dominant while making curvature visible
  in screenshots.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), [`../architecture/manifold.md`](../architecture/manifold.md)
- **Alternatives considered.** Pre-built indexed mesh per neuron arbor — correct
  junctions and caps, but requires a new mesh-buffer layout and a much more
  complex generator; deferred. Curved control points in `MorphSegment` —
  rejected because the shader can derive a stable bow from existing fields and
  keep the corruption-sensitive 48 B layout unchanged. Depth writes for all
  resting morphology — deferred because active-only opacity solves the firing
  solidity problem without turning the inactive forest opaque.
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  tube_curve_bend / vs_main / fs_main / vs_sphere / fs_sphere` (`TUBE_SIDES` /
  `SPHERE_SLICES` / `SPHERE_STACKS` override consts); `crates/brain-visualizer/src/sim/gpu/pipelines.rs →
  build_morph_pipelines`; `crates/brain-visualizer/src/sim/gpu/mod.rs →
  tube_verts`; `crates/brain-visualizer/src/sim/morphology.rs → LightingConfig,
  RenderQualityConfig`; `crates/brain-visualizer/src/sim/gpu/resources.rs →
  MorphUniforms`. The dev-panel exposure decision lives in
  [`dev-tooling.md`](dev-tooling.md) and [`manifold.md`](manifold.md).
- **Revisit when.** Resting self-occlusion becomes more important than additive
  readability, or branch junctions need explicit caps/fillets rather than
  shader-built tube continuity.

## True opacity for selected connection geometry, layered over the additive resting passes

- **Decision.** Selected connection geometry gets a genuine depth-tested redraw on top of the unchanged additive resting passes, so visible tubes occlude instead of reading as see-through. Tube selection still comes from the spike packet compaction path, and selected tube fragments return continuous straight alpha from the inactive floor toward the active ceiling using spike-packet proximity; subdued/inactive connection state is expressed through dark resting brightness/tint and low alpha, while lit packet brightness remains fragment-local so the impulse still travels. Soma opacity uses the same floor/ceiling model from soma activity.
- **Why.** Additive blending physically cannot occlude — it can only make things brighter, so everything read as uniformly muddy translucency. A real depth + alpha path lets active neurons read as solid and inactive structure drop to near-invisible without turning the opacity controls into binary thresholds. Keeping the active redraw encoded at the low end avoids the additive blowout caused by removing the only depth-tested morphology layer, while the soft ceiling preserves the expected "least emphasis" slider meaning. Layering the active redraw over the additive passes (rather than converting them) avoids reworking the whole bloom/HDR compositing pipeline: the active passes write the same HDR `scene_view` color, so bloom composes over them with zero bloom-path edits.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Alternatives considered.** Fake opacity by making active geometry brighter-additive — rejected because additive cannot occlude, which was the actual defect. Convert the resting passes to depth-tested too — rejected: needs pass sorting and breaks the bloom-friendly additive resting glow; resting self-occlusion stays deferred.
- **Tradeoffs.** The active passes redraw the same tube/soma geometry and the active layer owns its own depth clear. The legacy opacity-named knobs ride repurposed `MorphUniforms` pads as coverage/emphasis inputs, so the 192 B layout is unchanged; that size is gated by `cargo test` through `morph_layouts_locked`.
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full`; `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → fs_main_active / fs_sphere_active`; `crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines, build_morph_active_pipelines`.

## "Active" = firing, not click-selection (no picking)

- **Decision.** The opaque active layer is driven by *firing* (spike-keyed `last_spike` recency), reusing the same activity signal the additive lighting already uses. Click-to-select / GPU picking is not built; the opacity pass is shaped so a click-selected set *could* feed it later, but no picking subsystem is revived.
- **Why.** Keeps the opacity feature self-contained and avoids reviving the deferred picking subsystem (which stays in `future_roadmap.md`). Firing is already the renderer's universal activity key, so "opaque if active" needs no new buffer, selection state, or readback.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → fs_main_active / fs_sphere_active` (alpha from the spike-driven `activity`).
- **Revisit when.** Click-to-inspect / picking is built — the active layer's alpha source can then accept a selected-set input.

## Active-layer coverage knobs live in LightingConfig, not the VisualSettings Float32Array

- **Decision.** The legacy `active_opacity` and `inactive_opacity_floor` fields live in the morph-config-owned `LightingConfig` and ride two repurposed trailing `MorphUniforms` pad slots — the locked `VisualSettings` Float32Array index contract is untouched. Their UI labels describe active/inactive coverage because visible tube fragments are rendered solid.
- **Why.** `LightingConfig` is the established, contract-light path for morphology beauty knobs (it already carries `resting_brightness` / `active_boost` through `MorphUniforms`). Growing the Float32Array would touch the locked Rust↔TS index contract and the persistence schema for no benefit; repurposing reserved `MorphUniforms` pads keeps the 192 B layout assert green under `cargo test`.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → LightingConfig` (`active_opacity`, `inactive_opacity_floor`); `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms`.

## Connection visibility modes reuse GPU-indirect segment selection

- **Decision.** A GPU compute pass (`compact_morph_segments.wgsl`) selects each
  frame the segments for the current connection visibility mode. Active/recent
  mode selects the about-to-be-lit / lit / recently-lit packet band.
  Until-arrival mode selects every segment owned by a recent spike until the
  packet front has passed that segment endpoint, then lets it drop. Selection
  runs per segment chunk, and both morphology tube passes draw each chunk's
  compacted subset via `draw_indirect`. There is no whole-geometry debug draw
  path. Soma sphere passes are per-neuron and unaffected.
- **Why.** Frame cost must scale with visible spike activity rather than total generated segment count, while the review mode needs a readable whole-connection context during packet travel. Reusing compaction and indirect args keeps both behaviors on the existing GPU-side selection path.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md), [`../architecture/scaling.md`](../architecture/scaling.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl → reset / compact / write_args`; `crates/brain-visualizer/src/sim/gpu/mod.rs → render_full`; `crates/brain-visualizer/src/sim/gpu/pipelines.rs → build_morph_pipelines`; `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphSegmentChunk / MorphBuffers`.

## Per-frame segment selection is GPU-indirect, never CPU readback

- **Decision.** The compacted segment count flows from compaction into the tube
  passes entirely through chunk-local GPU indirect draw args
  (`active_draw_args`); the CPU never reads back the selection to size the draw.
  A blocking selected-count readback sums chunk counters only for profiler/test
  diagnostics.
- **Why.** Per-frame selection must not stall the render loop on a GPU→CPU map,
  which would break the no-readback-in-the-loop policy the whole backend is built
  around.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full, GpuBackend::read_active_segment_count`.

## Default connection visibility selects the packet band, not the whole fired arbor

- **Decision.** The compaction predicate selects only the segments under the
  traveling impulse **packet band** (a `HEAD_HEADROOM` lead plus a `TAIL_REACH`
  tail around `front = age * speed` along `path_len`) in the default
  active/recent mode, mirroring the render shader's per-segment activity exactly.
  `glow_tau` is not a packet lifetime or culling input; it controls soma/legacy
  afterglow only.
- **Why.** Selecting the whole arbor for the full glow lifetime would keep
  nearly all segments and defeat the scaling goal, while tying packet survival
  to `glow_tau` would make low afterglow settings truncate long-range packets
  before they reach their leaves.
- **Tradeoffs.** Users who need whole-connection context can opt into
  until-arrival mode; the default remains packet-local for readability and cost.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl → compact` (`HEAD_HEADROOM_MUL` / `TAIL_REACH_MUL`); `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → fs_main`.

## Faster, wider pulse for long-range axon segments

- **Decision.** Axon segments whose cumulative `path_len` passes
  `LONG_RANGE_PATH` use the faster, wider long-range impulse packet constants
  instead of the local-arbor packet constants. Both shaders carry the split; no
  extra uniform is required (`MorphUniforms` stays 192 B, gated by `cargo test`
  through `morph_layouts_locked`).
- **Why.** Waypoint-routed long axons are far longer than local arbors; at the
  local speed/width a single packet reads as the fiber blinking rather than a
  signal sweeping the projection. A faster, wider packet reads as motion along
  the long fiber.
- **Applies to.** [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl` (`AXON_IMPULSE_SPEED` / `IMPULSE_WIDTH` / `LONG_RANGE_IMPULSE_SPEED` / `LONG_RANGE_IMPULSE_WIDTH` / `LONG_RANGE_PATH`); mirrored in `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl`.

## See also

- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/active-edges.md`](../architecture/active-edges.md)
- [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
