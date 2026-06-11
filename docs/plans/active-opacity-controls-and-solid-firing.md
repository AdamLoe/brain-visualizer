---
status:        shipped
owner:         unassigned
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - architecture/gpu-rendering.md
  - decisions/rendering.md
---

# Active-opacity controls + solid firing segments

> Simple implementer plan. Covers user items 5 ("firing segments still
> see-through / no active opacity slider") and 6 ("inactive opacity not
> saved/loaded") — they share one root cause.

## Mission

The active-opacity layer's two knobs — `active_opacity` (ceiling) and
`inactive_opacity_floor` — already exist in Rust (`LightingConfig`, defaults
1.0 / 0.0) and feed the active uniform, but they were **never plumbed to the TS
side**: not in `morph-config.ts` descriptors, not in the dev panel, not persisted.
That is why there is no active-opacity slider (item 5) and why the inactive/active
floor doesn't survive a reload (item 6). Separately, even at `active_opacity = 1`,
firing segments don't read solid because alpha tracks the **per-fragment traveling
packet** — only the bright slice goes opaque. Done when: both knobs are
dev-panel-controllable and persisted, and a segment whose path interval is
currently overlapped by the traveling packet reads opaque along its **whole
length** while brightness still travels through it.

## Scope

Two independent pieces:

**Piece 1 — plumb + persist the two knobs (the bug).**
- Add `activeOpacity` and `inactiveOpacityFloor` to the TS `LightingConfig` shape
  and `MORPH_DESCRIPTORS` in `web/src/core/morph-config.ts` (lighting group,
  `live` impact, uniform apply — mirrors `restingBrightness` / `activeBoost`).
- They persist automatically under the existing `bv2_morph_v1` key via the
  merge-over-defaults path; confirm load round-trips. Bump the `bv2_morph_v1`
  version sentinel if defaults are considered to change.
- Verify the values reach `set_morphology_config` → the `MorphUniforms`
  `active_opacity` / `inactive_opacity_floor` fields (uniform-only, no layout
  change — they're the repurposed trailing slots).

**Piece 2 — whole-segment solidity (the behavior).**
- In `render_morphology.wgsl → fs_main_active`, the active alpha currently mixes by
  the per-fragment traveling-packet `activity`. Change the alpha gate to a
  per-segment overlap test: if the source-owned packet center/window intersects
  `[seg.path_len, seg.path_len + length(seg.b - seg.a)]`, the **whole segment**
  uses `active_opacity`; otherwise it falls to `inactive_opacity_floor` and the
  existing discard threshold. Keep brightness/material lighting driven by the
  fragment-local packet, so the visible impulse still moves through an opaque
  segment instead of flashing the whole arbor at once.
- Do **not** key alpha merely off "source neuron fired recently"; that would make
  every source-owned segment solid at once and erase propagation. The intended
  behavior is segment-solid, not arbor-solid.

Out of scope: `morphRestingOpacity` (Float32Array index 15 — already persisted
correctly; not this bug), the soma look, branch geometry.

## Approach

Piece 1 is mechanical TS plumbing + a persistence check. Piece 2 is a small WGSL
change to the active fragment alpha model. They can land together or as two
commits. **Also fix the stale docs**: `architecture/gpu-rendering.md` and
`architecture/dev-panel.md` currently claim these two knobs are dev-panel
`LightingConfig` controls — they describe the intended state, not the shipped one;
after this plan that becomes true.

## Implementation status

2026-06-09: Rawls implemented the TS descriptors/defaults for
`lighting.activeOpacity` and `lighting.inactiveOpacityFloor`, changed
`fs_main_active` alpha to use a segment interval overlap test while keeping
brightness fragment-local, and updated `architecture/dev-panel.md` plus
`architecture/gpu-rendering.md`.

Observed gates:

- `cd app/web && npm run typecheck` passed.
- `cd app && cargo test -p brain-visualizer` passed, including the WGSL
  determinism tests and doc tests.

Remaining before ship: final visual/browser artifact review with the rest of the
morphology refresh.

## Closure — 2026-06-11

Shipped as the controls/persistence foundation. Its original segment-solid alpha
model was superseded by
[`active-opacity-continuous-model.md`](active-opacity-continuous-model.md), which
is now shipped and documents the current continuous-opacity behavior. The
still-true facts from this plan are migrated into the dev-panel and rendering
docs.

## Exit gate

- Active-opacity and inactive-floor sliders appear in the dev panel Rendering →
  Morphology lighting group, take effect live, and survive a reload (`bv2_morph_v1`
  round-trip).
- At default settings, axon segments read **solid** along their length only while
  the traveling impulse overlaps that segment; resting structure stays
  soft/additive, and non-overlapped downstream segments do not flash solid early.
- `examples/render_check.rs` active-opacity assertion still passes; `cargo test`
  green.
- `architecture/dev-panel.md` (Morphology config controls / Settings persistence)
  and `architecture/gpu-rendering.md` (Active-opacity layer) updated to match
  reality.

## See also

- `architecture/gpu-rendering.md` — Active-opacity layer, `fs_main_active` alpha model.
- `architecture/dev-panel.md` — `MORPH_DESCRIPTORS`, `bv2_morph_v1` persistence.
- `crates/brain-visualizer/src/sim/morphology.rs → LightingConfig` — the two fields.
