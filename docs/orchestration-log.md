# Brain Visualizer — Orchestration Log

_Running record of the bootstrap orchestration: phase status, autonomous
decisions made by the orchestrator (not pre-locked in `decisions.md`), and
verification notes. Started 2026-06-03._

## Environment (as built)
- WSL2, 20 cores, 31 GB RAM, `/dev/dri/card0` present (GPU passthrough possible).
- Rust 1.95.0, cargo 1.95.0, wasm-pack 0.15.0, node 20.20.2, npm 10.8.2.
- wasm32-unknown-unknown target installed.
- **No browser installed.** The shipped app is browser-only
  (WebGPU/WebGL2/WASM-threads), so browser-runtime verification is a documented
  manual step, not performed in this environment.

## Verification policy (decided with user, 2026-06-03)
- Verify via: `cargo build`/`cargo test` (native), `wasm-pack build` (compile),
  Rust unit + golden-vector tests, and the native wgpu benchmark.
- Browser runtime checks (visuals, WebGPU device, COOP/COEP, rayon pool) are
  listed per phase as **manual TODOs for the user** — not blockers for "built".

## Crate versions (verified latest stable, 2026-06-03)
- wgpu 29.0.3 (matches doc pin "29")
- noise 0.9.0
- wasm-bindgen 0.2.122, wasm-bindgen-rayon 1.3.0

## Orchestrator decisions (autonomous; small calls not in decisions.md)
- **OD1 — Project layout:** project is already extracted to its own repo
  (`/home/adamg/brain_visualizer`). Source lives at repo root (`src/`, `web/`,
  `public/`, `bench/`, `Cargo.toml`, `package.json`) rather than nested under a
  `brain-visualizer/` subfolder. The phase docs' `brain-visualizer/` prefix is
  interpreted as the repo root.
- **OD2 — Git:** repo initialized; one commit per completed phase.

## Phase status
| Phase | Status | Commit | Notes |
|-------|--------|--------|-------|
| 0 Benchmark | complete | — | GPU=llvmpipe (no real GPU); CPU numbers collected; browser TODO |
| 1 Foundation | pending | — | |
| 2 GPU sim | pending | — | |
| 3 GPU render | pending | — | |
| 4 Near LOD | pending | — | |
| 5 Controls | pending | — | |
| 6 CPU backend | pending | — | |
| 7 Polish | pending | — | |

## Phase closeouts
_(Each phase appends a short closeout here: what was built, what was verified,
what is a manual/browser TODO, and any decisions made.)_

### Phase 0 — Benchmark Spike (2026-06-03)

**Built:** Standalone native Rust benchmark crate at `bench/` (isolated, not
a workspace member). Native GPU path (wgpu 29) + CPU path (rayon). Minimal
WGSL integrate + scatter shaders using the exact BV22 hash32/mix_key. Fixed-
point i32 scatter (S=4096, BV19). 2D scatter dispatch to work around
`maxComputeWorkgroupsPerDimension=65535`. GPU benchmark has graceful fallback
on adapter failure. Web microbench stub at `bench/web/` compiles via wasm-pack
(target web) but was not run.

**GPU adapter:** NOT found (real GPU). `/dev/dri/renderD128` and `card0`
returned `Permission denied` under WSL2. wgpu fell back to llvmpipe (software
Vulkan CPU emulation). GPU numbers are CPU emulation, not real GPU — discarded
for planning.

**Headline CPU numbers (rayon, 20 cores):**
- N=100k K=32: ~442 ticks/s, ~2.2 M syn-events/s
- N=500k K=32: ~388 ticks/s, ~10.4 M syn-events/s
- N=50k  K=64: ~390 ticks/s, ~2.2 M syn-events/s

**Manual TODOs for user:**
1. Grant GPU permissions (`sudo chmod a+rw /dev/dri/renderD128`) or set up a
   Vulkan ICD, re-run native bench to get real GPU numbers.
2. Serve `bench/web/index.html` with COOP+COEP headers in a real browser with
   WebGPU support; collect browser numbers and paste into architecture.md §9.1.
3. Browser WebGPU numbers are required before tier caps are locked for Phase 1.

**Decisions:** Tier caps from §9 remain provisional. 10M stretch path rejected
until confirmed by browser numbers. CPU Low tier realistic ceiling is ~10k–20k
neurons at 60 fps on a 4-core device (not 100k as initially assumed).
