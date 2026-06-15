# Decisions — Dynamics

## Self-organized criticality is the tuning target

- **Decision.** The network is tuned toward self-organized criticality (SOC):
  the "interesting" regime is the edge between silent and saturated, where
  activity forms neuronal avalanches (a branching ratio σ ≈ 1). Excitability is
  the single slider that sweeps silent → critical → seizure across that edge.
- **Why.** A point-LIF network with no learning has two boring attractors —
  everything dies, or everything fires together. The visually and scientifically
  interesting dynamics (cascades, propagating waves, scale-free avalanches) live
  only near the critical point, so that is what the defaults and the slider aim
  at. "Beauty" here *is* criticality.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → integrate` (the
  `gain = 0.5 + excitability*1.5` mapping); branching-ratio / cascade metrics in
  `crates/brain-visualizer/src/sim/gpu/mod.rs → metrics_snapshot`.

## Ambient `i_ext` into the input region is the sole external energy

- **Decision.** The only external energy injected into the network is a constant
  ambient current `i_ext` into input-region neurons. There is no special sink —
  dissipation is the global E/I balance plus the membrane leak. Activity
  self-organizes from this one source.
- **Why.** A single, localized, biologically-motivated energy source (sensory
  drive into a "posterior" input region) produces the natural
  silent → input → association → output propagation we want, instead of a
  uniform soup. Adding artificial sinks would be tuning hacks that hide whether
  the E/I balance is actually correct.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `crates/brain-visualizer/src/manifold/regions.rs → assign_regions`; the
  `is_input_region` test and `i_ext` add in
  `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → integrate`.

## Region topology is functional, not spatially blocked

- **Decision.** Input / association / output membership is assigned
  **uniformly at random over the volume**, not as contiguous anatomical lobes.
  "Input" means *receives ambient drive*, "output" means *no special treatment*
  — functional roles, not geography. Directionality comes from a mild anterior
  (+Z) feed-forward bias on a fraction of excitatory synapses.
- **Why.** Scattering the regions keeps drive sources spread through the visible
  volume (so propagation is visible everywhere, not just at one pole) while the
  feed-forward bias still gives activity a coherent front. Blocking the regions
  spatially looked like three glowing slabs and hid the recurrent structure.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md) (the anterior bias).
- **Code anchors.** `crates/brain-visualizer/src/manifold/regions.rs → assign_regions`,
  `RegionKind`; anterior-bias draw in
  `crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl → target_neuron`.

## E/I balance is hash-assigned

- **Decision.** The E/I flag is hash-assigned in the neuron type byte.
  Excitatory weights are positive, inhibitory negative — that asymmetry *is* the
  global dissipation mechanism.
- **Why.** A cortical-style excitatory/inhibitory mix with stronger inhibitory
  synapses is the standard balanced-network recipe for stable, critical-capable
  dynamics. Deriving E/I from the seed hash keeps it deterministic and
  backend-identical.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md),
  [`connectivity.md`](connectivity.md) (the signed weight rule).
- **Code anchors.** `crates/brain-visualizer/src/sim/backend.rs → neuron_type_byte`;
  `crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl → synapse_weight`.

## Per-neuron heterogeneity is hash-derived, and het=0 is the baseline

- **Decision.** Each neuron's threshold, leak, refractory, input-sensitivity, and
  weight-scale are the global constant perturbed by a deterministic symmetric draw
  from `hash32(seed, id, salt)`, scaled by a global `heterogeneity ∈ [0,1]`.
  **`heterogeneity = 0` reproduces the global-constant baseline bit-for-bit** —
  the `* het` term must vanish exactly, not approximately. The clean product
  default is `heterogeneity = 0.50`; users/tests can still set `0` for the
  homogeneous regression baseline.
- **Why.** Real neurons are not identical, and a spread of thresholds/leaks
  broadens the avalanche distribution and removes lockstep synchrony. Deriving it
  from the locked connectivity hash keeps it free (no stored per-neuron table) and
  reproducible. The het=0 ≡ baseline guarantee makes heterogeneity a safe,
  bisectable knob: any dynamics change at het=0 is a real regression, not noise.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl` (`hspread`, `SALT_*`,
  `*_SPREAD`); the shared hash is prepended via `pipelines::HASH_WGSL`.

## Weight normalization defaults to `sqrt_k`

- **Decision.** Recurrent synaptic current is scaled by a K-invariant
  `weight_norm_factor` with modes `none | sqrt_k | k`; the default is `sqrt_k`,
  computed relative to a reference degree `K_REF = 16`. At `K == K_REF` every mode
  yields exactly `1.0`.
- **Why.** As fan-in K grows, total recurrent drive per neuron grows with it;
  without normalization, changing K silently rescales excitability. `sqrt_k`
  matches the balanced-network scaling (input variance ∝ K, so std ∝ √K) and keeps
  per-neuron drive roughly K-invariant. Pinning the factor to `1.0` at the K=16
  default means the default config is bit-for-bit identical to the un-normalized
  era — normalization only matters when you change K.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/mod.rs → weight_norm_factor`, `K_REF`.
- **Tradeoffs.** `sqrt_k` is the principled middle ground; `k` over-attenuates
  (mean-field), `none` is only useful for studying raw K effects.

## Input-mode set: constant / poisson / pulsed / cursor_only / scripted / off

- **Decision.** Input-region drive shape is selectable: `constant` (steady
  `i_ext`, the default and tuning baseline), `poisson` (`i_ext` read as a per-tick
  spike probability, drawn per-neuron-per-tick from the hash), `pulsed` (periodic
  burst), `cursor_only` (no ambient — only `stimulate()`), `scripted`
  (placeholder, currently constant), `off`. Modes affect input-region neurons only.
- **Why.** Different demonstrations want different drive: `constant` for tuning
  the critical point, `poisson` for realistic stochastic input, `pulsed` for
  visible evoked waves, `cursor_only` for interactive pokes. `constant` is the
  default so the tunable baseline is the stable one; the noise/burst modes are
  opt-in. `scripted` is reserved for future timed stimulus sequences.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl` (`switch u.input_mode`).

## Low-excitability, low-`i_ext` as the product default (beauty/readability first)

- **Decision.** The accepted product defaults are `excitability=0.10`,
  `i_ext=0.014`, `n=6000`, `k=16`. Resting morphology connections are hidden
  by default (`morphRestingOpacity=0.0`, `lighting.restingBrightness=0.0`).
  Only active and recently-fired segments render.
- **Why.** At high excitability and high `i_ext` the network saturates quickly
  — activity covers everything and individual propagating cascades are invisible.
  A quiet network where signals propagate as sparse, visible wavefronts is both
  more beautiful and more informative for casual viewers. Hiding resting
  connections removes the dense static mesh that obscures firing structure at
  high N. The `connectionLayer=1` (Active/recent) mode together with low drive
  gives a clean signal-on-black aesthetic.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md),
  [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/core/types.ts → DEFAULT_CONFIG`;
  `web/src/core/settings.ts → DEFAULT_SETTINGS` (iExt idx 12,
  morphRestingOpacity idx 15); `web/src/core/morph-config.ts → DEFAULT_MORPH_CONFIG`
  (lighting.restingBrightness);
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::default`.

## Heavy-tailed synapse reach on by default

- **Decision.** The product defaults enable heavy-tailed long-range connectivity:
  `longRangeReachFrac=0.14` (14 % of synapses routed long-range) and
  `maxReachCells=14`. These are indices 24 and 25 of `VisualSettings` and are
  mirrored in the Rust `VisualSettings::default`.
- **Why.** Pure local connectivity produces visually monotonous short-range
  clusters. A heavy-tailed reach fraction adds long-range structure — distinct
  hub-to-hub links — that is visible at the product's default N=6000, making
  the network look more brain-like without any anatomy hard-coding. Setting the
  default to non-zero means new users see this structure immediately.
- **Applies to.** [`../architecture/simulation.md`](../architecture/simulation.md),
  [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Code anchors.** `web/src/core/settings.ts → DEFAULT_SETTINGS`
  (longRangeReachFrac idx 24, maxReachCells idx 25);
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::default`.
- **Revisit when.** N grows substantially beyond 6000 and long-range connections
  dominate render cost, or when a geometry-based connectivity model makes
  reach-fraction a derived rather than tunable parameter.

## See also

- [`../architecture/simulation.md`](../architecture/simulation.md).
- [`scope.md`](scope.md) — why point-LIF and at what scale.
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md).
