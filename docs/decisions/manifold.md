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

## First realism pass targets silhouette, structured folds, and folded placement

- **Decision.** The first realism pass improves the whole-brain silhouette,
  hemispheres/fissure, and cortical folds together, and neuron placement follows
  the folded outer radius. Regions remain hash-shuffled and non-anatomical.
- **Why.** Surface-only folds are hard to read when the optional surface is off;
  the neuron cloud has to carry the product's first impression. Keeping regions
  random avoids bundling a propagation/ambient-drive behavior change into a
  visual shape pass.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `crates/brain-visualizer/src/manifold/gyrify.rs →
  FoldField`; `crates/brain-visualizer/src/manifold/mod.rs →
  brain_outer_radius, folded_outer_radius, place_neurons`;
  `crates/brain-visualizer/src/manifold/regions.rs → assign_regions`.
- **Tradeoffs.** Folded placement changes spatial-grid occupancy and local
  connectivity density, so host tests and artifact metrics are part of the
  contract. The accepted pass kept `max_surface` and `max_neuron` below the
  frontend stimulation sphere radius.

## Region assignment by hash-shuffle, not spatial blocking

- **Decision.** Input (30%), Association (40%), and Output (30%) regions are
  assigned by shuffling neuron indices with a deterministic integer hash and
  slicing the result, producing a spatially random (non-contiguous) assignment.
- **Why.** The anterior–posterior spatial blocking originally proposed in
  `ANTERIOR_POSTERIOR_AXIS` is defined in the code but the `assign_regions`
  function does not use it. The hash-shuffle achieves the correct proportions
  with stable, reproducible results across any neuron count. The biological
  anterior–posterior gradient is expressed instead through the connectivity
  feed-forward bias owned by the simulation.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/manifold/regions.rs → assign_regions`;
  `crates/brain-visualizer/src/manifold/mod.rs → ANTERIOR_POSTERIOR_AXIS`
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
  descriptor first fork, and leaf sockets fixed.
  This replaces the earlier hand-tuned "shared trunk + 2-5 cluster branches +
  terminal twigs" three-tier fan. The dendrite tree, source-type bytes from
  region+seed, deterministic sockets, cubic-Bezier emission, per-branch `path_len`,
  and named segment budgets with slack are all retained.
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
  is consistent. The 48-byte
  `MorphSegment` layout remains the hard Rust ↔ WGSL contract — the tree ships
  inside it, with `radius_a`/`radius_b` carrying width and no new fields. The
  generator is also ~5× slower to initialize than the old fan (see the known
  limitation below).

## Branch smoothness uses bounded straight subdivision, not curved shader geometry

- **Decision.** Branch and long-range path smoothness is controlled by bounded
  subdivision knobs that change how many straight `MorphSegment` subsegments are
  emitted per generated hop. The shader still draws straight tapered tubes; no
  curved geometry segment type is added.
- **Why.** More polyline samples make turns progress gradually while preserving
  the existing 48-byte `MorphSegment` layout and render shader contract.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  adaptive_subsegments, GeneratorConfig::apply_to`; `web/src/core/morph-config.ts
  → MORPH_DESCRIPTORS`.

## Long-range waypoint routes stay inside the generated brain bounds

- **Decision.** Long-range axon leaf routes clamp generated waypoints, route
  endpoints, and route control points to a generation-time inner brain bounds
  helper before emitting straight subsegments.
- **Why.** Long projections should travel through the brain volume instead of
  escaping the silhouette. Clamping generated geometry is the durable fix;
  shrinking rendered tube radius would only hide the escape.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  BrainBounds, long_range_waypoints, generate`.

## Incoming dendrites from real reverse sockets, not decorative local trees

- **Decision.** Dendrites are generated from the target neuron's real incoming
  socket groups. The morphology builder stores every non-self raw
  `(source_id, synapse_index, target_id)` incoming synapse, aggregates duplicate
  `(source,target,socket)` records by weight for visible groups, and draws every
  unique group at the default N=1200/K=16 scale. Dendrite geometry is
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
- **Tradeoffs.** Shared roots/forks have multiple presynaptic owners, so v1
  leaves them structurally target-owned and not presynaptically active. Only
  source-specific leaves can pulse from the presynaptic source via existing
  `target_id`. Dense future scales must lower K or add an explicit cap policy;
  hidden visual sampling is not allowed. The older dead primary-min/span config
  fields were removed rather than revived; the live control vocabulary is
  socket placement plus root count, fork distance, curve tightness, branch
  thickness, taper, and group spacing.
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  build_incoming_view, emit_incoming_dendrites`; `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main`.

## Area-preserving √-width over literal-linear weight→width

- **Decision.** Branch radius encodes signal-carrying capacity via
  `radius = R_trunk · √(subtree_weight / total_weight)` for root/internal nodes,
  computed bottom-up after the tree is built (inhibitory weights enter as
  `unsigned_abs().max(1)`, floored at `R_trunk · twig_radius_fraction`). Terminal
  leaf nodes are set to the twig floor. The descriptor trunk carries 100% at full
  radius; each fork sheds its children's weight share.
- **Why.** Area-preserving √ scaling (Murray/Rall) keeps shared trunks visually
  substantial while still letting thin twigs read. The locked
  `axon_root_radius_fraction` is `0.90` so the primary trunk reads as a real
  trunk against the soma; terminal leaves go to the floor so single-target and
  low-degree arbors still taper instead of becoming constant-width hoses. The
  literal-linear "90% weight → 90% width" model the owner first proposed was
  rejected as too wispy — it collapses low-weight twigs to near-invisible
  threads and makes the trunk look anemic.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate`
  (width pass), `connectivity::weight`.
- **Alternatives considered.** Literal-linear weight fraction (rejected: wispy).
- **Tradeoffs.** Width is static — baked once at generation from synaptic weight;
  there is no runtime width path.

## Soft fork-degree penalty over a hard child cap

- **Decision.** The 2-3-children-per-fork tendency is a *soft* `tree_score_degree`
  term in the attach score (penalty monotonic in a node's current child count),
  not a hard cap. Relaxation then spreads siblings into fork-like geometry.
- **Why.** A soft penalty keeps the generator a single greedy pass with no
  synthetic-split bookkeeping, and lets occasional higher-degree forks happen
  where the geometry wants them rather than forcing artificial intermediate
  nodes everywhere.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate`
  (attach score), `MorphologyParams::tree_score_degree`.
- **Revisit when.** If review shows trunks visibly spraying 5+ ways, the known
  fallback is to convert `tree_score_degree` into a hard cap that synthesizes
  intermediate split nodes. The `cluster_count_histogram` (fork-degree, index
  5 = "5+") is the signal to watch.

## Adaptive edge subdivision over a single flat global subsegment count

- **Decision.** Edge tessellation is length- and curvature-aware:
  `adaptive_subsegments(edge_len, curvature, is_long_range, params)` returns
  `clamp(ceil(edge_len / max_len) + round(curvature · curvature_subsegment_boost),
  min_subsegments, edge_subsegments_max)`, with a *smaller* `max_len`
  (`long_range_max_segment_length`) for long-range hops than for local edges. The
  old flat `edge_subsegments` is kept only as a legacy/floor knob.
- **Why.** A single global subsegment count over-tessellates short local edges and
  under-samples long fibers, where a travelling pulse needs enough spatial samples
  to read as motion rather than a blinking span. Driving the count from already
  deterministic geometry (chord length + salt-seeded bend) keeps generation
  seed-reproducible while putting detail where curvature/length actually need it.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  adaptive_subsegments, MorphologyParams` (the `max_segment_length` /
  `long_range_max_segment_length` / `curvature_subsegment_boost` /
  `edge_subsegments_max` / `min_subsegments` fields).
- **Tradeoffs.** Per-edge segment counts now vary, so the buffer cap is sized for
  the worst case (`EDGE_SUBSEGMENTS_MAX`, and waypoint hops) rather than a flat
  multiply.

## Long-range axons route through deterministic bowed waypoints

- **Decision.** A leaf axon whose parent→socket chord exceeds
  `long_range_chord_cells · grid.cell_size` is emitted as `waypoints.len() + 1`
  bowed Bezier hops through 1–3 deterministic intermediate waypoints, each bowed
  outward from the brain centroid plus a salt-seeded lateral detour, instead of one
  span. Target identity and weight are unchanged — waypoints are visual route
  geometry only.
- **Why.** A single giant span reads as an unphysical wire crossing empty space;
  intermediate bowed waypoints make long axons read as biological projections
  curving around the volume.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  long_range_waypoints, LONG_RANGE_MAX_WAYPOINTS, MorphologyParams` (the
  `long_range_*` fields).
- **Tradeoffs.** Long leaves cost more segments (multiple hops); the per-target
  budget absorbs this via `(LONG_RANGE_MAX_WAYPOINTS + 1) · EDGE_SUBSEGMENTS_MAX`.

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
- **Tradeoffs.** Three decoration controls are dev-panel-exposed
  (`dendriteBranchletCount`, `dendriteTwigCount`, `dendriteDecorGroupMax`,
  runtime-clamped); the remaining decoration params stay locked.

## Known limitation: high-N init cost and segment-cap growth (graceful-degrade guard deferred)

- **Decision.** The Prim+relax generator ships without a "degrade gracefully at
  high N" guard; the default tier (N≈1200/K≈16) is the supported target.
- **Why.** The per-neuron init cost is ~5× the old fan — roughly 57ms → 281ms at
  N=1200/K=16 — because each arbor runs an O(leaves × nodes) greedy attach plus
  per-attach relaxation, and the per-arbor segment allocation cap grew ~1.5×.
  High-N tiers therefore initialize slowly and approach GPU buffer limits. The
  guard was scoped out to keep the rewrite to the generator alone; the cost is
  acceptable at default scale and the team chose to ship rather than block on it.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md),
  [`scaling.md`](scaling.md)
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

- **Decision.** Soma bodies are drawn via a dedicated `MorphSphereInstance` buffer (48 B, 16-aligned; one per neuron), emitted by `emit_soma_spheres` from existing neuron positions plus the host-side `ProcessRoot` descriptor. They are rendered by a separate `render_soma_spheres` sub-pass that reuses the same `last_spike` and `morph_uniform` buffers as the tube pass. `MorphSegment` remains the branch-only (48 B) contract; no `kind = 2` soma primitive was added to it.
- **Why.** Adding a `kind = 2` soma variant to `MorphSegment` (Q4=A from the plan) would either change the 48 B layout (breaking the Rust ↔ WGSL size assert) or repurpose fields like `b` as radius-only data in a non-obvious way. A separate buffer (Q4=C) keeps the branch contract clean, isolates the sphere geometry from the tube vertex shader, and lets the two sub-passes reuse shared uniforms without encoding the soma/branch distinction in a mixed struct.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md), [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Alternatives considered.** Keeping soma as the far-glow billboard only (Q4=B) — no layout change, but no true 3D body; a visible soma sphere body was the v0.3.0 deployment target.
- **Tradeoffs.** Two separate storage buffers and bind groups instead of one; the sphere bind group uses slots 3/4/5 to avoid WGSL name clashes with tube slots 0/1/2 in the shared shader module. The soma instance was widened in place to carry one dominant root direction and pull strength, avoiding a third soma deformation bind group.
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → MorphSphereInstance / emit_soma_spheres`; `crates/brain-visualizer/src/sim/gpu/pipelines.rs → GpuPipelines` (`render_soma_spheres` field); `crates/brain-visualizer/src/sim/gpu/resources.rs → init_morph_resources`

## Morphology generator exposed for tuning; budgets/slack/salts stay protected

- **Decision.** The generator shape fields of `MorphologyParams` (branch counts,
  reach, socket placement, radius/taper fractions, the tree-grammar knobs —
  `tree_score_*`, `relax_*`, the legacy floor `edge_subsegments` — plus three
  bushy-decoration controls `dendriteBranchletCount` / `dendriteTwigCount` /
  `dendriteDecorGroupMax`) are exposed as a tunable
  `MorphologyConfig` to the hidden dev panel, layered over the locked default at
  apply time. The four allocation/safety budgets (`dendrite_budget`,
  `trunk_cluster_budget`, `terminal_twig_budget`, `cap_slack`) and the `salt::*`
  hash constants are deliberately **not** exposed and are re-locked even when a
  config is applied.
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
