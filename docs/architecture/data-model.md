---
status:        active
owner:         adamg
last_updated:  2026-06-20
---

# Data Model

GPU-resident Structure-of-Arrays holding every neuron's dynamic state. One flat
array per field; shaders index by neuron id and never touch an array-of-structs.
All fields live in `wgpu::Buffer` handles managed by
`crates/brain-visualizer/src/sim/gpu/resources.rs → GpuResources`.

## What it owns

- The SoA fields: `pos_x / pos_y / pos_z` (f32, static), `v` (f32,
  membrane potential), `I` (i32, fixed-point current accumulator),
  `last_spike` (u32, packed) — see `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`
  binding declarations for authoritative types.
- The `last_spike` packed word layout and mask constants
  (`HAS_SPIKED_MASK`, `TYPE_MASK`, `TICK_MASK`) defined in
  `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`.
- The `tick_diff` modular 24-bit helper — `integrate.wgsl → tick_diff`.
  Rust and WGSL wrap behavior is gated by
  `crates/brain-visualizer/tests/wgsl_tick_wrap.rs`.
- The fixed-point current scale `FIXED_POINT_SCALE = 4096`
  (`crates/brain-visualizer/src/connectivity/mod.rs → FIXED_POINT_SCALE`);
  `crates/brain-visualizer/src/sim/backend.rs → FIXED_POINT_SCALE` re-exports
  that same authority for `SimConfig`.
- The shared chunked-buffer split math used by large GPU storage fields —
  `crates/brain-visualizer/src/buffers.rs → ChunkLayout`.

## What it does NOT own

- Buffer allocation and bind-group wiring — [`gpu-backend.md`](gpu-backend.md).
- Simulation passes that read/write these fields —
  [`simulation.md`](simulation.md).
- Connectivity rule that populates targets at scatter time —
  [`connectivity.md`](connectivity.md).

## Packed `last_spike` word

The exact mask layout lives in
`crates/brain-visualizer/src/sim/backend.rs → HAS_SPIKED_MASK, TYPE_MASK,
TICK_MASK, neuron_type, has_spiked, tick_diff` and the mirrored WGSL helpers in
`crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → HAS_SPIKED_MASK,
TYPE_MASK, TICK_MASK, neuron_type, has_spiked, tick_diff`. The packed tick wraps
modulo `TICK_MASK`; all comparisons use `tick_diff`, and the Rust/WGSL wrap
behavior is gated by `cargo test` via
`crates/brain-visualizer/tests/wgsl_tick_wrap.rs`.

Packing type into `last_spike` eliminates a dedicated type array and its
alignment padding. The current per-neuron footprint is locked by
`crates/brain-visualizer/src/sim/gpu/resources.rs → NeuronBuffers` and its
`neuron_buffer_layouts_match_n` test under `cargo test`; keep the budget
discussion in that owner instead of duplicating field math here.

New neurons start with `HAS_SPIKED = 0`, type bits initialized, tick bits
zero. Render shaders must treat `HAS_SPIKED = 0` as zero glow — never as a
fresh spike — to preserve the silent-start look. Far-body soma pulse age still
comes from this packed physics word. Morphology tube packet age comes from the
morphology-only `visual_spike` buffer; `last_spike` remains the type/color and
physics source for that pass.

## Fixed-point current accumulator

`I[i]` is an `i32` fixed-point value scaled by `FIXED_POINT_SCALE`. WGSL has no
f32 atomics, so `atomicAdd` on an `i32` buffer is the only race-free scatter
primitive. The integration pass converts on read through
`crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → integrate`.

The fixed-point scale keeps individual synaptic weights in a comfortable range,
but fan-in during synchronised firing can accumulate enough contributions to
overflow i32.
Scatter keeps a debug high-water `max_abs_current` atomic, read once per native
test/harness batch through `GpuBackend::max_abs_current_hw`; the product render
loop does not read it. `crates/brain-visualizer/tests/gpu_current_overflow.rs`
forces full-network synchrony at product max N with K above the product default
and fails unless the observed current stays well below `i32::MAX`.

Rust host tests and WGSL shaders use the same fixed-point scale and wrapping
helpers so deterministic gates can compare the live GPU kernels against Rust
golden behavior. `crates/brain-visualizer/tests/wgsl_weight_determinism.rs`
locks the scale authority, `ConnectUniforms` field offsets, and live WGSL
`synapse_weight` outputs against Rust `weight()`.

## Chunked storage layout

Each SoA field is a `ChunkedBuffer` (`crates/brain-visualizer/src/buffers.rs → ChunkedBuffer`).
When a field's total byte size exceeds `MAX_CHUNK_BYTES`, the buffer is split
into multiple `wgpu::Buffer` handles. Shaders index through the `ChunkLayout`
math instead of a second data model.

The layout math is GPU-free and fully host-testable. The `ChunkLayout`
unit tests in `crates/brain-visualizer/src/buffers.rs` gate this invariant.
Morphology segment storage reuses the same math for `MorphSegment` records, but
each render/compaction pass binds one segment chunk at a time rather than
shader-indexing across chunks; see
`crates/brain-visualizer/src/sim/gpu/resources.rs → morph_segment_chunk_layout`.

Positions are three independent 4-byte fields (`pos_x`, `pos_y`, `pos_z`),
never `array<vec3<f32>>`. Using a vec3 would impose a 16-byte stride and
break the compact per-neuron budget.

## Update when

- A new per-neuron SoA field is added or an existing one changes type.
- The `last_spike` bit packing changes (also requires updating
  `integrate.wgsl`, `render_far.wgsl`, and any other shader that reads
  the masks).
- The fixed-point scale S changes (update `FIXED_POINT_SCALE` in
  `crates/brain-visualizer/src/connectivity/mod.rs`, keep the backend re-export
  in `crates/brain-visualizer/src/sim/backend.rs`, and update `fixed_point_scale`
  consumers in `integrate.wgsl` / `scatter.wgsl`).
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
