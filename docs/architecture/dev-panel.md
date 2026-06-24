---
status:        active
owner:         adamg
last_updated:  2026-06-23
---

# Dev Panel

A hidden right-docked drawer (`web/src/ui/dev-panel.ts → DevPanel.PANEL_WIDTH_PX`)
for live metrics, tunable settings, review presets, and storage diagnostics
without cluttering the public UI.

## What it owns

- `DevPanel` class, open/close triggers, tab layout — `web/src/ui/dev-panel.ts → DevPanel`
- Hidden review preset payloads — `web/src/ui/dev-panel.ts → HIDDEN_REVIEW_PRESETS`
- `SysInfo` and `SimHandlers` callback interfaces — `web/src/ui/dev-panel.ts`
- Setting-impact classification system and colored dot rendering — `web/src/core/setting-metadata.ts → SETTING_IMPACT`
- `VisualizerSettings` interface, persistence, `toFloat32Array` serialisation — `web/src/core/settings.ts → VisualizerSettings`
- The localStorage schema and merge-over-defaults contract — `web/src/core/settings.ts → loadSettings, mergeOver`

## What it does NOT own

- The metrics pipeline that produces the numbers displayed in Monitor and Dynamics tabs — [`profiling.md`](profiling.md)
- The GPU simulation backend that consumes the settings Float32Array — [`gpu-backend.md`](gpu-backend.md)
- The public corner HUD — [`profiling.md`](profiling.md)

## Opening the panel

The open triggers are documented in the `web/src/ui/dev-panel.ts` module
comment: URL opt-in, keyboard toggle, and the bottom-right gear button wired by
`web/src/main.ts`.

The panel is closed by default. When closed, `pointer-events` apply only to the
canvas; the overlay never intercepts input.

`update(m, sys?)` is called from the once-per-second block in `rafLoop`
(`web/src/main.ts`) **only when `isOpen()` returns true** — no DOM work is done
when the panel is hidden.

The drawer is a desktop diagnostics surface only. `web/src/main.ts` does not
construct `DevPanel` when `isMobile()` is true and hides the gear button; mobile
diagnostics are intentionally unsupported until a future design owns that
workflow. Screenshot review of diagnostics uses desktop viewports.

Open/close focus is deterministic: `DevPanel._setOpen` remembers the opener,
focuses the close button after opening, and returns focus to the opener after
close. The tab strip is a real `tablist`/`tab`/`tabpanel` set with roving
`tabIndex`, `aria-selected`, and Arrow/Home/End keyboard handling. Tooltips are
still body-appended, but `_attachTip` also makes them focus-readable through
`focusin`/`focusout`, so keyboard users can discover the same help as hover
users. Sliders, number inputs, selects, reset buttons, and the public gear/pause
buttons carry explicit accessible names.

### Essentials vs Advanced (`?dev=1` vs `?dev=true`)

The panel renders in one of two tiers, fixed at construction from the `dev` URL
param (`web/src/ui/dev-panel.ts → _advanced`):

- **Essentials** (`?dev=1`, or backtick / gear opening at the boot tier) shows
  only a curated set of beauty knobs. The **Network** and **Morphology** tabs
  are omitted; **Monitor / Dynamics / Storage / Debug** stay visible.
- **Advanced** (`?dev=true`) reveals every row and every tab, exactly as the
  full panel.

Reload to switch tiers. Gating is **render-time only**: persistence and the
`VisualSettings` Float32Array are identical in both tiers — hidden rows keep
their persisted values, and hiding a row never changes what is sent to the
backend. The gating uses a single allow-set primitive, the source of truth for
the keep-list: `ESSENTIALS_SETTINGS` (by `VisualizerSettings` key),
`ESSENTIALS_MORPH` (by `jsonPath`), and `ADVANCED_ONLY_TABS`, all in
`web/src/ui/dev-panel.ts`. The Essentials keep-list is owned by those sets; see
[`../decisions/dev-tooling.md`](../decisions/dev-tooling.md) for the rationale.

## Tabs

Tabs are defined by `web/src/ui/dev-panel.ts → TABS`. The IA separates metrics,
network/drive controls, live appearance, morphology, debug, and storage/reset.

The Appearance tab groups render knobs that live in the `VisualSettings`
Float32Array. The rendered rows are owned by `web/src/ui/dev-panel.ts →
_buildAppearanceTab`; index assignment, tombstones, and default-written slots are
owned by `web/src/core/settings.ts → VisualizerSettings, toFloat32Array`.

`connectionLayer` is surfaced as the "Connections" dropdown in the "Morphology
Visibility" section. It offers off, active/recent, and until-arrival visibility,
and the fresh-state default is **until-arrival** (mode 2). Until-arrival keeps a
subdued view of the whole fired arbor until the aggregate packet arrives, then
fades it out over the live "Arrival hold" window; the "Arrival hold" slider is
the **fade-out duration** in ticks (the subdued branch ramps brightness and
opacity to nothing over those ticks, not "extra ticks kept fully visible"). The
"Reveal on arrival" On/Off toggle (right after "Arrival hold", `live`) layers on
until-arrival: when on, each segment stays hidden until the impulse front reaches
it (reveal-as-drawn), so the arbor draws in along the pulse instead of appearing
at once. It is off by default and inert in the off / active-recent modes; the
render-side front-gate is owned by [`gpu-rendering.md`](gpu-rendering.md).
Its current values and normalization live in
`web/src/core/settings.ts → normalizeConnectionLayer` and
`crates/brain-visualizer/src/sim/gpu/mod.rs → normalize_connection_layer`. The
resting-debug mode is not exposed and is not a runtime mode.

The tab also contains a "Morphology Lighting" section that renders the
`MORPH_DESCRIPTORS` rows with `group === "lighting"` via
`_buildMorphLightingRows`. These are `live`/`uniform` lighting controls;
`applyKind === "uniform"` rows call `onMorphLive` immediately.

The `Color by` selector includes Brain mode and reuses the existing `colorBy`
setting rather than adding a persistence key. Point-radius and surface controls
are not exposed or persisted; their Float32Array slots remain
compatibility slots written from defaults. The Rust surface path is active
through the default-written `surface = 1` slot so first boot has a dim
folded-brain shell, while the current product settings UI keeps surface controls
quarantined.

The settings boundary is guarded by executable contract tests on both sides:
TypeScript locks `toFloat32Array(DEFAULT_SETTINGS)`, length, and
reserved/default-written slots (`npm test`), while Rust locks
`VisualSettings::from_slice` index mapping and tombstone behavior (`cargo test`).
This protects the positional contract without adding a named-field protocol.

The Morphology tab renders the **morphology config** controls: a separate
surface (see the Morphology config controls section below) that does **not**
touch the Float32Array. These expose the generator and render-quality
parameters of the procedural neuron geometry, backed by their own
`bv2_morph_v2` localStorage key and a dedicated WASM entry point.

## Network-state classifier

The Monitor tab shows a SILENT / TUNED / OVERACTIVE verdict. Thresholds live as
constants next to `web/src/ui/dev-panel.ts → classify`; avoid duplicating them
here.

`pctFired*` values arrive from the GPU as fractions in [0, 1]; the panel
multiplies by 100 for display. The branching-ratio critical-band thresholds used
in the Dynamics tab are separate constants in the same file.
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

## Region Assignment Prototype

The Network tab includes a hidden-review checkbox labelled "A/P region
prototype". It toggles `web/src/core/types.ts → AppConfig.regionAssignmentMode`
between the default `"hash-random"` mode and the opt-in
`"anterior-posterior-prototype"` mode. It is deliberately not a
`VisualizerSettings` field and does not add a Float32Array index; changing it
persists through `CONFIG_LS_KEY` and requests a worker-prepared network rebuild
through `web/src/main.ts → requestPreparedNetwork`.

The checkbox defaults off because `DEFAULT_CONFIG.regionAssignmentMode` is
`"hash-random"`. `loadConfig()` normalizes unknown saved strings back to that
default. The Rust enum/order/type-byte encoding remains owned by
[`manifold.md`](manifold.md) and [`data-model.md`](data-model.md).

## Setting-impact classification

Every control in the Rendering and Network tabs carries a colored dot. The dot
color is driven entirely by `web/src/core/setting-metadata.ts → SETTING_IMPACT`, which
is the **single source of truth** for every setting's impact level:

`"live"` controls take effect through the next settings push, `"brain-reset"`
controls request a worker-prepared network rebuild, and `"renderer-rebuild"`
controls change generated or pipeline-owned geometry. `SETTING_IMPACT` is the
source of truth: `connectionCurveLift` is renderer-rebuild, the heavy-tailed
reach knobs are brain-reset, and the integrate-uniform knobs
(`heterogeneity`, `weightNormalization`, `inputMode`) are live because the
backend reads them every tick.
N/K/seed and region-assignment mode live in `AppConfig` and are out of scope
for `SETTING_IMPACT`.

Reserved/inert fields are kept only to preserve the Rust<->TypeScript
`VisualSettings` contract (see the index-contract section below). Their exact
slots and write behavior live in `web/src/core/settings.ts → toFloat32Array` and
`crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice`.
Re-arming a scaler is deferred — see
[`../plans/future_roadmap.md`](../plans/future_roadmap.md) and
[`scaling.md`](scaling.md).

## Hidden review presets

The Storage tab exposes **code-defined**, dev-only review presets. The preset
table lives in `web/src/ui/dev-panel.ts → HIDDEN_REVIEW_PRESETS`.
`accepted-default` is derived from `DEFAULT_CONFIG`, `DEFAULT_SETTINGS`, and
`DEFAULT_MORPH_CONFIG`; the other presets override visual/morphology payloads
for review. These are static review buttons, not a public preset manager.

## Morphology config controls

The morphology generator and render-quality parameters of the procedural neuron
geometry are exposed through a **descriptor-driven** surface independent of the
`VisualizerSettings` Float32Array. `web/src/core/morph-config.ts →
MORPH_DESCRIPTORS` is the single source of truth; the dev panel renders rows via
`web/src/ui/dev-panel.ts → _buildMorphConfigRows, _morphRow` rather than
hand-written controls. Descriptor defaults are checked against
`DEFAULT_MORPH_CONFIG` by `web/src/ui/dev-panel.test.ts` (`npm test`).

Descriptor groups, ranges, defaults, impact levels, and apply kinds live in
`web/src/core/morph-config.ts → MORPH_DESCRIPTORS` (the nested config shape is
`MorphologyConfig`). Generator and render-quality rows are rendered in the
Morphology tab by `_buildMorphConfigRows`; lighting rows are rendered in the
Appearance tab under "Morphology Lighting" by `_buildMorphLightingRows`.

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

**Rebuild Morphology runs in-place, never a worker prepare.** `onMorphRebuild`
(`web/src/main.ts`) ALWAYS routes to
`rebuildCoordinator.requestMorphConfig(json)`, which on the next rAF turn calls
`gpuBackend.set_morphology_config(json)` → Rust `regenerate_morphology`. A
generator edit therefore regenerates axon-tree geometry **in place** with no
worker round-trip and no network rebuild. This asserts the semantic split:
**Regenerate Network = topology rebuild** (full worker prepare via the N/K/seed
controls); **Rebuild Morphology = geometry rebuild, in-place**. Morphology
geometry is downstream of the network, so a generator change never changes
topology.

**Dendrite generator controls.** The target-owned incoming dendrite generator is
controlled by descriptor rows for socket placement, soma-proximal branching,
decoration, and bounded path sampling. The descriptor defaults/ranges are owned
by `MORPH_DESCRIPTORS`, with defaults checked against `DEFAULT_MORPH_CONFIG` by
`web/src/ui/dev-panel.test.ts` (`npm test`). Decoration and path-sampling
controls are clamped to existing generator/buffer limits; shader tube curvature
is render-owned rather than a separate dev-panel knob. Waypoint counts and
allocation budgets remain protected.

Obsolete persisted morphology fields such as `dendritePrimaryMin` and
`dendritePrimarySpan` are accepted, normalized to the current known key set, and
omitted on the next save.

`generator.axonCurveLift` is a special non-editable case: it has **no
`MORPH_DESCRIPTORS` entry**, so it is never user-tunable. `DEFAULT_MORPH_CONFIG`
pins it to exactly `0.15` and `morphConfigToJson` omits it from the saved JSON
(its `normalizeMorphConfig` omit-list ignores any persisted override). That omit
is inert: Rust supplies the same `0.15` via `#[serde(default)]`
(`crates/brain-visualizer/src/sim/morphology.rs → GeneratorConfig`), and Rust's
`axon_curve_lift` is the **effective source** for real axon-tree geometry
(consumed in the sampler). Both defaults are locked at `0.15`; unpinning the TS
value requires adding a descriptor (see the deliberate-pin comment in
`web/src/core/morph-config.ts → normalizeMorphConfig`).

## Settings persistence contract

**Keys:** `web/src/core/settings.ts → SETTINGS_LS_KEY`
(`bv2_settings_v2`), `web/src/core/morph-config.ts → MORPH_CONFIG_LS_KEY`
(`bv2_morph_v2`), and `web/src/core/types.ts → CONFIG_LS_KEY`
(`bv2_config_v2`).

**Schema:** settings persist as `{ version: 5, public: {…}, dev: {…} }`. The
`public` sub-object holds user-facing beauty knobs; `dev` holds tuning/debug
knobs. The split is defined by the `SavedPublic` and `SavedDev` interfaces in
`web/src/core/settings.ts`; clean defaults are owned by `DEFAULT_SETTINGS`.
Removed visual fields such as `bloomStrength` are omitted from both
`SavedPublic` and `SavedDev`.

**Version sentinel:** any saved settings object whose `version` field is not `5`
is silently discarded and defaults are used. No migration is attempted. Stale
versioned payloads are discarded rather than allowed to mask clean defaults.
Removed fields (`pointRadius`, `surfaceOpacity`, `surface`, `bloomStrength`) are
ignored and fall back to defaults because they are absent from the saved schema.

**Merge-over-defaults:** on load, each saved key is merged over
`DEFAULT_SETTINGS` with `?? base_value` guards (`web/src/core/settings.ts → mergeOver`),
then normalized to the finite ranges/enums exposed by the panel. Missing fields
(from older saves or partial schemas) fall back silently. Adding a new field to
`VisualizerSettings` is safe without a version bump; only semantically breaking
changes (changed defaults, repurposed indices) require a bump.

**Never persist counters:** `VisualizerSettings` contains only durable
configuration knobs. Runtime metrics live on `Metrics` and are never written to
localStorage.

**Separate morphology config key.** The morphology config (see Morphology config
controls above) persists under its **own** key `bv2_morph_v2`, independent of
`bv2_settings_v2`. Same versioned + merge-over-defaults shape:
`web/src/core/morph-config.ts → loadMorphConfig, saveMorphConfig, resetMorphConfig`,
with a `version` sentinel and `MorphologyConfig` defaults. Additive fields such
as the active-opacity lighting knobs and the dendrite branching controls are
backfilled from defaults when saved `bv2_morph_v2` payloads omit them. The
loader also normalizes to the current known key set and descriptor ranges, so
obsolete morphology fields are ignored on load and omitted on the next save/send
to WASM, while stale out-of-range values are clamped to the current control
bounds. It is deliberately NOT folded into `bv2_settings_v2` because it does not
cross the frozen Float32Array boundary — see
[`../decisions/dev-tooling.md`](../decisions/dev-tooling.md).

At boot, `web/src/main.ts` queues `morphConfigToJson(loadMorphConfig())` before
the backend exists and queues it again after `WasmGpuBackend.create()` succeeds,
so persisted morphology config reaches Rust on the first live frame without a
slider interaction. The dev panel constructor receives persisted Network/Drive
initial values from `main.ts`, so the Network tab is built from the current
runtime config.

**Reset:** `web/src/core/settings.ts → resetSettings` removes the settings
localStorage key, restores `current` to `DEFAULT_SETTINGS`, and notifies all
subscribers synchronously — which causes the dev panel's `_syncSliders` to
restore settings-backed control positions. The Storage reset handler also clears
the morphology key via `web/src/core/morph-config.ts → resetMorphConfig`,
re-syncs the morphology rows, calls `web/src/core/types.ts → resetConfig`, syncs
the Network tab controls to `DEFAULT_CONFIG`, updates `main.ts`'s in-memory
`AppConfig`, and schedules a rebuild back to the default N/K/seed. Reset restores
the live network to the clean default values, not only the stored controls. The
storage readout shows the app-owned keys,
including `bv2_morph_v2`.

**Rollback on failed structural apply:** structural settings and morphology
generator/render-quality changes may temporarily update controls while a
worker-prepared payload or morphology rebuild is being applied, but app-owned
localStorage stays on the last applied structural state until the backend apply
succeeds. `web/src/main.ts → rollbackStructuralState` restores the last applied
`VisualizerSettings`, `AppConfig`, and morphology JSON if worker preparation,
`apply_prepared_network`, or morphology apply fails. The dev panel syncs controls
back through `_syncSliders`, `setInitialValues`, and
`rollbackMorphologyConfig`, so reloads do not silently trust an unproven
structural state.

A **stale/superseded `failed` build never rolls back a newer applied build.**
The rafLoop failed branch guards on
`web/src/gpu-build/network-build-client.ts → isStaleFailure(sequence)`
(`sequence !== latestRequested`) in addition to the existing
`lastReportedNetworkBuildFailure` de-dupe, so a `failed` status from a
superseded request cannot revert a build the user has since re-requested and
applied. Worker `onerror` always surfaces a **non-empty** message —
`filename:lineno` when available, else "network build worker crashed (likely out
of memory at this N)" — so OOM-at-high-N crashes (which fire with an empty
`event.message`) no longer produce a blank rollback toast.

## Float32Array index contract (corruption risk)

`web/src/core/settings.ts → toFloat32Array` serialises `VisualizerSettings`
into the `Float32Array` passed to the WASM backend. `SETTINGS_LENGTH` is locked
by `web/src/core/settings-contract.test.ts` (`npm test`). The index
assignment is the **shared contract** with `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`
(`from_slice`). **Reordering or inserting a field in either file without
updating the other silently corrupts all downstream visual settings.** The
authoritative index list is in the inline comments of `web/src/core/settings.ts →
VisualizerSettings` and `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings`.
Tombstoned and quarantined slots are documented beside the serializers; Rust
mapping and tombstone behavior are covered by the `VisualSettings::from_slice`
tests (`cargo test`).

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
