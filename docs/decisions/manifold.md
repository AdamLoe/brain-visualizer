# Manifold decisions

## Procedural brain-shaped shell and shell-biased placement, no external mesh assets

- **Decision.** The cortical surface is generated entirely in code via icosphere
  subdivision, a shared deterministic brain-envelope shaping pass, and a
  structured deterministic `FoldField`; no external mesh files are loaded or
  bundled. Neuron placement uses that same folded field and is biased toward
  the cortical shell rather than filling a uniform sphere.
- **Why.** A flat patch or smooth blob does not read as a brain to a visitor. The
  visual recognizability — silhouette first, folds second — is the point of the
  project. Using one host-side envelope for both shell generation and placement
  keeps the mesh and neuron cloud in agreement, while still preserving the
  asset-free, deterministic, seed-reproducible generation route.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`;
  `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, gyrify`;
  `crates/brain-visualizer/src/manifold/mod.rs → ManifoldParams, brain_outer_radius, place_neurons`
- **Tradeoffs.** The field produces plausible but not anatomically accurate
  sulcal geometry, and folded-shell placement is still a stylized cortical
  volume rather than a real anatomical tissue model. The topology remains
  star-convex and radial; a true separated hemisphere mesh or overhanging folds
  are deferred. That is acceptable — the goal is recognizable impression, not
  neuroscience fidelity.
- **Revisit when.** A specific gyral atlas is required for region-accurate
  stimulation experiments, or when a real mesh can be streamed without bundling
  a large asset.

## Region assignment by hash-shuffle, not spatial blocking

- **Decision.** Input, association, and output regions are assigned by default by
  shuffling neuron indices with a deterministic integer hash and slicing the
  result, producing a spatially random non-contiguous assignment. A bounded
  internal prototype mode can instead use the
  anterior-posterior axis with deterministic jitter to bias input posterior and
  output anterior, but it is opt-in and exposed only as a hidden dev-panel
  Network-tab checkbox for side-by-side review.
- **Why.** The anterior–posterior spatial blocking originally proposed in
  `ANTERIOR_POSTERIOR_AXIS` is now available only as a review prototype because
  promoting it would change the startup visual/dynamics story. Keeping the
  toggle in the hidden dev panel gives reviewers a persistent side-by-side
  switch without making the prototype a product default. The hash-shuffle keeps
  stable, reproducible proportions across neuron counts; the split/default
  contract is gated by `cargo test` through
  `region_split_approx_30_40_30`,
  `default_region_assignment_mode_is_hash_random`, and
  `prototype_region_assignment_mode_is_opt_in`. The biological
  anterior-posterior gradient is expressed in the default build through the
  connectivity feed-forward bias owned by the simulation.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/manifold/regions.rs →
  RegionAssignmentMode, assign_regions, assign_regions_with_mode`;
  `crates/brain-visualizer/src/manifold/mod.rs → ManifoldParams,
  ANTERIOR_POSTERIOR_AXIS`;
  `web/src/core/types.ts → AppConfig.regionAssignmentMode`;
  `web/src/ui/dev-panel.ts → _buildNetworkTab`
- **Tradeoffs.** The prototype gives a more legible posterior-to-anterior visual
  story without changing connectivity, drive, or type-byte encoding, but the
  localized drive may change apparent wave startup and needs visual/dynamics
  review before any default promotion.
- **Revisit when.** Spatial blocking is needed for the cursor-stimulation
  "click posterior to activate input region" UX to work reliably.

## Axon arbor grown by Prim-like tree + relaxation, not a hand-tuned fan

- **Decision.** The axon arbor is a single tree grown by **Prim-like greedy
  attach plus local relaxation** after a descriptor-backed primary trunk. Leaves
  are the unique non-self targets. Each source neuron first emits the
  `ProcessRoot::soma_root → ProcessRoot::first_fork` trunk, and the attach loop
  is not allowed to attach leaves at the soma root or split that trunk edge.
  Each greedy step after the trunk adds the globally-best `(leaf, attach-point)`
  edge (which may split a non-trunk edge into a shared internal node), and a
  local ancestor-window pass relaxes internal nodes while holding the soma root,
  descriptor first fork, and leaf sockets fixed. The dendrite tree, source-type
  bytes from region+seed, deterministic sockets, per-branch `path_len`, and
  named segment budgets with slack are retained.
- **Why.** A fixed trunk→cluster→twig fan reads as a flat spray with no organic
  shared structure; the Prim+relax grammar grows real shared trunks that fork
  smoothly toward targets, so the arbor reads as a root/tree. Determinism is
  preserved because the greedy loop and relaxation use only ordered `Vec`
  structures and `(target_id, node index)` tie-breaks — no `HashMap` iteration,
  no float-equality dependence — and the `salt::TREE_*` draws are disjoint from
  connectivity.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate,
  ArborNode, MorphologyParams::locked_default, MorphologyStats, MorphSegment,
  segment_cap, max_segs_per_neuron`; `crates/brain-visualizer/src/connectivity/hash.rs →
  mix_key, hash32`; `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main`
- **Tradeoffs.** Internal trunk/fork edges carry the source id (shared paths stay
  source-lit); only leaf edges carry the real target id. Single-target arbors now
  pay the same descriptor-trunk overhead as fan-out arbors so the near-soma shape
  is consistent. The 48 B `MorphSegment` layout remains the hard Rust ↔ WGSL
  contract — gated by `cargo test` through `segment_layout_is_48_bytes` — so the
  tree ships inside it without new fields.

## Branch smoothness uses host path sampling plus shader-bowed tubes

- **Decision.** Branch and long-range path smoothness is split between bounded
  host-side path sampling and render-side curved tube tessellation. The generator
  still emits the existing 48 B `MorphSegment` samples; the shader expands each
  segment into a short multi-ring tube whose centerline bow is derived
  deterministically from existing segment fields.
- **Why.** Host sampling keeps real path ownership, `path_len`, and waypoint
  routing explicit for activity packets, while shader-bowed tubes remove the
  close-up "straight cylinder between samples" look without widening the
  Rust/WGSL storage contract. The segment layout stays gated by `cargo test`
  through `segment_layout_is_48_bytes`.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  adaptive_subsegments, GeneratorConfig::apply_to`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  tube_curve_bend / vs_main`; `web/src/core/morph-config.ts →
  MORPH_DESCRIPTORS`.

## Long-range waypoint routes stay inside the generated brain bounds

- **Decision.** Long-range axon leaf routes clamp generated waypoints, route
  endpoints, and route control points to a generation-time inner brain bounds
  helper before emitting path samples.
- **Why.** Long projections should travel through the brain volume instead of
  escaping the silhouette. Clamping generated path samples is the durable fix;
  shrinking rendered tube radius would only hide the escape.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  BrainBounds, long_range_waypoints, generate`.

## Incoming dendrites from real reverse sockets, not decorative local trees

- **Decision.** Dendrites are generated from the target neuron's real incoming
  socket groups. The morphology builder stores every non-self raw incoming
  synapse, aggregates duplicate source/target/socket records by weight for
  visible groups, and draws each unique group at product scale. Dendrite geometry is
  target-owned (`kind = 0`, `neuron_id = target_id`) but organized as
  soma-surface root collars, close first forks, and source-specific terminal
  leaves instead of one long shared stem per bucket. Shared internal roots/forks
  keep `target_id = neuron_id`, while source-specific terminal leaves carry
  `target_id = source_id` and are emitted socket-to-soma.
- **Why.** Decorative dendrites made axons land near plausible branches but did
  not represent the cell's actual inputs. The reverse socket view makes the
  handoff honest without widening `MorphSegment` or adding a side-channel
  activity buffer.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md).
- **Tradeoffs.** Shared roots/forks have multiple presynaptic owners, so the
  current design leaves them structurally target-owned and not presynaptically active. Only
  source-specific leaves can pulse from the presynaptic source via existing
  `target_id`. Dense future scales must lower K or add an explicit cap policy;
  hidden visual sampling is not allowed. Dead primary-min/span config fields are
  not part of the live control vocabulary, which is
  socket placement plus root count, fork distance, curve tightness, branch
  thickness, taper, and group spacing.
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  build_incoming_view, emit_incoming_dendrites`; `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main`.

## Area-preserving √-width over literal-linear weight→width

- **Decision.** Branch radius encodes signal-carrying capacity via
  the bottom-up area-preserving width rule in `generate` for root/internal
  nodes. Terminal leaf nodes are set to the twig floor. The descriptor trunk
  starts at full radius; each fork sheds its children's weight share.
- **Why.** Area-preserving √ scaling (Murray/Rall) keeps shared trunks visually
  substantial while still letting thin twigs read. The locked
  `axon_root_radius_fraction` keeps the primary trunk substantial against the
  soma; terminal leaves go to the floor so single-target and low-degree arbors
  still taper instead of becoming constant-width hoses. Literal-linear
  weight-fraction width was rejected as too wispy.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate`
  (width pass); `crates/brain-visualizer/src/connectivity/mod.rs → weight`.
- **Alternatives considered.** Literal-linear weight fraction (rejected: wispy).
- **Tradeoffs.** Width is static — baked once at generation from synaptic weight;
  there is no runtime width path.

## Soft fork-degree penalty over a hard child cap

- **Decision.** Fork-degree control is a *soft* `tree_score_degree` term in the
  attach score, not a hard cap. Relaxation then spreads siblings into fork-like
  geometry.
- **Why.** A soft penalty keeps the generator a single greedy pass with no
  synthetic-split bookkeeping, and lets occasional higher-degree forks happen
  where the geometry wants them rather than forcing artificial intermediate
  nodes everywhere.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate`
  (attach score), `MorphologyParams::tree_score_degree`.
- **Revisit when.** If review shows trunks visibly spraying too many children,
  the known fallback is to convert `tree_score_degree` into a hard cap that
  synthesizes intermediate split nodes. `MorphologyStats::cluster_count_histogram`
  is the signal to watch.

## Adaptive edge subdivision over a single flat global subsegment count

- **Decision.** Edge tessellation is length- and curvature-aware through
  `adaptive_subsegments`, with long-range hops using a smaller max segment
  length than local edges. The legacy flat `edge_subsegments` is kept only as a
  legacy/floor knob.
- **Why.** A single global subsegment count over-tessellates short local edges and
  under-samples long fibers, where a travelling pulse needs enough spatial samples
  to read as motion rather than a blinking span. Driving the count from already
  deterministic geometry (chord length + salt-seeded bend) keeps generation
  seed-reproducible while putting detail where curvature/length actually need it.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  adaptive_subsegments, MorphologyParams`.
- **Tradeoffs.** Per-edge segment counts now vary, so the buffer cap is sized for
  the worst case (`EDGE_SUBSEGMENTS_MAX`, and waypoint hops) rather than a flat
  multiply.

## Long-range axons route through deterministic bowed waypoints

- **Decision.** A leaf axon whose parent-to-socket chord exceeds the configured
  long-range distance threshold is emitted through deterministic bowed waypoints
  instead of one span. Target identity and weight are unchanged — waypoints are
  visual route geometry only.
- **Why.** A single giant span reads as an unphysical wire crossing empty space;
  intermediate bowed waypoints make long axons read as biological projections
  curving around the volume.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  long_range_waypoints, LONG_RANGE_MAX_WAYPOINTS, MorphologyParams` (the
  `long_range_*` fields).
- **Tradeoffs.** Long leaves cost more segments; the per-target budget is sized
  from the waypoint and subsegment max constants rather than from the live slider
  values.

## Classify "visually long" by world distance, not a connectivity flag

- **Decision.** Morphology decides whether to route an axon leaf through waypoints
  by world distance (chord vs `long_range_chord_cells · cell_size`), read-only —
  it does **not** read any per-synapse long-range flag from connectivity.
- **Why.** Connectivity bakes the heavy-tail coin into the resulting target id and
  exposes no per-synapse long-range flag, so geometry has nothing to query and must
  use distance. This keeps waypoint routing a pure visual concern that never
  touches synaptic semantics.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  long_range_waypoints`; `crates/brain-visualizer/src/connectivity/mod.rs →
  target, target_with_cell` (return target id only, no flag).

## Bushy local dendrite decoration, self-owned and per-neuron capped

- **Decision.** Each incoming group's tip grows a bushy local arbor — secondary
  branchlets and per-twig salt-curled terminal twigs with a trunk→twig radius
  taper. These decorations are **self-owned** (`target_id == neuron_id`) and never
  invent target ids, so the real presynaptic leaves
  (`kind == 0`, `target_id = source_id`, `path_len = 0`) keep sole ownership of
  presynaptic activity. Decoration density is bounded by the configured
  per-neuron group cap and does not ramp down with N.
- **Why.** One bow per edge reads as a generic radial star-burst; per-branch curl
  and twigs make dendrites read as organic local arbors. Self-ownership keeps the
  decoration from corrupting the honest presynaptic lighting contract. GPU
  segment chunking handles high-N storage pressure, so the generator does not
  silently change morphology shape to satisfy a binding limit. Deterministic
  salts everywhere keep same seed/config → bit-identical morphology.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  emit_incoming_dendrites, effective_decor_group_max, DENDRITE_DECOR_GROUP_MAX,
  salt` (`DENDRITE_BRANCHLET`/`DENDRITE_TWIG`/`DENDRITE_TWIG_CURL`);
  `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_segment_chunk_layout`.
- **Tradeoffs.** The decoration controls exposed through
  `web/src/core/morph-config.ts → MORPH_DESCRIPTORS` are runtime-clamped; the
  remaining decoration params stay locked.

## Known limitation: high-N init cost and segment-cap growth (graceful-degrade guard deferred)

- **Decision.** The Prim+relax generator ships without a high-N graceful-degrade
  guard; the default product tier is the supported target.
- **Why.** Each arbor runs a greedy attach plus per-attach relaxation, and the
  per-arbor segment allocation cap grows with fanout and branch detail. High-N
  tiers can therefore initialize slowly and approach GPU buffer limits. The
  guard is deferred because the default scale remains the product target.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/scaling.md`](../architecture/scaling.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate,
  segment_cap`.
- **Revisit when.** A high-N tier becomes a target — then add the deferred
  degrade-gracefully guard (cap effective leaves or subsegments per arbor) before
  the init cost or buffer cap becomes user-visible.

## Deterministic sockets over vague near-target landing

- **Decision.** Terminal twigs land on deterministic socket anchors near visible
  dendrite endpoints or branch points instead of stopping "close enough" to a
  target soma.
- **Why.** A vague near-target endpoint still looks detached at review scale.
  Sockets give the terminal twigs a visible landing cue and make the final
  target branches feel attached to the cell rather than floating in the void.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate,
  MorphologyStats`
- **Tradeoffs.** Socket placement is deterministic and local, but it is still a
  visual grammar choice rather than a claim about biological microscale detail.

## Soma body as a separate MorphSphereInstance buffer, not a MorphSegment kind

- **Decision.** Soma bodies are drawn via a dedicated `MorphSphereInstance`
  buffer, emitted by `emit_soma_spheres` from existing neuron positions plus the
  host-side `ProcessRoot` descriptor. They are rendered by a separate
  `render_soma_spheres` sub-pass that reuses the same `last_spike` and
  `morph_uniform` buffers as the tube pass. `MorphSegment` remains the
  branch-only contract and carries no soma primitive.
- **Why.** Adding a soma variant to `MorphSegment` would either change the 48 B
  layout (gated by `cargo test` through `segment_layout_is_48_bytes`) or
  repurpose fields like `b` as radius-only data in a non-obvious way. A separate
  buffer keeps the branch contract clean, isolates the sphere geometry from the
  tube vertex shader, and lets the two sub-passes reuse shared uniforms without
  encoding the soma/branch distinction in a mixed struct. The soma instance's 48
  B layout is gated by `cargo test` through `sphere_instance_layout_is_48_bytes`.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md), [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Alternatives considered.** Keeping soma as the far-glow billboard only — no layout change, but no true 3D body.
- **Tradeoffs.** Separate storage buffers and bind groups instead of one; the sphere bind group uses distinct binding slots to avoid WGSL name clashes with the tube bindings in the shared shader module. The soma instance carries one dominant root direction and pull strength, avoiding a third soma deformation bind group.
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → MorphSphereInstance / emit_soma_spheres`; `crates/brain-visualizer/src/sim/gpu/pipelines.rs → GpuPipelines` (`render_soma_spheres` field); `crates/brain-visualizer/src/sim/gpu/resources.rs → init_morph_resources`

## Morphology generator exposed for tuning; budgets/slack/salts stay protected

- **Decision.** The generator shape fields of `MorphologyParams` are exposed as a
  tunable `MorphologyConfig` to the hidden dev panel, layered over the locked
  default at apply time. Allocation/safety budgets and the `salt::*` hash
  constants are deliberately **not** exposed and are re-locked even when a config
  is applied.
- **Why.** The shape fields are exactly what a later visual-tuning pass needs to
  iterate on, and their ranges are bounded narrowly around the locked default so
  no slider can produce self-intersecting/inverted geometry. The budgets gate the
  per-neuron buffer cap (`max_segs_per_neuron`) — a UI slider there risks silent
  truncation or OOM with no visual upside — and the salts are the determinism
  namespace that keeps morphology hash draws disjoint from connectivity, so moving
  them would break seed reproducibility.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → MorphologyConfig, GeneratorConfig::apply_to, MorphologyParams::locked_default, max_segs_per_neuron`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::set_morphology_config`.
  (The separate JSON channel rationale is in
  [`dev-tooling.md`](dev-tooling.md).)
- **Revisit when.** A real tuning workflow needs the budgets as advanced
  diagnostic controls, or the tuning pass settles on new locked defaults.

## See also

- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`dev-tooling.md`](dev-tooling.md) — separate localStorage key + WASM entry point
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
