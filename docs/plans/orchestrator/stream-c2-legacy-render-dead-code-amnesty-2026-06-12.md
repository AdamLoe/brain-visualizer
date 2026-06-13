---
status:        active
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: false
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

- Retired ribbons, near-LOD spheres/cylinders, no-op brain-reset Apply flow,
  `connection_layer` mode 2, tombstoned bloom implementation, and scaler stubs
  only where removal does not retune active product scaling.

Out of scope:

- CPU backend retirement, telemetry, settings contract renumbering, morphology
  chunking, or product visual redesign.

## Context Routes

- `docs/architecture/gpu-rendering.md`
- `docs/architecture/active-edges.md`
- `docs/architecture/dev-panel.md`
- `docs/decisions/rendering.md`
- `docs/decisions/dev-tooling.md`
- Legacy anchors found by `rg "DRAW_LEGACY|EdgeBuffers|NearLod|bloom|connection_layer|adaptiveScaler|ApplyHandlers"`.

## Approach

1. Wait until B1 and D1 are not editing the same settings/render surfaces.
2. Remove one parked path at a time, preserving compatibility tombstones where
   settings indices require it.
3. Clamp or normalize persisted `connectionLayer = 2` rather than shifting
   settings indices.
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

At ship time, update `architecture/gpu-rendering.md`,
`architecture/active-edges.md`, `architecture/dev-panel.md`,
`decisions/rendering.md`, and `decisions/dev-tooling.md`.
