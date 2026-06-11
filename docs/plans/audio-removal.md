---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-frontend.md
  - architecture/profiling.md
  - architecture/scaling.md
  - decisions/interaction.md
---

# Audio removal

## Mission

Remove the audio/sonification feature from Brain Visualizer rather than hiding
it behind UI state. Done when the shipped web app has no sound toggle, creates
no `AudioContext`, ships no Web Audio implementation module, carries no
audio-specific profiler branch, and the docs describe the remaining public UI
and profiler/HUD behavior without sonification.

This is a planning-only stream under
`docs/plans/visual-product-polish-phase-hub.md`. The authoritative code surface
is `app/web/src/audio/sonification.ts` and its import/call sites in
`app/web/src`; docs are secondary and must be updated only after implementation
ships.

## Current-State Findings

The audio feature is product-facing and currently active on desktop only:

- `app/web/src/audio/sonification.ts` exports `SonificationEngine` and
  `deriveRegionFractions`. `SonificationEngine.enable()` creates a Web Audio
  graph with three sine oscillators, a low-pass filter, master gain, and an
  optional `ScriptProcessorNode` noise layer. `deriveRegionFractions()` exists
  only to feed that audio engine from total profiler counts.
- `app/web/src/main.ts` imports both exports, constructs
  `new SonificationEngine()` at boot, wires `#sound-toggle`, hides the sound
  button on mobile, and calls `sonification.update()` from the once-per-second
  profiler/HUD branch.
- `app/web/index.html` renders the top-right `#sound-toggle` button and comments
  frame the public controls as "sound + settings only".
- `app/web/src/render/profiler.ts` has no audio logic, but several comments and
  method docs describe `lastSnapshot` / `getLastSnapshot()` as feeding
  sonification as well as the corner HUD.
- No audio-specific persistence keys or settings controls were found. The
  versioned persisted config/settings surfaces are unaffected except for
  comments/docs that mention mobile "no sound".
- No dedicated sonification unit/e2e tests were found. Existing e2e specs do
  not appear to assert the sound toggle.

Docs with known audio references:

- `docs/architecture/web-frontend.md` lists sonification in the app-shell
  inventory, has a dedicated `## Sonification` section, mentions "no sound
  toggle" in the mobile profile, and has an update trigger for future
  sonification data.
- `docs/decisions/interaction.md` has the durable decision
  `## Sonification opt-in, muted by default`, plus a later profiler snapshot
  mention.
- `docs/architecture/index.md`, `docs/decisions/index.md`,
  `docs/repository-layout.md`, `docs/architecture/scaling.md`,
  `docs/_meta/manifest.md`, and `docs/_meta/ownership.json` mention
  sonification/audio in routing, inventory, ownership, or change-to-doc text.

## Removal Strategy

Lead decision on 2026-06-09: nuke all audio-related source, UI, docs, and tests.
Do not park reusable sonification code behind an unreachable path.
Follow-up lead decision on 2026-06-09: remove audio from current-state docs
entirely; do not keep a short "audio intentionally removed" rationale.

1. Delete the feature module.

   Remove `app/web/src/audio/sonification.ts`. If the now-empty
   `app/web/src/audio/` directory has no remaining files, remove the directory
   too. Do not keep a stub export; keeping an importable module invites silent
   reintroduction.

2. Remove top-bar sound UI.

   In `app/web/index.html`, delete `#sound-toggle` and update adjacent comments
   so the top-right public controls are settings-only. Keep the settings button
   and its pointer-event behavior intact.

3. Remove `main.ts` sonification wiring.

   Delete the import from `./audio/sonification`, the `new SonificationEngine()`
   instance, the `#sound-toggle` event-handler block, the mobile "no sound"
   comments/log text, and the profiler branch that derives region fractions and
   calls `sonification.update()`. Preserve the once-per-second HUD and dev-panel
   monitor update cadence. After removal, `Profiler.getLastSnapshot()` should
   still be used by HUD/dev-panel paths only.

4. Narrow profiler comments to actual consumers.

   In `app/web/src/render/profiler.ts`, change comments and method docs from
   "HUD / sonification" to the current consumers: corner HUD and dev-panel /
   monitor logic. Do not change the profiler API unless it becomes provably
   unused after the `main.ts` cleanup.

5. Update current-state docs after code lands.

   Remove or rewrite audio references in:
   `docs/architecture/web-frontend.md`, `docs/decisions/interaction.md`,
   `docs/architecture/index.md`, `docs/decisions/index.md`,
   `docs/repository-layout.md`, `docs/architecture/scaling.md`,
   `docs/_meta/manifest.md`, and `docs/_meta/ownership.json`.

   The interaction decision should not remain as "opt-in audio"; replace it
   with a short removed/superseded note only if the docs convention requires
   preserving a rationale. Otherwise remove it from the current decisions index
   because decisions docs describe what still holds.

## Owned Files Likely Impacted During Implementation

- `app/web/src/audio/sonification.ts` — delete.
- `app/web/index.html` — remove `#sound-toggle` and stale comments.
- `app/web/src/main.ts` — remove import, instance, click handler, mobile sound
  comments/log text, and once-per-second audio update branch.
- `app/web/src/render/profiler.ts` — comment/docstring cleanup only.
- `docs/architecture/web-frontend.md` — remove sonification inventory/section and
  update mobile profile/update triggers.
- `docs/architecture/profiling.md` — check for profiler consumer wording after
  implementation.
- `docs/architecture/scaling.md` — remove the "HUD and sonification" wording.
- `docs/architecture/index.md` — remove sonification from web-frontend routing.
- `docs/decisions/interaction.md` and `docs/decisions/index.md` — remove or
  supersede the opt-in sonification decision.
- `docs/repository-layout.md` — remove the `audio/sonification.ts` inventory.
- `docs/_meta/manifest.md` and `docs/_meta/ownership.json` — remove or update
  audio/sonification anchors and ownership summaries.

## Persistence And Settings Impact

No `localStorage` key, `AppConfig`, `SavedDev`, `VisualSettings`, or dev-panel
metadata field appears to represent audio state. Implementation should avoid a
settings-version bump unless a later audit finds an audio field hidden in a
persisted object. The expected outcome is no persistence migration.

## Test Plan

Per-stream gates should stay narrow:

- `cd app/web && npm run typecheck` after code deletion.
- `cd app/web && npm test -- --run src/render/profiler.ts src/ui/controls.test.ts`
  only if profiler or control comments/API-adjacent code changes make a targeted
  unit gate useful. If no behavior-bearing tested file changes, skip this and
  rely on typecheck.
- `cd app/web && npm run test:e2e -- --grep "settings|pause"` only if the
  implementation adjusts top-control selectors or spacing enough to risk
  existing Playwright UI locators. Do not run the full e2e suite for this stream.
- Manual browser smoke, desktop only: verify the top-right controls show the
  settings gear without a sound button, opening settings still works, the corner
  HUD still updates once per second, and no console logs mention
  `[sonification]`.

No Rust/Cargo gate is needed because the current audio surface is entirely web
frontend and docs.

## Sequencing Constraints

- Coordinate with the settings-overhaul stream before editing
  `app/web/index.html` or `app/web/src/main.ts`; both streams may touch the
  top-control area and dev-panel toggle comments.
- Coordinate with defaults/max-scale only if that stream edits mobile profile
  comments/logging in `main.ts`.
- Coordinate with docs migration after the broader product-polish wave so
  current-state docs do not claim a partially implemented intermediate.

## Risks And Fallback

Audio is not intertwined with simulation ticks, WebGPU rendering, settings
persistence, or backend contracts. It is attached to the once-per-second profiler
snapshot path, so the primary implementation risk is accidentally removing or
renaming profiler data still needed by `CornerHud` and the dev-panel monitor.

Fallback if implementation reveals deeper coupling: split into two steps. First,
land a recon/refactor patch that removes the product-facing `#sound-toggle` and
isolates profiler consumers behind HUD/dev-panel names. Second, delete the audio
module and remaining import sites once typecheck proves no hidden consumers
remain.

## Questions For The Lead

- Should the top-right public controls become gear-only, or should the settings
  entry move as part of the separate settings-overhaul stream?
- Should the manual acceptance criteria include checking that browser autoplay /
  audio permission prompts never appear, or is absence of `AudioContext`
  construction sufficient?

## Deferrals

- No replacement audiovisual feature is planned in this stream.
- No profiler API simplification beyond removing audio-facing comments is
  planned unless typecheck shows dead code after the audio branch is gone.
- No full-suite verification is proposed for this stream; consolidated gates
  belong to the product-polish hub after implementation waves merge.

## Exit Criteria

- The implementation removes all product-facing audio controls and source code.
- `rg -n "sonification|sound-toggle|AudioContext|ScriptProcessor|\\[sonification\\]" app/web/src app/web/index.html`
  returns no live product references.
- Current-state docs no longer route readers to audio as an active feature.
- Narrow web gate(s) listed above pass, with any skipped gate called out.

## Migration Notes

Migrated on 2026-06-11 by verifying no audio/sonification references remain in
current-state architecture/decision/source-inventory docs. Per lead direction,
no current-state rationale note was retained. Historical references remain only
inside this shipped plan and the visual-product-polish hub.

`okay_to_delete` remains `false` only because the visual-product-polish hub is
retaining all six stream plans until the real-WebGPU browser smoke blocker is
cleared or waived.
