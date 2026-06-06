# Decisions — Scope

## Built from scratch — wgpu is a thin binding, not an engine

- **Decision.** The simulation and renderer are written from scratch against
  raw WebGPU (via wgpu). There is no game/graphics engine and no third-party
  neural-sim library; wgpu is used only as a thin, portable binding to the GPU.
- **Why.** The whole artifact *is* the dynamics-on-the-GPU and the bespoke LOD /
  glow / morphology rendering — an off-the-shelf engine would add a scene-graph
  abstraction we'd fight, and a sim library would hide the per-tick model we want
  to expose and tune. Owning the compute pipeline end-to-end is the point.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).

## The spiking neural network IS the chosen "engine"

- **Decision.** A spiking neural network (SNN) is the simulation substrate — not
  a rate model, not a particle system dressed up as neurons. The thing being
  visualized is genuine spike propagation through a synaptic network.
- **Why.** Spikes give discrete, causal, visible events (a neuron fires → its
  targets receive current → they may fire) that read as *brain activity* rather
  than abstract field animation. The avalanche/criticality dynamics we want only
  exist in a spiking, thresholded system. See
  [`dynamics.md`](dynamics.md).
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).

## LIF first; richer neuron models (GLIF) later

- **Decision.** The neuron model is point leaky-integrate-and-fire (single
  compartment, single state variable `v`). Generalized LIF (GLIF: adaptation
  currents, multiple time constants) is a deliberate future extension, not in the
  current model.
- **Why.** Point-LIF is the minimal model that produces threshold spiking,
  refractory dynamics, and criticality, and it costs one `v` per neuron — which is
  what lets ~10k neurons run every frame on the GPU. Heterogeneity (per-neuron
  threshold/leak/refractory spread) already buys much of GLIF's diversity without
  the per-neuron extra state. Add GLIF only when a demo needs adaptation.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → integrate`;
  `crates/brain-visualizer/src/sim/cpu/core.rs → LifParams`.
- **Revisit when.** A demonstration needs spike-frequency adaptation or bursting
  that heterogeneity + input modes cannot fake.

## Beauty-first MVP scale: ~10k neurons, K=16 — justified by visible dynamics, not neuron count

- **Decision.** The default network is ~10k neurons with out-degree K=16. The
  scale is chosen to make the *dynamics* legible and beautiful, not to maximize a
  neuron-count headline. Bigger tiers exist but the MVP target is "you can see
  individual cascades," not "N is large."
- **Why.** At this scale every neuron can be drawn with real morphology and every
  spike can propagate visibly within a frame budget, so the viewer reads cause and
  effect. Pushing N into the millions would force LOD aggregation that hides the
  per-neuron causality that makes it compelling — the visible dynamics are the
  product, the count is not.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md) (K).
- **Code anchors.** `crates/brain-visualizer/src/sim/backend.rs → SimConfig` (n, k defaults).
- **Tradeoffs.** Caps the "how big" bragging dimension in favor of the "how
  clear" one; the weight-normalization machinery (see [`dynamics.md`](dynamics.md))
  keeps dynamics comparable if K is later changed per tier.

## See also

- [`../architecture/simulation.md`](../architecture/simulation.md).
- [`dynamics.md`](dynamics.md) — SOC target, energy model, the dynamics knobs.
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md).
