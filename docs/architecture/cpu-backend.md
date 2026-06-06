---
status:        parked
owner:         adamg
last_updated:  2026-06-04
---

# CPU Backend

Event-driven, active-list LIF simulation running on (optional) rayon, with a
WebGL2 instanced renderer. The CPU backend simulates the same network as the GPU
backend and was built to enable a direct CPU-vs-GPU comparison. It compiles and
passes parity tests, but is **unwired in V2**: GPU is forced at boot, the toggle
is hidden, and no V2 feature (heterogeneity, weight normalization, input modes,
ribbons) has a CPU implementation.

## What it owns

- `crates/brain-visualizer/src/sim/cpu/mod.rs → CpuBackend` — the `SimBackend` impl: initialize,
  tick loop, stimulate, render\_state, destroy.
- `crates/brain-visualizer/src/sim/cpu/core.rs → LifParams` — LIF constants (locked to match
  `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`); the single authoritative list of
  CPU-side dynamics parameters.
- `crates/brain-visualizer/src/sim/cpu/core.rs → CpuNeuronBuffers` — per-neuron SoA: `v`, `last_spike`,
  `i` (AtomicI32), `last_updated`, `v_render`, `input_neurons`, `cell_coord`.
- `crates/brain-visualizer/src/sim/cpu/core.rs → integrate_neuron`, `integrate_active` — single-neuron
  and active-list integration with lazy decay.
- `crates/brain-visualizer/src/sim/cpu/core.rs → scatter_tick`, `scatter_one_source` — parallel scatter:
  fixed-point `fetch_add` into target accumulators, rayon-gated by `cpu-threads`.
- `crates/brain-visualizer/src/sim/cpu/core.rs → update_v_render` — render snapshot: decays `v_render`
  for silent neurons before each WebGL upload.
- `web/src/cpu/cpu-worker.ts → handleInit`, `onmessage` — coordinator Web Worker: owns
  the WASM instance, optionally initializes the `wasm-bindgen-rayon` pool,
  advances ticks, posts SoA pointers over the SharedArrayBuffer bridge.
- `web/src/cpu/cpu-renderer.ts → CpuRenderer` — WebGL2 instanced billboard renderer:
  uploads `v_render` and `last_spike` each frame, draws N instanced quads with
  the same glow/region logic as `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl`.
- `crates/brain-visualizer/examples/cpu_check.rs` — native offline parity harness (see Parity check).
- `crates/brain-visualizer/Cargo.toml [features] cpu-threads` — gates rayon on native and
  wasm-bindgen-rayon on WASM (off by default; see below).

## What it does NOT own

- LIF dynamics definition → [`simulation.md`](simulation.md) (links here for the CPU-side mirror).
- SoA field layout and `last_spike` packing → [`data-model.md`](data-model.md).
- Procedural connectivity (`target`, `weight`) → [`connectivity.md`](connectivity.md).
- Threaded-WASM build recipe (nightly + build-std + COOP/COEP headers) →
  [`build-and-deploy.md`](build-and-deploy.md); this doc only notes the feature flag.
- GPU backend, WebGPU pipelines, compute shaders → [`gpu-backend.md`](gpu-backend.md).

## Sim core: pure Rust, native-testable

`crates/brain-visualizer/src/sim/cpu/core.rs` has no WASM or browser dependencies and runs identically
on the host and in WASM. Key invariants:

- **Same network as GPU.** Connectivity is derived from the shared
  `crates/brain-visualizer/src/connectivity → target_with_cell` / `weight`, seeded by the same
  `SimConfig.seed`. `CpuNeuronBuffers::build` packs `last_spike` with the same
  silent-start layout as the GPU path (`HAS_SPIKED=0`, type bits set, tick=0`).
- **Same fixed-point scale.** Current accumulates as i32 with S = 4096
  (`fixed_point_scale`), matching `integrate.wgsl`. Each synapse contributes via
  `AtomicI32::fetch_add`; the accumulator is swapped to 0 on integrate
  (`Ordering::AcqRel`). No per-thread partial buffers — atomic-add is the single
  scatter model.
- **Lazy decay.** Neurons not in the active list are not integrated each tick.
  When they reappear, `integrate_neuron` applies `leak_decay^(ticks_dormant - 1)`
  in one multiply before the normal step. `update_v_render` applies the same
  decay to `v_render` for all neurons before each WebGL upload so silent neurons
  lose glow without being re-scheduled.
- **Active-list propagation.** After scatter, `touched_out` (target ids +
  `input_neurons`, sorted and deduped) becomes the next tick's `active` list via
  `std::mem::swap`. Input neurons are always present so ambient `i_ext` flows
  each tick.
- **Precomputed cell coords.** `CpuNeuronBuffers.cell_coord` is filled once at
  build time from `SpatialGrid::cell_of_neuron_map`. The scatter hot path calls
  `target_with_cell` (O(1)) instead of `target` (which re-scans the grid).

## Browser topology

```
main thread                 coordinator Web Worker
───────────────             ──────────────────────────────────────────
input / camera         →    structured messages (init/tick/stim/destroy)
WebGL2 render          ←    SoA pointers (vRenderPtr, lastSpikePtr)
                            WASM instance
                            wasm-bindgen-rayon pool (if cpu-threads + COOP/COEP)
                            active-list tick loop
                            SharedArrayBuffer (wasm linear memory)
```

`web/src/cpu/cpu-worker.ts → handleInit` owns the WASM lifecycle: loads the module,
optionally initialises the rayon thread pool when `crossOriginIsolated` is true, and
constructs `WasmCpuBackend`. It posts SoA pointers (`vRenderPtr`, `lastSpikePtr`) back
to the main thread after init and after every tick; pointers are resent each tick
because a Vec reallocation would silently move them.

`web/src/cpu/cpu-renderer.ts → CpuRenderer.render` does a full `bufferSubData` upload
of `v_render` and `last_spike` each frame and draws N instanced quads. The GLSL vertex
shader mirrors `render_far.wgsl`: glow, region colour, and billboard expansion are
kept in sync by hand.

## `cpu-threads` cargo feature

Off by default. Adds `rayon` (native) and `wasm-bindgen-rayon` (WASM). On
native, `scatter_map` uses `fired.par_chunks(256)`; without the feature it falls
back to a single-threaded loop with identical logic. The threaded-WASM build
additionally requires a nightly toolchain and `build-std` — see
[`build-and-deploy.md`](build-and-deploy.md) for the recipe. The browser runtime
checks `crossOriginIsolated` before attempting the pool; it degrades gracefully
to single-threaded if COOP/COEP headers are absent.

## Parity check

`crates/brain-visualizer/examples/cpu_check.rs` is the offline harness. Run:

```
cargo run --release --example cpu_check --features cpu-threads
```

Four gates: (1) determinism — first 100 targets for neuron 0 match
`connectivity::target` bit-for-bit; (2) firing-rate parity — CPU vs GPU within
±10% at `focused` excitability; (3) lazy decay — a neuron silent 500 ticks
reaches `|v| < 1e-6`; (4) render decay — `v_render` of an untouched neuron
reaches `|v_render| < 1e-6`. Gate (1) uses `connectivity::target_with_cell`;
the WGSL == Rust closure is proved by `crates/brain-visualizer/tests/wgsl_target_determinism.rs`.

## Parked in V2

The repo is V2 = GPU-only. `DEFAULT_CONFIG.backend = "gpu"` in `web/src/core/types.ts`
forces GPU at boot. The backend toggle (`web/src/ui/controls.ts`) is hidden. None of
the V2 GPU features (per-neuron heterogeneity, weight normalization, input
modes, ribbon renderer) have a CPU counterpart. The CPU code compiles clean and
the parity harness passes, but nothing in the V2 render/sim path invokes
`CpuBackend` or `CpuRenderer`. The revive-or-retire decision is deferred until
the GPU beauty target is confirmed stable.

## Update when

- `crates/brain-visualizer/src/sim/cpu/core.rs → LifParams` constants change (must stay equal to
  `integrate.wgsl`).
- `CpuNeuronBuffers` gains or drops a field.
- The scatter model changes (e.g., spatial-partitioned atomics, SIMD128).
- `web/src/cpu/cpu-worker.ts` message protocol changes (new message types, pointer
  semantics change).
- `cpu-threads` feature dependencies change.
- The parked / unwired status changes (the revive-or-retire decision).

## See also

- [`simulation.md`](simulation.md) — LIF dynamics shared by both backends
- [`data-model.md`](data-model.md) — SoA layout, `last_spike` packing
- [`connectivity.md`](connectivity.md) — `target`, `weight`, `target_with_cell`
- [`gpu-backend.md`](gpu-backend.md) — the active backend in V2
- [`scaling.md`](scaling.md) — neuron/K tiers; CPU ceiling (~100k–500k)
- [`../decisions/backends.md`](../decisions/backends.md)
- [`build-and-deploy.md`](build-and-deploy.md) — threaded-WASM recipe, COOP/COEP
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
