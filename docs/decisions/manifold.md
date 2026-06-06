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

## Procedural per-neuron morphology geometry (no billboards-only)

- **Decision.** Each neuron is rendered as a real cell: a soma with a bushy
  dendrite tree (local, decorative) and a branching axon arbor that routes toward
  the neuron's actual synaptic targets. Geometry is a flat list of tapered line
  segments generated on the CPU once and uploaded to a static GPU buffer.
- **Why.** Billboard glows alone cannot convey directed signal flow. Giving each
  neuron a real axon arbor toward its actual targets — lit on spike (see
  [`rendering.md`](rendering.md)) — makes the brain's connectivity and activity
  pattern legible at a glance.
- **Applies to.** [`../architecture/manifold.md`](../architecture/manifold.md)
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs → generate, MorphSegment, max_segs_per_neuron`;
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl → vs_main`
- **Tradeoffs.** Static geometry means the axon arbor reflects the connectivity
  at initialization time; it does not update if connectivity were ever made
  dynamic. The 48-byte `MorphSegment` layout is a hard binary contract between
  Rust and WGSL — reordering fields silently corrupts the render.

## See also

- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
