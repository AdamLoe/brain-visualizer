---
status:        shipped
owner:         planner
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
  - decisions/rendering.md
---

# Dendrite branching near soma

## Mission

Improve the target-owned incoming dendrite arbor so it reads as branching near
the soma with tight organic curves, not as one thick cylinder with thin lines
running into it. Done when the default close-up `morph_view` artifact shows
short soma-proximal forks, visible but tapered child branches, and no long
unbranched dendrite barrels entering the soma.

Planning only: this document names implementation steps and file ownership. Do
not edit source until the implementation stream is explicitly launched.

## Grounding

Authoritative geometry is
`app/crates/brain-visualizer/src/sim/morphology.rs`.

Current incoming dendrite grammar:

- `generate` builds a reverse incoming view, then calls
  `emit_incoming_dendrites` once per target neuron.
- `emit_incoming_dendrites` sorts `IncomingSocketGroup`s by
  `(socket_idx, source_id)`.
- For each `socket_idx` bucket it computes a weighted direction and one
  `branch_point` off the soma.
- It emits one shared target-owned stem from `branch_point` to the soma with
  `target_id = neuron_id`.
- It emits one source-specific leaf from each socket to that same
  `branch_point` with `target_id = source_id`, so presynaptic dendrite leaves
  can pulse from the source neuron's `last_spike`.
- Both stems and leaves use `emit_bezier_path` with 2 samples today
  (`DENDRITE_STEM_SAMPLES = 2`, `DENDRITE_TWIG_SAMPLES = 2`).
- Dendrite radii are derived from `base_radius`,
  `dendrite_mid_radius_fraction`, `dendrite_tip_radius_fraction`, and a
  per-group weight scale.

Shader/material implications are in
`app/crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`:

- `MorphSegment` is a 48 B Rust/WGSL layout contract. This plan should preserve
  it.
- The shader renders every segment as a tapered tube; straight thick shared
  stems will naturally read as cylinders.
- Dendrites are hard-coded to a cool base color in `branch_base_color`; axons
  get region/E-I/identity behavior.
- Activity ownership is already compatible with source-specific dendrite
  leaves: `presynaptic_dendrite = kind == 0 && target_id != neuron_id` selects
  `target_id` for `last_spike`.
- The active-opacity pass uses segment interval overlap, so changing segment
  subdivision changes visual packet granularity but does not need a new buffer.

Acceptance artifacts are in
`app/crates/brain-visualizer/examples/morph_view.rs`:

- `/tmp/morph_0.rgba` through `/tmp/morph_3.rgba`
- `/tmp/morph_active_bright.rgba`
- `/tmp/morph_view_stats.json`
- `/tmp/morph_view_0.3.0_stats.json`
- `/tmp/morph_view_active_bright_stats.json`

## Proposed branch grammar

Replace the current "one bucket stem plus many leaves" shape with a bounded
soma-proximal tree per target neuron.

Lead decision on 2026-06-09: the target is both biological plausibility and
readability. Interpret this as biologically inspired dendrite branching that
prioritizes clear visual read over strict anatomical simulation unless the lead
later supplies a stricter reference.

Follow-up lead decisions on 2026-06-09: every incoming group must remain
individually legible, and the stream should add the UI controls needed for the
new behavior while deleting unused controls. Do not ship this as locked-default
only.

Second follow-up lead decision on 2026-06-09: expose all proposed first-pass
controls: primary root count, fork distance, curve tightness, branch
thickness/taper, and individual-group spacing. Exact names/ranges can be chosen
by the implementation worker and tuned against artifacts.

1. **Root collars, not center-entering stems.**
   Start dendrite branches at deterministic soma-surface collars:
   `root = soma + root_dir * base_radius * root_surface_factor`, then curve to a
   first fork just outside the soma. Avoid drawing thick tubes all the way into
   the soma center; that is the main cylinder read.

2. **Primary roots are angularly organized, but groups stay individually legible.**
   Keep socket id as a deterministic input, but cluster incoming groups by
   outward direction around the target soma. Use a small bounded primary count,
   for example 3-5 roots per populated neuron, with stable tie-breaks by
   `(socket_idx, source_id)`. Dense groups may share a nearby organizing root or
   fork, but each incoming group needs its own visible terminal/branch identity;
   do not hide groups by merging them into one indistinguishable twig.

3. **Forks happen close to the soma.**
   Place first forks around `1.25R-1.8R` from the soma center, and secondary
   forks around `1.8R-3.0R`, clamped by the actual socket distance. The goal is
   visible branching immediately around the soma, with terminal twigs reaching
   existing socket positions.

4. **Tight curved segments.**
   Use cubic Bezier control handles biased tangentially around the soma, not
   only along the edge direction. Clamp handles so curves do not pass through
   the soma sphere. Increase dendrite samples only as needed for the curve to
   read smoothly; if sample counts change, update `DENDRITE_MAX` and cap
   accounting in the same change.

5. **Taper without hairlines.**
   Short root collars should be visibly thinner than the soma/axon trunk, while
   child branches should stay thick enough to read at the default camera:
   roughly root `0.70R-0.85R`, first-fork `0.50R-0.65R`, terminal
   `0.35R-0.50R`, then tune against the artifact. Avoid the current failure mode
   where a large shared tube receives much thinner terminal lines.

6. **Preserve honest ownership.**
   Shared internal branches stay target-owned (`target_id = neuron_id`).
   Source-specific terminal leaves keep `target_id = source_id`. Do not add a
   side channel for shared-stem multi-source activity in this stream.

7. **Expose the new shaping levers.**
   Add UI/descriptors for the controls that are genuinely needed to tune the new
   branch grammar, and remove unused legacy controls in the same stream.
   Approved first-pass controls: primary root count, fork distance, curve
   tightness, branch thickness/taper, and individual-group spacing.

8. **Budget explicitly.**
   Recalculate dendrite worst-case segments from the new grammar, including
   max primary roots, max secondary forks, samples per root/fork/twig, and
   observed incoming group p99/max. The implementation should keep
   `dropped_count = 0` and `incoming_dropped_count = 0` at default N=1200/K=16.

## Implementation Plan

1. **Read-only recon before edits.**
   Check `git status` and inspect any existing diffs in `morphology.rs`,
   `render_morphology.wgsl`, and `morph_view.rs`; both `morphology.rs` and the
   shader were already dirty during this planning pass. Confirm whether
   `web/src/core/morph-config.ts` still applies older dendrite radius defaults
   in browser sessions before relying on a Rust-only default.

2. **Extract a local dendrite planning model.**
   Inside `morphology.rs`, introduce small host-only helpers for target-local
   dendrite groups, for example `DendriteCluster` / `DendriteNode` structs near
   `emit_incoming_dendrites`. Keep them private and deterministic. Do not alter
   `IncomingSynapse`, `IncomingSocketGroup`, or `MorphSegment`.

3. **Build soma-proximal clusters.**
   Convert the incoming groups for one target into bounded primary roots and
   optional child clusters. Inputs: socket position direction, socket distance,
   socket id, source id, and weight. Outputs: root collar, first fork, optional
   secondary fork, and terminal socket assignments.

4. **Emit curved root/fork/twig paths.**
   Reuse `emit_bezier_path` for all emitted edges. Give each edge a deterministic
   curl seed from existing dendrite salts plus source/target/socket ids. Maintain
   monotonic path lengths per branch so active packet timing remains coherent.

5. **Tune taper and samples against the artifact.**
   Start with no shader changes. Adjust host radii and sample counts until the
   close-up no longer shows fat cylinders with hairline inputs. If the tube
   material still reads too cylindrical after geometry is fixed, open a separate
   shader subtask for subtle dendrite-only material variation.

6. **Update budget/stats/tests.**
   Update `DENDRITE_MAX` comments and cap math if the grammar emits more
   segments. Add a per-feature test file, preferably
   `app/crates/brain-visualizer/tests/dendrite_branching_near_soma.rs`, that
   verifies default generation has no drops and measures root/fork distance and
   radius ratios through the public generator path. If public fixtures are
   insufficient, keep the narrowest possible test-only exposure rather than
   moving broad private helpers into public API.

7. **Add controls and remove unused controls.**
   Add descriptor-backed UI controls for the accepted shaping levers. Remove
   any unused/deduplicated dendrite controls in the same pass so the settings
   surface stays honest.

8. **Artifact pass.**
   Run `morph_view` and review the zoomed default frame. If the existing cameras
   do not isolate a soma well enough, update `morph_view.rs` with a deterministic
   close-up frame and include it in the artifact JSON.

9. **Doc migration after visual acceptance.**
   Update `architecture/manifold.md` with the new dendrite grammar and budget
   facts. Update `architecture/gpu-rendering.md` only if shader/material or
   artifact behavior changes. Record durable trade-offs in
   `decisions/manifold.md` and `decisions/rendering.md` as applicable.

## Owned Files Likely Impacted During Implementation

Primary owner:

- `app/crates/brain-visualizer/src/sim/morphology.rs` — dendrite grammar,
  deterministic clustering, radii, segment budget, stats, and host tests if
  existing in-module tests must be adjusted.

Likely additions or narrow edits:

- `app/crates/brain-visualizer/tests/dendrite_branching_near_soma.rs` — new
  per-feature geometry gate, if public test fixtures are practical.
- `app/crates/brain-visualizer/examples/morph_view.rs` — only if the existing
  close-up views are insufficient for acceptance or new stats need artifact
  exposure.
- `app/web/src/core/morph-config.ts` — required for new dendrite descriptors and
  removal of unused dendrite controls.
- `app/web/src/ui/dev-panel.ts` and `app/web/src/ui/dev-panel.test.ts` —
  required if the descriptor/control changes affect panel rendering or tests.

Conditional owners:

- `app/crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl` —
  only if geometry alone cannot remove the cylinder read and dendrite material
  needs a shader-side tweak.
- `docs/architecture/manifold.md`, `docs/architecture/gpu-rendering.md`,
  `docs/decisions/manifold.md`, `docs/decisions/rendering.md` — migration
  targets after acceptance.

## File Collision Risks

- Only one implementation worker should own `morphology.rs` at a time. This is
  the same collision rule as the morphology refresh and visual polish hubs.
- `render_morphology.wgsl` is shared with active-opacity and brain-color-mode
  work. Keep shader edits out of the first pass unless the geometry artifact
  still fails.
- `web/src/core/morph-config.ts` is shared with settings cleanup and defaults
  work. The lead decision now requires UI-exposed tuning for this stream, so
  sequence after settings IA removes stale controls and before final default/docs
  cleanup.
- `morph_view.rs` is an artifact harness used by multiple visual streams. If a
  new close-up frame is added, sequence with any other artifact changes.
- The source files `morphology.rs` and `render_morphology.wgsl` were dirty when
  this plan was written. Implementation must inspect current diffs instead of
  assuming the text observed here is clean mainline state.

## Narrow Gates and Artifacts

Per-stream gates should stay focused:

- `cd app && cargo test -p brain-visualizer --test dendrite_branching_near_soma`
  if the per-feature integration test is added.
- `cd app && cargo test -p brain-visualizer sim::morphology::incoming_synapses_drive_target_owned_dendrites`
  to preserve reverse incoming ownership and source-specific leaves.
- `cd app && cargo run -p brain-visualizer --example morph_view` for default
  visual artifacts and JSON stats.
- If `render_morphology.wgsl` changes, also run
  `cd app && cargo run -p brain-visualizer --example render_check`.

Acceptance artifact checks:

- Zoomed default frame shows multiple short soma-proximal branches and no long
  unbranched thick dendrite barrels.
- Terminal leaves remain visible at normal close-up distance and do not become
  hairlines.
- `/tmp/morph_view_stats.json` reports default N=1200/K=16, `dropped_count = 0`,
  `incoming_dropped_count = 0`, and unchanged reverse incoming raw/group counts
  unless a deliberate grouping policy changed them.
- If new artifact stats are added, include root/fork distance bands and dendrite
  radius-ratio bands so the visual claim has a numeric guard.

## Lead Questions

- How many primary dendrite roots should a typical soma show at default scale:
  sparse 3-4, denser 5-7, or driven entirely by incoming socket density?
- Is dendrite material/color allowed in this stream if geometry alone still
  reads cylindrical, or should shader color stay reserved for the brain color
  mode stream?
- Exact UI labels, ranges, and default values for the approved controls can be
  chosen by the implementation worker, then tuned by artifact review.

## Deferrals

- Do not change the production connectivity hash/target rule or reverse
  incoming build.
- Do not add multi-presynaptic activity encoding for shared internal dendrite
  branches.
- Do not introduce hidden incoming cap/sampling policy for high scale; if
  density becomes too high, plan that explicitly.
- Do not fold in the `Color by = Brain` shader semantics; that belongs to the
  brain color mode stream.
- Do not run full-suite gates per stream. Leave full `cargo test`, web
  typecheck/unit/e2e, and consolidated browser checks to the phase gate.

## Migration Notes

Migrated on 2026-06-11 into `architecture/manifold.md`,
`architecture/gpu-rendering.md`, `architecture/dev-panel.md`,
`decisions/manifold.md`, and `decisions/rendering.md`. Current-state docs record
the soma-surface root collars, close first forks, per-group child branches,
source-owned terminal leaves, preserved `MorphSegment` layout, exposed dendrite
controls/ranges/defaults, and artifact stats: `segment_count=103526`,
`dropped_count=0`, `incoming_dropped_count=0`,
`segment_cap_per_neuron=296`, `segment_cap=355200`, p99/max segments
`159/242`, and incoming visible groups mean/p99/max `10.356667/29/45`.

`okay_to_delete` remains `false` only because the visual-product-polish hub is
retaining all six stream plans until the real-WebGPU browser smoke blocker is
cleared or waived.
