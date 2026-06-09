---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    true
owning_docs:
  - plans/*
  - architecture/*
  - decisions/*
---

# Future Roadmap

Long-lived parking lot for deferred work, rejected ideas, and follow-up routes
that should not distract the active v0.3-v0.5 visual roadmap. Active
implementation details belong in the versioned plan docs, not here.

## Deferred candidates

### Real Mesh Brain Asset

- **Deferred.** Do not make external mesh acquisition part of v0.3.0.
- **Why.** The current accepted direction keeps the manifold procedural and
  asset-free for the active roadmap.
- **Revisit when.** A licensed mesh route is explicitly chosen as a new product
  decision after the procedural showcase is stable.
- **Route.** Would require license verification, attribution, source + runtime
  asset handling, mesh normalization, and an explicit replacement of the current
  no-external-mesh manifold decision.

### Soft Spatial Region Territories

- **Deferred.** Spatial Input/Association/Output geography is out of the v0.5
  critical path.
- **Why.** Hard bands previously read as slabs and fought the existing dynamics
  decision that region topology is functional, not anatomical.
- **Revisit when.** The showcase is stable and visual propagation still needs
  more directional structure.
- **Route.** Draft a new implementation plan after v0.5. Use soft
  probabilistic territories with deterministic jitter and preserved global
  ratios. Do not ship hard posterior/middle/anterior slabs.
- **Constraints.** Preserve approximate 30/40/30 global ratios, keep every
  visible zone mixed, keep `RegionKind` integer ordering stable, and treat the
  change as a dynamics review because input-region ambient drive moves with
  region geography.
- **Review.** Check region-color views, spikes/sec, branching ratio,
  silent/tuned/overactive classification, hover response, and low/balanced/max
  tier behavior. If the only working version requires hard bands, abandon it.

### External Texture Asset Pipeline

- **Deferred.** v0.3.2 stays procedural and subtle; no PNG/JPEG texture uploads,
  UV unwraps, sampler bind groups, or asset pipeline.
- **Revisit when.** Procedural material polish fails close-up review and the
  extra complexity has a clear visual payoff.

### Public Visual Presets

- **Deferred.** Keep default/performance/hero variants as review-harness or
  hidden-dev-panel concepts first.
- **Why.** The public product should open to the accepted default without asking
  visitors to choose a rendering mode.
- **Revisit when.** Real users need a public low-power mode or screenshot mode.

### Click-To-Inspect / Picking

- **Deferred.** Selection, per-neuron inspection, incoming/outgoing highlighting,
  and GPU picking remain outside the visual-roadmap critical path.
- **Why.** The interaction model is still a watch-and-perturb toy; click does
  nothing by design.

### Education Mode

- **Deferred.** Labels, lessons, anatomical overlays, and explanatory sequences
  are not part of the v0.3-v0.5 work.

### Sim-Accurate Conduction Delay

- **Deferred.** v0.3.3 traveling impulses are visual-only and derived from
  `last_spike` plus `path_len`; they do not delay synaptic delivery.

### Learning / Plasticity

- **Deferred.** STDP or any learning rule would change the simulation model and
  needs a separate dynamics plan.

### CPU Backend Revival

- **Deferred.** The CPU/WebGL2 backend stays parked during visual showcase work.

### High-N Morphology Degradation Guard

- **Deferred.** The branching-tree morphology generator
  (`crates/brain-visualizer/src/sim/morphology.rs → generate`) is tuned for the
  beauty-first default (~1.2k neurons) and has no high-N relief valve.
- **Why it matters.** Per-neuron build cost is ~5× the old fan (≈57→281 ms at
  N=1200/K=16) and grows with N, and the segment allocation cap grew ~1.5×, so
  high tiers pay a long one-time `initialize()` and approach the GPU
  storage-buffer ceiling — observable as "the app hangs on start" when persisted
  N/K is high.
- **Route.** Above an N or segment-budget threshold, degrade gracefully: drop
  `edge_subsegments` toward 1, simplify or skip the local relaxation pass, and/or
  cap `max_segs_per_neuron` harder — keeping high tiers interactive rather than
  blocking. The branching-tree plan called for this ("degrade gracefully at high
  N") but it was not implemented when the plan shipped.
- **Revisit when.** High-N tiers become part of the showcase, or the default
  scale is raised.

## Rejected for the active roadmap

- **Hard spatial region bands before v0.5.** Rejected because they risk the
  "three glowing slabs" failure mode.
- **Real mesh as the default v0.3.0 path.** Rejected for now because it turns a
  visual-shape pass into licensing, asset processing, and point-in-mesh work.
- **User-facing hero/performance/default presets in v0.4.0.** Rejected until
  review harness configs prove that separate public modes are worth exposing.

## See also

- [`00-roadmap-index.md`](00-roadmap-index.md)
- [`visual-acceptance-contract.md`](visual-acceptance-contract.md)
- [`v0.3.0-brain-shaped-arena.md`](v0.3.0-brain-shaped-arena.md)
- [`../decisions/manifold.md`](../decisions/manifold.md)
- [`../decisions/dynamics.md`](../decisions/dynamics.md)
