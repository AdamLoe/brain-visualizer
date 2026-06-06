# Testing how-to

## When does this apply

You're about to declare a change done, or you're adding/triaging a test. This
doc names the gates and the offline verification surface; the build pipeline
itself is owned by
[`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md).

## The gates (run before "done")

Run `cargo` from `app/` (workspace root) and `npm` from `app/web/`.

- **`cargo test`** — host unit tests plus the integration gates in `crates/brain-visualizer/tests/`:
  the CPU/GPU hash + target determinism gates
  (`crates/brain-visualizer/tests/wgsl_hash_determinism.rs`, `crates/brain-visualizer/tests/wgsl_target_determinism.rs`) and the
  sim-dynamics check (`crates/brain-visualizer/tests/gpu_sim_dynamics.rs`). These run under **llvmpipe**
  in headless/WSL2 — no real GPU needed (a software Vulkan adapter validates the
  WGSL).
- **`npm run typecheck`** — `tsc --noEmit` over `web/`.
- **`npm test`** — vitest unit tests (e.g. `web/controls.test.ts`).
- **`npm run test:e2e`** — Playwright e2e (`web/e2e/`). Needs a browser.

## Offline verification surface (examples)

The `crates/brain-visualizer/examples/*.rs` are runnable host checks that validate behavior without
a browser — the primary way to confirm sim/render changes offline. What each
covers is owned by
[`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
(e.g. `cpu_check` = CPU/GPU parity, `soc_sweep` = criticality sweep). Run with
`cargo run --release --example <name>`.

## Gotchas

- **llvmpipe is software emulation.** Any throughput/perf number it produces is
  not representative of a real GPU and must not be locked into docs as a
  benchmark. It validates correctness, not speed.
- The threaded CPU path needs the `cpu-threads` feature (and, on wasm, a nightly
  build-std recipe) — see
  [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md).

## See also

- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md) — build pipeline, examples, COOP/COEP.
- [`../architecture/connectivity.md`](../architecture/connectivity.md) — the determinism contract the gates protect.
- [`maintaining-docs.md`](maintaining-docs.md), [`index.md`](index.md).
