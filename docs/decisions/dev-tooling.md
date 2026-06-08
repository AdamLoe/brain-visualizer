# Decisions — Dev Tooling

## Hidden dev panel, not a public settings page

- **Decision.** All tuning controls, sim-drive knobs, and diagnostic readouts
  live in a hidden overlay (`?dev=1` / backtick / gear button) rather than the
  public UI.
- **Why.** The public surface is intentionally minimal — just the visualisation
  and a few beauty presets. Exposing dozens of sliders to all visitors adds
  visual noise and invites accidental mis-tuning. The hidden panel gives
  developers full access without polluting the product surface.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/ui/dev-panel.ts → DevPanel` (open triggers);
  `web/src/main.ts` (wires the gear button via `onVisibilityChange`).

## Colored-dot impact classification as the single source of truth

- **Decision.** Every control in the panel carries a colored dot (green =
  live / yellow = brain-reset / red = renderer-rebuild) whose color comes
  exclusively from `web/src/core/setting-metadata.ts → SETTING_IMPACT`. No other file
  makes impact decisions.
- **Why.** With 24 settings spread across multiple tabs and potentially
  multiple UIs, a single classification table prevents drift between the visual
  hint and the actual apply path. Adding a control means adding one entry to
  `SETTING_IMPACT`; no other coordination needed.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Tradeoffs.** The table is a flat `Record<keyof VisualizerSettings,
  SettingImpact>`; it does not express per-value conditions (e.g. "live unless
  changing from 0"). That granularity has not been needed.

## All settings currently live; brain-reset and renderer-rebuild slots preserved

- **Decision.** Every `VisualizerSettings` field is currently classified
  `"live"` in `SETTING_IMPACT`. The `brain-reset` API slots
  (`ApplyHandlers`, pending-dot, `clearPendingBrainReset`) are preserved as no-ops.
- **Why.** `heterogeneity`, `weightNormalization`, and `inputMode` are `"live"`
  because the integrate uniform is read from GPU memory every tick rather than
  cached at init. Keeping the API slots costs
  nothing and avoids breaking callers.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Revisit when.** A truly structural setting is added (e.g. one that changes
  buffer sizes or requires re-uploading connectivity).

## Versioned localStorage with merge-over-defaults; no preset manager

- **Decision.** Dev-panel settings persist under a versioned key
  (`bv2_settings_v1`) and app runtime config persists under `bv2_config_v1`. On
  load, saved fields are merged over defaults field-by-field with `?? base`
  guards. There is no preset manager — the Reset button removes both keys and
  reverts their in-memory stores and visible controls to defaults.
- **Why.** Merge-over-defaults means adding a new field is safe without a
  version bump and without migration logic: the new field simply falls back to
  its default for existing saves. A version bump is reserved for semantically
  breaking changes (repurposed indices, changed defaults) where old data would
  actively mislead. A preset manager adds infrastructure complexity for a
  developer-facing tool; Reset covers the primary need.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/core/settings.ts → loadSettings, mergeOver, resetSettings`; `web/src/core/types.ts → loadConfig, resetConfig`.
- **Tradeoffs.** No migration: users who had meaningful `dev` knob values set
  before a breaking change lose them silently. Acceptable for a dev panel.

## Morphology config on a separate key + WASM entry point, not the Float32Array

- **Decision.** The dev-panel morphology config (generator / render-quality /
  lighting) persists under its own `bv2_morph_v1` localStorage key and reaches
  the backend through a dedicated `set_morphology_config(json)` WASM entry point
  that takes a JSON string — **not** by adding slots to the `VisualSettings`
  Float32Array or to `bv2_settings_v1`. The dev panel renders its rows from a
  typed descriptor array (`MORPH_DESCRIPTORS`) rather than bespoke per-control
  code.
- **Why.** The 24-slot Float32Array index contract is a frozen, corruption-prone
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

- **Decision.** The 24-element `Float32Array` produced by
  `web/src/core/settings.ts → toFloat32Array` and consumed by
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice` is the sole settings
  boundary between the JS and Rust worlds. Index assignment is the contract;
  both files carry authoritative inline comments.
- **Why.** WASM passes a raw byte slice — there is no named-field protocol.
  Using a flat array with documented indices is simpler than building a
  serialisation layer, and the `from_slice` implementation is length-tolerant
  (new indices fall back to defaults), making forward-compatibility cheap.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).
- **Code anchors.** `web/src/core/settings.ts → toFloat32Array`;
  `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice`.
- **Tradeoffs.** Silent corruption if one side reorders fields without updating
  the other — the authoring brief names this as a corruption risk. A CI
  alignment test would close this gap.

## See also

- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../decisions/profiling.md`](../decisions/profiling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
