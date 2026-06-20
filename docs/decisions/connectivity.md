# Connectivity Decisions

## Procedural / implicit connectivity — no stored edge list

- **Decision.** Synapse targets and weights are pure deterministic functions of
  `(neuron_id, synapse_index, seed, source_type)`. No global edge list is stored
  or transmitted.
- **Why.** Storing all edges is the wrong browser tradeoff at this scale.
  Procedural wiring converts a memory wall into compute — and the compute is
  embarrassingly parallel and identical on Rust host and GPU paths, enabling the
  direct implementation comparison.
  It is also biologically appropriate: local cortex wiring follows a statistical
  spatial rule, not a named connectome.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/src/connectivity/mod.rs → target`, `weight`.
- **Tradeoffs.** Per-edge *activity* (spike count since first observed) cannot
  be regenerated and is accumulated lazily only for edges materialised in the
  zoomed-in view. This never touches the sim hot path.

## Reverse incoming view is morphology-only build data

- **Decision.** The simulation keeps procedural source-out connectivity, but the
  morphology builder materializes a deterministic reverse incoming view at
  network build time by evaluating production `target_with_cell` for every
  `(source_id, synapse_index)`. Every non-self raw incoming record is stored;
  visible socket groups aggregate duplicate `(source,target,socket)` records by
  summed absolute weight.
- **Why.** Real incoming dendrites need "who connects to this soma", which the
  hot sim path deliberately never stores. Building the reverse view once for
  morphology preserves the no-edge-list sim contract while giving the renderer
  honest dendrite sockets.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md),
  [`../architecture/manifold.md`](../architecture/manifold.md).
- **Tradeoffs.** This adds host-side init memory/time proportional to N*K.
  Morphology currently draws all unique incoming socket groups with no hidden
  visual drops. If density becomes too high, lower K or introduce an explicit
  cap policy before sampling.
- **Code anchors.** `crates/brain-visualizer/src/sim/morphology.rs →
  build_incoming_view, IncomingSynapse, IncomingRange, IncomingSocketGroup`.

## Per-tier out-degree K

- **Decision.** K is a per-tier knob, not a single global constant. Runtime tier
  presets and bounds are owned by `web/src/ui/controls.ts → TIER_PRESETS, N_MIN,
  N_MAX`; the Rust scaler range helper is
  `crates/brain-visualizer/src/sim/scaler.rs → TierRange`.
- **Why.** K × N drives synapse-event cost as much as N alone. Per-tier K lets
  lower-end devices run sparser-but-valid networks rather than just shrinking N,
  and gives finer control over the compute/quality tradeoff.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Alternatives considered.** A single global K was rejected because it forces
  lower tiers to pay full scatter cost for diminishing visual returns.

## Heavy-tailed long-range reach — local core + bounded tail, not a uniform stretch

- **Decision.** Long-range connectivity is a *heavy tail* on top of the local
  rule: a per-synapse integer hash coin (`REACH_COIN % REACH_FRAC_DEN`) flips a
  tunable fraction of synapses long-range, and those draw a wider bounded offset
  (`±max_reach`) that overwrites the local offset; the rest stay local. The
  local-only setting remains the deterministic baseline, while product defaults
  enable a non-zero tail through `VisualSettings::default`.
- **Why.** The network needed signal that visibly jumps across the cortex, not
  only diffuses locally — but the local clustering is the visual texture worth
  keeping.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Alternatives considered.** A single global multiplier on `LOCAL_D` (uniform
  stretch of every synapse's reach) was rejected: stretching all synapses
  uniformly washes out the local cluster density that gives the cortex its
  texture, instead of adding a sparse long-range tail on top of it.
- **Tradeoffs.** The coin hash is computed on every `target` call even when the
  chosen fraction is zero; this is deliberate and alters no target at the
  local-only baseline. Both knobs are kept integer (`long_range_frac` over a
  fixed `REACH_FRAC_DEN`, `max_reach` as a cell radius) so the rule never
  introduces a float distance comparison — preserving the Rust↔WGSL determinism
  contract. The knobs are a generation-time / brain-reset impact (they change
  target ids and thus the generated axon geometry), not a live render tweak.
- **Code anchors.** `crates/brain-visualizer/src/connectivity/mod.rs → ReachParams`,
  `long_offset_component`, `REACH_FRAC_DEN`;
  `crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl → target_neuron`.

## No per-synapse long-range flag — morphology classifies by world distance

- **Decision.** Connectivity bakes the heavy-tail reach coin into the resulting
  target id and exposes no per-synapse long-range flag; `target` /
  `target_with_cell` return the target id only. Morphology's curved long-range
  waypoint routing therefore classifies "visually long" by world distance, not by
  a connectivity flag.
- **Why.** Adding a flag would mean threading an extra return value (and an extra
  CPU↔GPU contract surface) purely for a visual concern. World-distance
  classification keeps the visual routing decoupled from the wiring rule, which
  stays target/weight-unchanged; waypoints never affect synaptic semantics.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md),
  [`../architecture/manifold.md`](../architecture/manifold.md).
- **Code anchors.** `crates/brain-visualizer/src/connectivity/mod.rs → target,
  target_with_cell`; `crates/brain-visualizer/src/sim/morphology.rs →
  long_range_waypoints`. (Full routing geometry in
  [`../architecture/manifold.md`](../architecture/manifold.md).)

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
  `crates/brain-visualizer/tests/wgsl_hash_determinism.rs`,
  `crates/brain-visualizer/tests/wgsl_target_determinism.rs`, and
  `crates/brain-visualizer/tests/wgsl_weight_determinism.rs`.
  The `target` gate runs with the heavy-tailed long-range branch **enabled**
  (non-zero `long_range_frac`/`max_reach`) and self-checks GPU `target_neuron`
  against the live Rust `target`, so the whole reach rule — not just the local
  path — is covered. The weight gate self-checks GPU `synapse_weight` against
  live Rust `weight()` for representative E/I source types and locks the shared
  fixed-point scale/layout contract. Neither target nor weight implementation
  (Rust / WGSL) may be edited without updating the other and re-running the
  matching gate.
- **Why.** Rust/WGSL determinism is load-bearing: the same seed must produce the
  same network in host-prepared data and GPU kernels. A silent constant drift
  would corrupt results without any visible crash.
- **Applies to.** [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `crates/brain-visualizer/tests/wgsl_hash_determinism.rs`,
  `crates/brain-visualizer/tests/wgsl_target_determinism.rs`,
  `crates/brain-visualizer/tests/wgsl_weight_determinism.rs`.

## See also

- [`../architecture/connectivity.md`](../architecture/connectivity.md)
- [`data-layout.md`](data-layout.md) — fixed-point scale shared with `weight()`
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
