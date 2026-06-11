---
status:        active
owner:         unassigned
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
---

# Axon trunk + thick axon + root-like curved branches

## Mission

Make each neuron read as a real cell with a single thick primary axon leaving the
soma before it branches, and with branches that curve smoothly like roots rather
than reading as straight chords. Runtime recon showed axons already emitted
sampled cubic-Bezier subsegments; the remaining work was to make the descriptor
trunk an actual protected trunk, keep the first fork displaced from the soma, and
make trunk/twig thickness read clearly. Covers the user's items 1, 2, 3 (they are
one coherent geometry change, not three). This plan consumes the shared
process-root/socket contract so the trunk root direction/thickness is the same
data the soma redesign consumes.

## Scope

In scope — all in `crates/brain-visualizer/src/sim/morphology.rs → generate` (axon
tree build + width rule + segment sampling):

- **Trunk / axon hillock (item 2).** Preserve the already-shipped
  `ProcessRoot` descriptor as the real primary axon process: an initial trunk
  from `soma_root` out to `first_fork` along the dominant target direction. The
  Prim attach loop must not attach leaves directly to `soma_root`, split the
  trunk edge, or relax the descriptor first-fork point. Single-target axons use
  the same source-lit descriptor trunk before their terminal edge.
- **Thick axon (item 3).** The trunk carries full radius and reads visibly thicker
  than downstream branches. Reuse the existing area-preserving (Murray/Rall sqrt)
  width rule, raise the locked root-radius fraction, and force terminal leaves to
  the twig-radius floor so the trunk-to-tip taper is legible even for
  single-target arbors.
- **Smooth tapering curves (item 1).** Keep the existing sampled cubic-Bezier
  axon emission and protect it with host tests; no `MorphSegment` layout or WGSL
  change is required.

Out of scope — soma appearance (its own plan), dendrite wiring (its own plan),
the active-opacity layer, any change to the connectivity `target`/`weight` rule or
the determinism contract. The trunk must not change which targets a neuron reaches.
Out of scope for the first implementation: exposing trunk length as a dev-panel
control. Use a locked default until screenshot review proves a tuning knob is
worth the TS/Rust/config/persistence surface.

## Approach

Single stream, one owner (the three pieces are tightly coupled and share the
generator). Suggested order so each step is independently visible in
`examples/morph_view.rs`:

0. Consume the shared process-root/socket contract: compute or receive the dominant
   trunk direction/radius and first-fork point once, then use those values for
   both branch emission and the soma handoff.
1. Protect the descriptor trunk: root is `ProcessRoot::soma_root`, first fork is
   `ProcessRoot::first_fork`, branches cannot attach to `soma_root`, the
   root→first-fork edge cannot split, and relaxation holds the descriptor fork.
2. Preserve the already-present Bezier axon emission and 48-byte
   `MorphSegment` contract.
3. Width pass: raise the locked trunk-radius lever, keep root/internal widths
   area-preserving, and set terminal leaves to the twig floor.

Keep all hash draws on the existing `salt::TREE_*` namespace so generation stays
seed-reproducible and disjoint from connectivity.

## Shared gates

- The process-root/socket contract plan is resolved first, including the chosen root
  descriptor fields and the combined segment-budget snapshot.
- The implementation artifact reports default N/K segment count, p99/max
  per-neuron segments, cap, and `Morphology::dropped`. Passing this plan alone is
  not enough if the combined axon+soma+dendrite budget would overflow.

## Implementation result

- `params::AXON_R0_FRACTION` is now `0.90`, and the locked default uses that
  constant for `axon_root_radius_fraction`.
- The descriptor trunk is protected in the Prim attach and relaxation passes.
  A host test verifies that each axon arbor with targets has exactly one segment
  leaving `soma_root`, that the source-lit trunk finishes at
  `ProcessRoot::first_fork`, and that real target segments start only after the
  trunk distance.
- Terminal leaf nodes now use `R_trunk * twig_radius_fraction`; a host test
  verifies terminal axon endpoints do not underflow the twig floor and include
  twig-floor endpoints for emitted targets.
- `MorphSegment` stayed 48 B; no WGSL or GPU resource layout changed.
- Default artifact from `cargo run -p brain-visualizer --example morph_view`:
  N=1200, K=16, segment_count=71829, p99=78, max=87,
  cap_per_neuron=166, cap=199200, dropped=0.

## Exit gate

- `cargo test -p brain-visualizer` green (layout asserts, determinism gates
  untouched and still pass).
- `examples/morph_view.rs` artifact ran in this environment and wrote the review
  RGBA frames plus `/tmp/morph_view_stats.json`. The generator-level artifact
  confirms no drops and cap headroom; final visual approval still requires human
  screenshot review of the generated frames.
- No increase in `Morphology::dropped` at default N/K, and the budget artifact
  shows the existing axon subsegments stay under the cap.
- `architecture/manifold.md` "Neuron morphology geometry" section updated to
  describe the trunk and axon curvature; `decisions/manifold.md` gets the
  trunk/curve rationale.

## Decisions made

- **Trunk length starts as a locked generator default**, not a `MorphologyConfig`
  knob. This avoids widening the TS/Rust config and localStorage surface before
  the look is proven. Revisit after screenshot review if the trunk length needs
  interactive tuning.

## Open questions (resolved by the shared contract)

- Does the thicker trunk need a soma-radius-aware floor so it doesn't look pinched
  where it meets the (separately redesigned) soma? **Coordinate with the soma
  plan** — the trunk root direction is also the soma's dominant stretch axis, so
  the two plans share the trunk↔soma junction; agree the root direction/thickness
  hand-off between them.

## See also

- `architecture/manifold.md` — morphology generator + `MorphSegment` contract.
- `architecture/gpu-rendering.md` — tube sub-pass + width rendering.
- `decisions/manifold.md` — area-preserving width rationale.
- `docs/plans/morphology-process-root-contract.md` — shared root/socket and
  budget contract.
