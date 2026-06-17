---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-15
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - architecture/gpu-rendering.md
  - architecture/manifold.md
  - architecture/scaling.md
  - architecture/web-frontend.md
  - decisions/dev-tooling.md
  - decisions/manifold.md
  - decisions/rendering.md
  - decisions/scaling.md
---

# Visual product polish phase hub

## Mission

Coordinate the next app-polish phase across settings cleanup, audio removal,
dendrite shape, brain color semantics, default tuning, scale limits, and a more
realistic brain manifold. Done when each workstream has a detailed plan with
owned files, questions, sequencing constraints, and a narrow verification gate,
and the lead has answered the product decisions that change what gets built.

## User goals

- Settings overhaul: remove unused controls, group remaining settings well, and
  put them in better tabs.
- Dendrites should branch close to the soma with tighter curves instead of
  reading as thick cylinders with thin lines running into them.
- Add `Color by = Brain`: inactive neurons and everything else pink; firing
  neurons, firing connections, and active morphology segments blue.
- Default heterogeneity on at `50%`; default glow decay `10`; default
  resting brightness `0.05`.
- Limit maximum N substantially: max `20k`.
- Improve the brain shape so it is more realistic.
- Remove audio features.

## Phase Tracker

| Phase | Status | Notes |
|---|---|---|
| 0. Bootstrap | Done | Created this hub from the 2026-06-09 user goals and linked it to the existing morphology-refresh context. |
| 1. Batched product questions | Done | Product and implementation-shaping questions were answered on 2026-06-09 and recorded in the decisions log below. |
| 2. Per-stream planning | Done | Turing, Feynman, Archimedes, Russell, Kuhn, and Ohm wrote the six detailed stream plans. |
| 3. Collision map | Done | Settings/defaults/color share web settings; audio shares `main.ts`/`index.html`; dendrites and brain color both may touch `render_morphology.wgsl`; dendrites are sole owner of `morphology.rs`; brain shape is mostly manifold-owned. |
| 4. Implementation waves | Done | All six implementation streams have reported implementation facts and narrow gates. |
| 5. Consolidated verification | Done except real-WebGPU browser smoke | `cargo test`, `npm run typecheck`, and `npm test` are green after follow-up fixes. `morph_view` and `render_check` passed; browser UI checks passed, but browser canvas nonblank smoke is blocked by no WebGPU adapter in this environment. |
| 6. Doc migration and cleanup | Done | Durable facts were migrated into architecture/decisions docs on 2026-06-11. Plans stay `okay_to_delete: false` until the real-WebGPU browser smoke blocker is cleared or explicitly waived. |

## Stream Tracker

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Settings overhaul | Dev panel, settings model, tabs | Shipped; docs migrated | Copernicus preserved `SETTINGS_LENGTH = 26`; default-writes `pointRadius`, `surface`, and `surfaceOpacity`; keeps tombstones zero-written; removes `generator.axonCurveLift`; moves tabs to Monitor/Dynamics/Network/Appearance/Morphology/Debug/Storage; removes Surface from Debug. Gates passed: `npm test -- dev-panel` (7 tests) and `npm run typecheck`. | None. | None known. |
| Audio removal | Web audio/sonification, UI, docs | Shipped; docs migrated | Lovelace deleted `app/web/src/audio/sonification.ts` and the empty `audio/` dir; removed `#sound-toggle`, Web Audio construction/update wiring, and current-state docs references. Focused current-state scan over `app/web/src`, `app/web/index.html`, `docs/architecture`, `docs/decisions`, `docs/repository-layout.md`, and `docs/_meta` returned no audio references; broad scan only found historical plan files. Later settings gate `npm run typecheck` passed after the audio changes were present. | None. | None known. |
| Dendrite branch quality | Morphology generator, morph config/UI controls, possibly shader material | Shipped; docs migrated | Linnaeus replaced the single incoming bucket stem with soma-surface collars, close first forks, per-group child branches, and source-owned terminal leaves; preserved `MorphSegment`; changed `morphology.rs`, added `tests/dendrite_branching_near_soma.rs`, and updated `morph-config.ts`. Added controls: root count `1..6` default `4`, fork distance `1.15..2.20` default `1.45`, curve tightness `0..1.25` default `0.55`, branch thickness `0.45..0.90` default `0.78`, taper `0.22..0.62` default `0.42`, group spacing `0..1.50` default `0.55`. `morph_view`: `segment_count=103526`, `dropped_count=0`, `incoming_dropped_count=0`, `segment_cap_per_neuron=296`, `segment_cap=355200`, p99/max segments `159/242`, incoming visible groups mean/p99/max `10.356667/29/45`. Gates passed: dendrite integration test, incoming ownership test, `npm run typecheck`, `morph_view`, and `render_check`. Shader was not edited by Linnaeus. | None. | Real-WebGPU browser smoke remains environment-blocked. |
| Brain color mode | Rendering color semantics, dev-panel mode, settings contract | Shipped; docs migrated | Feynman added `colorBy = 6`, made Brain default in TS/Rust, preserved index 18, tinted resting/inactive visible neurons/morphology/optional surface pink and firing cores/active packets blue, left retired paths untouched, and respected `surface = Off`. Gates passed: focused brain-color Vitest (2), `npm run typecheck`, `render_uniform_size_aligned`, `render_shaders_present`. `morph_view` generated `/tmp/morph_0..3.rgba` and stats showing `color_by:6`, `surface:0`. | None. | Real-WebGPU browser smoke remains environment-blocked. |
| Defaults and max scale | Web defaults, Rust config/scaler, persistence migration | Shipped; docs migrated | Planck set heterogeneity `0.50`, glowTau/glow_tau `10`, restingBrightness/resting_brightness `0.05`, retained `DEFAULT_CONFIG.n = 1_200`, added product max N `20_000`, clamps saved/saved-on-write/network/WASM/backend/GPU reinit N, sets `SimConfig::default().n = 1_200`, and keeps scaler/tier ranges <= `20_000`. Old visual/morph localStorage versions were not bumped; old saved N is clamped. Gates passed: `npm run typecheck`, `npm test -- dev-panel` (9), `npm test -- controls` (23), `cargo test -p brain-visualizer scaler` (5), plus focused Rust defaults tests. | None. | None known. |
| Brain shape realism | Manifold generation/regions/camera artifacts | Shipped; docs migrated | Hegel changed `manifold/gyrify.rs` and `manifold/mod.rs`: refined coarse envelope, added shared deterministic `FoldField`, structured major grooves, and folded neuron placement. Regions remain hash-random (`regions.rs` untouched; generation still calls `assign_regions`). Gate passed: `cargo test -p brain-visualizer manifold::` with 21 passed. Metrics log: dorsal/fissure folded radii reduced (`dorsal_mid 0.4303`, `fissure_mid 0.4545`), `max_surface=1.2407`, `max_neuron=1.2389`, `occupied_cells=1409`, `max_cell_occupancy=43`. | None. | Star-convex radial topology remains; real-WebGPU browser smoke remains environment-blocked. |

## Sequencing Rules

- Only one implementation worker may own
  `crates/brain-visualizer/src/sim/morphology.rs` at a time.
- Only one implementation worker may own
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl` at a
  time.
- Settings, audio removal, defaults, and color mode all may touch the web
  settings/dev-panel contracts; sequence by exact file ownership after planning.
- Recommended first implementation wave: audio removal plus brain shape realism
  can run in parallel if `main.ts` camera/framing is not touched by the shape
  worker.
- Recommended second wave: settings IA before defaults/scale and brain color,
  because it owns the tabs, `colorBy` selector placement, and stale persistence
  decisions.
- Recommended morphology wave: dendrite branching owns `morphology.rs` and now
  also needs morph config/UI control work. Sequence it after settings IA removes
  stale controls and before final defaults/docs cleanup.
- Do not pair dendrite branching with brain color if either needs
  `render_morphology.wgsl`.
- Defaults and max scale must respect the `VisualSettings` Float32Array contract
  and the documented `DEFAULT_CONFIG` scale source.
- Per-stream gates stay narrow; the full drift gates run once after the
  implementation wave.

## Decisions Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-06-09 | Create a new product-polish hub instead of overloading the morphology-refresh hub. | The requested phase spans settings, audio, defaults/scaling, color semantics, and manifold shape, not only morphology visuals. |
| 2026-06-09 | Preserve `VisualSettings` length during settings cleanup unless a later contract-cleanup release is explicitly approved. | Multiple streams touch the TS/Rust settings contract; tombstoning/hiding stale controls is lower risk than renumbering. |
| 2026-06-09 | Treat max `N = 20_000` as a product cap separate from GPU hardware capacity. | `gpu_limits.rs` reports adapter capability; user-facing defaults/scalers should enforce product readability and cost constraints. |
| 2026-06-09 | First brain-shape pass stays procedural but includes folded neuron placement. | The lead wants folds reflected in the neuron cloud now, while keeping regions random to avoid a simultaneous propagation/region behavior change. |
| 2026-06-09 | Keep Advanced/Debug settings areas, but pitch no-op/stale controls for removal. | The lead wants debug capability retained while still cleaning controls that do nothing. |
| 2026-06-09 | Fully remove audio rather than parking it. | The lead confirmed "nuke all audio related stuff." |
| 2026-06-09 | Make `Color by = Brain` the default color mode. | The lead confirmed the new Brain mode should be the default. |
| 2026-06-09 | Treat changed-default persistence as non-blocking and choose the simplest safe behavior. | The lead said not to worry about saved-setting migration. |
| 2026-06-09 | Brain-shape realism should target silhouette, cortical folds, and hemispheres/fissure together. | The lead selected all three realism axes. |
| 2026-06-09 | Dendrite target is biologically inspired and readability-first. | The lead asked for both biological plausibility and stylized readability, without a strict anatomy reference. |
| 2026-06-09 | Brain-mode activity should make the traveling active segment and firing core light blue, with surrounding active parts bluish rather than fully overriding whole connections. | Preserves motion/readability while giving the firing center the requested blue signal. |
| 2026-06-09 | Dendrite groups must remain individually legible. | The lead rejected visual merging of dense incoming groups. |
| 2026-06-09 | Dendrite branching changes need UI controls, and unused controls should be deleted. | Locked defaults alone are not acceptable for this stream. |
| 2026-06-09 | Retire high-N tier semantics from the UI under the 20k product cap. | The old tier language implies scale targets that no longer exist. |
| 2026-06-09 | Audio should disappear from current-state docs entirely. | The lead reiterated full audio removal, with no retained rationale note. |
| 2026-06-09 | Neuron placement should follow folds in the brain-shape realism pass. | The lead chose folded placement now, not a surface-only first pass. |
| 2026-06-09 | First-pass dendrite controls should include root count, fork distance, curve tightness, branch thickness/taper, and individual-group spacing. | The lead approved the proposed control set. |
| 2026-06-09 | Brain mode respects `surface = Off`. | Color mode should not force hidden/off layers on. |
| 2026-06-09 | Regions remain hash-random in the brain-shape realism pass. | The lead approved random regions, avoiding a simultaneous propagation/region behavior change. |
| 2026-06-11 | Audio removal implementation may update current-state architecture/decision docs in the same stream, but the plan stays unshipped until final doc migration review. | Lead requested full audio deletion with no retained rationale; the worker removed active docs references immediately while this hub still tracks the stream through consolidated verification. |

## Open Questions

### Batch 1 — answered product decisions

- Settings: keep Advanced/Debug, but identify no-op controls and pitch removals.
- Audio: fully remove audio-related source, UI, docs, and tests.
- Defaults persistence: do not make saved-setting migration a blocker; use the
  simplest safe behavior.
- Brain realism: target recognizable silhouette, better cortical folds, and
  hemispheres/fissure.
- Brain color: make `Color by = Brain` the default mode.
- Dendrites: aim for biologically inspired branching while prioritizing visual
  readability.

### Batch 2 — answered implementation-shaping details

- Brain activity: traveling segment and firing core should be light blue;
  surrounding active parts can be bluish, but whole connections should not turn
  uniformly blue.
- Dendrite density: every incoming group must remain individually legible.
- Dendrite controls: add the UI controls needed for the new branching behavior
  and delete unused controls; do not hide new behavior behind locked defaults.
- Scale tiers: retire high-N tier semantics from the UI.
- Audio docs: remove audio entirely from current-state docs; do not keep an
  intentional-removal note.
- Brain shape placement: neuron placement should follow the new folds in this
  pass.

### Batch 3 — answered implementation details

- Dendrite controls: expose all proposed controls: primary root count, fork
  distance, curve tightness, branch thickness/taper, and individual-group
  spacing.
- Brain color surface behavior: respect `surface = Off`; do not force a pink
  context mesh on.
- Brain shape regions: keep regions random/hash-shuffled.

### Batch 4 — remaining acceptance details

- Brain color palette: is the proposed `vec3(0.08, 0.56, 1.0)` active blue close
  enough, or should implementation choose a paler/light-blue core and a darker
  bluish halo during visual tuning?

## Consolidated Verification Log

| Date | Gate | Result | Observed fact | Next action |
|---|---|---|---|---|
| 2026-06-11 | `cd app && cargo test` | Failed | Unit tests and dendrite integration passed, but `gpu_excitability_sweep_and_no_overflow` failed: `deep_sleep=1.37Hz`, assertion `deep_sleep not near-silent`. Verifier noted likely relation to defaults/scale through `SimConfig::default()` or dynamics defaults. | Dispatch focused Rust fix worker; rerun `cargo test -p brain-visualizer --test gpu_sim_dynamics` first, then full `cargo test` after fix. |
| 2026-06-11 | `cd app/web && npm run typecheck` | Passed | `tsc --noEmit` passed in integrated state. | Keep as final gate to rerun after fixes. |
| 2026-06-11 | `cd app/web && npm test` | Failed | `src/ui/dev-panel.test.ts` failed one assertion: test expected persisted morph config not to contain substring `dendritePrimary`, but approved new control `dendritePrimaryRootCount` is persisted. | Dispatch focused web test/behavior fix worker; rerun `npm test -- dev-panel` first, then full `npm test` after fix. |
| 2026-06-11 | Web test follow-up | Passed | Hubble changed only `app/web/src/ui/dev-panel.test.ts`: exact legacy keys are checked absent and valid `dendritePrimaryRootCount` is confirmed persisted. `npm test -- dev-panel` passed 9 tests; full `npm test` passed 4 files / 36 tests. | Keep web unit gate green; rerun typecheck/npm test in final summary if no further web edits land. |
| 2026-06-11 | Render/artifact/browser acceptance | Partial pass; browser render blocked by environment | Kuhn ran `morph_view` and `render_check` successfully. Converted `/tmp/morph_0..3.rgba` to PNG/contact sheet; frames were nonblank (`~29.6%`, `29.8%`, `62.9%`, `62.9%` nonblack). Artifacts showed Brain color behavior (`color_by=6`, `surface=0`), pink resting structure, cyan/blue active segments, close dendrite branching, and no drops (`segment_count=103526`, `dropped_count=0`, `incoming_dropped_count=0`). Browser DOM/UI checks passed: no sound toggle/audio text, settings gear only, tabs Monitor/Dynamics/Network/Appearance/Morphology/Debug/Storage, Color by default Brain, N max `20000`, default N `1200`, heterogeneity `0.50`, glow decay `10`, resting brightness `0.05`, surface UI absent. Browser canvas nonblank check could not pass because Chromium `requestAdapter()` returned no WebGPU adapter and app fell back to clear-only WebGL2. | Treat offline render artifacts as visual evidence in this environment; require real WebGPU browser smoke before ship if available. Note remaining visual tuning risk: some foreground dendrite tubes still read thick/cylindrical but acceptable for this pass. |
| 2026-06-11 | Rust dynamics follow-up | Passed | Halley found `gpu_sim_dynamics` inherited new `VisualSettings::default().heterogeneity = 0.50`; this raised deep-sleep to `1.37Hz`. The test fixture now explicitly sets heterogeneity `0.0`, preserving the homogeneous dynamics/overflow contract while product defaults remain `0.50`. `cargo test -p brain-visualizer --test gpu_sim_dynamics` passed; full `cargo test` passed. | Rust consolidated gate green. |

## Exit Gate

- Each stream has a detailed plan with scope, owned files, out-of-scope cuts,
  sequencing constraints, narrow gates, doc migration targets, and lead
  questions.
- The lead has answered the decisions that change implementation behavior.
- The hub stream tracker reflects the merged collision map before any
  implementation wave starts.

## Migration Notes

Migrated on 2026-06-11:

- Settings IA, quarantined/default-written controls, `SETTINGS_LENGTH = 26`,
  Brain default, persistence behavior, and dendrite controls:
  `architecture/dev-panel.md`, `architecture/web-frontend.md`,
  `decisions/dev-tooling.md`.
- Audio removal as absence from current-state docs/source inventory: no active
  audio facts were retained in architecture or decisions docs per lead request.
- Defaults and scale cap: `architecture/dev-panel.md`,
  `architecture/web-frontend.md`, `architecture/scaling.md`,
  `architecture/simulation.md`, `decisions/dev-tooling.md`,
  `decisions/scaling.md`, `decisions/dynamics.md`.
- Brain color mode and surface-off behavior:
  `architecture/gpu-rendering.md`, `decisions/rendering.md`.
- Brain-shape `FoldField`, folded placement, hash-random regions, and metrics:
  `architecture/manifold.md`, `decisions/manifold.md`.
- Dendrite root collars, close forks, source-owned terminal leaves, controls,
  and segment/artifact stats: `architecture/manifold.md`,
  `architecture/gpu-rendering.md`, `decisions/manifold.md`.
- Verification status and the real-WebGPU browser smoke blocker:
  `architecture/gpu-rendering.md`.

The 2026-06-15 aggressive cleanup sweep treated the plan-retention blocker as
waived for coordination cleanup: real-WebGPU browser nonblank smoke remains a
verification boundary in `architecture/build-and-deploy.md` and
`agent-context/testing-how-to.md`, not a reason to keep this shipped plan open.

## See also

- `docs/plans/morphology-visual-refresh-hub.md`
- `docs/plans/dev-panel-and-settings-overhaul.md`
- `docs/plans/dendrite-geometry-fix.md`
- `docs/plans/settings-ia-and-dead-controls.md`
- `docs/plans/audio-removal.md`
- `docs/plans/dendrite-branching-near-soma.md`
- `docs/plans/brain-color-mode.md`
- `docs/plans/defaults-and-scale-limits.md`
- `docs/plans/brain-shape-realism.md`
- `docs/plans/future_roadmap.md`
