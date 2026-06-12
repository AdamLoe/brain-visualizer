---
status:        active
owner:         adamg
last_updated:  2026-06-12
---

# Web Frontend

The TypeScript app shell that drives everything visible in the browser. Its one
job is to own the rAF loop, broker all input events, and hold the single live
reference to `WasmGpuBackend` — routing every mutation through that loop to
avoid wasm-bindgen reentrancy panics.

## What it owns

- `web/src/main.ts` — boot sequence, startup overlay state, rAF loop (`rafLoop`), pending resize/stim
  plumbing, worker-prepared network rebuild wiring, `RebuildCoordinator` wiring
  for settings/morphology rebuild mutations, `CpuCoordinator`, `startGpuBackend`,
  `restartWithBackend`, `computeStimulation`, `raySphereIntersect`
- `web/src/gpu-build/network-build-client.ts → NetworkBuildClient` and
  `web/src/gpu-build/network-build-worker.ts` — latest-wins worker preparation
  for network rebuild payloads; the worker owns a worker-local WASM instance and
  never requests WebGPU
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
- `crates/brain-visualizer/src/lib.rs` — the wasm_bindgen entry surface:
  `WasmGpuBackend.create` / `create_staged` lifecycle, the JS-facing tick/render/settings API,
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
directly — they queue work for the next frame. At the **top** of every rAF turn,
before any backend call, pending DOM work is flushed in order:

1. `pendingResize` → `gpuBackend.resize()`
2. `NetworkBuildClient.consumeReady()` → if a worker-prepared network payload is
   ready, `gpuBackend.apply_prepared_network(...)`
3. `RebuildCoordinator.applyNext()` → at most one settings/morphology mutation:
   `gpuBackend.update_settings()` or `set_morphology_config(json)`
4. `pendingStim` → `gpuBackend.stimulate()`

Network N/K/seed changes no longer call `gpuBackend.reinitialize()` from rAF.
`main.ts` snapshots the current `VisualSettings` Float32Array and morphology
JSON, assigns a monotonic sequence, and sends the request to
`NetworkBuildClient`. The worker returns a flat `PreparedNetworkPayload`:
positions, region codes, surface vertices/faces, spatial-grid CSR arrays,
morphology segment field arrays, soma field arrays, and stats/config metadata.
The client accepts only the latest requested sequence; stale ready/failure
messages are ignored before they can reach the backend. rAF remains the only
backend mutator: when the latest payload is ready it calls
`WasmGpuBackend::apply_prepared_network`, then queues a settings and morphology
re-push so any newer UI state is restored after the structural rebuild.

`web/src/rebuild/rebuild-coordinator.ts → RebuildCoordinator` still owns
latest-wins settings pushes and morphology config pushes. Morphology generator
changes that are not part of a full network rebuild still go through
`set_morphology_config(json)` on the main thread; moving that CPU generation to
the worker is a remaining responsiveness follow-up.

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
any heavy work. `main.ts` updates `window.__bvStartup` and the overlay through
coarse page stages, then through the measured backend stages. The overlay shows
the current stage, detail text, elapsed time, completed stage count, frame
counter, progress bar, and the most recent per-stage timings. On success it
fades out after the first GPU frame; on backend failure it remains visible with
the error and any completed timings.

`boot()` starts a lightweight startup `requestAnimationFrame` loop before
`init()` so tests and users can see `window.__bvFrameCounter` advance while wasm
and backend work is pending. GPU startup begins as soon as config/canvas and the
pending-flag queues exist, while the remaining HUD/dev-panel/control wiring
continues during async WebGPU adapter/device acquisition. The real app `rafLoop`
replaces the lightweight loop later, but it sees `gpuBackend === null` until the
staged backend has completed every startup stage. This prevents the rAF loop
from touching half-built GPU resources.

The staged path is `WasmGpuBackend.create_staged()` followed by explicit
`startup_*` calls: build manifold, upload neuron/grid buffers, upload render
mesh, allocate LOD/edge buffers, generate/upload morphology, refresh bind
groups/reset state, compile render pipelines, and create render targets. `main.ts`
awaits one animation frame before each stage and records its wall-clock duration,
so the progress bar advances from completed work instead of a single fake
backend percentage. The legacy `WasmGpuBackend.create()` monolith remains as a
compatibility fallback. The old pre-backend `init_manifold()` logging path was
removed; the real manifold is now created only inside the backend startup path.

## Wasm call boundary

Three categories of backend call, each with a different cost profile:

| Call | When | Notes |
|------|------|-------|
| `gpuBackend.render_frame(mvp, right, up, eye, dist)` | Every frame | Cheap JS→wasm boundary; GPU work happens inside |
| `gpuBackend.tick(ticks, excitability)` | Every frame (time-based accumulator) | Submits compute passes; returns spike count |
| `gpuBackend.update_settings(Float32Array)` | On settings change | Pushes `VisualSettings` uniform; one per change event |
| `gpuBackend.set_morphology_config(json)` | On morphology config apply | Separate JSON path for the dev-panel morphology config; the backend diffs and runs the narrowest update. Distinct from the Float32Array — see below |
| `gpuBackend.apply_prepared_network(flat payload...)` | On worker-prepared N/K/seed rebuild | Validates the versioned flat typed-array payload, reconstructs Rust manifold/grid/morphology structs, then performs main-thread WebGPU upload/resource creation |
| `gpuBackend.startup_*()` | Startup only | Staged network/resource creation. JS yields between calls and records timings; the instance is not assigned to the rAF-owned `gpuBackend` until complete. |

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
persisted under its own `bv2_morph_v2` key) rather than a packed float array, and
the backend chooses the narrowest update path. The dev-panel apply is queued like
the other backend calls through `RebuildCoordinator`. Boot also queues
`morphConfigToJson(loadMorphConfig())` before backend creation and refreshes that
queued JSON immediately after `WasmGpuBackend.create()`, so
persisted morphology settings are applied to the Rust backend on the first
available frame even when the user never touches a morphology slider.
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

`web/src/render/renderer.ts → Renderer.init()` is intentionally passive: it logs
readiness but does not acquire the WebGPU adapter/device, configure the canvas,
or create fallback WebGL2/2D contexts. `WasmGpuBackend.create_staged()` (or the
legacy `create()` fallback) is the only live startup path that acquires the
browser WebGPU device and surface. `Renderer.render()` is a no-op before backend
readiness; the startup overlay owns visible feedback. The HDR render target
lives inside the wasm backend, not in the TS wrapper.

## Types and DEFAULT_CONFIG

`web/src/core/types.ts → DEFAULT_CONFIG` boots at `n=6_000, k=16,
excitability=0.10, ticksPerSec=30` — the high-scale beauty baseline where the
network is calm enough for propagation to be visible. The product neuron-count cap is
`PRODUCT_MAX_N = 20_000`; `loadConfig()` and `saveConfig()` clamp persisted or
incoming `n` through `clampNeuronCount()`, so old saved high-N localStorage
payloads cannot exceed the current product cap. Tier presets and the per-tier N
bounds are in `web/src/ui/controls.ts → TIER_PRESETS`, `N_MIN`, `N_MAX`;
tier→N/K logic belongs to [`scaling.md`](scaling.md).

## AppConfig persistence

The user-chosen runtime knobs in `AppConfig` are persisted to localStorage so a
reload restores the last-used network — they were previously lost on every reload.
`web/src/core/types.ts → loadConfig`, `saveConfig`, `resetConfig` own this; the key is
`bv2_config_v2`. The shape deliberately mirrors the dev-panel settings pattern
([`dev-panel.md`](dev-panel.md)): a versioned key, a version gate that falls back
to `DEFAULT_CONFIG` on mismatch/parse-error/missing key, a field-by-field
`?? base` merge over defaults, a hard clamp of saved `n` to `PRODUCT_MAX_N`, and
a `try/catch` so a blocked localStorage (private browsing, quota) degrades
silently.

**Persisted fields:** `n`, `k`, `tier`, `speed`, `excitability`, `ticksPerSec`.
Stale saved `backend: "cpu"` values are coerced back to `"gpu"` at boot because
V2 keeps the CPU path parked and hidden.
**Not persisted:** `seed` (a fixed constant) and any runtime counters.

Wiring:

- **Boot** — `web/src/main.ts → boot` seeds `config` from `loadConfig()` (not
  `DEFAULT_CONFIG` directly), then forces `backend = "gpu"` before backend
  startup so stale CPU saves cannot put the rAF loop on the parked CPU branch.
  The mobile profile override is applied **after** load and then re-saved, so
  the forced low-tier profile survives a reload.
- **On mutation** — every active `AppConfig` field change saves: tier/speed
  setters and the `Controls` class methods in `web/src/ui/controls.ts` call
  `saveConfig`, and the dev-panel N/K rebuild path in `main.ts` saves after
  mutating `config.n`/`config.k`. The old backend setter still exists for
  compatibility, but V2 boot forces GPU because the backend toggle is hidden.
- **Excitability** — the live control is the dev-panel excitability slider; its
  `onExcitability` handler in `main.ts` writes `config.excitability` and saves.
  At boot, `web/src/ui/controls.ts → seedExcitability` primes both the current
  and target of the excitability lerp from `config.excitability`, so a restored
  value applies immediately with no ramp from the default. (`setBrainState` —
  the named-state buttons removed in the UX overhaul — is dormant, like
  `scalerDecide`.)

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
