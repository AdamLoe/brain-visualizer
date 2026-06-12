---
status:        draft
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/cpu-backend.md
  - architecture/web-frontend.md
  - architecture/build-and-deploy.md
  - decisions/backends.md
  - decisions/dev-tooling.md
---

# Stream C1: CPU Backend Retirement Gate

## Mission

Make the product honestly GPU-first without accidentally erasing a still-valued
CPU-vs-GPU comparison story. The default implementation path is to retire CPU
from production/runtime/default builds and put any remaining CPU code behind an
explicit dev/archive feature. Outright deletion requires owner confirmation.

## Scope

In scope:

- Remove CPU from default browser startup, runtime toggles, restart paths,
  public config assumptions, and default test/build expectations.
- If kept, gate CPU behind an explicit dev/archive feature and document that it
  is not part of V2 production behavior.
- Simplify CPU-only build assumptions only after confirming no remaining active
  path depends on them.

Out of scope:

- Deleting legacy render paths, ribbons, near-LOD, bloom tombstones, scaler
  stubs, telemetry, settings schema work, morphology scaling, or visual-region
  changes.

## Context Routes

- `docs/architecture/cpu-backend.md`
- `docs/decisions/backends.md`
- `docs/architecture/web-frontend.md`
- `docs/architecture/build-and-deploy.md`
- `docs/decisions/dev-tooling.md`
- `app/crates/brain-visualizer/src/sim/cpu/`
- `app/crates/brain-visualizer/src/lib.rs`
- `app/crates/brain-visualizer/Cargo.toml`
- `app/web/src/main.ts`
- `app/web/src/cpu/`
- `app/web/e2e/brain_visualizer.spec.ts`

## Approach

1. Audit the remaining CPU backend surface and classify each path as production,
   dev/archive, or dead.
2. Remove CPU from default product behavior and stale saved-config handling.
3. Either feature-gate/archive the CPU code or delete it if the owner confirms
   the CPU comparison showcase is no longer wanted.
4. Update tests, build docs, and backend decisions to describe the truthful
   state.

## Exit Gate

- `cd app && cargo test`
- `cd app/web && npm run typecheck`
- `cd app/web && npm test`
- `cd app/web && npm run build`
- `cd app/web && npm run test:e2e`
- Search gates prove no production UI/default path exposes CPU as a V2 backend.

## Handoff Notes

Owner decision required before outright deletion: delete CPU, keep it
feature-gated/archive-only, or revive it as a future comparison showcase.

## Migration Notes

At ship time, update `architecture/cpu-backend.md`,
`architecture/web-frontend.md`, `architecture/build-and-deploy.md`, and
`decisions/backends.md`.
