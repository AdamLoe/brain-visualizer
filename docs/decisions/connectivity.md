# Connectivity Decisions

## Procedural / implicit connectivity — no stored edge list

- **Decision.** Synapse targets and weights are pure deterministic functions of
  `(neuron_id, synapse_index, seed, source_type)`. No global edge list is stored
  or transmitted.
- **Why.** Storing all edges is infeasible in-browser at any interesting neuron
  count (K=64 × 1M neurons × 4 B = 256 MB; 10M would be 2.5 GB). Procedural
  wiring converts a memory wall into compute — and the compute is embarrassingly
  parallel and identical on CPU and GPU, enabling the direct backend comparison.
  It is also biologically appropriate: local cortex wiring follows a statistical
  spatial rule, not a named connectome.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/src/connectivity/mod.rs → target`, `weight`.
- **Tradeoffs.** Per-edge *activity* (spike count since first observed) cannot
  be regenerated and is accumulated lazily only for edges materialised in the
  zoomed-in view. This never touches the sim hot path.

## Per-tier out-degree K

- **Decision.** K is a per-tier knob, not a single global constant. Low tier:
  K ≈ 16–32. Balanced: K ≈ 32–64. Max: K ≈ 64–128. The adaptive scaler may
  compress or expand K within a tier alongside N.
- **Why.** K × N drives synapse-event cost as much as N alone. Per-tier K lets
  lower-end devices run sparser-but-valid networks rather than just shrinking N,
  and gives finer control over the compute/quality tradeoff.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Alternatives considered.** A single global K=64 was rejected because it
  forces lower tiers to pay full scatter cost for diminishing visual returns.

## 32-bit hash — not u64 PCG

- **Decision.** The hash primitive is a pure `u32` lowbias32 avalanche hash
  implemented identically in Rust (`crates/brain-visualizer/src/connectivity/hash.rs → hash32`) and
  WGSL (`crates/brain-visualizer/src/sim/gpu/shaders/hash.wgsl → hash32`). `u64` PCG is not used.
- **Why.** WGSL has no native `u64`. Emulating it with manual carry arithmetic
  would add shader complexity and introduce a determinism risk between CPU and
  GPU backends. The 32-bit hash avoids both problems.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/src/connectivity/hash.rs → hash32`, `mix_key`;
  `crates/brain-visualizer/src/sim/gpu/shaders/hash.wgsl → hash32`, `mix_key`.

## Golden-vector gate for hash determinism

- **Decision.** The Rust `hash32` and `mix_key` implementations carry inline
  golden-vector tests (`crates/brain-visualizer/src/connectivity/hash.rs` test module). The WGSL
  implementations are validated against the same vectors by
  `crates/brain-visualizer/tests/wgsl_hash_determinism.rs` and `crates/brain-visualizer/tests/wgsl_target_determinism.rs`.
  Neither side may be edited without updating both together and re-deriving
  the golden vectors.
- **Why.** CPU/GPU determinism is load-bearing: the same seed must produce the
  same network on both backends, which is the entire basis of the backend
  comparison. A silent constant drift would corrupt results without any visible
  crash.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/tests/wgsl_hash_determinism.rs`,
  `crates/brain-visualizer/tests/wgsl_target_determinism.rs`.

## See also

- [`../architecture/connectivity.md`](../architecture/connectivity.md)
- [`data-layout.md`](data-layout.md) — fixed-point scale shared with `weight()`
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
