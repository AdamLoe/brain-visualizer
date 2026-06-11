---
status:        active
owner:         Hilbert
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/connectivity.md
  - architecture/manifold.md
  - decisions/connectivity.md
  - decisions/manifold.md
---

# Dendrites as real incoming synapses

> Implementation plan for user item 7. Chosen direction: a two-pass morphology
> build where outgoing synapses are first resolved all the way into their target
> somas, then each target soma aggregates its actual incoming synapses into a
> tighter dendrite arbor. Scale can come down first; efficiency comes later.

## Mission

Today dendrites are decorative: a local tree placed around the soma as "landing
context," not the cell's actual incoming connections. Axons go to real targets;
dendrites do not. The goal is to make morphology reflect the real directed
synapse relation in two passes:

1. For every source neuron, resolve its K outgoing synapses with the production
   `target` rule and route the axon geometry all the way to a socket/root near
   the target soma.
2. For every target soma, gather the synapses that terminate there and combine
   them into that neuron's dendrite arbor: tighter, closer branching around the
   soma, shaped differently from the outgoing axon trunk/tree but derived from
   the same real incoming synapses.

Done when dendrite tips/roots correspond to actual incoming synapses for that
soma, the axon-to-target-soma handoff is visible, and the first working version
can run at a reduced N/K if needed. Do not block the model on high-scale
efficiency; make it correct and beautiful first.

## The core problem

Connectivity is **source-out and implicit**: `target(i, j, …)` computes neuron
`i`'s K *outgoing* targets on demand; there is **no stored edge list and no reverse
map**. "Who connects to me" is exactly the information the system never
materializes. The new morphology needs a build-time reverse view of the same
real synapses:

- Each of N neurons emits K outgoing edges → N·K directed edges total.
- The incoming-synapse pass requires evaluating all N·K `target` calls once and
  bucketing by destination — an O(N·K) init pass plus storage for the incoming
  lists (variable in-degree; not the fixed K).
- It must stay **deterministic** and bit-consistent with the forward rule, and it
  must not perturb the locked hash/determinism gates.
- In-degree is unbounded/variable, unlike fixed out-degree K. The dendrite
  generator must aggregate incoming synapses into a bounded local arbor near the
  target soma instead of drawing a second full long-range wire for every incoming
  edge.
- The first version may reduce default N/K as much as needed to keep the full
  morphology readable and affordable. The later optimization problem is to scale
  this model back up, not to compromise the model before it exists.

## Approach

1. **Reverse synapse build.** During morphology generation, evaluate the
   production `target_with_cell` relation for every `(source, synapse_index)` and
   build a deterministic incoming list per target soma. Include source id,
   synapse index, target id, weight, and any target-socket/root placement data
   needed for the handoff. This is host-side geometry input, not a per-tick sim
   path, and it must not alter the forward target/hash determinism gates.
2. **Forward axon handoff.** Route each source neuron's axon tree to the real
   target soma sockets produced by the first pass. This keeps "where the axon
   goes" and "what the target soma receives" as the same synapse record, not two
   independent drawings.
3. **Incoming dendrite aggregation.** For each target soma, take its incoming
   synapse records and generate a tighter local dendrite arbor that combines
   nearby incoming roots. This should be shaped differently from the outgoing
   axon tree: closer to the soma, denser, more compact, and branchy rather than a
   second long-range layer. Cap or sample only if needed, and log dropped/capped
   incoming synapses explicitly.
4. **Segment ownership/activity semantics.** Decide how these new dendrite
   `MorphSegment`s encode `neuron_id` and `target_id` so rendering stays honest:
   the dendrite belongs geometrically to the target soma, but the incoming signal
   is caused by a presynaptic source firing. This is field ownership for the
   existing morphology renderer, not a separate visual layer. Prefer reusing
   existing `last_spike` data and the `target_id`/source-id fields before adding
   any new activity buffer.
5. **Scale-first defaults.** If the full version needs a lower first-pass
   default, lower N and/or K for this morphology mode. Capture the init-time,
   memory, segment count, and `Morphology::dropped` numbers so a later
   optimization pass knows what to recover.

## Exit gate

- `cargo test` green, **determinism gates unchanged and still passing** (the
  forward `target` rule and hash constants must not move).
- Build artifact reports reverse-build init time, memory, mean/p99/max in-degree,
  cap/sampling counts if any, total segment count, p99/max per-neuron segments,
  and `Morphology::dropped`.
- `examples/morph_view.rs` shows axons reaching target-soma sockets and the
  target soma's dendrite arbor combining those real incoming synapses into
  closer/tighter branches.
- Spot-check confirms a sample neuron's incoming dendrite records match the
  inverse of the forward `target` rule.
- If N/K defaults are lowered to ship the first correct version, document the new
  first-pass scale and the optimization target for later.
- `architecture/connectivity.md` (build-time incoming-synapse view),
  `architecture/manifold.md` (two-pass morphology generation), and
  `decisions/connectivity.md` / `decisions/manifold.md` updated;
  `_meta/ownership.json` gets the incoming-synapse morphology concept if it needs
  its own owner.

## Implementation result — 2026-06-09

Shipped v1 at the existing default review scale, N=1200/K=16. No N/K reduction
was needed.

- Reverse build uses production `target_with_cell` with precomputed source cell
  ids and production `weight`; connectivity target/hash logic was not edited.
- `Morphology` stores every non-self raw incoming synapse in
  `incoming_synapses`, with one `incoming_ranges` entry per target neuron.
- Duplicate `(source,target,socket)` records aggregate into
  `incoming_socket_groups` by summed absolute weight, with one
  `incoming_socket_group_ranges` entry per target neuron.
- Dendrite geometry is target-owned (`kind = 0`, `neuron_id = target_id`).
  Shared aggregate stems use `target_id = neuron_id`; source-specific terminal
  leaves use `target_id = source_id` and are emitted from socket inward.
- WGSL tube activity keeps color/material ownership on `neuron_id`, but
  source-specific dendrite leaves read activity from `last_spike[target_id]`.
  Shared stems do not presynaptically pulse in v1.

Default `morph_view` result:

- raw incoming: 17,850
- unique incoming socket groups: 13,010
- raw in-degree mean/p99/max: 14.88 / 49 / 86
- visible groups mean/p99/max: 10.84 / 29 / 46
- incoming capped/dropped: 0 / 0
- total segment count: 80,823
- segment cap: 199,200
- total dropped: 0

Artifacts:

- `/tmp/morph_0.rgba` through `/tmp/morph_3.rgba`
- `/tmp/morph_active_bright.rgba`
- `/tmp/morph_view_stats.json`
- `/tmp/morph_view_0.3.0_stats.json`
- `/tmp/morph_view_active_bright_stats.json`

Deferrals:

- Shared aggregate stems do not encode multiple presynaptic owners and therefore
  do not presynaptically pulse.
- No high-scale incoming cap/sampling policy was added; if future density is too
  high, lower K or design an explicit cap before hiding groups.

## Open questions

- Cap policy: draw all incoming, top-by-weight, spatially cluster then sample, or
  lower K until all incoming can be shown? The first implementation can lower K;
  hidden silent drops are not acceptable.
- Exactly where does the axon-to-dendrite handoff sit: target soma surface,
  target soma bulge/root, or a short incoming socket just outside the membrane?
- Segment activity semantics: should target-owned dendrite segments light from
  presynaptic `last_spike[target_id]`, from geometric owner `last_spike[neuron_id]`,
  or from a small source-id side channel if the current fields are insufficient?

## See also

- `architecture/connectivity.md` — forward `target`/`weight` rule, the thing being
  inverted; determinism gates.
- `architecture/manifold.md` — current decorative dendrite generator + segment budget.
- `decisions/connectivity.md` — why connectivity is implicit/source-out (the
  rationale this plan pushes against).
- `docs/plans/morphology-process-root-contract.md` — combined morphology budget
  gate and soma root handoff.
