---
status:        active
owner:         adamg
last_updated:  2026-06-05
---

# Scaling

Neuron count (N) and synaptic out-degree (K) are fixed at startup and changed
**only** by deliberate user action — picking a tier or editing N/K in the dev
panel. There is no runtime auto-scaler in the rAF loop. The scaling surface
keeps the experience honest: the user chooses a tier, and the UI never silently
jumps tiers or promises neuron counts the hardware cannot support.

The pure decision function `scalerDecide` and the Rust `scaler.rs` proposal stub
are retained but **dormant** — a tested seed for a future gentle auto-scaler (see
[deferred work](../plans/future_roadmap.md) and [`../decisions/scaling.md`](../decisions/scaling.md)).

## What it owns

- The tier→N/K presets and N bounds — `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`, `N_MAX`.
- The dormant JS scaler decision function — `web/src/ui/controls.ts → scalerDecide`, `ScalerAction` (no longer called from the rAF loop; tested by `web/src/ui/controls.test.ts`).
- The dormant Rust scaler proposal logic — `crates/brain-visualizer/src/sim/scaler.rs → propose`, `TierRange`, `ScaleProposal`, `TARGET_FRAME_MS`.
- The three difficulty tiers as N+K presets — `web/src/ui/controls.ts → TIER_PRESETS`; `crates/brain-visualizer/src/sim/scaler.rs → TierRange::for_tier`.
- The adapter-limits → derived-caps pipeline — `crates/brain-visualizer/src/gpu_limits.rs → GpuCaps::derive`, `LimitsInput`, `FIELD_ELEMENT_BYTES`.
- The 1M-default / 10M-gated-stretch cap policy (see below).

## What it does NOT own

- GPU buffer allocation and `reinitialize` — [`gpu-backend.md`](gpu-backend.md).
- The sim dynamics and excitability model — [`simulation.md`](simulation.md).
- CPU backend throughput and rayon pool — [`cpu-backend.md`](cpu-backend.md).
- Connectivity / synapse target math — [`connectivity.md`](connectivity.md).
- Build, test, and deploy — [`build-and-deploy.md`](build-and-deploy.md).

## Tier presets

Tiers are manual presets: the user picks one and N/K are set from that tier's
preset for the lifetime of that network. Switching tier triggers a backend
restart at the new N/K (see [`web-frontend.md`](web-frontend.md) →
`restartWithBackend`). Nothing changes N between restarts.

The V2 default is `low` at N=1 200 (morphology-first scale; see
`web/src/core/types.ts → DEFAULT_CONFIG`). The `TIER_PRESETS` table fixes K=16 across
all tiers for the current beauty-first phase; the `TierRange` table in
`crates/brain-visualizer/src/sim/scaler.rs` carries wider K ranges for when a scaler is
re-armed.

Auto-selection of a tier based on measured device performance is deferred: the
benchmarks needed to calibrate those heuristics require real browser/GPU numbers
that were not available in the WSL2 development environment (only llvmpipe
software-emulation numbers exist; they live in git history and are not
representative).

## The dormant scaler decision function

No code path changes N at runtime. The rAF loop's once-per-second `dumped` block
drives only the HUD and sonification — it no longer calls `scalerDecide` or
`gpuBackend.reinitialize`. `scalerDecide` (`web/src/ui/controls.ts`) survives as a
pure, stateless function kept under test (`web/src/ui/controls.test.ts`): given
p95 frame time, current N, tier, time since last resize, and a restart-in-progress
flag, it returns one of `{ kind: "none" }`, `{ kind: "shrink_n"; newN }`, or
`{ kind: "grow_n"; newN }`. Nothing consumes the result today.

The Rust-side `crates/brain-visualizer/src/sim/scaler.rs → propose` mirrors the same shrink/grow
math for host-testable correctness, but is an unused proposal stub.

Both are retained as a tested seed for a future re-armed auto-scaler. Why runtime
auto-scaling was pulled — the p95-blind feedback loop and the `reinitialize`
full-teardown stall — is recorded in [`../decisions/scaling.md`](../decisions/scaling.md); the
shape a revived scaler should take is in [`future_roadmap.md`](../plans/future_roadmap.md).

The `adaptiveScalerEnabled` settings field (Float32Array index 23) is left
**reserved/inert** to preserve the Rust↔TS `VisualSettings` contract; it is no
longer read by any decision path and no longer exposed in the dev panel (see
[`dev-panel.md`](dev-panel.md)).

## Adapter limits and derived caps

At device acquisition time, `crates/brain-visualizer/src/gpu_limits.rs → GpuCaps::derive` converts raw
`wgpu::Limits` into project-level caps (`GpuCaps`): workgroup size (largest
power-of-two ≤ both the X-size and invocation caps, capped at 256 for
occupancy), max neurons per single 4-byte storage binding, max flat dispatch
threads, max scan items, and max near-LOD instances. All downstream tier,
dispatch-split, and instance-count logic reads `GpuCaps` rather than hard-coding
device assumptions.

The derivation is pure (takes a plain `LimitsInput` struct) and is exercised by
`cargo test` without a real device. The `LimitsInput::webgpu_defaults()` fixture
encodes the conservative WebGPU baseline; the llvmpipe-specific fixture is in the
test suite at `crates/brain-visualizer/src/gpu_limits.rs`.

## The 1M-default / 10M-gated-stretch cap policy

The Max tier practical default is ~1 M neurons (`N_MAX[max]` in
`web/src/ui/controls.ts`). 10 M is a best-case stretch, unlocked only when:

1. The device's `maxStorageBufferBindingSize` (reported via `GpuCaps`) is large
   enough to hold the per-field arrays, **and**
2. A benchmark burst confirms the frame budget is sustainable at that N.

The visualizer does not imply 10 M is generally available. Honest adaptation to
the actual machine is the feature; inflated copy that most devices cannot sustain
is not. See [`../decisions/scaling.md`](../decisions/scaling.md).

## Update when

- `TIER_PRESETS` or `N_MIN`/`N_MAX` ranges change.
- A runtime auto-scaler is re-armed (some code path again changes N during the
  rAF loop) — see [`future_roadmap.md`](../plans/future_roadmap.md).
- `GpuCaps` gains or loses a derived field.
- The 10M unlock conditions change.

## See also

- [`build-and-deploy.md`](build-and-deploy.md) — how to run the offline verification examples and cargo test
- [`gpu-backend.md`](gpu-backend.md) — `reinitialize`, buffer lifecycle
- [`cpu-backend.md`](cpu-backend.md) — CPU tier throughput characteristics
- [`connectivity.md`](connectivity.md) — why N×K drives memory/compute cost
- [`web-frontend.md`](web-frontend.md) — `restartWithBackend`, AppConfig persistence
- [`../decisions/scaling.md`](../decisions/scaling.md)
- [`../plans/future_roadmap.md`](../plans/future_roadmap.md) — deferred smart auto-scaling
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
