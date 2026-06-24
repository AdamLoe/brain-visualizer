---
status:        shipped
owner:         adamg
last_updated:  2026-06-23
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - decisions/dev-tooling.md
---

# Dev-panel two-tier curation + Rebuild/Regenerate fix

## Mission

Two related changes ship together because both live in
`web/src/ui/dev-panel.ts` and the morphology-rebuild semantics overlap:

1. **Fix the Rebuild Morphology / Regenerate Network buttons** so they reliably
   apply their intended change instead of silently doing nothing / reverting.
   Two distinct semantics, asserted: **Regenerate Network = topology rebuild**
   (full worker-prepared network); **Rebuild Morphology = geometry rebuild,
   in-place** (no full network prepare).
2. **Two-tier curation of the dev panel.** The existing `?dev=1` / backtick /
   gear opener stays the **Essentials** view: ~12 beauty knobs only. A new
   `?dev=true` flag additionally reveals every **Advanced** control (all network
   + morphology tuning). Reload-to-get-Full is acceptable.

Done = (Phase 1) both buttons work and route correctly with the cheap path for
generator-only morph edits, axonCurveLift drift resolved, snapshot-alias fixed,
all named gates green; (Phase 2) `?dev=true` reveals Advanced rows, Essentials is
the ~12-knob keep-list, edgeSubsegments dispositioned, docs migrated.

## Scope

**In scope.** `web/src/ui/dev-panel.ts` (row gating, keep-list), `web/src/main.ts`
(`onMorphRebuild` routing, `rollbackStructuralState` gating, snapshot clone),
`web/src/gpu-build/network-build-client.ts` (surface worker error), the
`axonCurveLift` strip-vs-default drift across `web/src/core/morph-config.ts` and
`crates/brain-visualizer/src/sim/morphology.rs`, the stale shim comment at
`web/src/main.ts:66`, the `generator.edgeSubsegments` descriptor disposition, and
the matching doc migration.

**Out of scope / explicit non-goals.**
- **Do NOT touch the `VisualSettings` Float32Array index contract.** No
  reorder/insert/renumber of indices in `web/src/core/settings.ts → toFloat32Array`
  or `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings::from_slice`.
  Curation HIDES UI rows only. `SETTINGS_LENGTH` stays 28.
- **Do NOT delete the 7 inert Float32Array slots** (`pointRadius` idx1,
  `connectionLightPast`/`reserved_zero` idx9, `bloomStrength` idx10,
  `surfaceOpacity` idx11, `signalSource` idx16, `surface` idx20,
  `adaptiveScalerEnabled` idx23). They already have NO UI — invisible dead slots,
  not visible clutter. The tombstone decision
  (`decisions/dev-tooling.md → "Tombstone or quarantine dead Float32Array slots"`)
  forbids renumbering. Leave them; note them as known dead slots.
- No new persistence keys, no settings version bump, no migration logic. Hiding a
  row does not change what is persisted.
- No retuning of morphology ranges (exposure pass only — that decision stands).

## Background — verified code state (refs corrected against current code)

- **Dev-flag parse.** `web/src/ui/dev-panel.ts:493` reads
  `new URLSearchParams(window.location.search).get("dev") === "1"` and calls
  `this._setOpen(true)`. `web/src/main.ts` has no separate `dev` query parse (only
  the comment at `:706`). So the gate lives entirely in `dev-panel.ts`.
- **Both buttons funnel through `rollbackStructuralState`** (`web/src/main.ts:903`,
  not :903 exactly — the function header is at **:903**, confirmed). The rafLoop
  reads `networkBuildClient.currentStatus()` (`main.ts:1031`) and on a `failed`
  status with `sequence !== lastReportedNetworkBuildFailure` (`main.ts:1033-1041`)
  calls `rollbackStructuralState(...)` + `showToast(...)`, reverting BOTH network
  and morphology controls (`rollbackStructuralState` restores config, settings,
  AND morph via `rollbackMorphologyConfig`, `main.ts:904-924`).
- **Worker failures surface message-less.** `network-build-client.ts:38-44`
  `worker.onerror` sets `{ kind: "failed", message: event.message || "network
  build worker failed" }`; `event.message` is empty for many worker crashes (e.g.
  OOM at high N). `consumeReady` only flips status to idle on success
  (`network-build-client.ts:86-90`), so a stale `failed` lingers.
- **H2 confirmed.** `onMorphRebuild` (`main.ts:783-789`) sends ANY generator
  change to `requestPreparedNetwork("morphology generator", json)` — the
  heavyweight worker path — because `morphConfigRequiresPreparedNetwork`
  (`web/src/rebuild/rebuild-intent.ts:16-24`) returns true when
  `JSON.stringify(applied.generator) !== JSON.stringify(incoming.generator)`. The
  cheap in-place path already exists and already regenerates geometry:
  `GpuBackend::set_morphology_config` (`crates/brain-visualizer/src/sim/gpu/mod.rs:1091-1136`)
  computes `generator_changed` and calls `self.regenerate_morphology()`
  (`:1131-1133`). It is wired through `rebuildCoordinator.applyNext →
  applyMorphConfig → set_morphology_config` (`main.ts:1051-1053`, success at
  `:1068-1070`) and already has its own try/catch rollback (`main.ts:1072-1077`).
- **H3 confirmed.** `web/src/core/morph-config.ts:297` passes `["axonCurveLift"]`
  as the omit-list to `mergeKnownNumberGroup`, so `morphConfigToJson` (`:336`)
  **strips** `axonCurveLift` from serialized JSON. But `axonCurveLift` still
  exists in the type (`:26`) and `DEFAULT_MORPH_CONFIG.generator.axonCurveLift =
  0.15` (`:95`) and has **no `MORPH_DESCRIPTORS` entry** (so it is never
  editable). Rust deserializes via `#[serde(default)]` (`morphology.rs:365-373`,
  field `:373`) with its own default `0.15` (`morphology.rs:217`). Currently both
  defaults are 0.15 so geometry does not shift — but the dead, undescribed,
  stripped-on-save field is a latent drift trap and contradicts the architecture
  note that says duplicate `generator.axonCurveLift` is "accepted, normalized,
  and omitted on the next save".
- **H4 confirmed.** `main.ts:479`, `:626`, `:969` assign `lastSettingsSnapshot =
  getSettings()` directly. `getSettings()` (`web/src/core/settings.ts:279`) returns
  the shared live `current` ref, so `settingsRequirePreparedNetwork(
  lastSettingsSnapshot, settings)` (`main.ts:870`) can compare an object to
  itself. The clone form already exists at `main.ts:878` (`{ ...settings }`),
  `:905`, `:928`. Fix is cheap: clone at the three alias sites.
- **Stale shim.** `main.ts:60-69` declares `MorphCapableBackend` with a
  `TODO(v0.3.1): drop this shim once the regenerated pkg exports the method` —
  now stale (pkg exports `set_morphology_config`). Note for cleanup.
- **edgeSubsegments.** `generator.edgeSubsegments` IS a live descriptor
  (`morph-config.ts:198`, int 1–4, regenerate). `edgeSubsegmentsMax`
  (`:202`) is the budgeted cap. The brief calls `edgeSubsegments` legacy
  (superseded by adaptive subdivision) — disposition decided in Phase 2.

## Approach

Two serial phases. Phase 1 (the bug fix) is independent of Phase 2 (curation)
and lands first because it is the user-visible breakage; Phase 2 then hides those
(now-working) buttons behind Advanced. Within a phase, the stages are ordered.

### Phase 1 — Fix the buttons + routing semantics + cleanups

#### Stage 1.1 — Route generator-only morph edits in-place

- **Goal.** **Rebuild Morphology** for a generator-only change runs the cheap
  in-place `set_morphology_config` path, not the full worker network prepare.
  Assert the semantic split: Regenerate Network = topology; Rebuild Morphology =
  geometry, in-place.
- **Non-goals.** Do not change `requestPreparedNetwork` itself; do not change the
  Rust `set_morphology_config` (it already regenerates geometry on a generator
  change). Do not touch `connectionCurveLift` (it is a `VisualizerSettings`
  renderer-rebuild slot, idx7, not a morph-config generator field — leaves its
  existing `requestPreparedNetwork` path).
- **Authoritative docs.** `decisions/dev-tooling.md → "Morphology config on a
  separate key + WASM entry point"` and `"Most settings are live; rebuild-only
  controls stay explicit"`; `architecture/dev-panel.md → "Morphology config
  controls" → "Apply model"`.
- **Likely source areas.**
  - `web/src/main.ts → onMorphRebuild` (`:783-789`) — the routing decision.
  - `web/src/rebuild/rebuild-intent.ts → morphConfigRequiresPreparedNetwork`
    (`:16-24`) — currently the sole reason generator edits go to the worker.
  - `web/src/main.ts` rafLoop morph branch (`:1051-1053`, `:1068-1070`,
    `:1072-1077`) — the in-place apply target.
  - Rust `crates/brain-visualizer/src/sim/gpu/mod.rs → set_morphology_config`
    (`:1091-1136`), `regenerate_morphology` (`:1005`) — confirm geometry fully
    regenerates for a generator change. (Verified: `generator_changed →
    regenerate_morphology()`.)
- **Expected behavior.** A generator-field edit applied via the Rebuild
  Morphology button calls `rebuildCoordinator.requestMorphConfig(json)`, which on
  the next rAF turn calls `gpuBackend.set_morphology_config(json)` and updates
  `appliedMorphConfigJson` + persists — no worker round-trip, no network rebuild,
  no spurious rollback from a network failure.
- **Implementation notes.** Simplest correct change: make `onMorphRebuild` always
  route to `rebuildCoordinator.requestMorphConfig(json)` (the in-place path),
  dropping the `morphConfigRequiresPreparedNetwork` branch for the morph button —
  because the in-place Rust path already handles generator + render-quality +
  lighting. Confirm nothing else relies on a generator morph change forcing a
  *network* prepare (it should not: morphology geometry is downstream of the
  network, and a real topology change goes through Regenerate Network / the N/K/
  seed controls, which already call `requestPreparedNetwork`). If something does
  rely on it, instead keep `requestPreparedNetwork` only for the specific fields
  that genuinely change network structure and route the rest in-place — but the
  evidence says generator fields do not change topology. Decide and record which.
  `morphConfigRequiresPreparedNetwork` may become dead after this — remove it and
  its test if so, or leave it if still used for the boot/settings path (grep:
  it is only referenced by `onMorphRebuild`).
- **Cheapest sufficient checks.** `npm run typecheck`; `npm test` (vitest —
  includes `web/src/rebuild/*` and dev-panel/morph tests; update or delete the
  `morphConfigRequiresPreparedNetwork` test if the symbol is removed); `cargo
  test` (no Rust change expected, but the determinism gates confirm nothing
  shifted). The actual button click is browser-only —
  `web/e2e/rebuild_responsiveness.spec.ts` (Playwright) exercises it and **cannot
  run on this GPU-less box** (no WebGPU adapter). Name it as the real-browser
  gate the user must run before merge.
- **Stop and report if.** Generator fields turn out to change network topology
  (e.g. target ids) — then the in-place path is wrong for them and the routing
  must stay split; report which fields.
- **Open decisions.** Whether to delete `morphConfigRequiresPreparedNetwork`
  entirely (lean: yes, if `onMorphRebuild` is its only caller).

#### Stage 1.2 — Surface the real worker error + gate spurious rollback (H1)

- **Goal.** A silent/duplicate worker `failed` status cannot revert an
  already-applied newer build, and when a real failure occurs the toast shows a
  meaningful message instead of empty.
- **Non-goals.** Do not redesign the worker protocol; do not add retry. Do not
  remove rollback for genuine same-sequence failures.
- **Authoritative docs.** `decisions/dev-tooling.md → "Most settings are live;
  rebuild-only controls stay explicit"` (rollback contract);
  `architecture/dev-panel.md → "Rollback on failed structural apply"`.
- **Likely source areas.**
  - `web/src/gpu-build/network-build-client.ts → worker.onerror` (`:38-44`),
    `currentStatus` (`:63`), `consumeReady` (`:86-90`), the `failed` message path
    (`:108-120`).
  - `web/src/main.ts` rafLoop failed-status branch (`:1031-1041`),
    `lastReportedNetworkBuildFailure` (`:477`).
- **Expected behavior.** (a) `worker.onerror` produces a non-empty message
  (include `event.filename`/`event.lineno` or a generic "network build worker
  crashed (likely out of memory at this N)" when `event.message` is empty). (b)
  rollback only fires for a `failed` sequence that is the **latest requested** and
  has not been superseded by a later successful apply — i.e. guard on
  `networkBuildStatus.sequence === latestRequested` (or compare against the last
  successfully-applied sequence) so a stale `failed` from a superseded request
  cannot revert a newer applied build.
- **Implementation notes.** `network-build-client.ts` already tracks
  `latestRequested`; expose it (or a `isStaleFailure(seq)` helper) so the rafLoop
  guard is cheap. Keep the existing `lastReportedNetworkBuildFailure` de-dupe.
  Prefer the smallest change that (1) yields a useful message and (2) refuses to
  roll back a superseded/stale failed sequence.
- **Cheapest sufficient checks.** `npm run typecheck`; `npm test` — add/extend a
  `network-build-client` unit test asserting: empty `event.message` yields a
  non-empty status message; a `failed` status whose sequence is not the latest
  does not trigger rollback (this is unit-testable without a browser by driving
  the client + a small rafLoop seam, per MEMORY "Boot path needs stubbed
  integration test" — use the existing seams, not a real adapter).
- **Stop and report if.** The rafLoop guard would also suppress a legitimate
  same-sequence failure rollback — that must still fire.
- **Open decisions.** Exact message wording for the empty-`event.message` case.

#### Stage 1.3 — Fix `lastSettingsSnapshot` aliasing (H4)

- **Goal.** `lastSettingsSnapshot` is a clone, never the live `current` ref, so
  `settingsRequirePreparedNetwork` compares distinct objects.
- **Likely source areas.** `web/src/main.ts:479`, `:626`, `:969` — change
  `getSettings()` to `{ ...getSettings() }`. (`:878`, `:905`, `:928` already
  clone.)
- **Authoritative docs.** none beyond the rollback contract above.
- **Expected behavior.** Structural-setting diffs are detected correctly after a
  settings change; no self-comparison.
- **Cheapest sufficient checks.** `npm run typecheck`; `npm test`.
- **Stop and report if.** Cloning breaks a place that relied on the live
  reference identity (none expected — the other sites already clone).
- **Open decisions.** none.

#### Stage 1.4 — Resolve `axonCurveLift` strip-vs-default drift (H3)

- **Goal.** Remove the latent drift: a dead, undescribed field that is
  stripped on save but kept in the default with a serde-default twin on the Rust
  side.
- **Non-goals.** Do not add a UI control for it (it is not a beauty knob and the
  exposure decision stands). Do not change the Float32Array contract (this is the
  separate morph-config JSON channel).
- **Authoritative docs.** `decisions/dev-tooling.md → "Expose only bounded
  runtime-safe morphology knobs"` and `"Morphology config on a separate key"`;
  `architecture/dev-panel.md → "Morphology config controls"` (the note that
  duplicate `generator.axonCurveLift` is normalized + omitted).
- **Likely source areas.**
  - `web/src/core/morph-config.ts` — type field (`:26`), default (`:95`), the
    `["axonCurveLift"]` omit-list (`:297`).
  - `crates/brain-visualizer/src/sim/morphology.rs` — `axon_curve_lift` in
    `GeneratorConfig` serde struct (`:373`, default `:217`), its `apply_to`
    usage (`:291`), and the geometry consumer (`:2739`).
- **Expected behavior.** Recommended (lean): keep the Rust field and its default
  (it drives real geometry at `morphology.rs:2739`) and keep TS stripping it, but
  make the intent explicit and drift-proof — either (a) drop the TS-side
  `axonCurveLift` from `MorphologyConfig`/`DEFAULT_MORPH_CONFIG` entirely so TS no
  longer carries a value it never sends and Rust's `#[serde(default)] = 0.15`
  becomes the single source of truth, OR (b) keep TS carrying it but stop
  stripping (send it) so both sides agree explicitly. Pick (a) unless a test or
  artifact-capture path reads the TS field. Either way the two 0.15 defaults must
  stay locked, and the contract test (`web/src/ui/dev-panel.test.ts` checks
  descriptor defaults vs `DEFAULT_MORPH_CONFIG`) must still pass.
- **Cheapest sufficient checks.** `npm run typecheck`; `npm test` (morph-config +
  dev-panel descriptor-default test); `cargo test` (`morphology.rs:2967` asserts
  `axon_curve_lift == 0.15` — keep it green; the determinism gates confirm
  geometry unchanged).
- **Stop and report if.** Removing the TS field breaks a serialization round-trip
  test or an artifact-capture comparison that expects the key present.
- **Open decisions.** (a) drop TS-side field vs (b) stop stripping. Lean (a).

#### Stage 1.5 — Remove the stale shim TODO

- **Goal.** Drop or update the stale `TODO(v0.3.1)` at `main.ts:66`.
- **Implementation notes.** The pkg now exports `set_morphology_config`. If the
  generated `.d.ts` truly carries the method, the `MorphCapableBackend` cast at
  `main.ts:1052` can be replaced with a direct call and the interface deleted;
  otherwise just remove the stale TODO line and keep the shim with a current
  comment. Verify against the generated pkg type before deleting the cast.
- **Cheapest sufficient checks.** `npm run typecheck`.
- **Open decisions.** Delete cast vs keep shim — depends on the generated type.

### Phase 2 — `?dev=true` Advanced tier + keep-list + edgeSubsegments + docs

#### Stage 2.1 — Add the `?dev=true` Advanced gate

- **Goal.** `?dev=1` (or backtick/gear) opens the **Essentials** panel; `?dev=true`
  opens it AND reveals all **Advanced** rows. Reload to switch tiers is fine.
- **Non-goals.** No in-panel Essentials/Full toggle required (optional, not in
  scope). No change to the open/close mechanics, focus management, or tab
  keyboard handling. Do not change which tabs exist — gate rows/sections, and
  whole Advanced-only tabs, by tier.
- **Authoritative docs.** `decisions/dev-tooling.md → "Hidden dev panel, not a
  public settings page"`, `"Task-oriented settings IA over one oversized
  rendering tab"`; `architecture/dev-panel.md → "Opening the panel"`, `"Tabs"`.
- **Likely source areas.**
  - `web/src/ui/dev-panel.ts:492-495` — the existing `?dev` parse. Add an
    `advanced` flag: `params.get("dev") === "true"` (treat `"true"` as Full;
    `"1"` stays Essentials). Store on the instance (e.g. `this._advanced`).
  - The module comment at `:7` and `_setOpen` open triggers — keep `?dev=1` /
    backtick / gear as Essentials openers; backtick/gear open at the current
    tier (whatever the URL established at boot).
  - Tab build methods: `_buildAppearanceTab` (`:1512`), `_buildNetworkTab`
    (`:1154`), `_buildMorphologyTab` (`:1692`), `_buildMorphLightingRows`
    (`:1677`), `_buildMorphConfigRows` (`:1710`), the `TABS` array (Network and
    Morphology become Advanced-only; Monitor/Dynamics/Appearance/Debug/Storage
    stay). Row helpers `_sliderRow` (`:1907`), `_selectRow` (`:1938`),
    `_morphRow` (`:1749`), `_sep` (used throughout).
- **Expected behavior.** With `?dev=1`: only Essentials rows render (Network +
  Morphology tabs hidden or empty; Appearance shows only the keep-list; the
  Rebuild/Regenerate buttons are not present). With `?dev=true`: every current
  control renders exactly as today. Persistence and the Float32Array are
  identical in both tiers — hidden rows still have their persisted values; hiding
  is purely a render-time filter.
- **Implementation notes.** Prefer a single gating primitive: tag each
  Essentials control (by `VisualizerSettings` key for settings rows, by
  `jsonPath` for morph rows, and a small set for the lighting Essentials) in an
  `ESSENTIALS` allow-set, and have the row builders skip non-Essentials rows when
  `!this._advanced`. Skip whole sections/`_sep` headers that would end up empty.
  For tabs, either omit Advanced-only tab buttons from `TABS` when
  `!this._advanced`, or render the tab empty — prefer omitting the tab button so
  Essentials is genuinely minimal. Keep the gating in ONE place (an allow-set +
  `_advanced` check) rather than scattering `if` branches, mirroring the
  single-source-of-truth ethos of `SETTING_IMPACT`.
- **Cheapest sufficient checks.** `npm run typecheck`; `npm test` — add a
  dev-panel unit test that constructs the panel with the Essentials tier and
  asserts (a) the keep-list rows are present, (b) a representative Advanced row
  (e.g. `iExt`, `N`, a generator descriptor) is absent, and with the Full tier
  asserts all rows present. (jsdom-friendly; no browser needed.) The visual reveal
  is confirmable in a real browser only.
- **Stop and report if.** Hiding a row changes persistence or the Float32Array
  (it must not — if a build path reads the DOM row instead of the settings store,
  flag it).
- **Open decisions.** Whether Advanced-only tabs are omitted vs rendered-empty
  (lean: omit the tab button). Whether Storage/Monitor/Dynamics/Debug count as
  Essentials-visible (lean: keep Monitor + Dynamics + Storage visible as they are
  read-only/diagnostic; Debug is read-only labels — keep or gate, user's call).

#### Stage 2.2 — Finalize the Essentials keep-list

- **Goal.** Lock the ~12–13 Essentials beauty knobs; everything else is Advanced.
- **The keep-list (FINALIZED, 13 controls — see "Open decisions" for the count
  tension):**

  Appearance-tab `VisualizerSettings` rows (8):
  1. `colorBy` — "Color by" (select)
  2. `neuronVisibility` — "Neurons" (select)
  3. `glowTau` — "Glow decay (τ)"
  4. `neuronVisualRadius` — "Visual radius"
  5. `activeNeuronRadiusBoost` — "Active boost ×"
  6. `inactiveNeuronOpacity` — "Inactive opacity"
  7. `connectionLayer` — "Connections" (select)
  8. `revealOnArrival` — "Reveal on arrival" (select)

  Morphology-lighting descriptor rows (5):
  9. `lighting.ambient` — "Ambient"
  10. `lighting.diffuseIntensity` — "Diffuse intensity"
  11. `lighting.rimIntensity` — "Rim intensity"
  12. `lighting.activeBoost` — "Active boost" (brightness multiplier)
  13. `lighting.restingBrightness` — "Resting brightness"

  **Everything else → Advanced:** `voltageGlowStrength`, `connectionLightNext`,
  `morphRestingOpacity`, `connectionVisualWidth` ("Width"), `connectionCurveLift`
  ("Curve"), `arrivalHoldTicks` ("Arrival hold"); all lighting extras
  (`lightDirX/Y/Z`, `rimPower`, `activeOpacity` "Active coverage",
  `inactiveOpacityFloor` "Inactive coverage floor"); the whole **Network** tab
  (N/K/seed, **Regenerate network**, Excitability, Speed, I_ext, synaptic scale,
  heterogeneity, weight-norm, input-mode, the reach knobs, A/P region prototype);
  the whole **Morphology** tab (all 27 generator + render-quality descriptors and
  the **Rebuild Morphology** button).

- **Non-goals.** No relabeling, no range changes, no new controls.
- **Likely source areas.** `web/src/ui/dev-panel.ts → _buildAppearanceTab`
  (`:1512-1671`) and `_buildMorphLightingRows` (`:1677-1688`); the `ESSENTIALS`
  allow-set from Stage 2.1.
- **Expected behavior.** Essentials renders exactly these 13; nothing else.
- **Cheapest sufficient checks.** Covered by the Stage 2.1 unit test (assert the
  exact keep-list set).
- **Stop and report if.** A keep-list control turns out not to be a
  `VisualizerSettings`/lighting row reachable by the allow-set mechanism.
- **Open decisions.** **Count tension:** the brief's proposed list named "Active
  boost (neuron)" (= `activeNeuronRadiusBoost`, kept) AND lighting "Active boost
  (brightness)" (= `lighting.activeBoost`, kept) — two different "Active boost"
  controls. The brief also listed "Inactive opacity" (kept) and lighting
  "Resting brightness" (kept). This finalized list keeps both Active-boosts; the
  two share a label stem and may confuse — consider renaming the lighting one to
  "Active brightness" in a later pass (out of scope here). The brief listed
  "Connections (mode)" + "Reveal on arrival" but NOT "Arrival hold" for
  Essentials — honored (Arrival hold → Advanced), though note Reveal-on-arrival
  and the until-arrival fade are only meaningful together; flagged.

#### Stage 2.3 — Disposition `generator.edgeSubsegments`

- **Goal.** Decide demote-vs-remove for the legacy `generator.edgeSubsegments`
  descriptor (brief: superseded by adaptive subdivision).
- **Likely source areas.** `web/src/core/morph-config.ts:198`
  (`generator.edgeSubsegments` descriptor), `:202` (`edgeSubsegmentsMax`), the
  Rust consumer (`morphology.rs` — `edge_subsegments` in the serde struct +
  `EDGE_SUBSEGMENTS_MAX`).
- **Expected behavior.** Recommendation: **demote, do not remove.** It is an
  Advanced-tier control already (Stage 2.2 puts all generator descriptors in
  Advanced), and removing a generator field touches the morph-config JSON
  contract + Rust serde + the `decisions/dev-tooling.md → "Expose only bounded
  runtime-safe morphology knobs"` decision that explicitly names
  `EDGE_SUBSEGMENTS_MAX`. Lowest-risk: leave the descriptor as an Advanced knob
  and confirm whether adaptive subdivision has made it inert in Rust; if inert,
  note it as a future-removal candidate rather than removing it in this plan.
  Only remove if grep proves no Rust path reads `edge_subsegments`.
- **Cheapest sufficient checks.** `npm test`; `cargo test` if Rust touched.
- **Stop and report if.** `edge_subsegments` is still read by the axon-tree
  sampler (then it is not legacy — keep it, correct the brief's premise).
- **Open decisions.** Demote (lean) vs remove — gated on the Rust grep.

#### Stage 2.4 — Docs migration note (applied at ship, per the manifest)

- Per `docs/_meta/manifest.md` change→doc table, the rows changed here
  (`web/src/ui/dev-panel.ts`, `web/src/core/settings.ts`/`setting-metadata.ts`,
  `web/src/core/morph-config.ts` + `set_morphology_config` + `morphology.rs`
  config structs, and `web/src/main.ts`) require updating:
  **`architecture/dev-panel.md`** and **`decisions/dev-tooling.md`** (primary),
  plus `architecture/web-frontend.md` (the `onMorphRebuild` routing) and
  `architecture/manifold.md`/`architecture/gpu-rendering.md` only if morphology
  geometry behavior is described there (axonCurveLift). Specifically at ship:
  - `architecture/dev-panel.md`: add an **Essentials vs Advanced (`?dev=1` vs
    `?dev=true`)** subsection under "Opening the panel"/"Tabs"; update the
    "Morphology config controls → Apply model" to state Rebuild Morphology now
    runs the **in-place** `set_morphology_config` path (no worker prepare) for
    generator changes; update the "Rollback on failed structural apply" para to
    note the stale/superseded-sequence rollback guard; update the
    `axonCurveLift` normalization note.
  - `decisions/dev-tooling.md`: add a decision **"Two-tier dev panel:
    Essentials (`?dev=1`) vs Advanced (`?dev=true`)"** with the keep-list
    rationale; extend **"Most settings are live; rebuild-only controls stay
    explicit"** with the Regenerate=topology / Rebuild=in-place-geometry split
    and the rollback-guard fix; note the `axonCurveLift` and `edgeSubsegments`
    dispositions under **"Expose only bounded runtime-safe morphology knobs"**.
  - Confirm the manifest **drift-verification** rows stay true: `SETTINGS_LENGTH`
    28 unchanged, indices unchanged, `MorphUniforms` 192 B unchanged.

## Exit gate

- **Phase 1:** `cargo test`, `npm run typecheck`, `npm test` all green.
  Manual/browser: `web/e2e/rebuild_responsiveness.spec.ts` passes on a real
  WebGPU browser (cannot run on this box) — Regenerate Network rebuilds topology,
  Rebuild Morphology rebuilds geometry in-place without reverting, no spurious
  rollback toast.
- **Phase 2:** the new dev-panel Essentials/Full unit test passes (`npm test`);
  `npm run typecheck` green; `architecture/dev-panel.md` + `decisions/dev-tooling.md`
  reflect the two-tier IA and the rebuild-semantics fix; manifest
  drift-verification rows re-checked (Float32Array contract untouched).

## Discipline rules

- **Float32Array contract is frozen.** No index reorder/insert/delete in
  `settings.ts → toFloat32Array` or `gpu/mod.rs → VisualSettings::from_slice`.
  Curation hides UI only. Treat any diff touching those functions as a stop.
- Stage by filename; never `git add -A`. Run all three named gates before each
  commit. The e2e button test is browser-only and explicitly out of the
  headless gate set on this box.

## Migration notes (filled in at ship time)

Walk every fact/decision here into `architecture/dev-panel.md` and
`decisions/dev-tooling.md` per Stage 2.4; add an `_meta/ownership.json` entry
only if a genuinely new concept appears (none expected — Essentials/Advanced is a
sub-fact of the existing dev-panel ownership). Then set `status: shipped` +
`okay_to_delete: true`.

## See also

- `docs/plans/index.md` — where live plans land.
- `~/.agentdocs/plan-lifecycle.md` — status metadata + ship-time migration.
- `docs/architecture/dev-panel.md`, `docs/decisions/dev-tooling.md` — owning docs.
- `docs/_meta/manifest.md` — change→doc table + Float32Array drift-verification.
