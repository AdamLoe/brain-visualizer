---
status:        active
owner:         adamg
last_updated:  2026-06-12
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

Seven tabs are defined in the `TABS` constant in `web/src/ui/dev-panel.ts`:

| Tab | Content |
|---|---|
| Monitor | Live spike/voltage/E-I metrics + network-state classifier |
| Dynamics | E/I balance bar, branching-ratio band, per-region rates, interpretive summary |
| Network | Rebuild controls (N/K/seed) + live Drive and Structure knobs |
| Appearance | Color, neuron glow/body, morphology visibility, reach, and morphology lighting |
| Morphology | Descriptor-driven generator and render-quality knobs |
| Debug | Read-only string labels for current visual-mode settings |
| Storage | Reset-to-defaults button + hidden review presets + localStorage key/size readout |

The Appearance tab groups the live render knobs that live in the
`VisualSettings` Float32Array: `colorBy`, `neuronVisibility`, `glowTau`,
`neuronVisualRadius`, `activeNeuronRadiusBoost`, `inactiveNeuronOpacity`,
`voltageGlowStrength`, `connectionLayer`, `connectionLightNext`,
`morphRestingOpacity`, `connectionVisualWidth`, `connectionCurveLift`,
`longRangeReachFrac`, and `maxReachCells`. `connectionLightPast` was removed;
Float32Array index 9 is tombstoned as `reserved_zero`. `bloomStrength` was also
removed from the panel and persistence surface; Float32Array index 10 is
tombstoned as `reserved_zero`.

`connectionLayer` (index 17, default 1) is a 3-mode enum surfaced as a
labeled "Connections" dropdown in the "Morphology Visibility" section:

| Value | Label | Behaviour |
|---|---|---|
| 0 | Off | Skips all morphology work — compaction compute, tube passes, soma passes |
| 1 | Active/recent | Draws only segments near a spike (DEFAULT) |
| 2 | Resting debug | Full resting morphology; currently requires a build with `DRAW_LEGACY_ALL_SEGMENTS`; otherwise behaves like mode 1 |

The tab also contains a "Morphology Lighting" section that renders the
`MORPH_DESCRIPTORS` rows with `group === "lighting"` via
`_buildMorphLightingRows`. These are `live`/`uniform` lighting controls;
`applyKind === "uniform"` rows call `onMorphLive` immediately.

The `Color by` selector includes `Brain` (`colorBy = 6`) and Brain is the clean
default. It reuses the existing `colorBy` setting at Float32Array index 18 and
does not add a persistence key. The older `pointRadius`, `surfaceOpacity`, and
`surface` controls are no longer exposed or persisted. Their Float32Array slots
remain for compatibility and are written from defaults (`pointRadius` index 1,
`surfaceOpacity` index 11, `surface` index 20). The Rust optional surface path
still exists and Brain mode can tint it pink when it is explicitly on, but the
current product settings UI keeps surface controls quarantined and default-off.

The settings boundary is guarded by executable contract tests on both sides:
TypeScript locks the full `toFloat32Array(DEFAULT_SETTINGS)` layout and the
reserved/default-written slots, while Rust locks `VisualSettings::from_slice`
index mapping and tombstone behavior. This protects the 26-slot positional
contract without replacing it with a named-field protocol.

The Morphology tab renders the **morphology config** controls: a separate
surface (see the Morphology config controls section below) that does **not**
touch the Float32Array. These expose the generator and render-quality
parameters of the procedural neuron geometry, backed by their own
`bv2_morph_v2` localStorage key and a dedicated WASM entry point.

## Network-state classifier

The Monitor tab shows a SILENT / TUNED / OVERACTIVE verdict. Thresholds live
as constants in `web/src/ui/dev-panel.ts → classify`:

- **SILENT** if `pctFired500ms < 0.005` (< 0.5 % of neurons fired in 500 ms)
- **OVERACTIVE** if `pctFired100ms > 0.30` OR `branchingRatio > 1.5`
- **TUNED** otherwise

`pctFired*` values arrive from the GPU as fractions in [0, 1]; the panel
multiplies by 100 for display. The branching-ratio critical-band thresholds
(0.9 / 1.1) used in the Dynamics tab are separate constants in the same file.
Derived readouts are labelled in-place: synaptic events/sec is shown as an
estimate (`spikes/sec × K`), and the current cascade size row remains explicitly
approximate.

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

The `pointRadius`, `surfaceOpacity`, `surface`, and `adaptiveScalerEnabled`
fields are **reserved/inert** in the panel:

- `pointRadius` (index 1) is stale because live far billboards size from
  `neuronVisualRadius`; it is not saved in `SavedDev` and `toFloat32Array()`
  writes `DEFAULT_SETTINGS.pointRadius`.
- `surfaceOpacity` (index 11) and `surface` (index 20) feed a dormant optional
  surface path; both are absent from public persistence/debug UI and are written
  from defaults.
- `adaptiveScalerEnabled` (index 23) stays zero-written because runtime
  auto-scaling was removed.

The fields and indices are kept only to preserve the Rust<->TypeScript
`VisualSettings` contract (see the index-contract section below). Re-arming a
scaler is deferred — see
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

The morphology generator and render-quality parameters of the procedural
neuron geometry are exposed through a **descriptor-driven** surface that is
independent of the `VisualizerSettings` Float32Array. The descriptor array is
the single source of truth for the controls — `web/src/core/morph-config.ts → MORPH_DESCRIPTORS`.
Each descriptor carries its json path, group, label, min/max/step, default,
impact, and apply kind; the dev panel renders one row per descriptor
(`web/src/ui/dev-panel.ts → _buildMorphConfigRows, _morphRow`) rather than
hand-written rows. Rows use the same slider + number input + reset button +
tooltip helper as numeric rendering controls. To change which controls exist,
edit the descriptor array — not the panel. A unit test in
`web/src/ui/dev-panel.test.ts` asserts each descriptor default matches
`DEFAULT_MORPH_CONFIG`.

Three descriptor groups exist in `MORPH_DESCRIPTORS` (the nested
`MorphologyConfig` shape is in `web/src/core/morph-config.ts → MorphologyConfig`):

- **generator** — maps into the Rust `MorphologyParams` generator fields (branch
  counts, reach, radius/taper fractions, socket placement). Impact
  `renderer-rebuild`; applied by morphology regeneration. Rendered in the
  Morphology tab by `_buildMorphConfigRows`.
- **renderQuality** — `tubeSides` and soma sphere tessellation (`sphereSlices`,
  `sphereStacks`). Impact `renderer-rebuild`; applied by a morph pipeline
  rebuild. Also rendered in the Morphology tab.
- **lighting** — light direction x/y/z, ambient/diffuse/rim, the
  resting/active brightness split (`restingBrightness`, `activeBoost`), and the
  active-opacity layer controls (`activeOpacity`, `inactiveOpacityFloor`).
  Impact `live`; applied by a uniform-only write. Rendered in the
  **Appearance** tab under "Morphology Lighting" by `_buildMorphLightingRows`.

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

**Dendrite generator controls.** The target-owned incoming dendrite generator is
controlled by the live socket-placement controls (`socketCount*`,
`socketRadius*`, `socketTipPreference`) plus the soma-proximal branching
controls and dendrite decoration controls:

| Control | Default | Range |
|---|---:|---:|
| `dendritePrimaryRootCount` | `4` | `1..6` |
| `dendriteForkDistance` | `1.45` | `1.15..2.20` |
| `dendriteCurveTightness` | `0.55` | `0..1.25` |
| `dendriteMidRadiusFraction` | `0.78` | `0.45..0.90` |
| `dendriteTipRadiusFraction` | `0.42` | `0.22..0.62` |
| `dendriteGroupSpacing` | `0.55` | `0..1.50` |
| `dendriteBranchletCount` | `1` | `0..1` |
| `dendriteTwigCount` | `1` | `0..2` |
| `dendriteDecorGroupMax` | `12` | `0..16` |

The three decoration controls (`dendriteBranchletCount`, `dendriteTwigCount`,
`dendriteDecorGroupMax`) are runtime-clamped to the compile-time buffer maxes;
the `hero-review` preset maximizes them for close-up screenshots but they are
NOT the product defaults. The generator also exposes bounded straight
subdivision controls (`maxSegmentLength`, `longRangeMaxSegmentLength`,
`curvatureSubsegmentBoost`, `edgeSubsegmentsMax`, `minSubsegments`). These tune
how many straight `MorphSegment`s approximate each local/long branch and are
clamped to the existing `EDGE_SUBSEGMENTS_MAX` cap; they do not add curved shader
geometry. Waypoint counts and allocation budgets remain protected.

The older dead `dendritePrimaryMin` / `dendritePrimarySpan` fields and duplicate
`generator.axonCurveLift` descriptor are removed from the exposed descriptor
surface. Old persisted morphology payloads that still contain those fields are
accepted, normalized to the current known key set, and omitted on the next save.

## Settings persistence contract

**Keys:** `bv2_settings_v2`, `bv2_morph_v2`, `bv2_config_v2`.

**Schema:** settings persist as `{ version: 5, public: {…}, dev: {…} }`. The
`public` sub-object holds user-facing beauty knobs; `dev` holds tuning/debug
knobs. The split is defined by the `SavedPublic` and `SavedDev` interfaces in
`web/src/core/settings.ts`. Current clean defaults include `colorBy = 6`
(`Brain`), `glowTau = 10`, `heterogeneity = 0.50`, `iExt = 0.014`,
`morphRestingOpacity = 0.0`, `longRangeReachFrac = 0.14`, and
`maxReachCells = 14`. Removed visual fields such as `bloomStrength` are omitted
from both `SavedPublic` and `SavedDev`.

**Version sentinel:** any saved settings object whose `version` field is not `5`
is silently discarded and defaults are used. No migration is attempted. The
high-scale default changes bumped `bv2_settings_v1` → `bv2_settings_v2` so
old high-excitability/high-`iExt` saved values are discarded rather than
masking the new low-firing defaults. Removed fields (`pointRadius`,
`surfaceOpacity`, `surface`, `bloomStrength`) are ignored and fall back to
defaults because they are no longer in the saved schema.

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
controls above) persists under its **own** key `bv2_morph_v2`, independent of
`bv2_settings_v2`. Same versioned + merge-over-defaults shape:
`web/src/core/morph-config.ts → loadMorphConfig, saveMorphConfig, resetMorphConfig`,
with a `version` sentinel and `MorphologyConfig` defaults. Additive fields such
as the active-opacity lighting knobs and the dendrite branching controls are
backfilled from defaults when older saved `bv2_morph_v2` payloads omit them.
The high-scale default changes bumped `bv2_morph_v1` → `bv2_morph_v2` so
stale `lighting.restingBrightness` values do not suppress the new hidden-resting
default (`restingBrightness = 0.0`). The loader also normalizes to the current
known key set, so obsolete morphology fields from older saved payloads are
ignored on load and omitted on the next save/send to WASM. It is deliberately
NOT folded into `bv2_settings_v2` because it does not cross the frozen
Float32Array boundary — see
[`../decisions/dev-tooling.md`](../decisions/dev-tooling.md).

At boot, `web/src/main.ts` queues `morphConfigToJson(loadMorphConfig())` before
the backend exists and queues it again after `WasmGpuBackend.create()` succeeds,
so persisted morphology config reaches Rust on the first live frame without a
slider interaction. The dev panel constructor receives persisted Network/Drive
initial values from `main.ts`, so the Network tab no longer builds with defaults
and then rebuilds itself after `setInitialValues()`.

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
keys, including `bv2_morph_v2`.

## Float32Array index contract (corruption risk)

`web/src/core/settings.ts → toFloat32Array` serialises all 26 `VisualizerSettings`
fields into a `Float32Array` that is passed to the WASM backend. The index
assignment is the **shared contract** with `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`
(`from_slice`). **Reordering or inserting a field in either file without
updating the other silently corrupts all downstream visual settings.** The
authoritative index list is in the inline comments of `web/src/core/settings.ts →
VisualizerSettings` and `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`.
Index 9 (`connectionLightPast`), index 10 (`bloomStrength`), index 16
(`signalSource`), and index 23 (`adaptiveScalerEnabled`) are zero-written
tombstones. Index 1 (`pointRadius`), index 11 (`surfaceOpacity`), and index 20 (`surface`) are default-written
quarantined slots: TypeScript keeps runtime fields where needed for the frozen
type/metadata surface, but persistence and the panel omit the removed settings.

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
  the `MorphologyConfig` shape / `bv2_morph_v2` schema changes.
- The instant-tooltip mechanism (`_buildTooltip` / `_attachTip` / `data-tip`) changes.

## See also

- [`profiling.md`](profiling.md) — metrics pipeline that feeds Monitor/Dynamics tabs
- [`gpu-backend.md`](gpu-backend.md) — `VisualSettings.from_slice` consumer
- [`web-frontend.md`](../architecture/web-frontend.md) — `rafLoop` that calls `update()`
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
