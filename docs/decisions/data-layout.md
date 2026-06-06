# Data Layout Decisions

## Packed valid-bit and type in `last_spike`

- **Decision.** `last_spike` is a single `u32` packing `HAS_SPIKED` (bit 31),
  7-bit neuron type (bits [30:24]), and 24-bit last-fire tick (bits [23:0]).
  There is no separate type array.
- **Why.** Eliminates a dedicated type buffer and its alignment padding, giving
  a 25% cache-density improvement (24 B vs ~32 B per neuron) in the hot
  integrate loop on both CPU L2/L3 and GPU L1/L2. Extraction is a mask + shift;
  zero extra bandwidth cost.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → HAS_SPIKED_MASK`,
  `TYPE_MASK`, `TICK_MASK`, `neuron_type`, `has_spiked`, `tick_diff`;
  `crates/brain-visualizer/src/connectivity/mod.rs → is_excitatory`.
- **Tradeoffs.** The 24-bit tick wraps at ~4.6 h of real-time simulation.
  Modular `tick_diff` stays correct for any interval shorter than half the wrap
  range; this is not a correctness concern at practical session lengths.

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
  `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → fixed_point_scale` (uniform field).

## Fixed-point overflow policy

- **Decision.** S = 2^12 is not an overflow proof: high fan-in during
  synchronised firing can overflow i32. Production code must either (a) measure
  and prove a per-tick fan-in bound for every tier with a debug high-water
  counter and warning threshold, or (b) use a saturating atomic
  compare-exchange loop. Plain `atomicAdd` is acceptable only during early
  development if overflow detection is treated as a blocker before tier caps
  are locked.
- **Why.** Silent overflow-to-negative causes hyperpolarisation instead of
  depolarisation — a correctness bug invisible from the render side.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md).
- **Revisit when.** Biological weights, K, connectivity locality, or
  excitability are revised upward.

## CPU scatter uses the same fixed-point atomics

- **Decision.** The CPU backend uses the same i32 fixed-point representation
  and applies every synaptic contribution via `AtomicI32::fetch_add`. Per-thread
  partial current buffers with a full reduction are rejected.
- **Why.** A shared representation makes the CPU and GPU paths directly
  comparable and eliminates a conversion step on the CPU side. Allocation and
  zeroing of per-thread buffers per tick would cost more than simple atomics
  at the neuron counts this backend targets.
- **Applies to.** [`../architecture/data-model.md`](../architecture/data-model.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/cpu/mod.rs` (scatter loop).
- **Revisit when.** Profiling shows AtomicI32 contention dominates CPU tick
  time, at which point spatial partitioning becomes worth measuring.

## See also

- [`../architecture/data-model.md`](../architecture/data-model.md)
- [`connectivity.md`](connectivity.md) — fixed-point scale shared with weight()
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
