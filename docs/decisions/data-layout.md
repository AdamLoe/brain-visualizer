# Data Layout Decisions

## Packed valid-bit and type in `last_spike`

- **Decision.** `last_spike` is a single `u32` packing `HAS_SPIKED` (bit 31),
  7-bit neuron type (bits [30:24]), and 24-bit last-fire tick (bits [23:0]).
  There is no separate type array.
- **Why.** Eliminates a dedicated type buffer and its alignment padding in the
  hot integrate loop. Extraction is a mask + shift; zero extra bandwidth cost.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → HAS_SPIKED_MASK`,
  `TYPE_MASK`, `TICK_MASK`, `neuron_type`, `has_spiked`, `tick_diff`;
  `crates/brain-visualizer/src/connectivity/mod.rs → is_excitatory`.
- **Tradeoffs.** The packed tick wraps by design. Modular `tick_diff` stays
  correct within the representable comparison window, and `cargo test` gates the
  Rust/WGSL behavior through `crates/brain-visualizer/tests/wgsl_tick_wrap.rs`.

## Fixed-point current scale S = 2^12

- **Decision.** The i32 synaptic current accumulator uses scale factor
  `S = 4096 (2^12)`. WGSL has no f32 atomics; i32 `atomicAdd` is the only
  race-free scatter primitive, and S = 2^12 keeps individual contributions in
  a comfortable range.
- **Why.** f32 atomics do not exist in WGSL. A power-of-two scale makes
  the conversion `f32(I) / fixed_point_scale` exact and lets the compiler
  replace the division with a multiply by a compile-time constant.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md).
- **Code anchors.** `crates/brain-visualizer/src/connectivity/mod.rs → FIXED_POINT_SCALE`;
  `crates/brain-visualizer/src/sim/backend.rs → FIXED_POINT_SCALE`;
  `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → fixed_point_scale`;
  `crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl → synapse_weight`;
  `crates/brain-visualizer/tests/wgsl_weight_determinism.rs`.

## Fixed-point overflow policy

- **Decision.** The production scatter path keeps plain i32 `atomicAdd` plus a
  `max_abs_current` high-water counter, and the native test suite proves the
  product envelope with a synchronous full-network stress gate. The gate fails
  if the measured high-water loses its fixed i32 safety margin; saturating
  atomics are reserved for the moment that executable bound no longer holds.
- **Why.** Silent overflow-to-negative causes hyperpolarisation instead of
  depolarisation, but saturating compare-exchange would add complexity to the hot
  scatter path before the measured product envelope needs it.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl → scatter`;
  `crates/brain-visualizer/tests/gpu_current_overflow.rs → synchronized_scatter_current_stays_below_i32_margin`.
- **Revisit when.** Biological weights, K, connectivity locality, excitability,
  or tier caps are revised upward enough to shrink the measured margin.

## Chunk large GPU storage bindings by byte budget

- **Decision.** Large GPU storage arrays use `ChunkLayout` with the default chunk
  budget from `crates/brain-visualizer/src/buffers.rs → MAX_CHUNK_BYTES` and any
  tighter adapter storage-binding limit. Morphology segment storage reuses that
  byte-budget math for `MorphSegment` records, while render/compaction bind one
  segment chunk at a time.
- **Why.** WebGPU storage bindings can be smaller than the logical data set.
  Chunking preserves the flat logical data model without forcing CPU readback,
  layout changes, or hidden generator throttles to stay below one binding.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).
- **Code anchors.** `crates/brain-visualizer/src/buffers.rs → ChunkLayout`;
  `crates/brain-visualizer/src/sim/gpu/resources.rs → morph_segment_chunk_layout`.

## See also

- [`../architecture/data-model.md`](../architecture/data-model.md)
- [`connectivity.md`](connectivity.md) — fixed-point scale shared with weight()
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
