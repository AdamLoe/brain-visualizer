# Brain Visualizer

Hardware-adaptive spiking-neural-network visualizer: point/LIF neurons on a
procedurally folded cortical manifold, locally wired by a deterministic hash
rule, simulated in real time with two interchangeable backends (WebGPU compute
/ CPU rayon). See `docs/` for the full architecture and locked decisions.

This is **Phase 1 — Foundation**: all module boundaries, key types, the
toolchain, and the real (non-stub) logic pieces are in place. The sim kernels
and rendering are stubbed (nothing draws yet).

## Layout

```
src/                 Rust crate (compiles to WASM; pure logic also host-tested)
  lib.rs             wasm_bindgen entry points + host-callable manifold build
  sim/               SimBackend trait, GPU/CPU backends (stub), adaptive scaler
  manifold/          icosphere + gyrification + region assignment (real)
  connectivity/      BV22 hash, integer spatial grid, target/weight (real)
  buffers.rs         chunked SoA layout math (real)
  profiler.rs        ring buffer + per-second JSON dump (real)
  gpu_limits.rs      adapter-limits → derived caps (real)
web/                 TypeScript harness (rAF loop, controls, camera, renderer)
public/              coi-serviceworker.js (COOP/COEP shim)
tests/               WGSL-vs-Rust hash determinism gate (BV22)
bench/               throwaway benchmark crate (NOT part of this workspace)
```

## Build & test

```bash
# 1. Host build + unit tests + golden vectors (no browser needed)
cargo build
cargo test

# 2. BV22 determinism gate: native WGSL vs Rust hash (runs under llvmpipe in
#    headless/WSL2 environments; needs /dev/dri access for a real adapter)
cargo test --test wgsl_hash_determinism -- --nocapture

# 3. WASM bundle (non-threaded scaffold path — see below)
wasm-pack build --target web        # or: npm run wasm

# 4. Web harness (typecheck + bundle). Builds the wasm pkg first.
npm install
npm run build                       # wasm-pack + tsc --noEmit + vite build

# 5. Dev server (COOP/COEP headers set; SharedArrayBuffer available)
npm run dev
```

The `build` script runs `wasm-pack build --target web` first (producing
`pkg/`), then `tsc --noEmit`, then `vite build`. `web/main.ts` imports the
generated `pkg/brain_visualizer.js` directly.

## Threaded WASM build (phase 6 concern)

The CPU backend (BV24) uses `wasm-bindgen-rayon`, which requires WASM threads:
a **nightly** toolchain, `RUSTFLAGS='-C target-feature=+atomics,+bulk-memory'`,
and a `build-std` rebuild of `std`:

```bash
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory' \
  rustup run nightly wasm-pack build --target web -- \
  -Z build-std=std,panic_abort --features cpu-threads
```

To keep the phase-1 scaffold building cleanly on stable, `wasm-bindgen-rayon`
and `rayon` are behind the **default-off `cpu-threads` cargo feature**. CPU
simulation lands in phase 6; the threaded build is set up then. The non-threaded
wasm build (`wasm-pack build --target web`) is the phase-1 deliverable.

## Browser verification (manual TODO — no browser in build env)

These cannot be checked headless and are deferred to a real browser:

- canvas appears (renderer clears to black);
- `crossOriginIsolated === true` (COOP/COEP active, SharedArrayBuffer);
- rAF loop runs and the profiler dumps one JSON line/sec to the console;
- speed presets visibly change tick rate;
- WebGPU adapter info / limits logged at startup.
```
