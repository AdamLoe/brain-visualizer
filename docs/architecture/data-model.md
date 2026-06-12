---
status:        active
owner:         adamg
last_updated:  2026-06-04
---

# Data Model

GPU-resident Structure-of-Arrays holding every neuron's dynamic state. One flat
array per field; shaders index by neuron id and never touch an array-of-structs.
All fields live in `wgpu::Buffer` handles managed by
`crates/brain-visualizer/src/sim/gpu/resources.rs → GpuResources`.

## What it owns

- The six SoA fields: `pos_x / pos_y / pos_z` (f32, static), `v` (f32,
  membrane potential), `I` (i32, fixed-point current accumulator),
  `last_spike` (u32, packed) — see `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`
  binding declarations for authoritative types.
- The `last_spike` packed word layout and the three mask constants
  (`HAS_SPIKED_MASK`, `TYPE_MASK`, `TICK_MASK`) defined in
  `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`.
- The `tick_diff` modular 24-bit helper — `integrate.wgsl → tick_diff`.
  Rust and WGSL wrap behavior is gated by
  `crates/brain-visualizer/tests/wgsl_tick_wrap.rs`.
- The fixed-point current scale `FIXED_POINT_SCALE = 4096`
  (`crates/brain-visualizer/src/connectivity/mod.rs → FIXED_POINT_SCALE`).
- The chunked-buffer split strategy for fields that exceed the 128 MiB
  WebGPU binding limit — `crates/brain-visualizer/src/buffers.rs → ChunkLayout`.

## What it does NOT own

- Buffer allocation and bind-group wiring — [`gpu-backend.md`](gpu-backend.md).
- Simulation passes that read/write these fields —
  [`simulation.md`](simulation.md).
- Connectivity rule that populates targets at scatter time —
  [`connectivity.md`](connectivity.md).

## Packed `last_spike` word

```
bit 31      = HAS_SPIKED  (1 = has ever fired)
bits [30:24] = 7-bit type  (E/I flag in bit 24, cortical region in bits [30:25])
bits [23:0]  = 24-bit tick of last fire
```

The 24-bit tick counter wraps at 2^24 ≈ 16.7M ticks (≈ 4.6 h at 1 ms/tick).
All comparisons use `tick_diff(a, b) = (a − b) & TICK_MASK`; this stays
correct as long as the compared interval is less than half the wrap range. The
native Rust helper and the production WGSL helpers used by integrate, metrics,
and far-glow rendering are covered by the tick-wrap gate.

Packing type into `last_spike` eliminates a dedicated type array and its
alignment padding. Per-neuron footprint is **24 B** (25% better cache
density than the naïve 32 B layout). At 1M neurons that is 24 MB of GPU
buffers, well within budget on integrated and discrete hardware alike.

New neurons start with `HAS_SPIKED = 0`, type bits initialized, tick bits
zero. Render shaders must treat `HAS_SPIKED = 0` as zero glow — never as a
fresh spike — to preserve the silent-start look. The render path now uses this
same packed word for both the far-body soma pulse and the morphology traveling
impulse: shaders derive age as `tick_diff(current_tick, last_tick)` and only
emit pulse/flash/core terms when `HAS_SPIKED != 0`.

## Fixed-point current accumulator

`I[i]` is an `i32` fixed-point value scaled by `S = 4096 (2^12)`. WGSL
has no f32 atomics, so `atomicAdd` on an `i32` buffer is the only race-free
scatter primitive. The integration pass converts on read:
`current = f32(I[i]) / fixed_point_scale`.

S = 2^12 keeps individual synaptic weights in a comfortable range, but fan-in
during synchronised firing can accumulate enough contributions to overflow i32.
Scatter keeps a debug high-water `max_abs_current` atomic, read once per native
test/harness batch through `GpuBackend::max_abs_current_hw`; the product render
loop does not read it. `crates/brain-visualizer/tests/gpu_current_overflow.rs`
forces full-network synchrony at product max N with K above the product default
and fails unless the observed current stays well below `i32::MAX`.

The CPU backend uses the same representation with `AtomicI32` adds, so
the Rust and WGSL paths produce identical fixed-point values for the same
inputs.

## Chunked SoA layout

Each SoA field is a `ChunkedBuffer` (`crates/brain-visualizer/src/buffers.rs → ChunkedBuffer`).
When a field's total byte size exceeds `MAX_CHUNK_BYTES` (64 MiB), the
buffer is split into multiple `wgpu::Buffer` handles. Shaders index via
`chunk = neuron_id / chunk_size`, `local = neuron_id % chunk_size`.

The layout math is GPU-free and fully host-testable. The `ChunkLayout`
unit tests in `crates/brain-visualizer/src/buffers.rs` gate this invariant.

Positions are three independent 4-byte fields (`pos_x`, `pos_y`, `pos_z`),
never `array<vec3<f32>>`. Using a vec3 would impose a 16-byte stride and
break the 24 B per-neuron budget.

## Update when

- A new per-neuron SoA field is added or an existing one changes type.
- The `last_spike` bit packing changes (also requires updating
  `integrate.wgsl`, `render_far.wgsl`, and any other shader that reads
  the masks).
- The fixed-point scale S changes (update `FIXED_POINT_SCALE` in
  `crates/brain-visualizer/src/connectivity/mod.rs` and `fixed_point_scale` in the uniform struct
  in `integrate.wgsl`).
- `MAX_CHUNK_BYTES` in `crates/brain-visualizer/src/buffers.rs` changes.

## See also

- [`connectivity.md`](connectivity.md) — procedural rule that feeds `I`
  via the scatter pass; shares `FIXED_POINT_SCALE`.
- [`simulation.md`](simulation.md) — the integrate + scatter passes that
  read and write these buffers each tick.
- [`gpu-backend.md`](gpu-backend.md) — `GpuResources` allocation and
  bind-group lifecycle.
- [`../decisions/data-layout.md`](../decisions/data-layout.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
