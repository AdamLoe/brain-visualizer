# Brain Visualizer

Hardware-adaptive spiking-neural-network visualizer: point/LIF neurons on a
procedurally folded cortical manifold, locally wired by a deterministic hash
rule, simulated and rendered in real time by the WebGPU backend. See `docs/`
for the full architecture and locked decisions.

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
    src/ public/ e2e/             app modules, COI service-worker shim, Playwright specs
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

# 3. Dev server (COOP/COEP headers set for WebGPU/WASM support)
#    Stop any stale server on :5173 first (`fuser -k 5173/tcp`) — Vite silently
#    falls back to 5174+, and the Playwright e2e suite hard-codes :5173.
npm run dev
```

`npm` (run from `app/web/`) builds the crate at `../crates/brain-visualizer`,
emitting its `pkg/`; `web/src/main.ts` imports
`../crates/brain-visualizer/pkg/brain_visualizer.js` directly.

## Current runtime

The browser product has one live backend: WebGPU compute plus WebGPU rendering.
Startup prepares the network payload in a worker where possible, then the main
thread uploads GPU resources and owns all WebGPU calls. The hidden dev panel
exposes network rebuild controls, live visual settings, morphology batching, and
storage diagnostics.

Deleted runtime boundaries are documented in `docs/architecture/`; reviving an
alternate backend would be new product work.

## Browser verification

Use `npm run test:e2e` from `app/web/` for the Playwright suite. For manual
checks, load the Vite dev server in a WebGPU-capable browser and confirm the
startup overlay reaches the first rendered frame, the pause control freezes
simulation ticks while camera motion remains live, and `?dev=1` opens the hidden
panel.
