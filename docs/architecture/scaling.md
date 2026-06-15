---
status:        active
owner:         adamg
last_updated:  2026-06-15
---

# Scaling

Neuron count (N) and synaptic out-degree (K) are fixed at startup and changed
**only** by deliberate rebuild actions. Today that means direct N/K edits in the
dev panel, the mobile low-tier override in `web/src/main.ts`, or legacy preset
calls through `web/src/ui/controls.ts → setTier`. There is no runtime
auto-scaler in the rAF loop, and the UI never silently changes scale after a
network is built.

The pure decision function `scalerDecide` and the Rust `scaler.rs` proposal stub
are retained but **dormant** — a tested seed for a future gentle auto-scaler (see
[deferred work](../plans/future_roadmap.md) and [`../decisions/scaling.md`](../decisions/scaling.md)).

## What it owns

- The tier→N/K presets and N bounds — `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`, `N_MAX`.
- The dormant JS scaler decision function — `web/src/ui/controls.ts → scalerDecide`, `ScalerAction` (no longer called from the rAF loop; tested by `web/src/ui/controls.test.ts`).
- The dormant Rust scaler proposal logic — `crates/brain-visualizer/src/sim/scaler.rs → propose`, `TierRange`, `ScaleProposal`, `TARGET_FRAME_MS`.
- The web preset table and dormant Rust proposal tiers — `web/src/ui/controls.ts → TIER_PRESETS`; `crates/brain-visualizer/src/sim/scaler.rs → TierRange::for_tier`.
- The adapter-limits → derived-caps pipeline — `crates/brain-visualizer/src/gpu_limits.rs → GpuCaps::derive`, `LimitsInput`, `FIELD_ELEMENT_BYTES`.
- The product neuron cap — `web/src/core/types.ts → PRODUCT_MAX_N`; `crates/brain-visualizer/src/sim/backend.rs → PRODUCT_MAX_N`.

## What it does NOT own

- GPU buffer allocation and `reinitialize` — [`gpu-backend.md`](gpu-backend.md).
- The sim dynamics and excitability model — [`simulation.md`](simulation.md).
- Retired CPU backend boundary — [`cpu-backend.md`](cpu-backend.md).
- Connectivity / synapse target math — [`connectivity.md`](connectivity.md).
- Build, test, and deploy — [`build-and-deploy.md`](build-and-deploy.md).

## Tier presets

Tier labels are a web-layer preset surface, not a live backend enum. The dev
panel's Network tab edits N/K directly (`web/src/ui/dev-panel.ts → DevPanel`),
while the legacy `Controls` facade still exposes `setTier` and
`TIER_PRESETS` for older callers and console access. The WASM/backend
construction path takes N and K only (`crates/brain-visualizer/src/lib.rs →
WasmGpuBackend::create`, `create_staged`, `reinitialize`), so tier metadata
affects the live backend only indirectly through the chosen N/K pair. Nothing
changes N between rebuilds.

The current default is N=6 000, K=16 (`web/src/core/types.ts → DEFAULT_CONFIG`).
The product maximum is `PRODUCT_MAX_N = 20_000`; clamp points live in
`web/src/core/types.ts → PRODUCT_MAX_N, clampNeuronCount, loadConfig, saveConfig`,
the dev-panel N control (`web/src/ui/dev-panel.ts → DevPanel`),
`crates/brain-visualizer/src/sim/backend.rs → PRODUCT_MAX_N, clamp_neuron_count`,
and the WASM/backend construction path in `crates/brain-visualizer/src/lib.rs`.
The dormant scaler proposal ranges in `crates/brain-visualizer/src/sim/scaler.rs →
TierRange::for_tier` also stay under that cap. `TIER_PRESETS` fixes K=16 across
its web preset labels; the Rust `TierRange` table still carries wider K ranges
for its dormant Low/Balanced/Max proposal math.

**Active/recent rendering makes high-N affordable.** Morphology tube render cost
no longer scales with total generated segment count. A GPU compaction pass selects
only active/recent segments and both tube passes draw that compacted set via
indirect draw. At the N=6 000 low-firing default, only the subset written into
each chunk's `active_draw_args` is drawn per frame; total generated segment
count lives in `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphBuffers`
and `crates/brain-visualizer/src/sim/morphology.rs → MorphologyStats`. N, K,
and branch detail can therefore grow without proportionally growing frame cost —
the renderer tracks visible activity, not total geometry. Generation (one-time,
at brain build) is not the first-order target and remains a few seconds at high
N (axon Prim-attach dominates); runtime rendering is what was optimised.
GPU rendering details live in [`gpu-rendering.md`](gpu-rendering.md).

**Morphology segment budget and storage bindings.** The GPU segment allocation is
sized to the actual generated segment count, not a pre-allocated cap, and the
48 B `MorphSegment` list is split into chunked storage bindings by
`crates/brain-visualizer/src/sim/gpu/resources.rs → morph_segment_chunk_layout`.
Each segment chunk stays under the project chunk budget in
`crates/brain-visualizer/src/buffers.rs → MAX_CHUNK_BYTES` and under the
adapter's `max_storage_buffer_binding_size`; each chunk owns its compaction
buffers and indirect draw args. The host-side per-run segment cap is still sized
to worst-case per-neuron bounds and is comfortably unmet at N=6 000. Dendrite
decoration is bounded by the configured per-neuron hard cap, not by a hidden
neuron-count throttle to avoid the old single-binding ceiling.

Auto-selection of a tier based on measured device performance is deferred: the
benchmarks needed to calibrate those heuristics require real browser/GPU numbers
that were not available in the WSL2 development environment (only llvmpipe
software-emulation numbers exist; they live in git history and are not
representative).

## The dormant scaler decision function

No code path changes N at runtime. The rAF loop's once-per-second `dumped` block
drives only the HUD and dev-panel monitor — it no longer calls `scalerDecide` or
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
threads, and max scan items. All downstream tier and dispatch-split logic reads
`GpuCaps` rather than hard-coding device assumptions.

The derivation is pure (takes a plain `LimitsInput` struct) and is exercised by
`cargo test` without a real device. The `LimitsInput::webgpu_defaults()` fixture
encodes the conservative WebGPU baseline; the llvmpipe-specific fixture is in the
test suite at `crates/brain-visualizer/src/gpu_limits.rs`.

## Product cap vs adapter capacity

`PRODUCT_MAX_N = 20_000` is a product/readability cap, not a hardware-capacity
claim. The cap is enforced in both languages:

- Web: `web/src/core/types.ts → PRODUCT_MAX_N`, `clampNeuronCount`,
  `loadConfig`, and `saveConfig`.
- Web UI/scaler: `web/src/ui/dev-panel.ts` N control and
  `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`, `N_MAX`, `scalerDecide`.
- Rust: `crates/brain-visualizer/src/sim/backend.rs → PRODUCT_MAX_N`,
  `clamp_neuron_count`, and the WASM/backend construction path in
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend`.
- Rust scaler stub: `crates/brain-visualizer/src/sim/scaler.rs →
  TierRange::for_tier` and `propose`.

`crates/brain-visualizer/src/gpu_limits.rs → GpuCaps::derive` still reports
adapter capacity from WebGPU limits. Those hardware-derived values can be much
larger than 20k and should stay larger; downstream product surfaces clamp to
`PRODUCT_MAX_N` separately.

Old saved browser config keeps the storage key `bv2_config_v2` and the persisted
payload `version: 1`, but saved `n` values above `20_000` are clamped on load
and on save. The visual/morph settings keys were not bumped for this cap.

## Update when

- `TIER_PRESETS` or `N_MIN`/`N_MAX` ranges change.
- `PRODUCT_MAX_N` or the web/Rust clamp points change.
- A runtime auto-scaler is re-armed (some code path again changes N during the
  rAF loop) — see [`future_roadmap.md`](../plans/future_roadmap.md).
- `GpuCaps` gains or loses a derived field.

## See also

- [`build-and-deploy.md`](build-and-deploy.md) — how to run the offline verification examples and cargo test
- [`gpu-backend.md`](gpu-backend.md) — `reinitialize`, buffer lifecycle
- [`cpu-backend.md`](cpu-backend.md) — retired CPU backend boundary
- [`connectivity.md`](connectivity.md) — why N×K drives memory/compute cost
- [`web-frontend.md`](web-frontend.md) — `restartWithBackend`, AppConfig persistence
- [`../decisions/scaling.md`](../decisions/scaling.md)
- [`../plans/future_roadmap.md`](../plans/future_roadmap.md) — deferred smart auto-scaling
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
