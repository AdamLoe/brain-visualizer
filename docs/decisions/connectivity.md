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

## Heavy-tailed long-range reach — local core + bounded tail, not a uniform stretch

- **Decision.** Long-range connectivity is a *heavy tail* on top of the local
  rule: a per-synapse integer hash coin (`REACH_COIN % REACH_FRAC_DEN`) flips a
  tunable fraction of synapses long-range, and those draw a wider bounded offset
  (`±max_reach`) that overwrites the local offset; the rest stay local. It is
  off by default (`long_range_frac = 0`).
- **Why.** The network needed signal that visibly jumps across the cortex, not
  only diffuses locally — but the local clustering is the visual texture worth
  keeping.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Alternatives considered.** A single global multiplier on `LOCAL_D` (uniform
  stretch of every synapse's reach) was rejected: stretching all synapses
  uniformly washes out the local cluster density that gives the cortex its
  texture, instead of adding a sparse long-range tail on top of it.
- **Tradeoffs.** The coin hash is computed on every `target` call even when the
  feature is dormant; this is deliberate and costs nothing observable (it alters
  no target at `frac = 0`, so output stays bit-identical to the local-only
  network). Both knobs are kept integer (`long_range_frac` over a fixed
  `REACH_FRAC_DEN`, `max_reach` as a cell radius) so the rule never introduces a
  float distance comparison — preserving the CPU↔GPU determinism contract. The
  knobs are a generation-time / brain-reset impact (they change target ids and
  thus the generated axon geometry), not a live render tweak.
- **Code anchors.** `crates/brain-visualizer/src/connectivity/mod.rs → ReachParams`,
  `long_offset_component`, `REACH_FRAC_DEN`;
  `crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl → target_neuron`.

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
  The `target` gate runs with the heavy-tailed long-range branch **enabled**
  (non-zero `long_range_frac`/`max_reach`) and self-checks GPU `target_neuron`
  against the live Rust `target`, so the whole reach rule — not just the local
  path — is covered. None of the three `target` implementations (Rust / CPU /
  WGSL) may be edited without updating the others and re-running this gate.
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
