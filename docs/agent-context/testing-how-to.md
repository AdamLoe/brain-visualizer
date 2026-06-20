# Testing how-to

## When does this apply

You're about to declare a change done, or you're adding/triaging a test. This
doc names the gates and the offline verification surface; the build pipeline
itself is owned by
[`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md).

## The gates (run before "done")

Run `cargo` from `app/` (workspace root) and `npm` from `app/web/`.

- **`cargo test`** — host unit tests plus the integration gates in `crates/brain-visualizer/tests/`:
  the Rust/WGSL hash + target + weight determinism gates
  (`crates/brain-visualizer/tests/wgsl_hash_determinism.rs`, `crates/brain-visualizer/tests/wgsl_target_determinism.rs`),
  `crates/brain-visualizer/tests/wgsl_weight_determinism.rs`), sim dynamics
  (`crates/brain-visualizer/tests/gpu_sim_dynamics.rs`), fixed-point current
  overflow stress (`crates/brain-visualizer/tests/gpu_current_overflow.rs`),
  and 24-bit tick wrap (`crates/brain-visualizer/tests/wgsl_tick_wrap.rs`). These
  run under **llvmpipe** in headless/WSL2 — no real GPU needed (a software Vulkan
  adapter validates the WGSL).
- **`BV_REQUIRE_WGPU_TESTS=1 cargo test`** — same native test
  suite, but adapter/device acquisition failure is a hard failure. Without this
  env var, local no-adapter machines skip the native wgpu tests with explicit
  `SKIP ... no wgpu adapter/device` messages.
- **`cargo run -p brain-visualizer --example render_check`** — native render-path smoke gate:
  offscreen render, stimulation path, morphology draw, active-opacity deltas,
  bloom lazy allocation, and bloom path.
- **`cargo run -p brain-visualizer --example morph_view`** — native artifact/review harness:
  regenerates the accepted-default morphology views and writes the current `/tmp/morph_*`
  stats artifacts for manual/defaults verification.
- **`cargo run -p brain-visualizer --example time_network_payload --release`** —
  times the "Prepare network payload" boot phase at the 6k/K16 default and prints
  per-phase wall-clock ms (folding manifold, source types, morphology TOTAL +
  the MorphTimer breakdown setup/incoming/dendrite/axon/finalize, soma spheres),
  plus an emit-cadence check on `prepare_with_progress` (count, monotonicity, max
  gap between emits). **This is the local verification for the boot-stall fix:**
  the worker payload build is GPU-free (no WebGPU device), so the exact phase
  that parks the overlay can be measured on this no-adapter box. Use it to prove
  no payload phase exceeds ~2s of silent work and to spot which morphology
  sub-phase owns the time before touching it.
- **`npm run typecheck`** — `tsc --noEmit` over `web/`.
- **`npm test`** — vitest unit tests (e.g. `web/src/ui/controls.test.ts`,
  `web/src/core/settings-contract.test.ts`).
- **`npm run build`** — production-equivalent `wasm-pack build` + TypeScript check +
  Vite bundle. This is the shipping static-bundle gate, not just a dev-server check.
- **`npm run test:e2e`** — Playwright e2e (`web/e2e/*.spec.ts`). Needs a browser.
  The UX audit visual-proof spec requires a real WebGPU adapter by default and
  fails with an explicit adapter-unavailable blocker when Chromium only exposes
  fallback/non-adapter WebGPU; set `BV_REQUIRE_WEBGPU_VISUAL=0` only for local
  non-strict verification of the rest of that spec.
- **`npm run test:e2e:smoke`** — focused real-hardware/browser smoke. Writes a
  JSON artifact and screenshot with adapter availability, startup timings,
  nonblank canvas evidence, and frame-health samples; `BV_REQUIRE_WEBGPU=1`
  turns no-adapter from an environment skip into a failure.
- **`npm run test:e2e:responsiveness`** — focused browser responsiveness smoke
  for high-N worker-prepared rebuilds. It proves rAF/frame-counter progress
  during worker CPU preparation, not real-hardware WebGPU throughput.

When the change touches first-load defaults, reset behavior, presets, or build wiring,
also run a production preview check (`npm run preview`) and verify the built page loads
with COOP/COEP headers. If Chromium can load the page but `navigator.gpu.requestAdapter()`
returns no adapter, report that as an environment limitation; do not claim a real WebGPU
beauty pass from that run.

## Offline verification surface (examples)

The `crates/brain-visualizer/examples/*.rs` are runnable host checks that validate behavior without
a browser or when browser WebGPU is unavailable — the primary way to confirm sim/render
changes offline. What each covers is owned by
[`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
(e.g. `soc_sweep` = criticality sweep, `render_check` = render-path smoke). Run with
`cargo run --release --example <name>`.

## Gotchas

- **llvmpipe is software emulation.** Any throughput/perf number it produces is
  not representative of a real GPU and must not be locked into docs as a
  benchmark. It validates correctness, not speed.
- **Strict adapter mode can only prove the strict branch when no adapter is
  actually absent.** On machines with llvmpipe, `BV_REQUIRE_WGPU_TESTS=1` proves
  the tests run under strict mode, not the no-adapter failure path itself.
## See also

- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md) — build pipeline, examples, COOP/COEP.
- [`../architecture/connectivity.md`](../architecture/connectivity.md) — the determinism contract the gates protect.
- [`maintaining-docs.md`](maintaining-docs.md), [`index.md`](index.md).
