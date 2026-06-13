---
status:        active
owner:         adamg
last_updated:  2026-06-13
---

# Connectivity

Procedural / implicit synapse wiring: there is no stored edge list. Every
synapse target and weight is a pure deterministic function of the source neuron
id, synapse index, and a global seed — computed on demand by both the Rust CPU
path and the WGSL GPU path, producing bit-identical results.

## What it owns

- The `target(i, j, grid, k, seed, source_type)` and `weight(i, j, source_type)`
  pure functions — `crates/brain-visualizer/src/connectivity/mod.rs → target`, `weight`.
- The integer spatial grid (`SpatialGrid`), its CSR layout, and the
  `pack` / `unpack` / `cell_of` / `neurons_in_cell` API —
  `crates/brain-visualizer/src/connectivity/spatial.rs → SpatialGrid`.
- The local neighbourhood constant `LOCAL_D` and the anterior feed-forward bias
  (`ANTERIOR_BIAS_NUM / ANTERIOR_BIAS_DEN`) —
  `crates/brain-visualizer/src/connectivity/mod.rs → LOCAL_D`, `ANTERIOR_BIAS_NUM`.
- The heavy-tailed long-range reach: the `ReachParams` knobs, the
  `REACH_FRAC_DEN` integer denominator, and the `long_offset_component` helper —
  `crates/brain-visualizer/src/connectivity/mod.rs → ReachParams`, `REACH_FRAC_DEN`.
- The `hash32` / `mix_key` primitives and their locked constants —
  `crates/brain-visualizer/src/connectivity/hash.rs → hash32`, `mix_key` and
  `crates/brain-visualizer/src/sim/gpu/shaders/hash.wgsl → hash32`, `mix_key`.
- The salt constants that decorrelate hash uses —
  `crates/brain-visualizer/src/connectivity/mod.rs → salt`.
- The per-tier out-degree knob K and the store-once vs regenerate tradeoff.
- The determinism gate tests:
  `crates/brain-visualizer/tests/wgsl_hash_determinism.rs`, `crates/brain-visualizer/tests/wgsl_target_determinism.rs`.

## What it does NOT own

- The fixed-point current scale applied to weights —
  [`data-model.md`](data-model.md) (`FIXED_POINT_SCALE`).
- The scatter pass that consumes targets and weights each tick —
  [`simulation.md`](simulation.md).
- The GPU buffer that caches `cell_of_neuron` — [`gpu-backend.md`](gpu-backend.md).

## How target and weight are computed

`target(i, j, ...)` is a pure integer function:

1. Look up the packed integer cell id of source neuron `i` via `SpatialGrid`.
2. Hash `(seed, i, j, CELL_OFFSET)` → three independent axis offsets in
   `[−LOCAL_D, LOCAL_D]`. For excitatory neurons, a deterministic fraction
   (`ANTERIOR_BIAS_NUM / ANTERIOR_BIAS_DEN ≈ 31%`) of synapses force the Z
   offset to `+LOCAL_D`, creating the posterior→anterior feed-forward bias.
   Inhibitory synapses are left unbiased.
3. **Heavy-tailed reach coin flip.** Hash `(seed, i, j, REACH_COIN) %
   REACH_FRAC_DEN` (denominator `256`) and compare against the per-run
   `ReachParams.long_range_frac`. When `coin < long_range_frac`, the synapse is
   long-range: a fresh `(seed, i, j, REACH_OFFSET)` hash, sliced 10 bits per
   axis like the local path, **overwrites** the local (already anterior-biased)
   `(dx, dy, dz)` with wider offsets in `[−max_reach, max_reach]`
   (`long_offset_component`). Most synapses keep their local offset; a tunable
   tail jumps far. This branch is integer-only — no float distance compare.
4. Clamp the candidate cell coordinate to the grid boundary.
5. If the candidate cell is empty, walk outward by increasing Chebyshev radius
   until an occupied cell is found (`nearest_occupied` in
   `crates/brain-visualizer/src/connectivity/mod.rs`).
6. Hash `(seed, i, j, IN_CELL_PICK)` to pick a neuron within the chosen cell.

The two reach knobs are integers so the path stays float-free:
`long_range_frac` is a numerator over `REACH_FRAC_DEN` (`0` = always local,
`256` = always long-range) and `max_reach` is a cell radius `≥ 1`. They are
**dormant by default**: `ReachParams::LOCAL_ONLY` (`long_range_frac: 0`) and the
`VisualSettings` default (`long_range_reach_frac: 0.0`) leave the coin compare
always false, so the long-range branch never runs and output is bit-identical to
the local-only network (the coin hash is still computed but changes no target).
The dev-panel `longRangeReachFrac` (0..1) / `maxReachCells` knobs convert to
these integers at the `set_visual_settings` boundary
(`crates/brain-visualizer/src/sim/gpu/mod.rs → current_reach`); changing either
rebuilds the axon geometry (a brain-reset/morphology-rebuild impact, not live).

`weight(i, j, source_type)` hashes `(0, i, j, WEIGHT)` — seed-independent
so weight is a property of the synapse identity, not the network instance.
Excitatory weights land in `[1000, 4095]` fixed-point units; inhibitory in
`[−2000, −1000]`. Both are already scaled by `S = 4096`.

There is **no float distance comparison** anywhere on this path. All
arithmetic after the initial world→cell quantization is integer.

## Build-time reverse view for morphology

Connectivity remains source-out and implicit for simulation. The morphology
builder is the one exception: at network build time it evaluates the production
`target_with_cell` rule once for every `(source_id, synapse_index)` and stores a
deterministic host-side reverse view for rendering incoming dendrites. This does
not alter `target`, `target_with_cell`, `weight`, or the Rust/WGSL scatter path.

The stored shape is:

- `IncomingSynapse`: every non-self raw incoming record, sorted into the target
  neuron's range, carrying `(source_id, synapse_index, target_id)`, the
  deterministic target socket index/position, and raw signed weight.
- `IncomingRange`: one half-open range per target neuron into the raw incoming
  vector.
- `IncomingSocketGroup`: visible socket groups aggregated by duplicate
  `(source_id, target_id, socket_idx)` with absolute weights summed.
- `incoming_socket_group_ranges`: one range per target neuron into those visible
  groups.

At the default review scale (N=1200/K=16), v1 draws every unique incoming socket
group; it does not sample or silently drop dense targets. If density becomes too
high at a later scale, lower K or add an explicit cap policy before hiding
groups.

**No per-synapse long-range flag.** The heavy-tail reach coin (step 3 above) is
baked into the resulting target id; `target` / `target_with_cell` return only the
target, never a "this synapse is long-range" flag. So when morphology wants to
route a long axon through curved waypoints it classifies "visually long" by
**world distance** (leaf chord vs a cell-size threshold), read-only — it cannot
and does not consult connectivity for a flag. The target/weight rule is unchanged
and waypoints are pure visual route geometry; routing detail lives in
[`manifold.md`](manifold.md).

## Spatial grid

`SpatialGrid` (`crates/brain-visualizer/src/connectivity/spatial.rs`) partitions neuron positions into
a uniform `dim × dim × dim` grid. Cells are addressed by a **packed `u32`
linear id** (`x + y*dim + z*dim*dim`), never by string keys. Neuron
membership is stored CSR-style (`cell_start` offsets into flat `cell_neurons`)
so `neurons_in_cell(id)` returns a contiguous slice with zero allocation.

The grid is built once at startup or tier resize (geometry is static).
`SpatialGrid::cell_of_neuron_map` inverts the CSR layout in O(N) to produce
a per-neuron `cell_of_neuron` buffer suitable for GPU upload; this avoids the
O(N²) scan in `cell_of_index`, which is reserved for off-hot-path host logic.

## Hash primitive

`hash32` is the lowbias32 avalanche variant. `mix_key` decorrelates four inputs
`(seed_lo, neuron_id, synapse_j, salt)` with distinct odd multiplier constants
before the final avalanche, keeping target, weight, and bias draws independent.

The constants are **locked**: all multiplies wrap modulo 2^32 (WGSL `u32`
multiply; Rust `wrapping_mul`); no `u64` appears anywhere. The Rust
implementation in `crates/brain-visualizer/src/connectivity/hash.rs` and the WGSL in
`crates/brain-visualizer/src/sim/gpu/shaders/hash.wgsl` must be byte-identical.

`target` has **two bit-identical implementations** that must move together:
the shared Rust `target_with_cell` and the GPU WGSL `target_neuron`
(`crates/brain-visualizer/src/sim/gpu/shaders/scatter.wgsl`).
The determinism gate `crates/brain-visualizer/tests/wgsl_hash_determinism.rs`
embeds the WGSL source and asserts the GPU output equals the Rust golden vectors;
`crates/brain-visualizer/tests/wgsl_target_determinism.rs` does the same
end-to-end for `target()` and **runs with the long-range tail enabled**
(`LONG_RANGE_FRAC = 64`, `MAX_REACH = 6`) so the contract is proven with the
branch live, self-checking GPU `target_neuron` against the live Rust `target`.
No side (Rust / WGSL) may be edited without updating the other and re-running
the gate.

## Per-tier K and store-once vs regenerate

K is the synaptic out-degree per neuron and is set per tier:

| Tier     | N (typical) | K range   | Edge buffer strategy |
|----------|-------------|-----------|----------------------|
| Low      | ~10k        | 16–32     | store once (CSR)     |
| Balanced | ~100k–1M    | 32–64     | store once           |
| Max      | ~1M+        | 64–128    | regenerate per tick  |

Storing K=64 for 1M neurons requires ~256 MB; 10M would be ~2.5 GB, which
is infeasible in-browser. The max tier regenerates targets by hashing — zero
storage at the cost of more compute. The adaptive scaler may compress or
expand K within a tier alongside N.

## Update when

- `hash32` or `mix_key` constants change (must update both `hash.rs` and
  `hash.wgsl` and re-derive golden vectors).
- `LOCAL_D` or `ANTERIOR_BIAS_NUM / ANTERIOR_BIAS_DEN` change.
- The `target` or `weight` algorithm changes (edit all three of Rust
  `target_with_cell`, WGSL `target_neuron`, and the CPU `ConnParams` path
  together, then recheck `crates/brain-visualizer/tests/wgsl_target_determinism.rs`).
- The heavy-tailed reach rule, `REACH_FRAC_DEN`, the `REACH_COIN`/`REACH_OFFSET`
  salts, or the `ConnectUniforms` `long_range_frac` / `max_reach` fields change
  (Rust `resources.rs` ↔ WGSL `scatter.wgsl` struct ↔ the inline copy in the
  determinism test).
- `SpatialGrid` packing formula changes.
- Per-tier K ranges or the store-once threshold change.
- `target` / `target_with_cell` start returning a per-synapse long-range flag
  (today they return target id only, and morphology classifies by world distance).

## See also

- [`data-model.md`](data-model.md) — fixed-point scale shared with `weight()`;
  `last_spike` type bits that `target()` reads via `source_type`.
- [`simulation.md`](simulation.md) — scatter pass that calls `target`/`weight`
  each tick.
- [`gpu-backend.md`](gpu-backend.md) — GPU scatter shader that inlines the
  hash and spatial grid logic.
- [`../decisions/connectivity.md`](../decisions/connectivity.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
