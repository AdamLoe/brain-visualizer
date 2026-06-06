# Decisions — Profiling

## First-class profiling from the start; always-on corner HUD

- **Decision.** The profiler (`crates/brain-visualizer/src/profiler.rs → Profiler`, `web/src/render/profiler.ts →
  Profiler`) and the public corner HUD (`web/src/ui/hud.ts → CornerHud`) run on every
  page load — not behind a debug flag.
- **Why.** "Where did the time go?" needs to be answerable without a special
  build or a hidden toggle. A small corner readout (FPS, N, backend, syn/s)
  gives developers and curious users a continuous ground truth at near-zero
  rendering cost. The profiler overhead is a fixed-size frame-time ring push
  and a counter increment per tick — negligible.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md).
- **Code anchors.** `crates/brain-visualizer/src/profiler.rs → Profiler`; `web/src/ui/hud.ts → CornerHud`.

## Periodic GPU reduction + async non-blocking readback; no per-tick readback

- **Decision.** GPU metrics are collected by dispatching a read-only compute
  reduction pass once every `METRICS_ISSUE_INTERVAL = 15` ticks and reading
  the result back via a non-blocking `map_async` / staging-buffer pair
  (`MetricsReadState::Idle` / `Pending`). The render loop never stalls waiting
  for GPU data.
- **Why.** Synchronous GPU-to-CPU readback in the render loop kills frame rate.
  The two-state machine (issue when Idle; poll when Pending; never copy into a
  mapped buffer) gives fresh metrics every ~250 ms at 60 fps while keeping the
  rAF loop entirely non-blocking. Per-tick readback would require a
  `device.poll(Wait)` every frame — a known WebGPU performance cliff.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → MetricsReadState`;
  `crates/brain-visualizer/src/sim/gpu/shaders/metrics.wgsl → reduce_metrics`;
  `crates/brain-visualizer/src/sim/gpu/resources.rs → METRICS_SLOT_COUNT`.
- **Tradeoffs.** Metrics are delayed by up to `METRICS_ISSUE_INTERVAL` ticks
  plus one `map_async` round-trip (~1–2 frames). Acceptable for diagnostics;
  the HUD is explicitly labelled as a ~1 Hz readout.

## Cheap counters only in the hot loop; synaptic events = spikes × K

- **Decision.** The per-tick GPU scatter loop accumulates no per-synapse
  counters. `synapticEventsPerSec` is derived CPU-side as
  `spikes_per_sec × K` (`crates/brain-visualizer/src/sim/gpu/mod.rs`, `synaptic_events_per_sec =
  spikes_per_sec * self.config.k`).
- **Why.** The scatter pass iterates over every active synapse — O(spikes × K)
  per tick, potentially hundreds of millions of atomic adds per second at high
  N. Counting there would dominate the sim budget. For a homogeneous random
  network the `spikes × K` estimate is exact; the only approximation is that K
  is the mean fan-out, which equals the actual fan-out in the fixed-K wiring.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md),
  [`../architecture/simulation.md`](../architecture/simulation.md).
- **Revisit when.** Heterogeneous fan-out is introduced (K varies per neuron),
  in which case `spikes × K_mean` becomes an estimate and the GPU scatter pass
  would need a cheap per-spike accumulator instead.

## Branching ratio computed CPU-side from a rolling spike history

- **Decision.** `branchingRatio` is computed on the CPU from the last 64
  `spikes_this_tick` samples (`METRICS_HISTORY_LEN = 64`,
  `crates/brain-visualizer/src/sim/gpu/mod.rs`), not from a dedicated GPU accumulator.
- **Why.** The branching parameter σ is the ratio of successive avalanche sizes
  averaged over a window — a CPU-side rolling division over already-available
  spike counts. No new GPU slot is needed; the signal is inherently noisy
  enough that a 64-sample window (≈ 1 s at 60 fps) is adequate for the
  subcritical / critical / supercritical classification.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md).

## See also

- [`../architecture/profiling.md`](../architecture/profiling.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
