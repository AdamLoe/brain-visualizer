# Decisions — Backends

## GPU is the only live runtime backend

- **Decision.** The product has one live simulation/rendering backend:
  `GpuBackend` exposed to the browser as `WasmGpuBackend`.
- **Why.** Maintaining a second CPU/WebGL2 runtime doubled the surface area for
  every simulation, settings, rendering, and startup change while the shipped
  experience is the WebGPU sculpture.
- **Applies to.** [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/backend.rs → BackendKind`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend`;
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend`.

## Delete retired backend code instead of feature-gating it

- **Decision.** Retired CPU/WebGL2 backend code is deleted from source rather
  than hidden behind a cargo feature or kept as parked modules.
- **Why.** The current app should be honest about what can run. Git history is
  enough archive for a removed experiment, and keeping dead backend code would
  keep forcing drift work across Rust, WASM, TypeScript, docs, and tests.
- **Applies to.** [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md),
  [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md).
- **Tradeoffs.** Reviving a CPU comparison later becomes a deliberate rebuild
  from history, not an assumed dormant path.

## Stale CPU app configs normalize to GPU

- **Decision.** Old localStorage app configs that contain `backend: "cpu"` are
  accepted but normalized to `"gpu"` during `web/src/core/types.ts → loadConfig`.
- **Why.** Visitors with old saved configs must not fail startup after the CPU
  implementation is removed, but keeping the field tolerant should not keep any
  CPU runtime code alive.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Code anchors.** `web/src/core/types.ts → loadConfig`.

## Unsupported WebGPU fails closed with visitor guidance

- **Decision.** Browsers without `navigator.gpu`, or devices that fail WebGPU
  startup, keep the startup overlay visible with visitor-facing WebGPU guidance
  and do not attempt a CPU/WebGL fallback.
- **Why.** The product has one honest runtime path. A clear unsupported state is
  better than reviving a second backend or exposing raw adapter diagnostics as
  the page-level message.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md).
- **Code anchors.** `web/src/boot-failure.ts → hasWebGpuSupport, webGpuUnsupportedStage`;
  `web/src/main.ts → startGpuBackend`.

## Same-seed backend switching is no longer a runtime contract

- **Decision.** Backend switching, parity examples for a second runtime, and threaded-WASM
  CPU build recipes are not part of the current runtime or verification surface.
- **Why.** Determinism is still enforced where it matters: Rust host helpers and
  WGSL kernels must agree on hash/connectivity rules, and GPU simulation gates
  exercise the live backend. A deleted CPU runtime no longer needs parity UX or
  worker-thread build support.
- **Applies to.** [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/tests/wgsl_hash_determinism.rs`;
  `crates/brain-visualizer/tests/wgsl_target_determinism.rs`.

## See also

- [`../architecture/cpu-backend.md`](../architecture/cpu-backend.md)
- [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md)
- [`../architecture/web-frontend.md`](../architecture/web-frontend.md)
- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
- [`../architecture/connectivity.md`](../architecture/connectivity.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
