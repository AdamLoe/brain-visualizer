---
status:        active
owner:         adamg
last_updated:  2026-06-08
---

# Web Frontend

The TypeScript app shell that drives everything visible in the browser. Its one
job is to own the rAF loop, broker all input events, and hold the single live
reference to `WasmGpuBackend` — routing every mutation through that loop to
avoid wasm-bindgen reentrancy panics.

## What it owns

- `web/src/main.ts` — boot sequence, rAF loop (`rafLoop`), all pending-flag
  plumbing (`pendingResize`, `pendingStim`, `pendingSettingsPush`,
  `pendingNetworkRebuild`), `CpuCoordinator`, `startGpuBackend`,
  `restartWithBackend`, `computeStimulation`, `raySphereIntersect`
- `web/src/render/camera.ts → Camera` — orbit/zoom/pan state machine; produces MVP matrix,
  billboard right/up vectors, and unprojection rays
- `web/src/ui/controls.ts` — `BRAIN_STATES`, `tickExcitability`, `setExcitabilityTarget`,
  `TIER_PRESETS`, `scalerDecide`, `ticksThisFrame`, `isMobile`, `Controls`
- `web/src/render/renderer.ts → Renderer` — WebGPU canvas context + device acquisition;
  fallback clear path used only when `WasmGpuBackend` is not yet ready
- `web/src/core/types.ts → AppConfig`, `DEFAULT_CONFIG`, `SpeedPreset`, `BackendKind`,
  `Tier`, `BrainState`, `TickStats`, plus `AppConfig` localStorage persistence
  (`loadConfig`, `saveConfig`)
- `web/src/audio/sonification.ts → SonificationEngine`, `deriveRegionFractions`
- `web/src/ui/hud.ts → CornerHud` — public HUD shell (layout and update cadence);
  metric internals are owned by [`profiling.md`](profiling.md)
- `crates/brain-visualizer/src/lib.rs` — the wasm_bindgen entry surface:
  `WasmGpuBackend.create` lifecycle, the JS-facing tick/render/settings API,
  the &mut reentrancy discipline (the data contracts that cross —
  `VisualSettings`, `SimConfig`/`TickStats` — are owned by
  [`simulation.md`](simulation.md) and [`dev-panel.md`](dev-panel.md); this
  doc owns the *bridge mechanics*)
- The natural silent-start invariant: there is no intro code anywhere in the
  frontend; see [Natural start](#natural-start) below

## What it does NOT own

- Dev panel, settings persistence, settings schema → [`dev-panel.md`](dev-panel.md)
- Perf profiler internals, metric field layout, GPU timestamp queries →
  [`profiling.md`](profiling.md)
- GPU pipeline objects, shader dispatch, wgpu resources → [`gpu-rendering.md`](gpu-rendering.md)
- Sim dynamics, tick logic, neuron model → [`simulation.md`](simulation.md)
- Tier presets, N_MIN/N_MAX tables, the dormant scaler decision fn → [`scaling.md`](scaling.md)

## The rAF loop and the &mut discipline

`web/src/main.ts → rafLoop` is the single owner of `WasmGpuBackend`. The browser
event handlers (pointermove, resize, devPanel callbacks) never call the backend
directly — they set one of the pending flags. At the **top** of every rAF turn,
before any backend call, all pending flags are flushed in order:

1. `pendingResize` → `gpuBackend.resize()`
2. `pendingNetworkRebuild` → `gpuBackend.reinitialize()`
3. `pendingSettingsPush` → `gpuBackend.update_settings()`
4. `pendingStim` → `gpuBackend.stimulate()`

After flushing, the loop calls `gpuBackend.tick(ticks, excitability)` then
`gpuBackend.render_frame(mvp, right, up, eye, dist)`. Violating this ordering
triggers the wasm-bindgen "recursive use of an object" panic at runtime.

The JS `Renderer` wrapper (`web/src/render/renderer.ts`) is kept alive as a fallback
clear-to-black path for the brief window between boot and the async
`WasmGpuBackend.create()` completing. Once the backend is live, all rendering
goes through it.

## Wasm call boundary

Three categories of backend call, each with a different cost profile:

| Call | When | Notes |
|------|------|-------|
| `gpuBackend.render_frame(mvp, right, up, eye, dist)` | Every frame | Cheap JS→wasm boundary; GPU work happens inside |
| `gpuBackend.tick(ticks, excitability)` | Every frame (time-based accumulator) | Submits compute passes; returns spike count |
| `gpuBackend.update_settings(Float32Array)` | On settings change | Pushes `VisualSettings` uniform; one per change event |
| `gpuBackend.set_morphology_config(json)` | On morphology config apply | Separate JSON path for the dev-panel morphology config; the backend diffs and runs the narrowest update. Distinct from the Float32Array — see below |

`render_frame` receives the MVP matrix and billboard axes from `Camera`; it does
not read back any GPU state. The struct contract for `VisualSettings` and the
tick return value live in `crates/brain-visualizer/src/lib.rs`; cross-link to
[`simulation.md`](simulation.md) for the sim-side contract. This doc owns the
*bridge mechanics* (call ordering, pending-flag discipline, reentrancy rules);
the data-layout contracts for `VisualSettings` and `SimConfig`/`TickStats` are
owned by [`dev-panel.md`](dev-panel.md) and [`simulation.md`](simulation.md)
respectively.

The morphology config travels a **separate** channel from the Float32Array:
`crates/brain-visualizer/src/lib.rs → WasmGpuBackend::set_morphology_config` takes
a JSON string (the `MorphologyConfig` from `web/src/core/morph-config.ts`,
persisted under its own `bv2_morph_v1` key) rather than a packed float array, and
the backend chooses the narrowest update path. The dev-panel apply is queued like
the other backend calls (a `pendingMorphConfig` flag flushed in the rAF loop).
Why a separate key + entry point rather than extending the frozen Float32Array:
see [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md).

## Camera

`web/src/render/camera.ts → Camera` is a pure orbit camera with a movable
target: azimuth/elevation/distance plus `target` feed `mvpMatrix()`,
`cameraRight()`, `cameraUp()`, `eye()`. Left-drag updates azimuth/elevation;
right-drag and Shift-left-drag pan the target in screen space; wheel/pinch
updates distance. A keyboard `R` shortcut recenters the target. Touch remains
one-finger orbit and two-finger pinch zoom. The camera has no readback path and
no coupling to the sim — it computes vectors on the JS side and hands them to
`render_frame` each frame.

`Camera.unproject()` produces world-space rays for cursor stimulation; the
ray-sphere intersection (`raySphereIntersect`, manifold radius `MANIFOLD_SPHERE_RADIUS = 1.4`)
runs in `computeStimulation` in `main.ts`. The hit point is queued as
`pendingStim`, not applied inline.

## Controls

`web/src/ui/controls.ts → BRAIN_STATES` maps the five named states to excitability
values on `[0, 1]`. Setting a brain state calls `setExcitabilityTarget()`, which
sets `_targetExcitability`; `tickExcitability()` advances `_currentExcitability`
toward the target at `EXCITABILITY_LERP = 0.08` per frame. The smoothed value is
passed to `gpuBackend.tick()` each frame.

Speed is now a time-based accumulator (`targetTicksPerSec`, set by the dev
panel) rather than the older `SpeedPreset` frame-count multiplier. `ticksThisFrame`
in `controls.ts` still exists for backward compat but the main rAF loop uses the
accumulator path exclusively.

The `Controls` class is a thin backwards-compat facade; `main.ts` wires DOM
handlers directly to the module-level functions.

A bottom-center **pause** button (`#pause-toggle` in `index.html`, wired in
`main.ts`) flips a `paused` flag the rAF loop reads: while paused it zeroes the
per-frame tick count and drains `tickAccumulator` (so resume doesn't burst), but
`render_frame` and the camera keep running — the sculpture freezes mid-flight
while orbit/zoom stay live. It is a pure JS flag (no backend `&mut` call) and so
is available on mobile too.

## Renderer (canvas + device acquisition)

`web/src/render/renderer.ts → Renderer.init()` acquires the WebGPU adapter and device and
configures the canvas context. This is only used during the brief init window
before `WasmGpuBackend.create()` completes. After that, the backend owns the
surface and `Renderer.render()` is called only as a black-canvas fallback
(clear-only). The HDR render target lives inside the wasm backend, not in the
TS wrapper.

## Types and DEFAULT_CONFIG

`web/src/core/types.ts → DEFAULT_CONFIG` boots at `n=1_200, k=16` — the morphology
beauty target where each neuron can be drawn as a procedural soma+dendrite+axon.
Tier presets and the per-tier N bounds are in `web/src/ui/controls.ts →
TIER_PRESETS`, `N_MIN`, `N_MAX`; tier→N/K logic belongs to
[`scaling.md`](scaling.md).

## AppConfig persistence

The user-chosen runtime knobs in `AppConfig` are persisted to localStorage so a
reload restores the last-used network — they were previously lost on every reload.
`web/src/core/types.ts → loadConfig`, `saveConfig`, `resetConfig` own this; the key is
`bv2_config_v1`. The shape deliberately mirrors the dev-panel settings pattern
([`dev-panel.md`](dev-panel.md)): a versioned key, a version gate that falls back
to `DEFAULT_CONFIG` on mismatch/parse-error/missing key, a field-by-field
`?? base` merge over defaults, and a `try/catch` so a blocked localStorage
(private browsing, quota) degrades silently.

**Persisted fields:** `n`, `k`, `tier`, `backend`, `speed`, `excitability`.
**Not persisted:** `seed` (a fixed constant) and any runtime counters.

Wiring:

- **Boot** — `web/src/main.ts → boot` seeds `config` from `loadConfig()` (not
  `DEFAULT_CONFIG` directly). The mobile profile override is applied **after**
  load and then re-saved, so the forced low-tier profile survives a reload.
- **On mutation** — every `AppConfig` field change saves: tier/backend/speed
  setters and the `Controls` class methods in `web/src/ui/controls.ts` call
  `saveConfig`, and the dev-panel N/K rebuild path in `main.ts` saves after
  mutating `config.n`/`config.k`. `setBackend` saves *after* `restartFn` so a
  CPU→GPU fallback (backend reverting on failure) persists the actual backend.
- **Excitability** — the live control is the dev-panel excitability slider; its
  `onExcitability` handler in `main.ts` writes `config.excitability` and saves.
  At boot, `web/src/ui/controls.ts → seedExcitability` primes both the current
  and target of the excitability lerp from `config.excitability`, so a restored
  value applies immediately with no ramp from the default. (`setBrainState` —
  the named-state buttons removed in the UX overhaul — is dormant, like
  `scalerDecide`.)

## Sonification

`web/src/audio/sonification.ts → SonificationEngine` holds a Web Audio voice bank: three
sine oscillators (input/assoc/output regions at 110/220/440 Hz) plus a
`ScriptProcessorNode` white-noise layer. The `AudioContext` is created on first
`enable()` call (user-gesture gate). Gain is updated once per second from the
profiler snapshot via `update(regionFractions, totalFraction)` — never in the
hot rAF path. Disabled on mobile. Muted by default (user must click the sound
toggle).

`deriveRegionFractions` approximates per-region rates from total `spikesPerSec`
using fixed anatomical fractions (30% / 40% / 30%); the backend does not expose
per-region spike counts to JS.

## Natural start

There is no intro code, no scripted seed spike, and no animation sequence in
`main.ts` or anywhere in the frontend. The sim starts immediately at boot; the
`boot()` function calls `startGpuBackend()` and `requestAnimationFrame(rafLoop)`
without any deferral. The posterior→anterior propagation that serves as the
visual "wake-up" emerges from the sim's ambient input-region drive — the sim
owns that drive; the frontend's only role is to not suppress it. See
[`simulation.md`](simulation.md) for the `I_ext` wiring.

## Mobile profile

`isMobile()` in `web/src/ui/controls.ts` gates the mobile profile in `main.ts`:
0.75× DPR, GPU backend only, no cursor stimulation, no sound toggle, no dev
panel. The canvas-resize handler accounts for the dev panel width when open.

## Update when

- `WasmGpuBackend`'s public JS surface changes (new tick/render/settings call
  signatures, new pending-flag categories).
- `Camera` gains new outputs used by `render_frame` (new vectors, new LOD
  inputs).
- The time-based tick accumulator is replaced or extended.
- Sonification gains per-region data from the backend (removes the approximation
  in `deriveRegionFractions`).
- `DEFAULT_CONFIG` default neuron count changes.
- The mobile profile changes (DPR scale, feature exclusions).

## See also

- [`simulation.md`](simulation.md) — tick contract, `VisualSettings` struct,
  `I_ext` ambient drive that produces natural start
- [`scaling.md`](scaling.md) — tier presets, N bounds, the dormant scaler
  decision fn
- [`dev-panel.md`](dev-panel.md) — settings panel, settings persistence,
  Monitor tab metrics
- [`profiling.md`](profiling.md) — `Profiler` internals, metric layout,
  `CornerHud` data sources
- [`gpu-rendering.md`](gpu-rendering.md) — GPU pipeline objects and shader
  dispatch owned by the wasm backend
- [`../decisions/interaction.md`](../decisions/interaction.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
