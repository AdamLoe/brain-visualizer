# Decisions — Interaction

## Pretty toy, not a tool

- **Decision.** The interaction model is intentionally shallow: the sim is a
  thing you watch and lightly perturb, not an instrument you operate.
- **Why.** Fitting the "silly, pretty toy" framing keeps the experience inviting
  without competing with benchmark or scientific visualization tools. The visual
  and auditory richness carry the page on their own.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).

## Input scheme: hover=stimulate, drag=orbit, scroll=zoom; click does nothing

- **Decision.** Hover (no button held) over the canvas injects current into
  neurons within a world-space radius of the ray hit point. Left-drag orbits.
  Scroll/pinch zooms. Click has no MVP action. Disabled on mobile (no
  stimulation, one-finger orbit, two-finger pinch zoom).
- **Why.** Click-to-inspect (neuron selection, incoming/outgoing connections)
  requires GPU id-buffer picking or CPU ray tests plus near-LOD
  materialization — non-trivial complexity for a post-MVP feature. The experience
  does not depend on it.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/render/camera.ts → Camera.onPointerMove` (returns `true` when
  orbit, `false` when hover); `web/src/main.ts → computeStimulation`,
  `raySphereIntersect`.
- **Revisit when.** Neuron inspection is revived post-MVP.

## Natural propagation IS the intro; no scripted wake-up

- **Decision.** The sim starts immediately from a silent state. Input-region
  neurons receive constant ambient `I_ext` drive; activity propagates naturally
  posterior→anterior. There is no scripted seed spike, no timed animation
  sequence, and no special intro code anywhere in the frontend.
- **Why.** The natural ramp-up is free, repeatable, and more honest than
  scripted drama. A scripted intro would diverge from actual sim behavior and
  require maintenance.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/simulation.md`](../architecture/simulation.md).
- **Code anchors.** `web/src/main.ts → boot` (no deferred rAF, no seed-spike call);
  sim-side drive in `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`.
- **Alternatives considered.** Scripted seed spike at t=0, timed fade-in — both
  rejected: brittle, disconnected from real dynamics.

## Sonification opt-in, muted by default

- **Decision.** The `SonificationEngine` is constructed at boot but the
  `AudioContext` is not created until the user clicks the sound toggle.
  Sound is disabled entirely on mobile.
- **Why.** Autoplay audio is blocked by browsers and is annoying when unexpected.
  Muted-by-default keeps the page polite; the toggle is visible when the visitor
  wants it.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/audio/sonification.ts → SonificationEngine.enable`;
  `web/src/main.ts` sound-toggle click handler; `web/index.html` `#sound-toggle`.
- **Tradeoffs.** `ScriptProcessorNode` (noise layer) is deprecated; an
  `AudioWorklet` is the modern replacement but is deferred as an enhancement.

## Discrete speed presets (not a continuous slider)

- **Decision.** Simulation speed is a small set of named rates (¼×, ½×, 1×, 2×
  expressed as `targetTicksPerSec`) configured via the dev panel, not a
  continuous scrub slider. The rAF loop uses a time-based accumulator to convert
  the target rate to an integer tick count per frame.
- **Why.** A few discrete presets cover the useful range (slow enough to watch
  individual wavefronts; fast enough to compress time) with simpler UX than a
  slider. The accumulator avoids the frame-count coupling of the older
  `ticksThisFrame` modulo approach.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/main.ts` (`targetTicksPerSec`, `tickAccumulator`);
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

## Backend switch tears down and restarts with the same seed

- **Decision.** Switching between GPU and CPU backends cancels the rAF,
  destroys the current backend, and reinitialises the new one from the same
  network seed. No mid-run state transfer.
- **Why.** Transferring state between GPU buffers and WASM SharedArrayBuffer is
  complex for zero user benefit. The same seed guarantees the visitor sees the
  same network topology on both backends, which is the meaningful comparison.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- **Code anchors.** `web/src/main.ts → restartWithBackend`.

## Scaling is an explicit, persisted user action — not automatic

- **Decision.** N (and the tier/backend/speed/excitability that go with it) change
  only when the user acts — picking a tier or editing N/K in the dev panel — and
  the chosen config is persisted to localStorage so a reload restores the
  last-used network. There is no automatic, frame-time-driven N change.
- **Why.** A runtime auto-scaler was tried and pulled (see
  [`scaling.md`](scaling.md)); fixing N until the user decides keeps the
  experience predictable and the morphology target stable. Persisting the choice
  fixes a bug where the user's scaling reset on every reload.
- **Applies to.** [`../architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`../architecture/scaling.md`](../architecture/scaling.md).
- **Code anchors.** `web/src/core/types.ts → loadConfig`, `saveConfig`
  (key `bv2_config_v1`); `web/src/ui/controls.ts → setTier`, `setBackend`, `setSpeed`.

## See also

- [`../architecture/web-frontend.md`](../architecture/web-frontend.md)
- [`../architecture/simulation.md`](../architecture/simulation.md) — `I_ext`
  ambient drive, excitability parameter contract
- [`../architecture/scaling.md`](../architecture/scaling.md) — tier presets,
  fixed-N startup, the dormant scaler decision fn
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md) — settings
  panel, speed/brain-state knobs
- [`../architecture/profiling.md`](../architecture/profiling.md) — profiler
  snapshot that feeds sonification
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
