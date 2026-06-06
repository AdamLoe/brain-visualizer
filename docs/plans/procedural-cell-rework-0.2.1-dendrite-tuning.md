---
status:        draft
owner:         adamg
last_updated:  2026-06-06
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - architecture/dev-panel.md
  - architecture/profiling.md
  - decisions/manifold.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
  - decisions/profiling.md
---

# v0.2.1 - Dendrite Readability & Morphology Tuning

## Mission

Polish the v0.2.0 arbor so it looks intentional at both homepage distance and
close fly-in distance. This is a patch release: no new render contract, no new
connectivity behavior, and no second settings architecture. Tune through the
v0.2.0 morphology config/dev-panel surface first; add a new parameter only if
the accepted issue list cannot be expressed by the existing one. Done = the
arbor feels continuously wired, cells read cleanly near camera, the final
settings/ranges still match the accepted defaults, and the package version reads
**0.2.1**.

This plan starts from accepted v0.2.0 review artifacts. If the trunk/branch/twig
grammar, socket contract, or target coverage is still in question, stay in the
v0.2.0 plan instead of using this patch as a second design pass.

The first act of v0.2.1 is a written artifact review, not code. Copy the
accepted v0.2.0 baseline/candidate artifact paths from the orchestration hub,
list the specific visual issues being fixed, copy the accepted config snapshot,
and leave any new contract work in v0.2.0 or a later plan.

## Scope

**In scope**

- Tune dendrite reach, branching count, socket spread, terminal twig taper, and
  resting morphology brightness/width.
- Use the v0.2.0 `MorphologyParams`/settings surface as the tuning entry point;
  no late hidden constants.
- Improve dendrite shape using the same cubic Bezier sampler from v0.2.0.
- Compare visual review artifacts against the accepted v0.2.0 near/mid/far
  `morph_view` frames, config snapshots, stats/profile JSON, and
  resting-opacity-zero frame.
- Add lightweight acceptance stats to the morphology example if helpful:
  segment count, dropped count, terminal-to-socket distance bands, and coverage.
- Preserve the v0.2.0 socket contract: terminal twigs should remain visually
  attached to dendrite anchors after tuning, not just statistically close.
- Update dev-panel ranges/default labels only for settings that actually ship
  with new accepted values.
- Bump package/crate versions from 0.2.0 to 0.2.1.
- Update docs only for actual shipped parameter/behavior changes.

**Out of scope**

- No source-target, socket, or branch-topology contract change from v0.2.0.
- No reverse incoming-direction dendrite bias.
- No per-region morphology variation.
- No morph-pass soma primitive.
- No shader layout change.
- No new dev-panel control outside the v0.2.0 Morphology group by default, and
  no public control.
- No whole-path upstream-lighting shader change.

## Approach

**Stream A - visual parameter tuning**

Owned files: `crates/brain-visualizer/src/sim/morphology.rs`,
`crates/brain-visualizer/examples/morph_view.rs` if artifacts improve review.

- Start from a written list of issues observed in the accepted v0.2.0 artifact
  set, not a general desire to make the forest nicer.
- Copy the accepted v0.2.0 morphology config snapshot before editing; every
  candidate artifact records the changed values relative to that baseline.
- Classify each issue before editing: clutter, weak continuity, weak far-view
  directionality, detached soma/readability, brightness/taper imbalance, or
  stats regression. If an issue does not fit one of those buckets, write down why
  it belongs in this patch.
- Tune from screenshots, not from code taste.
- Prefer existing v0.2.0 parameters and dev-panel settings. If a new parameter
  is truly needed, add it to the config surface first, classify it, record it in
  artifacts, and only then decide whether it belongs in the hidden UI.
- Keep the segment budget from v0.2.0 unless screenshots prove a small increase
  is worth it.
- Prefer shape changes over brightness hacks when solving clutter.
- Use one tuning implementer. Multiple agents can review artifacts, but they
  should not edit `morphology.rs` in parallel.

**Stream B - dendrite curve cleanup**

Owned file: `crates/brain-visualizer/src/sim/morphology.rs`.

- Move dendrites onto the same Bezier sampler if v0.2.0 left them on the older
  straight/bifurcated form.
- Keep dendrites decorative and local. They may support socket landing visually,
  but they do not become simulated compartments.
- If changing socket spread or dendrite reach, compare terminal-to-socket
  distance bands before and after; do not trade visible continuity for a prettier
  isolated dendrite silhouette.

**Stream C - settings/UI cleanup**

Owned files: `web/src/core/settings.ts`, `web/src/core/setting-metadata.ts`,
`web/src/ui/dev-panel.ts`, Rust `VisualSettings` consumers if touched, and docs.

- Only run this stream if v0.2.1 changes defaults, ranges, labels, impact
  classifications, or the promoted UI set.
- Keep the Morphology settings together. Do not leave one tuning parameter as a
  code-only default if it is needed to reproduce the accepted visual and was
  classified as `dev-panel-candidate`.
- If defaults or meanings change in persisted settings, update the localStorage
  schema sentinel as part of the same patch.
- If no UI/default/range changes are needed, record that explicitly in the
  migration notes and skip this stream.

**Artifact expectations**

- Reuse the v0.2.0 camera set, seed, visual settings, and accepted config
  snapshot so before/after differences are visible.
- Save candidate frames and stats/profile JSON under names that include
  `0.2.1`, and record the paths and config snapshot in the orchestration
  artifact ledger.
- Include the issue list and human review decision in this plan's migration
  notes before marking it shipped.

**Stream D - version + docs**

- Bump `app/crates/brain-visualizer/Cargo.toml` and `app/web/package.json` to
  0.2.1.
- Update `architecture/manifold.md` if generation parameters or dendrite shape
  materially change.
- Update `architecture/gpu-rendering.md` only if render-facing semantics change.
- Update `architecture/dev-panel.md` if settings, ranges, defaults, impact
  classes, or persistence/index details change.
- Update `architecture/profiling.md` if artifact stats/profile schema changes.
- Update `decisions/manifold.md` or `decisions/rendering.md` only for a new
  durable trade-off, not for numeric tuning alone.
- Update `decisions/dev-tooling.md` / `decisions/profiling.md` only for new
  durable rationale.

## Exit Gate

- `cd app && cargo test -p brain-visualizer`
- `cd app && cargo run -p brain-visualizer --example morph_view`, with saved
  review frames, config snapshot, and stats/profile JSON from the same camera
  set as v0.2.0.
- `cd app && cargo run -p brain-visualizer --example render_check`
- `cd app/web && npm run typecheck` if package metadata, settings defaults, or
  TS surfaces are touched.
- Package/crate versions read `0.2.1`.
- Default-scale visual review accepts:
  close cell = branching but not overgrown;
  far brain = directional grain but not fog;
  stats = no worse target coverage than v0.2.0;
  config = final parameter values are recorded and either reflected in the
  hidden Morphology UI or explicitly classified review-only/protected;
  resting-opacity-zero = live signal still legible.

## Discipline rules

- Do not begin this plan until v0.2.0 has shipped or been explicitly abandoned.
- Do not add new controls to compensate for weak defaults unless the existing
  v0.2.0 config surface has already been exhausted.
- Do not tune by adding hidden constants. Every changed knob goes through the
  morphology config snapshot.
- Do not increase segment count without naming the visual benefit.
- Do not tune away the v0.2.0 branch grammar just because a parameter change is
  easier than a targeted visual fix.
- Do not reopen the v0.2.0 source-type, socket, or branch-grammar contracts
  unless v0.2.0 is formally abandoned.

## Migration notes (filled in at ship time)

Route durable content into:

- `architecture/manifold.md` - final dendrite/socket/taper shape and any changed
  caps.
- `architecture/gpu-rendering.md` - only if render-facing morphology semantics
  changed.
- `architecture/dev-panel.md` - only if final settings/ranges/defaults changed.
- `architecture/profiling.md` - only if artifact stats/profile schema changed.
- `decisions/manifold.md` / `decisions/rendering.md` - only for new durable
  rationale.
- `decisions/dev-tooling.md` / `decisions/profiling.md` - only if tuning creates
  new durable settings/profiling rationale.

## See also

- [`procedural-cell-rework.md`](procedural-cell-rework.md) - roadmap.
- [`procedural-cell-rework-orchestration.md`](procedural-cell-rework-orchestration.md)
  - high-level sequencing and agent handoffs.
- [`procedural-cell-rework-0.2.0-axon-arbor.md`](procedural-cell-rework-0.2.0-axon-arbor.md)
- [`future_roadmap.md`](future_roadmap.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../architecture/profiling.md`](../architecture/profiling.md)
