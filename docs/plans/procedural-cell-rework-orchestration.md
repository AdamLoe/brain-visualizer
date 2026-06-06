---
status:        shipped
owner:         adamg
last_updated:  2026-06-06
okay_to_delete: true
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

# Procedural Cell Rework Orchestration

## Mission

Coordinate the v0.2.x morphology rework without turning it into a many-agent
merge fight. Done = v0.2.0 ships a configuration/profiling-first morphology
pipeline, the source-type-accurate shared arbor, review artifacts, and a
consolidated hidden dev-panel Morphology settings group; v0.2.1 either ships a
narrow tuning patch or is skipped; all durable facts migrate into the owning
architecture/decisions docs.

This is a coordination hub, not canonical architecture. Use it with
[`../agent-context/orchestrating.md`](../agent-context/orchestrating.md) and the
leaf plans:
[`procedural-cell-rework-0.2.0-axon-arbor.md`](procedural-cell-rework-0.2.0-axon-arbor.md)
and
[`procedural-cell-rework-0.2.1-dendrite-tuning.md`](procedural-cell-rework-0.2.1-dendrite-tuning.md).

## Doc Roles

- [`procedural-cell-rework.md`](procedural-cell-rework.md) is the mostly-static
  release map: what ships in v0.2.0/v0.2.1 and what stays deferred.
- The leaf plans own implementation contracts, gates, and ship-time migration
  notes for each version.
- This orchestration hub owns live observed state: stream status, artifact
  paths, human visual decisions, fallbacks taken, and open questions.
- If a fact changes while work is underway, record the observed fact or decision
  here first, then update the leaf plan only when the contract itself changes.

## Operating stance

- One morphology implementer owns `crates/brain-visualizer/src/sim/morphology.rs`
  at a time. Do not parallelize streams that edit it.
- Agents may review artifacts in parallel, but only after the artifact-producing
  stream reports exact outputs and gates.
- Human visual review is the decision gate for "looks better." Agents can
  generate frames, stats, and critiques; they should not silently decide the
  homepage visual is accepted.
- The first implementation wave builds the morphology config/profile surface.
  All later morphology edits use that surface; no hidden tuning constants should
  be introduced once it exists.
- Per-stream gates stay narrow. Run the heavier `morph_view`, `render_check`,
  docs/version, and web typecheck gates only at the consolidated checkpoints.
- No v0.3-style work during v0.2.x: no per-region morphology, no morph-pass soma,
  no inspect mode, and no shader/layout change for whole-path upstream lighting.
- Do not decide visual acceptance from prose alone. The accepted shape needs
  artifact paths, config snapshots, stats/profile JSON, and an explicit human
  review note in this hub.

## Live Tracker

Use this table during the run. Do not pre-fill optimism; update rows only from
disk truth, agent reports, or human review.

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| 0 | Baseline recon | Complete | Worktree clean; crate and web package are `0.1.2`; `generate()` is scalar-based; source type is hardcoded to `0u8`; `morph_view` writes raw RGBA only with no PNG/stats JSON. | None. | None |
| 1 | Config/profile foundation | Complete | `MorphologyParams`, `MorphologyStats`, `generate(..., params)`, and `/tmp/morph_view_stats.json` landed; gate `cd app && cargo test -p brain-visualizer morphology` passed with 8 tests, 72 filtered; `cargo check -p brain-visualizer --example morph_view` also passed. | None. | None |
| 2 | Source-type preflight | Complete | Morphology now builds production `neuron_type_byte(...)` values from region+seed, passes them through initialize and `regenerate_morphology`, and uses them with `target_with_cell`; gate `cd app && cargo test -p brain-visualizer morphology` passed with 9 tests, 72 filtered. | None. | None |
| 3 | Branch grammar | Complete | Shared-root/cluster/terminal-twig grammar landed with deterministic sockets, terminal segments carrying real target ids, shared segments carrying source id, unique-target stats, and default-scale `dropped == 0` expected; gate `cd app && cargo test -p brain-visualizer morphology` passed with 10 tests. | None. | None |
| 4 | Artifact harness | Complete | `morph_view` passed under llvmpipe and wrote `/tmp/morph_view_stats.json` plus `/tmp/morph_{0,1,2,3}.rgba`; PNG review copies also exist at `/tmp/morph_{0,1,2,3}.png`; latest run reports `segment_count=68854`, `dropped_count=0`, full unique-target coverage, and per-frame opacity snapshots. | None. | None |
| 5 | Visual review | Complete | Human review accepted that the candidate is moving in the right direction and cleared the plan to continue from the v0.2.0 artifacts. | None. | None |
| 6 | Settings UI consolidation | Complete | Hidden Rendering-tab controls were re-homed under a Morphology section: `connectionLayer`, `connectionLightNext`, `connectionLightPast`, `morphRestingOpacity`, `connectionVisualWidth`, `connectionCurveLift`; `connectionCurveLift` metadata now reflects geometry rebuild; no Float32/Rust/default/persistence changes; `cd app/web && npm run typecheck` passed. | None. | None |
| 7 | Ship v0.2.0 | Complete | Versions now read `0.2.0`; durable docs were migrated; gates passed: `cargo test -p brain-visualizer`, `morph_view`, `render_check`, and `npm run typecheck`; v0.2.0 leaf plan is `shipped + okay_to_delete: true`. | None. | None |
| 8 | v0.2.1 tuning | Complete | v0.2.1 shipped as a narrow tuning patch: calmer dendrite reach/count, thinner/tapered shared branches, updated render defaults, versions at `0.2.1`, and gates passed (`morphology`, full Rust, `morph_view`, `render_check`, web typecheck). | None. | None |

## Artifact Ledger

Record exact outputs here. A skipped gate is an observed result, not a pass.
Every visual artifact row must include the morphology config snapshot used to
produce it.

| Version | Artifact set | Command | Output paths | Config snapshot | Stats/profile path | Review decision |
|---|---|---|---|---|---|---|
| baseline | Current morphology | Not yet produced by this run; recon observed native `morph_view` writes `/tmp/morph_{0,1,2,3}.rgba` and asserts non-black only. | `/tmp/morph_0.rgba` through `/tmp/morph_3.rgba` when run | Default visual behavior observed: `connection_curve_lift=0.15`, `connection_layer=1`, `morph_resting_opacity=0.25`; no morphology config snapshot exists yet. | No stats/profile JSON exists yet. | Pending baseline artifact capture |
| 0.2.0 candidate | Shared arbor | `cd app && cargo run -p brain-visualizer --example morph_view`; PNG conversion via `ffmpeg` | `/tmp/morph_0.rgba` through `/tmp/morph_3.rgba`; `/tmp/morph_0.png` through `/tmp/morph_3.png` | In `/tmp/morph_view_stats.json`; includes morphology params plus base/final/per-frame visual settings. | `/tmp/morph_view_stats.json` (`status=pass`, `segment_count=68854`, `dropped_count=0`, `unique_target_coverage=1.0`) | Accepted to continue v0.2.0 plan |
| 0.2.1 candidate | Tuning patch | `cd app && cargo run -p brain-visualizer --example morph_view`; PNG conversion via `ffmpeg` | `/tmp/morph_0.2.1_0.rgba` through `/tmp/morph_0.2.1_3.rgba`; `/tmp/morph_0.2.1_0.png` through `/tmp/morph_0.2.1_3.png` | In `/tmp/morph_view_0.2.1_stats.json`; includes morphology params plus base/final/per-frame visual settings. | `/tmp/morph_view_0.2.1_stats.json` (`status=pass`, `segment_count=56058`, `dropped_count=0`, `unique_target_coverage=1.0`, `all_k_coverage=true`) | Shipped as v0.2.1 tuning patch |

## Decision Log

| Decision | Why | Source |
|---|---|---|
| Stream 0 recon established current baseline before implementation. | Avoid dispatching Stream 1 against stale hub assumptions: current worktree is clean, packages are `0.1.2`, `generate()` has no config object, source type is still fixed to `0u8`, and `morph_view` lacks stats/profile JSON. | Cheap explorer report, 2026-06-06 |
| Stream 2 source-type contract should use production `neuron_type_byte(...)` values, not a new classifier. | CPU/GPU scatter decode the same byte from `last_spike`; only bit 0 affects routing, but the full byte is the source-of-truth contract. | Cheap explorer report, 2026-06-06 |
| Stream 1 parameter surface classifications are initial, not final UI commitments. | Popper classified current defaults as generator-owned, `axon_curve_lift` as review override, `axon_segments_per_branch`/`cap_slack`/hardcoded source type as protected, and no dev-panel candidates yet. | Stream 1 worker report, 2026-06-06 |
| Stream 2 source-type plumbing is protected internal behavior, not a user-facing morphology knob. | It aligns morphology target resolution with production scatter and should remain tied to region+seed `neuron_type_byte(...)` rather than dev-panel tuning. | Stream 2 worker report, 2026-06-06 |
| Stream 3 accepts terminal-only upstream lighting semantics for shared paths. | Terminal twig segments carry real target ids while shared root/cluster segments carry the source id; shader/layout work for whole-path upstream lighting remains out of v0.2.0. | Stream 3 worker report, 2026-06-06 |
| Stream 4 artifact pass is not visual-review-ready until visual settings metadata is fixed. | The first `morph_view` run passed and produced frames/stats, but JSON recorded final opacity-zero settings as the artifact-level visual settings for all frames. | Read-only review report, 2026-06-06 |
| Stream 4 stabilization fixed artifact metadata without changing grammar semantics. | The JSON now has base/final visual settings and per-frame visual settings; source-type coverage was removed as misleading; empty-network builder byte stats were clarified. | Stabilization worker report, 2026-06-06 |
| Morphology build timings are native-only; WASM reports zero timings. | Browser WASM panicked because `std::time::Instant::now()` is unsupported on this target during `morphology::generate()`. A target-aware timer preserves native artifact timings and avoids browser initialization panic. | Browser console report and local fix, 2026-06-06 |
| v0.2.0 visual direction is accepted enough to continue. | Human review said the candidate is moving in the right direction and cleared continuation of the plan. | Human review, 2026-06-06 |
| Stream 6 does not expose structural morphology generator parameters. | The shipped hidden UI groups existing render/rebuild controls only; base radius, sockets, cluster bounds, samples, budgets, and slack remain protected code/config snapshot values. | Stream 6 worker report, 2026-06-06 |
| v0.2.0 shipped after consolidated gates. | Versions, architecture docs, decisions docs, and plan migration notes were updated; full Rust, `morph_view`, `render_check`, and web typecheck gates passed. | Stream 7 worker report, 2026-06-06 |
| v0.2.1 should proceed as a narrow tuning patch. | Artifact review found no stats regression but identified visual clutter, weak far-view directionality, weak continuity legibility, detached soma/readability, and brightness/taper imbalance; source-target/socket/topology/shader contracts must not change. | v0.2.1 artifact review, 2026-06-06 |
| v0.2.1 shipped without reopening v0.2.0 contracts. | Tuning reduced segment count and visual density while preserving `dropped_count=0`, unique-target coverage, source-target behavior, socket/topology contracts, shader layout, and terminal-only upstream lighting. | v0.2.1 worker report and orchestrator artifact check, 2026-06-06 |

## Open Questions

- None yet.

## Waves

| Wave | Outcome | Primary files | Gate | Parallelism |
|---|---|---|---|---|
| 0. Baseline recon | Confirm current version, current `generate()` signature, current `morph_view` outputs, artifact paths, and any dirty docs/code state. | Read-only | Report only | Can be one read-only agent |
| 1. Config/profile foundation | `MorphologyParams` defaults, parameter classification, artifact config snapshots, and morphology build/profile stats exist before grammar tuning. | `morphology.rs`, `examples/morph_view.rs`, `sim/gpu/resources.rs`, maybe `sim/gpu/mod.rs`, tests | `cd app && cargo test -p brain-visualizer morphology`; `morph_view` should write stats JSON or clearly report skip | Serial; morphology owner |
| 2. Source-type preflight | Morphology target resolution uses the same source type contract as production scatter. | `morphology.rs`, `sim/gpu/resources.rs`, `sim/gpu/mod.rs`, tests | `cd app && cargo test -p brain-visualizer morphology` | Serial; morphology owner |
| 3. Branch grammar | Unique non-self targets are clustered into a shared arbor with deterministic sockets and terminal twigs. | `morphology.rs`, tests | `cd app && cargo test -p brain-visualizer morphology` | Serial; same owner preferred |
| 4. Artifact harness | `morph_view` emits fixed review frames, config snapshots, and stats: segment count, dropped count, coverage, terminal-to-socket distances, timings. | `examples/morph_view.rs`, maybe tests | `cd app && cargo run -p brain-visualizer --example morph_view` | Can follow Wave 3; avoid editing `morphology.rs` unless needed |
| 5. Visual review | Decide whether v0.2.0 is visually accepted, needs grammar retune, or should take the fallback grammar. | Plan notes only | Human review of artifacts + stats/profile JSON | No implementation while unresolved |
| 6. Settings UI consolidation | Accepted morphology settings are exposed together in the hidden dev panel with impact dots, persistence, and Rust/TS settings contract updates. | `web/src/core/settings.ts`, `web/src/core/setting-metadata.ts`, `web/src/ui/dev-panel.ts`, Rust `VisualSettings` consumers if needed | `cd app/web && npm run typecheck`; targeted Rust tests if settings cross WASM | Serial UI/settings finisher |
| 7. Ship v0.2.0 | Version bump and doc migration for the accepted shape/settings/profile surface. | Cargo/package metadata, owning docs | `cargo test`, `morph_view`, `render_check`, `npm run typecheck` | Serial doc/version finisher |
| 8. Optional v0.2.1 | Tune only residual dendrite/socket/brightness issues from accepted v0.2.0 artifacts and config snapshots. | `morphology.rs`, `morph_view.rs`, docs/version; UI defaults/ranges only if needed | Same camera set as v0.2.0 plus narrow tests | Serial tuning owner |

## Handoff Rules

Every implementation agent should report:

- Files changed.
- The exact gate command and result.
- Artifact paths when a visual gate ran, including whether the gate passed,
  failed, or skipped due to adapter/runtime availability.
- The morphology config snapshot and stats/profile JSON path for any visual
  artifact.
- Any new/changed parameter classification (`generator-default`,
  `review-override`, `dev-panel-candidate`, or `protected`).
- Any contract fallback taken, especially the shared-root fallback or
  terminal-only upstream-lighting consequence.
- Anything deferred to [`future_roadmap.md`](future_roadmap.md).
- The tracker row or artifact-ledger row that should be updated from the report.

Use this precedence order when facts disagree:

1. Current Rust/WGSL implementation and tests.
2. Human review decisions and observed outcomes recorded in this orchestration
   hub.
3. The leaf plan for the active wave.
4. Architecture/decisions docs.
5. The static roadmap.

## Stop Conditions

- If source-type target coverage does not match production connectivity, do not
  start branch grammar work.
- If the config/profile surface is not in place, do not start source-type or
  branch grammar implementation.
- If the branch grammar makes `morph_view` worse than the current visual, retune
  v0.2.0 before starting v0.2.1.
- If fixing upstream lighting requires shader/layout changes, defer it.
- If a proposed tuning change needs a new setting, add it to the morphology
  config surface first and classify it. Do not add a one-off dev-panel control
  before the UI consolidation wave.
- If profiling a candidate would require hot-loop per-synapse counters or
  synchronous readback, redesign the metric as artifact-only or defer it.

## Exit gate

- v0.2.0 leaf plan is `shipped + okay_to_delete: true`, or explicitly
  abandoned with useful context migrated.
- Accepted v0.2.0 artifacts include config snapshots and stats/profile JSON.
- Hidden dev-panel Morphology settings are consolidated, or a decision log entry
  explains why a candidate setting stayed review-only/protected.
- v0.2.1 is either shipped as a narrow patch, skipped, or abandoned.
- Owning docs reflect the final shipped morphology shape.
- Deferred ideas have been recorded in [`future_roadmap.md`](future_roadmap.md).

## Migration notes (filled in at ship time)

Route durable content into:

- `architecture/manifold.md` - final parameter surface, generation grammar,
  source-type input, sockets, cap formula, and coverage contract.
- `architecture/gpu-rendering.md` - final `target_id` and lighting semantics.
- `architecture/dev-panel.md` - final hidden Morphology settings group and
  impact/persistence/index contract.
- `architecture/profiling.md` - morphology build/profile artifact stats and
  their boundary with always-on runtime metrics.
- `decisions/manifold.md` - why shared arbors/sockets/Bezier were chosen.
- `decisions/rendering.md` - why terminal-only upstream lighting is accepted for
  shared paths, if that remains true.
- `decisions/dev-tooling.md` / `decisions/profiling.md` - any durable rationale
  introduced by the settings or profiling shape.

## See also

- [`procedural-cell-rework.md`](procedural-cell-rework.md) - active roadmap.
- [`procedural-cell-rework-0.2.0-axon-arbor.md`](procedural-cell-rework-0.2.0-axon-arbor.md)
- [`procedural-cell-rework-0.2.1-dendrite-tuning.md`](procedural-cell-rework-0.2.1-dendrite-tuning.md)
- [`future_roadmap.md`](future_roadmap.md)
- [`../agent-context/orchestrating.md`](../agent-context/orchestrating.md)
