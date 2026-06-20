# Decisions — Interaction

## Pretty toy, not a tool

- **Decision.** The interaction model is intentionally shallow: the sim is a
  thing you watch and lightly perturb, not an instrument you operate.
- **Why.** Fitting the "silly, pretty toy" framing keeps the experience inviting
  without competing with benchmark or scientific visualization tools. The visual
  richness carries the page on its own.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).

## Input scheme: hover=stimulate, left-drag=orbit, right-drag=pan, scroll=zoom; click does nothing

- **Decision.** Hover (no button held) over the canvas injects current into
  neurons within a world-space radius of the ray hit point. Left-drag orbits.
  Right-drag pans in screen space, with Shift-left-drag as the desktop
  fallback. Scroll/pinch zooms. Click has no MVP action. Hover stimulation is
  suppressed while any button drag is active. Disabled on mobile (no
  stimulation, one-finger orbit, two-finger pinch zoom).
- **Why.** Click-to-inspect (neuron selection, incoming/outgoing connections)
  requires GPU id-buffer picking or CPU ray tests plus materializing enough
  geometry for selection — non-trivial complexity for a post-MVP feature. The
  experience does not depend on it.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/render/camera.ts → Camera.onPointerMove`, `Camera.pan`;
  `web/src/main.ts → boot, computeStimulation`,
  `raySphereIntersect`.
- **Revisit when.** Neuron inspection is revived post-MVP.

## Natural propagation IS the intro; no scripted wake-up

- **Decision.** The sim starts immediately from a silent state. Input-region
  neurons receive constant ambient `I_ext` drive; activity propagates naturally
  posterior→anterior. There is no scripted seed spike, no timed animation
  sequence, and no special simulation intro code anywhere in the frontend. The
  startup overlay is status UI only; it does not drive or fake neural activity.
- **Why.** The natural ramp-up is free, repeatable, and more honest than
  scripted drama. A scripted intro would diverge from actual sim behavior and
  require maintenance.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `web/src/main.ts → boot` (startup overlay + early rAF, no seed-spike call);
  sim-side drive in `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`.
- **Alternatives considered.** Scripted seed spike at t=0, timed fade-in — both
  rejected: brittle, disconnected from real dynamics.

## Startup feedback is DOM-only until the backend owns WebGPU

- **Decision.** The page shows an immediate loading/progress overlay from
  `index.html`, starts a lightweight rAF frame counter before wasm/backend work,
  and keeps visible failure state if WebGPU startup fails. Backend startup is
  driven through measured `WasmGpuBackend.create_staged` / `startup_*` stages,
  with a browser frame yield between stages and structured boot timings recorded
  in `window.__bvBootTimings`. The overlay itself remains product-facing status
  UI, not a diagnostics surface. Failure state exposes reset-saved-settings,
  reload-defaults, and retry actions. The frontend fallback renderer does not
  acquire WebGPU, WebGL2, or 2D canvas contexts during GPU startup.
- **Why.** Users should never see a blank or hung page while the heavy backend
  initializes, but the wasm backend must remain the single WebGPU surface owner.
  DOM/CSS feedback plus staged backend calls gives paintable, measured progress
  without creating duplicate devices or locking the canvas into the wrong
  context. The staged backend instance is not handed to the rAF loop until all
  resource stages finish.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/index.html → #startup-overlay`;
  `web/src/main.ts → updateStartupOverlay, startGpuBackend, wireStartupRecoveryActions`;
  `web/src/boot-failure.ts → resetAppOwnedStorage`;
  `web/src/boot-timings.ts → recordBootTiming, logBootSummary`;
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend::create_staged`;
  `web/src/render/renderer.ts → Renderer`.

## Keyboard focus is first-class for controls and diagnostics

- **Decision.** Public controls and the desktop diagnostics drawer must be
  operable and understandable by keyboard: named buttons, deterministic
  open/close focus return, tablist keyboard semantics, selected-state exposure,
  and focus-readable help matching hover tooltips.
- **Why.** The app is visually led, but recovery and diagnostics cannot depend
  on pointer hover or hidden focus. The desktop-only panel is dense enough that
  roving tabs and focus help are cheaper and more reliable than a separate
  explanatory UI.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/dev-panel.md`](../architecture/dev-panel.md).
- **Code anchors.** `web/index.html → #settings-toggle, #pause-toggle`;
  `web/src/ui/dev-panel.ts → _setOpen, _onTabKeydown, _attachTip, _buildTooltip`.

## Speed is target ticks/sec, not frame-count presets

- **Decision.** Simulation speed is a persisted numeric ticks/sec target
  configured via the dev panel. The rAF loop uses a time-based accumulator to
  convert the target rate to an integer tick count per frame.
- **Why.** A target ticks/sec control is independent of browser frame rate and
  avoids the frame-count coupling of the older `ticksThisFrame` modulo approach.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/main.ts` (`targetTicksPerSec`, `tickAccumulator`);
  `web/src/ui/dev-panel.ts → _buildNetworkTab`;
  `web/src/ui/controls.ts → ticksThisFrame` (legacy, kept for compat).

## Named brain-state presets are labels on the excitability axis

- **Decision.** Deep sleep / Relaxed / Focused / Hyperstimulated / Seizure are
  five fixed values on `[0, 1]` for the existing excitability parameter. They
  are not new sim parameters, not new UI controls, and not new backend state.
  Selecting one calls `setExcitabilityTarget()`; the rAF loop's lerp smoothly
  reaches the new value.
- **Why.** Named states give visitors a frame of reference without neuroscience
  background. The implementation cost is five constants and a lerp that already
  existed.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `web/src/ui/controls.ts → BRAIN_STATES`, `tickExcitability`,
  `setExcitabilityTarget`.

## Scaling is explicit or mobile-capped — not automatic

- **Decision.** N (and the tier/backend/speed/excitability that go with it)
  changes only when the user acts — picking a tier or editing N/K in the dev
  panel — or when mobile boot clamps a saved desktop-scale config down to no
  heavier than `DEFAULT_CONFIG`. The chosen bounded config is persisted to
  localStorage so a reload restores the last-used network. There is no automatic,
  frame-time-driven N change.
- **Why.** A runtime auto-scaler was tried and pulled (see
  [`scaling.md`](scaling.md)); fixing N until the user decides keeps the
  experience predictable and the morphology target stable. Persisting the choice
  fixes a bug where the user's scaling reset on every reload.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/core/types.ts → CONFIG_LS_KEY, loadConfig, saveConfig`;
  `web/src/core/mobile-config.ts → applyMobileConfig`;
  `web/src/ui/controls.ts → setTier, setSpeed`.

## See also

- [`../architecture/web-frontend.md`](../architecture/web-frontend.md)
- [`../architecture/simulation.md`](../architecture/simulation.md) — `I_ext`
  ambient drive, excitability parameter contract
- [`../architecture/scaling.md`](../architecture/scaling.md) — tier presets,
  fixed-N startup, the dormant scaler decision fn
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md) — settings
  panel, speed/brain-state knobs
- [`../architecture/profiling.md`](../architecture/profiling.md) — profiler
  snapshot that feeds HUD and monitor updates
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
