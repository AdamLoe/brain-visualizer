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
| 0 | Baseline recon | Not started | Current docs are draft/untracked in the working tree. | Confirm dirty state, current versions, current `generate()` signature, and baseline artifact behavior. | None |
| 1 | Config/profile foundation | Not started | Plan now requires `MorphologyParams`-style config and JSON stats before grammar work. | Add config object, default preset, artifact snapshot, and build/profile stats. | Stream 0 |
| 2 | Source-type preflight | Not started | Plan expects morphology target resolution to stop using fixed `0u8`. | Implement or verify source-type input through the config/stat path, then run the narrow morphology gate. | Stream 1 |
| 3 | Branch grammar | Not started | Contract: shared arbor, visible dendrite sockets, one terminal per unique non-self target. | Start after Stream 2 coverage test is green, using only the named config surface for tuning. | Stream 2 |
| 4 | Artifact harness | Not started | Needs baseline/candidate frames, config snapshots, and stats JSON. | Add or verify artifact output after branch grammar exists. | Stream 3 |
| 5 | Visual review | Not started | Human acceptance is required. | Review fixed camera set, config snapshot, and stats. | Stream 4 |
| 6 | Settings UI consolidation | Not started | Hidden dev-panel Morphology settings group waits for accepted defaults. | Promote accepted settings together; update impact metadata, persistence, and Rust/TS boundary. | Stream 5 |
| 7 | Ship v0.2.0 | Not started | Version/docs migration wait for accepted shape and UI consolidation. | Bump versions and update owning docs after review. | Stream 6 |
| 8 | v0.2.1 tuning | Not started | Starts from accepted v0.2.0 artifacts, config snapshots, and a written issue list. | Decide whether the patch is needed after v0.2.0. | Stream 7 |

## Artifact Ledger

Record exact outputs here. A skipped gate is an observed result, not a pass.
Every visual artifact row must include the morphology config snapshot used to
produce it.

| Version | Artifact set | Command | Output paths | Config snapshot | Stats/profile path | Review decision |
|---|---|---|---|---|---|---|
| baseline | Current morphology | TBD | TBD | TBD | TBD | TBD |
| 0.2.0 candidate | Shared arbor | TBD | TBD | TBD | TBD | TBD |
| 0.2.1 candidate | Tuning patch, if used | TBD | TBD | TBD | TBD | TBD |

## Decision Log

| Decision | Why | Source |
|---|---|---|
| TBD | TBD | TBD |

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
