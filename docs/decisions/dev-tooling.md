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

- **Decision.** Settings persist under a versioned key (`bv2_settings_v1`,
  schema `version: 3`). On load, saved fields are merged over `DEFAULT_SETTINGS`
  field-by-field with `?? base` guards. There is no preset manager — the
  Reset button removes the key and reverts to defaults.
- **Why.** Merge-over-defaults means adding a new field is safe without a
  version bump and without migration logic: the new field simply falls back to
  its default for existing saves. A version bump is reserved for semantically
  breaking changes (repurposed indices, changed defaults) where old data would
  actively mislead. A preset manager adds infrastructure complexity for a
  developer-facing tool; Reset covers the primary need.
- **Applies to.** [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/src/core/settings.ts → loadSettings, mergeOver, resetSettings`.
- **Tradeoffs.** No migration: users who had meaningful `dev` knob values set
  before a breaking change lose them silently. Acceptable for a dev panel.

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
