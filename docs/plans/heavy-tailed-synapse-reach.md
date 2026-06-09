---
status:        draft
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/connectivity.md
  - decisions/connectivity.md
---

# Heavy-tailed synapse reach

## Mission

Give the network a controllable amount of **long-range connectivity**: keep most
synapses local (the current behaviour) but route a tunable fraction to
genuinely distant neurons, so signal visibly jumps across the cortex instead of
only diffusing locally. Done when a dev-panel knob varies the long-range
fraction, the Rust and WGSL paths stay bit-identical, and
`architecture/connectivity.md` + `decisions/connectivity.md` document the rule.

## Scope

**In scope** — the `target(i, j, ...)` reach logic in
`crates/brain-visualizer/src/connectivity/mod.rs` and its WGSL twin in
`crates/brain-visualizer/src/sim/gpu/shaders/hash.wgsl` (or wherever `target` is
inlined on the GPU scatter path):

- A heavy-tailed reach: most synapses stay within the current `LOCAL_D`
  Chebyshev neighbourhood; a tunable fraction draw a much larger offset (up to a
  max-reach bound).
- The knob(s): a **long-range fraction** plus a **max-reach** bound.

**Out of scope** — the feed-forward Z-bias semantics (preserve them), the
fixed-point weight scale, the scatter pass itself, and morphology (a separate
plan consumes the longer targets visually).

## Locked design decisions

- **Heavy-tailed, not a uniform stretch.** Keep local cluster texture; add a
  long-range *tail*. A single global multiplier on `LOCAL_D` (uniform stretch)
  was considered and rejected — it washes out local density.
- **Selection stays integer + hash-driven.** The local-vs-long-range coin flip
  and the long-range offset are deterministic hashes (`mix_key`/`hash32` with a
  new dedicated `salt`), keeping the "no float distance comparison" invariant.

## Approach

1. Add a new salt and a per-synapse hash draw that decides local vs long-range
   by comparing against the fraction knob.
2. Long-range branch: draw a larger integer cell offset (bounded by max-reach),
   reusing `nearest_occupied` for empty-cell fallback and the in-cell pick.
3. Thread the knob(s) from config → backend. Default exposure: a connectivity
   parameter surfaced in the hidden dev panel; confirm with owner if a tier
   preset is preferred instead.
4. **Re-derive the determinism golden vectors.** Update both
   `crates/brain-visualizer/tests/wgsl_target_determinism.rs` and
   `wgsl_hash_determinism.rs` if the hash surface changes; Rust and WGSL must be
   edited together and stay byte-identical.

## Exit gate

- `cargo test` green, **including the regenerated** `wgsl_target_determinism.rs`
  golden vectors (Rust ↔ WGSL bit-identical with the long-range tail enabled).
- Visual check: with the fraction raised, long axons span the cortex and signal
  visibly jumps; with it at 0, output is bit-identical to today.
- `architecture/connectivity.md` (target computation) and
  `decisions/connectivity.md` document the heavy-tailed rule and the knobs.

## Discipline rules

The Rust and WGSL hash/target implementations are a **locked bit-identical
contract**. Never edit one without the other and without re-deriving the golden
vectors. This is the single highest-risk part of this plan.

## Migration notes (filled in at ship time)

Route the reach rule and knob semantics into `architecture/connectivity.md`;
route the heavy-tailed-vs-uniform-stretch trade-off into
`decisions/connectivity.md`.

## Implementation detail

This section is the concrete, code-grounded execution plan. Symbols below are
real and were read at planning time; line numbers are intentionally omitted (use
the symbol names).

### The reach rule (the locked algorithm both languages implement)

Insert the long-range branch **after** the existing local offset decode and the
anterior-bias step, but **before** `clamp_cell` — in both Rust
`target_with_cell` (`crates/brain-visualizer/src/connectivity/mod.rs`) and WGSL
`target_neuron` (`shaders/scatter.wgsl`). The local `(dx, dy, dz)` and the
anterior-bias logic are unchanged; the long-range branch *replaces* the offset
when the coin flips long.

1. Coin flip — new salt `salt::REACH_COIN`. Draw
   `coin = mix_key(seed, i, j, REACH_COIN) % REACH_FRAC_DEN` and compare against
   the per-run `long_range_frac` knob (expressed in the same `/REACH_FRAC_DEN`
   units — see "knob encoding"). `coin < long_range_frac` ⇒ this synapse is
   long-range. Keep the fraction denominator a fixed power-of-two-ish integer
   constant (e.g. `REACH_FRAC_DEN = 256`) so the knob is a pure integer compare
   with no float on the path.
2. Long-range offset — new salt `salt::REACH_OFFSET`. One fresh
   `h2 = mix_key(seed, i, j, REACH_OFFSET)`. Decode three signed components, each
   in `[-max_reach, +max_reach]`, with a *new* helper
   `long_offset_component(bits, max_reach)` =
   `(bits % (2*max_reach + 1)) - max_reach`. Slice `h2` into three independent
   fields exactly like the local path slices `h` (10 bits each:
   `h2 & 0x3ff`, `(h2>>10)&0x3ff`, `(h2>>20)&0x3ff`). When long, **overwrite**
   `dx, dy, dz` with these wider components. Apply the anterior bias the same way
   afterward is NOT what we want — to preserve the locked feed-forward Z-bias
   semantics, run the existing bias step on its current branch only. Decision:
   keep the anterior-bias block exactly where it is (operating on the local
   `dz`), and gate the long-range overwrite so that when a synapse goes
   long-range it overrides the already-biased `dz` too. This keeps "most synapses
   local + biased; a tail jumps far" and the bias remains untouched for local
   synapses. (The owner's Z-bias preservation requirement is about not changing
   the *local* posterior→anterior flow, which this satisfies.)
3. Everything downstream is reused verbatim: `clamp_cell` bounds the (now larger)
   delta to the grid, `nearest_occupied` handles the empty-cell spiral, and the
   `IN_CELL_PICK` hash chooses the occupant. No new fallback code.

**Default-off bit-identical guarantee.** When `long_range_frac == 0` the coin
compare `coin < 0` is always false, so the long-range branch never executes and
the draws for `REACH_COIN`/`REACH_OFFSET` never alter output — but note the coin
hash is still *computed* every call. That is fine: it does not change any target.
Output is byte-for-byte today's output at `frac = 0`, satisfying the exit gate's
"bit-identical to today" clause. (Confirmed: the only writes that affect a target
are gated behind `coin < long_range_frac`.)

### Knob encoding (avoid floats on the determinism path)

The two knobs reach the kernel as integers in the existing uniform/params:

- `long_range_frac: u32` — numerator over `REACH_FRAC_DEN` (256). UI exposes a
  0..1 float; convert to `round(frac01 * 256)` clamped to `0..=256` *at the
  boundary* (TS→settings or Rust `set_visual_settings`), never inside `target`.
- `max_reach: u32` — integer cell radius, `>= 1`. Clamp/validate at the boundary;
  the kernel trusts it. A sane UI range is 2..(grid_dim-1); default e.g. 6.

Keeping both integers means `target`/`target_neuron` stay 100% integer and the
WGSL twin is a literal transcription — the locked "no float distance comparison"
invariant holds.

### Salt additions (Rust + WGSL must match)

In `connectivity::mod.rs` `mod salt` add two distinct odd values continuing the
sequence:

```
pub const REACH_COIN: u32   = 0x0000_0005;
pub const REACH_OFFSET: u32 = 0x0000_0006;
```

Mirror them in `scatter.wgsl` as `SALT_REACH_COIN`/`SALT_REACH_OFFSET` next to
the existing `SALT_*` consts. These are new *uses* of the existing `mix_key`; the
`hash32`/`mix_key` primitives themselves and their golden vectors in
`hash.wgsl`/`hash.rs`/`wgsl_hash_determinism.rs` are **untouched** (so
`wgsl_hash_determinism.rs` does NOT need regeneration — its GOLDEN vectors test
the primitive, not `target`).

### Function-signature change (one new Copy param, threaded)

Add `#[derive(Clone, Copy)] pub struct ReachParams { pub long_range_frac: u32,
pub max_reach: u32 }` to `connectivity::mod.rs`, with
`ReachParams::LOCAL_ONLY` (`{ long_range_frac: 0, max_reach: 1 }`). Add it as the
final parameter to both `target` and `target_with_cell`. Edit order to keep the
tree compiling and the contract intact:

1. `connectivity/mod.rs`: add salts, `ReachParams`, the helper, the branch; add
   the param to `target`/`target_with_cell`; pass `ReachParams::LOCAL_ONLY` in
   the in-module `#[cfg(test)]` callers (target_is_deterministic, target_in_range,
   seed_changes_targets, anterior_bias_present_for_excitatory) so existing tests
   keep asserting today's behaviour.
2. `scatter.wgsl`: add the two salts, `long_offset_component`, and the branch in
   `target_neuron`, reading `cu.long_range_frac` / `cu.max_reach`. Transcribe the
   Rust branch line-for-line.
3. `ConnectUniforms` (Rust `resources.rs` *and* WGSL `scatter.wgsl` struct):
   repurpose two of the three `_pad` u32 slots into `long_range_frac` and
   `max_reach`; keep `_pad: [u32; 1]`. **Size stays 32 B** — no realignment, the
   `uniform_sizes_aligned` test still passes. Update both struct definitions
   atomically (manifest "high-risk surfaces" rule). Wire the two new fields where
   `ConnectUniforms` is constructed in `resources.rs` (init) from
   `config`/`visual`, and re-write the `connect_uniform` buffer in
   `set_visual_settings` (see "runtime wiring").
4. CPU path: add `long_range_frac`/`max_reach` to `ConnParams`
   (`cpu/core.rs`), populate from `self.config`/settings in `CpuBackend::tick`
   (`cpu/mod.rs`), pass through `scatter_map`/`scatter_one_source` into
   `target_with_cell`. This keeps the CPU↔GPU networks identical at any knob
   value (the determinism contract is broader than the WGSL gate — `cpu_check.rs`
   compares CPU vs shared Rust `target`).
5. Morphology: `sim/morphology.rs` calls `target_with_cell` to build axon arbors.
   Thread the same `ReachParams` from the `GeneratorConfig`/morph-params path so
   long axons render. Update the three morphology `#[cfg(test)]` callers and the
   `manifold/mod.rs` determinism test + `examples/cpu_check.rs` to pass the param
   (use `ReachParams::LOCAL_ONLY` where they assert today's local behaviour).

### Runtime wiring (dev-panel knob → all three consumers)

Follow the existing `connection_curve_lift` pattern (it is also a
generation-time knob that triggers `regenerate_morphology()` on change):

- **TS contract** (`web/src/core/settings.ts`): add `longRangeReachFrac` (index
  24, float 0..1) and `maxReachCells` (index 25, integer) to
  `VisualizerSettings`, `DEFAULT_SETTINGS` (frac `0.0` so default = today),
  `SavedDev` schema + merge, `toFloat32Array` (a[24]/a[25]), and bump
  `SETTINGS_LENGTH` to 26. `from_slice` on the Rust side is length-tolerant so it
  reads the new indices with safe defaults.
- **Rust `VisualSettings`** (`sim/gpu/mod.rs`): add `long_range_reach_frac: f32`
  (idx 24) and `max_reach_cells: f32` (idx 25, integer carried as f32 like the
  other mode fields) to the struct, `Default`, `from_slice` (`f(24,..)`,
  `f(25,..)`), and `to_json`.
- **`set_visual_settings`** (`sim/gpu/mod.rs`): convert the float frac to the
  integer `long_range_frac` (`round(frac*256)` clamped 0..=256) and
  `max_reach` (`round`+clamp `>=1`); re-write the `connect_uniform` buffer with
  the new values via `queue.write_buffer` so the sim picks them up next tick
  (the buffer is currently written once at init — add a small writer here, or
  mark `connect_uniform_dirty` and flush in `tick`). Detect a change vs the
  stored `self.visual` and call `regenerate_morphology()` (mirror `curve_changed`)
  so the axon geometry reflects the new reach. Both knobs are **brain-reset /
  morphology-rebuild** impact, NOT pure "live", because they change target ids and
  thus generated geometry — set them to `"brain-reset"` in
  `web/src/core/setting-metadata.ts` (matching how a structural change is
  classified), and add two `_sliderRow` controls in `web/src/ui/dev-panel.ts`
  (frac: min 0 max 1 step 0.01; reach: min 2 max e.g. 16 step 1).

### Determinism gate — regenerate / re-verify

`wgsl_target_determinism.rs` is the gate that must reflect the new rule. It
constructs `ConnectUniforms` *inline* (its own copy of the struct, currently
`n,k,fixed_point_scale,seed_lo,grid_dim,_pad[3]`). Edits:

1. Update the test's local `ConnectUniforms` to the new field layout
   (`long_range_frac`, `max_reach`, `_pad[1]`) and set non-trivial values
   (e.g. `long_range_frac = 64` ⇒ 25%, `max_reach = 6`) so the gate exercises the
   long-range tail, not just the local path.
2. Update the Rust reference call `target(i, j, grid, K, SEED, st)` to pass a
   matching `ReachParams { long_range_frac: 64, max_reach: 6 }`. The test then
   proves WGSL `target_neuron` (reading the uniform) equals Rust `target` *with
   the tail enabled* — exactly the exit-gate requirement. No separate "golden
   vector" file to regenerate for `target`; this end-to-end test *is* the golden
   gate, and it self-checks against live Rust output.
3. `wgsl_hash_determinism.rs` is **not** edited (primitives unchanged).

Run order for the gates (cargo runs from `app/`):

```
cd /home/adamg/brain_visualizer/app
cargo test -p brain-visualizer connectivity            # unit: branch + default-off
cargo test -p brain-visualizer --test wgsl_target_determinism   # the regenerated gate
cargo test -p brain-visualizer --test wgsl_hash_determinism     # must still pass untouched
cargo test                                              # full host suite incl. morphology determinism
cargo run --example cpu_check                           # CPU vs shared Rust target parity
```

Then the web contract:

```
cd /home/adamg/brain_visualizer/app/web
npm run typecheck && npm test
```

### Edit-order summary (smallest compiling steps)

1. `connectivity/mod.rs` — salts, `ReachParams`, helper, branch, signature, fix
   in-module tests.
2. `scatter.wgsl` — salts, helper, branch, `ConnectUniforms` struct fields.
3. `resources.rs` — `ConnectUniforms` Rust struct + init population.
4. `cpu/core.rs` + `cpu/mod.rs` — `ConnParams` + threading.
5. `morphology.rs` + `manifold/mod.rs` test + `examples/cpu_check.rs` — thread
   `ReachParams`, fix callers/tests.
6. `sim/gpu/mod.rs` — `VisualSettings` fields, `set_visual_settings` (uniform
   re-write + morph regen).
7. TS: `settings.ts`, `setting-metadata.ts`, `dev-panel.ts`.
8. `wgsl_target_determinism.rs` — enable the tail in the gate.
9. Docs: `architecture/connectivity.md` + `decisions/connectivity.md` per the
   plan's migration notes.

## See also

- The app's [`index.md`](index.md) — where live plans land.
- [`../architecture/connectivity.md`](../architecture/connectivity.md) — current `target`/`weight` rule and the determinism gates.
- [`../decisions/connectivity.md`](../decisions/connectivity.md).
- [`morphology-branching-tree.md`](morphology-branching-tree.md) — coupled: longer targets become long terminal axons that generator must route.
