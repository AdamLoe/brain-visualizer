# Phase 6 — CPU Backend

_The CPU backend runs the same LIF simulation in WASM using rayon over an
active-list/event-driven loop, producing a meaningful GPU-vs-CPU comparison on
identical networks. Add SIMD128 after the scalar active-list path is correct and
profiling shows integration is worth vectorizing._

## Done when
- Switching to CPU backend via the top-right toggle restarts and runs correctly.
- Console profiler shows CPU `ticks/sec` and `synaptic_events/sec`.
- Mean firing rate at `focused` excitability matches the GPU backend (±10%).
- CPU backend renders via WebGL2; the brain looks visually identical to GPU.
- At identical seeds, the procedural connectivity is bit-identical on both
  backends (verified by logging first 100 target IDs for neuron 0 on both).
- Lazy decay correctness: a neuron that receives no input for 500ms has a
  membrane potential of `v_init * leak_decay^500 ≈ 0` (verified in test).
- Render decay correctness: untouched silent neurons do not keep stale
  subthreshold voltage glow; `v_render` or shader-side `last_updated` decay is
  verified visually and in a small test.

## Architecture

```
[Dedicated CPU sim coordinator Web Worker]
        |
        v
[rayon worker pool (wasm-bindgen-rayon)]
        |
        | sim runs into SharedArrayBuffer SoA
        v
[SharedArrayBuffer: v[], last_spike[], I[]]
        |
        | main thread uploads deltas to GPU
        v
[WebGL2 render (same visual output as GPU path)]
```

SharedArrayBuffer requires COOP+COEP — already ensured by phase 1.

Ownership boundary (BV24):
- coordinator worker owns CPU sim state, tick scheduling, active lists, and the
  rayon pool;
- main thread owns input, controls, WebGL2 rendering, and profiler/HUD display;
- backend/tier/speed/excitability changes are messages to the coordinator;
- CPU sim work must not run on the main thread except for tiny startup/self-test
  code.

## Rust data structures

### Per-neuron state (CPU SoA in SharedArrayBuffer)
```rust
// Exposed to JS as SharedArrayBuffer slices:
pub struct CpuNeuronBuffers {
    pub v: Vec<f32>,                // membrane potential
    pub last_spike: Vec<u32>,       // bit31 valid, [30:24]=type, [23:0]=tick
    pub I: Vec<AtomicI32>,          // fixed-point current accumulator (S=4096)
    pub last_updated: Vec<u32>,     // tick of last integrate call (for lazy decay)
    pub v_render: Vec<f32>,         // decayed voltage snapshot for WebGL upload
    pub input_neurons: Vec<u32>,    // receives I_ext every tick
}
```

Note: CPU uses the same fixed-point current scale as the GPU path. This makes
parallel scatter a simple `AtomicI32::fetch_add()` and keeps CPU/GPU dynamics
close enough to compare directly. The `last_spike` packing is identical to GPU
for determinism and so the same render shader can read both.

## Lazy decay (correctness fix — required)

Event-driven sims skip silent neurons. Without lazy decay, `v` is stale.

```rust
fn integrate_neuron(
    i: usize,
    current_tick: u32,
    buffers: &mut CpuNeuronBuffers,
    params: &LIFParams,
) -> bool {
    let ticks_dormant = current_tick - buffers.last_updated[i];
    if ticks_dormant > 1 {
        // Apply accumulated leak: v(t) = v(t0) * leak_decay^ticks_dormant
        let decay = params.leak_decay.powi(ticks_dormant as i32);
        buffers.v[i] *= decay;
    }
    buffers.last_updated[i] = current_tick;

    let packed = buffers.last_spike[i];
    let neuron_type = ((packed >> 24) & 0x7F) as u8;
    let last_fire = packed & 0x00FFFFFF;
    let is_input = (neuron_type >> 2) == 0;

    let mut current = buffers.I[i].swap(0, Ordering::AcqRel) as f32
        / params.fixed_point_scale as f32;
    if is_input { current += params.i_ext; }

    let gain = 0.5 + params.excitability * 1.5;
    buffers.v[i] = buffers.v[i] * params.leak_decay + current * gain;

    if buffers.v[i] >= params.threshold
        && ((packed & 0x80000000) == 0
            || ((current_tick - last_fire) & 0x00FFFFFF) > params.refractory_ticks)
    {
        // emit spike
        buffers.v[i] = params.reset_potential;
        buffers.last_spike[i] = 0x80000000 | ((neuron_type as u32) << 24) | (current_tick & 0x00FFFFFF);
        return true;
    }
    false
}
```

## Parallel scatter (fixed-point atomics + touched list)

```rust
pub fn scatter_tick(
    fired: &[u32],
    buffers: &CpuNeuronBuffers,
    grid: &SpatialGrid,
    params: &ConnParams,
    touched_out: &mut Vec<u32>,
) {
    let per_thread_touched: Vec<Vec<u32>> = fired
        .par_chunks(256)
        .map(|chunk| {
            let mut local_touched = Vec::with_capacity(chunk.len() * params.k);
            for &src in chunk {
                let src_type = (buffers.last_spike[src as usize] >> 24) & 0x7F;
                for j in 0..params.k {
                    let tgt = connectivity::target(src, j as u32, grid, params.k) as usize;
                    let w = connectivity::weight(src, j as u32, src_type);
                    buffers.I[tgt].fetch_add(w, Ordering::Relaxed);
                    local_touched.push(tgt as u32);
                }
            }
            local_touched
        })
        .collect();

    touched_out.clear();
    for mut local in per_thread_touched {
        touched_out.append(&mut local);
    }
    touched_out.extend_from_slice(&buffers.input_neurons);
    touched_out.sort_unstable();
    touched_out.dedup();
}
```

All target current writes are atomic. This is intentionally simpler than
region-partitioned scatter; profile before adding partition complexity.

## Active-list integration

The CPU backend is event-driven: integrate neurons that received current this
tick plus input-region neurons that receive ambient `I_ext`. Silent neurons are
not updated every tick; their decay is applied lazily the next time they appear
in the active list.

```rust
pub fn integrate_active(
    active: &[u32],
    fired_next: &mut Vec<u32>,
    buffers: &mut CpuNeuronBuffers,
    current_tick: u32,
    params: &LIFParams,
) {
    fired_next.clear();
    for &idx in active {
        let i = idx as usize;
        if integrate_neuron(i, current_tick, buffers, params) {
            fired_next.push(idx);
        }
    }
}
```

SIMD128 is still useful once the scalar active-list path is correct, but it is
an optimization over sorted active runs, not a reason to reintroduce data races.
Start scalar; add SIMD for contiguous runs after profiling.

## SIMD128 integration optimization

After the scalar active-list path is correct, add a SIMD128 fast path for
contiguous runs in the sorted active list:
1. Scan `active` for runs where indices are consecutive.
2. For runs of at least 16 neurons, load `v` in `f32x4` chunks.
3. Convert fixed-point `I` values to `f32` after `swap(0)`; this part may stay
   scalar if conversion cost dominates.
4. Use SIMD for leak/gain/threshold compare, then scalar extraction for fired
   indices and refractory checks.

Do not integrate the full neuron array just to make SIMD easier. The CPU
backend's comparison value is its event-driven execution model.

## WebGL2 rendering

The CPU backend writes to SharedArrayBuffer SoA (same layout as GPU buffers).
Each frame, the main thread uploads the changed region to WebGL2 textures or
buffer objects:

```typescript
// In CpuRenderer (WebGL2):
// Upload v_render[] and last_spike[] as two ARRAY_BUFFERs
gl.bindBuffer(gl.ARRAY_BUFFER, vBuf);
gl.bufferSubData(gl.ARRAY_BUFFER, 0, sharedVRender);    // full upload each frame
gl.bindBuffer(gl.ARRAY_BUFFER, lastSpikeBuf);
gl.bufferSubData(gl.ARRAY_BUFFER, 0, sharedLastSpike);

// Same vertex shader as GPU path; reads from attribute instead of storage buffer
// Fragment shader identical
```

Because the CPU sim uses lazy decay, rendering must not read stale `v` directly.
Before upload, update `v_render[i] = v[i] * leak_decay^(current_tick -
last_updated[i])` for the visible/uploaded range, or expose `last_updated` to the
WebGL shader and apply the same decay there. The MVP chooses `v_render` because
it keeps the WebGL shader matched to the GPU render shader.

Full upload each frame is acceptable for N ≤ 200k (≈ 1.6 MB / frame for
`v_render` + `last_spike`). For larger N, delta upload (only changed neurons) is
a future optimization.

## Determinism verification

On backend switch or explicit debug check, log first 100 targets for neuron 0
from both backends:
```
GPU targets[0..10]: [1423, 8821, 3302, ...]
CPU targets[0..10]: [1423, 8821, 3302, ...]  ← must match exactly
```

If they diverge, the BV22 hash or spatial target rule has drifted between Rust
and WGSL. Fix the golden-vector mismatch before continuing.

For the GPU path this is a one-off debug readback or tiny debug compute/test
path, never part of normal frame ticking. Normal CPU-vs-GPU comparison still
allows no CPU readback from the GPU backend in the rAF loop.

## Performance expectations (update with actual numbers from phase 0)
- CPU backend target: 100k–500k neurons at 60 fps on 8-core machine.
- Primary cost: scatter (dominated by random-access writes) + WebGL2 upload.
- SIMD gains mainly in the integrate pass (memory bandwidth bound).
