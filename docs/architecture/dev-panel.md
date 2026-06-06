---
status:        active
owner:         adamg
last_updated:  2026-06-06
---

# Dev Panel

A hidden right-docked drawer (`PANEL_WIDTH_PX = 360 px`) that gives developers
and power-users access to live metrics, all tunable settings, and storage
diagnostics — without cluttering the public UI.

## What it owns

- `DevPanel` class, open/close triggers, tab layout — `web/src/ui/dev-panel.ts → DevPanel`
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
| Storage | Reset-to-defaults button + localStorage key/size readout |

The Rendering tab now includes a compact Morphology subsection that groups the
accepted live render controls together: `connectionLayer`,
`connectionLightNext`, `connectionLightPast`, `morphRestingOpacity`,
`connectionVisualWidth`, and `connectionCurveLift`. That grouping is UI
consolidation only; it does not add new Float32Array indices, but v0.2.1 did
ship narrowed defaults for the existing render knobs and therefore bumped the
localStorage schema sentinel. The branch-grammar inputs that shaped v0.2.0
remain code-only / protected and are captured in morphology build artifacts and
stats instead.

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

As of the current codebase **all** `VisualizerSettings` fields are `"live"`;
`heterogeneity`, `weightNormalization`, and `inputMode` were downgraded from
`"brain-reset"` to `"live"` once the integrate uniform was read every tick. N/K/seed live in `AppConfig` and are out of scope for `SETTING_IMPACT`.

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

## Settings persistence contract

**Key:** `bv2_settings_v1` (hardcoded in both `web/src/core/settings.ts → LS_KEY` and
`web/src/ui/dev-panel.ts → DevPanel.LS_KEY`).

**Schema:** `{ version: 4, public: {…}, dev: {…} }`. The `public` sub-object
holds user-facing beauty knobs; `dev` holds tuning/debug knobs. The split is
defined by the `SavedPublic` and `SavedDev` interfaces in `web/src/core/settings.ts`.

**Version sentinel:** any saved object whose `version` field is not `4` is
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

**Reset:** `web/src/core/settings.ts → resetSettings` removes the localStorage key,
restores `current` to `DEFAULT_SETTINGS`, and notifies all subscribers
synchronously — which causes the dev panel's `_syncSliders` to restore all
control positions.

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
- The instant-tooltip mechanism (`_buildTooltip` / `_attachTip` / `data-tip`) changes.

## See also

- [`profiling.md`](profiling.md) — metrics pipeline that feeds Monitor/Dynamics tabs
- [`gpu-backend.md`](gpu-backend.md) — `VisualSettings.from_slice` consumer
- [`web-frontend.md`](../architecture/web-frontend.md) — `rafLoop` that calls `update()`
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
