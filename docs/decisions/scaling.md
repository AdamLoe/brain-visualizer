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
- **Current default note.** This tier decision is separate from the clean
  first-load `DEFAULT_CONFIG`, which boots at `n=6000`, `k=16`, `tier="low"`.
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

## 20k product cap, separate from GPU hardware capacity

- **Decision.** The product maximum neuron count is `20_000`. Web UI bounds,
  saved config load/save, JS scaler ranges, Rust `SimConfig::default()`, WASM
  backend construction, and Rust scaler proposals all clamp to that cap.
  `GpuCaps` continues to report hardware-derived adapter capacity and is not
  lowered to 20k.
- **Why.** The current product is a morphology-rich visual sculpture, not a
  raw-scale demo. Above 20k the readability, initialization cost, and GPU buffer
  pressure of detailed soma/dendrite/axon morphology dominate the experience.
  Keeping hardware capacity separate preserves honest diagnostics while keeping
  product controls bounded to what the current visual design supports.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/core/types.ts → PRODUCT_MAX_N, clampNeuronCount`;
  `web/src/ui/controls.ts → TIER_PRESETS, N_MIN, N_MAX`;
  `crates/brain-visualizer/src/sim/backend.rs → PRODUCT_MAX_N, clamp_neuron_count`;
  `crates/brain-visualizer/src/sim/scaler.rs → TierRange::for_tier`;
  `crates/brain-visualizer/src/gpu_limits.rs → GpuCaps`.
- **Tradeoffs.** Old saved `bv2_config_v1` payloads keep their schema version but
  saved `n` is clamped. This avoids a broad settings reset while preventing
  stale localStorage from bypassing the cap.

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

## N=6 000 as the beauty-first default

- **Decision.** The clean first-load default is N=6 000, K=16. This is the
  high-scale baseline for the current beauty-first phase, parked below the
  high-N tiers and the dormant adaptive scaler.
- **Why.** The active/recent GPU compaction pass makes N=6 000 affordable at
  runtime: frame cost tracks visible firing activity rather than total segment
  count. The one-time generation cost (a few seconds at N=6 000) is acceptable
  for a startup path.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/core/types.ts → DEFAULT_CONFIG`;
  `crates/brain-visualizer/src/sim/backend.rs → SimConfig::default`.
- **Revisit when.** A higher default scale is targeted or the auto-scaler is
  re-armed.

## Active/recent compaction over near/far LOD or auto-scaling

- **Decision.** Morphology tube rendering draws only active/recent segments
  selected by a GPU compaction pass (indirect draw), rather than all segments
  every frame.
- **Why.** This directly caps frame cost by activity level: all geometry stays
  available in the segment buffer, but only segments whose activity owner fired
  recently are drawn. Near/far LOD would require a separate distance-based
  pipeline split without capping overdraw on active neurons; runtime auto-scaling
  would require restarting the backend to reduce segment count. Compaction keeps
  the full morphology intact and scales frame cost with what is visually lit.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md).
- **Code anchors.**
  `crates/brain-visualizer/src/sim/gpu/mod.rs → render_full` (indirect tube draws);
  `crates/brain-visualizer/src/sim/gpu/resources.rs` (active segment index buffer).

## Throttle dendrite decoration with N rather than raise the GPU buffer cap

- **Decision.** Dendrite decoration density is linearly ramped from full (below
  N≈2 400) to zero (above N=8 000) rather than chunking the segment buffer to
  raise the per-binding ceiling.
- **Why.** Morphology segments are bound as a single GPU storage buffer; the
  WebGPU `max_storage_buffer_binding_size` limit (128 MiB) caps the segment
  buffer at ~2.76 M segments (~N=12 000). Throttling decoration keeps the total
  segment count within the binding limit at the cost of reduced dendrite bushiness
  at high N (where close-up detail is less legible anyway). Chunking or
  multi-binding the buffer is the correct long-term fix but is deferred.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.**
  `crates/brain-visualizer/src/sim/morphology.rs → DECOR_FULL_N, DECOR_ZERO_N, effective_decor_group_max`.
- **Tradeoffs.** Dendrite bushiness is reduced above N≈2 400. The storage-buffer
  chunking fix (splitting the segment buffer across multiple bindings in
  `crates/brain-visualizer/src/sim/gpu/resources.rs`) would remove this tradeoff
  but is not yet implemented.
- **Revisit when.** The segment buffer is chunked into multiple bindings.

## See also

- [`../architecture/scaling.md`](../architecture/scaling.md)
- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
