---
status:        active
owner:         adamg
last_updated:  2026-06-06
---

# Dev Panel

A hidden right-docked drawer (`PANEL_WIDTH_PX = 360 px`) that gives developers
and power-users access to live metrics, all tunable settings, hidden review
presets, and storage diagnostics — without cluttering the public UI.

## What it owns

- `DevPanel` class, open/close triggers, tab layout — `web/src/ui/dev-panel.ts → DevPanel`
- Hidden review preset payloads — `web/src/ui/dev-panel.ts → HIDDEN_REVIEW_PRESETS`
- `SysInfo` and `ApplyHandlers` / `SimHandlers` callback interfaces — `web/src/ui/dev-panel.ts`
- Setting-impact classification system and colored dot rendering — `web/src/core/setting-metadata.ts → SETTING_IMPACT`
- `VisualizerSettings` interface, persistence, `toFloat32Array` serialisation — `web/src/core/settings.ts → VisualizerSettings`
- The localStorage schema and merge-over-defaults contract — `web/src/core/settings.ts → loadSettings, mergeOver`

## What it does NOT own

- The metrics pipeline that produces the numbers displayed in Monitor and Dynamics tabs — [`profiling.md`](profiling.md)
- The GPU simulation backend that consumes the settings Float32Array — [`gpu-backend.md`](gpu-backend.md)
- The public corner HUD — [`profiling.md`](profiling.md)

## Opening the panel

Three triggers (all documented in the `web/src/ui/dev-panel.ts` module comment):

1. URL `?dev=1` — opens at boot.
2. Backtick key (`` ` ``) — toggles open/closed; no modifier keys needed.
3. A small bottom-right gear button (via `onVisibilityChange` wired in `web/src/main.ts`).

The panel is closed by default for ordinary visitors. When closed, `pointer-events`
apply only to the canvas; the overlay never intercepts input.

`update(m, sys?)` is called from the once-per-second block in `rafLoop`
(`web/src/main.ts`) **only when `isOpen()` returns true** — no DOM work is done
when the panel is hidden.

## Tabs

Six tabs are defined in the `TABS` constant in `web/src/ui/dev-panel.ts`:

| Tab | Content |
|---|---|
| Monitor | Live spike/voltage/E-I metrics + network-state classifier |
| Dynamics | E/I balance bar, branching-ratio band, per-region rates, interpretive summary |
| Network | Rebuild controls (N/K/seed) + live Drive and Structure knobs |
| Rendering | Visual knobs with impact dots |
| Debug View | Read-only string labels for current visual-mode settings |
| Storage | Reset-to-defaults button + hidden review presets + localStorage key/size readout |

The Rendering tab includes a compact Morphology subsection. The top of it groups
the live render knobs that live in the `VisualSettings` Float32Array:
`connectionLayer`, `connectionLightNext`, `morphRestingOpacity`,
`connectionVisualWidth`, and `connectionCurveLift`. (`connectionLightPast` was
removed; Float32Array index 9 is tombstoned as `reserved_zero`.) That grouping is
UI consolidation only; it does not add new Float32Array indices.

The `Color by` selector includes the additive identity-color mode; it reuses
the existing `colorBy` setting and does not add a Float32Array index or a
persistence key. Below those, the subsection renders the **morphology config** controls — a
separate surface (see the Morphology config controls section below) that does
**not** touch the Float32Array. These expose the generator/render-quality/lighting
parameters of the procedural neuron geometry, backed by their own
`bv2_morph_v1` localStorage key and a dedicated WASM entry point.

## Network-state classifier

The Monitor tab shows a SILENT / TUNED / OVERACTIVE verdict. Thresholds live
as constants in `web/src/ui/dev-panel.ts → classify`:

- **SILENT** if `pctFired500ms < 0.005` (< 0.5 % of neurons fired in 500 ms)
- **OVERACTIVE** if `pctFired100ms > 0.30` OR `branchingRatio > 1.5`
- **TUNED** otherwise

`pctFired*` values arrive from the GPU as fractions in [0, 1]; the panel
multiplies by 100 for display. The branching-ratio critical-band thresholds
(0.9 / 1.1) used in the Dynamics tab are separate constants in the same file.

## Instant tooltips

Controls and the Monitor/Dynamics metric rows carry a custom **zero-delay**
tooltip rather than a native `title=` attribute (which has a ~1 s show delay) or
a CSS `::after` (which the panel's scroll container would clip). A single
floating element (`.dp-tooltip`) is appended to `<body>` once at construction and
positioned on hover; two **delegated** `mouseover`/`mouseout` listeners on
`document` find the nearest `[data-tip]` ancestor and show/hide it instantly. Per-element
text is registered via `web/src/ui/dev-panel.ts → _attachTip` (sets the
`data-tip` attribute); the build/positioning logic is `_buildTooltip`. Self-evident
items (section separators, the × close button) carry no tip. See
[`../decisions/dev-tooling.md`](../decisions/dev-tooling.md) for the rationale.

## Setting-impact classification

Every control in the Rendering and Network tabs carries a colored dot. The dot
color is driven entirely by `web/src/core/setting-metadata.ts → SETTING_IMPACT`, which
is the **single source of truth** for every setting's impact level:

| Dot color | Level | Meaning |
|---|---|---|
| Green | `"live"` | Takes effect immediately; the shader reads the uniform next tick |
| Yellow | `"brain-reset"` | Requires `reinitialize()` with the same seed |
| Red | `"renderer-rebuild"` | Full pipeline rebuild required |

As of the current codebase, every `VisualizerSettings` field except
`connectionCurveLift` is `"live"`. `connectionCurveLift` remains
`"renderer-rebuild"` because it changes baked morphology geometry.
`heterogeneity`, `weightNormalization`, and `inputMode` were downgraded from
`"brain-reset"` to `"live"` once the integrate uniform was read every tick.
N/K/seed live in `AppConfig` and are out of scope for `SETTING_IMPACT`.

The `adaptiveScalerEnabled` field (Float32Array index 23) is **reserved/inert**:
runtime auto-scaling was removed, so its "Adaptive scaler" select is gone from
the panel. The field and its index are kept only to preserve the Rust↔TS
`VisualSettings` contract (see the index-contract section below); no decision path
reads it. Re-arming a scaler is deferred — see
[`../plans/future_roadmap.md`](../plans/future_roadmap.md) and
[`scaling.md`](scaling.md).

The `brain-reset` pending-dot and Apply button exist in the API
(`ApplyHandlers`, `setApplyHandlers`, `clearPendingBrainReset`) but are
currently no-ops — kept for callers that still wire them.

## Hidden review presets

The Storage tab exposes three **code-defined**, dev-only review presets:

- `accepted-default`
- `performance-review`
- `hero-review`

The preset table lives in `web/src/ui/dev-panel.ts → HIDDEN_REVIEW_PRESETS`.
`accepted-default` is derived directly from `DEFAULT_CONFIG`,
`DEFAULT_SETTINGS`, and `DEFAULT_MORPH_CONFIG`, so it matches the clean
first-load values by construction rather than carrying a separate tuned payload.
`performance-review` and `hero-review` keep the same app config but override the
visual and morphology payloads for lower-cost and screenshot-oriented review.

These presets are not a public preset manager: they are static buttons inside
the already-hidden dev panel, intended only for review and comparison.

## Morphology config controls

The morphology generator, render-quality, and lighting parameters of the
procedural neuron geometry are exposed through a **descriptor-driven** surface
that is independent of the `VisualizerSettings` Float32Array. The descriptor
array is the single source of truth for the controls — `web/src/core/morph-config.ts → MORPH_DESCRIPTORS`.
Each descriptor carries its json path, group, label, min/max/step, default,
impact, and apply kind; the dev panel renders one row per descriptor
(`web/src/ui/dev-panel.ts → _buildMorphConfigRows, _morphRow`) rather than
hand-written rows. To change which controls exist, edit the descriptor array —
not the panel.

Three groups (the structure is the nested `MorphologyConfig` shape in
`web/src/core/morph-config.ts → MorphologyConfig`):

- **generator** — maps into the Rust `MorphologyParams` generator fields (branch
  counts, reach, radius/taper fractions, socket placement). Impact
  `renderer-rebuild`; applied by morphology regeneration.
- **renderQuality** — `tubeSides` and soma sphere tessellation (`sphereSlices`,
  `sphereStacks`). Impact `renderer-rebuild`; applied by a morph pipeline rebuild.
- **lighting** — light direction x/y/z, ambient/diffuse/rim, plus the
  resting/active brightness split (`restingBrightness`, `activeBoost`). Impact
  `live`; applied by a uniform-only write.

Ranges are deliberately **narrow bounds around the locked generator default**
(`crates/brain-visualizer/src/sim/morphology.rs → MorphologyParams::locked_default`) —
this is an exposure pass, not a retuning pass; a later tuning pass widens them.
The protected budget/slack/salt fields are intentionally absent — see
[`manifold.md`](manifold.md).

**Apply model.** generator and renderQuality controls (red `renderer-rebuild`
dot) edit a *pending* config and apply only when the **Rebuild Morphology** button
is pressed — avoiding mid-drag regen/pipeline rebuilds. lighting controls (green
`live` dot) apply immediately via the uniform-only path. The button and pending
state live in `web/src/ui/dev-panel.ts → _buildMorphConfigRows`; the apply call
crosses the wasm boundary through `set_morphology_config` (see
[`web-frontend.md`](../architecture/web-frontend.md)), which diffs and runs the
narrowest update. The impact-dot colors mean the same thing as in the table
above; the only difference is that for morphology the red-dot controls are
batched behind the Rebuild button instead of pushing on each change.

## Settings persistence contract

**Keys:** `bv2_settings_v1`, `bv2_morph_v1`, `bv2_config_v1`.

**Schema:** settings persist as `{ version: 5, public: {…}, dev: {…} }`. The `public` sub-object
holds user-facing beauty knobs; `dev` holds tuning/debug knobs. The split is
defined by the `SavedPublic` and `SavedDev` interfaces in `web/src/core/settings.ts`.

**Version sentinel:** any saved settings object whose `version` field is not `5` is
silently discarded and defaults are used. No migration is attempted. (The
sentinel is bumped whenever a Float32Array index is repurposed or a default
changes so old saves cannot silently mislead.) The v0.2.1 Morphology tuning
did require a bump because the shipped render defaults changed even though the
float-array contract stayed intact.

**Merge-over-defaults:** on load, each saved key is merged over
`DEFAULT_SETTINGS` with `?? base_value` guards (`web/src/core/settings.ts → mergeOver`).
Missing fields (from older saves or partial schemas) fall back silently. Adding a
new field to `VisualizerSettings` is safe without a version bump; only
semantically breaking changes (changed defaults, repurposed indices) require a
bump.

**Never persist counters:** `VisualizerSettings` contains only durable
configuration knobs. Runtime metrics (`spikesPerSec`, `branchingRatio`, etc.)
are on the separate `Metrics` interface and are never written to localStorage.

**Separate morphology config key.** The morphology config (see Morphology config
controls above) persists under its **own** key `bv2_morph_v1`, independent of
`bv2_settings_v1`. Same versioned + merge-over-defaults shape:
`web/src/core/morph-config.ts → loadMorphConfig, saveMorphConfig, resetMorphConfig`,
with a `version` sentinel and `MorphologyConfig` defaults. It is deliberately
NOT folded into `bv2_settings_v1` because it does not cross the frozen
Float32Array boundary — see [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md).

**Reset:** `web/src/core/settings.ts → resetSettings` removes the settings
localStorage key, restores `current` to `DEFAULT_SETTINGS`, and notifies all
subscribers synchronously — which causes the dev panel's `_syncSliders` to
restore settings-backed control positions. The Storage reset handler also clears
the morphology key via `web/src/core/morph-config.ts → resetMorphConfig`,
re-syncs the morphology rows, calls `web/src/core/types.ts → resetConfig`, syncs
the Network tab controls to `DEFAULT_CONFIG`, updates `main.ts`'s in-memory
`AppConfig`, and schedules a rebuild back to the default N/K/seed. That means
reset now restores the live network to the clean default values instead of only
changing the stored controls. The storage readout shows all three app-owned
keys, including `bv2_morph_v1`.

## Float32Array index contract (corruption risk)

`web/src/core/settings.ts → toFloat32Array` serialises all 24 `VisualizerSettings`
fields into a `Float32Array` that is passed to the WASM backend. The index
assignment is the **shared contract** with `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`
(`from_slice`). **Reordering or inserting a field in either file without
updating the other silently corrupts all downstream visual settings.** The
authoritative index list is in the inline comments of `web/src/core/settings.ts →
VisualizerSettings` and `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`.

## Slider/select sync

All slider and select elements are registered in `sliderElements` (selects
stored with a `__select` key suffix). `_syncSliders` iterates this map on any
settings change — including an external `resetSettings()` — to keep all control
positions consistent. The `subscribe()` / `unsubscribe()` cleanup function is
stored as `_unsubSettings` and called in `destroy()`.

## Update when

- A field is added to or removed from `VisualizerSettings` (update both
  `toFloat32Array` and `VisualSettings.from_slice`; consider a schema version bump).
- A new tab is added to `TABS`.
- Classifier thresholds change.
- Any setting's impact level changes in `SETTING_IMPACT`.
- The localStorage version sentinel changes, or a UI consolidation repurposes a
  saved setting without changing the underlying float-array contract.
- A morphology control is added/removed/re-ranged (edit `MORPH_DESCRIPTORS`), or
  the `MorphologyConfig` shape / `bv2_morph_v1` schema changes.
- The instant-tooltip mechanism (`_buildTooltip` / `_attachTip` / `data-tip`) changes.

## See also

- [`profiling.md`](profiling.md) — metrics pipeline that feeds Monitor/Dynamics tabs
- [`gpu-backend.md`](gpu-backend.md) — `VisualSettings.from_slice` consumer
- [`web-frontend.md`](../architecture/web-frontend.md) — `rafLoop` that calls `update()`
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
