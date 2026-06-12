---
status:        shipped
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/simulation.md
  - decisions/manifold.md
---

# Stream E: Visual Outcome Polish And Region Coherence Prototype

## Mission

Prototype spatially coherent region assignment so the natural wake-up can read
as posterior-to-anterior propagation instead of random speckle, without changing
default dynamics until review approves it. HUD truthfulness is owned by the
settings/metrics contract plan; this stream should only review final wording for
visual/product fit.

## Scope

In scope:

- A small opt-in prototype for spatially coherent Input / Association / Output
  regions.
- Visual and dynamics review against the current hash-random baseline.
- Review of public HUD wording for truthfulness if Stream B changes it.

Out of scope:

- Connectivity rule changes, LIF constants, excitability, input modes,
  weight normalization, CPU backend work, scaling, telemetry, dev-panel setting
  expansion, or promoting the prototype to default without review.

## Context Routes

- `docs/architecture/manifold.md`
- `docs/architecture/simulation.md`
- `docs/architecture/web-frontend.md`
- `docs/architecture/profiling.md`
- `docs/decisions/manifold.md`
- `docs/decisions/dynamics.md`
- `docs/decisions/interaction.md`
- `app/crates/brain-visualizer/src/manifold/regions.rs`
- `app/crates/brain-visualizer/src/manifold/mod.rs`
- `app/web/src/ui/hud.ts`
- `app/web/src/main.ts`
- `app/web/src/render/profiler.ts`
- `app/crates/brain-visualizer/src/sim/backend.rs`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs`

## Approach

1. Preserve current hash-random assignment as the default and regression
   baseline.
2. Add an opt-in internal/prototype region assignment mode using the existing
   anterior-posterior axis.
3. Preserve the 30/40/30 Input / Association / Output split, deterministic
   same-seed behavior, enum ordering, and type-byte encoding.
4. Avoid hard anatomical slabs. Prefer soft spatial coherence: posterior-biased
   input, anterior-biased output, association between them, and deterministic
   jitter/transition mixing.
5. Compare baseline and prototype with the same seed, default N/K, default
   visual/dynamics settings, and the first 5-20 seconds after startup.

## Exit Gate

- `cd app && cargo test` for manifold/sim changes.
- `cd app/web && npm run typecheck` if frontend/HUD code changes.
- Existing random/default region behavior remains deterministic.
- Prototype mode is deterministic and preserves 30/40/30 proportions within the
  existing tolerance.
- Tests cover empty input, proportion preservation, determinism, and posterior /
  association / anterior ordering.
- Visual review records baseline vs prototype screenshots or clips.
- Dynamics safety review records available `spikesPerSec`, `pctFired2s`, and
  `branchingRatio` evidence and flags saturation/runaway before promotion.

## Handoff Notes

No decision is needed to build the bounded prototype. A decision is needed to
promote the prototype to default or to add a public/dev-panel control for region
assignment.

## Migration Notes

Shipped as the bounded Rust-only prototype. Current-state facts were migrated
into `architecture/manifold.md` and `architecture/simulation.md`: default
region assignment remains hash-random, `ManifoldParams` has an opt-in
`RegionAssignmentMode::AnteriorPosteriorPrototype`, and the prototype is a
build-time assignment mode that does not retune connectivity, drive, or LIF
constants. The default-vs-prototype tradeoff was migrated into
`decisions/manifold.md`.

No web/HUD/profiling docs changed because this stream added no public UI,
settings contract, HUD wording, or metrics surface. Visual/dynamics promotion
review remains manual: the prototype is intentionally internal and not exposed
through the browser/dev panel, so screenshots or clips require a reviewer to
wire a temporary local call site or inspect a dedicated future harness before
changing the default.
