# Manifold decisions

## Procedural brain-shaped shell and shell-biased placement, no external mesh assets

- **Decision.** The cortical surface is generated entirely in code via icosphere
  subdivision, a shared deterministic brain-envelope shaping pass, and
  multi-octave simplex-noise gyrification; no external mesh files are loaded or
  bundled. Neuron placement uses that same envelope and is biased toward the
  cortical shell rather than filling a uniform sphere.
- **Why.** A flat patch or smooth blob does not read as a brain to a visitor. The
  visual recognizability — silhouette first, folds second — is the point of the
  project. Using one host-side envelope for both shell generation and placement
  keeps the mesh and neuron cloud in agreement, while still preserving the
  asset-free, deterministic, seed-reproducible generation route.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`;
  `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, gyrify`;
  `crates/brain-visualizer/src/manifold/mod.rs → ManifoldParams, brain_outer_radius, place_neurons`
- **Tradeoffs.** The simplex noise approach produces plausible but not
  anatomically accurate sulcal geometry, and the shell-biased placement is still
  a stylized cortical volume rather than a real anatomical tissue model. That is
  acceptable — the goal is recognizable impression, not neuroscience fidelity.
- **Revisit when.** A specific gyral atlas is required for region-accurate
  stimulation experiments, or when a real mesh can be streamed without bundling
  a large asset.

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
  attach plus local relaxation**: leaves are the unique non-self targets, each
  greedy step adds the globally-best `(leaf, attach-point)` edge (which may split
  an existing edge into a shared internal node), and a local ancestor-window
  pass relaxes internal nodes while holding the soma root and leaf sockets fixed.
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
  source-lit); only leaf edges carry the real target id. The 48-byte
  `MorphSegment` layout remains the hard Rust ↔ WGSL contract — the tree ships
  inside it, with `radius_a`/`radius_b` carrying width and no new fields. The
  generator is also ~5× slower to initialize than the old fan (see the known
  limitation below).

## Area-preserving √-width over literal-linear weight→width

- **Decision.** Branch radius encodes signal-carrying capacity via
  `radius = R_trunk · √(subtree_weight / total_weight)`, computed bottom-up after
  the tree is built (inhibitory weights enter as `unsigned_abs().max(1)`, floored
  at `R_trunk · twig_radius_fraction`). The trunk carries 100% at full radius;
  each fork sheds its children's weight share.
- **Why.** Area-preserving √ scaling (Murray/Rall) keeps shared trunks visually
  substantial while still letting thin twigs read. The literal-linear
  "90% weight → 90% width" model the owner first proposed was rejected as too
  wispy — it collapses low-weight twigs to near-invisible threads and makes the
  trunk look anemic.
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

- **Decision.** Soma bodies are drawn via a dedicated `MorphSphereInstance` buffer (32 B, 16-aligned; one per neuron), emitted by `emit_soma_spheres` from existing neuron positions. They are rendered by a separate `render_soma_spheres` sub-pass that reuses the same `last_spike` and `morph_uniform` buffers as the tube pass. `MorphSegment` remains the branch-only (48 B) contract; no `kind = 2` soma primitive was added to it.
- **Why.** Adding a `kind = 2` soma variant to `MorphSegment` (Q4=A from the plan) would either change the 48 B layout (breaking the Rust ↔ WGSL size assert) or repurpose fields like `b` as radius-only data in a non-obvious way. A separate buffer (Q4=C) keeps the branch contract clean, isolates the sphere geometry from the tube vertex shader, and lets the two sub-passes reuse shared uniforms without encoding the soma/branch distinction in a mixed struct.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md), [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- **Alternatives considered.** Keeping soma as the far-glow billboard only (Q4=B) — no layout change, but no true 3D body; a visible soma sphere body was the v0.3.0 deployment target.
- **Tradeoffs.** Two separate storage buffers and bind groups instead of one; the sphere bind group uses slots 3/4/5 to avoid WGSL name clashes with tube slots 0/1/2 in the shared shader module.
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → MorphSphereInstance / emit_soma_spheres`; `crates/brain-visualizer/src/sim/gpu/pipelines.rs → GpuPipelines` (`render_soma_spheres` field); `crates/brain-visualizer/src/sim/gpu/resources.rs → init_morph_resources`

## Morphology generator exposed for tuning; budgets/slack/salts stay protected

- **Decision.** The generator shape fields of `MorphologyParams` (branch counts,
  reach, socket placement, radius/taper fractions, and the tree-grammar knobs —
  `tree_score_*`, `relax_*`, `edge_subsegments`) are exposed as a tunable
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
