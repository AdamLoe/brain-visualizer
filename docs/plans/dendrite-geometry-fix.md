---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
  - decisions/rendering.md
---

# Dendrite geometry fix

> User goal 3: "Dendrites look horrific — they are either broken or horribly
> configured." **Supersedes the geometry portion of**
> [`dendrites-real-incoming-synapses.md`](dendrites-real-incoming-synapses.md),
> which shipped the reverse-synapse / target-owned dendrite arbor but with
> several concrete geometry defects.

## Mission

The incoming-dendrite system (target-owned arbors built from reverse synapses)
is structurally in place but its geometry reads as a collapsed, sub-visible,
kinked tangle on the soma instead of a legible dendritic tree. Done when: a
neuron's incoming dendrites read as a believable branching arbor reaching off
the soma at visible thickness, the dead/duplicated dendrite parameters are
either wired up or removed (coordinated with the dev-panel plan), and the
default look is accepted in a `morph_view` capture.

## Grounding — confirmed defects in `emit_incoming_dendrites`

All in [morphology.rs](../../app/crates/brain-visualizer/src/sim/morphology.rs),
`emit_incoming_dendrites` (~lines 1239–1374), unless noted:

1. **Branch points collapse onto the soma.** The aggregation point is clamped:
   `branch_distance = (min_socket_dist * 0.86).max(base_radius * 1.02).min(min_socket_dist * 0.96)`
   (~lines 1284–1287). With `base_radius = 0.006` and incoming sockets only
   ~0.007–0.015 away (after `axon_stop_fraction = 0.85`), the `.max(base_radius
   * 1.02 ≈ 0.0061)` floor pins the branch point essentially *inside* the soma
   for close sources → stubby, spaghetti-on-the-surface stems instead of a tree
   that reaches out.
2. **Stem-control-point hypothesis is stale.** Current implementation review
   found the old stem-control-point diagnosis does not match the corrected
   generator; collapsed placement and thin radii were the actionable geometry
   defects. Do not carry the old hypothesis into durable docs.
3. **Sub-visible radii.** Dendrite radii are `r_mid = base_radius * 0.6 ≈
   0.0036` and `r_tip = base_radius * 0.3 ≈ 0.0018` (~lines 1289–1290), then
   `.max(1e-4)` (~lines 1363–1364) — roughly half the socket radius and below
   the visible threshold at normal camera distance, so dendrites read as hair
   or vanish. (Cross-check against `connectionVisualWidth` and tube tessellation.)
4. **Dead / orphaned parameters.** `dendrite_reach_lo/hi` and
   `dendrite_primary_min/span` are defined, serialized, and exposed as dev-panel
   sliders but **never read** by the incoming-dendrite generator — leftovers from
   the replaced "primary dendrites" system. Tuning them does nothing. Decide:
   revive (make the new generator honour reach/primary-count) or remove.
5. **Always-blue, source-agnostic color.** Dendrites are hard-coded
   `vec3(0.22, 0.34, 0.5)` in
   [render_morphology.wgsl](../../app/crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl)
   (~lines 233–234) regardless of source E/I or region, so individual synaptic
   contacts are visually indistinguishable. Confirm this is intended vs a
   readability problem.
6. **Aggregation density.** Many incoming synapses sharing a `socket_idx`
   aggregate at nearly the same point → near-coincident branches (tangle). The
   shipped decision was "draw all unique socket groups at N=1200/K=16; lower K
   if too dense." Revisit if the tangle is a core part of "horrific."

Lighting/ownership semantics (`presynaptic_dendrite` branch keyed on
`target_id != neuron_id`, render_morphology.wgsl ~lines 461–468) appear correct
and are **not** the geometry problem — leave them unless the fix requires it.

## Scope

In scope: the dendrite geometry generation (branch-point placement, stem/leaf
Bézier shaping, radii/taper), the dead-parameter decision (revive vs remove),
and dendrite coloring if it's a readability blocker. The fix should make the
default look good without requiring slider tweaks.

Out of scope: axon trunk geometry, soma geometry, the connectivity/reverse-synapse
*build* (the `IncomingSocketGroup` data is fine — this is about turning it into
geometry), and the active-opacity model (→
[`active-opacity-continuous-model.md`](active-opacity-continuous-model.md)).

## Implementation status — 2026-06-09

Code is done for the geometry-only portion and durable docs have been migrated,
but the plan remains active because visual acceptance and the legacy-control
decision are still open. Gibbs changed only
`crates/brain-visualizer/src/sim/morphology.rs`: incoming branch points now use
weighted socket distance, maximum socket extent, deterministic lateral spread,
and a `1.65 * base_radius` off-soma floor; default dendrite taper is thicker
(`mid 0.95`, `tip 0.60`), with a `0.75` leaf weight floor. Reverse-synapse
grouping, target-owned semantics, segment grammar, cap formulas, segment
count/caps, and shader color are unchanged. Current `morph_view` facts:
`segment_count=80823`, `segment_cap=199200`, `dropped=0`, incoming raw/groups
`17850/13010`, incoming capped/dropped `0/0`. Reported gates: focused
morphology tests, `cargo test -p brain-visualizer`, and `morph_view` passed.

The legacy reach/primary-count controls have now been removed from the Rust
config structs, TypeScript config/defaults/descriptors, and architecture docs.
Live dendrite placement controls remain `socketCount*`, `socketRadius*`, and
`socketTipPreference`; old saved morphology payloads that still include the
removed keys are accepted and normalized away rather than breaking reload.

## Closure — 2026-06-11

Shipped. `morph_view` regenerated the default and close-up morphology artifacts
with 174,633 segments at N=1200/K=16, 0 dropped, and visible incoming-dendrite
branching. `render_check` passed the production render smoke and active/recent
compaction checks. Final gates passed: `cargo test -p brain-visualizer`,
`npm run typecheck`, `npm test`, and server-backed Playwright
`npm run test:e2e:server` (4 passed, 1 expected CPU-backend skip; WebGPU adapter
device assertions gated by the WSL2 environment).

## Approach

One stream — owned by `morphology.rs` (and the dendrite color line in
`render_morphology.wgsl` if touched). Per the hub's old discipline, only one
agent owns `morphology.rs` at a time; this plan should not run concurrently
with axon/soma generator work on the same file.

1. Capture the current bad state in `examples/morph_view.rs` (a close-up of one
   neuron's incoming arbor) as the before-shot.
2. Fix placement + shaping (defects 1, 2): branch points that reach off the
   soma, correct Bézier control points. This is the bulk of "horrific."
3. Fix radii/visibility (defect 3) so dendrites read at normal distance without
   over-thickening.
4. Resolve dead parameters (defect 4) **in coordination with**
   [`dev-panel-and-settings-overhaul.md`](dev-panel-and-settings-overhaul.md) —
   that plan explicitly deferred legacy dendrite-control removal to this plan.
   The landed decision was removal from `MorphologyParams`/`GeneratorConfig` and
   the descriptor table, with persisted old fields ignored.
5. Revisit color/density (defects 5, 6) only if still reading badly.
6. Re-capture `morph_view` for acceptance.

## Exit gate

- A `morph_view` close-up of one neuron's incoming dendrites reads as a legible
  branching arbor reaching off the soma at visible thickness — no collapsed
  stubs, no kinks, no vanished twigs. Before/after captures attached.
- The legacy dendrite reach/primary-count controls are fully removed from Rust +
  descriptors + docs, and old persisted payloads load without reviving them.
- `cd app && cargo test -p brain-visualizer` green (morphology, target
  determinism, `render_check`, `morph_view`); if `MorphSegment` layout changes,
  update the Rust↔WGSL contract atomically and the manifest drift notes.
- `architecture/manifold.md` (dendrite/incoming-socket generation) and
  `architecture/gpu-rendering.md` (dendrite sub-pass/coloring if changed)
  updated; `decisions/manifold.md` records the reach/primary-count decision.

## Migration notes (filled in at ship time)

Route the corrected dendrite-generation facts into `architecture/manifold.md`,
any shader/coloring change into `architecture/gpu-rendering.md`, and the
revive-vs-remove decision for the legacy dendrite parameters into
`decisions/manifold.md`. Confirm the predecessor
[`dendrites-real-incoming-synapses.md`](dendrites-real-incoming-synapses.md) is
closed out — its still-true facts (reverse-synapse build, target-owned
aggregation, stats) migrate; its geometry is replaced by this plan.

## See also

- [`dendrites-real-incoming-synapses.md`](dendrites-real-incoming-synapses.md) — superseded predecessor.
- [`dev-panel-and-settings-overhaul.md`](dev-panel-and-settings-overhaul.md) — owns the dendrite-parameter removal coordination.
- `architecture/manifold.md`, `architecture/gpu-rendering.md`,
  `decisions/manifold.md`, `decisions/rendering.md` — owning docs.
</content>
