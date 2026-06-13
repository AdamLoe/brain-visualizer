---
status:        shipped
owner:         Codex
last_updated:  2026-06-13
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/cpu-backend.md
  - architecture/web-frontend.md
  - architecture/build-and-deploy.md
  - decisions/backends.md
  - decisions/dev-tooling.md
---

# Stream C1: CPU Backend Deletion

## Mission

Make the product honestly GPU-only by deleting the parked CPU/WebGL2 backend
and its default-build/runtime references. Owner decision on 2026-06-12: delete
CPU completely, not feature-gate/archive it.

## Scope

In scope:

- Remove CPU from browser startup, runtime toggles, restart paths, public config
  assumptions, default test/build expectations, Rust modules, WASM exports,
  worker code, examples, and docs.
- Simplify CPU-only build assumptions after confirming no remaining active path
  depends on them.

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
- Rust CPU simulation modules
- `app/crates/brain-visualizer/src/lib.rs`
- `app/crates/brain-visualizer/Cargo.toml`
- `app/web/src/main.ts`
- TypeScript CPU worker/renderer modules
- `app/web/e2e/brain_visualizer.spec.ts`

## Approach

1. Audit the remaining CPU backend surface.
2. Delete CPU Rust modules/exports, TypeScript worker/render/coordinator code,
   CPU restart/config paths, CPU examples, CPU-only tests, and build feature
   dependencies.
3. Normalize stale saved `backend: "cpu"` values to GPU or ignore the field
   without keeping CPU code alive.
4. Update tests, build docs, architecture, manifest references, and backend
   decisions to describe the GPU-only state.

## Exit Gate

- `cd app && cargo test`
- `cd app/web && npm run typecheck`
- `cd app/web && npm test`
- `cd app/web && npm run build`
- `cd app/web && npm run test:e2e`
- Search gates prove no production UI/default path exposes CPU as a V2 backend.

## Handoff Notes

Owner decision is resolved: delete CPU completely. Git history is the archive.

## Migration Notes

Migrated:

- `architecture/cpu-backend.md` now records the current GPU-only state, removed
  CPU runtime surface, stale-config normalization, and no threaded-WASM lane.
- `architecture/web-frontend.md` documents `loadConfig()` normalizing old CPU
  backend saves and the single `WasmGpuBackend` runtime.
- `architecture/build-and-deploy.md` removes CPU-thread build instructions and
  CPU parity/e2e expectations from the current build/test surface.
- `decisions/backends.md` records the owner decision to delete rather than
  feature-gate/archive the retired backend.
- Supporting routing/docs were updated where they claimed CPU was live or
  parked: overview, architecture index, repository layout, ownership/manifest,
  simulation/data/connectivity/scaling docs, and testing/dev-loop guidance.
