---
status:        active
owner:         adamg
last_updated:  2026-06-06
---

# Profiling

First-class performance instrumentation that feeds both a small public corner
HUD and the dev panel's Monitor/Dynamics tabs. The design principle is:
**zero-cost in the hot loop** — only cheap counters accumulate per tick; all
heavier work (GPU reduction, CPU aggregation, JSON dump) is deferred and
amortised. The morphology review harness (`morph_view`) stays outside that
always-on runtime profiler: it records build/review stats and artifact metadata,
not live per-frame metrics.

## What it owns

- `Profiler` ring buffer, per-second structured dump — `crates/brain-visualizer/src/profiler.rs → Profiler, ProfileSnapshot`
- JS-side profiler mirror — `web/src/render/profiler.ts → Profiler, ProfileSnapshot`
- Public corner HUD — `web/src/ui/hud.ts → CornerHud`
- GPU metrics reduction pass — `crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl → reduce_metrics`
- Reduction pipeline constants and uniform — `crates/brain-visualizer/src/sim/gpu/resources.rs → MetricsUniforms, METRICS_SLOT_COUNT`
- Async staging readback state machine — `crates/brain-visualizer/src/sim/gpu/mod.rs → MetricsReadState`
- `parseMetrics` layout mapping — `web/src/core/settings.ts → parseMetrics, METRICS_LAYOUT`
- Branching-ratio rolling history — `crates/brain-visualizer/src/sim/gpu/mod.rs` (`METRICS_HISTORY_LEN = 64`)
- Morphology build/review stats and artifact JSON — `crates/brain-visualizer/examples/morph_view.rs`
- Morphology active/recent draw-count readback — `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::read_active_segment_count`

## What it does NOT own

- Dev panel display of the metrics — [`dev-panel.md`](dev-panel.md)
- General async-readback pattern shared with stats/edge-emitted buffers — [`gpu-backend.md`](gpu-backend.md)
- Simulation data layouts read by the reduction pass — [`data-model.md`](data-model.md)

## Headline metric: synaptic events/sec

`synapticEventsPerSec` is derived as `spikes_per_sec × K` — not by
instrumenting individual synapse activations in the scatter loop
(`crates/brain-visualizer/src/sim/gpu/mod.rs → build_metrics_snapshot`). The
scatter pass is on the GPU hot path; per-synapse counters
would require atomic adds for every edge traversal at O(spikes × K) per tick.
Multiplying the already-counted spike total by K gives the same result for a
homogeneous random network at near-zero extra cost.

The hot loop accumulates only: `spike_count` (GPU-written into the indirect
dispatch buffer, read back via the stats staging path) and nothing else.
User-visible labels reflect that estimate: the HUD uses `syn/s est`, and the
dev panel uses `Syn. events/sec (est.)`.

## crates/brain-visualizer/src/profiler.rs ring + per-second dump

`crates/brain-visualizer/src/profiler.rs → Profiler` holds a fixed-capacity frame-time ring
(`RingBuffer<120>`) and per-second counters. `record_frame(now_ms, frame_ms,
stats)` pushes a frame time and accumulates `TickStats` — no allocation. Once
per second the profiler emits a one-line JSON string via `dump` and resets the
window. The clock is injected (caller passes `now_ms`) rather than calling
`performance.now()` internally, keeping the profiler unit-testable without a
browser.

`web/src/render/profiler.ts → Profiler` mirrors the same shape in TypeScript for the
JS-side rAF loop.

## Corner HUD

`web/src/ui/hud.ts → CornerHud` is a small bottom-right fixed `<div>` updated once
per profiler dump (~1/s). It displays FPS, N, backend (GPU/CPU), and
`syn/s`. Optional debug fields (`renderResScale`, `nearLodInstances`,
`gpuTimingTotalMs`, `scalerReason`) appear only when `debugEnabled = true`.
The HUD runs **regardless of dev-panel state**.

## GPU metrics reduction pipeline

### Reduction pass

`crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl → reduce_metrics` runs once every
`METRICS_ISSUE_INTERVAL = 15` ticks (`crates/brain-visualizer/src/sim/gpu/mod.rs`). One thread per
neuron; read-only over `last_spike` and `v`. This pass **must not mutate any
simulation buffer** — determinism is preserved bit-for-bit.

All outputs accumulate via `atomicAdd` into a `METRICS_SLOT_COUNT = 32`-element
`array<atomic<u32>>`. The slot layout is documented as the authoritative
comment in `crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl` (slots 0–11 are scalar
accumulators; 16–31 are the 16-bin voltage histogram; 12–15 reserved).

### 64-bit voltage accumulation

WGSL has no `atomic<f32>`. Voltage is accumulated as a fixed-point `u32`
(offset and scaled by `MetricsUniforms` fields `volt_lo`, `volt_hi`,
`volt_scale = 1024`). For N up to 10 M the sum can exceed the `u32` max, so
the accumulation is split across slots 9 (lo) and 10 (hi): each neuron's
contribution is `atomicAdd`-ed into slot 9, and if that add wraps, `+1` is
carried into slot 10. The CPU recombines with `(hi * 2^32 + lo) / scale / N -
volt_lo`. See `crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl → add_voltage_fp`.

### Windowed pct-fired counts

Slots 6, 7, 8 accumulate the count of neurons that spiked within the last 6,
30, and 120 ticks respectively (≈ 100 ms / 500 ms / 2 s at 60 fps). These are
raw counts; the CPU divides by N to get the fractions delivered in `Metrics` as
`pctFired100ms`, `pctFired500ms`, `pctFired2s`. The tick-window boundaries are
approximate (they assume ~60 fps); `METRICS_ISSUE_INTERVAL = 15` means the
CPU sees a fresh reading roughly every 250 ms.

## Non-blocking async readback state machine

The readback uses a two-state machine so the render loop is never stalled
waiting for GPU-to-CPU copy to complete. The state is `MetricsReadState` in
`crates/brain-visualizer/src/sim/gpu/mod.rs`:

- **Idle → Pending:** when `Idle` and `ticks_since_metrics_issue ≥
  METRICS_ISSUE_INTERVAL`: zero `metrics_buf` via `write_buffer`, dispatch the
  reduce pass, `copy_buffer_to_buffer` → `metrics_staging`, submit, call
  `map_async`. Transition to `Pending`.
- **Pending polling:** each tick, `device.poll(Maintain::Poll)` is called. When
  the callback fires (sets `metrics_ready` via `Arc<AtomicBool>`), the mapped
  slice is read into `metrics_cpu`, `metrics_staging` is unmapped, and state
  returns to `Idle`.
- **Corruption invariant:** **never call `copy_buffer_to_buffer` into
  `metrics_staging` while `Pending`**. The buffer is mapped/locked; writing into
  it is the documented bug. The `Idle` check before issuing a new reduction is
  the guard. (The same pattern applies to `edge_emitted_staging`; the general
  readback mechanism is shared — see [`gpu-backend.md`](gpu-backend.md).)

## parseMetrics and the JS Metrics interface

`web/src/core/settings.ts → parseMetrics` maps the 33-float array (17 scalars at indices
0–16, 16 histogram bins at indices 17–32) returned by the WASM `metrics()`
method into the typed `Metrics` interface. The authoritative index order is
`web/src/core/settings.ts → METRICS_LAYOUT`. `METRICS_SCALAR_COUNT = 17` and
`METRICS_LENGTH = 33` are exported constants that are guarded by TypeScript and
Rust tests against the Rust backend's layout, scalar order, and histogram
offset.

## Branching ratio

`branchingRatio` is computed **CPU-side** from a rolling history of
`spikes_this_tick` samples (`METRICS_HISTORY_LEN = 64` in
`crates/brain-visualizer/src/sim/gpu/mod.rs`), not directly from a GPU slot. It is the ratio of
successive spike-count samples averaged over the window, approximating the
avalanche branching parameter σ. The dev panel's Dynamics tab classifies σ
into subcritical (< 0.9) / critical (0.9–1.1) / supercritical (> 1.1) bands.

## Morphology draw-count (selected active/recent segments)

The morphology active/recent compaction (see [`gpu-rendering.md`](gpu-rendering.md))
exposes a draw-count metric: the number of segments the last `render_full`
selected for the tube passes versus the total generated segment count.
`crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::read_active_segment_count`
copies the GPU-written `active_selected` counter into `selected_staging` and maps
it, returning `(selected, total)`. This is the morphology draw-count metric for
profiler/HUD diagnostics — selected stays near ~0.6% of total at the low-firing
default and rises with activity. It is a **blocking** readback off the per-frame
path (the live draw sizes itself from GPU indirect args, never from this count),
so it is native-only diagnostics, not an always-on HUD counter.

## Morphology review stats

`crates/brain-visualizer/examples/morph_view.rs` writes artifact-only
statistics for review runs, including the morphology config snapshot, visual
settings snapshot, build timings, segment budget usage, unique-target coverage,
socket/terminal distance bands, and render-artifact paths. These stats are not
part of the always-on profiler and do not feed the HUD.

On native targets the harness can time morphology generation with `Instant`.
Under browser WASM that timing path reads as zero because native `Instant` is
not available there; the artifact still records the run and its output paths,
but the timing field is not comparable to native runs.

## Update when

- `METRICS_SLOT_COUNT` changes or the slot layout in `metrics.wgsl` changes
  (update `METRICS_LAYOUT`, `parseMetrics`, and `MetricsUniforms`).
- `METRICS_ISSUE_INTERVAL` changes (affects HUD freshness and window-tick
  approximations).
- A new scalar metric is added to the GPU reduction (update `METRICS_LAYOUT`,
  `parseMetrics`, and the `Metrics` interface).
- The profiler dump JSON schema changes (`crates/brain-visualizer/src/profiler.rs → ProfileSnapshot`
  and `web/src/render/profiler.ts → ProfileSnapshot` must stay in sync).
- Morphology review artifact contents or timing sources change.
- The morphology selected-segment draw-count readback (`read_active_segment_count` / `active_selected`) changes shape or wiring.

## See also

- [`dev-panel.md`](dev-panel.md) — displays metrics owned here
- [`gpu-backend.md`](gpu-backend.md) — general async-readback pattern; stats staging path
- [`simulation.md`](simulation.md) — `last_spike` / `v` buffers read by the reduction
- [`data-model.md`](data-model.md) — `last_spike` packed layout the reduction decodes
- [`../decisions/profiling.md`](../decisions/profiling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
