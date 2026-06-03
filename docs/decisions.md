# Brain Visualizer — Decisions

_Locked decisions for the spiking-neural-network visualizer. Do not silently
overwrite — add a dated amendment if something changes. Open items live in
`current_plan.md`; engineering detail in `architecture.md`._

_Context: this project currently lives inside the `adamloe.com` repo and serves
as that site's homepage centerpiece, but it is designed to be extracted into its
own standalone project. These decisions cover the visualizer only; site-level
decisions (tone, hosting, content) live in the repo-root `decisions.md`._

## BV1 — Scope & approach: from-scratch, point-neuron, peak-optimized
- **Date:** 2026-06-02
- **Decision:** A genuinely compute-heavy, hardware-adaptive spiking-NN
  simulation that auto-scales problem size to the visitor's CPU cores, per-core
  throughput, GPU capability, and on-screen pixel count — pushing the machine
  near a target frame budget *without* pointless busy-work. Built **from
  scratch**: our own shaders, kernels, data layout, and connectivity algorithm;
  **no graphics engine** (no three.js/Babylon). `wgpu` is used only as a thin
  GPU-API binding, not an engine. **Point/LIF neurons, NOT biophysical** —
  multi-compartment ion-channel detail is the expensive part real sims spend
  supercomputers on and it isn't visible; we keep neuron count + the look, drop
  the invisible fidelity.
- **Why:** Adam wants the site itself to be proof of systems/perf skill, and a
  real hard problem (not inflated math). See `architecture.md` §0.

## BV2 — Engine: spiking neural network (SNN)
- **Date:** 2026-06-02
- **Decision:** Integrate-and-fire neurons with spikes cascading across
  synapses. Other engine ideas (N-body layout, particle field, live
  forward-pass/training) are deferred (`possible_future_work.md`).
- **Why:** Most "alive"-looking, embarrassingly parallel, scales cleanly with
  neuron/synapse count.

## BV3 — Three difficulty tiers, all built and benchmarked
- **Date:** 2026-06-02
- **Decision:** Ship three presets (low / balanced / max); build + perf-test all
  three. Manual switch now; per-device auto-selection heuristic deferred.
- **Why:** Adam wants to compare all three before committing to selection logic.

## BV4 — Two interchangeable backends: GPU (WebGPU) and CPU (WebGL2)
- **Date:** 2026-06-02
- **Decision:** One `SimBackend` interface, two implementations compared head to
  head on the same network: **GPU** = WebGPU compute via `wgpu`, clock-driven /
  data-parallel, atomic scatter; **CPU** = event-driven active-list simulation
  with `rayon` over Web Workers, WebGL2 for render only. WASM SIMD128 is an
  optimization target after the scalar active-list path is correct.
- **Why:** Adam wants a direct CPU-vs-GPU measurement; the two natural execution
  models map onto CPU vs GPU, so it doubles as a systems-design showcase.

## BV5 — Neuron model: LIF first, GLIF later
- **Date:** 2026-06-02
- **Decision:** Start with leaky integrate-and-fire; leave room for Allen-style
  GLIF adaptation terms as an upgrade.
- **Why:** LIF is the cheapest faithful model and reproduces the look.

## BV6 — Connectivity: procedural / implicit (no stored edge list)
- **Date:** 2026-06-02
- **Decision:** Synapse targets + weights from a deterministic spatial-hash rule
  (distance-decay, biologically local). The hash primitive is the 32-bit
  WGSL-friendly hash locked in BV22, not `u64` PCG. No global edge list.
  Cache-once vs regenerate-per-tick is a per-tier knob (store viable to ~1–2M
  neurons, regenerate beyond). Weight is deterministic/recomputable; per-edge
  *activity* is lazily accumulated only for zoomed-in edges.
- **Why:** Storing billions of edges is impossible in-browser; procedural wiring
  is correct for local cortex AND turns a memory wall into compute.

## BV7 — Rendering: LOD point-glow (far) → spheres + cylinders (near)
- **Date:** 2026-06-02
- **Decision:** Far = additive point-sprite glow (recency-based brightness,
  color by region). Near = instanced low-poly spheres (neurons) + impostor
  cylinders (synapses), materialized only for the frustum. Placeholder geometry
  now; richer neuron geometry much later.
- **Why:** Matches real brain-viz look; keeps synapse rendering view-bounded so
  it never caps global neuron scale.

## BV8 — First-class performance profiling
- **Date:** 2026-06-02
- **Decision:** Per-second structured console dump (HUD later): FPS/frame-time
  (avg + p95/p99), sim tick rate, **synaptic events/sec**, spikes/sec + mean
  firing rate, per-pass GPU timing (timestamp queries) / per-stage CPU timing,
  GPU + WASM memory.
- **Why:** Adam wants extreme profiling; also core to the CPU-vs-GPU comparison
  and the adaptive scaler. See `architecture.md` §8.
- **Amendment 2026-06-02:** ship a **small corner-of-screen HUD** alongside the
  console dump from the start (not "console-first, HUD later"). Compact, tucked
  in a corner — not a full dashboard.

## BV9 — Dynamics target: self-organized criticality (neuronal avalanches)
- **Date:** 2026-06-02
- **Decision:** Don't just make it "flash randomly" — tune E/I balance, gain, and
  connectivity so the network sits near the **critical point**, producing
  **neuronal avalanches** (spike cascades of all sizes, power-law distributed).
  A single **excitability slider** sweeps silent → critical (the beautiful
  regime) → runaway synchronized firing (epileptiform). Live avalanche-size
  distribution is available to view (ties into BV8 profiling).
- **Why:** It's the difference between random prettiness and a system that is
  visibly brain-like — and it's a free interactive/dynamics win on top of the
  sim we're already building.

## BV10 — Interaction model: a pretty toy with slight interactivity
- **Date:** 2026-06-02
- **~~SUPERSEDED in part — see amendments below~~**
- **Decision:** Framing is a **silly, pretty toy** — slight interactivity, NOT a
  benchmark/score product. Interactions:
  - **Cursor stimulation:** moving the cursor over the cortex injects current
    into nearby neurons → ripples propagate out. (Touch on mobile.)
  - **Camera:** click-and-drag to **orbit/rotate**, scroll/pinch to **zoom**.
  - ~~**Neuron inspection**~~ — **DEFERRED** (see amendment 2026-06-03 below).
  - ~~**"Wake up" intro:** cortex loads dark/silent; a seed spike fires; activity
    spreads to fill the sheet (~2s).~~ — **REPLACED** by natural propagation
    (see amendment 2026-06-03 below). Do not implement the scripted intro.
- **Input scheme:** hover (no button) = stimulate, left-drag = orbit,
  scroll/pinch = zoom. Click has no MVP behavior; selection returns only if
  neuron inspection is revived post-MVP.
- **Why:** Adam's choice; keeps it fun and inviting without turning into a
  competitive tool. See denied: machine/benchmark score.

## BV10 — Amendment 2026-06-03: "wake up" is natural propagation, not scripted
- **Decision:** The simulation starts from a silent state. Input-region neurons
  receive ambient `I_ext` drive (BV17); activity propagates naturally
  posterior→anterior. No scripted seed spike, no special intro code.
  The natural ramp-up is the intro.
- **Why:** Free, more honest, more interesting to watch repeatedly.

## BV10 — Amendment 2026-06-03: neuron inspection deferred
- **Decision:** Click-to-inspect (incoming/outgoing connections, close-up
  firing view) is deferred to post-MVP. Input scheme has no click/select action
  in the MVP.
- **Why:** Reverse lookup + near-LOD materialization is non-trivial; the
  visual experience does not depend on it.

## BV11 — Sonification: spikes → sound (opt-in, muted by default)
- **Date:** 2026-06-02
- **Decision:** Sonify activity — region as pitch, firing rate as texture; a
  critical-state brain "sounds like rain building to storms." **Muted by
  default**, visitor opts in. Web Audio; cheap.
- **Why:** Adam wants to try it; memorable, low cost, fits the toy framing.

## BV12 — Backend comparison UX: top-right toggle now, side-by-side later
- **Date:** 2026-06-02
- **Decision:** Expose the GPU-vs-CPU backend choice (BV4) as a **toggle in the
  top-right corner** for now. The full **side-by-side "race"** (both backends on
  the same seed, throughput counters racing) is a cool target but **deferred** to
  `possible_future_work.md`.
- **Why:** Adam wants the comparison available but doesn't want to build the
  dual-render side-by-side as a first step.

## BV13 — Cortical manifold: procedurally folded brain surface (gyri/sulci)
- **Date:** 2026-06-03
- **Decision:** The neuron placement surface must look like a recognizable brain
  from the outside — procedurally generated gyri (ridges) and sulci (folds).
  **Neuron count may be reduced to preserve the brain-like visual** — fidelity
  to brain shape takes priority over maximizing N. No external mesh assets;
  generate entirely from code (subdivide + gyrification noise, neurons settle on
  surface).
- **Why:** A flat patch or smooth blob does not read as a brain. The visual
  recognizability is the point.

## BV14 — Simulation speed controls: top-left multi-option
- **Date:** 2026-06-03
- **Decision:** Expose a small top-left speed selector with a few discrete
  options (e.g. ¼×, ½×, 1×, 2×). Slow modes drop the sim to a handful of
  ticks/sec so individual spike wavefronts are visible at human-perceptible
  speed. Fast mode compresses time. Normal is default.
- **Why:** Slow-motion reveals what the simulation is actually doing; a few
  presets are simpler than a scrub slider and cover the useful range.

## BV15 — Named brain-state excitability presets
- **Date:** 2026-06-03
- **Decision:** Label discrete points on the excitability axis as recognizable
  brain states (e.g. Deep sleep → Relaxed → Focused → Hyperstimulated →
  Seizure). These are just preset values for the existing excitability control
  (BV9); no new sim code. Labels keep the toy framing intuitive.
- **Why:** Named states give visitors an immediate frame of reference without
  requiring any neuroscience background. Zero implementation cost on top of the
  slider already planned.

## BV16 — Backend switch: full restart (same seed)
- **Date:** 2026-06-03
- **Decision:** Swapping the active `SimBackend` (BV12 toggle) tears down all
  sim and render state and restarts from scratch using the same network seed.
  No mid-run state transfer between backends.
- **Why:** State transfer between fundamentally different memory layouts (GPU
  buffers vs WASM SharedArrayBuffer) is complex and error-prone for zero user
  benefit. A restart is instant and deterministic. The same seed guarantees the
  visitor sees the same network, which is the meaningful comparison.

## BV17 — Cortical region topology: input / association / output mapped to anatomy
- **Date:** 2026-06-03
- **Decision:** The folded surface (BV13) is divided into three region classes
  following rough human cortical anatomy:
  - **Input regions** (posterior surface — sensory cortex analog: occipital,
    temporal, parietal): neurons in these patches receive a small constant
    ambient drive `I_ext` simulating thalamic relay input. This is the only
    external energy entering the system.
  - **Association regions** (central / prefrontal / parietal association):
    no special drive; integrate and relay activity.
  - **Output regions** (anterior surface — motor cortex analog): no special
    treatment; activity flows here naturally by topology and dissipates through
    the E/I balance (inhibitory neurons + refractory periods) everywhere.
  Energy is not "removed" at a special sink — dissipation is handled globally
  by inhibitory interneurons (~20% of neurons) and the LIF leak term (BV5).
  The input `I_ext` is what creates the natural posterior→anterior flow from a
  silent start (BV10 amendment). To make that flow visually reliable, the
  procedural connectivity may include a mild anterior/feed-forward bias for
  excitatory local targets; inhibition stays local and mostly unbiased.
- **Why:** Matches the biological reality (thalamo-cortical drive → sensory →
  association → motor) with zero added complexity beyond a region label per
  neuron and a constant drive term. Makes the natural propagation direction
  visually legible.

## BV18 — Per-tier synaptic out-degree K combined with N
- **Date:** 2026-06-03
- **Decision:** K (synaptic out-degree per neuron) is a **per-tier knob**
  combined with N, not a single global constant. Example ranges:
  - Low tier: small N, K ≈ 16–32
  - Balanced tier: medium N, K ≈ 32–64
  - Max tier: large N, K ≈ 64–128
  The adaptive scaler (BV1) may further adjust K within a tier's range based
  on measured throughput — both N and K are valid axes for the scaler to
  compress or expand. No separate "connectivity level" control exposed to the
  user; it is subsumed into the existing tier/preset (BV3).
- **Why:** K × N drives the synapse-event cost as much as N alone. Varying K
  per tier gives finer control over computational load and lets lower-end
  devices run sparser but still valid networks rather than just shrinking N.

## BV19 — Fixed-point current scale factor and overflow policy: S = 2^12
- **Date:** 2026-06-03
- **Decision:** The i32 fixed-point accumulator for synaptic current (GPU
  backend, WGSL atomics) uses scale factor **S = 4096 (2^12)**. This scale is
  safe for individual synaptic contributions, but **out-degree is not an
  overflow proof**: a target can receive contributions from many sources in one
  tick, especially under local clustering or synchronized firing.

  Production code must therefore use one of these enforced policies:
  1. a measured/proven per-tick fan-in/current bound for every tier, with a
     debug high-water counter and warning threshold; or
  2. saturating fixed-point accumulation implemented with an atomic
     compare-exchange loop.

  The phase 2 MVP may start with plain `atomicAdd` only if it also logs max
  absolute current and treats overflow detection as a blocker before tier caps
  are locked. If biological weights, K, connectivity locality, or excitability
  are revised upward, recheck the bound.
- **Why:** Prevents silent overflow-to-negative (which would cause
  hyperpolarization instead of depolarization — a correctness bug invisible
  from the rendering side).

## BV20 — CPU scatter: fixed-point atomics first
- **Date:** 2026-06-03
- **Decision:** CPU scatter uses the same fixed-point current representation as
  the GPU path and applies every synaptic contribution with an atomic integer
  add. Region partitioning and border-only atomics are deferred optimizations,
  not the MVP implementation model. Per-thread partial current buffers with
  full reduction remain rejected: allocation + zeroing per tick is too
  expensive.
- **Why:** A simple all-target atomic path is race-free, deterministic, and much
  easier to compare against the GPU backend. If profiling later shows atomics
  dominate CPU time, spatial partitioning can be introduced with measured
  benefit rather than designed in prematurely.

## BV21 — Data layout: valid bit + type packed into `last_spike`
- **Date:** 2026-06-03
- **Decision:** `last_spike` is a packed `u32`:
  - bit 31 = `HAS_SPIKED`
  - bits [30:24] = 7-bit neuron type (E/I flag + cortical region)
  - bits [23:0] = tick of last fire

  New neurons start with `HAS_SPIKED = 0`, type bits initialized, and tick bits
  zero. Render shaders must treat `HAS_SPIKED = 0` as zero spike glow, preserving
  the silent-start requirement. Refractory and glow math must use a shared
  24-bit modular tick-difference helper: `(now - then) & 0x00FF_FFFF`.

  Tick counter occupies 24 bits (max 2^24 ≈ 16.7M ticks ≈ 4.6 h at 1 ms/tick),
  and modular differences remain correct as long as no compared interval exceeds
  half the wrap range. Eliminates the dedicated type array and its alignment
  padding.
  Per-neuron footprint: **24 B** (down from ~32 B). 1M neurons ≈ 24 MB.
- **Why:** 25% better cache density in the hot integration loop on both CPU
  (L2/L3) and GPU (L1/L2) while still representing "never spiked" correctly.
  No new storage needed; extraction is a mask+shift in the shader.

## BV22 — Hash primitive: WGSL-friendly 32-bit hash with golden vectors
- **Date:** 2026-06-03
- **Decision:** Procedural connectivity uses a pure `u32` hash implemented
  identically in Rust and WGSL. Do not use `u64` PCG in shaders. Locked hash:
  ```
  x ^= x >> 16;
  x *= 0x7feb352d;
  x ^= x >> 15;
  x *= 0x846ca68b;
  x ^= x >> 16;
  ```
  All multiplies wrap modulo 2^32. Inputs are mixed from `(seed_lo, neuron_id,
  synapse_index, salt)` using `wrapping_add`, xor, and distinct odd constants.
  Phase 1 must include golden-vector tests for representative `(i, j, salt)`
  values and the WGSL/Rust outputs must match exactly before GPU sim work
  proceeds.
- **Why:** WGSL has no native `u64`. A 32-bit hash avoids manual carry
  arithmetic, keeps shader code small, and removes a major determinism risk
  between CPU and GPU backends.

## BV23 — Max tier promise: 1M default, 10M gated stretch
- **Date:** 2026-06-03
- **Decision:** Treat ~1M neurons as the practical Max-tier default. 10M neurons
  remains a best-case discrete-GPU stretch only when device limits
  (`maxStorageBufferBindingSize`, total buffer budget) and the benchmark burst
  both support it. UI/docs must not imply 10M is generally available.
- **Why:** The visualizer should be impressive because it adapts honestly to the
  machine, not because the copy promises a number most browsers/devices cannot
  sustain.

## BV24 — CPU topology: dedicated coordinator worker + rayon pool
- **Date:** 2026-06-03
- **Decision:** The CPU backend runs simulation off the main thread. A dedicated
  CPU simulation coordinator Web Worker owns the WASM module instance, initializes
  the `wasm-bindgen-rayon` worker pool, advances the event-driven active-list
  loop, and writes SoA state into SharedArrayBuffer. The main thread handles
  input, WebGL2 rendering, UI, and profiler display, and communicates control
  changes to the coordinator through structured messages plus shared config
  fields where useful. Do not run CPU simulation work on the main thread except
  for tiny startup/self-test code.
- **Why:** This keeps the CPU-vs-GPU comparison honest without freezing orbit,
  controls, WebGL uploads, or the HUD. It also gives the CPU backend one clear
  ownership boundary: worker-side sim state, main-thread render state, and
  SharedArrayBuffer as the bridge.
