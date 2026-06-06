# Decisions — Backends

## Two interchangeable backends on the same network

- **Decision.** One `SimBackend` trait, two implementations: GPU (WebGPU
  compute, clock-driven) and CPU (event-driven active-list with rayon, WebGL2
  render-only). Both derive their network from the same `SimConfig.seed` via the
  same procedural connectivity, same `last_spike` packing, and same fixed-point
  current scale (S = 4096).
- **Why.** The point is a direct CPU-vs-GPU measurement on identical networks;
  the two natural execution models (all-parallel GPU clock-tick vs. event-driven
  CPU active-list) map cleanly onto the two hardware targets and double as a
  systems-design showcase.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/mod.rs → SimBackend`; `crates/brain-visualizer/src/sim/cpu/mod.rs →
  CpuBackend`; `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend`.

## Backend choice exposed as a top-right toggle; side-by-side race deferred

- **Decision.** The GPU/CPU choice is a toggle in the top-right UI corner.
  A simultaneous side-by-side "race" view (both backends rendering in parallel
  with throughput counters) is deferred.
- **Why.** The comparison is the goal, but building a dual-render layout in
  parallel with the rest of the sim adds scope with no user-visible benefit
  until the individual backends are solid. A toggle covers the comparison use
  case and can be upgraded later.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Revisit when.** Both backends reach visual parity and the team wants to make
  the comparison a first-class UX feature.

## Backend switch = full teardown + restart with the same seed

- **Decision.** Switching the active backend tears down all sim and render state
  and reinitializes from scratch using the same seed. No mid-run state transfer
  between GPU buffers and the CPU SharedArrayBuffer.
- **Why.** State transfer between fundamentally different memory layouts (GPU
  storage buffers vs. WASM SharedArrayBuffer) is complex and error-prone for
  zero user benefit. A restart is instant and deterministic; the same seed
  guarantees the visitor sees the same network, which is the meaningful
  comparison unit.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/cpu/mod.rs → CpuBackend::initialize`,
  `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::initialize`.

## CPU coordinator-worker + rayon-pool topology

- **Decision.** CPU simulation runs entirely off the main thread. A dedicated
  coordinator Web Worker owns the WASM instance, initializes the
  `wasm-bindgen-rayon` pool, advances the event-driven active-list loop, and
  writes SoA state into SharedArrayBuffer. The main thread owns input, WebGL2
  rendering, UI, and profiler display, and communicates with the coordinator via
  structured messages.
- **Why.** Keeping simulation off the main thread makes the CPU-vs-GPU
  comparison honest (no jank from sim work freezing orbit, controls, or the HUD)
  and establishes a clean ownership boundary: worker-side sim state,
  main-thread render state, SharedArrayBuffer as the bridge.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Code anchors.** `web/src/cpu/cpu-worker.ts → handleInit, onmessage`;
  `web/src/cpu/cpu-renderer.ts → CpuRenderer`.
- **Tradeoffs.** The SharedArrayBuffer bridge requires COOP/COEP response headers
  for `crossOriginIsolated`; without them the pool falls back to single-threaded
  WASM. See [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md).

## CPU scatter: fixed-point atomics, no per-thread partial buffers

- **Decision.** Each synaptic contribution is applied directly to the target's
  `AtomicI32` accumulator via `fetch_add` (S = 4096). Per-thread partial current
  buffers with a full reduction are permanently rejected.
- **Why.** Allocating and zeroing per-tick partial buffers dominates the hot-loop
  cost at any useful N. The atomic-add model is race-free, deterministic, and
  directly comparable to the GPU scatter path. Spatial partitioning (to reduce
  atomic contention) is a deferred optimization that requires a measured
  bottleneck first.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/cpu/core.rs → scatter_one_source`,
  `crates/brain-visualizer/src/sim/cpu/core.rs → scatter_map`.

## CPU backend parked for V2; GPU-only; code kept but unwired

- **Decision.** V2 is GPU-only. `DEFAULT_CONFIG.backend = "gpu"` forces GPU at
  boot; the toggle is hidden. The V2 feature set (per-neuron heterogeneity,
  weight normalization, input modes, ribbon renderer) is implemented only in
  GPU/WGSL. CPU code compiles and the parity harness passes, but nothing in the
  V2 render/sim path invokes `CpuBackend` or `CpuRenderer`. The revive-or-retire
  decision is deferred until the GPU beauty target is confirmed stable.
- **Why.** Parking the CPU backend roughly halves the surface area of every V2
  phase. The CPU/GPU comparison showcase remains a future option; building it
  in parallel with the ribbon renderer and heterogeneity system would delay the
  beauty target with no user-visible benefit in V2.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Code anchors.** `web/src/core/types.ts → DEFAULT_CONFIG`; `web/src/ui/controls.ts` (toggle
  hidden); `crates/brain-visualizer/src/sim/cpu/mod.rs → CpuBackend`; `crates/brain-visualizer/examples/cpu_check.rs` (parity
  harness still runnable natively).
- **Revisit when.** The GPU beauty target is confirmed stable — then decide
  revive or retire. Reviving requires porting heterogeneity, weight
  normalization, and input-mode plumbing to the CPU path.

## See also

- [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md)
- [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md)
- [`../architecture/simulation.md`](../architecture/simulation.md) — shared LIF dynamics
- [`../architecture/data-model.md`](../architecture/data-model.md) — SoA layout, packing
- [`../architecture/connectivity.md`](../architecture/connectivity.md) — shared procedural connectivity
- [`../architecture/scaling.md`](../architecture/scaling.md) — tier/N/K knobs; CPU ceiling
- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md) — threaded-WASM recipe
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
