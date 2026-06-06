---
status:        active
owner:         adamg
last_updated:  2026-06-04
---

# Build and Deploy

How the Rust/WASM + TypeScript codebase is compiled, tested, and shipped. The
one job: produce a static bundle (WASM + JS/CSS) that a GitHub Pages host can
serve with the correct cross-origin isolation headers so WebGPU and
SharedArrayBuffer both work.

## What it owns

- The npm scripts and their ordering (`web/package.json → scripts`).
- The cross-platform Rust crate: `cdylib` for WASM, `rlib` for host unit tests (`crates/brain-visualizer/Cargo.toml → [lib]`).
- The `cpu-threads` cargo feature and the two different thread backends it unlocks (`crates/brain-visualizer/Cargo.toml → [features]`).
- COOP/COEP header strategy: dev/preview server headers (`web/vite.config.ts → crossOriginIsolation`) and the static-host service-worker shim (`web/public/coi-serviceworker.js`).
- The offline verification surface: the `crates/brain-visualizer/examples/` harnesses (see below).
- The test gates: `cargo test`, vitest unit tests, Playwright e2e (`web/package.json → scripts`).
- The `wasmHotRebuild` Vite plugin that watches `crates/brain-visualizer/src/**/*.rs` and `crates/brain-visualizer/Cargo.toml` during `dev` and triggers debounced `wasm-pack build --dev` + full browser reload (`web/vite.config.ts → wasmHotRebuild`).

## What it does NOT own

- GPU sim dynamics and shader logic — [`gpu-backend.md`](gpu-backend.md).
- Connectivity / hash determinism rule — [`connectivity.md`](connectivity.md) (build-and-deploy owns only that the gate test exists and how to run it).
- Adaptive scaler and tier caps — [`scaling.md`](scaling.md).

## Build pipeline

`npm` runs from `app/web/` (the JS project root); its configs are the
conventional flat `web/{vite,vitest,playwright,tsconfig}` files. The production
build runs three steps in order:

```
npm run build   (in app/web/)
  └─ wasm-pack build ../crates/brain-visualizer --target web   (Cargo → pkg/ in the crate)
  └─ tsc --noEmit                                              (typecheck only; vite handles transpile)
  └─ vite build                                               (bundle + emit dist/)
```

The wasm-pack output `pkg/` lands inside the crate
(`crates/brain-visualizer/pkg/`); `web/src/main.ts` and `web/src/cpu/cpu-worker.ts` import
`../crates/brain-visualizer/pkg/brain_visualizer.js`.

Dev mode (`npm run dev`) runs `wasm-pack build … --dev` once, then starts the
Vite dev server. The `wasmHotRebuild` Vite plugin watches the crate's `.rs`
files and `crates/brain-visualizer/Cargo.toml`; on change it debounces 150 ms,
spawns another `wasm-pack build --dev`, and sends `full-reload` to the browser.
Rebuilds are serialized: a burst of saves collapses into one build.

`npm run preview` serves the already-built `dist/` with the same COOP/COEP
headers as `dev`, making it the closest local approximation of the deployed site.

## Cross-platform Rust crate

The crate builds on both `x86_64` (host: `cargo build`, `cargo test`, `cargo run
--example`) and `wasm32-unknown-unknown` (browser target via `wasm-pack`).
WASM-only glue (`wasm-bindgen`, `web-sys`, `console_error_panic_hook`,
`wasm-bindgen-rayon`) is gated behind
`#[target.'cfg(target_arch = "wasm32")'.dependencies]` so host builds stay clean.

## The `cpu-threads` cargo feature

The feature is **off by default** (the stable WASM build stays single-threaded).
What it does differs by target:

| Target | Effect |
|---|---|
| Host (native) | Enables `rayon` directly — multi-threaded CPU backend on x86_64. |
| WASM | Enables `wasm-bindgen-rayon` — requires a **nightly** toolchain, `RUSTFLAGS="-C target-feature=+atomics,+bulk-memory"`, and a `build-std` rebuild of `std`. See README §"Threaded WASM build". |

The WASM-threaded path also requires COOP/COEP to be active (for
`SharedArrayBuffer`). The default `npm run wasm` and `npm run build` scripts use
stable and do not set these flags; the threaded-WASM recipe is a manual step
documented in the README.

## COOP/COEP and SharedArrayBuffer

`SharedArrayBuffer` (required by `wasm-bindgen-rayon` and used by the CPU
coordinator worker) is gated behind cross-origin isolation. Two delivery paths:

- **Dev and preview servers:** Vite injects `Cross-Origin-Opener-Policy:
  same-origin` and `Cross-Origin-Embedder-Policy: require-corp` on every
  response (`web/vite.config.ts → crossOriginIsolation`).
- **GitHub Pages / static hosts:** Cannot set custom headers. The
  `web/public/coi-serviceworker.js` shim (coi-serviceworker v0.1.7) is registered
  on first load; it intercepts fetches and adds the required headers so
  `crossOriginIsolated === true` on subsequent loads.

A key gotcha: on the very first page load the service worker is not yet
registered, so `crossOriginIsolated` is false. The CPU threaded path gracefully
degrades (single-threaded) rather than crashing. `crates/brain-visualizer/src/lib.rs →
log_cross_origin_isolation` logs the isolation state at boot for debugging.

The ES-module worker format (`web/vite.config.ts → worker: { format: "es" }`) is
required for code-splitting inside the CPU coordinator worker and for
`crossOriginIsolated` to propagate into the worker context.

## Offline verification surface (the examples)

The `crates/brain-visualizer/examples/` directory contains runnable host harnesses that exercise the
production Rust code against the native wgpu device (llvmpipe on WSL2). They are
the primary offline correctness gate because the browser is not available in the
build environment.

**Key gotcha: llvmpipe is a CPU software rasteriser exposed as a Vulkan ICD.
Numbers from these harnesses are software-emulation throughput, not real GPU
performance. They validate shader correctness and dynamics logic; they do not
substitute for browser WebGPU numbers on real hardware.**

| Example | How to run | What it verifies |
|---|---|---|
| `cpu_check.rs` | `cargo run --release --example cpu_check --features cpu-threads` | CPU/GPU parity: first 100 synapse targets, firing-rate agreement within ±10%, lazy decay, render decay. |
| `sim_check.rs` | `cargo run --release --example sim_check` | GPU dynamics: non-zero spikes, excitability sweep (sleep→seizure), no NaN/overflow under seizure, i32 accumulator range. |
| `soc_sweep.rs` | `cargo run --release --example soc_sweep` | Criticality sweep: i_ext parameter sweep + five brain-state acceptance bands. |
| `render_check.rs` | `cargo run --release --example render_check` | Render pipeline: offscreen render to 512×512 texture, non-black pixels, distinct region colours, stimulation response, zero Naga shader-compile errors. |
| `near_lod_check.rs` | `cargo run --release --example near_lod_check` | Near-LOD retirement: instance counts at close/far distance, clamp/overflow counters. |
| `morph_view.rs` | `cargo run --example morph_view` | Morphology renderer: renders three camera views to `/tmp/morph_{0,1,2,3}.rgba` for manual visual inspection; asserts non-black pixels. |

## Test gates

Three test surfaces, all runnable offline:

**`cargo test`** — unit + integration tests on the host. Includes:
- `crates/brain-visualizer/src/gpu_limits.rs` — `GpuCaps::derive` correctness against fixture inputs.
- `crates/brain-visualizer/src/sim/scaler.rs` — `propose` shrink/grow/clamp logic.
- `crates/brain-visualizer/tests/wgsl_hash_determinism.rs` — runs the production `hash.wgsl` under
  llvmpipe and compares golden-vector output to the Rust `hash32`/`mix_key`
  implementation. Skips (not fails) if no adapter is available.
- `crates/brain-visualizer/tests/wgsl_target_determinism.rs` — proves `target_neuron` WGSL and Rust
  `connectivity::target()` produce bit-identical synapse targets for a real
  manifold grid under llvmpipe. The definitive cross-language determinism gate.
- `crates/brain-visualizer/tests/gpu_sim_dynamics.rs` — drives the GPU backend through an excitability
  sweep and asserts qualitative dynamics (non-zero spikes, seizure > focused,
  no NaN/overflow). Skips if no adapter available.

**`npm test` (vitest)** — pure-logic TypeScript unit tests (`web/**/*.test.ts`).
Runs in Node without a browser. Covers `scalerDecide`, `tickExcitability`, and
other pure functions.

**`npm run test:e2e` (Playwright)** — browser integration tests
(`web/e2e/brain_visualizer.spec.ts`). Covers: smoke/boot (WASM loads, no
`recursive use of an object` panic), WebGPU adapter presence (gated: skips when
no adapter), resize reentrancy regression, controls correctness, CPU backend
toggle. Requires the dev server running on `localhost:5173`; set
`USE_WEBSERVER=1` (`npm run test:e2e:server`) for Playwright to start it
automatically. The port is hard-coded: Vite falls back to 5174+ when 5173 is
already held by a stale `npm run dev`, and a manual e2e run would then point at
the wrong server. Stop the process on 5173 before starting (see
[`../agent-context/dev-loop.md`](../agent-context/dev-loop.md) → "Run the app").

## Update when

- A new `crates/brain-visualizer/examples/` harness is added (update the table above).
- The `cpu-threads` threaded-WASM recipe changes (nightly flags, `build-std`).
- The COOP/COEP strategy changes (e.g. GitHub Pages gains header support).
- `wasm-pack` target or `web/vite.config.ts` worker format changes.
- New test files are added to `crates/brain-visualizer/tests/` or `web/**/*.test.ts`.

## See also

- [`scaling.md`](scaling.md) — tier presets, adaptive scaler, adapter caps
- [`gpu-backend.md`](gpu-backend.md) — GPU sim and render architecture
- [`cpu-backend.md`](cpu-backend.md) — CPU backend and worker topology
- [`connectivity.md`](connectivity.md) — hash determinism rule (the gate test proves the invariant)
- [`../decisions/scaling.md`](../decisions/scaling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
