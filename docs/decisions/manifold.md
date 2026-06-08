# Manifold decisions

## Procedurally folded brain surface (gyri/sulci), no external mesh assets

- **Decision.** The cortical surface is generated entirely in code via icosphere
  subdivision followed by two-octave simplex noise gyrification; no external mesh
  files are loaded or bundled. Neuron count may be reduced to preserve the
  brain-like visual shape.
- **Why.** A flat patch or smooth blob does not read as a brain to a visitor. The
  visual recognizability — folded gyri and sulci — is the point of the project.
  Procedural generation keeps the repo asset-free, makes the surface deterministic
  and seed-reproducible, and lets the subdivision level be tuned at runtime.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`;
  `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, gyrify`;
  `crates/brain-visualizer/src/manifold/mod.rs → ManifoldParams`
- **Tradeoffs.** The simplex noise approach produces plausible but not
  anatomically accurate sulcal geometry. That is acceptable — the goal is
  recognizable impression, not neuroscience fidelity.
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

## Shared arbor, not K independent splines

- **Decision.** Each neuron is rendered as a real cell with a soma, a local
  dendrite tree, a deterministic shared trunk/root, 2-5 cluster branches, and
  one terminal twig per unique non-self target. The generator uses a locked
  morphology preset plus build stats, source-type bytes derived from region+seed,
  deterministic sockets, and named per-kind segment budgets with slack. The
  branch grammar is cubic Bezier, not a sin-bow or Catmull-Rom.
- **Why.** Independent per-target splines read like unrelated sticks; they do
  not show shared structure or terminal identity. A shared arbor with visible
  sockets gives the brain connected directional grain while still making the
  actual target terminals legible. Using the same source-type bytes as
  production connectivity keeps the morphology honest.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate,
  MorphologyParams::locked_default, MorphologyStats, MorphSegment,
  max_segs_per_neuron`; `crates/brain-visualizer/src/connectivity/hash.rs →
  mix_key, hash32`; `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main`
- **Tradeoffs.** The shared trunk/cluster segments do not carry a distinct real
  target id; they use the source id and defer upstream lighting to the terminal
  twigs. The 48-byte `MorphSegment` layout remains the hard binary contract
  between Rust and WGSL.

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
  reach, socket placement, radius/taper fractions) are exposed as a tunable
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
