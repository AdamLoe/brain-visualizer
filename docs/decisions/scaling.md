# Decisions — Scaling

## Three tiers built; auto-select deferred

- **Decision.** Ship three presets (low / balanced / max) plus a minimal
  `basic` entry; all are built and testable. Per-device automatic tier
  selection is deferred — the user switches tiers manually.
- **Why.** Auto-selection requires browser WebGPU benchmark numbers on real
  hardware to calibrate the heuristics. Those numbers were not available in
  the WSL2 development environment (only llvmpipe software-emulation data
  exists, which is not representative). Shipping the manual switch now lets
  all three tiers be exercised and verified before committing to selection
  logic.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/ui/controls.ts → TIER_PRESETS`; `crates/brain-visualizer/src/sim/scaler.rs → TierRange::for_tier`.
- **Revisit when.** Real-hardware browser WebGPU benchmark data is collected.

## K is a per-tier knob alongside N

- **Decision.** Synaptic out-degree K is fixed per tier (not a single global
  constant), set from the tier preset at startup. No separate "connectivity
  level" control is exposed to the user; K is subsumed into the tier preset. (The
  dormant scaler math also treats K as a per-tier axis, for a future re-arm.)
- **Why.** K × N drives synapse-event cost as much as N alone. Varying K per
  tier gives finer control over computational load: a lower-end device can run
  a sparser but still valid network rather than just shrinking N.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`, `N_MAX`; `crates/brain-visualizer/src/sim/scaler.rs → TierRange`.

## 1M-default / 10M-gated-stretch honest-adaptation promise

- **Decision.** The Max tier practical default is ~1 M neurons. 10 M is a
  best-case discrete-GPU stretch, unlocked only when device limits
  (`maxStorageBufferBindingSize` via `GpuCaps`) and a benchmark burst both
  confirm it is sustainable. The UI and documentation must not imply 10 M is
  generally available.
- **Why.** The visualizer's value proposition is honest adaptation to the
  actual machine — not inflated copy that most browsers cannot sustain.
  Promising 10 M when typical mid-range GPUs cannot sustain it at 60 fps
  would be misleading.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/ui/controls.ts → N_MAX`; `crates/brain-visualizer/src/gpu_limits.rs → GpuCaps`.
- **Tradeoffs.** The unlock condition (device limits + benchmark burst) requires
  a runtime benchmark pass that is not yet implemented; for now `N_MAX[max]`
  is capped conservatively.

## Runtime auto-scaling removed; N is fixed at startup, user-driven only

- **Decision.** No code path changes N during the rAF loop. The network is built
  at a fixed N at startup and stays there until the user picks a different tier or
  edits N/K in the dev panel (which restarts the backend). The pure `scalerDecide`
  function and the Rust `scaler.rs` stub are kept dormant + tested as a seed for a
  future scaler; `adaptiveScalerEnabled` (Float32Array index 23) is left
  reserved/inert.
- **Why.** The live scaler was unshippable in two compounding ways. (1) It decided
  on **p95** frame time, but each `grow_n` called `GpuBackend.reinitialize` — a
  full teardown (rebuild manifold + connectivity, reallocate every GPU buffer,
  recompile every render pipeline) that stalled exactly one frame per resize.
  One outlier per 120-frame window never moves p95, so the loop read "healthy" and
  grew again — an unbounded feedback loop (observed `frame_ms_avg` climbing
  26 → 91 ms while `frame_ms_p95` stayed pinned at 4.2 ms, fps flapping 0.3 ↔ 240).
  (2) Even decided correctly, the `reinitialize`-per-resize cost is a multi-second
  stall. Pulling it entirely is the honest fix until a gentle, stall-aware scaler
  exists.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/ui/controls.ts → scalerDecide` (dormant);
  `crates/brain-visualizer/src/sim/scaler.rs → propose` (unused stub);
  `web/src/main.ts → boot` (rAF `dumped` block no longer calls the scaler).
- **Revisit when.** A gentle, hysteretic, stall-aware auto-scaler is taken on —
  decide on avg not p95, and split buffer-resize from pipeline recompile so a
  resize is cheap. See [`../plans/future_roadmap.md`](../plans/future_roadmap.md).

## See also

- [`../architecture/scaling.md`](../architecture/scaling.md)
- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
