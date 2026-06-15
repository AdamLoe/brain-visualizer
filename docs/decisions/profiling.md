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
  `spikes_per_sec × K` (`crates/brain-visualizer/src/sim/gpu/mod.rs →
  build_metrics_snapshot`) and is labelled as estimated in the HUD and dev
  panel.
- **Why.** The scatter pass iterates over every active synapse — O(spikes × K)
  per tick, potentially hundreds of millions of atomic adds per second at high
  N. Counting there would dominate the sim budget. For a homogeneous random
  network the `spikes × K` estimate is exact; the only approximation is that K
  is the mean fan-out, which equals the actual fan-out in the fixed-K wiring.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md),
  [`../architecture/simulation.md`](../architecture/simulation.md).
- **Revisit when.** Heterogeneous fan-out exists (K varies per neuron),
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

## Morphology build stats stay out of the always-on profiler

- **Decision.** The morphology review harness (`morph_view`) records its own
  build/review stats, config snapshots, and artifact metadata rather than
  folding them into the always-on runtime profiler or corner HUD.
- **Why.** Those numbers are acceptance evidence for a specific morphology
  build, not a live per-frame signal. Keeping them separate prevents the runtime
  profiler from accumulating one-off review noise and makes the browser WASM
  timing behavior explicit: morphology timing is zero there because native
  `Instant` is not available.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md)
- **Code anchors.** `crates/brain-visualizer/examples/morph_view.rs`;
  `crates/brain-visualizer/src/profiler.rs → Profiler`

## Telemetry remains disabled unless an endpoint is configured

- **Decision.** Browser telemetry has a typed contract and tests, but the
  client sends nothing unless an endpoint is explicitly configured. Every send is
  also gated by query/local/browser privacy opt-outs and a fixed payload
  allowlist.
- **Why.** Real-user startup and frame-health signals are useful only if they
  are privacy-bounded and impossible to enable accidentally. Keeping the
  transport inert by default lets tests protect event shape and sanitization now
  without committing to a telemetry provider or retention policy.
- **Applies to.** [`../architecture/profiling.md`](../architecture/profiling.md),
  [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md).
- **Code anchors.** `web/src/observability/telemetry.ts → createTelemetryClient,
  telemetryDisabledReason, buildTelemetryBody`; `web/src/observability/telemetry.test.ts`.
- **Revisit when.** A telemetry endpoint, retention policy, and opt-in/opt-out
  posture are chosen.

## See also

- [`../architecture/profiling.md`](../architecture/profiling.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
