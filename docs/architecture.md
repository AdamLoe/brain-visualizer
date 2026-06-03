# Brain Visualizer — Architecture

_Technical design for the spiking-neural-network visualization. Self-contained so
it can be reviewed independently. Last updated: 2026-06-03._
_See `decisions.md` (BV1–BV8) for the locked choices this expands on. Site-level
integration decisions (homepage layout, hosting) live in the repo-root docs._

## 0. What this is
An interactive 3D spiking neural network rendered as a glowing, firing patch of
cortex. Point/LIF neurons placed on a cortical manifold, locally connected via a
procedural rule, simulated in real time, hardware-scaled to push the visitor's
machine near a frame budget. Not biophysically detailed (no ion channels /
multi-compartment morphology) — that's the part Fugaku-scale sims spend
supercomputers on and that you can't see anyway. We keep the *neuron count and
the look*, drop the invisible fidelity.

Reference points: Allen Institute / Fugaku mouse-cortex sim (9M biophysical
neurons, 26B synapses, 145,728 nodes, ~32s per 1s real-time — visualization
shows only ~1% of neurons, flashing on spike). Allen's own GLIF *point-neuron*
version of V1 (~230k neurons) shows point neurons are a legitimate, cheap
simplification.

## 1. Two backends (the comparison) — BV4
One `SimBackend` interface, two implementations, switchable at runtime:

| | **GPU backend** | **CPU backend** |
|---|---|---|
| API | WebGPU via `wgpu` | WebGL2 (`web-sys`) for render only |
| Sim location | GPU compute shaders | WASM on Web Worker pool |
| Execution model | **clock-driven, data-parallel** (update every neuron each tick) | **event-driven** (process only neurons that fire) |
| Parallelism | GPU SIMT (thousands of threads) | `rayon` over active lists; SIMD128 after profiling |
| Scatter | atomic add (fixed-point i32) into current buffer | fixed-point atomic add into current buffer |
| State→render | stays GPU-resident, zero readback | sim writes SharedArrayBuffer; uploaded to GPU each frame |
| Ceiling (target) | practical default ~1M; stretch ~10M only on high-end discrete GPUs | ~100k–500k neurons |

The render layer shares the LOD scheme but uses WebGPU vs WebGL2 pipelines
respectively. The point of building both is to **measure** CPU vs GPU on
identical networks.

## 2. Data model (Structure-of-Arrays, GPU-resident)
SoA, not array-of-structs — for SIMD on CPU and coalesced access on GPU.

```
positions : f32 x3   (x[], y[], z[]) — static after placement
v          : f32     membrane potential
I          : i32     accumulated input current (fixed-point; see §5 atomics)
last_spike : u32     bit 31 = HAS_SPIKED; bits [30:24] = 7-bit type
                     (E/I flag + cortical region); bits [23:0] = tick.
                     24-bit tick → 2^24 ticks ≈ 4.6 h at 1 ms/tick.
                     Packing type here eliminates the dedicated type array and
                     its alignment padding.
```
Per-neuron = **24 B** → **1M ≈ 24 MB, 10M ≈ 240 MB** (GPU buffers, not WASM heap).
25% better cache density vs the naïve 32 B layout.

Positions are true SoA (`pos_x`, `pos_y`, `pos_z`) in GPU storage buffers. Do
not use `array<vec3<f32>>` for positions unless the memory budget is explicitly
changed to a padded 16-byte stride. Render shaders reconstruct
`vec3(pos_x[i], pos_y[i], pos_z[i])`.

Shared helpers used by all shaders/backends:
```
HAS_SPIKED_MASK = 0x8000_0000
TYPE_MASK       = 0x7F00_0000
TICK_MASK       = 0x00FF_FFFF
type(packed)    = (packed >> 24) & 0x7F
has_spiked(p)   = (p & HAS_SPIKED_MASK) != 0
tick_diff(a,b)  = (a - b) & TICK_MASK
```
Render glow is zero when `has_spiked(last_spike[i]) == false`, so silent startup
does not look like a fresh spike.

## 3. Connectivity — procedural / implicit (BV6)
No global edge list. Targets derived from a deterministic rule:
1. Neurons placed on/in the cortical manifold; space partitioned by a uniform
   grid (spatial hash).
2. Each neuron connects to ~K neighbors sampled from nearby cells with
   **distance-decay probability** (Gaussian/exponential) → local cortex.
   Optional sparse long-range "highway" edges = future (hybrid).
3. Determinism via a **WGSL-friendly 32-bit integer hash** (BV22) seeded by
   neuron id + synapse index — stateless, parallel-safe, no `Math.random`.
   `target(i, j)` and `weight(i, j)` are pure functions of ids + type.
4. To make the natural startup read as posterior→anterior instead of merely
   isotropic glow around input regions, excitatory local targets include a mild
   feed-forward bias along the anterior axis. Inhibition remains local and mostly
   unbiased. This is still procedural local cortex, not stored long-range
   connectome data.

**K is a per-tier knob (BV18):** K ≈ 16–32 (low), 32–64 (balanced), 64–128
(max). The adaptive scaler may compress/expand K within a tier alongside N.
A single global K=64 is not used.

**Store-once vs regenerate (per-tier knob):** geometry is static, so we may
generate neighbor lists once into a CSR-like buffer (K=64 → 1M×64×4B = 256 MB;
10M would be 2.5 GB → infeasible) OR regenerate targets per-tick by hashing (zero
storage, more compute). Low/mid tiers store; max tier regenerates.

**Edge values on zoom:** weights are recomputable exactly from the rule; we
lazily accumulate per-edge *activity* (spike count since first observed) only for
edges materialized in the zoomed-in view. Never touches the sim hot path.

## 4. Neuron dynamics (LIF, biologically faithful at the functional level) — BV5
Per tick (dt ≈ 1 ms biological):
```
v[i] = v[i] * leak_decay + I[i]          // leaky integration
I[i] = 0                                  // reset accumulator (or double-buffer)
if v[i] >= threshold and tick_diff(tick, last_spike_tick[i]) > refractory:
    emit spike(i)
    v[i] = reset_potential
    last_spike[i] = tick
```
- **E/I balance:** ~80% excitatory (positive weights) / ~20% inhibitory
  (negative) — required for stable avalanche/traveling-wave dynamics.
- **Conduction delay:** spikes take time ∝ axon length. Two options:
  - *Sim-accurate:* delay ring buffer `I_buf[(tick+delay) % D][t]` (D=32 →
    1M×128 MB, 10M×1.3 GB → heavy; cap D or only on lower tiers).
  - *Visual-only (pragmatic default):* sim scatters instantly; the *renderer*
    animates spikes traveling along edges over a few frames. Decouples cost from
    correctness. Decide per tier.
- A firing neuron **broadcasts** to all targets weighted per-synapse; it does not
  divide a conserved charge. Competition is downstream at integration.

## 5. Core per-tick mechanics by backend

### GPU (clock-driven, WebGPU compute)
Each tick = a few `dispatch`es, all state in storage buffers:
1. **integrate+threshold** — 1 thread/neuron, fully parallel; appends fired ids
   to a spike list via an atomic counter.
2. **scatter** — production target is 1 thread per `(spike × synapse)` event;
   for each target do a fixed-point atomic accumulation into `I_next[t]`.
   A one-thread-per-spike loop over K is allowed only as a debug/prototype path
   because it underuses the GPU at high K.
   - **WGSL has no f32 atomics** → accumulate current in **fixed-point i32**
     (scale factor S, convert on read). S = 2^12 is the locked scale factor.
     Because local clustering can create high in-degree during synchronized
     firing, production code must either enforce/measure a fan-in/current bound
     per tier or use saturating accumulation via atomic compare-exchange.
   - **Workgroup-local accumulation (future optimization):** if profiling shows
     global atomic contention is the scatter bottleneck, threads can accumulate
     into `workgroup` LDS first and flush one atomic per bucket to global
     memory. Measure first — local connectivity already reduces contention.
3. **(optional) delay** — write into the delay ring slot instead of `I_next`.
4. swap buffers; render reads `v`/`last_spike` directly (no CPU readback).
Sync between passes via dispatch barriers; contention handled by atomics.

### GPU frame graph and resource lifecycle
The production GPU backend should be built around a small, explicit frame graph,
not around UI events calling ad-hoc GPU work. A normal frame is:

1. apply pending config changes (rare path: tier resize, backend restart, shader
   feature toggle);
2. update small uniform buffers only if their rounded value changed (camera,
   tick/dt, excitability, render options);
3. encode simulation passes (`integrate`, indirect scatter, optional stimulation
   / delay);
4. encode optional instrumentation/debug passes only if enabled;
5. encode render passes (far LOD, optional near LOD, optional HDR/bloom);
6. resolve timestamp queries asynchronously if enabled;
7. submit once.

All large buffers are persistent across frames. Recreate them only on structural
changes: tier resize, backend restart, device-loss recovery, render resolution
change for render targets, or a changed buffer shape. Any buffer recreation that
feeds a bind group must immediately refresh the dependent bind groups. Treat this
as an explicit dependency graph in code (`GpuResources` / `GpuPipelines`) rather
than letting setup logic spread through controls, render, and sim modules.

Avoid CPU readbacks in the rAF loop. Readbacks are allowed for benchmark output,
asynchronous timestamp staging, one-off debug snapshots, and backend
determinism checks. Scatter dispatch size must be GPU-driven via an indirect
dispatch buffer written from `spike_count`; the CPU must never map `spike_count`
to decide the next pass.

### Spatial structures and scan primitives
The same integer spatial grid should serve three jobs: procedural local
connectivity, cursor stimulation lookup, and near-LOD culling/materialization.
For fixed geometry, build the grid once at startup or tier resize. For any
dynamic list that must be compacted on the GPU, use the count → prefix scan →
scatter/compact pattern:

1. clear counts;
2. count items per cell/bin with atomics;
3. prefix-scan counts into offsets;
4. scatter items into a compact/sorted buffer using per-bin atomic counters;
5. consume contiguous ranges in later kernels.

This pattern is required for near-LOD visible-neuron/synapse lists and may be
used for materialized neighbor lists on low/mid tiers. Use workgroup-local scans
for large arrays; a naive global iterative prefix sum is acceptable only for a
throwaway benchmark or a deliberately tiny debug path.

### CPU (event-driven, rayon active list)
1. Take this tick's **fired set** (queue/double-buffered Vec).
2. **Parallel scatter:** each fired source is processed in parallel; every target
   current update is a fixed-point `AtomicI32` add. This is simpler and
   race-free in WASM. Spatial partitioning can be profiled later, but is not the
   MVP execution model.
3. **Parallel integrate+threshold** over active/touched neurons. Add SIMD128
   only for contiguous active runs after the scalar path is correct:
   (`f32x4`) across 4 neurons at a time:
   - Compare: `f32x4_ge(v_vec, threshold_vec)` → integer bitmask via
     `i32x4_bitmask()`.
   - If bitmask == 0: skip this block of 4, no state writes.
   - If non-zero: use `trailing_zeros()` to extract fired indices and append
     to next-tick spike queue. Hot loop is branchless for silent blocks.
4. Upload changed `v`/`last_spike` to GPU for WebGL2 rendering (this upload is a
   real, measured cost of the CPU path).

## 6. Rendering & LOD (BV7)
- Projection 3D→2D = MVP matrix in the vertex shader (free; no "engine" needed).
- **Far LOD:** additive-blended instanced billboards/quads; brightness =
  `has_spiked ? exp(-tick_diff(tick, last_spike_tick)/tau) : 0`; color by
  region. Zoomed out → volumetric glow.
  WebGPU does not expose portable programmable point sizes, so do not use
  `@builtin(point_size)` for neuron glow.
- **Near LOD (GPU backend):** frustum culling runs entirely in a **compute
  shader** — iterates spatial hash cells, clips against camera planes, evaluates
  the 32-bit hash rule to materialize connection lines, writes visible instance
  data into an append buffer, then fires `drawIndexedIndirect`. Zero CPU↔GPU
  readback; the CPU never touches per-instance data.
- **Near LOD (CPU backend):** CPU evaluates frustum + PRNG, uploads instance
  buffer, issues instanced draw via WebGL2. Acceptable since the CPU backend
  already pays an upload cost per frame.
- One draw call per primitive class (spheres, cylinders) via instancing.

**Reverse lookup (post-MVP neuron inspect):** finding incoming sources requires
searching which neurons' procedural neighbor lists include the selected neuron.
The search space is bounded by the same hard cutoff radius used in forward
connectivity (Gaussian: cap at 3σ; exponential: cap at ~4λ where probability <
2%). Any neuron outside that radius has effectively zero probability of
connecting; skip them. Reverse lookup is a spatial hash query over the capped
radius only. This is intentionally deferred; the MVP input scheme has no
click/select action.

## 7. Concurrency map (where parallelism lives)
1. **GPU SIMT** (GPU backend): per-neuron threads (integrate) and per-spike
   threads (scatter). Barriers between passes; atomic scatter.
2. **CPU pool** (CPU backend): a dedicated CPU simulation coordinator **Web
   Worker** owns the WASM module instance and initializes the
   `wasm-bindgen-rayon` worker pool. The pool writes fixed-point currents and
   state into **SharedArrayBuffer**; the main thread renders from/upload views of
   that shared state and sends control/config messages to the coordinator
   (BV24).
3. **Sim/render decoupling:** fixed-timestep sim accumulator; render interpolates.
   - GPU backend: main thread owns the device, runs compute+render in the rAF
     loop; few CPU threads needed.
   - CPU backend: coordinator worker + rayon pool run sim into SharedArrayBuffer;
     main thread renders + handles input. This worker/main split is the key
     concurrency boundary.
4. **Cross-origin isolation:** SharedArrayBuffer / WASM threads need COOP+COEP.
   When embedded on GitHub Pages (which can't set headers), a
   `coi-serviceworker.js` shim sets them client-side.

## 8. Performance profiling (BV8) — first-class
Emitted every second to the console and mirrored in a small corner HUD;
structured (one JSON line/sec) so it's easy to paste elsewhere:
- **frame:** fps, frame-time ms (rolling avg + p95/p99)
- **sim:** ticks/sec (may differ from fps), spikes/sec, mean firing rate (Hz)
- **throughput:** **synaptic events/sec** (the headline number; spikes×K),
  neurons simulated
- **GPU timing:** per-pass ms via WebGPU **timestamp queries** (integrate /
  scatter / render) — gate on `timestamp-query` feature availability
- **CPU timing:** per-stage `performance.now()`; per-thread utilization
- **memory:** GPU buffer bytes, WASM heap bytes
- **state:** active backend, active tier, current N (from the scaler)
Counters must be cheap in the hot loop (derive synapse events as spikes×K rather
than instrumenting the inner scatter).

Timestamp queries and GPU debug overlays are opt-in. They are invaluable while
tuning, but they add allocations, resolve/copy work, and possible driver
pipeline pressure. Resolve timestamps into a small staging-buffer pool and map
results asynchronously; if all staging buffers are in flight, skip that frame's
timing read rather than stalling.

## 9. Adaptive scaling & difficulty tiers (BV1, BV3)
- On load: feature-detect WebGPU; read `navigator.hardwareConcurrency`; run a
  short benchmark burst to measure real throughput on this machine.
- Maintain a **target frame budget** (e.g. ~10–14 ms for 60 fps with headroom) by
  adjusting N (neurons), K (out-degree), and render resolution via simple
  feedback (grow when under budget, shrink when over) up to the tier cap.
- **Three presets built + benchmarked** (low / balanced / max). Manual switch
  now; auto-pick-per-device heuristic deferred. The scaler operates **inside**
  the selected tier and does not silently jump Low/Balanced/Max.
- **Max tier defaults to ~1M neurons.** 10M is a best-case stretch only when
  device limits (`maxStorageBufferBindingSize`, total buffer budget) AND the
  benchmark burst both support it. The adaptive scaler cap for Max tier is
  1M as the practical default; 10M only unlocks when the device reports a
  large `maxStorageBufferBindingSize` AND benchmark latency sustains the
  frame budget.
- **"10M is a best-case discrete GPU target, not a promise."** The visualizer
  should be impressive because it adapts honestly to the machine, not because
  the copy promises a number most browsers/devices cannot sustain (BV23).
  UI label: "Max (up to 1M — up to 10M on high-end discrete GPU)".
- Scaling decisions should include hysteresis and a cooldown so resize/realloc
  work does not thrash when the workload hovers near the frame budget. Shrink
  quickly when over budget; grow only after sustained headroom.

### 9.1 Benchmark results

**Machine:** WSL2 (Linux 5.15 on x86-64), 20 cores, 31 GB RAM.
**Date:** 2026-06-03.
**Rust:** 1.95.0, wgpu 29.0.3, rayon 1.12.

#### GPU adapter
No real GPU adapter found. `/dev/dri/renderD128` and `/dev/dri/card0` both
returned `Permission denied` under WSL2 (no Vulkan ICD installed or no DRI
passthrough configured). wgpu fell back to `llvmpipe` (LLVM software
rasteriser via Vulkan). All "GPU" numbers below are CPU software emulation
and are **not** representative of real GPU performance — they are thrown away.

**llvmpipe limits (software fallback):**
- `maxStorageBufferBindingSize` = 134 217 728 (128 MiB)
- `maxBufferSize` = 2 147 483 647 (2 GiB)
- `maxComputeWorkgroupsPerDimension` = 65 535
- `maxComputeInvocationsPerWorkgroup` = 1 024
- `maxComputeWorkgroupSizeX` = 1 024
- `timestamp_query` = true (llvmpipe exposes it)

#### GPU benchmark results (llvmpipe — software, NOT real GPU)
Synaptic events estimated at 5% biological firing rate; no per-tick readback.
Scatter uses 2D dispatch to stay within `maxComputeWorkgroupsPerDimension=65535`.

| N       | K  | ticks | time   | ticks/s | syn_events/s (est) |
|---------|----|-------|--------|---------|--------------------|
| 100 000 | 32 | 1000  | 1.53 s | 652     | 104 M              |
| 500 000 | 32 | 500   | 2.35 s | 212     | 170 M              |
| 1 000 000 | 32 | 200 | 1.67 s | 120     | 192 M              |
| 5 000 000 | 32 | 100 | 4.33 s | 23      | 185 M              |
| 500 000 | 64 | 500   | 29.9 s | 17      | 27 M               |

These numbers are pure CPU software emulation. A real mid-range discrete GPU
running the same shaders should be 10–100× faster at large N (see real-GPU
targets in §9).

#### CPU benchmark results (rayon, 20 threads, real hardware)
Active-list event-driven. Fixed-point AtomicI32 scatter. Drive injected to
every 20th neuron to maintain ~5% firing rate. Measured synaptic events are
real (actual fired×K, not estimated).

| N       | K  | ticks | time   | ticks/s | syn_events/s | avg fired/tick |
|---------|----|-------|--------|---------|--------------|----------------|
| 10 000  | 32 | 2000  | 1.58 s | 1 263   | 0.63 M       | 15             |
| 50 000  | 32 | 1000  | 2.45 s | 407     | 1.02 M       | 77             |
| 100 000 | 32 | 500   | 1.13 s | 442     | 2.23 M       | 157            |
| 500 000 | 32 | 200   | 0.52 s | 388     | 10.4 M       | 835            |
| 50 000  | 64 | 1000  | 2.56 s | 390     | 2.18 M       | 87             |

**CPU observations:**
- Throughput at N=500k K=32 ≈ 388 ticks/sec with ~835 firings/tick (5% rate).
  That maps to ~10.4 M synaptic scatter events/sec on 20 cores.
- Performance dips at smaller N (e.g. N=100k: 442 ticks/s) vs N=10k (1263
  ticks/s) because rayon thread-pool startup and atomic contention overhead
  becomes visible at lower absolute scatter load.
- The CPU event-driven path scales well: at 5% firing rate it only processes
  the 5% that fired, making it efficient at the low/mid tiers.

#### Browser benchmark
**NOT collected — no browser in build environment (WSL2 headless).**
The WebGPU/WASM microbench at `bench/web/` compiles via `wasm-pack` but was
not run. Browser numbers are the real reference for shipped tier caps.
**This is a manual TODO before tier caps are locked.**

#### Tier cap assessment (preliminary, pending browser numbers)
Based on CPU numbers and WSL2 GPU fallback only:

- **Low tier (CPU backend):** 50k–100k neurons, K=32 → ≈400 ticks/sec.
  At 60 fps (1 tick/frame): comfortable. At 1000 ticks/sec biological: 2.5×
  slower than target on 20 cores; real device with 4–8 cores will be ~3–5×
  slower again → realistic CPU low tier cap is ~10k–20k neurons at 60 fps.
- **Balanced tier (GPU backend):** Requires real browser WebGPU numbers.
  Architecture target ~200k neurons remains plausible but unconfirmed.
- **Max tier (GPU backend):** Architecture target ~1M neurons. Cannot confirm
  without real discrete GPU adapter. 10M stretch path is **rejected** until
  browser GPU numbers support it.
- **Action:** User must run the browser microbench (`bench/web/index.html`
  served with COOP/COEP headers) on the target deployment machine and paste
  results here before finalising tier caps in Phase 1.

_Native numbers are upper bounds only. Shipped caps must be set from browser
WebGPU/WASM results as specified in phase-0-benchmark.md._

#### Phase 7 final perf audit — balanced-tier native numbers
**Machine:** WSL2 (Linux 5.15 on x86-64), 20 cores, 31 GB RAM.
**Device:** llvmpipe (LLVM 20.1.2, software Vulkan) — CPU emulation, NOT real GPU.
**Date:** 2026-06-03.

Balanced tier: N=200k, K=32, focused (excit=0.55), i_ext=0.040, synaptic_scale=0.03.
From Phase 2 GPU bench table: ~212 ticks/s, ~170M syn-events/s (llvmpipe).
From cpu_check.rs Phase 6: CPU 12.42 Hz vs GPU 12.42 Hz at focused (N=30k).

**These are software-emulation numbers. Real GPU numbers are a manual TODO
(browser microbench on target hardware).**

**Per-item perf audit (Phase 7 checklist):**
1. No CPU readbacks in normal rAF frames — PASS. JS rAF loop: no readbacks.
   GPU tick() path: one 8-B stats staging read per batch (after submit, not
   per-tick); no dispatch size from CPU readback (GPU indirect buffer drives scatter).
2. No per-frame buffer/bind-group/pipeline/texture creation — PASS.
   All large buffers persistent; bind groups rebuilt only when `bind_groups_dirty`
   set. Render targets recreate only on size change (`resize_render_targets`
   guard).
3. Timestamp query resolve async, skipped when staging busy — PARTIAL.
   Timestamp writes are `None` in all passes (deferred OD10). The near-LOD stats
   readback (24 B) blocks synchronously (OD15 — documented, acceptable at 24 B).
   A full async staging-pool path is a follow-up.
4. Debug overlays off by default — PASS. Corner HUD `debugEnabled=false`;
   no hidden passes. Near-LOD disabled by default when far from surface.
5. Render targets recreate only on size/format change — PASS. `resize_render_targets`
   guards on `t.width != width || t.height != height`.
6. Backend restart/tier resize rebuilds cleanly with same seed — PASS.
   `initialize()` re-runs manifold + resize_neurons + refresh_bind_groups + tick=0.
   Verified by cpu_check.rs (CPU 12.42 Hz == GPU 12.42 Hz on same seed).
7. Profiler derives inner-loop totals cheaply — PASS. `synaptic_events = spikes * k`
   (no scatter instrumentation). Per-region rates approximated from anatomical
   fractions (30/40/30) in `deriveRegionFractions()` — no per-neuron scan.

## 10. Known constraints & risks
- **WebGPU f32 atomics absent** → fixed-point i32 accumulation (§5).
- **`maxStorageBufferBindingSize` defaults to 128 MiB** in WebGPU; large buffers
  need higher requested limits (device-dependent, up to ~GBs on desktop) or
  chunking across bindings. Affects multi-M-neuron buffers.
- **Dispatch dimensions and storage binding caps vary by adapter.** Query
  limits at startup, choose scan workgroup sizes from adapter limits, split
  oversized dispatches, and clamp bin/instance counts to both hardware limits
  and measured performance caps.
- **WebGL2 has no compute / no atomics** → CPU does all sim on that path (that's
  by design; it's the CPU backend).
- **Delay ring buffer memory** can dominate at high N → visual-only delay as the
  default (§4).
- **COOP/COEP** when embedded on GitHub Pages only via service-worker shim;
  first-load registration timing must be handled.
- **wgpu WASM bundle size** + first-load compile of large shaders — measure.
- Throttling: respect `prefers-reduced-motion`; pause sim when tab hidden.

## 10.1 Implementation guardrails
- Keep sim, render, controls, profiler, and GPU resource lifecycle in separate
  modules. Do not let the main component/page become the owner of all state.
- No per-frame allocation in hot paths: no rebuilding large `Vec`s, JS arrays,
  string-keyed maps, bind groups, pipelines, or render targets unless a
  structural change requires it.
- Keep debug visualizations and HUD/timing code behind flags. Debug passes may
  consume production buffers, but production passes must not depend on debug
  buffers.
- Avoid string keys for spatial cells in any performance path. Use packed
  integer cell ids or linearized `u32` indices.
- Resize/tier/backend changes are rare-path operations. It is acceptable for
  them to allocate and briefly pause; it is not acceptable for normal frames to
  inherit that cost.

## 11. Region topology & ambient drive (BV17)
The folded surface is divided into three region classes (stored as the `type`
byte on each neuron):
- **Input regions** (posterior — sensory cortex analog): receive a per-tick
  `I_ext` constant added to their accumulator before the integrate step.
  This is the sole source of external energy entering the system; it simulates
  thalamic relay drive. Magnitude tuned so input neurons approach threshold
  slowly from silence (~hundreds of ms on biological time).
- **Association regions** (central / prefrontal): no drive; pure relay.
- **Output regions** (anterior — motor cortex analog): no special treatment.
  Activity arrives here by topology and dissipates through the global E/I
  balance (inhibitory interneurons + LIF leak).

Region membership is assigned at placement time from surface position. The
same `type` byte already used for E/I and color (§2) encodes region class
in its upper bits — no extra per-neuron storage.

## 12. Backend switch (BV16)
Switching the active `SimBackend` via the top-right toggle (BV12) performs a
**full teardown + restart** with the same network seed. No mid-run state
transfer. Teardown sequence: halt sim loop → release GPU buffers or terminate
workers → reinitialize backend → restart from silent state (§11 ambient drive
ramps back up naturally).

## 13. Interaction, dynamics target & sound (BV9–BV12)
- **Self-organized criticality (BV9):** tune E/I balance + gain + connectivity so
  the network operates near the critical point → neuronal avalanches (power-law
  cascade sizes). An **excitability** control scales effective gain (e.g. global
  weight multiplier or threshold offset) to sweep silent → critical → seizure.
  The avalanche-size distribution is derived from the existing per-tick spike
  count stream (BV8) — accumulate cascade sizes, bin to a log-log histogram off
  the hot path. This becomes a correctness/quality signal, not just eye candy.
- **Cursor stimulation (BV10):** unproject the pointer to the manifold, find
  neurons within a radius via the §3 spatial hash, add a current bump to their
  `I`. Cheap; same scatter path the sim already uses.
- **Camera (BV10):** orbit (click-drag) + zoom (scroll/pinch) are just MVP-matrix
  updates (§6) — no readback, no sim coupling. Proposed input scheme:
  hover=stimulate, left-drag=orbit, scroll=zoom. Click/select is deferred.
- **Neuron inspection:** post-MVP. If revived, pick the neuron (GPU id-buffer pick
  or CPU ray test), materialize outgoing targets from the procedural rule, and
  find incoming sources via bounded reverse lookup.
- **Sound (BV11):** Web Audio, muted by default. Drive a small voice bank from
  per-region spike rates (already in the profiler) rather than per-spike events —
  keeps it off the hot path.
- **Backend toggle (BV12):** runtime swap of the active `SimBackend`; same seed →
  same network. Side-by-side dual render deferred.
- **Speed controls (BV14):** top-left selector: ¼×, ½×, 1×, 2×. Implemented as
  a `sim_speed` multiplier controlling ticks-per-frame; renderer runs at native
  rAF rate regardless. Slow modes (¼×/½×) make individual wavefronts visible.
- **Named brain states (BV15):** preset excitability values labeled Deep sleep /
  Relaxed / Focused / Hyperstimulated / Seizure. Tied to the BV9 excitability
  control; no new sim parameters.
- **Natural start (BV10 amendment):** sim begins silent; no scripted intro.
  Input-region neurons receive ambient drive, activity propagates inward then
  outward to output regions. Network topology does the work.

## 14. Open technical questions
- Sim-accurate conduction delay on any tier, or visual-only everywhere?
- Store-once connectivity threshold (exact N where we switch to regenerate)?
- ~~Worker-based sim for the CPU backend on main thread vs dedicated worker~~ —
  **resolved (BV24):** dedicated CPU sim coordinator worker + rayon pool; main
  thread renders and handles input.
- ~~Cortical manifold source~~ — **resolved (BV13):** procedurally folded
  brain surface. Approach: start from a subdivided sphere, apply layered
  noise to produce gyri/sulci folds, place neurons on/near the surface via
  projection. Exact subdivision resolution and noise parameters TBD; neuron
  count is secondary to brain-shape fidelity.
- Gyrification noise approach: Perlin/simplex noise at multiple frequencies
  to produce realistic ridge/valley ratio, or a physically motivated
  buckling simulation (simpler to tune visually: iterative displacement).
- ~~Speed control implementation~~ — **resolved (BV14):** a global `sim_speed`
  multiplier scaling the
  number of ticks executed per rAF frame (e.g. ¼× = 1 tick/4 frames,
  2× = 2 ticks/frame). The renderer interpolates at its own rate regardless.
