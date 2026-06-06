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

## See also

- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
