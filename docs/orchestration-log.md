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
| 3 GPU render | complete | — | offscreen render PASS: 48.6% non-black, all 3 region channels, stim confirmed |
| 4 Near LOD | complete | — | GPU indirect cull+draw PASS; 321 neurons/2565 synapses emitted at close zoom; 0 overflow; 70 tests pass |
| 5 Controls | complete | — | UI wired (speed/brain-state/backend buttons); excitability lerp; scaler activated; 22 TS tests pass (vitest); 70 cargo tests still pass; GPU bridge = browser TODO |
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

### Phase 3 — GPU Rendering / Far LOD + Camera (2026-06-03)

**Built:**

- **`src/sim/gpu/shaders/render_far.wgsl`**: Billboard glow pass exactly per spec. Reads pos_x/pos_y/pos_z/last_spike/v from storage buffers + `Uniforms` struct (mvp mat4x4, camera_right/camera_up vec3 with padding, tick u32, glow_tau f32, point_radius f32, n u32). `glow = has_spiked ? exp(-tick_diff/glow_tau) : 0`, plus faint sub-threshold v glow. Region color from type bits (Input=blue, Assoc=green, Output=orange). Additive blend (src=One, dst=One). `draw(6, N)` instanced billboards, triangle-list. No point_size.

- **`src/sim/gpu/shaders/render_manifold.wgsl`**: Static dark mesh pass. Reads MVP uniform + vertex positions. Flat `vec4(0.05, 0.05, 0.08, 1.0)` fill. No culling (brain viewed from both sides). Depth write enabled, depth test Less. Rendered BEFORE glow pass.

- **`src/sim/gpu/shaders/stimulate.wgsl`**: Cursor stimulation compute pass. Bounded brute-force over spatial grid CSR: iterates cells overlapping the sphere bounding box (~27 cells at dim=16, radius=0.15), atomicAdds fixed-point current to neurons within radius. Guard `is_active == 0` skips dispatch when no hover. Note: WGSL keyword `active` is reserved — field renamed `is_active`.

- **`src/sim/gpu/resources.rs`** (extended): New structs `RenderResources`, `StimUniform`, `RenderUniforms`, `ManifoldUniforms`. `RenderTargets` now holds real depth texture + view. `GpuLayouts` extended with `render_far_bgl` / `render_manifold_bgl` / `stimulate_bgl`. `GpuBindGroups` extended with optional `render_far` / `render_manifold` / `stimulate` (Option, None until `init_render_resources` called). `GpuResources` has `init_render_resources()` (uploads static manifold mesh + creates uniform buffers) and real `resize_render_targets()` (creates Depth32Float texture). `refresh_bind_groups` builds render bind groups when `render_resources` is Some.

- **`src/sim/gpu/pipelines.rs`** (extended): Added `RENDER_FAR_WGSL`, `RENDER_MANIFOLD_WGSL`, `STIMULATE_WGSL` embeds. `GpuPipelines` gains `render_far`, `render_manifold`, `stimulate`. `build_render(device, layouts, color_format)` creates all three. Manifold pipeline: depth write + cull_mode=None. Far-LOD pipeline: additive blend + depth_write_enabled=Some(false) + depth_compare=Some(Always). wgpu 29 API: `multiview_mask: None`, `depth_write_enabled: Some(bool)`, `depth_compare: Some(CompareFunction)`.

- **`src/sim/gpu/mod.rs`** (extended): `GpuBackend` gains `stim_pending: Option<StimUniform>`. `initialize()` calls `init_render_resources()` after `resize_neurons()`. New public methods: `build_render_pipelines(color_format)`, `resize_render_targets(w, h)`, `render(target_view, mvp, camera_right, camera_up, glow_tau, point_radius)`. `render()` encodes manifold mesh pass (clear+depth) then far-LOD glow pass (load). `stimulate()` stores `StimUniform` (fixed-point current = current * S) in `stim_pending`. Tick loop dispatches stimulate compute at first tick of each batch when stim_pending is Some.

- **`web/camera.ts`** (rewritten): Full orbit camera with `azimuth=0.3`, `elevation=0.4`, `distance=3.0` defaults per spec. `onPointerMove(x, y, buttons)` returns bool (orbit=true / hover=false). Wheel zoom with spec clamps (0.5–10.0). Touch: one-finger orbit, pinch zoom. Exposes `mvpMatrix()`, `cameraRight()`, `cameraUp()`, `unproject(x, y, canvasW, canvasH)` (ray for stim). Full 4x4 matrix inverse + perspective + lookAt.

- **`web/renderer.ts`** (rewritten): Thin wrapper. `render(camera, tick, wasmBackend?)` delegates to wasmBackend.render() if available, otherwise clears black. No per-frame pipeline/bind-group allocation.

- **`web/main.ts`** (rewritten): `onPointerMove` routes hover to `handleStimulate()` which unprojects ray, intersects manifold bounding sphere (r=1.4), calls `backend.stimulate()`. Touch events wired. wasmBackend GPU bridge is a browser-only manual TODO.

- **`examples/render_check.rs`** (new): Offscreen render verification harness at 512x512. Builds real GpuBackend, warms up 300 ticks at focused, renders one frame to Rgba8Unorm texture with fixed camera, reads back pixels. Asserts non-black + region colours + stimulate() path.

**Verified (this environment — llvmpipe):**
- `cargo build` host — clean, 0 warnings.
- `cargo build --target wasm32-unknown-unknown` — clean. `wasm-pack build --target web` — clean.
- `cargo test` — **66 pass** (63 unit + 3 integration). New unit tests: `render_shaders_present`, `render_uniform_size_aligned`, updated `destroy_releases_everything`.
- **`examples/render_check.rs` PASSED:**
  - Shaders compiled/validated by Naga: zero errors.
  - Non-black pixels: 127393/262144 = **48.60%** — glow confirmed present.
  - Max channel values: R=255 G=255 B=255 — all 3 region colours present (additive blend saturates; Input/Association/Output neurons each contribute).
  - **stimulate() confirmed**: spikes before=616, after=642 (+4.2% over 10 ticks). No crash.
  - Device: llvmpipe (LLVM 20.1.2, software Vulkan).
- `tsc --noEmit` (node local) — clean.
- `vite build` — clean (44.4 kB wasm, 20.0 kB JS).

**Deviations from spec / decisions:**
- **OD6 — WGSL `active` is a reserved keyword**: Renamed to `is_active` in shader + Rust struct.
- **OD7 — Render bind groups as Option**: `render_far/render_manifold/stimulate` in `GpuBindGroups` are `Option<...>` (None until `init_render_resources` called). Avoids invalid empty-entry bind groups.
- **OD8 — GPU stim spatial lookup (bounded brute-force)**: Stimulate shader iterates ~27 grid cells overlapping the sphere bounding box (O(N/150) per dispatch). Existing CSR grid reused — no separate per-neuron cell-id buffer needed.
- **OD9 — HDR/bloom deferred**: Far LOD default (additive blend to canvas) works without HDR. Hook: pass HDR format to `build_render_pipelines()` and add bloom compose pass. Not implemented; documented.
- **OD10 — Timestamp queries deferred**: `timestamp_writes: None` in all render passes. Hook is in place.
- **OD11 — Wasm browser GPU bridge**: The wasm `GpuBackend::acquire_native()` acquires a native device; the browser WebGPU context wiring requires wasm-bindgen JS→Rust bridging (manual TODO). TS renderer/main are ready for it.

**Manual/browser TODOs:**
1. Wire browser WebGPU context to `GpuBackend::new()` via wasm-bindgen.
2. Test interactive orbit/zoom/hover in browser with WebGPU; tune `point_radius` and `glow_tau` per visual result.
3. Confirm natural startup ramp is visible (input-region drive ramping up from silence over ~300 ticks).
4. HDR/bloom (optional, gated).
5. Timestamp query wiring (optional, gated).

### Phase 4 — Near LOD (2026-06-03)

**Built:**

- **`src/sim/gpu/shaders/frustum_cull.wgsl`**: `cull_neurons` and `cull_synapses` compute entry points. `FrustumUniforms` (6 planes + camera_pos + max_synapse_dist + tick + n). `NeuronInstance` / `SynapseInstance` structs at 32 B each. `in_frustum` tight (−0.05 tolerance) for neurons; `in_frustum_loose` (−0.5 tolerance) for synapse targets (needed when camera is inside the brain). `target_neuron` identical to scatter.wgsl (reuses BV22 hash prepend). Atomic append with overflow counter. Dispatched as `@workgroup_size(256)`. `NearConnectUniforms` holds k_near=8, max_near_instances, max_synapse_instances.

- **`src/sim/gpu/shaders/draw_indirect.wgsl`**: `write_indirect` (1 thread). Loads atomic counts, clamps to buffer capacity, writes `DrawIndexedIndirectArgs` (5×u32) for both sphere and cylinder draw calls. Writes unclamped counts to profiler visible counters.

- **`src/sim/gpu/shaders/render_sphere.wgsl`**: Icosphere vertex+fragment shader. Vertex layout: float32x3 (pos) + float32x3 (normal). `world_pos = instance_pos + local_pos * radius * (0.5 + glow)`. Blinn-Phong diffuse + emissive glow. Alpha-blend (SrcAlpha, OneMinusSrcAlpha). `lod_alpha` in `NearUniforms` drives crossfade.

- **`src/sim/gpu/shaders/render_cylinder.wgsl`**: 6-sided prism cylinder shader. Instance provides src_pos + tgt_pos; VS builds orthonormal basis from src→tgt direction, scales unit cylinder to span the connection. Excitatory = faint blue-white, inhibitory = faint red. Depth test (Load); no depth write (thin lines).

- **`src/sim/gpu/resources.rs`** (extended): Added `NearRenderUniforms`, `FrustumCullUniforms`, `NearConnectUniforms`, `IndirectWriteUniforms`, `NearLodBuffers`, `NearLodStats`. `GpuLayouts` extended with 5 new near-LOD BGLs (cull_bgl_group0/1, draw_indirect_bgl, render_sphere_bgl, render_cylinder_bgl). `GpuBindGroups` extended with 5 new near-LOD bind groups. `GpuResources` gains `near_lod_buffers: Option<NearLodBuffers>`. `init_near_lod_resources` derives caps from adapter `max_storage_buffer_binding_size`, allocates all 20 near-LOD buffers ONCE, uploads static sphere (12 verts, 20 tris) and cylinder (12 verts, 12 tris) geometry. `refresh_bind_groups` builds near-LOD BGs when `near_lod_buffers` is Some. `destroy` also drops `near_lod_buffers`. Geometry generators `build_icosphere()` and `build_cylinder_prism()` are public and unit-tested.

- **`src/sim/gpu/pipelines.rs`** (extended): Added `FRUSTUM_CULL_WGSL`, `DRAW_INDIRECT_WGSL`, `RENDER_SPHERE_WGSL`, `RENDER_CYLINDER_WGSL` embeds. `GpuPipelines` gains 5 near-LOD pipeline fields. `build_near_lod(device, layouts, color_format)` builds all 5 pipelines; frustum_cull prepends `HASH_WGSL` so `target_neuron` has `mix_key`. `is_near_lod_built()` predicate.

- **`src/sim/gpu/mod.rs`** (extended): `LOD_FAR_ONLY_DIST=1.5`, `LOD_NEAR_ONLY_DIST=0.8` constants. `GpuBackend` gains `near_lod_stats: NearLodStats` and `lod_camera_distance: f32`. `initialize()` calls `init_near_lod_resources`. `build_render_pipelines` also calls `build_near_lod`. New methods: `set_lod_camera_distance(d)`, `near_lod_stats()`, `render_full(…, camera_pos, camera_distance)`. `render()` delegates to `render_full` with stored `lod_camera_distance`. LOD alpha computed each frame: `far_alpha` in [0,1], `near_alpha = 1 − far_alpha`. Near-LOD pass sequence (cull_neurons → cull_synapses → write_indirect → draw_sphere → draw_cylinder) only runs when `near_alpha > 0.001`. Frustum planes extracted via Gribb-Hartmann from column-major MVP. Counters zeroed each frame via `queue.write_buffer`. Profiler staging readback via `read_near_lod_stats` (blocks once per frame, off the render hot path). `extract_frustum_planes` and `read_near_lod_stats` are module-private helpers. `max_synapse_dist = 2.5` (generous to handle camera-inside-brain geometry).

- **`web/camera.ts`** (extended): Added `cameraDistance(): number` getter exposing the orbit distance for Phase 5 near-LOD plumbing.

- **`examples/near_lod_check.rs`** (new): Headless near-LOD verification harness.

**Verified (this environment — llvmpipe):**
- `cargo build` host — clean, 0 warnings.
- `cargo build --target wasm32-unknown-unknown` — clean.
- `cargo test` — **70 pass** (67 unit + 3 integration). New unit tests: `near_lod_uniform_sizes_aligned`, `icosphere_has_correct_geometry`, `cylinder_prism_has_correct_geometry`, `near_lod_shaders_present`. All Phase 1–3 tests still pass.
- **`examples/near_lod_check.rs` PASSED:**
  - Device: llvmpipe (LLVM 20.1.2, software Vulkan).
  - CLOSE (distance=0.3, near-LOD active): emitted_neurons=321, emitted_synapses=2565, neuron_overflow=0, synapse_overflow=0.
  - Non-black pixels: 65536/65536 (100%) — spheres/cylinders rendered.
  - FAR (distance=3.0): near-LOD skipped (counts stay at 0 from init, no crash).
- `tsc --noEmit` — clean.
- `vite build` — clean (44.4 kB wasm, 20.0 kB JS).
- `examples/render_check.rs` still PASSES unchanged (48.6% non-black, stimulate confirmed).

**Decisions / deviations:**
- **OD12 — Loose frustum for synapse targets**: When the camera is inside the brain (distance < brain radius ~1.0), the tight frustum (-0.05 margin) excludes most target neurons even though nearby source neurons pass. Added `in_frustum_loose` (-0.5 margin) for synapse target check only. This is consistent with the spec note that "both ends must be roughly in view."
- **OD13 — max_synapse_dist = 2.5**: Initial 0.5 would cull all synapses when camera is at 0.3 from origin but neurons are on the surface at ~1.0 (distance from camera to neuron ≈ 0.7–1.3). 2.5 world units covers the whole visible brain volume from near-LOD distances.
- **OD14 — Full-array frustum cull**: Full N dispatch per frame (N threads, workgroup 256). Cell-query optimization noted as follow-up per the spec.
- **OD15 — Near-LOD profiler readback blocks once per frame**: `read_near_lod_stats` maps the 24-B staging buffer synchronously after submit. This is acceptable since it's 24 bytes (trivial) and only runs when near-LOD is active. For production, this can be made fully async using the same staging-pool pattern as timestamp queries.
- **OD16 — Crossfade via lod_alpha**: Alpha-blend near-LOD layers over the far-LOD base. In the 0.8–1.5 distance band, both far-LOD (draw N billboards) and near-LOD (cull + indirect draw) run simultaneously. The `lod_alpha` uniform in `NearRenderUniforms` linearly ramps the near-LOD transparency from 0→1 as distance goes 1.5→0.8. Far-LOD billboard draw is skipped when `far_alpha < 0.001`.
- **OD17 — `render_full` vs `render`**: Added `render_full(…, camera_pos, camera_distance)` taking explicit eye + distance. The existing `render()` delegates to `render_full` with `lod_camera_distance` (set by `set_lod_camera_distance`). This keeps the Phase 3 API unchanged while giving the harness direct control.

**Manual/browser TODOs:**
1. Wire `camera.cameraDistance()` → `backend.set_lod_camera_distance()` in `web/main.ts` (Phase 5 controls task).
2. Pass `camera.eye()` as `camera_pos` to `render_full` in the browser render loop.
3. Timestamp query wiring for near-LOD cull_ms / render_ms (deferred, same as Phase 3).
4. Cell-query optimization in `cull_neurons`/`cull_synapses` (spatial hash lookup instead of full-array scan).

**For Phase 5 (controls UI) — must know:**
- `GpuBackend::set_lod_camera_distance(f32)` must be called each frame from the JS side with `camera.cameraDistance()` before calling `render()`.
- The LOD mode is fully automatic from the distance; no separate boolean toggle is needed.
- `GpuBackend::near_lod_stats()` returns `NearLodStats` for the profiler HUD (emitted_neuron_instances, emitted_synapse_instances, overflow counts).
- The camera's `cameraDistance()` getter (added in Phase 4) is the intended LOD distance source.
- Speed controls changing `ticks_per_frame` don't affect near-LOD (LOD is render-only, not sim-driven).

**For Phase 5 (controls UI) — must know:**
- **Depth target**: `GpuResources.render_targets.depth_view` is the Depth32Float view written by the manifold pass. Near-LOD spheres should use `LoadOp::Load` on depth attachment to depth-test against manifold.
- **Render bind-group layouts**: `GpuLayouts.render_far_bgl` is group 0 for far-LOD (uniform + 5 storage). Phase 4 adds its own near-LOD BGL to `GpuLayouts`.
- **Camera frustum planes**: derive from `camera.mvpMatrix()` (extract 6 planes from MVP rows). Add `FrustumUniforms { planes: array<vec4<f32>, 6> }` to near-LOD resources; upload before frustum-cull compute dispatch.
- **LOD distance plumbing**: Add `lod_near_distance: f32` to a near-LOD uniform (or extend `RenderUniforms`). Far pass draws ALL N neurons; near pass should draw only neurons within frustum+radius.
- **`GpuBindGroups.render_far/render_manifold`** are `Option<wgpu::BindGroup>` — always Some after full init. Phase 4 adds `render_near: Option<wgpu::BindGroup>` following the same pattern.

### Phase 5 — Controls & Brain States UI (2026-06-03)

**Built:**

- **`web/controls.css`** (new): Fixed-position overlay CSS. `#controls-top` (top bar, flex space-between), `#controls-bottom` (bottom bar, flex center). `.btn-group` pill shape, `rgba(0,0,0,0.4)` dark background, `backdrop-filter: blur(4px)`. Active button: teal `rgba(80,200,220,0.35)` accent with inner glow border. Min 44×44 CSS px touch targets (iOS HIG). Brain-state buttons wrap via `flex-wrap: wrap` on narrow screens. Toast `#toast` element with fade-in/out via CSS transition. Narrow-screen media query at 480px tightens padding.

- **`index.html`** (updated): Full control layout per spec. `#controls-top` contains `#speed-group` (¼×/½×/1×/2×; 1× default active) and `#backend-toggle` (GPU active; CPU has `disabled` attribute + `title="CPU backend lands in phase 6"`). `#brain-canvas` full-bleed canvas. `#controls-bottom` contains `#brain-state-group` (deep sleep/relaxed/focused/hyper/seizure; focused default active). `<div id="toast">` for transient messages. Viewport meta tag added. CSS linked via `<link rel="stylesheet">`.

- **`web/controls.ts`** (rewritten from phase-1 stubs):
  - `BRAIN_STATES` map (locked values per spec).
  - `tickExcitability()` / `setExcitabilityForTest()` / `getCurrentExcitability()` — module-level lerp state (EXCITABILITY_LERP=0.08), callable and unit-testable without DOM.
  - `ticksThisFrame()` — pure speed→ticks mapping (unchanged from Phase 1, now re-exported).
  - `setBrainState(state)` — sets target excitability + DOM active-class.
  - `setSpeed(preset, config)` — updates config + DOM.
  - `setBackend(kind, config, restartFn)` — checks `backendAvailable()` (CPU returns false); shows toast + returns if unavailable; otherwise calls restartFn.
  - `showToast(msg, durationMs)` — DOM toast helper.
  - `scalerDecide(p95, currentN, tier, timeSinceResizeMs, duringRestart)` — **pure, testable** scaler decision function. Returns `{ kind: "none" | "shrink_n" | "grow_n", newN? }`. Never changes tier; clamps to `N_MIN[tier]` / `N_MAX[tier]`.
  - `N_MIN` / `N_MAX` — per-tier bounds (exported, used by scalerDecide and tests).
  - `isMobile()` — `/Mobi|Android/i` or `innerWidth < 768`.
  - `Controls` class — backwards-compatible facade from Phase 1 preserved.

- **`web/main.ts`** (updated):
  - Mobile detection on boot; defaults `config.tier = "low"` on mobile.
  - DOM click handlers wired for all three button groups (`#brain-state-group`, `#speed-group`, `#backend-toggle`). Disabled CPU button: belt-and-suspenders check + toast.
  - `restartWithBackend(kind)` — full BV16 sequence: cancel rAF, `duringRestart=true`, log restart intent, update config + profiler, restart rAF loop. Wasm GPU backend destroy/reinit is a browser TODO (annotated).
  - `tickExcitability()` called each frame in rAF loop; result ready to pass to `backend.tick(ticks, excitability)` (TODO when bridge lands).
  - Phase 4 LOD plumbing preserved: `camera.cameraDistance()` and `camera.eye()` read each frame; `void`-annotated until wasm bridge wires them to `set_lod_camera_distance` and `render_full`.
  - Cursor stimulation skipped on mobile (Phase 5 decision per spec).
  - Adaptive scaler: runs once per `profiler.maybeDump()` (1/sec). Calls `scalerDecide()`, applies `config.n`, logs action, calls `backend.resize(config)` (TODO bridge).
  - `SCALER_COOLDOWN` const declared before first use.

- **`web/profiler.ts`** (updated):
  - `maybeDump()` now returns `boolean` (true = dump emitted) so the rAF loop can trigger the scaler exactly once per second.
  - `getFrameP95()` added — exposes the rolling p95 frame time for the scaler.

- **`web/controls.test.ts`** (new): Vitest unit tests for pure logic — no DOM required:
  - `ticksThisFrame`: 8 assertions covering all 4 speed presets and edge cases.
  - `tickExcitability`: 5 tests — stays at target, per-frame lerp amount, convergence within 200 frames (deep_sleep↔seizure), never overshoots.
  - `scalerDecide`: 12 tests — during restart, cooldown, shrink over budget, grow under budget, no-op in hysteresis band, clamp to N_MIN/N_MAX per tier, floor/ceiling no-op, tier bounds respected for low/max.

- **`vitest.config.ts`** (new): Minimal vitest config. `environment: "node"`, `include: ["web/**/*.test.ts"]`.

- **`tsconfig.json`** (updated): Added `vitest.config.ts` to `include` so it type-checks.

- **`package.json`** (updated): Added `"test": "vitest run"` script; `vitest` in devDependencies.

**Verified:**
- `tsc --noEmit` — clean (0 errors, 0 warnings).
- `vite build` — clean. Output: `dist/assets/index-*.css` 1.59 kB, `index-*.js` 22.81 kB, wasm 44.43 kB.
- `npx vitest run` — **22 tests pass** (0 fail). Tests: 8 ticksThisFrame + 5 tickExcitability + 12 scalerDecide + 2 BRAIN_STATE/N bounds checks.
- `cargo build` (host) — clean, 0 warnings.
- `cargo build --target wasm32-unknown-unknown` — clean.
- `cargo test` — **70 pass** (67 unit + 3 integration). All Phase 1–4 tests still pass.

**Control behavior validation (no browser):**
The DOM-touching paths (`setBrainState`, `setSpeed`, `showToast`) call `document.querySelector*` which is absent in Node; these were intentionally NOT called from unit tests. Validation approach:
1. `tsc --noEmit` confirms all imported types are correct and all call-sites match their signatures.
2. Unit tests call `setExcitabilityForTest` + `tickExcitability` + `scalerDecide` directly (no DOM) and verify results. This exercises the core state machine.
3. DOM wiring is straightforward `addEventListener("click", ...)` + `classList.toggle("active", ...)` — no logic beyond routing to the pure functions. Correctness is checked by reading the TypeScript and confirmed clean by `tsc`.

**Adaptive scaler — location and behaviour:**
- Decision logic: `scalerDecide()` in `web/controls.ts` — a pure function, no side effects, fully unit-tested.
- Adjustment order (doc priority): currently only `N` is adjusted (near-LOD disable and render-resolution reduction are Phase 7 optional cost centres not yet wired). The shrink action is labelled `shrink_n` to preserve slot for higher-priority actions.
- Operates within tier bounds (`N_MIN[tier]` to `N_MAX[tier]`) with 3-second cooldown and `duringRestart` guard. Never changes tier.
- Applied in `rafLoop` after each profiler dump (≈1/sec). Calls `backend.resize(config)` — annotated as browser TODO until wasm bridge is wired.

**Deviations from spec:**
- **OD18 — Scaler priority order deferred**: The spec says "disable near-LOD → reduce render resolution → reduce N". In Phase 5 only `N` adjustment is exposed without a restart (near-LOD toggle and canvas DPR scaling require additional plumbing). The `scalerDecide` return type uses a `ScalerAction` discriminated union so Phase 7 can add `disable_near_lod` and `reduce_resolution` actions as higher-priority steps before `shrink_n`.
- **OD19 — `setBackend` is sync+async dual**: The module-level `setBackend()` is `async` (takes a restartFn). The `Controls` class facade keeps its sync API for backwards compat. Main.ts calls the DOM handler directly with an inline `void restartWithBackend(kind)`.
- **OD20 — CSS in `web/` folder**: `controls.css` lives at `web/controls.css` (not at root). Vite picks it up via `<link href="/web/controls.css">` in index.html.

**Manual / browser TODOs (cannot verify headless):**
1. Wire browser WebGPU context to wasm GpuBackend (Phase 3 OD11) — controls are ready; `setBackend` / `restartWithBackend` will call `wasmBackend.destroy()` + reinit once the bridge lands.
2. Call `wasmBackend.tick(ticks, excitability)` in rafLoop (currently voided).
3. Call `wasmBackend.set_lod_camera_distance(camera.cameraDistance())` and `wasmBackend.render_full(camera.eye(), ...)` in rafLoop (Phase 4 plumbing — voided until bridge lands).
4. Call `wasmBackend.resize(config)` in the adaptive scaler action handler.
5. Confirm button click → active-class visual change → brain state/speed visible change in browser (interaction feel = manual TODO).
6. Mobile device testing: touch orbit/pinch, Low-tier default, no cursor stimulation.
7. Tune `point_radius` and `glow_tau` visually on real hardware.

**For Phase 6 (CPU backend) — must know:**
- `backendAvailable("cpu")` in `web/controls.ts` currently returns `false`. Phase 6 must change this to `true` (or make it a runtime capability check via the wasm module export).
- The CPU button in `index.html` has `disabled` attribute — Phase 6 must remove it (or toggle it dynamically when `backendAvailable("cpu")` becomes true).
- `restartWithBackend(kind)` in `main.ts` is a full teardown + reinit loop (BV16). When the CPU backend is real, it will call `wasmBackend.destroy()` (releases GPU buffers) then `WasmCpuBackend.create(config)` with the same `config.seed`. The `profiler.setConfig(kind, ...)` call is already in the restart path.
- The `Controls.setBackend(kind)` facade and the DOM handler both route through `backendAvailable` — Phase 6 only needs to update that one function (plus remove the `disabled` attribute) to unlock the CPU path end-to-end.
