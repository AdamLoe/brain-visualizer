# Brain Visualizer

Hardware-adaptive spiking-neural-network visualizer: point/LIF neurons on a
procedurally folded cortical manifold, locally wired by a deterministic hash
rule, simulated in real time with two interchangeable backends (WebGPU compute
/ CPU rayon). See `docs/` for the full architecture and locked decisions.

This is **Phase 1 — Foundation**: all module boundaries, key types, the
toolchain, and the real (non-stub) logic pieces are in place. The sim kernels
and rendering are stubbed (nothing draws yet).

## Layout

The repo root holds only `docs/`, `app/`, `README.md`, and dotfiles. All source
lives under `app/`; **cargo runs from `app/`, npm runs from `app/web/`**.

```
app/                              Workspace root (run cargo here)
  crates/brain-visualizer/        The Rust crate (compiles to WASM; pure logic host-tested)
    src/                          lib.rs, sim/, manifold/, connectivity/, buffers.rs, …
    examples/                     Host-verification harnesses (`cargo run --example <name>`)
    tests/                        WGSL-vs-Rust determinism gates + gpu_sim_dynamics
  web/                            TypeScript app + JS project root (run npm here)
    package.json index.html       npm manifest + Vite entry
    vite/vitest/playwright/tsconfig configs (flat, conventional)
    main.ts … cpu-worker.ts       app modules; public/ (coi-serviceworker shim); e2e/ (Playwright)
docs/                            Architecture, decisions, agent-context (agent-docs v1)
```

## Build & test

```bash
# 1. Host build + unit tests + determinism gates (no browser needed) — from app/
cd app
cargo build
cargo test
cargo test --test wgsl_hash_determinism -- --nocapture   # native WGSL vs Rust hash (llvmpipe)

# 2. Web harness — from app/web/. `npm run build` runs wasm-pack first.
cd web
npm install
npm run build                       # wasm-pack (../crates/brain-visualizer) + tsc --noEmit + vite build

# 3. Dev server (COOP/COEP headers set; SharedArrayBuffer available)
#    Stop any stale server on :5173 first (`fuser -k 5173/tcp`) — Vite silently
#    falls back to 5174+, and the Playwright e2e suite hard-codes :5173.
npm run dev
```

`npm` (run from `app/web/`) builds the crate at `../crates/brain-visualizer`,
emitting its `pkg/`; `web/main.ts` imports
`../crates/brain-visualizer/pkg/brain_visualizer.js` directly.

## CPU backend & threaded WASM build (phase 6)

The CPU backend is event-driven (active-list) LIF on rayon, running the SAME
network as the GPU backend (shared `connectivity::target`/`weight`, same BV21
packing, same fixed-point current scale S=4096). It lives in `app/crates/brain-visualizer/src/sim/cpu/`
(`core.rs` = pure sim, native-testable; `mod.rs` = `CpuBackend`). Browser glue:
`web/cpu-worker.ts` (coordinator Web Worker, BV24) + `web/cpu-renderer.ts`
(WebGL2 instanced-billboard glow, GLSL ES 3.0 port of `render_far.wgsl`).

### Native verification (no browser needed — the real path)
The sim core is pure Rust and rayon runs natively. The harness runs the real
`CpuBackend` and compares it to the `GpuBackend` on identical seeds:

```bash
# Single-threaded core check:
cargo run --release --example cpu_check
# Host rayon (20 cores) — determinism + firing-rate parity vs GPU:
cargo run --release --example cpu_check --features cpu-threads
```

It asserts: first-100-targets CPU == shared Rust `target()` (== WGSL, the BV22
gate); CPU vs GPU mean firing rate within ±10% at `focused`; lazy decay and
render decay reach ≈0; and reports CPU ticks/s + syn-events/s.

### Threaded WASM build (browser, optional)
In the browser the rayon pool is `wasm-bindgen-rayon`, which requires WASM
threads: a **nightly** toolchain + `rust-src`, `RUSTFLAGS='-C
target-feature=+atomics,+bulk-memory'`, and a `build-std` rebuild of `std`:

```bash
cd app   # workspace root
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory' \
  rustup run nightly wasm-pack build crates/brain-visualizer --target web -- \
  -Z build-std=std,panic_abort --features cpu-threads
```

This recipe is **verified to compile** in this environment (nightly + rust-src).
To keep the default scaffold building cleanly on stable, `rayon` and
`wasm-bindgen-rayon` are behind the **default-off `cpu-threads` cargo feature**.
The default wasm build (`wasm-pack build --target web`) stays non-threaded; the
CPU backend then runs **single-threaded in the browser** (still correct, just
slower). Cross-origin isolation (COOP/COEP) is already set by the dev/preview
server and the `coi-serviceworker.js` shim, so SharedArrayBuffer/threads are
available where the threaded build is deployed.

## Browser verification (manual TODO — no browser in build env)

These cannot be checked headless and are deferred to a real browser:

- canvas appears (renderer clears to black);
- `crossOriginIsolated === true` (COOP/COEP active, SharedArrayBuffer);
- rAF loop runs and the profiler dumps one JSON line/sec to the console;
- speed presets visibly change tick rate;
- WebGPU adapter info / limits logged at startup.
```
