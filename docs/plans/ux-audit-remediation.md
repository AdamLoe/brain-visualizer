---
status:        active
owner:         adamg
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
and representative dev-panel controls, and mobile diagnostics is explicitly
disabled/unsupported in docs/tests for now.

## Scope

Audit source note:

- There is no separate UX audit source document. This plan is the authoritative
  record for the `BV-UX-AUDIT-*` IDs, so each implementation/review pass should
  preserve the ID mapping in commit messages, test names, or plan notes rather
  than looking for another audit artifact.

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

- Explicitly keep mobile dev diagnostics unsupported for now. The mobile profile
  should not expose a usable dev-panel/diagnostics workflow.
- Make the disabled/unsupported mobile policy explicit in docs and tests, and
  ensure screenshot review instructions use desktop for dev diagnostics.
- Do not add a mobile-safe diagnostics path under this plan. A later plan can
  revisit mobile diagnostics if product ownership wants that workflow.

Parallelism and serialization:

- Read-only investigation can run in parallel, but mutating implementation on
  the shared tree should run serially because all streams touch or may touch
  `web/src/main.ts`, shared Playwright fixtures, or shared startup/recovery
  behavior.
- Suggested mutation waves:
  1. Add concrete boot visibility/screenshot evidence gates.
  2. Implement startup failure recovery and structural settings rollback or
     explicit failed/pending recovery, keeping persisted-state changes together.
  3. Implement keyboard/focus/dev-panel accessibility behavior.
  4. Disable/document/test mobile diagnostics as unsupported.
  5. Touch Rust/WGSL/GPU backend code only if the boot evidence proves the
     blank healthy boot is a render/backend problem.
- Final e2e screenshot and accessibility gates run once after all UI,
  persistence, and docs changes land.

## Exit gate

Required assertions:

- Healthy boot desktop screenshot after overlay clear shows unmistakable
  brain/visual state in a real WebGPU browser, is not effectively black by a
  documented pixel check, and has no console errors.
- Healthy boot mobile-ish screenshot after overlay clear shows unmistakable
  brain/visual state in a real WebGPU browser, is not effectively black by a
  documented pixel check, and has no console errors.
- Screenshot/pixel gates name the desktop and mobile-ish viewport sizes, the
  sampled region or threshold rule, and the artifact paths used for review.
- If a real WebGPU browser/adapter is unavailable, stop and report that blocker
  with evidence. Do not treat fallback/non-WebGPU output as equivalent for the
  visual proof.
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
- Mobile diagnostics is documented/tested as unsupported, with screenshot
  reviews using desktop diagnostics.

Commands:

- From `app/web/`: `npm run typecheck`
- From `app/web/`: `npm test`
- From `app/web/`: `npm run build`
- From `app/web/`: `npm run test:e2e`
- From `app/`: `cargo test` if any Rust/WGSL/GPU backend code changes

## Discipline rules

- Do not persist structural settings before the backend/network state has proven
  the change, unless the UI clearly marks the state as pending/failed and
  recoverable.
- Do not make the boot overlay a diagnostics dump; keep detailed diagnostics in
  console/test hooks unless they are needed for user recovery.
- Do not make mobile dev diagnostics implicitly available. Either support it
  deliberately in a future plan or document/test the unsupported policy here.
- Keep new tests focused in per-feature files where practical.

## Open decisions

- Failed structural apply UX: resolved as rollback-to-last-applied. Structural
  settings and rebuild-backed morphology stay out of app-owned localStorage
  until backend apply succeeds; failed preparation/application rolls controls and
  storage back to the last applied state.
- Remaining verification blocker: this host cannot provide the required real
  WebGPU browser/adapter proof. The strict e2e gate now fails with an explicit
  adapter-unavailable blocker instead of passing fallback/non-adapter output.

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
intentionally disabled/unsupported for now.

Implementation notes:
Start with tests that reproduce black successful boot, forced rebuild failure,
startup failure recovery, and keyboard/focus gaps. Then fix behavior in
serialized UI/recovery passes, and finish with docs migration.

Cheapest sufficient checks:
`npm run typecheck`, `npm test`, targeted then full `npm run test:e2e`,
`npm run build`, screenshot/pixel assertions for desktop and mobile-ish boot in
a real WebGPU browser, and `cargo test` only if Rust/WGSL changes.

Stop and report if:
The black successful boot is caused by GPU/device behavior that cannot be
reproduced under an available real WebGPU browser, if no real WebGPU
browser/adapter is available for the required screenshot proof, or if rollback
and pending-state recovery both require a larger settings architecture rewrite
than this plan assumes.

Open decisions:
None for implementation. Remaining work is strict real-WebGPU visual proof on a
machine/browser where `navigator.gpu.requestAdapter()` returns a real adapter.

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

- 2026-06-20: Implementation commits `7e5b574` and `4724158` migrated durable
  behavior into `architecture/web-frontend.md`, `architecture/dev-panel.md`,
  `decisions/dev-tooling.md`, `decisions/interaction.md`, and
  `agent-context/testing-how-to.md`.
- 2026-06-20: Not shipped yet. Strict visual proof remains blocked in this
  environment: Chromium exposes `navigator.gpu`, but
  `navigator.gpu.requestAdapter()` returns no adapter. Run
  `BV_WEBGPU_BROWSER_MODE=hardware USE_WEBSERVER=1 npm run test:e2e -- e2e/ux_audit_remediation.spec.ts --grep "real WebGPU boot"`
  from `app/web/` on a real-WebGPU machine, then fill in desktop/mobile-ish
  screenshot artifact paths and set `status: shipped` only if that gate passes.

## See also

- `docs/plans/index.md`
- `docs/plans/future_roadmap.md`
- `docs/plans/whole-app-review-fixes.md`
- `docs/agent-context/testing-how-to.md`
- `docs/agent-context/maintaining-docs.md`
