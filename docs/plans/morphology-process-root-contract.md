---
status:        shipped
owner:         unassigned
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
---

# Morphology process-root and socket contract

> Coordination preflight for the axon-trunk, organic-soma, and real-incoming
> dendrite plans. This is not a separate visual feature; it defines the shared
> root/socket data and budget contract those plans must use so agents do not
> collide in `morphology.rs` and `render_morphology.wgsl`.

## Mission

Define one deterministic source of truth for each neuron's near-soma process
roots and incoming sockets: at minimum the dominant axon-trunk direction and
radius/weight, the soma surface root point, and the target-soma socket/root data
that the incoming-dendrite pass aggregates. Done when an implementer can build
the trunk, deform the soma, and hand incoming synapses into dendrites from the
same generated data, without adding a shader-side search over `MorphSegment` or
inventing a second root/socket convention.

## Scope

In scope:

- **Dominant process root descriptor.** During morphology generation, compute the
  trunk/root direction that leaves the soma, its root radius, and the first-fork
  position. The axon plan emits branch geometry from it; the soma plan uses the
  same direction to stretch/bulge the soma.
- **Incoming socket descriptor.** During the two-pass synapse build, compute the
  deterministic target-soma socket/root where each real incoming synapse lands.
  The outgoing axon handoff and the target soma's dendrite aggregation must refer
  to the same socket record, not two separately sampled points.
- **Soma data path decision.** Prefer baking the dominant root descriptor into
  the soma instance data (either by widening `MorphSphereInstance` or by adding a
  small parallel soma-deformation buffer). Do not make `vs_sphere` scan
  `MorphSegment` for near-soma roots; that adds indexing complexity to the render
  path and makes ownership unclear.
- **Combined segment-budget gate.** Before axon curves or real incoming
  dendrites ship, record the active N/K, segment counts, p99/max per-neuron
  segments, cap, `Morphology::dropped`, and expected multiplier from added axon
  and dendrite subsegments. Lowering N/K is acceptable for the first correct
  version; silent segment drops are not.
- **Ordering.** Active-opacity can ship independently. Axon trunk/curves and
  organic soma must share this contract. Real incoming dendrites can follow once
  the same socket/root convention exists.

Out of scope: the final soma aesthetic, the exact trunk length tuning, and the
reverse-connectivity optimization work.

## Approach

1. Add a small root-descriptor construction step inside
   `crates/brain-visualizer/src/sim/morphology.rs → generate` alongside the axon
   tree root calculation. Keep all deterministic draws in the existing
   morphology salt namespace.
2. Choose and document the data path. Default recommendation: widen
   `MorphSphereInstance` only if the required fields stay compact and
   16-aligned; otherwise use a parallel deformation buffer with one record per
   soma. Either way, update Rust and WGSL together and keep size asserts.
3. Add the budget snapshot to the implementation artifact output, preferably via
   `MorphologyStats`, so review agents do not scrape logs.
4. Add incoming socket/root records to the same generation stage when the
   dendrite plan runs, so outgoing axons and target-owned dendrites meet at one
   deterministic handoff.
5. Update the axon, soma, and dendrite plans to consume this contract rather than
   carrying their own root/socket assumptions.

## Host-side foundation shipped

Implemented in `app/crates/brain-visualizer/src/sim/morphology.rs`:

- `Morphology::process_roots` now contains exactly one `ProcessRoot` per source
  neuron. The descriptor records `neuron_id`, source soma center, dominant root
  direction, soma-surface root point, first-fork point, root radius, summed root
  weight, and unique target count.
- The dominant root direction is derived from the already-resolved unique axon
  socket positions. Empty arbors use deterministic fallback draws from the
  existing morphology salt namespace. This does not participate in
  `target_with_cell`, target deduping, or weight selection.
- The current axon tree consumes `ProcessRoot::soma_root` and
  `ProcessRoot::first_fork` as its root and trunk seed. Single-target axons now
  use the same descriptor-backed root edge as fan-out axons, followed by one real
  terminal edge; the real terminal target remains the existing
  `target_with_cell` result.
- `MorphologyStats` now reports the budget gate fields needed by downstream
  plans: `neuron_count`, `fanout_k`, `segment_cap_per_neuron`,
  `segments_per_neuron_p99`, and `segments_per_neuron_max`, alongside existing
  total cap, byte cap, utilization, and dropped-count fields. `morph_view`
  artifact JSON already includes these via `morphology_stats`.
- No soma deformation, `MorphSphereInstance` widening, incoming socket records,
  shader changes, or WGSL layout changes are part of this foundation step.

## Exit gate

- The axon and soma plans point here for their shared root/socket contract.
- A single root-direction convention is documented: source neuron position,
  soma surface root point, dominant trunk direction, root radius/weight, and
  first-fork point.
- A single incoming socket convention is documented: target soma, incoming source
  id/synapse index, socket point/root direction, and any weight/radius used by
  dendrite aggregation.
- The chosen soma data path is explicit, with the Rust↔WGSL layout risk called
  out if a buffer/instance layout changes.
- A combined segment-budget acceptance gate exists before any plan that adds
  subsegments can ship.

## Closure — 2026-06-11

Shipped. The downstream axon, soma, and incoming-dendrite plans have all consumed
the process-root/socket contract and are closed. The contract is documented in
`architecture/manifold.md`, `architecture/gpu-rendering.md`, and
`decisions/manifold.md`; final Rust, web, native render, and Playwright gates
passed for the integrated state.

## See also

- `docs/plans/axon-trunk-and-root-like-branches.md`
- `docs/plans/organic-soma-redesign.md`
- `docs/plans/dendrites-real-incoming-synapses.md`
- `architecture/manifold.md` — morphology generator and layout contracts.
- `architecture/gpu-rendering.md` — tube and soma render sub-passes.
