---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - architecture/web-frontend.md
  - decisions/dev-tooling.md
---

# Settings IA and dead-controls cleanup

## Mission

Finish the settings-overhaul work that remains after the boot/apply and control
widget fixes. Done means the dev panel exposes only controls that a tuner can
trust, groups them by the job they perform, removes or quarantines controls with
no live effect, and preserves the Rust/TypeScript settings contracts unless the
lead explicitly accepts a contract-versioning pass.

This is a planning document only. The authoritative implementation facts are
the current code in:

- `app/web/src/core/settings.ts`
- `app/web/src/core/morph-config.ts`
- `app/web/src/ui/dev-panel.ts`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs` (`VisualSettings` and
  `set_visual_settings`)

Existing docs are secondary; the older
`docs/plans/dev-panel-and-settings-overhaul.md` is useful mostly because it
records which earlier work already shipped.

## Current shape

The broad control overhaul has already landed:

- `DevPanel` has six tabs: Monitor, Dynamics, Network, Rendering, Debug View,
  and Storage.
- Numeric controls now use one slider plus number-input helper with reset,
  impact dot, and tooltip behavior.
- Morphology config rows are generated from `MORPH_DESCRIPTORS`.
- Persisted visual settings, morphology config, and app config are pushed at
  boot by the already-closed settings overhaul stream.
- `signalSource` at Float32Array index 16 and `adaptiveScalerEnabled` at index
  23 are tombstoned to zero in `toFloat32Array`; they are not exposed in the
  panel or persisted in `SavedDev`.

Remaining problems are not primarily widget bugs. They are information
architecture, contract hygiene, and a few controls that still look live while
being stale, duplicated, hidden-but-persisted, or mislabeled.

## Control audit

### Keep as live public/dev controls

These controls have confirmed live effects and should remain, though several
should move to better groups:

- Dynamics/network: `iExt`, `synapticScale`, `heterogeneity`,
  `weightNormalization`, `inputMode`, `excitability`, `ticksPerSec`, `n`, `k`,
  and `seed`.
- Far-glow neuron body: `glowTau`, `neuronVisualRadius`,
  `activeNeuronRadiusBoost`, `inactiveNeuronOpacity`,
  `voltageGlowStrength`, `colorBy`, and `neuronVisibility`.
- Morphology layer: `connectionLayer`, `connectionLightNext`,
  `connectionVisualWidth`, `connectionCurveLift`,
  `longRangeReachFrac`, `maxReachCells`, and most non-duplicate
  `MORPH_DESCRIPTORS`.
- Morphology lighting/rendering: `lighting.*`, `renderQuality.*`, and
  generator controls other than the duplicate curve control described below.
- Post: `bloomStrength`.

Important correction: the Rendering tab currently labels
`neuronVisualRadius`, `activeNeuronRadiusBoost`, and
`inactiveNeuronOpacity` as "Neuron Body" with a stale "(applies in Phase E)"
caption. Rust writes these fields into `RenderUniforms`, and
`render_far.wgsl` uses them in the live billboard pass. They are not dead.

### Confirmed stale or no-op controls

- `pointRadius`: The panel exposes it and `toFloat32Array` writes index 1, but
  the live far shader sizes billboards from `neuron_visual_radius`. The old
  `point_radius` uniform still exists, and a legacy near-LOD path can derive a
  sphere radius from it, but near-LOD spheres are gated off by
  `DRAW_LEGACY_NEAR_SPHERES = false`. Treat the UI control as stale unless an
  implementation recon proves another live path still reads it.
- `generator.axonCurveLift`: `MORPH_DESCRIPTORS` exposes it, but Rust builds
  effective morphology params by applying `self.visual.connection_curve_lift`
  after `morph_config.to_params()`. Changing the morph descriptor can trigger a
  morphology rebuild while the generated geometry still uses
  `connectionCurveLift`. This should become one curve control, not two.

### Hidden but still persisted/debug-visible

- `surface` and `surfaceOpacity`: the Rendering tab removed surface controls,
  and default `surface = 0` skips the manifold pass. Rust still has a live
  optional surface path if `surface != 0`, and `surfaceOpacity` only matters in
  that path. Today both are still in the persisted public schema, and Debug View
  still shows Surface. That creates a hidden localStorage/debug path for a
  product feature that the UI intentionally removed.

### Existing tombstones to preserve

- `signalSource` index 16 and `adaptiveScalerEnabled` index 23 should stay as
  zero-written contract slots. Do not renumber the Float32Array as part of this
  stream.
- `connectionLightPast` index 9 is already reserved zero. Keep it out of the
  panel and out of persistence.

## Plan decisions

1. Preserve the 26-slot `VisualSettings` Float32Array in this workstream.
   Renumbering is unnecessary for product polish and would create avoidable
   Rust/TS contract risk.
2. Retire stale controls from UI and persistence, but keep contract slots in
   `VisualizerSettings` and Rust until a dedicated contract-cleanup release.
3. Collapse duplicate curve ownership by keeping `connectionCurveLift` as the
   single exposed curve control for now. It already owns generation-time axon
   bow in Rust. Remove or hide `generator.axonCurveLift` from the morph
   descriptor surface and stop persisting user edits to that duplicate field.
4. Remove `surface` and `surfaceOpacity` from public persistence and Debug View
   unless the lead wants a deliberately named Advanced/Legacy section. Keep the
   Rust optional surface path and contract fields dormant.
5. Rename the live neuron billboard group so users understand what it controls.
   Recommended group label: "Neuron points" or "Neuron glow/body", not
   "Neuron Body (applies in Phase E)".
6. Split the oversized Rendering tab into task-oriented groups or sub-tabs.
   The current single tab mixes appearance modes, neuron billboards,
   morphology visibility, morphology geometry, render quality, lighting,
   reach/network rebuild knobs, and bloom.

## Lead decisions

- Keep an Advanced/Debug area. The overhaul should not remove live-but-rarely
  used internal controls solely because they are advanced.
- Still pitch removal for controls that do nothing, duplicate another live
  control, or expose hidden stale state. The first removal pitch should include
  `pointRadius`, `generator.axonCurveLift`, and hidden surface persistence/debug
  state.

## Proposed information architecture

Keep Monitor, Dynamics, Network, and Storage broadly as they are. Rework the
settings-heavy surface into clearer tuning areas:

- Network tab
  - Scale: `n`, `k`, `seed`, Regenerate network.
  - Drive: `excitability`, `ticksPerSec`, `iExt`, `synapticScale`,
    `inputMode`.
  - Dynamics shape: `heterogeneity`, `weightNormalization`.
  - Reach: move `longRangeReachFrac` and `maxReachCells` here because they
    change target IDs and network/morphology structure, not just rendering.

- Appearance tab
  - Color and visibility: `colorBy`, `neuronVisibility`.
  - Neuron points: `glowTau`, `neuronVisualRadius`,
    `activeNeuronRadiusBoost`, `inactiveNeuronOpacity`,
    `voltageGlowStrength`.
  - Morphology visibility: `connectionLayer`, `connectionLightNext`,
    `connectionVisualWidth`, `connectionCurveLift`.
  - Post: `bloomStrength`.

- Morphology tab
  - Generator: remaining generator descriptors that truly feed
    `MorphologyParams`.
  - Render quality: `tubeSides`, `sphereSlices`, `sphereStacks`.
  - Lighting/material: `lighting.*`.

- Debug tab
  - Keep read-only render mode state that corresponds to visible controls.
  - Remove Surface from the default readout if surface stays hidden.
  - Optionally add a compact "Contract tombstones" readout only in an explicit
    Advanced/Debug area, not in the main product-tuning path.

This can be implemented as new tabs, or as the current Rendering tab plus
collapsible sections. New tabs are cleaner because the morphology descriptor
list is long enough to drown out appearance tuning.

## Implementation streams

### Stream 1 - Contract and dead-control quarantine

Owned files:

- `app/web/src/core/settings.ts`
- `app/web/src/core/setting-metadata.ts`
- `app/web/src/ui/dev-panel.ts`
- `app/web/src/ui/dev-panel.test.ts`

Steps:

1. Leave `SETTINGS_LENGTH = 26` and the Rust indices unchanged.
2. Remove `pointRadius` from the visible UI and from `SavedDev` persistence, or
   explicitly alias it to `neuronVisualRadius` if the lead wants backward
   compatibility for old saved payloads. Preferred: stop exposing/persisting it
   and keep index 1 written as the default value.
3. Remove `surface` and `surfaceOpacity` from `SavedPublic` persistence and
   Debug View. Keep `DEFAULT_SETTINGS.surface = 0` and
   `surfaceOpacity = 1.0`.
4. Keep `signalSource`, `connectionLightPast`, and `adaptiveScalerEnabled`
   tombstoned. Do not add UI or persistence for them.
5. Update tests to assert that stale/tombstoned slots are either zero-written
   or default-written as intended, and that saved payloads no longer contain
   removed controls.

Narrow gate:

- `cd app/web && npm test -- dev-panel`
- `cd app/web && npm run typecheck`

Do not run the full e2e suite for this stream.

### Stream 2 - Curve ownership and morph descriptor cleanup

Owned files:

- `app/web/src/core/morph-config.ts`
- `app/web/src/ui/dev-panel.ts`
- `app/web/src/ui/dev-panel.test.ts`
- Rust only if the lead chooses a contract change; otherwise no Rust edits.

Steps:

1. Remove `generator.axonCurveLift` from `MORPH_DESCRIPTORS` and from the
   saved TS morphology config surface, or mark it hidden and default-only.
2. Keep `connectionCurveLift` as the user-facing curve control because Rust
   already applies it last in `current_morph_params`.
3. Ensure `normalizeMorphConfig` drops obsolete `axonCurveLift` from old saved
   payloads if it is removed from the TS config shape.
4. Add a unit test proving old persisted `generator.axonCurveLift` is ignored
   or normalized away, and that the exposed descriptor list no longer includes
   a no-op duplicate.

Narrow gate:

- `cd app/web && npm test -- dev-panel`
- `cd app/web && npm run typecheck`

Rust gates are needed only if the Rust JSON contract or
`current_morph_params` behavior changes.

### Stream 3 - Settings IA re-layout

Owned files:

- `app/web/src/ui/dev-panel.ts`
- `app/web/src/ui/dev-panel.css`
- `app/web/src/ui/dev-panel.test.ts` if tests need updated selectors or tab
  assumptions.

Steps:

1. Introduce the new tab/section structure from "Proposed information
   architecture".
2. Rename "Neuron Body" to a live far-glow/billboard label and delete the stale
   "(applies in Phase E)" caption.
3. Move reach controls out of Appearance/Rendering and into Network or a
   clearly structural group.
4. Keep impact dots aligned with actual apply behavior:
   - green: uniform/live update
   - yellow: network/target ID change
   - red: morphology generation or pipeline rebuild
5. Keep reset behavior per setting and preserve the current storage reset
   behavior.

Narrow gate:

- `cd app/web && npm test -- dev-panel`
- `cd app/web && npm run typecheck`
- Manual dev-panel smoke: open `?dev=1`, visit every tab, change one live
  value, one rebuild value, and one reset button.

### Stream 4 - Documentation migration after implementation

Owned docs:

- `docs/architecture/dev-panel.md`
- `docs/architecture/web-frontend.md` only if persistence/load behavior changes
  beyond field removal.
- `docs/decisions/dev-tooling.md`
- `docs/_meta/manifest.md` only if the Float32Array contract changes.

Steps:

1. Update the dev-panel settings index to match the new tabs and removed
   controls.
2. Record the decision to preserve tombstone slots instead of renumbering.
3. Record single curve ownership.
4. Record any lead decision about Advanced/Legacy controls.

Docs gate:

- Lightweight link/path check as used by the docs workflow.
- No source test required for docs-only migration.

## Collision map

- Defaults and max-scale stream: will touch `DEFAULT_SETTINGS`,
  `DEFAULT_CONFIG`, `MORPH_DESCRIPTORS`, hidden presets, and the Network tab.
  Sequence default changes before final UI copy/persistence assertions, or have
  both streams share one owner for `settings.ts` and `types.ts`.
- Brain color mode stream: will touch `colorBy` options, `settings.ts`,
  `dev-panel.ts`, `render_far.wgsl`, `render_morphology.wgsl`, and Rust uniform
  plumbing. Do not finalize Appearance tab labels until that stream decides the
  exact mode name/value.
- Audio removal stream: may shrink Storage/Debug or module imports, but should
  not touch the Float32Array contract.
- Dendrite branch-quality stream: will touch morphology generator controls and
  may change which generator descriptors remain useful. Sequence before or with
  Stream 2.
- Brain shape realism stream: may add manifold controls or remove the hidden
  surface path entirely. Keep `surface` cleanup reversible until that decision
  is made.

## Open questions for lead

- Should there be any Advanced/Legacy section, or should all controls without a
  public visual-tuning purpose disappear from the panel completely?
- Should `pointRadius` be retired outright, or should it become an alias for
  `neuronVisualRadius` for one release?
- Is the optional manifold surface path still valuable for debugging, or should
  the next implementation stream plan to delete it from Rust/shaders later?
- For the duplicate curve control, is `connectionCurveLift` the preferred
  surviving name, or should the UI name it "Axon curve lift" while still using
  the existing Float32Array slot?
- When removed fields are dropped from persistence, should the app bump the
  `bv2_settings_v1` version sentinel or simply stop writing/reading those keys
  while accepting old localStorage payloads?

## Deferred areas

- No source edit is proposed here for the new `Color by = Brain` mode. That
  belongs to the color-mode stream, though this plan reserves space for it in
  Appearance.
- No default-value changes are planned here. Heterogeneity, glow decay, resting
  brightness, and max N are owned by the defaults/max-scale stream.
- No deletion of Rust fields, WGSL uniforms, or Float32Array indices is planned
  here. Contract shrinkage should be a separate, explicitly approved release.
- No full docs migration happens until implementation ships.

## Exit gate

- The panel no longer exposes controls with no live effect.
- Hidden product features are not persisted or shown as current visual modes by
  default.
- One and only one curve control affects generated axon bow.
- The Rendering/Appearance surface is short enough to tune without scrolling
  through unrelated morphology internals.
- `VisualSettings` indices remain stable, with tests protecting tombstones and
  retired/default-written slots.

## Migration Notes

Migrated on 2026-06-11 into `architecture/dev-panel.md`,
`architecture/web-frontend.md`, and `decisions/dev-tooling.md`. Current-state
docs record the seven-tab IA, retired/default-written `pointRadius`,
`surfaceOpacity`, and `surface` slots, zero-written tombstones, removed duplicate
`generator.axonCurveLift`, and the preserved 26-slot `VisualSettings` contract.

`okay_to_delete` remains `false` only because the visual-product-polish hub is
retaining all six stream plans until the real-WebGPU browser smoke blocker is
cleared or waived.
