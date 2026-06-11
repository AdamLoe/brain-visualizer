---
status:        active
owner:         orchestrator
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - decisions/rendering.md
  - architecture/dev-panel.md
---

# Active-opacity continuous model

> User goal 2: "Active opacity at 0 makes it super bright. Active brightness
> at any other level makes it all mostly see-through and the slider does
> nothing — just 0 or not 0." **Supersedes**
> [`active-opacity-controls-and-solid-firing.md`](active-opacity-controls-and-solid-firing.md),
> which shipped the controls + a segment-overlap alpha gate but produced
> exactly the two pathologies described here.

## Mission

The active-opacity layer is supposed to let firing geometry read solid while
resting structure stays soft, with a slider that smoothly trades between them.
Instead the slider behaves binary and `active_opacity = 0` blows the whole
scene out bright. Both come from the same shipped design: a **boolean**
per-segment alpha gate on tubes, plus a **boot guard that skips the entire
active pass** when `active_opacity == 0`. Done when: the active-opacity slider
varies opacity *continuously* from "resting-soft" to "firing-solid" across its
whole range, `active_opacity = 0` produces the *least* emphasis (not a
blowout), and a traveling impulse still reads as a moving bright packet through
an opaque segment rather than flashing the whole arbor.

## Grounding — original root causes before this wave

**Cause A — binary tube alpha (slider felt like 0-or-not-0).** Before this wave,
in
[render_morphology.wgsl](../../app/crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl)
`fs_main_active` gated tube alpha with a boolean:

```wgsl
let segment_active = impulse_overlaps_segment(in.segment_start, in.segment_end,
                                              in.spike_age, in.glow, in.kind);
// Packet-overlap drives opacity from the inactive floor to the active ceiling.
let active_alpha = select(u.inactive_opacity_floor, u.active_opacity, segment_active);
```

`impulse_overlaps_segment` returned a **bool** (interval intersection of the
traveling packet against the segment's path-length interval). So a segment was
either fully at `active_opacity` or fully at `inactive_opacity_floor` (default
0.0 → discarded). There was no gradient: most of the tree rested → discarded
→ "mostly see-through"; only the thin overlapped band rendered, and at any
non-zero `active_opacity` it jumped from invisible to that fixed value. The soma
shader already used a continuous floor/ceiling model. Tubes needed the same
continuous treatment.

**Cause B — dropping the active pass blew out brightness at 0.** Before this
wave, [mod.rs](../../app/crates/brain-visualizer/src/sim/gpu/mod.rs) tied the
active redraw guard to a positive requested opacity. At the zero end, the
depth-tested active pass (tubes + somas) was not encoded. That pass owns the
depth clear; without it, the additive resting passes (which use no depth test,
`LoadOp::Load`) composited **unoccluded** and accumulated — the scene read
"super bright." The slider's 0 end therefore did the opposite of what a user
expected (it should be the *dimmest/least-solid* end, not a blowout).

Draw order today (mod.rs ~1355–1549): resting tubes (additive, no depth) →
active tubes (alpha blend, depth clear+write) → resting somas (additive, no
depth) → active somas (alpha blend, depth load). The active pass is the only
depth-tested occluder.

## Scope

In scope:

1. **Continuous tube alpha.** Replace the boolean `select()` in `fs_main_active`
   with a continuous factor (mirror the soma `mix()`): drive alpha from a smooth
   per-fragment activity/packet-proximity term so `active_opacity` reads as a
   real ceiling and intermediate slider values are visible. Keep brightness
   fragment-local so the impulse still *travels* — i.e. opacity may be
   whole-segment-ish while the bright slice moves, but the slider must not be
   binary.
2. **Make 0 behave as "least emphasis."** Decouple the active-pass-enabled
   decision from `active_opacity == 0`, or guarantee depth occlusion exists
   regardless, so turning active opacity down does not remove the occluder and
   blow out the additive layer. Define what `active_opacity = 0` *should* mean
   (likely: firing geometry no more opaque than resting) and make the pipeline
   honour it.
3. **Re-validate `inactive_opacity_floor`** against the new model so the two
   knobs compose sensibly across the full range.

Out of scope: the controls/persistence plumbing (already shipped; its boot-push
bug is handled in [`dev-panel-and-settings-overhaul.md`](dev-panel-and-settings-overhaul.md)),
soma geometry, dendrite geometry.

## Implementation status — 2026-06-09

Code is done and durable docs have been migrated, but the plan remains active
because its visual acceptance frames are still missing. Boole changed
`render_morphology.wgsl`, `sim/gpu/mod.rs`, and `examples/render_check.rs`.
Current behavior: tube active alpha uses continuous segment proximity from
`inactive_opacity_floor` to an active ceiling; brightness remains fragment-local
so the packet travels; `active_opacity = 0` maps to a soft low-emphasis ceiling
of `0.10` and still encodes the active redraw; somas share that low-end ceiling.
Reported gates: `cargo run -p brain-visualizer --example render_check` passed
and `cargo test -p brain-visualizer` passed.

## Approach

One stream — it's a shader + pass-guard change owned by
`render_morphology.wgsl` and the active-pass encoding in `mod.rs`. Recommended
order:

1. Reproduce both pathologies in `examples/morph_view.rs` /
   `examples/render_check.rs` (a frame at `active_opacity` ∈ {0.0, 0.5, 1.0})
   so the fix is measurable, not just eyeballed.
2. Fix Cause B first (the blowout) — it's a one-line guard/encoding decision and
   makes the slider's low end sane.
3. Fix Cause A (continuous tube alpha) — port the soma `mix()` approach to tubes,
   preserving traveling-packet brightness.
4. Tune `inactive_opacity_floor` interaction; confirm resting structure still
   honours `morphRestingOpacity` (the resting passes, separate from this layer).

## Exit gate

- Dragging the active-opacity slider 0→1 produces a *visibly continuous* change
  in firing-geometry opacity (capture frames at 0.0 / 0.25 / 0.5 / 0.75 / 1.0
  via `examples/morph_view.rs`); no 0-or-not-0 cliff.
- `active_opacity = 0` is the least-emphasis end and does **not** brighten the
  scene relative to a small positive value (no blowout).
- A traveling impulse still reads as a moving bright packet along an opaque
  segment, not a whole-arbor flash.
- `examples/render_check.rs` active-opacity assertion updated to the new model
  and passing; `cd app && cargo test -p brain-visualizer` green.
- `architecture/gpu-rendering.md` (Active-opacity layer / `fs_main_active`
  alpha model, active-pass guard, draw order) and `decisions/rendering.md`
  updated; `architecture/dev-panel.md` lighting-control description matches.

## Migration notes (filled in at ship time)

Route the new alpha model and the active-pass-guard decision into
`architecture/gpu-rendering.md` and `decisions/rendering.md`. Confirm the old
[`active-opacity-controls-and-solid-firing.md`](active-opacity-controls-and-solid-firing.md)
is closed out (its durable, still-true facts — the two knobs exist and persist
— migrate; its alpha-model claims are replaced by this plan).

## See also

- [`active-opacity-controls-and-solid-firing.md`](active-opacity-controls-and-solid-firing.md) — superseded predecessor.
- [`dev-panel-and-settings-overhaul.md`](dev-panel-and-settings-overhaul.md) — owns the controls/boot-push side.
- `architecture/gpu-rendering.md`, `decisions/rendering.md` — owning docs.
</content>
