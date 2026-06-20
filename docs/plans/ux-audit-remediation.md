---
status:        draft
owner:         unassigned
last_updated:  2026-06-20
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/web-frontend.md
  - architecture/dev-panel.md
  - architecture/gpu-backend.md
  - architecture/gpu-rendering.md
  - decisions/interaction.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
---

# UX audit remediation

## Mission

Fix the approved UX audit findings that can make the app look blank, leave users
trapped after bad persisted state or startup failure, or make the controls/dev
diagnostics inaccessible. Done means a healthy boot visibly renders the brain on
desktop and mobile-ish viewports, structural setting failures are recoverable and
do not silently persist unproven state, startup failures expose readable
reset/default recovery, keyboard and assistive workflows cover public controls
and representative dev-panel controls, and the mobile diagnostics story is
either supported or explicitly bounded in docs/tests.

## Scope

In scope:

- `BV-UX-AUDIT-001` Blank successful boot.
- `BV-UX-AUDIT-002` Structural settings persist before proven applied.
- `BV-UX-AUDIT-003` Startup failure UI not recoverable enough.
- `BV-UX-AUDIT-004` Public controls/dev panel lack robust keyboard/assistive UX.
- `BV-UX-AUDIT-005` Mobile diagnostics path incoherent.
- `BV-UX-AUDIT-006` Narrow-width canvas clamp risk, bundled into
  accessibility/diagnostics.
- `BV-UX-AUDIT-007` Missing e2e persistence/recovery journey, bundled into
  recovery.

Out of scope:

- High-N morphology degradation owned by `docs/plans/future_roadmap.md`.
- New public settings IA, new visual design language, or broader render
  retuning beyond making successful boot visibly nonblank.
- Reopening shipped `docs/plans/whole-app-review-fixes.md`.

## Approach

Stream 1: Boot/render visibility (`BV-UX-AUDIT-001`, `BV-UX-AUDIT-006`)

Owned source/docs:

- `app/web/src/main.ts`
- `app/crates/brain-visualizer/src/sim/gpu/*`
- `docs/architecture/web-frontend.md`
- `docs/architecture/gpu-backend.md`
- `docs/architecture/gpu-rendering.md`
- `docs/decisions/rendering.md`

Work:

- Add an e2e success gate that waits for the startup overlay to clear and proves
  the canvas is not effectively black on desktop and mobile-ish viewports.
- Capture screenshots as review artifacts for both viewports.
- Fail the gate on browser console errors during boot/render.
- Fix the rendering or boot sequencing path if the app reports healthy startup
  but produces a black frame.
- Include narrow-width canvas sizing/clamp assertions where they affect
  successful visual output.

Stream 2: Transactional settings/rebuild recovery (`BV-UX-AUDIT-002`,
`BV-UX-AUDIT-007`)

Owned source/docs:

- `app/web/src/main.ts`
- `app/web/src/core/settings.ts`
- `app/web/src/core/types.ts`
- `app/web/src/core/morph-config.ts`
- `docs/architecture/dev-panel.md`
- `docs/decisions/dev-tooling.md`

Work:

- Make structural setting persistence follow proven apply success, or expose an
  explicit recoverable pending/failed state that cannot be mistaken for applied
  state.
- On forced prepared-network or morphology rebuild failure, keep the running
  backend/network intact.
- Roll back panel control values and localStorage to the last applied state,
  unless the chosen UX is an explicit failed/pending state with reset/retry
  controls.
- Add an e2e persistence/recovery journey covering failed rebuild, reload, and
  reset/default recovery.

Stream 3: Startup failure recovery (`BV-UX-AUDIT-003`)

Owned source/docs:

- `app/web/index.html`
- `app/web/src/main.ts`
- `app/web/src/boot-failure.ts`
- `docs/architecture/web-frontend.md`
- `docs/decisions/dev-tooling.md`
- `docs/decisions/interaction.md`

Work:

- Keep unsupported-WebGPU and bad-persisted-config guidance readable at narrow
  widths.
- Preserve the existing `aria-live` behavior.
- Add visible recovery actions: reset app-owned storage, reload defaults, and
  retry/reload as appropriate.
- Ensure the recovery path handles invalid persisted config without requiring
  devtools.

Stream 4: Keyboard and assistive UX (`BV-UX-AUDIT-004`, `BV-UX-AUDIT-006`)

Owned source/docs:

- `app/web/index.html`
- `app/web/src/main.ts`
- `app/web/src/ui/dev-panel.ts`
- `app/web/src/ui/controls.ts`
- `docs/architecture/web-frontend.md`
- `docs/architecture/dev-panel.md`
- `docs/decisions/interaction.md`
- `docs/decisions/dev-tooling.md`

Work:

- Add accessible names/labels for public controls and representative dev-panel
  controls.
- Make drawer open/close focus behavior deterministic, including focus return.
- Ensure dev-panel tabs have keyboard semantics and selected state exposed to
  assistive tech.
- Replace or supplement hover-only tooltip access with keyboard/focus-readable
  help.
- Assert no overlapping/truncated controls at narrow widths relevant to the
  public UI and diagnostics surfaces.

Stream 5: Mobile diagnostics policy (`BV-UX-AUDIT-005`)

Owned source/docs:

- `app/web/src/main.ts`
- `app/web/src/ui/dev-panel.ts`
- `docs/architecture/web-frontend.md`
- `docs/architecture/dev-panel.md`
- `docs/decisions/dev-tooling.md`

Work:

- Preferred default: keep the dev panel unsupported on mobile, because current
  architecture documents "no dev panel" in the mobile profile.
- Make that policy explicit in docs and tests, and ensure screenshot review
  instructions use desktop for dev diagnostics.
- If implementation discovers the mobile panel is already close to safe, it may
  instead support a mobile-safe diagnostics path, but that is not required for
  this plan unless the owner changes this decision.

Parallelism and serialization:

- Streams 1 and 4 can be investigated in parallel, but edits serialize where
  they touch `web/src/main.ts` or shared Playwright fixtures.
- Streams 2 and 3 must serialize because both own persisted state,
  reset/recovery behavior, and startup failure paths.
- Stream 5 should run after Streams 3 and 4 choose the final
  recovery/accessibility behavior.
- Final e2e screenshot and accessibility gates run once after all UI,
  persistence, and docs changes land.

## Exit gate

Required assertions:

- Healthy boot desktop screenshot after overlay clear shows unmistakable
  brain/visual state, is not effectively black by pixel check, and has no
  console errors.
- Healthy boot mobile-ish screenshot after overlay clear shows unmistakable
  brain/visual state, is not effectively black by pixel check, and has no
  console errors.
- Forced structural rebuild failure preserves the running network/backend and
  either rolls back panel values plus app-owned localStorage or shows an
  explicit recoverable failed/pending state with reset/retry.
- Reload after failed structural apply does not boot into a silently trusted bad
  persisted state.
- Unsupported WebGPU and bad persisted config states keep guidance readable,
  preserve `aria-live`, and expose reset/default recovery.
- Playwright/accessibility assertions cover public controls, representative
  dev-panel controls, drawer focus behavior, tabs, and focus-accessible
  help/tooltips.
- Mobile diagnostics is either supported by a mobile-safe panel path or
  explicitly documented/tested as unsupported, with screenshot reviews using
  desktop diagnostics.

Commands:

- From `app/web/`: `npm run typecheck`
- From `app/web/`: `npm test`
- From `app/web/`: `npm run test:e2e`
- From `app/`: `cargo test` if any Rust/WGSL/GPU backend code changes

## Discipline rules

- Do not persist structural settings before the backend/network state has proven
  the change, unless the UI clearly marks the state as pending/failed and
  recoverable.
- Do not make the boot overlay a diagnostics dump; keep detailed diagnostics in
  console/test hooks unless they are needed for user recovery.
- Do not make mobile dev diagnostics implicitly available. Either support it
  deliberately or document/test the unsupported policy.
- Keep new tests focused in per-feature files where practical.

## Open decisions

- Mobile diagnostics default: this plan recommends documenting/testing mobile
  dev-panel unsupported rather than building a mobile drawer. Change only if
  product ownership wants mobile diagnostics as a supported workflow.
- Failed structural apply UX: implementers may choose rollback-to-last-applied
  or explicit pending/failed state. Rollback is the recommended default unless
  the existing dev-panel structure makes pending state clearer and cheaper.

## Implementer brief

Goal:
Fix the approved UX audit findings with one coordinated remediation pass
covering boot visibility, recovery, accessibility, and diagnostics policy.

Non-goals:
Do not retune high-N morphology, redesign the public UI, add a public settings
page, or reopen shipped plan work.

Authoritative docs:
`docs/architecture/web-frontend.md`, `docs/architecture/dev-panel.md`,
`docs/architecture/gpu-backend.md`, `docs/architecture/gpu-rendering.md`,
`docs/decisions/interaction.md`, `docs/decisions/rendering.md`,
`docs/decisions/dev-tooling.md`.

Likely source areas:
`app/web/index.html`, `app/web/src/main.ts`, `app/web/src/boot-failure.ts`,
`app/web/src/core/settings.ts`, `app/web/src/core/types.ts`,
`app/web/src/core/morph-config.ts`, `app/web/src/ui/dev-panel.ts`,
`app/web/src/ui/controls.ts`, Playwright tests, and possibly
`app/crates/brain-visualizer/src/sim/gpu/*`.

Expected behavior:
Successful boot visibly renders the brain after overlay clear; failed startup
and failed structural settings are recoverable without devtools; controls and
dev diagnostics are keyboard/assistive usable; mobile diagnostics is
intentionally supported or intentionally unsupported.

Implementation notes:
Start with tests that reproduce black successful boot, forced rebuild failure,
startup failure recovery, and keyboard/focus gaps. Then fix behavior in
serialized UI/recovery passes, and finish with docs migration.

Cheapest sufficient checks:
`npm run typecheck`, `npm test`, targeted then full `npm run test:e2e`,
screenshot/pixel assertions for desktop and mobile-ish boot, and `cargo test`
only if Rust/WGSL changes.

Stop and report if:
The black successful boot is caused by GPU/device behavior that cannot be
reproduced under the available Playwright environment, or if rollback and
pending-state recovery both require a larger settings architecture rewrite than
this plan assumes.

Open decisions:
Mobile diagnostics support vs documented unsupported policy; rollback vs
explicit pending/failed state for structural apply failure.

## Migration notes (filled in at ship time)

Before setting `status: shipped`, route durable facts and decisions into:

- `architecture/web-frontend.md` - startup overlay/recovery behavior, successful
  boot e2e hooks, mobile diagnostics policy, public control accessibility
  behavior.
- `architecture/dev-panel.md` - dev-panel keyboard/focus/tab/help behavior,
  structural apply recovery, persistence/reset contract.
- `architecture/gpu-backend.md` and `architecture/gpu-rendering.md` - any
  render/backend changes needed to make successful boot visibly nonblank.
- `decisions/interaction.md` - any user-facing recovery or accessibility
  interaction decisions.
- `decisions/rendering.md` - any render-path decision made to prevent black
  successful frames.
- `decisions/dev-tooling.md` - persistence/recovery and mobile diagnostics
  policy decisions.
- `_meta/ownership.json` - only if a new concept needs a canonical owner.

Migration record:

- TBD at ship time.

## See also

- `docs/plans/index.md`
- `docs/plans/future_roadmap.md`
- `docs/plans/whole-app-review-fixes.md`
- `docs/agent-context/testing-how-to.md`
- `docs/agent-context/maintaining-docs.md`
