---
status:        shipped
owner:         Codex
last_updated:  2026-06-13
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - architecture/active-edges.md
  - architecture/dev-panel.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
---

# Stream C2: Legacy Render Dead-Code Amnesty

## Mission

Remove or archive legacy render/dev paths that are documented as parked and no
longer part of the live V2 product, without colliding with active GPU scaling or
settings-contract work.

Owner decision on 2026-06-12: remove legacy/dead code and references outright.

## Scope

In scope after dependencies clear:

- Retired ribbon/close-body render branches, no-op brain-reset Apply flow,
  the inactive third connection-layer value, tombstoned bloom implementation,
  and scaler stubs only where removal does not retune active product scaling.

Out of scope:

- CPU backend retirement, telemetry, settings contract renumbering, morphology
  chunking, or product visual redesign.

## Context Routes

- `docs/architecture/gpu-rendering.md`
- `docs/architecture/active-edges.md`
- `docs/architecture/dev-panel.md`
- `docs/decisions/rendering.md`
- `docs/decisions/dev-tooling.md`
- Legacy render/dev anchors found by search across GPU resources, pipelines,
  shaders, settings, and dev-panel surfaces.

## Approach

1. Wait until B1 and D1 are not editing the same settings/render surfaces.
2. Remove one parked path at a time, preserving compatibility tombstones where
   settings indices require it.
3. Clamp or normalize the former third connection-layer value rather than
   shifting settings indices.
4. Update architecture and decisions docs after each coherent removal batch.

## Exit Gate

- `cd app && cargo test`
- `cd app/web && npm run typecheck`
- `cd app/web && npm test`
- `cd app/web && npm run build`
- `cd app && cargo run -p brain-visualizer --example render_check`
- Search gates show no live references to removed legacy paths and no stale docs
  describe them as active.

## Handoff Notes

Do not run this in parallel with D1 morphology segment scaling. Do not use this
stream to shift or repurpose settings indices.

## Migration Notes

Shipped on 2026-06-13. Removed retired ribbon/close-body GPU resources,
pipelines, shaders, native example coverage, no-op brain-reset Apply UI wiring,
and the inactive third connection-layer surface. Persisted settings above the
active range now normalize to active/recent without changing Float32Array
indices. Bloom stayed because the backend setter and `render_check` still
exercise that internal path. Updated `architecture/gpu-rendering.md`,
`architecture/active-edges.md`, `architecture/dev-panel.md`,
`architecture/gpu-backend.md`, `decisions/rendering.md`, and
`decisions/dev-tooling.md`, plus metadata/layout docs that held stale removed
path references.
