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

# v0.2.0 - Axon Arbor Foundation

## Mission

Replace the current K independent soma-to-target axon curves with a deterministic
shared arbor: soma/root -> one trunk -> 2-5 direction clusters -> terminal twigs
that land on visible dendrite socket anchors for each real target. Done = the
far view gains directional vein-grain, the near view reads as a branching
neuron, rendered terminal targets match the sim's actual unique non-self
targets, and the package version reads **0.2.0**.

This is the main release. It deliberately leaves richer dendrites, region
variation, and soma-primitive work for later plans so the core visual can be
validated alone.

The configuration surface is part of the release, not cleanup. Build the
morphology settings/profiling layer first, consume it while implementing the
arbor, and only then gather the accepted controls into the hidden dev panel as a
coherent Morphology group.

## Scope

**In scope**

- Thread the real source type into morphology generation so target resolution
  matches production scatter, including excitatory anterior bias.
- Treat source-type plumbing as a preflight correctness slice before branch
  grammar work begins.
- Add deterministic dendrite sockets per neuron: a small stable set of local
  landing anchors around the soma, generated from morphology salts and tied to
  visible dendrite tips/branch points where possible.
- Add a single morphology configuration surface before branch grammar work:
  named generation parameters, default preset, review overrides, config
  snapshots in artifacts, and no hidden one-off constants.
- Add morphology build/profile stats before tuning so every candidate records
  generation cost, segment budget pressure, coverage, socket quality, and render
  artifact metadata.
- Replace per-target sin-bow axons with a budgeted branch grammar:
  shared trunk segments, cluster branch segments, and one terminal twig per
  unique non-self target.
- Use one cubic Bezier sampler for trunk/branch/twig curves.
- Keep `MorphSegment` at 48 bytes; no WGSL struct layout change.
- Define shared-segment `target_id` semantics explicitly.
- Add `morph_view` review artifacts and lightweight stats if the current harness
  does not expose enough information to judge the new grammar.
- Promote the accepted morphology settings into one hidden dev-panel UI group at
  the end of v0.2.0, with impact metadata, persistence, Rust/TS settings
  boundary updates, and range/default labels checked together.
- Bump `app/crates/brain-visualizer/Cargo.toml` and `app/web/package.json`
  from 0.1.2 to 0.2.0.
- Update owning docs and decisions at ship time.

**Out of scope**

- No inspect/select mode.
- No connectivity-rule change; morphology consumes the same rule the sim uses.
- No incoming-direction dendrite bias or reverse who-targets-me pass.
- No per-region morphology variation.
- No morph-pass soma primitive.
- No Catmull-Rom.
- No public settings page or preset manager. The final UI surface is hidden
  dev-panel tooling only.
- No shader/layout work to make shared trunk/cluster segments light as whole
  upstream paths.

## Approach

**Stream A - configuration + profiling foundation**

Owned files: `crates/brain-visualizer/src/sim/morphology.rs`,
`crates/brain-visualizer/examples/morph_view.rs`, callers in
`crates/brain-visualizer/src/sim/gpu/resources.rs`,
`crates/brain-visualizer/src/sim/gpu/mod.rs`, and tests.

- Start here before source-type or arbor grammar work. The implementation should
  tune through a named parameter surface, not by repeatedly editing ad hoc
  constants.
- Introduce a `MorphologyParams`-style config object with a locked default
  preset and generated artifact snapshot. `generate()` consumes this object from
  the start, even if early fields just mirror today's constants.
- Move the current morphology constants into named parameters or explicitly mark
  them as protected internals. Initial parameter groups:
  base radius; dendrite primary count/reach/curl/taper; socket count/spread/
  radius/tip preference; arbor mode; cluster min/max and assignment bias;
  trunk/cluster/twig Bezier samples; trunk length/split fraction; axon stop
  fraction; root/branch/twig radii; taper curve; per-kind segment budgets; slack.
- Classify every parameter before tuning:
  `generator-default` = Rust-owned default, `review-override` = `morph_view`
  only, `dev-panel-candidate` = may become UI, `protected` = not exposed because
  it guards a contract.
- Add a `MorphologyStats` / build-profile result alongside `Morphology` so
  tests and `morph_view` can read facts without scraping log text. Include at
  least: segment count, dropped count, cap utilization, per-neuron segment
  bands, unique-target coverage, duplicate/self-target counts, cluster count
  histogram, terminal-to-socket distance bands, socket reuse bands, generation
  timings by phase, segment-buffer bytes, and adapter/native-vs-llvmpipe status
  when available.
- Extend `morph_view` before branch grammar review so it saves one JSON stats
  file per artifact set. The JSON should include the config snapshot, visual
  settings snapshot, seed, N/K, warmup, camera set, output paths, skip/pass
  status, and simple image/readability proxies (non-black pixel percentage,
  optional luminance bands) where cheap.
- Keep these stats outside the always-on hot-loop profiler. They are build and
  review instrumentation, not per-frame dynamics counters.

**Stream B - source-type accurate morphology target input**

Owned files: `crates/brain-visualizer/src/sim/morphology.rs`, callers in
`crates/brain-visualizer/src/sim/gpu/resources.rs`,
`crates/brain-visualizer/src/sim/gpu/mod.rs`, and any tests.

- Stream B starts only after Stream A's config/stat object is in place.
- Extend `generate()` to receive enough per-neuron type data to call
  `connectivity::target_with_cell` with the true source type, not fixed `0u8`.
- Build those source types from the same `neuron_type_byte` contract used for
  `last_spike`; do not invent a second E/I classifier. Preferred shape:
  precompute a `source_types: &[u8]` (or equivalent) from the manifold regions
  before morphology generation, then pass it through both initialization and
  regeneration.
- Ensure both initialization and `regenerate_morphology()` use the same source
  type input.
- Keep the connectivity function itself unchanged.
- Add a test that compares morphology terminal targets against
  `connectivity::target(..., source_type)` for a mixed E/I probe set.

**Stream C - sockets + branch grammar**

Owned file: `crates/brain-visualizer/src/sim/morphology.rs`.

- Stream C starts only after Stream B's target-coverage test is green.
- Add deterministic dendrite sockets per neuron. A socket is a generated local
  anchor position/radius band backed by a visible dendrite endpoint or branch
  point; it is not a new GPU struct. Terminal twigs should visually attach to
  the socket, not merely stop somewhere near the target soma.
- Use the Stream A configuration surface for all grammar constants: min/max
  clusters, Bezier samples per trunk/cluster/twig, width/taper bands, socket
  radius bands, and slack.
- Resolve each source neuron's unique non-self targets, group them into bounded
  deterministic direction clusters, and emit:
  trunk/root segments with shared-segment identity;
  one branch to each cluster centroid;
  one terminal twig from cluster branch to a socket near the target.
- For one unique target, emit the direct branch/twig form. For two or more
  unique targets, use 2-5 clusters, clamped by unique-target count and the hard
  segment budget.
- Define deterministic tie-breaks before tuning: duplicate/self targets are
  removed, remaining targets are sorted stably, and cluster assignment is pure
  from source id, target id, positions, and morphology salts.
- Terminal twig segments carry the target's real `target_id`; socket selection
  is deterministic from source id, target id, target position, and morphology
  salts.
- Shared trunk/cluster segments carry `target_id = neuron_id`; upstream
  `light_past` therefore lights terminal twigs only. Downstream/source lighting
  (`light_next`) is the v0.2.0 acceptance baseline for shared segments. If
  upstream looks too weak, do not silently reinterpret `target_id`; move
  whole-path upstream lighting into a future shader plan.
- Fallback if shared trunking looks worse: ship a simpler shared root plus
  clustered terminal twigs, then record the discarded trunk shape in the plan
  migration notes.

**Stream D - budget, tests, visual gate**

Owned files: morphology tests plus examples if they need output naming tweaks.

- Set the hard per-neuron segment budget through `MorphologyParams` before
  tuning. Keep separate named budgets for dendrites, trunk/cluster segments,
  terminal twig segments, and slack; do not grow the cap by accident while
  iterating.
- Keep `Morphology::dropped` and make `dropped == 0` part of the default-scale
  acceptance gate.
- Re-intent `draws_all_k_axon_branches` to assert one terminal per unique
  non-self target, not one rendered branch per raw `j`.
- Add tests for deterministic clustering, terminal coverage, and socket landing
  distance.
- Capture `morph_view` near/mid/far frames plus a resting-opacity-zero frame as
  review artifacts before moving to v0.2.1.
- Artifact contract: save a baseline and candidate set with the same seed,
  N/K, camera views, warmup, visual settings, and morphology config snapshot.
  Record raw frame paths, PNG paths if conversion happened, and a stats JSON
  path in the orchestration artifact ledger.
- Have `morph_view` print or save at least: segment count, dropped count,
  unique-target coverage, terminal-to-socket distance bands, and whether the
  native GPU adapter was present or the example skipped.

**Stream E - dev-panel settings consolidation**

Owned files: `web/src/core/settings.ts`, `web/src/core/setting-metadata.ts`,
`web/src/ui/dev-panel.ts`, Rust `VisualSettings` consumers if the settings cross
WASM, and docs.

- Start only after the v0.2.0 grammar has an accepted default and the artifact
  ledger records the config values used to accept it.
- Promote the accepted `dev-panel-candidate` parameters into one hidden
  Morphology section/group. Do this as a single UI pass so names, ranges,
  defaults, persistence, impact dots, and Rust/TS indices are reviewed together.
- Treat morphology generation settings as `brain-reset` if they require
  rebuilding/re-uploading static morphology, unless the implementation proves a
  field is a pure live render uniform. `connectionVisualWidth`,
  `morphRestingOpacity`, `connectionLightNext`, and `connectionLightPast` remain
  live-style render controls; structural arbor/socket/sample/budget parameters
  should not pretend to be live.
- If the Float32Array settings contract grows, append indices only, update both
  TS and Rust comments, update `SETTING_IMPACT`, update persistence defaults, and
  bump the localStorage schema sentinel if defaults or meanings would mislead
  older saves.
- Keep review-only/protected parameters out of the UI, but leave them in the
  stats/config snapshot so future tuning can reproduce the accepted build.

**Stream F - version + docs**

After code and visual gate pass:

- Bump package/crate versions to 0.2.0.
- Update `architecture/manifold.md` with the current tree/arbor generation,
  source-type target input, socket landing, cap formula, and terminal coverage
  contract.
- Update `architecture/gpu-rendering.md` with shared-segment `target_id`
  semantics and the upstream-lighting consequence.
- Update `architecture/dev-panel.md` with the final Morphology settings group,
  impact classifications, persistence/index changes, and any localStorage
  sentinel bump.
- Update `architecture/profiling.md` with morphology build/profile stats and
  clarify which stats are artifact-only versus always-on runtime metrics.
- Update `decisions/manifold.md` with shared arbors over independent splines,
  Bezier over sin-bow, and sockets over vague "near target" landing.
- Update `decisions/rendering.md` with terminal-only upstream lighting for
  shared paths.
- Update `decisions/dev-tooling.md` if the settings group changes the dev-panel
  contract, and `decisions/profiling.md` if the profiling boundary changes.

## Exit Gate

- `cd app && cargo test -p brain-visualizer`
- `cd app && cargo run -p brain-visualizer --example morph_view`, inspected at
  far, mid, and near distances:
  far = directional grain, not isotropic fuzz;
  near = recognizable trunk/branch/twig neuron;
  resting-opacity-zero = live signal still legible;
  stats JSON = config snapshot, profile stats, and full unique non-self terminal
  coverage;
  visible sockets = terminal twigs appear attached to dendrite anchors, not
  floating near target somas;
  default scale = no dropped morphology segments.
- `cd app && cargo run -p brain-visualizer --example render_check`
- `cd app/web && npm run typecheck`
- Hidden dev-panel Morphology settings group exposes the accepted controls
  together, with correct impact dots and no orphan config fields.
- `app/crates/brain-visualizer/Cargo.toml` and `app/web/package.json` read
  `0.2.0`.
- Owning docs reflect the shipped shape; this plan can be marked
  `shipped + okay_to_delete: true`.

## Discipline rules

- Do not change connectivity math or the `wgsl_*_determinism` gates.
- Do not add new WGSL layout fields in this release.
- Do not start source-type or branch-grammar edits before the config/stat
  surface exists.
- Do not add hidden morphology constants once `MorphologyParams` exists. Add a
  parameter, classify it, and make artifacts record it.
- Do not add a one-off dev-panel slider during tuning. UI comes after accepted
  defaults, as one consolidated Morphology settings group.
- Do not raise default K or hide visual clutter by making the default scale
  smaller.
- Do not split Streams B/C/D across parallel implementers; they converge on the
  same morphology contract and file.
- Do not treat whole-path upstream lighting as a v0.2.0 blocker. Terminal-only
  upstream lighting is acceptable for shared paths.
- Do not make invisible socket math carry the visual contract. If terminal
  continuity is not visible in review frames, retune dendrite/socket geometry or
  document the fallback before shipping.
- If `morph_view` does not look better after Stream C, stop and retune the
  branch grammar before starting v0.2.1.

## Migration notes (filled in at ship time)

Route durable content into:

- `architecture/manifold.md` - final morphology params, source-type accurate
  target input, socket generation, branch grammar, cap formula, coverage
  contract.
- `architecture/gpu-rendering.md` - shared segment `target_id` semantics.
- `architecture/dev-panel.md` - final Morphology settings group and impact
  classes.
- `architecture/profiling.md` - morphology build/profile artifact stats.
- `decisions/manifold.md` - why shared arbor + sockets.
- `decisions/rendering.md` - why upstream lighting is terminal-only for shared
  paths.
- `decisions/dev-tooling.md` / `decisions/profiling.md` - only if the shipped
  settings/profiling shape creates new durable rationale.

## See also

- [`procedural-cell-rework.md`](procedural-cell-rework.md) - roadmap.
- [`procedural-cell-rework-orchestration.md`](procedural-cell-rework-orchestration.md)
  - high-level sequencing and agent handoffs.
- [`procedural-cell-rework-0.2.1-dendrite-tuning.md`](procedural-cell-rework-0.2.1-dendrite-tuning.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../architecture/profiling.md`](../architecture/profiling.md)
