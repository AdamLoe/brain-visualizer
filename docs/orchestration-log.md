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
| 2 GPU sim | complete | — | Real LIF on GPU (llvmpipe verified); indirect scatter, no per-tick readback; 64 tests pass; WGSL target==Rust target gate PASS; rates deep_sleep 0Hz / focused 12.4Hz / seizure 33Hz; browser/real-GPU 100k confirmation = manual TODO |
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

### Phase 2 — GPU Simulation Core (2026-06-03)

**Built:** Real GPU LIF simulation on WebGPU/wgpu compute.
- **Shaders** (`src/sim/gpu/shaders/`): `integrate.wgsl` (wg 256; ambient i_ext
  to input-region neurons via region bits, excitability gain 0.5+excit*1.5,
  leak, threshold+5-tick refractory, atomic spike append, BV21 last_spike pack),
  `scatter.wgsl` (wg 64; production `target_neuron` reimplements the Phase-1
  spatial rule in WGSL — cell-offset + anterior bias + nearest-occupied spiral +
  in-cell pick over the CSR grid — NOT modulo-N; `target_neuron_debug` is the
  labelled modulo-N fallback, never dispatched; fixed-point atomicAdd + BV19
  high-water `atomicMax`), `write_scatter_dispatch.wgsl` (wg 1; computes
  `ceil(spike_count*K/64)` into a DispatchIndirect buffer on the GPU). All three
  reuse the locked BV22 `HASH_WGSL` by prepend; the hash is never re-authored.
- **Dispatch (per tick, one encoder, GPU-driven):** writeBuffer tick uniform +
  clear spike_count → integrate → write_scatter_dispatch → scatter via
  `dispatch_workgroups_indirect(dispatch_args)` → flip I/I_next by bind-group
  parity (two prebuilt integrate/scatter bind groups; no realloc, no per-frame
  allocation).
- **Resources/lifecycle:** `GpuResources::resize_neurons` allocates real
  `ChunkedBuffer` device buffers (single chunk ≤16M; multi-chunk path compiles),
  uploads positions from the manifold, packs `last_spike` (HAS_SPIKED=0 silent
  start + region/E-I type bits), v=0, I=0, and the static CSR grid buffers
  (`cell_of_neuron` via new O(N) `SpatialGrid::cell_of_neuron_map`, `cell_start`,
  `cell_neurons`). `refresh_bind_groups` honors `bind_groups_dirty`. E/I:
  `hash32(id ^ seed_lo) % 5 == 0` → inhibitory (verified ~20%).
- **Backend:** `GpuBackend` owns a `GpuContext{device,queue}`. Device acquisition
  factored: `acquire_native()` (cfg(not wasm); high-perf adapter → llvmpipe
  fallback; lifts `max_storage_buffers_per_shader_stage`/binding-size from
  adapter — downlevel default of 4 is too few for the 5-/8-binding passes) vs
  the wasm path which constructs the same platform-agnostic `GpuBackend::new`
  from a browser-acquired context. **wasm build remains green.**
- **Stats / no-readback:** the per-tick path NEVER reads spike_count to size the
  scatter dispatch — the GPU-written indirect buffer does. Stats are staged ONCE
  per `tick()` batch (8 B: final spike_count + high-water max|I|) and mapped
  after submit; never inside the loop. `debug_dynamics_snapshot()` (off the hot
  path) reads v/last_spike and asserts mean(v)∈[-0.5,1.5] + warns >80% fire.
- **Verification harness:** `examples/sim_check.rs` (`cargo run --release
  --example sim_check`) drives the real backend on the native device.

**Verified (this environment — llvmpipe software Vulkan):**
- `cargo build` host — clean, 0 warnings.
- `cargo build --target wasm32-unknown-unknown` — clean. `wasm-pack build
  --target web` — clean.
- `cargo test` — **64 pass** (61 unit + 3 integration). New integration tests:
  `tests/gpu_sim_dynamics.rs` (real backend excitability sweep + overflow/NaN)
  and `tests/wgsl_target_determinism.rs` — **WGSL `target_neuron` == Rust
  `connectivity::target()` for all 128,000 (i,j) pairs on a real manifold**
  (the GPU scatter wires the identical network to the CPU path). BV22 hash gate
  still PASS.
- **Dynamics (N=30k, K=32, llvmpipe, 600-tick measure after 200 warm-up):**
  | preset | excit | mean rate | spikes/s | syn-events/s |
  |--------|-------|-----------|----------|--------------|
  | deep_sleep | 0.10 | 0.00 Hz (silent) | 0 | 0 |
  | focused | 0.55 | 12.4 Hz (plausible) | 372,560 | 11.9 M |
  | seizure | 1.00 | 33.3 Hz (elevated) | 999,617 | 32.0 M |
  Monotone gradient silent→moderate→high. debug snapshot at focused: mean_v=0.19
  (in range), 1.24% fired/tick (well under 80%).
- **Overflow (BV19):** seizure 2000 ticks, max|accumulated current| (fixed-point)
  = **2,674,316** vs i32_max 2,147,483,647 → **803× headroom. Verdict: SAFE**
  with plain atomicAdd at these tier params; saturating compare-exchange NOT
  needed yet. No NaN membrane potentials (full v scan).

**Tuning (documented; NO locked BV value changed):**
- The Phase-1 locked synaptic weights are strong vs threshold=1.0 (≈one input is
  suprathreshold), which makes the raw network **bistable** — silent below an
  i_ext cliff (~0.037) and refractory-capped at ~166 Hz above it, with no graded
  band. Two runtime knobs open a biological 5–20 Hz regime without touching
  weights, fixed_point_scale (stays 4096), or any BV constant:
  - `i_ext = 0.040` (just above the input-region firing cliff so input neurons
    seed activity from silence; config default 0.06 over-saturates). Spec default
    0.02 leaves the network silent because leak 0.95 gives sub-threshold
    equilibrium.
  - **`synaptic_scale = 0.03`** — a NEW integration-side knob (uniform field +
    `GpuBackend::set_synaptic_scale`) that scales accumulated recurrent current.
    It sets how many coincident presynaptic spikes are needed to fire
    (biological realism) and is the lever that produces the graded sweep. Default
    is 1.0 (neutral). This is an integration-side scaling, explicitly NOT a
    change to the locked weight rule (GPU still matches Rust `weight()`/`target()`
    bit-for-bit). Phase 3+/scaler should expose it per-tier or fold it into the
    excitability mapping.
- **Tier caps:** the Low 50k/16, Balanced 200k/32, Max 1M/64 table is NOT
  contradicted by these numbers and stays as-is (caps still pending real browser
  GPU numbers per Phase 0). N=30k used here only because llvmpipe is slow.

**For Phase 3 (rendering) — must know:**
- `render_state()` returns `RenderState::Gpu { v_buf, last_spike_buf, pos_x_buf,
  pos_y_buf, pos_z_buf, neuron_count }` — real `&wgpu::Buffer` handles, all
  STORAGE | COPY_SRC, GPU-resident, zero readback. They are
  `neuron_buffers.<field>.chunks[0]` (single chunk for N≤16M).
- Render shader reads: `v[i]` (f32 membrane), `last_spike[i]` (u32: bit31
  HAS_SPIKED, bits[30:24] type = (region<<2)|EI with region Input=0/Assoc=1/
  Output=2 and EI bit0=1→inhibitory, bits[23:0] tick). Reconstruct position as
  `vec3(pos_x[i], pos_y[i], pos_z[i])` (true SoA, 4-B stride — never vec3 array).
  Glow must be 0 when `has_spiked==false` (silent start). Recency =
  `tick_diff(tick, last_spike&0xFFFFFF)` with the shared 24-bit modular helper.
- **Tick counter** lives on the CPU side: `GpuBackend::tick_count()` (u32,
  24-bit-wrapping semantics in shaders). The render pass needs the current tick
  to compute glow recency — pass it in a render uniform each frame.
- The sim bind-group layouts are independent of render; Phase 3 builds its own
  layouts/bind groups in `GpuResources` (extend `RenderTargets` /
  `refresh_bind_groups`), reusing the same buffers as read-only storage.
- Device/queue: `GpuBackend::device()` / `queue()` accessors exist for render
  setup. The backend owns one device+queue; the render pass shares them.

**Manual/browser TODOs (cannot verify headless):**
1. Run the harness on a real GPU / in-browser at N=100k–1M to confirm the
   `done-when` "non-zero spikes_per_sec at N=100k" and tier throughput; llvmpipe
   numbers are software emulation (slow), used only for correctness.
2. Re-tune `i_ext` / `synaptic_scale` on real hardware if the avalanche/critical
   regime (BV9) needs sharpening; seizure here is elevated (33 Hz) but not a full
   synchronized burst — the 20% inhibition + refractory keep it stable. Push
   synaptic_scale up (≈0.12→100 Hz, 0.2→159 Hz observed) for a harder seizure.
3. Timestamp queries: llvmpipe reports TIMESTAMP_QUERY=true and the feature is
   requested when present, but per-pass timestamp wiring is deferred to the
   render frame-graph (Phase 3); current timing is wall-clock `tick_ms`.
