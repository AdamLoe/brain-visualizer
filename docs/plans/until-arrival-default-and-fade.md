---
status:        shipped
owner:         unassigned
last_updated:  2026-06-23
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - architecture/dev-panel.md
  - architecture/web-frontend.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
---

# Until-arrival default + arrival-hold fade-out

## Mission

Two coupled, product-decided changes to morphology connection visibility:

1. Make `connectionLayer` default = **2 ("Until arrival")** instead of 1
   ("Active/recent").
2. Make the until-arrival branch **fade out** over the existing
   `arrivalHoldTicks` window instead of hard-cutting to invisible. Reuse
   `arrivalHoldTicks` AS the fade duration — no new settings index, no new
   uniform *field* (an existing free pad is repurposed). During ages
   `[arrival .. arrival+hold]` the branch ramps brightness AND opacity from its
   subdued rest value down to zero, then drops.

Done = default is 2 for fresh/cleared state, mode-2 branches fade smoothly to
nothing over `arrivalHoldTicks` instead of popping out, all determinism gates
green, and the reversed documentation reflects the new default + fade.

## Scope

**In scope**
- Default flip in `DEFAULT_SETTINGS` (settings.ts) and the matching Rust default
  derive path.
- Render-only fade in `render_morphology.wgsl` for mode-2 (`connection_layer >= 2`).
- Threading `arrival_hold_ticks` into the render uniform via a repurposed free
  pad (no 192 B layout growth) — see "Uniform availability" below.
- Dev-panel tooltip rewrites for "Connections" and "Arrival hold".
- Doc reconciliation (reverses a documented decision — see checklist).

**Out of scope (accepted cuts, do not re-litigate)**
- **No perf mitigation.** Whole-arbor-until-arrival is now the default; we accept
  the cost. No new culling, LOD, or budget work.
- **No forced migration of saved settings.** The default applies to FRESH /
  cleared `localStorage` only. Users with a persisted `connectionLayer` keep it.
- **No new VisualSettings Float32Array index** and **no new uniform struct
  field** — `arrivalHoldTicks` stays the single knob; the render uniform reuses
  an existing pad.
- No change to compaction selection logic (it already keeps mode-2 segments
  selected through `28 + arrival_hold_ticks` — verified).

## Key facts (verified against source)

- **Default + settings.** `app/web/src/core/settings.ts:75` `connectionLayer: 1`;
  comment at :74 documents `2 = Visible until impulse arrival`.
  `arrivalHoldTicks: 30.0` at :84. Metadata `"live"` at
  `app/web/src/core/setting-metadata.ts:48` (`connectionLayer`) and :61
  (`arrivalHoldTicks`). Dev-panel select at `app/web/src/ui/dev-panel.ts:1589-1598`
  (`connectionLayer`, options 0/1/2), slider at :1600-1606 (`arrivalHoldTicks`,
  min 0 / max 180).
- **Rust default mirror.** `app/crates/brain-visualizer/src/sim/gpu/mod.rs:663`
  `arrival_hold_ticks: 30.0` in the defaults struct; the settings packing/clamp
  is at :704/:737/:1912 (clamped `0.0..=300.0`). The Rust-side `connection_layer`
  default is `mod.rs:653` `connection_layer: 1`, with a "(default)" doc comment at
  :601-603 and a test asserting `== 1` at **:2997**. The implementer must flip
  ALL THREE (the `1` at :653, the doc comment at :601-603, and the test
  expectation at :2997) so the fresh Rust default and the TS `DEFAULT_SETTINGS`
  agree. `normalize_connection_layer` (:742) already accepts 0/1/2, so no clamp
  change is needed.
- **Compaction already keeps the segments (no compaction change needed).**
  `compact_morph_segments.wgsl`: `ARRIVAL_MODE_MAX_TRAVEL_TICKS = 28.0` (:90);
  in `compact()` the mode-2 branch (`u.connection_layer >= 2u`, :159-165) selects
  while `age <= lifetime`, `lifetime = 28 + max(arrival_hold_ticks, 0)`. So
  segments stay selected through the entire hold window — the fade is a
  RENDER-ONLY change. **Leave the compaction predicate alone**, so the render
  fade can use exactly `[28 .. 28+hold]` and the segment is still drawn while it
  fades.
- **Render mode-2 today (the hard cut).** `render_morphology.wgsl`:
  - Mode-2 rest brightness is the constant `ARRIVAL_MODE_REST_BRIGHTNESS = 0.11`
    (:172), applied via `tube_resting_brightness(connection_layer, configured)`
    (:439-441), called at :652 in `fs_main` (additive pass) and :688 in
    `fs_main_active`.
  - Opacity floor in mode 2 is forced to 1.0 via
    `visible_selected = select(0.0, 1.0, connection_layer >= 2u)` (:710) →
    `active_alpha = max(mix(floor, ceiling, segment_activity), visible_selected)`
    (:711) in `fs_main_active`. `fs_main` is the additive pass (writes alpha 1.0
    but the additive blend uses brightness, not alpha — fade there = brightness
    only).
  - The branch disappears purely because compaction stops selecting it past
    `28+hold`; render does no ramp.
- **The age signal is already available to render, ungated.** In `vs_main` the
  activity owner is resolved (`activity_id`, :597) and `activity_packed =
  visual_spike[activity_id]` is read (:598) **before** the `spike_enabled` gate.
  The gate (:603-605) only zeroes the *lit* `age`/`glow` outputs. So the shader
  can compute an independent mode-2 fade age from `activity_packed` regardless of
  whether the packet is still "lit". This is the same `visual_spike` word and the
  same `tick_diff` age basis compaction uses (`compact()` :124/:131), so render
  and compaction agree on "age".

## Uniform availability — RESOLVED

**Question:** does `MorphUniforms` (render, 192 B) carry the `arrival_hold_ticks`
value needed to compute the ramp? It currently lives only in `CompactUniforms`.

**Answer: NO, it is not currently in `MorphUniforms` — but it can be added with
ZERO layout growth by repurposing a free pad.** Resolution, with concrete names:

- `MorphUniforms` (`resources.rs:101-145`, byte map at :85-95) has **four
  genuinely-free pad slots, all currently zero-written and read by no entry
  point**: `_pad_a`, `_pad_b`, `_pad_c` at offset 128 (the `color_by` block), and
  `_pad3` at offset 160 (end of the lighting block). The WGSL twin
  (`render_morphology.wgsl:74-76`, :83) declares the same `_pad_a/_pad_b/_pad_c`
  and `_pad3`.
- **Plan: repurpose `_pad_a` → `arrival_hold_ticks: f32`** (offset 128) on BOTH
  sides atomically:
  - Rust `resources.rs`: rename `_pad_a: u32` → `arrival_hold_ticks: f32`; update
    the byte-map doc comment at :91. Leave `_pad_b`, `_pad_c`, `_pad3` as pads.
  - WGSL `render_morphology.wgsl`: rename `_pad_a: u32` → `arrival_hold_ticks: f32`
    in the `MorphUniforms` struct (:74) and update the layout comment block
    (:56-57).
  - Construction site `mod.rs:1948-1981`: replace `_pad_a: 0,` (:1965) with
    `arrival_hold_ticks: self.visual.arrival_hold_ticks.clamp(0.0, 300.0),`.
    `self.visual.arrival_hold_ticks` is already in scope here (it is read into
    the `CompactUniforms` at :1912 in the same method).
- **Size stays 192 B** (a `u32`→`f32` swap in place). The asserts at
  `mod.rs:1817-1818` / `resources.rs` size-asserts stay green. This mirrors the
  already-shipped `_pad4/_pad5 → active_opacity/inactive_opacity_floor` move
  (see `decisions/rendering.md:249-252` and `manifest.md` drift-verification :75),
  so the pattern and its house rule already exist.

**Residual constraint:** the Rust↔WGSL `MorphUniforms` repack must be atomic —
the manifest drift-verification (:75) flags this struct as "must update both
sides atomically". Update the resources.rs byte-map comment, the WGSL struct +
comment, and the construction site in the same change.

## Fade-math spec

All in `render_morphology.wgsl`, render-only. Compute once and apply in BOTH
`fs_main` and `fs_main_active` (the same way `tube_resting_brightness` is already
called in both).

Define the mode-2 fade factor (only meaningful when `u.connection_layer >= 2u`):

```
// Age basis: same word/age compaction uses. Available ungated in vs_main as
// spike_age(u.tick, activity_packed); pass it through a TubeVertOut location
// (e.g. arrival_age) OR recompute in the FS from a passed-through packed word.
// Prefer threading the already-computed age to avoid a second visual_spike read.
hold       = max(u.arrival_hold_ticks, 0.0);
// 28.0 is ARRIVAL_MODE_MAX_TRAVEL_TICKS — mirror the compaction const. Add it as
// a named const in render_morphology.wgsl (it is currently compaction-only) so
// the render fade window matches the selection window exactly.
fade_start = ARRIVAL_MODE_MAX_TRAVEL_TICKS;            // = 28.0
// Guard hold == 0: with hold 0 the segment is dropped by compaction at age 28,
// so there is effectively no fade window; clamp denominator to avoid /0.
denom      = max(hold, 1.0);
arrival_fade = 1.0 - clamp((arrival_age - fade_start) / denom, 0.0, 1.0);
```

Behavior of `arrival_fade`:
- `age <= 28` → `1.0` (full subdued rest value; unchanged from today).
- `28 < age < 28+hold` → ramps `1.0 → 0.0` linearly.
- `age >= 28+hold` → `0.0` (matches the compaction drop point; the segment is no
  longer selected anyway, but a fragment in the boundary frame reads ~0).

Apply it ONLY in mode 2 (multiply by a `connection_layer >= 2` select so modes
0/1 are byte-identical to today):

- **Brightness.** Today: `resting_brightness = tube_resting_brightness(...)` then
  `brightness = resting_brightness + activity * active_boost` (:652-653 /
  :688-689). Multiply the **mode-2 resting term** by `arrival_fade`. Cleanest:
  fold the fade into the resting brightness for mode 2, e.g.
  `resting = select(resting, resting * arrival_fade, u.connection_layer >= 2u)`.
  Leave the `activity * active_boost` term unfaded (a still-traveling packet
  should still punch through) OR fade it too — **decision below.** Recommend
  fading only the resting term: the packet term self-terminates via
  `impulse_*` timing and a passing packet at end-of-life is rare; fading the
  resting term is what removes the pop.
- **Opacity (mode 2 only, `fs_main_active`).** Today the floor is forced to 1.0
  via `visible_selected` (:710-711). Replace the constant `1.0` with
  `arrival_fade` for mode 2:
  `visible_selected = select(0.0, arrival_fade, u.connection_layer >= 2u)` then
  the existing `active_alpha = max(mix(floor, ceiling, segment_activity),
  visible_selected)`. As `arrival_fade → 0`, the floor ramps to 0 and the
  existing `if active_alpha < 0.004 { discard; }` (:713) cleanly drops the
  fragment. `fs_main` (additive) needs no alpha change — its fade is the
  brightness multiply only.
- **hold == 0 edge.** `denom = max(hold, 1.0)` prevents /0; with hold 0
  compaction drops at age 28 so the visible result is the existing hard behavior
  with no regression.

Do NOT touch `fs_sphere*` — somas are per-neuron and out of mode-2 scope.

## Approach (ordered steps)

Single implementer; the slices serialize because the shader repack and the Rust
struct must land together.

1. **Uniform repack (atomic).** `resources.rs`: `_pad_a: u32` →
   `arrival_hold_ticks: f32` + byte-map comment. `render_morphology.wgsl`:
   `_pad_a: u32` → `arrival_hold_ticks: f32` + layout comment. `mod.rs:1965`:
   set it from `self.visual.arrival_hold_ticks.clamp(0.0, 300.0)`.
2. **Render fade.** Add `ARRIVAL_MODE_MAX_TRAVEL_TICKS = 28.0` const to
   `render_morphology.wgsl` (mirror of compaction). Thread the ungated
   `arrival_age` (= `spike_age(u.tick, activity_packed)`) from `vs_main` through a
   new `TubeVertOut` location into the FS. Compute `arrival_fade` per the spec.
   Apply to mode-2 resting brightness (both `fs_main`/`fs_main_active`) and to
   `visible_selected` (in `fs_main_active`). Mode 0/1 paths unchanged.
3. **Default flip.** `settings.ts:75` `connectionLayer: 2`; update the inline
   comment at :74. Flip the Rust-side default: `mod.rs:653` `connection_layer: 2`,
   the doc comment at :601-603, and the test expectation at :2997 (`== 2`).
   Confirm fresh-state only (no migration shim; persisted values win — verify the
   load/merge path does not overwrite a saved value).
4. **Tooltips.** `dev-panel.ts:1592` "Connections" tooltip: state Until-arrival
   is now the default and describes the whole-branch-until-arrival behavior.
   `dev-panel.ts:1603` "Arrival hold" tooltip: now describes the **fade-out
   duration** (subdued branch ramps to invisible over these ticks after the
   aggregate arrival point), not "extra ticks kept visible".
5. **Doc migration** (checklist below).
6. **Run gates** (below). All determinism gates must stay green.

## Doc-migration checklist

Per `docs/_meta/manifest.md` change-to-doc table (:48, :55, :58) and the reversed
decision:

- **`decisions/rendering.md:281` ("Default connection visibility selects the
  packet band, not the whole fired arbor") — REWRITE.** This entry asserts the
  old default. Rewrite it so until-arrival (whole fired arbor until packet
  arrival, then a `arrivalHoldTicks` fade-out) is the default; state the accepted
  whole-arbor cost tradeoff (no perf mitigation) and that active/recent (packet
  band) is now the opt-in mode. Update the entry title accordingly.
- **`decisions/rendering.md:254-264` ("Connection visibility modes reuse
  GPU-indirect segment selection") — REVISE the mode-2 description.** Add that
  until-arrival now fades the branch out over `arrivalHoldTicks` (render-only
  brightness+opacity ramp over `[28 .. 28+hold]`) rather than hard-cutting at the
  compaction drop point. Note compaction selection is unchanged.
- **`architecture/gpu-rendering.md`** — shader change: document the mode-2 fade
  (render-only ramp) and the `arrival_hold_ticks` field now living in
  `MorphUniforms` (repurposed `_pad_a`). Update the `MorphUniforms` field list.
- **`decisions/rendering.md:249-252`** — extend the "repurposed pad" decision to
  note `_pad_a → arrival_hold_ticks` alongside the existing `_pad4/_pad5` entry,
  or add a sibling bullet.
- **`docs/_meta/manifest.md` drift-verification :75** — update the `MorphUniforms`
  192 B note to list `arrival_hold_ticks` among the pad-repurposed fields (still
  192 B).
- **`architecture/dev-panel.md` + `decisions/dev-tooling.md`** — settings-index
  change: the `connectionLayer` default is now 2; the "Arrival hold" tooltip
  semantics changed to fade duration.
- **`architecture/web-frontend.md`** — note the `DEFAULT_SETTINGS`
  `connectionLayer` default flip (fresh-state only; persisted values unaffected).

## Exit gate

Implementer's verification gates (run by the implementer, not the planner). No
WebGPU adapter on this box, so visual proof is the **llvmpipe** path, not a
browser.

- `cargo test` (host unit + determinism + `gpu_sim_dynamics` under llvmpipe) —
  the 192 B `MorphUniforms` size asserts and all determinism gates must be green.
- `npm run typecheck`.
- `npm test`.
- **Visual proof (cheapest sufficient):** the llvmpipe
  `gpu_sim_dynamics` / examples render path — confirm a mode-2 branch ramps to
  nothing over `arrivalHoldTicks` rather than popping. If an existing example
  already captures morphology frames, reuse it; do not stand up new browser
  tooling.

## Open decisions that survive intake

1. **Fade the packet/active term too, or only the resting term?** Recommend
   fading **only the mode-2 resting term** (the source of the pop); leave
   `activity * active_boost` unfaded since a packet still in flight should read.
   The implementer may fade both if a still-traveling end-of-life packet looks
   wrong under llvmpipe — but resting-only is the default and the simpler change.
2. **Which pad to repurpose.** Recommend `_pad_a` (offset 128). Any of
   `_pad_a/_pad_b/_pad_c/_pad3` works; `_pad_a` keeps the rename local to the
   `color_by` block. Not load-bearing — pick one and update both sides atomically.

## Migration notes (shipped)

All durable context migrated; this plan is `okay_to_delete: true`.

- Mode-2 render fade + the `[28 .. 28+hold]` window → `architecture/gpu-rendering.md`
  (visible-until-arrival paragraph) and `decisions/rendering.md` ("Connection
  visibility modes reuse GPU-indirect segment selection" mode-2 revision).
- `arrival_hold_ticks` now in `MorphUniforms` (repurposed `_pad_a`, still 192 B)
  → `architecture/gpu-rendering.md` (Active Opacity layout note),
  `decisions/rendering.md` ("Active-layer coverage knobs..." pad decision extended),
  and the `manifest.md` drift-verification `MorphUniforms` note.
- Default = 2 (fresh-state only) → `decisions/rendering.md` ("Default connection
  visibility is until-arrival..." entry, rewritten from the old packet-band
  default), `architecture/web-frontend.md` (DEFAULT_SETTINGS note),
  `architecture/dev-panel.md`, and `decisions/dev-tooling.md` (new entry).
- Tooltip semantics for Connections / Arrival hold → `architecture/dev-panel.md`
  and `decisions/dev-tooling.md`.

Implementation deltas vs. the plan as written:
- The Rust default-flip test was the new `connection_layer == 2` assertion added
  to `visual_settings_default_matches_product_defaults` (mod.rs), not an edit at
  the cited `:2997` — that line is the index-mapping test (`from_slice` value 18 →
  `normalize_connection_layer` → 1) and must stay `== 1`.
- Fade math is GPU-only; visual proof is the deterministic CPU mirror test
  `crates/brain-visualizer/tests/arrival_fade_factor.rs` (pins the ramp shape and
  the hold==0 guard) plus the shader compiling/running clean under llvmpipe
  (`gpu_sim_dynamics`). An aggregate-luminance render sweep was inconclusive
  because the live recurrent population has no single global spike age.
- Open decision #1: faded **only** the mode-2 resting term, left the packet/active
  term unfaded. Open decision #2: repurposed `_pad_a` (offset 128).

## See also

- `docs/plans/index.md` — where live plans land.
- `~/.agentdocs/plan-lifecycle.md` — status metadata + ship-time migration.
- `decisions/rendering.md`, `architecture/gpu-rendering.md` — owning docs.
