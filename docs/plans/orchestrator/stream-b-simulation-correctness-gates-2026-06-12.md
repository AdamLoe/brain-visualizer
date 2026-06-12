---
status:        shipped
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/data-model.md
  - architecture/simulation.md
  - architecture/build-and-deploy.md
  - decisions/data-layout.md
  - decisions/dynamics.md
---

# Stream B2: Simulation Correctness Gates

## Mission

Turn known simulation correctness risks into failing gates. Done means native
WGSL determinism tests fail in strict CI when no adapter is available,
fixed-point current overflow has an executable policy under high
synchrony/seizure demos, and 24-bit tick wrap behavior is covered by host and
shader tests.

## Scope

In scope:

- Adapter skip/fail behavior in WGSL hash/target determinism and GPU dynamics
  tests.
- Fixed-point `i32` synaptic current overflow detection or saturation policy.
- 24-bit tick wrap tests for Rust helpers and WGSL helpers used by metrics,
  simulation, and render timing.
- Testing/doc updates for strict gate behavior.

Out of scope:

- CPU retirement, telemetry, morphology scaling, region aesthetics, or product
  retuning unless required by the overflow policy.

## Context Routes

- `docs/architecture/data-model.md`
- `docs/architecture/simulation.md`
- `docs/architecture/build-and-deploy.md`
- `docs/agent-context/testing-how-to.md`
- `docs/decisions/data-layout.md`
- `docs/decisions/dynamics.md`
- `app/crates/brain-visualizer/src/sim/backend.rs`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs`
- `app/crates/brain-visualizer/src/sim/gpu/resources.rs`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl`
- `app/crates/brain-visualizer/tests/wgsl_hash_determinism.rs`
- `app/crates/brain-visualizer/tests/wgsl_target_determinism.rs`
- `app/crates/brain-visualizer/tests/gpu_sim_dynamics.rs`

## Approach

Create one shared native-test adapter helper or equivalent local pattern. If
adapter acquisition fails locally, tests may skip with explicit output. If
`CI` or `BV_REQUIRE_WGPU_TESTS=1` is set, adapter acquisition failure must fail
the gate.

Make the current overflow policy executable. Keep or improve `max_abs_current`
high-water instrumentation, add a high-synchrony stress gate with a defensible
margin below `i32::MAX`, and switch to saturating atomics in `scatter.wgsl` only
if the stress gate cannot pass safely.

Add Rust and WGSL tick-wrap coverage for `0x00ff_ffff -> 0`, small forward ages
across wrap, intervals near half range, and metrics windows across wrap.

## Exit Gate

- `cd app && BV_REQUIRE_WGPU_TESTS=1 cargo test -p brain-visualizer`
- `cd app && cargo test -p brain-visualizer`
- `cd app/web && npm run typecheck` if shader or wasm-facing Rust changed.
- Strict no-adapter mode fails instead of silently passing.
- Local no-adapter mode skips with explicit messages.
- Current high-water stress tests cover seizure/high-synchrony settings.
- Tick-wrap tests cover Rust and WGSL modular age calculations.

## Handoff Notes

Do not run this in parallel with B1 unless file-region ownership is explicit.
Both streams may edit `sim/gpu/mod.rs`. Do not weaken the no-readback
render-loop policy to make tests easier; native test harness readback is
acceptable.

## Migration Notes

Migrated:

- `architecture/data-model.md` now owns the executable fixed-point overflow
  high-water policy and the 24-bit tick-wrap gate.
- `architecture/build-and-deploy.md` and `agent-context/testing-how-to.md` now
  document the strict native wgpu adapter behavior and the new overflow/tick
  integration tests.
- `decisions/data-layout.md` now records the current choice to keep plain
  scatter `atomicAdd` behind an executable stress margin, with saturating atomics
  as the revisit path.

No `simulation.md` or `decisions/dynamics.md` update was needed because the LIF
dynamics, energy model, and tuning semantics did not change.
