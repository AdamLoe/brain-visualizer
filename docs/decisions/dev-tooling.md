# Decisions — Dev Tooling

## Hidden dev panel, not a public settings page

- **Decision.** All tuning controls, sim-drive knobs, and diagnostic readouts
  live in a hidden overlay (`?dev=1` / backtick / gear button) rather than the
  public UI.
- **Why.** The public surface is intentionally minimal — just the visualisation,
  transport, and top-level toggles. Exposing dozens of sliders or review
  presets to all visitors adds visual noise and invites accidental mis-tuning.
  The hidden panel gives developers full access without polluting the product
  surface.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/ui/dev-panel.ts → DevPanel` (open triggers);
  `web/src/main.ts` (wires the gear button via `onVisibilityChange`).

## Boot overlay is a clean status panel, not a diagnostics surface

- **Decision.** The startup overlay shows only a title, a progress bar, and a
  percent + current-stage row. Boot diagnostics stay out of overlay DOM and out
  of `updateStartupOverlay`; timing detail is logged/recorded through
  `boot-timings.ts`. `__bvFrameCounter` / `__bvStartup.status` remain as E2E
  hooks.
- **Why.** The boot panel is part of the product first impression, not a dev
  surface; the diagnostics were noise for the common case and added DOM/state for
  numbers a developer can read from the console or the e2e smoke artifact. Felt
  speed is driven by honest, continuous progress (the sub-stage weighting), not by
  exposing raw timings.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/index.html` (`#startup-overlay` markup + CSS);
  `web/src/main.ts → updateStartupOverlay, startGpuBackend`;
  `web/src/boot-timings.ts → recordBootTiming, logBootSummary`.

## Colored-dot impact classification as the single source of truth

- **Decision.** Every control in the panel carries a colored dot (green =
  live / yellow = brain-reset / red = renderer-rebuild) whose color comes
  exclusively from `web/src/core/setting-metadata.ts → SETTING_IMPACT`. No other file
  makes impact decisions.
- **Why.** With settings spread across multiple tabs and potentially
  multiple UIs, a single classification table prevents drift between the visual
  hint and the actual apply path. Adding a control means adding one entry to
  `SETTING_IMPACT`; no other coordination needed.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Tradeoffs.** The table is a flat `Record<keyof VisualizerSettings,
  SettingImpact>`; it does not express per-value conditions (e.g. "live unless
  changing from 0"). That granularity has not been needed.

## Most settings are live; rebuild-only controls stay explicit

- **Decision.** Most `VisualizerSettings` fields are `"live"`, but the
  heavy-tailed reach knobs stay `"brain-reset"`, `connectionCurveLift` stays
  `"renderer-rebuild"`, and the descriptor-driven morphology generator /
  render-quality groups are still rebuild-backed. The region-assignment
  prototype is an `AppConfig` dev-panel checkbox, not a `VisualizerSettings`
  field, and also rebuilds through the worker-prepared network path. The old
  brain-reset Apply API and pending UI are removed; structural changes go
  through the network/morphology rebuild controls.
- **Why.** `heterogeneity`, `weightNormalization`, and `inputMode` are `"live"`
  because the integrate uniform is read from GPU memory every tick rather than
  cached at init. Reach knobs change target ids and generated geometry,
  `connectionCurveLift` changes baked morphology geometry, and morphology
  generator/quality controls change generated geometry or WGSL overrides, so
  keeping them explicit avoids pretending they are cheap live knobs. Removing
  the no-op Apply surface avoids suggesting there is a second rebuild path.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Revisit when.** A truly structural setting is added (e.g. one that changes
  buffer sizes or requires re-uploading connectivity).

## Versioned localStorage with merge-over-defaults; static hidden review presets only

- **Decision.** Dev-panel settings persist under a versioned key
  (`bv2_settings_v2`), morphology config persists under `bv2_morph_v2`, and app
  runtime config persists under `bv2_config_v2`; this includes the hidden
  region-assignment review mode. On load, saved fields are merged over defaults
  field-by-field with `?? base` guards. There is still no
  public preset manager; the only presets are the static hidden review buttons
  `accepted-default`, `performance-review`, and `hero-review` in the Storage tab.
- **Why.** Merge-over-defaults means adding a new field is safe without a
  version bump and without migration logic: the new field simply falls back to
  its default for existing saves. A version bump is reserved for semantically
  breaking changes (repurposed indices, changed defaults) where old data would
  actively mislead. Morphology loading also filters each group to known current
  fields, and app config normalizes stale backend values to GPU, so obsolete
  config keys from older saves are ignored rather than sent back to Rust. The
  review presets cover the reproducibility need without growing a user-editable
  preset system.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/core/settings.ts → loadSettings, mergeOver, resetSettings`;
  `web/src/core/morph-config.ts → loadMorphConfig, resetMorphConfig`;
  `web/src/core/types.ts → loadConfig, resetConfig`;
  `web/src/ui/dev-panel.ts → HIDDEN_REVIEW_PRESETS`.
- **Tradeoffs.** Default changes do not automatically reset old saved
  visual/morph values unless the version sentinel is bumped. App config is the
  exception for scale safety: saved `n` is clamped to the product cap on
  load/save.

## Region assignment mode stays in AppConfig, not VisualSettings

- **Decision.** The anterior/posterior region prototype toggle lives in
  `AppConfig.regionAssignmentMode` and is sent with worker-prepared network
  requests; it is not added to the `VisualizerSettings` Float32Array or to the
  morphology JSON config.
- **Why.** Region assignment is a build-time structural choice that changes the
  generated per-neuron region codes. It does not belong in the live uniform
  settings array, and putting it there would risk the frozen positional index
  contract for a value Rust only consumes during preparation.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/manifold.md`](../architecture/manifold.md).
- **Code anchors.** `web/src/core/types.ts → AppConfig, normalizeRegionAssignmentMode`;
  `web/src/ui/dev-panel.ts → _buildNetworkTab`;
  `web/src/gpu-build/prepared-network.ts → PreparedNetworkRequest`;
  `crates/brain-visualizer/src/lib.rs → prepare_network_payload`.

## Morphology config on a separate key + WASM entry point, not the Float32Array

- **Decision.** The dev-panel morphology config (generator / render-quality /
  lighting) persists under its own `bv2_morph_v2` localStorage key and reaches
  the backend through a dedicated `set_morphology_config(json)` WASM entry point
  that takes a JSON string — **not** by adding slots to the `VisualSettings`
  Float32Array or to `bv2_settings_v2`. The boot path queues the persisted
  morphology config before backend creation and again after backend creation, so
  Rust receives it without any slider interaction. The dev panel renders its rows
  from a typed descriptor array (`MORPH_DESCRIPTORS`) rather than bespoke
  per-control code, and descriptor defaults must match `DEFAULT_MORPH_CONFIG`.
- **Why.** The positional Float32Array index contract is a frozen, corruption-prone
  Rust↔TS boundary (see Float32Array decision below); the morphology config is a
  larger, nested, evolving surface where adding/removing a field should not risk
  silently shifting every other visual setting. A separate JSON channel lets the
  Rust side deserialize by name (serde), diff incoming vs current, and run the
  narrowest update (uniform-only for lighting; regenerate for generator; pipeline
  rebuild for render-quality) — none of which fits a flat positional float array.
  Descriptor-driven rows keep adding a control to a single array entry.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/manifold.md`](../architecture/manifold.md).
- **Code anchors.** `web/src/core/morph-config.ts → MORPH_DESCRIPTORS, MorphologyConfig, loadMorphConfig`;
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend::set_morphology_config`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::set_morphology_config`.
- **Tradeoffs.** Two persistence keys and two backend channels to keep coherent
  (the reset path must clear both); a JSON round-trip per apply instead of a raw
  byte slice. Acceptable — morphology config is applied on explicit edits, not
  per-frame.

## Numeric dev controls share one slider/input/reset widget

- **Decision.** Rendering and morphology numeric controls use the shared
  slider + number input + reset button + instant-tooltip helper in
  `web/src/ui/dev-panel.ts`; morphology rows remain descriptor-driven.
- **Why.** Tiny morphology ranges are not usable as drag-only sliders, and reset
  buttons are only trustworthy when they read the same defaults the backend sees.
  A shared helper keeps the panel's numeric controls mechanically consistent
  while preserving the existing impact-dot model.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/ui/dev-panel.ts → _sliderWithInput, _sliderRow,
  _morphRow`; `web/src/core/morph-config.ts → MORPH_DESCRIPTORS`.

## Tombstone or quarantine dead Float32Array slots instead of renumbering

- **Decision.** Removed visual settings are tombstoned or quarantined in
  `web/src/core/settings.ts → toFloat32Array` and
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice`
  rather than renumbering later `VisualSettings` slots.
- **Why.** Renumbering the Rust/TypeScript flat-array contract is a corruption
  risk with little payoff. Tombstoning/quarantining removes misleading controls
  while keeping old positional meaning stable and keeping dormant Rust render
  paths available for explicit future work.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md).
- **Code anchors.** `web/src/core/settings.ts → SavedDev, toFloat32Array`;
  `web/src/ui/dev-panel.ts → _buildAppearanceTab, _buildDebugViewTab`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice`.

## Version-reset over migration for semantically breaking default changes

- **Decision.** When a wave of default changes would cause old saved values to
  actively mislead — such as high-excitability/high-`iExt` saves masking the
  new quiet-network defaults — the LS key version string is bumped (e.g.
  `bv2_settings_v1` → `bv2_settings_v2`) rather than writing migration logic.
  App-owned keys move together for semantically coupled default waves.
- **Why.** A migration that rewrites old high-excitability saves to the new
  low-firing defaults is indistinguishable from a reset for the user; a version
  bump is simpler, audit-proof, and has no edge cases. The cost is that saved
  visual preferences are discarded — acceptable for a dev-tool panel where users
  can re-tune in a few seconds. The merge-over-defaults shape means the reset
  leaves the user on the new clean defaults immediately.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/core/settings.ts → SETTINGS_LS_KEY`;
  `web/src/core/morph-config.ts → MORPH_CONFIG_LS_KEY`;
  `web/src/core/types.ts → CONFIG_LS_KEY`.

## Expose only bounded runtime-safe morphology knobs

- **Decision.** The dev panel exposes only a small, bounded set of
  morphology generator knobs at runtime. Decoration controls are capped by
  their compile-time maxima, and path-sampling controls are clamped to the
  already-budgeted `EDGE_SUBSEGMENTS_MAX`. Allocation budgets, salts, waypoint
  counts, and shader tube-ring curvature remain protected.
- **Why.** The GPU morphology buffers are pre-allocated to fixed maxes at
  pipeline build time. Changing a buffer-sized parameter without resizing the
  buffer silently drops segments or overruns memory. The exposed path-sampling
  knobs only vary how much of the existing per-hop cap is used, so they can make
  turns smoother/coarser before render-side tube bending without changing buffer
  sizing.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md).
- **Code anchors.** `web/src/core/morph-config.ts → MORPH_DESCRIPTORS`;
  `crates/brain-visualizer/src/sim/morphology.rs → GeneratorConfig::apply_to,
  MorphologyParams::locked_default, EDGE_SUBSEGMENTS_MAX`.
- **Revisit when.** The pipeline rebuild path accepts dynamic buffer sizes, or a
  separate "needs-rebuild" flow is added for buffer-sized changes.

## Task-oriented settings IA over one oversized rendering tab

- **Decision.** The dev panel uses the task-oriented tab set defined by
  `web/src/ui/dev-panel.ts → TABS`. Appearance owns live visual settings and
  morphology lighting; Morphology owns descriptor-driven generator/render-quality
  config; Debug is read-only current-state labels.
- **Why.** The old Rendering tab mixed color, glow, connection visibility,
  morphology generation, lighting, reach, quality, and stale surface state. A
  task-oriented split keeps live tuning surfaces easier to trust while letting
  advanced/debug capability remain hidden rather than deleted wholesale. Lighting
  controls (uniform/live) sit in Appearance because they feel like live render
  knobs, not geometry parameters; generator/quality controls (rebuild-backed)
  stay in Morphology where users expect to rebuild.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/ui/dev-panel.ts → TABS, _buildAppearanceTab,
  _buildMorphLightingRows, _buildMorphConfigRows, _buildDebugViewTab`.

## Custom instant tooltips, not native `title=`

- **Decision.** Dev-panel controls and metric rows use a custom zero-delay
  tooltip: a single floating `.dp-tooltip` element appended to `<body>` and
  positioned by two delegated `mouseover`/`mouseout` listeners on `document`
  keyed off a `data-tip` attribute. Native `title=` and CSS `::after` tooltips
  are not used.
- **Why.** Native `title=` waits ~1 s before showing — too slow for a dense
  panel where hovering to learn what a control does should be instant. A CSS
  `::after` tooltip would be clipped by the panel's scrolling container; a
  body-appended floating element is not. Delegated listeners mean adding a tip
  to a new control is a single `_attachTip` call (no per-element wiring).
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/ui/dev-panel.ts → _buildTooltip, _attachTip`.

## Float32Array index contract is the shared Rust/TS boundary

- **Decision.** The `Float32Array` of length `SETTINGS_LENGTH` produced by
  `web/src/core/settings.ts → toFloat32Array` and consumed by
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice` is the sole settings
  boundary between the JS and Rust worlds. Index assignment is the contract;
  both files carry authoritative inline comments and executable contract tests.
  Removed settings reserve their existing indices and are zero/default-written
  rather than shifting later slots.
- **Why.** WASM passes a raw byte slice — there is no named-field protocol.
  Using a flat array with documented indices is simpler than building a
  serialisation layer, and the `from_slice` implementation is length-tolerant
  (new indices fall back to defaults), making forward-compatibility cheap.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).
- **Code anchors.** `web/src/core/settings.ts → toFloat32Array`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice`.
- **Tradeoffs.** The flat array is still less self-describing than JSON, so
  changes require synchronized Rust and TypeScript edits. The guardrail is
  duplicated explicit golden tests instead of shared generated schema plumbing,
  which keeps the current contract small and reviewable.

## See also

- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../decisions/profiling.md`](../decisions/profiling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
