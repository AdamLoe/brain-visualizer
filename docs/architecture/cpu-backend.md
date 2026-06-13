---
status:        retired
owner:         adamg
last_updated:  2026-06-13
---

# CPU Backend

There is no live CPU/WebGL2 simulation backend in the current product. The app
is GPU-only: browser startup, rebuilds, rendering, tests, examples, and the Rust
WASM export surface all route through `GpuBackend` / `WasmGpuBackend`.

The retired CPU implementation is not feature-gated or archived in source. Git
history is the archive.

## What exists now

- `crates/brain-visualizer/src/sim/backend.rs → BackendKind` has a single live
  variant: `Gpu`.
- `crates/brain-visualizer/src/sim/mod.rs` exports the shared backend contract
  and the GPU implementation only.
- `crates/brain-visualizer/src/lib.rs` exports GPU/manifold/prepared-network
  WASM entry points only; there is no CPU WASM wrapper or threaded-pool export.
- `web/src/main.ts` owns one runtime backend reference, `WasmGpuBackend`, and
  restarts that GPU backend for tier/reset-style config changes.
- `web/src/core/types.ts → loadConfig` normalizes stale saved `backend: "cpu"`
  payloads to `"gpu"` so old localStorage data cannot break startup.
- `crates/brain-visualizer/Cargo.toml` has no CPU-thread feature and no rayon
  or threaded-WASM dependency.

## What was removed

- Rust CPU simulation modules.
- The CPU browser binding and threaded-pool export.
- The TypeScript CPU coordinator worker and WebGL2 renderer under
  the web source tree.
- CPU restart/stimulation/render branches in `web/src/main.ts`.
- CPU-only example and e2e expectations.
- The CPU-thread / threaded-WASM build recipe.

## Operational consequences

There is no backend toggle, fallback WebGL2 renderer, second-runtime parity
example, or threaded-WASM lane to maintain. Cross-origin isolation still matters
for the static-host COOP/COEP strategy and for browser WebGPU behavior, but not
for CPU-worker SharedArrayBuffer or rayon setup.

When adding new simulation behavior, implement it in the GPU path and the Rust
host-side determinism helpers that the WGSL gates compare against. Do not add a
parallel CPU runtime path unless the product decision changes.

## See also

- [`gpu-backend.md`](gpu-backend.md) — the live simulation backend
- [`web-frontend.md`](web-frontend.md) — browser startup, rAF ownership, stale
  app-config normalization
- [`build-and-deploy.md`](build-and-deploy.md) — current build/test surface
- [`../decisions/backends.md`](../decisions/backends.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
