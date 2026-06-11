---
status:        active
owner:         orchestrator
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/connectivity.md
  - architecture/dev-panel.md
  - architecture/gpu-rendering.md
  - architecture/manifold.md
  - architecture/web-frontend.md
  - decisions/connectivity.md
  - decisions/dev-tooling.md
  - decisions/manifold.md
  - decisions/rendering.md
---

# Morphology visual refresh hub

## Mission

Coordinate the current morphology visual-refresh plans as one effort without
letting independent agents collide in the morphology generator, shader layouts,
or dev-panel persistence contracts. Done when the feature plans have either
shipped with migrated architecture/decision docs or have explicit deferrals.

Current wave: coordinate the three corrective plans
`active-opacity-continuous-model.md`, `dendrite-geometry-fix.md`, and
`dev-panel-and-settings-overhaul.md`. These supersede parts of the earlier
visual-refresh wave and should ship together only after opacity, dendrite
geometry, and boot/panel settings are all verified against the same runtime
state.

## Phase Tracker

| Phase | Status | Notes |
|---|---|---|
| 0. Bootstrap | Done | Hub created from the five draft plans on 2026-06-09. |
| 1. Recon and collision map | Done | Zeno reported current layouts, root/socket recommendation, stats gaps, and collision map. |
| 2. Independent opacity stream | Code done | Rawls implemented TS descriptors/persistence, segment-overlap alpha, and docs; `npm run typecheck` and `cargo test -p brain-visualizer` passed. |
| 3. Root/socket contract | Host foundation done | Wegener added `ProcessRoot`, descriptor-backed single-target trunk roots, and p99/max per-neuron budget stats; cargo gates passed. |
| 4. Axon + soma geometry wave | Code done | Archimedes completed trunk-dominant soma deformation with 48 B `MorphSphereInstance`; cargo, `render_check`, and `morph_view` passed. |
| 5. Incoming dendrite wave | Code done | Hilbert implemented v1 reverse incoming sockets, target-owned dendrite aggregation, stats, and minimal tube activity semantics; focused gates and full Rust package test passed. |
| 6. Consolidated verification | Passed with environment skips | Newton reported `cargo test`, web typecheck, web unit tests, `render_check`, and `morph_view` passed. Curie fixed stale e2e expectations and reran server-backed Playwright: 4 passed, 1 skipped for intentionally hidden CPU backend UI. |
| 7. Doc migration and cleanup | In progress | Read-only recon (opus) confirmed all five streams' code is present and durable facts are genuinely migrated into owning docs. Two drift gaps found and fixed: TS `axonRootRadiusFraction` default now matches Rust `AXON_R0_FRACTION = 0.90`, and manifest MorphSphereInstance now records 48 B; `npm run typecheck` green. Artifacts regenerated (5 PNGs in /tmp/morph_png). Awaiting visual/aesthetic acceptance before marking plans shipped. |
| 8. Corrective-wave bootstrap | Done | User asked to orchestrate `active-opacity-continuous-model.md`, `dendrite-geometry-fix.md`, and `dev-panel-and-settings-overhaul.md` as one wave on 2026-06-09; hub updated to track the wave. |
| 9. Corrective implementation wave 1 | Code done | Boole completed active-opacity shader/pass behavior; Galileo completed web settings/dev-panel boot/control behavior. |
| 10. Corrective implementation wave 2 | Code done | Gibbs completed geometry-only dendrite fixes in `morphology.rs`; Volta removed the legacy dendrite reach/primary controls from TS/Rust config/descriptors/docs while preserving old saved-config compatibility. |
| 11. Corrective doc migration | Docs done, pending visual/reload acceptance | Durable docs updated for active-opacity, dendrite geometry, dev-panel/settings facts, and legacy dendrite-control removal. Archimedes reported cargo/npm/example gates passed, with e2e blocked by read-only filesystem during setup. Meitner reran `render_check` and `morph_view` after cleanup; both passed. Feature plans remain active because visual and manual reload acceptance are still open. |

## Stream Tracker

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Active opacity | Dev-panel persistence + active alpha shader | Code done, pending final visual gate | Rawls added `lighting.activeOpacity` / `lighting.inactiveOpacityFloor`, changed tube alpha to segment interval overlap, updated docs, and reported `npm run typecheck` + `cargo test -p brain-visualizer` passed. | Include in final visual/browser artifact pass before marking plan shipped. | Manual visual verification deferred. |
| Root/socket contract | Shared process-root descriptors, soma data path, budget gate | Host foundation done | Wegener added one `ProcessRoot` per source neuron, `Morphology::process_roots`, descriptor-backed root/first-fork use, and `neuron_count`/`fanout_k`/per-neuron p99/max stats in JSON. `cargo test -p brain-visualizer` passed; focused `sim::morphology` rerun passed after a comment-only correction. | Let axon and soma consume the descriptor; defer incoming socket records to dendrite wave. | Incoming activity semantics remain later decision. |
| Axon trunk + curved branches | Morphology generator and tube shader | Code done, pending visual review | Epicurus raised trunk radius fraction to `0.90`, protected descriptor trunk/fork behavior, tapered terminal leaves, kept `MorphSegment` 48 B, added tests, and reported `cargo test -p brain-visualizer` plus `morph_view` passed. Default stats: N=1200/K=16, 71,829 segments, p99=78, max=87, cap=199,200, dropped=0. | Include `/tmp/morph_0.rgba`-`/tmp/morph_3.rgba` in final visual review. | Subjective trunk readability not yet approved. |
| Organic soma | Soma instance layout and sphere shader | Code done, pending visual review | Archimedes widened `MorphSphereInstance` to 48 B (`root_dir` + `root_pull`), deforms `vs_sphere` toward the dominant root, kept `MorphSegment` unchanged, and reported cargo, `render_check`, and `morph_view` passed. | Include `/tmp/morph_*.rgba` and `/tmp/morph_active_bright.rgba` in final visual review. | Top-few-root deformation and true multi-root/tube-side blending deferred. |
| Real incoming dendrites | Reverse synapse build + target-owned dendrite arbor | Code done, pending visual review | Hilbert added raw incoming sockets/ranges, unique socket groups, target-owned dendrite aggregation, incoming stats, and a minimal WGSL activity owner branch. Default stats: raw incoming 17,850; unique groups 13,010; in-degree mean/p99/max 14.875/49/86; visible groups mean/p99/max 10.841666/29/46; incoming capped/dropped 0/0; total segments 80,823; cap 199,200; per-neuron p99/max 108/157; dropped 0; incoming storage 1,130,000 B. Focused morphology, target determinism, `morph_view`, and full Rust package tests passed. | Include artifacts in final visual review and run consolidated gates. | Shared aggregate stems intentionally do not presynaptically pulse in v1. |
| Consolidated gates | Cargo/npm/examples/artifacts | Passed with environment skips | Newton reported `cargo test` passed with 93 lib tests plus integration/determinism tests, `npm run typecheck` passed, `npm test` passed with 3 files / 26 tests, `render_check` passed with 717,099 segments for 5,000 neurons and 0 dropped, and `morph_view` passed. `/tmp/morph_view_stats.json` exists with N=1200/K=16, 80,823 segments, cap 199,200, dropped 0, incoming raw/groups 17,850/13,010, incoming capped/dropped 0/0, p99/max segments 108/157. RGBA artifacts exist and are nonzero. Curie fixed stale e2e expectations and reported `npm run typecheck` passed, `npm test` passed, and escalated `npm run test:e2e:server` passed with 4 passed / 1 skipped. | Final visual/aesthetic approval of generated morphology artifacts, then mark plans shipped. | WebGPU-device assertions skipped due unavailable adapter; CPU backend e2e skipped because public backend toggle is intentionally hidden in V2. |
| Continuous active opacity | Shader active alpha + active-pass guard | Code/docs done, pending visual acceptance | Boole changed `render_morphology.wgsl`, `sim/gpu/mod.rs`, and `examples/render_check.rs`. Tube active alpha now uses continuous segment proximity from `inactive_opacity_floor` to an active ceiling; brightness remains fragment-local so the packet travels. `active_opacity = 0` stays encoded and maps to a soft low-emphasis ceiling of `0.10`; somas use the same zero-end ceiling. Reported `render_check` passed with active-opacity diffs/deltas and full `cargo test -p brain-visualizer` passed; Meitner reran `render_check` after cleanup and it passed with the same active-opacity stats. | User/manual visual acceptance of opacity behavior before plan closure. | No browser slider capture in this environment; `render_check` numeric gate passed. |
| Dendrite geometry fix | Incoming dendrite generation + parameter decision | Code/docs done, pending visual acceptance | Gibbs made incoming branch points reach off-soma using weighted socket distance/max extent/lateral spread with a `1.65 * base_radius` floor, thickened default taper (`mid 0.95`, `tip 0.60`), and raised leaf weight floor to `0.75`. Segment count/cap semantics unchanged: `segment_count=80823`, `segment_cap=199200`, `dropped_count=0`, incoming raw/groups `17850/13010`, incoming capped/dropped `0/0`. Volta removed the legacy reach/primary-count controls from Rust/TS descriptors/docs; old persisted morphology payloads are normalized/ignored. Meitner reran `morph_view` after cleanup and it passed. | User/manual visual acceptance of `/tmp/morph_png/*.png`, then plan closure. | Shader color intentionally deferred. |
| Dev panel/settings overhaul | Boot apply, unified controls, defaults, dead settings | Code/docs done, pending manual reload acceptance | Galileo fixed boot morph-config push, removed the Network tab build-then-rebuild path, moved rendering/morphology numeric controls to slider + number input + reset + tooltip, fixed descriptor/default drift for `axonRootRadiusFraction = 0.90`, and tombstoned `signalSource`/`adaptiveScalerEnabled` without Float32Array renumbering. Volta removed legacy dendrite reach/primary controls. Web gates passed before and after cleanup. | Manual browser reload/backend acceptance before plan closure. | `surface`/`surfaceOpacity` and legacy neuron-body knobs intentionally left intact; Playwright server blocked by environment. |
| Corrective consolidated gates | Cargo/npm/examples/artifacts/e2e | Passed with e2e environment block | Archimedes reported `cargo test` passed (96 total: 93 lib, 3 integration, 0 doctests), `npm run typecheck` passed, `npm test` passed (3 files / 28 tests), `render_check` passed, and `morph_view` passed. Volta then reported `npm run typecheck`, `npm test` (3 files / 29 tests), and `cargo test -p brain-visualizer` passed after legacy-control cleanup. Meitner reran `render_check` and `morph_view`: both passed; active-opacity diffs remained `42031/262144` and `49935/262144`, and `morph_view` wrote `/tmp/morph_0.rgba` through `/tmp/morph_3.rgba` plus `/tmp/morph_active_bright.rgba`. | No implementation follow-up from gates. | `npm run test:e2e:server` blocked before tests started by read-only filesystem during `wasm-bindgen` setup; escalation was rejected. |

## Sequencing Rules

- Active-opacity work may run in parallel with root/socket recon because it owns
  mostly dev-panel config and active fragment alpha behavior.
- Only one implementation agent may own `crates/brain-visualizer/src/sim/morphology.rs`
  at a time unless the work is explicitly paired as one stream.
- Only one implementation agent may own
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl` at a time.
- Per-stream gates stay narrow. The full drift gates run once at the end.
- In the corrective wave, `dev-panel-and-settings-overhaul.md` may run in
  parallel with `active-opacity-continuous-model.md` if the workers respect
  file fences: web settings/panel files vs shader/pass-guard files.
- Do not run dendrite geometry concurrently with any other worker that edits
  `crates/brain-visualizer/src/sim/morphology.rs`. If dendrite color changes
  require `render_morphology.wgsl`, sequence them after active-opacity lands.
- The dendrite/dev-panel shared legacy-control decision has landed: remove the
  old reach/primary-count controls and keep socket count/radius/tip-preference
  as the live target-owned incoming dendrite vocabulary.

## Decisions Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-06-09 | Treat `morphology-process-root-contract.md` as the prerequisite coordination stream, not as an optional feature. | Axon trunk, organic soma, and incoming dendrites all need one root/socket convention and budget artifact. |
| 2026-06-09 | Keep active-opacity as an independent first implementation stream. | It does not depend on the root/socket geometry contract and fixes existing user-visible controls/persistence. |
| 2026-06-09 | Defer real incoming dendrites until after the root/socket contract and initial geometry wave. | It introduces reverse connectivity, variable in-degree, cap policy, and segment activity semantics; it should not collide with first-pass layout work. |
| 2026-06-09 | First root/socket implementation should be host-side: one `ProcessRoot` per source neuron plus per-neuron segment-budget stats. | It unblocks axon/soma sequencing while avoiding the shared `render_morphology.wgsl` file that the active-opacity worker currently owns. |
| 2026-06-09 | Single-target axons should get the same trunk/first-fork convention as fan-out axons. | This removes a special case before soma deformation consumes the dominant root descriptor. |
| 2026-06-09 | Soma first pass should use trunk-only deformation with one dominant direction/strength. | A compact 48 B `MorphSphereInstance` is lower risk than a new bind-group path; top-few-root deformation can follow visual review. |
| 2026-06-09 | Incoming dendrite v1 keeps shared aggregate stems structurally target-owned and non-presynaptically active. | Existing `MorphSegment` can honestly pulse source-specific terminal leaves via `target_id`, but a shared stem has multiple presynaptic sources and needs a future side channel if it must pulse. |
| 2026-06-09 | Incoming dendrite v1 draws all unique incoming socket groups at current N=1200/K=16; if too dense, lower K before introducing hidden visual drops. | This preserves the plan's correctness-first constraint and makes any density/scaling tradeoff explicit. |
| 2026-06-09 | Fixed TS `axonRootRadiusFraction` default to match the Rust source of truth. | Obvious-default bug: the TS config is pushed to Rust via `set_morphology_config`, so the stale TS default was silently overriding the axon-trunk plan's headline 0.90 trunk in every web session. Rust assert + decisions/manifold.md both say 0.90. |
| 2026-06-09 | Fixed stale manifest drift-verification note `MorphSphereInstance 32 B → 48 B`. | The struct was widened 32→48 B by the organic-soma stream (added `root_dir`/`root_pull`); the high-risk-surface checklist a future doc-sweep verifies against was still claiming the old size. |
| 2026-06-09 | Treat the active-opacity continuous model, dendrite geometry fix, and dev-panel/settings overhaul as one corrective wave. | They share visual acceptance and settings contracts; shipping one without the others can leave persisted controls lying about the runtime state. |
| 2026-06-09 | First corrective wave can parallelize active-opacity and dev-panel work, but dendrite geometry waits. | Active-opacity owns shader/pass-guard files, dev-panel owns web UI/config files, while dendrites own `morphology.rs` and the shared dendrite-control decision. |
| 2026-06-09 | Default dead-settings strategy is tombstone, not Float32Array renumber, unless a worker proves the cleanup value justifies the contract risk. | The VisualSettings Float32Array index contract is a documented corruption risk and Rust already tolerates reserved/inert slots. |
| 2026-06-09 | Do not remove the legacy dendrite reach/primary-count controls in the dev-panel stream before the dendrite geometry stream decides whether to revive them. | The dendrite plan may make those controls meaningful; premature UI removal would force rework or hide required tuning. |
| 2026-06-09 | Treat the plan's old dendrite stem-control-point claim as stale unless implementation finds a concrete current-code kink. | Read-only recon found the current stem control direction is conventional for the emitted segment; collapsed placement and thin radii better explain the bad shape. |
| 2026-06-09 | Remove legacy dendrite reach/primary-count controls after geometry lands, not revive them. | The current target-owned generator uses socket count/radius/tip-preference controls for the same concepts; reviving the old controls would duplicate or conflict with live parameters. |

## Open Questions

- Combined morphology budget: what are the current default N/K segment counts,
  p99/max per-neuron segments, cap, and dropped count before adding axon curves
  or real incoming dendrites?
- Soma strength: should the first pass use a subtle ovoid stretch or a visibly
  pulled shape?
- Incoming dendrite artifact result: can draw-all unique socket groups stay under
  cap with `Morphology::dropped == 0` at N=1200/K=16?
- Dendrite parameters: closed. Legacy reach/primary-count controls were removed
  from Rust/config/docs after geometry landed; no product reason to keep
  duplicate controls was identified.
- Dev panel information architecture: final section names can be decided by the
  worker, but must reflect tuning workflows rather than implementation history.

## Exit Gate

- Each stream reports files changed, the exact gate command and result, and any
  deferrals.
- Final consolidated gates: `cargo test`, `npm run typecheck`, `npm test`, and
  `npm run test:e2e` where applicable to the final browser state.
- Visual artifacts from `examples/morph_view.rs` and `examples/render_check.rs`
  cover the accepted close-up and opacity behaviors.
- Durable facts are migrated into the owning architecture/decision docs before
  any feature plan is marked `shipped + okay_to_delete: true`.

## Migration Notes

Fill at ship time. Route current-state facts into architecture docs and durable
trade-offs into decisions docs listed in frontmatter.

## See also

- `docs/plans/active-opacity-controls-and-solid-firing.md`
- `docs/plans/active-opacity-continuous-model.md`
- `docs/plans/dendrite-geometry-fix.md`
- `docs/plans/dev-panel-and-settings-overhaul.md`
- `docs/plans/morphology-process-root-contract.md`
- `docs/plans/axon-trunk-and-root-like-branches.md`
- `docs/plans/organic-soma-redesign.md`
- `docs/plans/dendrites-real-incoming-synapses.md`
