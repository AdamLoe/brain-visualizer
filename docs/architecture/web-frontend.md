---
status:        active
owner:         adamg
last_updated:  2026-06-20
---

# Web Frontend

The TypeScript app shell owns the rAF loop, brokers browser input, and holds the
single live `WasmGpuBackend` reference — routing every mutation through that loop
to avoid wasm-bindgen reentrancy panics.

## What it owns

- `web/src/main.ts` — boot sequence, startup overlay state, rAF loop (`rafLoop`),
  pending resize/stim plumbing, worker-prepared rebuild wiring, and cursor
  stimulation helpers (`computeStimulation`, `raySphereIntersect`)
- `web/src/gpu-build/network-build-client.ts → NetworkBuildClient` and
  `web/src/gpu-build/network-build-worker.ts` — latest-wins worker preparation
  for network rebuild payloads; the worker owns a worker-local WASM instance and
  never requests WebGPU
- `web/src/boot-sequencer.ts → runGpuStartup` — staged GPU-backend startup,
  weighted progress, payload-progress buffering, and GPU-free boot tests
- `web/src/boot-overlay.ts` — pure startup-overlay label/band helpers
  (`formatSubStageLabel`, `mapSubStageProgress`)
- `web/src/boot-timings.ts` — `window.__bvBootTimings`, boot summary logging,
  and dev-only stall watchdog (`evaluateStall`, `startBootWatchdog`)
- `web/index.html` — the immediate DOM/CSS startup overlay and full-viewport
  canvas shell
- `web/src/render/camera.ts → Camera` — orbit/zoom/pan state machine; produces MVP matrix,
  billboard right/up vectors, and unprojection rays
- `web/src/ui/controls.ts` — `BRAIN_STATES`, `tickExcitability`, `setExcitabilityTarget`,
  `TIER_PRESETS`, `scalerDecide`, `ticksThisFrame`, `isMobile`, `Controls`
- `web/src/render/renderer.ts → Renderer` — passive startup renderer facade;
  it deliberately does not claim WebGPU/WebGL/2D canvas contexts before
  `WasmGpuBackend` owns the live WebGPU surface
- `web/src/core/types.ts → AppConfig`, `DEFAULT_CONFIG`, `SpeedPreset`, `BackendKind`,
  `Tier`, `BrainState`, `TickStats`, plus `AppConfig` localStorage persistence
  (`loadConfig`, `saveConfig`)
- `web/src/ui/hud.ts → CornerHud` — public HUD shell (layout and update cadence);
  metric internals are owned by [`profiling.md`](profiling.md)
- `crates/brain-visualizer/src/lib.rs` — `WasmGpuBackend` wasm_bindgen entry
  surface and JS-facing tick/render/settings API; this doc owns bridge mechanics,
  not the `VisualSettings` or `SimConfig`/`TickStats` data contracts
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
directly — they queue work for the next frame. At the **top** of every rAF turn,
before any backend call, pending DOM work is flushed in order:

1. `pendingResize` → `gpuBackend.resize()`
2. `NetworkBuildClient.consumeReady()` → if a worker-prepared network payload is
   ready, `gpuBackend.apply_prepared_network(...)`
3. `RebuildCoordinator.applyNext()` → at most one settings/morphology mutation:
   `gpuBackend.update_settings()` or `set_morphology_config(json)`
4. `pendingStim` → `gpuBackend.stimulate()`

Network N/K/seed and region-assignment-mode changes go through
worker-prepared payloads instead of direct `gpuBackend.reinitialize()` calls
from rAF. `main.ts` snapshots the current
`VisualSettings` Float32Array, morphology JSON, and
`AppConfig.regionAssignmentMode`, assigns a monotonic sequence, and sends the request to
`NetworkBuildClient`. The worker returns a flat `PreparedNetworkPayload`:
positions, region codes, surface vertices/faces, spatial-grid CSR arrays,
morphology segment field arrays, soma field arrays, and stats/config metadata.
The client accepts only the latest requested sequence; stale ready/failure
messages are ignored before they can reach the backend. rAF remains the only
backend mutator: when the latest payload is ready it calls
`WasmGpuBackend::apply_prepared_network`, then queues a settings and morphology
re-push so any newer UI state is restored after the structural rebuild.

`web/src/rebuild/rebuild-coordinator.ts → RebuildCoordinator` still owns
latest-wins immediate settings pushes and non-generator morphology config
pushes. `web/src/rebuild/rebuild-intent.ts` classifies structural UI changes:
N/K/seed, region assignment, connection-curve lift, reach knobs, and morphology generator config
changes request a worker-prepared payload for the current network; uniform-only
lighting and render-quality-only morphology changes can still flow through
`set_morphology_config(json)` because they do not run the morphology generator.

After flushing, the loop calls `gpuBackend.tick(ticks, excitability)` then
`gpuBackend.render_frame(mvp, right, up, eye, dist)`. Violating this ordering
triggers the wasm-bindgen "recursive use of an object" panic at runtime.

The JS `Renderer` wrapper (`web/src/render/renderer.ts`) is kept alive only as a
passive compatibility facade during GPU startup. It does not request a WebGPU
adapter/device and does not acquire WebGL2 or 2D fallback contexts on the brain
canvas, because any pre-backend canvas context can prevent `WasmGpuBackend` from
claiming the WebGPU surface. The visible pre-backend state is the DOM startup
overlay and CSS canvas background. Once the backend is live, all rendering goes
through `WasmGpuBackend.render_frame()`.

## Startup Feedback

`web/index.html` includes a fixed `#startup-overlay` in the initial HTML, so the
browser paints a loading surface before the TypeScript module and wasm init do
any heavy work. The overlay is intentionally product-facing: title, progress,
percent, and current stage only. `web/src/main.ts → updateStartupOverlay` accepts
`{ status?, stage?, progress? }`, updates `window.__bvStartup`, and keeps
`__bvFrameCounter` / `__bvStartup.status` available as E2E hooks. On success the
overlay fades out after the first GPU frame. On missing WebGPU support or backend
startup failure it stays visible with visitor-facing WebGPU guidance from
`web/src/boot-failure.ts`, `failed` in the percent slot, and raw diagnostics only
in the console. Detailed timing state lives in `web/src/boot-timings.ts`, not in
overlay DOM.

`web/src/main.ts → boot` starts a lightweight startup rAF before `init()` so the
frame counter advances while wasm and backend work are pending. GPU startup then
delegates to `web/src/boot-sequencer.ts → runGpuStartup`, which starts the
network-build worker early, overlaps payload preparation with WebGPU acquisition,
and uses `WasmGpuBackend.create_staged()` plus explicit `startup_*` calls. The
real app `rafLoop` can run during startup, but `gpuBackend` remains null until
every staged resource step has completed; this keeps the rAF-owned backend
discipline intact.

`runGpuStartup` owns the weighted backend-stage table, progress-listener
lifecycle, frame-yield points, and latest buffered worker-progress fraction. The
pure label/band helpers live in `web/src/boot-overlay.ts →
formatSubStageLabel, mapSubStageProgress`, and the continuous worker-payload
fractions come from `crates/brain-visualizer/src/sim/gpu/mod.rs →
PreparedNetworkBuild::prepare_with_progress` through
`crates/brain-visualizer/src/lib.rs → prepare_network_payload` and
`web/src/gpu-build/network-build-client.ts → NetworkBuildClient.onProgress`.
Only real progress fractions add a within-stage percent label; stages without a
sub-stage callback show their bare label.

`web/src/boot-timings.ts` owns `window.__bvBootTimings`, `recordBootTiming`,
`logBootSummary`, `evaluateStall`, and the dev-only `startBootWatchdog`. Timing
rows are collected for console/dev inspection, while the overlay remains compact
status UI. After the first rendered frame, `rafLoop` calls
`gpuBackend.build_deferred_render_pipelines()` so bloom and active morphology
variants compile off the boot-critical path; `render_full` guards deferred
pipeline access until those pipelines exist. See [`gpu-backend.md`](gpu-backend.md)
and [`../decisions/rendering.md`](../decisions/rendering.md).

## Wasm call boundary

The JS-facing backend surface is owned by
`crates/brain-visualizer/src/lib.rs → WasmGpuBackend`. The hot-frame calls are
`tick` and `render_frame`; startup and structural rebuilds use the staged
`create_staged` / `startup_*` / prepared-network methods; settings and
morphology apply paths use `update_settings` and `set_morphology_config`. This
doc owns the *bridge mechanics* (call ordering, pending-flag discipline,
reentrancy rules). The data-layout contracts for `VisualSettings` and
`SimConfig`/`TickStats` are owned by [`dev-panel.md`](dev-panel.md) and
[`simulation.md`](simulation.md) respectively.
`VisualSettings` changes remain latest-wins, and render-only knobs such as
until-arrival hold should flow through that packed settings call rather than a
structural rebuild path.

The morphology config travels a **separate** channel from the Float32Array:
`crates/brain-visualizer/src/lib.rs → WasmGpuBackend::set_morphology_config` takes
a JSON string (the `MorphologyConfig` from `web/src/core/morph-config.ts`,
persisted under its own `bv2_morph_v2` key) rather than a packed float array, and
the backend chooses the narrowest immediate update path. The dev-panel apply is
queued like the other backend calls through `RebuildCoordinator` only when the
change is uniform-only or render-quality-only; generator changes request a
worker-prepared payload instead. Boot passes `morphConfigToJson(loadMorphConfig())`
into the startup worker request, so persisted generator settings are already
present in the prepared startup payload even when the user never touches a
morphology slider.
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

Speed uses a time-based accumulator (`targetTicksPerSec`, set by the dev
panel) rather than the legacy `SpeedPreset` frame-count multiplier. `ticksThisFrame`
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

`web/src/render/renderer.ts → Renderer.init()` is intentionally passive: it logs
readiness but does not acquire WebGPU or fallback canvas contexts.
`WasmGpuBackend.create_staged()` (or the legacy `create()` fallback) is the only
startup path that claims the browser WebGPU device and surface. `Renderer.render()`
is a no-op before backend readiness; the startup overlay owns visible feedback.

## Types and DEFAULT_CONFIG

`web/src/core/types.ts → DEFAULT_CONFIG` is the authoritative boot scale and
runtime-default snapshot. `PRODUCT_MAX_N`, `clampNeuronCount()`, `loadConfig()`,
and `saveConfig()` enforce the product neuron-count cap for persisted or incoming
`n`, while `loadConfig()` / `saveConfig()` also normalize the persisted runtime
knobs to the same bounded domains the dev panel exposes. Tier presets and the
per-tier N bounds are in `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`,
`N_MAX`; tier→N/K logic belongs to [`scaling.md`](scaling.md).

## AppConfig persistence

The user-chosen runtime knobs in `AppConfig` are persisted to localStorage so a
reload restores the last-used network.
`web/src/core/types.ts → loadConfig`, `saveConfig`, `resetConfig` own this; the key is
`web/src/core/types.ts → CONFIG_LS_KEY` (`bv2_config_v2`). The loader uses a
version gate, merge-over-defaults, bounded value normalization, and silent
storage failure handling, matching the dev-panel persistence pattern.

The persisted subset is defined by `web/src/core/types.ts → SavedConfig`. The
only live backend value is `"gpu"`; stale saved backend values are normalized by
`loadConfig()` so old localStorage payloads cannot break startup. Unknown
`regionAssignmentMode` strings normalize to the default so stale prototype saves
cannot promote an unrecognized mode. `seed` remains a fixed constant, and runtime
counters are not persisted.

Wiring:

`web/src/main.ts → boot` seeds `config` from `loadConfig()`, applies/saves the
mobile profile override through `web/src/core/mobile-config.ts → applyMobileConfig`,
and wires mutation handlers to `saveConfig`. The mobile profile lowers render DPR
and disables cursor stimulation without increasing `n` above
`web/src/core/types.ts → DEFAULT_CONFIG`. The dev-panel excitability handler
saves `config.excitability`; at boot,
`web/src/ui/controls.ts → seedExcitability` primes both the current and target
of the excitability lerp so a restored value applies immediately. `setBrainState`
and `scalerDecide` are retained compatibility paths, not active DOM controls.

## Natural start

There is no intro code, no scripted seed spike, and no simulation animation
sequence in `main.ts` or anywhere in the frontend. Startup has a DOM loading
overlay only; once the backend is ready, the sim starts from its natural silent
state. The `boot()` function starts rAF before async GPU creation and drives
staged backend startup with frame yields so the page keeps painting while the
backend is pending. The posterior→anterior propagation
that serves as the visual "wake-up" emerges from the sim's ambient input-region
drive — the sim owns that drive; the frontend's only role is to not suppress it. See
[`simulation.md`](simulation.md) for the `I_ext` wiring.

## Mobile profile

`isMobile()` in `web/src/ui/controls.ts` gates the mobile profile in `main.ts`:
0.75× DPR, GPU backend only, no cursor stimulation, no dev panel. The
canvas-resize handler accounts for the dev panel width when open.

## Update when

- `WasmGpuBackend`'s public JS surface changes (new tick/render/settings call
  signatures, new pending-flag categories).
- `Camera` gains new outputs used by `render_frame` (new vectors, new LOD
  inputs).
- The time-based tick accumulator is replaced or extended.
- `DEFAULT_CONFIG` default neuron count changes.
- `PRODUCT_MAX_N` or the config clamp behavior changes.
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
