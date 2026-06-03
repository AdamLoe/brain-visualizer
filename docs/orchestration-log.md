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
| 1 Foundation | complete | — | scaffold builds (host+wasm), 58 tests pass, BV22 WGSL=Rust gate PASS (llvmpipe); CPU-threads wasm build deferred to phase 6 |
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

### Phase 1 — Foundation (2026-06-03)

**Built:** Full scaffold at repo root (OD1): Rust crate (`src/`) + TS harness
(`web/`) + `public/coi-serviceworker.js` + `index.html` + `package.json` +
`vite.config.ts` + `tsconfig.json` + root `README.md`. `bench/` untouched and
excluded from the workspace (`exclude = ["bench"]`).

Module layout matches phase-1 doc / architecture §10.1 (no god-object):
- `connectivity/hash.rs` — BV22 `hash32`/`mix_key` verbatim, golden-vector tests.
- `connectivity/spatial.rs` — integer grid, packed `u32` cell ids, CSR membership
  (no string keys, BV §10.1).
- `connectivity/mod.rs` — integer-only `target()` (spatial-local + anterior bias
  for excitatory) and fixed-point `weight()` (S=4096, BV19).
- `manifold/` — icosphere (L5 → ~10k verts), 2-octave OpenSimplex gyrification,
  barycentric neuron placement, anterior–posterior region split (`regions.rs`).
- `buffers.rs` — `ChunkedBuffer`/`ChunkLayout` (≤64 MiB/chunk, host-testable math).
- `profiler.rs` — `RingBuffer<120>`, injected clock, per-second JSON snapshot.
- `gpu_limits.rs` — `LimitsInput`→`GpuCaps` derivation (host-testable).
- `sim/backend.rs` — `SimBackend` trait, `SimConfig` (incl `fixed_point_scale`),
  `TickStats`, `RenderState`, `SpeedPreset`/`BackendKind`/`Tier`, BV21 packing
  helpers.
- `sim/gpu/` — `GpuBackend` stub, `GpuResources` lifecycle (resize/refresh/destroy
  + `bind_groups_dirty`), `GpuPipelines` (embeds WGSL via `include_str!`), WGSL
  stub shaders (`hash.wgsl` is the real BV22; integrate/scatter are stubs).
- `sim/cpu/` — `CpuBackend` stub. `sim/scaler.rs` — proposal-only adaptive scaler.

**Verified (this environment):**
- `cargo build` (host) — clean, 0 warnings.
- `cargo build --target wasm32-unknown-unknown` — clean.
- `wasm-pack build --target web` — clean (293 KB wasm). pkg exports
  `init_manifold`, `log_cross_origin_isolation`, `start`.
- `cargo test` — 58 pass (57 unit/golden + 1 WGSL determinism integration).
- **BV22 gate (`tests/wgsl_hash_determinism.rs`): PASS** — WGSL `hash32`/`mix_key`
  run natively under llvmpipe match Rust exactly for 9 golden vectors. This gate
  must hold before phase-2 GPU sim.
- Manifold sanity: N placed exactly; region split ≈30/40/30; deterministic.
- Connectivity: deterministic, targets in-range, E>0 / I<0 weights, anterior
  bias measurable.
- `npm install` OK; `tsc --noEmit` OK; `vite build` OK (bundles the wasm pkg).

**Decisions / deviations:**
- **OD3 — Threaded wasm deferred to phase 6.** `wasm-bindgen-rayon` requires
  nightly + `+atomics,+bulk-memory` + build-std and fails on the stable
  scaffold. Gated behind a default-off `cpu-threads` cargo feature; the
  non-threaded wasm build is the phase-1 deliverable. Threaded build recipe
  documented in `README.md`. CPU sim lands in phase 6, so this is in-scope-later.
- **OD4 — `target()` signature extended.** Doc shows `target(i,j,grid,k)`; impl
  is `target(i,j,grid,k,seed,source_type)` because real determinism keys on
  `SimConfig.seed` and the anterior bias needs the E/I flag. `seed` is `mix_key`'s
  `seed_lo`. Documented in the module.
- **OD5 — wgpu/noise/bytemuck are normal (non-gated) deps.** They are
  cross-platform and build on host (the determinism test needs wgpu natively);
  only true wasm glue (`web-sys`/`js-sys`/`wasm-bindgen*`/`console_error_panic_hook`)
  is `cfg(target_arch = "wasm32")`-gated.

**Manual/browser TODOs (cannot verify headless):**
1. Serve via `npm run dev`, confirm canvas appears (black), `crossOriginIsolated
   === true`, rAF loop runs, profiler dumps one JSON line/sec, speed presets
   change tick rate, WebGPU adapter/limits logged.
2. Confirm `wasm-pack`-built `pkg/` loads in-browser and `init_manifold` logs the
   region split.

**For phase 2:** BV22 hash is locked & gated — reuse `pipelines::HASH_WGSL`
(prepend to scatter). Buffer naming: SoA fields are `pos_x/pos_y/pos_z`, `v`,
`i_current`, `last_spike` (all 4 B/elem, single chunk at ≤16M). `GpuResources`
owns buffers + `bind_groups_dirty`; allocate in `resize_neurons`/`refresh_bind_groups`
(phase-1 builds layouts only — `wgpu::Buffer`s are NOT yet allocated). Shaders
embed via `include_str!` from `src/sim/gpu/shaders/`. `RenderState::Empty` is a
phase-1 addition for the stub state. `target()`'s per-call cell lookup uses an
O(n) scan on host (`SpatialGrid::cell_of_index`); phase 2 must store per-neuron
cell ids in a GPU buffer instead.
