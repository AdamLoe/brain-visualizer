---
status:        shipped
owner:         adamg
last_updated:  2026-06-06
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - architecture/dev-panel.md
  - architecture/profiling.md
  - decisions/manifold.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
  - decisions/profiling.md
---

# Procedural Cell Rework Roadmap

## Mission

Stage the current morphology rework into versioned, shippable releases instead
of one all-or-nothing plan. The goal is still the same: replace the current
per-target radial axon curves with deterministic neurons that read as connected
cells, not isolated stars. The split below protects the biggest visual payoff
first: a shared axon arbor whose terminal twigs land near real target dendrites.
The active roadmap is deliberately v0.2.x only; richer cell-identity ideas have
been moved to [`future_roadmap.md`](future_roadmap.md) until the arbor itself
earns more complexity.

This file is the release map. Keep day-to-day stream status, artifact paths,
and human review decisions in
[`procedural-cell-rework-orchestration.md`](procedural-cell-rework-orchestration.md)
so this roadmap stays short enough to route a fresh chat.

Current package version is **0.2.1**. v0.2.0 shipped the morphology shape
change, and v0.2.1 shipped the narrow readability/tuning patch from that
baseline.

## Version Order

1. **v0.2.0 - Axon arbor foundation.**
   Shipped. The release delivered the settings/profiling-first path:
   morphology parameter surface and artifact stats, real source-type target
   resolution, deterministic branch grammar, shared trunks/cluster branches,
   terminal twigs for unique non-self targets, dendrite landing sockets, a hard
   segment budget, and one consolidated hidden dev-panel Morphology settings
   group. See
   [`procedural-cell-rework-0.2.0-axon-arbor.md`](procedural-cell-rework-0.2.0-axon-arbor.md).

2. **v0.2.1 - Dendrite readability and tuning patch.**
   Shipped. The release tuned the remaining close-up readability,
   width/brightness/taper, and review artifacts without changing the
   source-target, socket, branch-topology, or shader-layout contracts. See
   [`procedural-cell-rework-0.2.1-dendrite-tuning.md`](procedural-cell-rework-0.2.1-dendrite-tuning.md).

Deferred cell-identity polish, morph-pass soma work, whole-path upstream
lighting, inspect/select UX, and incoming-direction dendrite bias live in
[`future_roadmap.md`](future_roadmap.md), not in the active release chain.

## Migration notes

The durable v0.2.0 facts now live in the owning docs:

- `architecture/manifold.md` - locked morphology preset, source-type bytes,
  deterministic sockets, shared root/cluster branches, terminal twigs, and the
  48-byte contract.
- `architecture/gpu-rendering.md` - shared-segment `target_id` semantics and
  terminal-only upstream lighting for shared paths.
- `architecture/dev-panel.md` - Morphology UI grouping of the live render
  controls, with no schema/index drift.
- `architecture/profiling.md` - `morph_view` build/review stats and browser
  WASM timing behavior.
- `decisions/manifold.md` - shared arbor plus sockets over independent
  splines.
- `decisions/rendering.md` - terminal-only upstream lighting for the shared
  arbor.
- `decisions/profiling.md` - morphology review stats stay out of the always-on
  profiler.

## Cuts From The Old Plan

- No inspect / pick / single-neuron-select mode.
- No connectivity-rule change; the sim's `target`/`weight` hash rule remains
  authoritative.
- No Catmull-Rom in the first pass; use one cubic Bezier sampler.
- No incoming-direction dendrite bias until the simpler socket model is proven.
- No per-region morphology variation in the active v0.2.x roadmap.
- No `kind = 2` soma or Rust/WGSL layout change in v0.2.x.
- No public settings page or preset manager. Morphology controls land only in
  the hidden dev panel after defaults are accepted.
- No shader work to make shared trunk/cluster segments light as a whole upstream
  path. In v0.2.x, `light_past` may be terminal-only for shared arbors.
- No promise that every `j in 0..K` yields a distinct rendered terminal.
  Connectivity may duplicate targets or self-target; the visual contract is one
  terminal twig per **unique non-self target** resolved from the real rule.

## Shared Invariants

- Every random draw uses `connectivity::hash::{mix_key, hash32}` with
  morphology-range salts disjoint from connectivity salts.
- Morphology settings come first: tuning parameters live in one named config
  surface before branch grammar work begins, and artifacts record a config
  snapshot every time.
- Source-type accurate target resolution is a preflight correctness gate before
  visual grammar work.
- Rust/WGSL `MorphSegment` stays 48 bytes throughout v0.2.x.
- `target_id` semantics must be explicit for every segment kind. Shared trunk
  segments do not pretend to carry one target.
- Segment budget is a design input, not cleanup at the end.
- Dendrite sockets should be visible dendrite anchors/tips generated by the
  morphology grammar, not invisible math that terminal twigs merely stop near.
  If v0.2.0 takes a weaker socket fallback, call that out in the orchestration
  decision log and migration notes.
- `morph_view` screenshots and lightweight morphology stats are required
  acceptance artifacts for visual plans.
- Review artifacts use fixed seeds, camera views, and baseline/candidate naming;
  the orchestration hub records exact paths, config snapshots, stats/profile
  JSON, skips, and human decisions.
- The final UI step is consolidation, not exploration: accepted
  `dev-panel-candidate` settings are added together with impact metadata,
  persistence/defaults/ranges, and Rust/TS settings-boundary checks.

## Exit Gate

This roadmap is done when its versioned leaf plans have either shipped with
context migrated into the owning docs, or been abandoned and moved into
[`future_roadmap.md`](future_roadmap.md) / considered-and-rejected.

## Migration notes (filled in at ship time)

This file is a routing hub. Durable facts should migrate from the leaf plan that
actually ships, not from this roadmap.

## See also

- [`index.md`](index.md) - where live plans land.
- [`procedural-cell-rework-orchestration.md`](procedural-cell-rework-orchestration.md)
  - how to coordinate this work without agent/file collisions.
- [`procedural-cell-rework-0.2.0-axon-arbor.md`](procedural-cell-rework-0.2.0-axon-arbor.md)
- [`procedural-cell-rework-0.2.1-dendrite-tuning.md`](procedural-cell-rework-0.2.1-dendrite-tuning.md)
- [`future_roadmap.md`](future_roadmap.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../architecture/profiling.md`](../architecture/profiling.md)
