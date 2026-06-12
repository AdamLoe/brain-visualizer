---
status:        shipped
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - architecture/simulation.md
  - architecture/profiling.md
  - decisions/dev-tooling.md
  - decisions/profiling.md
---

# Stream B1: Settings And Metrics Contract Gates

## Mission

Prevent silent Rust / TypeScript / WGSL contract drift in the existing settings
and metrics channels. The runtime boundary stays unchanged in this stream:
`VisualSettings` remains the 26-slot `Float32Array`, and metrics remain the
33-float array. Done means tombstoned settings slots, default-written slots,
metrics indices, and Rust/WGSL uniform layouts are protected by executable
tests, and stale comments no longer misstate the contract.

## Scope

In scope:

- Harden `web/src/core/settings.ts`: `SETTINGS_LENGTH`, `toFloat32Array`,
  `METRICS_LAYOUT`, and `parseMetrics`.
- Harden Rust mapping in `VisualSettings::from_slice` and `metrics_snapshot`.
- Add layout checks for `IntegrateUniforms`, `ConnectUniforms`, and
  `MetricsUniforms`.
- Add golden or manifest-backed tests for settings tombstones 9, 10, 16, and
  23, and quarantined/default-written slots 1, 11, and 20.
- Label HUD/dev-panel metrics that are estimated rather than measured.

Out of scope:

- Replacing `VisualSettings` with JSON, CPU retirement, morphology config
  expansion, morphology scaling, worker generation, telemetry, or region tuning.

## Context Routes

- `docs/architecture/dev-panel.md`
- `docs/architecture/simulation.md`
- `docs/architecture/profiling.md`
- `docs/decisions/dev-tooling.md`
- `docs/decisions/profiling.md`
- `docs/agent-context/testing-how-to.md`
- `app/web/src/core/settings.ts`
- `app/web/src/ui/dev-panel.test.ts`
- `app/web/src/render/profiler.ts`
- `app/web/src/ui/hud.ts`
- `app/crates/brain-visualizer/src/lib.rs`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs`
- `app/crates/brain-visualizer/src/sim/gpu/resources.rs`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl`

## Approach

Add contract tests on both sides of the boundary. Prefer a small
machine-readable golden contract consumed by TypeScript and Rust tests if it
stays compact; duplicated explicit golden tables are acceptable only if shared
contract plumbing would dominate the change.

Settings gates must assert full default array values, sentinel behavior for
tombstones/default-written slots, Rust field mapping, length-tolerant defaults,
and TypeScript/Rust default agreement. Metrics gates must assert scalar count,
total length, scalar order, histogram offset, and Rust snapshot order. Uniform
layout gates should assert exact current sizes, updating docs and tests together
if the implementation discovers different exact sizes.

HUD/dev-panel wording should distinguish estimated fanout math from measured GPU
values without adding verbose public UI.

## Exit Gate

- `cd app && cargo test -p brain-visualizer`
- `cd app/web && npm test`
- `cd app/web && npm run typecheck`
- Tests fail if settings length, tombstones, mirrored defaults, metrics order,
  or histogram offset drift.
- Public HUD/dev-panel labels no longer present estimated profiler-derived
  values as directly measured GPU values.

## Handoff Notes

This stream may touch `app/crates/brain-visualizer/src/sim/gpu/mod.rs`, which
the simulation correctness stream may also touch. Sequence those edits or split
ownership carefully.

## Migration Notes

At ship time, migrate durable facts into `architecture/dev-panel.md`,
`architecture/simulation.md`, `architecture/profiling.md`,
`decisions/dev-tooling.md`, and `decisions/profiling.md`.

Shipped on 2026-06-12. Migrated current-state facts into:

- `architecture/dev-panel.md`: settings contract tests and estimated dev-panel
  metric labels.
- `architecture/simulation.md`: settings Float32Array contract tests and
  tombstone/default-written slot guarantees.
- `architecture/profiling.md`: estimated `synapticEventsPerSec` labeling and
  metrics layout test guarantees.
- `decisions/dev-tooling.md`: duplicated explicit golden tests as the guardrail
  for the flat Float32Array contract.
- `decisions/profiling.md`: derived synaptic-event metric remains cheap and is
  labelled as estimated.
