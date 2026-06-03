# Phase 1 — Foundation (Toolchain + All Scaffolding)

_Nothing renders yet. Goal: every architectural hook is in place as a stub or
real implementation. Later phases fill in stubs; they do not add new layers._

## Done when
- `wasm-pack build` succeeds; Vite serves the page; canvas appears.
- rAF loop runs; tick loop calls `backend.tick()` (stub returns zeros).
- Speed multiplier (¼×/½×/1×/2×) visibly changes tick rate in console.
- Profiling counters print to console once per second.
- Folded brain manifold geometry is generated and logged (neuron positions
  exist as an array even though nothing draws them).
- `SimConfig` can be changed in code; adaptive scaler stub reads it.
- GPU adapter info and limits are logged once at startup when WebGPU is present.
- Resource lifecycle hooks exist: backend `destroy()`, resize path, device-loss
  handler stub, and bind-group refresh after buffer recreation.
- COOP/COEP headers active (SharedArrayBuffer available).

## Directory structure
```
brain-visualizer/
  src/                         # Rust crate (lib, compiled to WASM)
    lib.rs                     # wasm_bindgen entry point
    sim/
      mod.rs
      backend.rs               # SimBackend trait + SimConfig + TickStats
      gpu/
        mod.rs                 # GpuBackend (stub in phase 1)
        resources.rs           # buffers/textures/bind groups lifecycle
        pipelines.rs           # pipeline creation and shader modules
        shaders/               # .wgsl files (empty stubs in phase 1)
      cpu/
        mod.rs                 # CpuBackend (stub in phase 1)
    manifold/
      mod.rs                   # Cortical manifold generation
      icosphere.rs             # Subdivided sphere
      gyrify.rs                # Noise-based folding
      regions.rs               # Input / association / output assignment
    connectivity/
      mod.rs                   # Procedural connectivity
      hash.rs                  # BV22 u32 hash, integer-only
      spatial.rs               # Spatial hash grid over manifold
    buffers.rs                 # SoA layout, chunked buffer abstraction
    gpu_limits.rs              # adapter limits + derived caps
    profiler.rs                # TickStats accumulator + console dump
  web/                         # TypeScript harness
    index.html
    main.ts                    # rAF loop, tick loop, input events
    controls.ts                # UI stubs (speed, brain states, backend toggle)
    camera.ts                  # Orbit + zoom, MVP matrix
    renderer.ts                # WebGPU/WebGL2 renderer stub
  public/
    coi-serviceworker.js       # Cross-origin isolation shim
  Cargo.toml
  package.json
  vite.config.ts
```

## Key types (define in phase 1; all phases use these)

### `src/sim/backend.rs`
```rust
pub trait SimBackend {
    /// Advance the simulation by one or more ticks.
    /// `ticks` is determined by the speed preset; excitability is [0.0, 1.0].
    fn tick(&mut self, ticks: u32, excitability: f32) -> TickStats;

    /// Inject current into neurons within `radius` of `pos` (world space).
    fn stimulate(&mut self, pos: [f32; 3], radius: f32, current: f32);

    /// Return read-only view of current neuron state for rendering.
    fn render_state(&self) -> RenderState;

    /// Resize the network. Triggers reallocation; call only on tier change.
    fn resize(&mut self, config: &SimConfig);

    /// Release owned GPU resources / terminate workers. Required for backend
    /// switch, tier restart, page teardown, and device-loss recovery.
    fn destroy(&mut self);
}

pub struct SimConfig {
    pub n: usize,           // neuron count
    pub k: usize,           // synaptic out-degree
    pub seed: u64,
    pub tier: Tier,
    pub speed: SpeedPreset,
    pub backend: BackendKind,
    pub i_ext: f32,         // ambient drive for input-region neurons
    pub fixed_point_scale: i32, // locked at 4096 (2^12); do not change without overflow check
}

pub struct TickStats {
    pub tick_count: u32,
    pub spikes: u64,
    pub synaptic_events: u64,
    pub tick_ms: f32,       // wall time for all ticks this frame
}

pub enum SpeedPreset { Quarter, Half, Normal, Double }
pub enum BackendKind { Gpu, Cpu }
pub enum Tier { Low, Balanced, Max }

/// Data the renderer reads each frame. GPU backend: raw buffer handles.
/// CPU backend: slice of changed neurons (index + packed value).
pub enum RenderState<'a> {
    Gpu {
        v_buf: &'a wgpu::Buffer,
        last_spike_buf: &'a wgpu::Buffer,   // bit31 valid, bits[30:24] type, bits[23:0] tick
        pos_x_buf: &'a wgpu::Buffer,
        pos_y_buf: &'a wgpu::Buffer,
        pos_z_buf: &'a wgpu::Buffer,
        neuron_count: usize,
    },
    Cpu {
        v_render: &'a [f32],
        last_spike: &'a [u32],
        positions: &'a [[f32; 3]],
    },
}
```

### `src/buffers.rs` — chunked SoA
```rust
/// One logical SoA field split across multiple wgpu::Buffers when
/// N * element_size > MAX_BINDING (128 MiB default).
/// Shaders index via: buffer_index = neuron_id / CHUNK_SIZE,
///                    local_index  = neuron_id % CHUNK_SIZE.
pub struct ChunkedBuffer {
    pub chunks: Vec<wgpu::Buffer>,
    pub chunk_size: usize,   // neurons per chunk
    pub element_bytes: usize,
    pub total: usize,
}
```
Chunk size should be chosen so each chunk ≤ 64 MiB (conservative; fits even
on integrated GPU). At 4 B/element: 64 MiB / 4 = 16M neurons per chunk —
likely a single chunk for most tiers. Positions are three independent 4-byte
fields (`pos_x`, `pos_y`, `pos_z`), not `array<vec3<f32>>`; using `vec3` in a
storage buffer silently changes the effective stride and invalidates the memory
budget. Request higher `maxStorageBufferBindingSize` on init; fall back to
chunked layout if the device limit is lower.

### `src/manifold/` — cortical surface generation
```rust
pub struct Manifold {
    pub vertices: Vec<[f32; 3]>,  // folded surface vertices
    pub faces: Vec<[u32; 3]>,
    pub neuron_positions: Vec<[f32; 3]>,   // N points on surface
    pub neuron_regions: Vec<RegionKind>,    // per-neuron
    pub spatial_grid: SpatialGrid,          // for connectivity + stimulation lookup
}

pub enum RegionKind {
    Input,        // posterior — receives I_ext
    Association,  // central
    Output,       // anterior — no special treatment
}
```

**Generation algorithm (`manifold/icosphere.rs` + `manifold/gyrify.rs`):**
1. Start from icosahedron; subdivide 4–5 times → ~5k–20k surface vertices.
2. Apply two octaves of simplex noise to vertex positions along surface normal:
   - Large scale (frequency ~1.5): gyri (ridges), amplitude ~15% of radius.
   - Small scale (frequency ~4.0): sulci (fine folds), amplitude ~5% of radius.
3. Renormalize vertex positions to lie on the deformed surface.
4. Place N neuron points: sample random barycentric coordinates on random
   faces; project onto surface. Store as `neuron_positions`.
5. Assign regions by dot product of position with a fixed anterior–posterior
   axis: top 30% of dot product → Input, bottom 30% → Output, rest → Association.

**`manifold/regions.rs`:** region assignment only; exports
`assign_regions(positions: &[[f32;3]], axis: [f32;3]) -> Vec<RegionKind>`.

### `src/connectivity/` — integer-only procedural rule
All connectivity must be integer-only so CPU (Rust) and GPU (WGSL) produce
**bit-identical** target lists from the same neuron ID.

```rust
/// Returns target neuron index for synapse j of neuron i.
/// Pure function of (i, j, grid) — no float math.
pub fn target(i: u32, j: u32, grid: &SpatialGrid, k: usize) -> u32 {
    // 1. Look up cell of neuron i in integer grid.
    // 2. BV22 hash of (i, j, seed, salt) → candidate offset within distance D cells.
    //    For excitatory neurons, bias a small fraction of offsets along the
    //    anterior axis so input-region activity visibly propagates forward.
    // 3. Decode offset → target cell → pick neuron from that cell
    //    (hash again to index within cell).
    // 4. If candidate cell is empty or out of bounds, wrap to nearest occupied.
    // No float distance comparison; all arithmetic on integer cell coordinates.
}

pub fn weight(i: u32, j: u32, source_type: u8) -> i32 {
    // Returns fixed-point weight (already scaled by S=4096).
    // Excitatory (type bit 0 = 0): positive, ~1000–4096 (0.25–1.0 mV × 4096).
    // Inhibitory (type bit 0 = 1): negative, ~-2000 to -1000.
    // Deterministic from (i, j, source_type) via BV22 hash.
}
```

**`connectivity/hash.rs`:** BV22 32-bit hash. Input: `u32` mixed from seed,
neuron id, synapse index, and salt. Output: `u32`. Must be identical in Rust
and WGSL.
```rust
pub fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

pub fn mix_key(seed_lo: u32, neuron_id: u32, synapse_j: u32, salt: u32) -> u32 {
    hash32(
        seed_lo
            ^ neuron_id.wrapping_mul(0x9e37_79b1)
            ^ synapse_j.wrapping_mul(0x85eb_ca6b)
            ^ salt.wrapping_mul(0xc2b2_ae35),
    )
}
```
Use the same constants verbatim in WGSL. Phase 1 must include golden-vector
tests for `hash32()` and `mix_key()`; GPU work does not proceed until Rust and
WGSL outputs match.

### `src/profiler.rs`
```rust
pub struct Profiler {
    frame_times: RingBuffer<f32, 120>,  // last 120 frames
    tick_stats: TickStats,
    last_dump: f64,  // performance.now() equivalent
}

impl Profiler {
    pub fn record_frame(&mut self, frame_ms: f32, stats: TickStats) { ... }

    /// Dumps one JSON line to console if ≥1s since last dump.
    pub fn maybe_dump(&mut self) {
        // { fps, frame_ms_avg, frame_ms_p95, ticks_per_sec,
        //   spikes_per_sec, synaptic_events_per_sec, backend, tier, n, k }
    }
}
```

### `web/main.ts` — rAF loop
```typescript
const SPEED_TICKS: Record<SpeedPreset, number> = {
  quarter: 0,   // 1 tick every 4 frames (use frame counter % 4)
  half:    0,   // 1 tick every 2 frames
  normal:  1,
  double:  2,
};

function rafLoop(timestamp: DOMHighResTimeStamp) {
  const ticks = ticksThisFrame(config.speed, frameCounter);
  const stats = backend.tick(ticks, config.excitability);
  profiler.recordFrame(timestamp - lastTimestamp, stats);
  profiler.maybeDump();
  renderer.render(backend.renderState());
  frameCounter++;
  lastTimestamp = timestamp;
  requestAnimationFrame(rafLoop);
}
```

### `web/controls.ts` — all control stubs
Wire these to DOM in phase 1 (even if buttons don't exist yet, the functions
must exist and be callable from console):
```typescript
export function setSpeed(preset: SpeedPreset): void
export function setBrainState(state: BrainState): void  // sets excitability
export function setBackend(kind: BackendKind): void      // triggers restart
export function setTier(tier: Tier): void
```

Brain state → excitability mapping (lock these values):
```typescript
const BRAIN_STATES = {
  deep_sleep:     0.10,
  relaxed:        0.30,
  focused:        0.55,
  hyperstimulated:0.80,
  seizure:        1.00,
};
```

## Toolchain setup

**`Cargo.toml` dependencies:**
```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
wasm-bindgen-rayon = "1"
rayon = "1"
js-sys = "0.3"
web-sys = { version = "0.3", features = ["Window", "Performance", "console", "Worker", "Navigator"] }
wgpu = "29"        # current stable major as of 2026-06-03; recheck at scaffold time
noise = "0.9"      # simplex noise for manifold gyrification
console_error_panic_hook = "0.1"

[dev-dependencies]
# bench binary lives in bench/ crate, not here
```

Use the current stable `wgpu` release compatible with the chosen browser/WASM
target. The `29` major pin was current when these docs were written; if the
scaffold happens later, verify the latest major before creating `Cargo.toml`.
If a lower version is intentionally selected, document the compatibility reason
in `decisions.md` before implementation. CPU backend dependencies are included
in phase 1 because SharedArrayBuffer/thread setup is a build-system concern even
though CPU simulation lands in phase 6.

Threaded WASM build requirements:
- compile with atomics/bulk-memory support as required by `wasm-bindgen-rayon`;
- initialize the rayon worker pool before constructing the CPU backend;
- fail gracefully when cross-origin isolation is unavailable;
- keep COOP/COEP checks in the phase 1 startup log.

## GPU resource lifecycle scaffolding

Phase 1 should create the ownership boundaries even though the real buffers are
mostly empty stubs:

```rust
pub struct GpuResources {
    pub neuron_buffers: Option<NeuronBuffers>,
    pub render_targets: Option<RenderTargets>,
    pub bind_groups_dirty: bool,
}

impl GpuResources {
    pub fn resize_neurons(&mut self, device: &wgpu::Device, config: &SimConfig) {
        // recreate buffers; mark bind groups dirty
    }

    pub fn resize_render_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        // recreate color/depth/HDR targets only when dimensions or format change
    }

    pub fn refresh_bind_groups(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
        // called after any buffer/texture recreation
        self.bind_groups_dirty = false;
    }
}
```

The rAF loop must not recreate pipelines, bind groups, large buffers, or render
targets. Resize/backend/tier changes may allocate; ordinary frames may only
write small uniforms and encode passes.

## Device limits and derived caps

`gpu_limits.rs` should capture adapter limits and convert them into project
caps:

- selected compute workgroup sizes;
- max neuron count by buffer size;
- max near-LOD instance counts;
- max scan/bin count before chunking or clamping;
- whether timestamp queries are available.

Log these once in a structured form. Later phases use these caps instead of
hard-coded desktop assumptions.

## Hot-path hygiene

The foundation should make bad performance patterns hard to introduce:

- spatial cells are integer ids, not strings;
- per-frame profiler/HUD updates allocate nothing and run at most once/second;
- debug flags exist but default off;
- controls call rare-path methods (`resize`, `restart`, `set_render_mode`) rather
  than mutating GPU internals directly.

**`vite.config.ts`:**
```typescript
export default {
  server: {
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
  plugins: [wasm()],  // vite-plugin-wasm
}
```

**`public/coi-serviceworker.js`:** standard coi-serviceworker from
`gzuidhof/coi-serviceworker`. Registered in `index.html` before any other
script. Required for SharedArrayBuffer on GitHub Pages (no header control).

## What is stubbed in phase 1
- `GpuBackend::tick()` → returns zeroed `TickStats`, allocates no buffers.
- `CpuBackend::tick()` → same.
- `renderer.render()` → clears canvas to black.
- Adaptive scaler → reads `SimConfig`, logs proposed N/K, does not resize.
- Speed control UI → JS functions exist, no DOM elements yet.

## What is real in phase 1
- Manifold geometry generated on startup, neuron positions logged.
- Connectivity `target(i, j)` and `weight(i, j)` functions implemented and
  unit-tested (Rust tests).
- `ChunkedBuffer` abstraction implemented.
- `Profiler` implemented and dumping to console.
- `SimConfig` with all fields; `SpeedPreset` enum wired to `ticksThisFrame()`.
- COOP/COEP confirmed active (SharedArrayBuffer check in console).
- BV22 hash tested with golden vectors; Rust and WGSL implementations produce
  identical values for the same input.
