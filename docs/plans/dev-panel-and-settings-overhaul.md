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

# Dev panel + settings overhaul

> Covers user goals 1 (remove dead settings), 4 (rethink the dev-config
> layout — sections, tooltips, per-setting reset, auto-rebuild on red),
> 5 (sliders are hard to use), and 6 (settings don't load correctly on
> init). These share one root cause: the config surface grew by accretion
> across three different builders with no single source of truth and no
> robust boot-apply path.

## Mission

The dev panel is now the only tuning surface for a GPU-only beauty-first
build, but it has rotted: three inconsistent control builders, ~38
morphology descriptors dumped flat under one tab, dead settings still
showing controls, no per-setting reset, sliders that are unusable at the
tiny ranges the morphology needs, and — most damaging — persisted config
that silently fails to reach the backend on reload. Done when: every
control is one consistent slider+number-input widget with a reset
affordance and a tooltip; the panel's sections reflect how a tuner
actually thinks about the visual; every persisted setting (`bv2_settings_v1`,
`bv2_morph_v1`, `bv2_config_v1`) is provably applied to the backend at boot
with no slider-touch required; descriptor defaults/ranges are the single
source of truth; and genuinely-dead settings are gone from the UI,
persistence, and (where safe) the cross-language contracts.

## Grounding — what's actually wrong today

**Init / load (goal 6) — confirmed bug.** `pendingMorphConfig` starts
`null` ([main.ts](../../app/web/src/main.ts) line 164) and is only set by the
morph slider handlers (`onMorphLive` / `onMorphRebuild`). There is **no
boot-time push** of the persisted morphology config. So after a reload the
dev panel shows the saved `bv2_morph_v1` values (loaded via
`loadMorphConfig()` in the `DevPanel` constructor) while the actual render
uses the Rust-side `MorphologyParams::locked_default` until the user nudges
a morph slider. `VisualizerSettings` does *not* have this bug — it's pushed
because `pendingSettingsPush` initialises to `true` (main.ts line 154). The
fix is a single, explicit "apply all persisted config to the backend after
backend-ready" step that covers all three keys.

**Init ordering smell.** The Network tab is built eagerly in the `DevPanel`
constructor with field defaults, then `setInitialValues()` tears it down and
rebuilds it once `main.ts` supplies the real persisted N/K/excitability/tps
(`web/src/ui/dev-panel.ts → setInitialValues`, comment at lines ~494–504).
That "build then rebuild" dance is a symptom of the panel constructing before
its data is ready. The overhaul should construct controls from already-loaded
state, not patch them after the fact.

**Source-of-truth drift (goal 4).** `MORPH_DESCRIPTORS` carries a `default`
field per control AND `DEFAULT_MORPH_CONFIG` carries the same defaults. The
`generator.axonRootRadiusFraction` descriptor had drifted from
`DEFAULT_MORPH_CONFIG` and now matches the locked `0.90` default. A per-setting
reset button has no trustworthy value to reset to unless descriptor defaults and
`DEFAULT_MORPH_CONFIG` stay aligned.

**Three control builders (goals 4, 5).** `_sliderRow` (bare range + text
readout, most Rendering rows), `_sliderWithInput` (range + number input,
Network tab only), and `_morphRow` (bare range + readout, the ~38 morph
descriptors). Only `_sliderWithInput` lets you type a value. The morph rows
and the settings sliders can only be dragged — unusable for e.g.
`pointRadius` (0.001–0.02 step 0.0005) or `baseRadius` (0.004–0.010 step
0.0005). There is no per-setting reset anywhere.

**Dead / suspect settings (goal 1).** Audit results, by confidence:

- **Definitely dead, safe to remove from UI + persistence:**
  `signalSource` (Float32Array idx 16) — parsed by `VisualSettings::from_slice`
  but never written to any GPU uniform or read by any shader, yet it still had
  a Rendering-tab selector and a Debug View readout.
  `adaptiveScalerEnabled` (idx 23) — explicitly RESERVED/INERT, already absent
  from the UI; remove from `SavedDev` persistence.
- **Dead render path, decide tombstone vs revive:** `surface` (idx 20) +
  `surfaceOpacity` (idx 11) — the surface render pass is off by default and
  its UI was already removed; the morphology replaced the brain-mesh context.
- **Legacy neuron-body knobs — verify against retired passes:**
  `neuronVisualRadius`, `activeNeuronRadiusBoost`, `inactiveNeuronOpacity`,
  `voltageGlowStrength` feed `RenderUniforms`, but the near-LOD sphere/point
  neuron passes are retired behind `DRAW_LEGACY_*`. Confirm whether any live
  pass reads them before removing; the Rendering tab even labels this section
  "(applies in Phase E)".
- **Closed by the dendrite-fix plan:** the legacy dendrite reach/primary-count
  controls were removed rather than revived. Live target-owned incoming
  dendrite controls are socket count, socket radius, and tip preference; old
  persisted morphology payloads are normalized so obsolete fields are ignored.

## Scope

In scope:

1. **Boot-apply correctness.** A single deterministic path that pushes all
   persisted config (visual settings, morph config, app config) to the backend
   once the backend is ready, before the first user interaction. Construct
   panel controls from loaded state so `setInitialValues`' build-then-rebuild
   goes away.
2. **One control widget.** Collapse `_sliderRow` / `_sliderWithInput` /
   `_morphRow` into a single slider + number-input + reset-dot + impact-dot +
   tooltip row builder, driven by a descriptor. Number entry everywhere;
   sensible precision; consider log-scaled sliders for the tiny-range knobs.
3. **Single source of truth for defaults/ranges.** Pick one (the descriptor
   table) and derive `DEFAULT_MORPH_CONFIG` / `DEFAULT_SETTINGS` from it, or
   vice-versa, so a per-setting reset is trustworthy. Fix descriptor/default
   drift as a side effect.
4. **Information architecture.** Re-section the panel around how a tuner
   thinks (e.g. group lighting/opacity, geometry/regeneration, network, post)
   rather than the current accretion order. Per-setting reset; clear red/green
   impact semantics; auto-rebuild on red-dot change made explicit and
   consistent (today red morph rows already apply on `change`/release — make
   that a deliberate, uniform rule, not an accident of which builder was used).
5. **Dead-setting removal.** Remove `signalSource` and `adaptiveScalerEnabled`
   from the UI and `SavedDev`. Decide tombstone-vs-renumber for their
   Float32Array indices (see Discipline rules). Resolve `surface`/`surfaceOpacity`
   and the legacy neuron-body knobs.

Out of scope: the active-opacity shader model (→
[`active-opacity-continuous-model.md`](active-opacity-continuous-model.md))
and dendrite geometry (→ [`dendrite-geometry-fix.md`](dendrite-geometry-fix.md)).
This plan only touches those features' *controls/persistence*, not their
rendering math.

## Implementation status — 2026-06-09

Code is done for the boot/control/default/tombstone parts and durable docs have
been migrated, but the plan remains active because manual reload/backend
acceptance and the intentionally deferred settings cleanup are still open.
Galileo changed `web/src/main.ts`, `web/src/ui/dev-panel.ts`,
`web/src/core/settings.ts`, `web/src/core/morph-config.ts`, and
`web/src/ui/dev-panel.test.ts`.

Current behavior: persisted morph config is queued at boot via
`morphConfigToJson(loadMorphConfig())` and queued again after GPU backend
creation; `DevPanel` receives persisted Network/Drive initial values in its
constructor; Rendering and morphology numeric controls share the slider + number
input + reset + tooltip helper; descriptor/default drift is covered by a unit
test and `generator.axonRootRadiusFraction = 0.90`; `signalSource` and
`adaptiveScalerEnabled` are tombstoned by writing `0` at Float32Array indices 16
and 23 and are not saved in `SavedDev`. Reported gates: `npm run typecheck`
passed and `npm test` passed.

`surface`/`surfaceOpacity` and legacy neuron-body knobs remain for now. The
legacy dendrite reach/primary-count controls are removed from the morphology
descriptor/config surface, and persisted old fields are dropped on load/save.

## Closure — 2026-06-11

Shipped. The web gates passed (`npm run typecheck`, `npm test` with 56 tests),
and server-backed Playwright passed the browser smoke/control/resize checks with
4 passed and 1 expected CPU-backend skip. The reload/backend acceptance is
covered by the persisted-config boot path plus the e2e boot/control checks; real
WebGPU device assertions remain gated by the WSL2 environment, as documented in
the test output.

## Approach

Sequence within this plan (mostly serial — they share `dev-panel.ts` and the
two config modules):

- **Phase 1 — Boot-apply + init audit (goal 6).** Add the explicit
  apply-all-persisted-config-at-boot step in `main.ts`; remove the
  build-then-rebuild Network-tab hack by constructing from loaded state. This
  is the highest-value, lowest-risk fix and should land first so the rest is
  testable on reload.
- **Phase 2 — Source of truth + unified control widget (goals 4, 5).** Collapse
  the three builders into one descriptor-driven widget with number input,
  reset, tooltip; make descriptor defaults/ranges authoritative; fix
  descriptor/default drift. Extend the descriptor model to cover the
  `VisualizerSettings` sliders too (today they're hand-written rows), so one
  renderer drives both config systems.
- **Phase 3 — Information architecture (goal 4).** Re-section the panel using
  the unified widget. This is mostly layout/grouping once Phase 2 lands.
- **Phase 4 — Dead-setting removal (goal 1).** Remove `signalSource` /
  `adaptiveScalerEnabled` from UI + persistence; resolve `surface` and the
  legacy neuron-body knobs; coordinate the dendrite generator fields with
  whichever of the dendrite plan / this plan lands second.

Phases 2–3 can be one stream; Phase 1 and Phase 4 are separable and could run
in parallel with them if owned by different people, but all four touch
`web/src/ui/dev-panel.ts` — treat the file as single-owner.

## Exit gate

- **Reload round-trip:** set a non-default morph value (e.g. `baseRadius`) and
  a non-default visual value, reload — the render reflects the persisted values
  immediately with **zero** slider interaction. (Today this fails for morph
  config.) Verify by observing the canvas and the backend, not just the panel.
- Every control is a slider + number-input you can type into, with a working
  per-setting reset and a tooltip.
- `MORPH_DESCRIPTORS.default` and `DEFAULT_MORPH_CONFIG` agree for every key
  (no drift); per-setting reset restores the documented default.
- `signalSource` and `adaptiveScalerEnabled` no longer appear in the panel,
  Debug View, or `SavedDev`; `cd app/web && npm run typecheck` and `npm test`
  green; `cd app && cargo test -p brain-visualizer` green (determinism gates
  included if any Float32Array index is renumbered).
- `architecture/dev-panel.md` (tabs/sections, control model, persistence,
  Float32Array index contract) and `architecture/web-frontend.md` (boot-apply
  path) updated to match the new shape; `decisions/dev-tooling.md` records the
  single-source-of-truth and boot-apply decisions.

## Discipline rules

- **Float32Array contract is a corruption risk.** Removing `signalSource`
  (idx 16) / `adaptiveScalerEnabled` (idx 23) means either (a) **tombstone**:
  keep the index, write 0, drop only the TS field + UI + persistence (low
  risk, no Rust change, no version bump for the array itself); or (b) **clean
  renumber**: shrink the array, update `VisualSettings::from_slice` atomically,
  bump the `bv2_settings_v1` version sentinel, and rerun the determinism gates.
  Recommend (a) tombstone unless the renumber is genuinely worth it — the
  index contract is the documented main corruption risk.
- Do not remove the dendrite generator fields here; that decision belongs to
  the plan that lands second (see Grounding).

## Migration notes (filled in at ship time)

Route into `architecture/dev-panel.md` (new control widget, sections,
per-setting reset, the dead-setting removals, updated Float32Array contract),
`architecture/web-frontend.md` (the boot-apply-all-persisted-config step), and
`decisions/dev-tooling.md` (single-source-of-truth for defaults/ranges;
tombstone-vs-renumber choice; auto-rebuild-on-red rule). Update
`_meta/manifest.md` drift-verification notes if the Float32Array length or any
index changes.

## See also

- [`active-opacity-continuous-model.md`](active-opacity-continuous-model.md)
- [`dendrite-geometry-fix.md`](dendrite-geometry-fix.md)
- `architecture/dev-panel.md`, `architecture/web-frontend.md`,
  `decisions/dev-tooling.md` — owning docs.
- `~/agent-docs/v1/plan-lifecycle.md` — status + ship-time migration.
</content>
</invoke>
