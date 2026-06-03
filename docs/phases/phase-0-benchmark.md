# Phase 0 — Throwaway Benchmark Spike

_Throwaway. Purpose: measure real LIF throughput on target hardware before
committing to tier numbers. Start with native `wgpu` for fast iteration, then
run a browser WebGPU/WASM microbench before any shipped tier caps are locked.
Delete or archive after numbers are captured._

## Goal
Know, not guess, how many neurons and synaptic events per second are achievable
on a mid-range laptop GPU and a typical 8-core CPU. Validate or revise the tier
ceilings in `architecture.md §9`.

## Done when
- GPU benchmark reports neurons/sec and synaptic events/sec at N = 100k, 500k,
  1M, 5M (stop early if frame budget exceeded).
- CPU benchmark reports same at N = 10k, 50k, 100k, 500k.
- GPU benchmark logs adapter name/type plus relevant limits:
  `maxStorageBufferBindingSize`, `maxBufferSize`,
  `maxComputeWorkgroupsPerDimension`, `maxComputeInvocationsPerWorkgroup`,
  `maxComputeWorkgroupSizeX`, and whether `timestamp-query` is available.
- GPU benchmark records per-pass timings when `timestamp-query` is supported
  and falls back to wall-clock tick timing otherwise.
- Browser WebGPU/WASM microbench reports the same headline metrics at enough
  representative N/K points to set shipped tier caps. Native numbers may be
  used only as upper bounds.
- Numbers are pasted into `architecture.md §9` with the test machine spec.

## What to build

### Rust binary (native upper-bound benchmark)
`bench/src/main.rs` — standalone binary, not part of the main crate.

**GPU path (wgpu native):**
1. Init wgpu, request adapter, create device.
   - Request a high-performance adapter first.
   - Print adapter info and limits before allocating buffers.
   - Select scan/workgroup sizes from adapter limits rather than hard-coding
     desktop assumptions.
2. Allocate hot SoA buffers: `v: [f32; N]`, `I: [i32; N]`,
   `last_spike: [u32; N]` (12 B/neuron hot sim state). Add a dummy
   `positions: [[f32; 3]; N]` buffer only for memory-pressure runs that should
   match the full 24 B/neuron production layout.
3. Write minimal WGSL shaders:
   - Integrate pass: `v[i] = v[i] * LEAK + I[i] * SCALE_INV; I[i] = 0; if v[i] >= THRESH { spike_list[atomicAdd(spike_count, 1)] = i; v[i] = RESET; }`
   - Scatter pass: for each spike, for j in 0..K: `atomicAdd(I_next[hash_target(spike, j)], WEIGHT)`
4. Run 1000 ticks, record wall time, derive ticks/sec and synaptic events/sec.
5. Repeat at each N.

**GPU resource discipline for the benchmark:** allocate buffers and pipelines
once per `(N, K)` run, then reuse them for all ticks. Do not recreate bind groups,
pipelines, staging buffers, or command-independent resources inside the timed
loop. Use one command encoder per measured batch unless intentionally comparing
encoder overhead.

**Optional scan/compact micro-benchmark:** add a small separate measurement for
the count → prefix scan → scatter/compact pattern over synthetic bins. This is
not part of the SNN result, but it validates the primitive needed for near LOD
and any GPU-side compaction. Report bin count, scan implementation, and ms/pass.

### Browser WebGPU/WASM microbench
After the native run, build a minimal browser harness that runs the same shader
shape through the actual deployment stack:

1. WASM initializes WebGPU through the same bindings planned for production.
2. Allocate the same hot buffers and indirect dispatch buffers.
3. Run 300-1000 ticks per N/K point.
4. Report ticks/sec, synaptic events/sec, browser adapter limits, timestamp
   support, and whether cross-origin isolation / WASM threads are available.
5. Compare browser numbers to native numbers and mark shipped tier caps from
   browser results only.

No final UI is needed. A tiny HTML page with console JSON output is enough.

**CPU path (rayon, single-threaded first then parallel):**
1. Same SoA layout in Vec.
2. Event-driven: maintain `fired: Vec<u32>`.
3. Scatter fired → fixed-point `AtomicI32` current buffer, same representation
   as the GPU path.
4. Integrate with SIMD128 (or scalar first, SIMD second pass).
5. Record same metrics.

**Hash / connectivity:** use the locked BV22 32-bit hash of
`(neuron_id, synapse_index, salt)` modulo N as the target. Not the final spatial
rule; just needs to produce scatter pressure similar to the real design while
exercising the exact hash primitive the browser build will use.

## Output
```
=== GPU Benchmark ===
adapter=... type=... timestamp_query=true
limits maxStorageBufferBindingSize=... maxBufferSize=...
N=100000  K=32  ticks=1000  time=1.2s  ticks/s=833  synaptic_events/s=26.6M
  passes: integrate=...ms scatter=...ms
N=500000  K=64  ...
...

=== CPU Benchmark (8 threads) ===
N=10000   K=16  ...
...
```

Paste results into `architecture.md §9` under a "Benchmark results" subsection
with machine spec (GPU model, core count, RAM).

## What NOT to do
- No rendering.
- No final application UI.
- No final connectivity rule.
- Do not spend time making this code clean — it will be deleted.
