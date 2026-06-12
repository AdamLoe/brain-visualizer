---
status:        active
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/build-and-deploy.md
  - architecture/gpu-backend.md
  - architecture/gpu-rendering.md
  - architecture/manifold.md
  - architecture/web-frontend.md
---

# Stream D: Rebuild Responsiveness

## Mission

Keep the browser responsive during startup, tier/N/K rebuilds, and morphology
generator rebuilds by moving pure CPU preparation off the main thread where
feasible and replacing monolithic rAF-time rebuild calls with staged,
latest-wins orchestration. WebGPU device and surface ownership remains on the
main thread.

## Scope

In scope:

- Replace monolithic `gpuBackend.reinitialize(...)` from the rAF mutation flush
  with staged rebuild orchestration and progress.
- Add a dedicated build worker for pure network preparation: manifold surface,
  neuron placement, spatial grid, morphology segments, soma instances, and build
  stats.
- Add a versioned prepared-network payload between worker WASM and main-thread
  WebGPU upload.
- Route tier/N/K changes, morphology generator changes, and reach/curve changes
  through async rebuild on wasm.
- Preserve the current `WasmGpuBackend` mutable-access discipline.

Out of scope:

- Moving WebGPU rendering to a worker, CPU/WebGL2 fallback work, telemetry,
  settings schema redesign, region aesthetics, or simulation/connectivity
  semantic changes.

## Context Routes

- `docs/architecture/web-frontend.md`
- `docs/architecture/gpu-backend.md`
- `docs/architecture/manifold.md`
- `docs/architecture/gpu-rendering.md`
- `docs/architecture/build-and-deploy.md` if worker packaging changes.
- `app/web/src/main.ts`
- `app/crates/brain-visualizer/src/lib.rs`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs`
- `app/crates/brain-visualizer/src/manifold/mod.rs`
- `app/crates/brain-visualizer/src/connectivity/spatial.rs`
- `app/crates/brain-visualizer/src/sim/morphology.rs`

## Approach

1. Add a frontend rebuild coordinator before adding the worker. Mirror startup
   staging, yield between upload stages, and keep "latest request wins" state.
2. Define a GPU-agnostic prepared-network contract for pure build output:
   manifold,
   positions, regions, spatial grid CSR, morphology segments, soma instances,
   stats, and config/reach metadata. Validate version, N/K/seed/config,
   lengths, region codes, grid lengths, and layout sizes.
3. Add `web/src/gpu-build/network-build-worker.ts` or equivalent. The worker
   owns a worker-local WASM instance, performs only pure CPU preparation, sends
   progress, and returns transferable prepared payloads. It must never request
   WebGPU or touch the canvas.
4. Route structural setting changes away from synchronous generation. Lighting
   and uniform-only changes can stay immediate; generator-affecting changes
   schedule async preparation.
5. Document the rebuild state machine and prepared-build/upload boundary.

## Exit Gate

- `cd app && cargo test`
- `cd app/web && npm run typecheck`
- `cd app/web && npm test`
- Host tests for prepared payload validation and deterministic equivalence with
  the current direct build path.
- Frontend tests for latest-wins behavior, stale result rejection, worker
  failure state, and post-rebuild settings/morph-config re-push.
- Browser smoke or Playwright evidence that the frame/startup counter continues
  advancing while a high-N prepare job runs in the worker.
- Evidence that browser rAF paths no longer run synchronous
  `reinitialize()`/`regenerate_morphology()` for structural changes.

## Handoff Notes

Wave 1 can start before segment chunking lands. Worker payloads should stay
flat and GPU-agnostic; chunking is a main-thread WebGPU upload policy. Worker
upload integration should follow D1 unless implementation proves the payload
contract remains independent. High-conflict files: `web/src/main.ts`, `lib.rs`,
`sim/gpu/mod.rs`, and `resources.rs`.

## Status Notes

- Wave 1 frontend coordinator groundwork is landed in
  `web/src/rebuild/rebuild-coordinator.ts` and wired from `web/src/main.ts`.
  DOM/dev-panel handlers now enqueue latest-wins network/settings/morphology
  rebuild requests, and the rAF loop applies at most one rebuild-related backend
  mutation per frame.
- The coordinator still delegates to the existing main-thread wasm methods
  (`reinitialize`, `update_settings`, `set_morphology_config`). No build worker,
  prepared-network payload, Rust upload API, or morphology chunking has been
  added.
- Focused frontend coverage lives in
  `web/src/rebuild/rebuild-coordinator.test.ts` for latest-wins coalescing,
  staged post-rebuild pushes, and newer-network preemption of queued follow-up
  work.
- Wave 2 payload/upload checkpoint is landed. `PreparedNetworkBuild` defines a
  versioned GPU-agnostic flat payload for manifold, placement, spatial-grid CSR,
  morphology segments, soma instances, and metadata; host tests validate
  round-trip reconstruction and bad region-code rejection. `GpuResources` now
  has `init_morph_resources_from_prepared`, so worker-prepared morphology enters
  the same main-thread chunked upload/resource path as direct generation.
- `web/src/gpu-build/network-build-worker.ts` owns a worker-local WASM instance
  and prepares N/K/seed network rebuild payloads without requesting WebGPU.
  `NetworkBuildClient` rejects stale ready/failure messages by sequence, and
  `main.ts` applies only the latest ready payload from rAF via
  `WasmGpuBackend::apply_prepared_network`.
- Remaining D2 work: startup still uses main-thread staged `startup_build_manifold`
  / `startup_upload_morphology`, and standalone morphology generator rebuilds
  still call `set_morphology_config(json)` on the main thread. A dedicated
  frame-counter/high-N worker responsiveness smoke is still deferred.

## Migration Notes

Wave 2 current-state facts were migrated into `architecture/web-frontend.md`,
`architecture/gpu-backend.md`, `architecture/manifold.md`, and
`architecture/build-and-deploy.md`. Keep the plan active until startup and
standalone morphology rebuild preparation are moved off the main thread or
explicitly deferred by the owner.
