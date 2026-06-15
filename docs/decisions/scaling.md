# Decisions — Scaling

## Legacy preset table retained; auto-select deferred

- **Decision.** Keep the web preset table in `web/src/ui/controls.ts →
  TIER_PRESETS` (including the compatibility `basic` entry), but leave
  per-device automatic tier selection deferred. The live app scales only when a
  new N/K pair is chosen explicitly.
- **Why.** Auto-selection requires browser WebGPU benchmark numbers on real
  hardware to calibrate the heuristics. Those numbers were not available in
  the WSL2 development environment (only llvmpipe software-emulation data
  exists, which is not representative). The current live UI already rebuilds
  from explicit choices, so keeping the preset table as a manual/legacy surface
  is enough until real benchmark data exists.
- **Current default note.** This tier decision is separate from the clean
  first-load `DEFAULT_CONFIG`, which boots at `n=6000`, `k=16`, `tier="low"`.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/ui/controls.ts → TIER_PRESETS`;
  `web/src/ui/dev-panel.ts → DevPanel`;
  `crates/brain-visualizer/src/sim/scaler.rs → TierRange::for_tier`.
- **Revisit when.** Real-hardware browser WebGPU benchmark data is collected.

## K stays first-class even though presets pin it

- **Decision.** Synaptic out-degree K remains a first-class scaling knob. The
  legacy preset table happens to pin K=16 for its canned entries, but the live
  dev panel exposes direct K edits and the dormant scaler math still models K as
  a per-tier axis for a future re-arm.
- **Why.** K × N drives synapse-event cost as much as N alone. Varying K per
  rebuild gives finer control over computational load: a lower-end device can
  run a sparser but still valid network rather than just shrinking N.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`, `N_MAX`;
  `web/src/ui/dev-panel.ts → DevPanel`;
  `crates/brain-visualizer/src/sim/scaler.rs → TierRange`.

## 20k product cap, separate from GPU hardware capacity

- **Decision.** The product maximum neuron count is `20_000`. Web UI bounds,
  saved config load/save, the dev-panel N control, JS scaler ranges, WASM
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
  `web/src/ui/dev-panel.ts → DevPanel`;
  `crates/brain-visualizer/src/sim/backend.rs → PRODUCT_MAX_N, clamp_neuron_count`;
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend`;
  `crates/brain-visualizer/src/sim/scaler.rs → TierRange::for_tier`;
  `crates/brain-visualizer/src/gpu_limits.rs → GpuCaps`.
- **Tradeoffs.** Saved `bv2_config_v2` payloads keep their storage key and
  `version: 1`, but `n` is clamped on load and on save. This avoids a broad
  settings reset while preventing stale localStorage from bypassing the cap.

## Runtime auto-scaling removed; N is fixed at startup, user-driven only

- **Decision.** No code path changes N during the rAF loop. The network is built
  at a fixed N at startup and stays there until the user edits N/K in the dev
  panel, a mobile boot forces the low-tier profile, or a legacy preset rebuild
  is requested. The pure `scalerDecide` function and the Rust `scaler.rs` stub
  are kept dormant + tested as a seed for a future scaler;
  `adaptiveScalerEnabled` (Float32Array index 23) is left reserved/inert.
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
  `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphSegmentChunk`.

## Chunk morphology segment storage instead of throttling decoration

- **Decision.** Morphology segments are split into chunked storage resources,
  with per-chunk active/recent compaction buffers and indirect draw args, rather
  than suppressing dendrite decoration at high N to fit one storage binding.
- **Why.** `MorphSegment` is 48 B and product-scale morphology can exceed the
  WebGPU `max_storage_buffer_binding_size` if bound as one buffer. Chunking keeps
  every segment binding below the project chunk budget in
  `crates/brain-visualizer/src/buffers.rs → MAX_CHUNK_BYTES` and the adapter
  limit while preserving the full generator output and the GPU-driven indirect
  render path.
- **Applies to.** [`../architecture/scaling.md`](../architecture/scaling.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md).
- **Code anchors.**
  `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_segment_chunk_layout, MorphSegmentChunk, MorphBuffers`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → render_full`;
  `crates/brain-visualizer/src/sim/morphology.rs → effective_decor_group_max`.
- **Tradeoffs.** The frame graph loops more bind groups and indirect draws when
  segment counts cross a chunk boundary, but all resources are still persistent
  and the per-frame work remains GPU-only.

## See also

- [`../architecture/scaling.md`](../architecture/scaling.md)
- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
