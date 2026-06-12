---
status:        active
owner:         Codex orchestrator
last_updated:  2026-06-12
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/build-and-deploy.md
  - architecture/dev-panel.md
  - architecture/web-frontend.md
  - architecture/gpu-backend.md
  - architecture/gpu-rendering.md
  - architecture/data-model.md
  - architecture/simulation.md
  - architecture/manifold.md
  - decisions/dev-tooling.md
  - decisions/backends.md
  - decisions/rendering.md
  - decisions/data-layout.md
  - decisions/dynamics.md
  - decisions/manifold.md
---

# App overhaul orchestration hub

## Mission

Coordinate the app overhaul requested from the pasted LLM critique. The work
should turn the critique into implementation-ready plans, review those plans,
ship them in staged agent passes, and migrate durable facts back into
architecture and decisions docs before plans are marked shipped.

## Source critique

The pasted critique groups the highest-value fixes into:

- Real-hardware and field observability: close the gap between local llvmpipe
  tests and deployed real-browser WebGPU behavior.
- Configuration and cross-language contracts: replace or harden hand-mirrored
  float/index layouts.
- Maintenance debt: decide the CPU/WebGL2 backend and parked rendering paths
  instead of keeping them in the default build indefinitely.
- Scaling and responsiveness: remove the morphology segment binding ceiling
  and avoid main-thread rebuild freezes.
- Correctness and outcome polish: harden overflow/determinism/tick-wrap gates,
  clarify estimated HUD metrics, and prototype visually coherent regions.

## Lifecycle

Tracked plan lifecycle, deep effort. The request is cross-cutting,
user-facing, correctness-sensitive, and explicitly asks for subagents and many
stages.

## Phase Tracker

| Phase | Status | Last observed fact | Next action |
|---|---|---|---|
| Intake/bootstrap | Done | Manifest, docs router, overview, global orchestration rules, plan lifecycle, and pasted critique read. | Dispatch planning agents. |
| Planning | Done | Eight child draft plan docs persisted under `docs/plans/orchestrator/` after splitting Stream C. | Activate first implementation wave. |
| Plan review | Done | High-level review recommended staging A, splitting C, serializing B1/B2, and clarifying D1/D2. | Apply plan review edits and activate first wave. |
| Implementation | In progress | First wave landed: Stream A A0/A1, B1, and D2 wave 1. | Commit wave 1 and continue later waves separately. |
| Work review | Done for wave 1 | Review-work pass found one smoke artifact gap and fixed it; no remaining blocking findings. | Commit wave 1. |
| Closeout | In progress | Consolidated gates passed except real-adapter smoke remains environment-dependent. | Commit wave 1 and report next waves. |

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| A | Real hardware, startup beacons, field telemetry | Partially implemented | A0 smoke and A1 disabled telemetry contract landed; web typecheck, unit tests, and focused smoke passed; local artifact skipped because no adapter was available. | Review A changes and defer A2 production enablement. | Telemetry sink/retention and real-adapter lane still needed. |
| B1 | Settings/metrics/schema contracts | Implemented | Contract tests, tombstone hardening, uniform size checks, estimated labels, docs migration landed; cargo test, npm test, and typecheck passed. | Include in work review/final commit. | None observed. |
| B2 | Simulation correctness gates | Drafted | `stream-b-simulation-correctness-gates-2026-06-12.md` persisted. | Plan review. | Strict no-adapter CI behavior must be verified. |
| C1 | CPU backend retirement/feature gate | Drafted | Split from original Stream C after plan review. | Await user decision before destructive deletion; feature-gate/archive path can proceed after activation. | Delete-vs-feature-gate decision. |
| C2 | Legacy render/dead-code amnesty | Drafted | Split from original Stream C after plan review. | Wait until D1 and B1 are no longer touching render/settings surfaces. | Conflicts with D1/B1. |
| D1 | Morphology segment scaling | Drafted | `stream-d-morphology-segment-scaling-2026-06-12.md` persisted. | Plan review. | Serializes with D2 prepared payload shape. |
| D2 | Rebuild responsiveness | Partially implemented | Wave 1 coordinator groundwork landed; web typecheck/tests passed; plan remains active for worker/payload phases. | Include in first-wave commit. | Prepared payload should stay GPU-agnostic and follow D1 upload boundary later. |
| E | Visual outcome and region coherence | Drafted | `stream-e-visual-outcome-polish-hud-truthfulness-2026-06-12.md` persisted. | Plan review. | Prototype can ship opt-in; default promotion needs review. |

## Activation Plan

| Wave | Status | Plans | Reason |
|---|---|---|---|
| 1 | Ready | A0 real-hardware smoke from Stream A; B1 settings/metrics contracts; D2 wave 1 rebuild coordinator groundwork | Low-regret foundations that do not require production telemetry, CPU deletion, or region-default decisions. |
| 2 | Pending | B2 simulation correctness gates | Valuable but overlaps `sim/gpu/mod.rs`; run after B1 or with strict file-region ownership. |
| 3 | Pending | D1 morphology segment scaling | High-risk GPU resource work; run after D2 coordinator groundwork and not in parallel with legacy render amnesty. |
| 4 | Pending | D2 worker upload integration | Depends on a settled GPU upload/chunking boundary from D1 unless payload remains flat and GPU-agnostic. |
| 5 | Pending | Stream E spatial-region prototype | Can run after correctness/contracts; promotion to default requires review. |
| Decision-gated | Pending | A2 production telemetry enablement, C1 CPU deletion, C2 legacy render amnesty | Requires owner decision or dependency clearance. |

## Decisions Log

- Use tracked plan lifecycle rather than quick-fix or briefed implementation.
- Treat existing deleted plan files in the working tree as user-owned cleanup;
  do not restore them as part of this orchestration.
- Default to privacy-respecting, opt-out telemetry planning unless existing
  docs or code show a stricter analytics policy.
- Do not start source edits until plan docs are reviewed, except for hub/plan
  coordination files.
- After plan review, first implementation wave should avoid production
  telemetry enablement, CPU deletion, and default region promotion.
- Stream B1 and B2 must not edit `sim/gpu/mod.rs` concurrently.
- D2 worker-prepared payloads should remain GPU-agnostic; chunking is a
  main-thread WebGPU upload policy unless implementation proves otherwise.

## Open Questions

- Whether field telemetry may use an external service or must be a self-hosted
  endpoint/static-log flow.
- Whether CPU backend should be deleted outright or moved behind an explicit
  dev-only feature first.
- Whether spatial region coherence should ship by default or as a dev-panel
  visual/dynamics mode after prototype review.

## Agent Log

- 2026-06-12: Stream A planning dispatched to agent `019ebd82-479d-7b50-b358-2ee9af333804` (`Feynman`).
- 2026-06-12: Stream B planning dispatched to agent `019ebd82-77a4-7de0-9aba-ac4d2c234e28` (`Harvey`).
- 2026-06-12: Stream C planning dispatched to agent `019ebd82-ae1c-7a93-95e9-3812d700a355` (`Kant`).
- 2026-06-12: Stream D planning dispatched to agent `019ebd82-dd2e-71b2-a760-36dc0ba37e41` (`Darwin`).
- 2026-06-12: Stream E planning dispatched to agent `019ebd83-0891-78e3-b6a7-622d6867ecc2` (`Aristotle`).
- 2026-06-12: Plan review dispatched to agent `019ebd8a-f5a8-7262-8f6e-73df92fda2ea` (`Lorentz`); review edits applied.
- 2026-06-12: B1 implementation dispatched to agent `019ebd90-2f2d-7100-baf5-e08207444a05` (`Confucius`); cargo test, npm test, and typecheck passed.
- 2026-06-12: A0/A1 implementation dispatched to agent `019ebd90-8abc-77e2-a6c4-e34eec6a0fa9` (`Goodall`); typecheck, npm test, and focused smoke passed with no-adapter skip.
- 2026-06-12: D2 wave 1 implementation dispatched to agent `019ebd9f-b6ba-7d81-9639-2fc5d3810d26` (`Rawls`); typecheck and npm test passed.
- 2026-06-12: First-wave work review dispatched to agent `019ebda6-92f7-7811-879f-0c07e88f3313` (`McClintock`); smoke artifact fix applied.

## Gate Evidence

- `cd app && cargo test -p brain-visualizer` passed.
- `cd app && cargo run -p brain-visualizer --example render_check` passed.
- `cd app/web && npm run typecheck` passed.
- `cd app/web && npm test` passed.
- `cd app/web && npm run build` passed outside the sandbox after the sandbox
  blocked `wasm-pack` temp install files.
- `cd app/web && USE_WEBSERVER=1 npm run test:e2e:smoke` passed outside the
  sandbox after the sandbox blocked Chromium startup.
- Real-adapter smoke remains outstanding: this environment can produce a
  no-adapter/llvmpipe-style artifact, not real hardware proof.

## Migration Notes

At ship time, migrate durable current-state facts and decisions into the owning
architecture and decisions docs listed in frontmatter, then mark this hub
`shipped` only when all child plans are shipped or intentionally abandoned.
