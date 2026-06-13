---
status:        shipped
owner:         Codex orchestrator
last_updated:  2026-06-13
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/web-frontend.md
  - architecture/profiling.md
  - architecture/dev-panel.md
  - architecture/build-and-deploy.md
  - decisions/profiling.md
  - decisions/dev-tooling.md
---

# Stream A: Real-Hardware Verification And Field Observability

## Mission

Close the confidence gap between local llvmpipe correctness checks and real
browser / real GPU behavior. The shipped app should emit lightweight,
privacy-respecting signals for WebGPU startup, adapter/init failures, startup
stage timings, first-frame readiness, runtime frame health, crashes, and
dev-panel usage. The repo should also have a repeatable real-hardware smoke
path that proves the default experience is nonblank and meets a basic frame
health floor when a real WebGPU adapter exists.

## Scope

In scope:

- Client observability for startup, WebGPU init, first frame, coarse adapter
  class where safely available, startup timings, low-cadence frame histogram,
  crash buckets, and dev-panel usage counts.
- A telemetry transport that is disabled when no endpoint is configured, bounded
  in cadence, and opt-out.
- Privacy controls: standalone opt-out, privacy-signal respect, no cookies, no
  persistent identity, no raw stack traces, no slider values, and no
  localStorage dumps.
- Real-browser verification: adapter availability, startup ready, canvas
  nonblank, FPS/frame-time sample, JSON artifacts, and screenshot artifacts.

Out of scope:

- Settings-schema work, CPU retirement, morphology scaling, simulation tuning,
  visual-region changes, shader changes, or hot-loop GPU readback.

## Context Routes

- `docs/_meta/manifest.md`
- `docs/architecture/web-frontend.md`
- `docs/architecture/profiling.md`
- `docs/architecture/dev-panel.md`
- `docs/architecture/build-and-deploy.md`
- `docs/decisions/profiling.md`
- `docs/decisions/dev-tooling.md`
- `docs/agent-context/testing-how-to.md`
- `app/web/src/main.ts`
- `app/web/src/render/profiler.ts`
- `app/web/src/ui/hud.ts`
- `app/web/src/ui/dev-panel.ts`
- `app/web/e2e/brain_visualizer.spec.ts`
- `app/web/playwright.config.ts`
- `app/web/package.json`

## Approach

Stage A0: real-hardware smoke and artifact reporting.

- Add or refine a browser verification path that reports adapter availability,
  startup readiness, canvas nonblank/variance evidence, frame health, screenshot
  path, and explicit skip/fail reason.
- With `BV_REQUIRE_WEBGPU=1`, fail when no adapter exists. Without it, emit a
  clear environment-skip artifact instead of claiming success.
- This stage can become active before production telemetry decisions.

Stage A1: disabled-by-default telemetry contract and mocked tests.

- Define the event contract and privacy gate, but keep transport inert when no
  endpoint is configured.
- Test payload allowlists, opt-out, privacy signals, and mocked endpoint sends.
- Keep dev-panel usage telemetry out of the first implementation unless a later
  decision explicitly adds it.

Stage A2: production telemetry enablement.

- Enable only after the owner chooses a telemetry sink/provider, retention
  policy, and opt-in/opt-out/dogfood posture.

1. Define a typed observability contract before wiring calls. Event families:
   `session_start`, `webgpu_init`, `startup_timing`, `runtime_perf`, `crash`,
   and, in a later stage only, `dev_panel_usage`.
2. Add a standalone privacy gate. Disable telemetry with absent endpoint,
   `?telemetry=0`, local opt-out, global privacy control, or do-not-track.
3. Reuse existing sources: startup stage state from `main.ts` and one-second
   profiler snapshots from `Profiler`. Do not add synchronous GPU readback.
4. Add a Playwright/reporting path for real-hardware WebGPU. With
   `BV_REQUIRE_WEBGPU=1`, fail if no adapter exists; without it, produce an
   explicit environment-skip artifact.
5. Support local, deployed preview, and optional real-GPU runner lanes. Treat
   llvmpipe/SwiftShader evidence as correctness-only, never as a product
   performance claim.

## Exit Gate

- `cd app && cargo test -p brain-visualizer`
- `cd app && cargo run -p brain-visualizer --example render_check`
- `cd app/web && npm run typecheck`
- `cd app/web && npm test`
- `cd app/web && npm run build`
- `cd app/web && npm run test:e2e:server`
- For A0 specifically: real-hardware smoke script/report exists and records
  adapter status, startup timings, nonblank evidence, frame health, screenshot
  path, and skip/fail reason.
- Unit tests cover payload allowlist, opt-out, privacy-signal disablement,
  histogram bucketing, and endpoint-absent no-op behavior.
- Playwright mocked-endpoint tests prove enabled sends, opted-out silence, and
  no disallowed payload fields.
- A real-hardware smoke artifact records adapter status, startup timings,
  nonblank evidence, FPS/frame-time sample, screenshot path, and skip/fail
  reason.

## Handoff Notes

Implementation can land A0 and A1 without a production telemetry decision.
Owner decision on 2026-06-12: keep production telemetry disabled for now. A2
production enablement is intentionally deferred until a future telemetry sink,
retention period, and opt-in/opt-out/dogfood posture are selected.

Accepted-scope ship note, 2026-06-13: A0 real-hardware/browser smoke and A1
disabled telemetry contract are implemented and documented. Local strict smoke
was run with `BV_REQUIRE_WEBGPU=1 USE_WEBSERVER=1 npm run test:e2e:smoke`; the
test infrastructure reached Chromium and wrote
`app/web/test-results/real_hardware_smoke-real-h-d19c1--canvas-and-frame-artifacts-chromium/real-hardware-smoke.json`,
but this machine exposed `navigator.gpu` without a WebGPU adapter
(`hasAdapter: false`). That is an environment limitation for real-adapter proof,
not an open implementation task. Production telemetry enablement remains
explicitly out of scope.

## Migration Notes

At ship time, migrate event shapes and startup reporting into
`architecture/profiling.md` and `architecture/web-frontend.md`; dev-panel usage
and opt-out behavior into `architecture/dev-panel.md`; real-hardware smoke
lanes into `architecture/build-and-deploy.md` and
`agent-context/testing-how-to.md`; privacy rationale into
`decisions/profiling.md` and `decisions/dev-tooling.md`.
