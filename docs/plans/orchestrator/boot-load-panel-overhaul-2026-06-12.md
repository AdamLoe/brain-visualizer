---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/web-frontend.md
  - architecture/dev-panel.md
  - architecture/gpu-backend.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
  - architecture/build-and-deploy.md
---

# Boot / Load-Panel Overhaul

## Goal

Make the boot *feel* fast and honest: speed up the GPU-acquire/compile stage,
surface real intermediate progress from inside it, and strip the loading panel
down to a clean three-row layout (name / bar / percent + stage).

---

## Key facts established during investigation (read before starting)

- **wgpu version is 29.0.3.** The public `wgpu::Device` API in v29 exposes only
  the **blocking** `create_compute_pipeline` / `create_render_pipeline`. The
  async variants (`create_compute_pipeline_async`) exist **only** in the
  low-level `webgpu_sys` binding (`wgpu-29.0.3/src/backend/webgpu/webgpu_sys/gen_GpuDevice.rs:228`),
  **not** in the safe `api/device.rs`. So "just switch to
  `create_*_pipeline_async`" is **NOT feasible** through wgpu's safe API without
  dropping to raw `web_sys::gpu`/`wasm-bindgen` plumbing (large, risky rewrite).
  Treat async pipeline creation as out of scope; the wins below do not need it.
- **13 pipelines total** compile synchronously, each with its own
  `create_shader_module`: 4 compute in `build_sim` (integrate, write_dispatch,
  scatter, metrics) + 1 stimulate compute + 1 compact compute + ~7 render
  pipelines (manifold, far points, 3 bloom, morphology additive+active, soma
  additive+active) in `build_render` (`pipelines.rs`). On WebGPU each
  `create_*_pipeline` blocks the calling thread while the browser compiles the
  shader — this is the dominant cost of the "Acquire GPU + core pipelines" +
  "Compile render pipelines" stages.
- **The worker payload already overlaps the GPU handshake.** `startGpuBackend`
  fires `requestPreparedNetwork("startup", …)` at `main.ts:479` *before* running
  the GPU-acquire stage; the worker (`network-build-worker.ts`, its own WASM
  instance) generates manifold/placement/grid/morphology in parallel while the
  main thread `await`s `acquire_web`. The "Prepare network payload" stage just
  `await`s the already-running worker. So that suspect is **largely a non-issue**
  — but see A4 for the one true serialization gap (worker WASM `init()` is lazy).
- **"Generate morphology" stage is mislabeled.** With a worker payload,
  `initialize_morph_resources` (`mod.rs:1423`) takes the `prepared_morphology`
  branch — it only uploads buffers, it does **not** regenerate. No CPU morphology
  work happens on the main thread during boot.
- **Device limits are over-generous** but probably cheap: `acquire_web`
  (`mod.rs:1166-1187`) starts from `downlevel_webgl2_defaults()` then bumps four
  limits to the adapter max. This is unlikely to be a real cost on the WebGPU
  path (see A3 — low priority, verify-don't-assume).

---

## Workstream A — Boot speedup (prioritized)

### A1 (HIGHEST CONFIDENCE) — Defer the render pipelines that are not needed for the first frame
- **File/symbol:** `crates/brain-visualizer/src/sim/gpu/pipelines.rs` `build_render`
  (lines ~233-740); `mod.rs` `build_render_pipelines` (~1497); the staged stage
  ordering in `main.ts` stages array (~579-588).
- **What:** The first rendered frame only needs the scene pipelines that actually
  draw it (manifold + far points + morphology + soma). The **3 bloom pipelines**
  (`bloom_bright`, `bloom_blur`, `bloom_composite`, ~lines 399-490) and the
  **`render_*_active` variants** are not required to paint frame 1 and can be
  compiled lazily on first use (or one rAF turn after `ready`). Split
  `build_render` into `build_render_core()` (manifold/far/morph/soma + stimulate
  + compact) called in the "Compile render pipelines" startup stage, and
  `build_render_deferred()` (bloom + active variants) called from the rAF loop on
  the first frame *after* `firstReadyFrameSeen`, or guarded behind a "bloom
  enabled" check. Add a `pipelines_complete: bool` guard so `render_frame`
  no-ops the bloom pass until the deferred pipelines exist.
- **Payoff:** Removes 3-5 synchronous shader compiles from the critical path →
  the largest single felt win after A2. Bloom appears ~1 frame late (invisible).
- **Risk:** Medium. `render_full` must tolerate missing bloom pipelines for the
  first frame(s); guard every `bloom_*.as_ref().unwrap()` (search `render_full`).
  Must not break the no-bloom render path. Verify visually on hardware that bloom
  still turns on.
- **Verify:** `cargo test` (pipeline build unit tests at bottom of `pipelines.rs`);
  on hardware, confirm first frame paints and bloom appears within ~1 frame.

### A2 (HIGH CONFIDENCE) — Yield to the browser between pipeline compiles so the progress bar paints
- **File/symbol:** `main.ts` `runStage` (~591-617) + new sub-stage emits from
  Rust (see Workstream B). Also `pipelines.rs` `build_sim`/`build_render`.
- **What:** Today both compile-heavy stages ("Acquire GPU + core pipelines" and
  "Compile render pipelines") run as a single synchronous Rust call with one
  `await nextAnimationFrame()` *before* and the progress jump *after*. The main
  thread is blocked for the whole compile, so the bar visibly freezes. This does
  **not** make compilation faster, but it is the core of the *felt-slowness*
  complaint. Pair with Workstream B: emit a progress event from Rust between each
  `create_*_pipeline` and let the JS callback `await` a microtask/rAF so the bar
  repaints. Net effect: same wall-clock, but the user sees continuous motion +
  honest labels instead of a 54%→freeze→snap.
- **Payoff:** Large *perceived* speedup; this is most of what the user is asking
  for in (1)+(2) combined. No real compute saved.
- **Risk:** Low. The only subtlety: yielding mid-pipeline-build means the Rust
  side must expose finer-grained build steps OR the progress callback fires from
  Rust without yielding (browser still can't paint mid-synchronous-call). The
  pragmatic version: keep Rust building synchronously but split the *Rust-exposed
  stages* finer (e.g. `startup_build_compute_pipelines` vs
  `startup_build_render_pipelines`) so JS gets a real rAF gap between them, and
  have Rust emit a label via the B channel just before each group.
- **Verify:** `npm run typecheck`, `npm test`; visually the bar advances smoothly.

### A3 (LOW PRIORITY / VERIFY-DON'T-ASSUME) — Trim device limits to what's used
- **File/symbol:** `mod.rs` `acquire_web` (~1166-1187).
- **What:** Currently raises 4 limits to adapter max. On WebGPU the browser
  validates requested limits against the device; requesting max is generally
  cheap (it does **not** force a slower driver path the way some native backends
  might). Likely a **non-issue**. Only act if hardware profiling shows
  `request_device` itself is slow. If so, request only the concrete limits the
  buffers need (compute storage buffer size for N=20k). 
- **Payoff:** Probably ~0. Listed for completeness so the implementer doesn't
  chase it.
- **Risk:** Low to change, but could under-provision a limit and break large N.
  Leave alone unless profiled.

### A4 (MEDIUM CONFIDENCE) — Warm the worker WASM instance earlier
- **File/symbol:** `web/src/gpu-build/network-build-worker.ts` (lazy `init()`),
  `network-build-client.ts` constructor (~23-35), `main.ts` `boot()` (the worker
  is constructed inside `startGpuBackend` via `new NetworkBuildClient()` at
  ~399, and the first `postMessage` triggers the worker's `await init()`).
- **What:** The worker downloads + instantiates its **own** copy of the WASM
  module on first message. That instantiation is serialized *before* payload
  generation can start, partially eating into the parallel window with the GPU
  handshake. Construct `NetworkBuildClient` and post a cheap warm-up/no-op (or the
  real startup prepare) as early as possible in `boot()` — ideally right after
  `await init()` of the main module (line 289), before canvas/renderer setup — so
  the worker's WASM instantiate overlaps the main-thread renderer init and the
  GPU handshake. The main module's WASM bytes are already in the browser cache by
  then, so the worker's fetch is a cache hit.
- **Payoff:** Small-to-medium; shaves the worker-init tail so "Prepare network
  payload" rarely blocks.
- **Risk:** Low. Must keep the existing latest-wins sequence discipline; the
  startup request already has its own sequence so no contract change.
- **Verify:** `npm test` (network-build-client tests); on hardware, the "Prepare
  network payload" stage should be near-instant (already ready by the time it's
  awaited).

### A5 (NOTE, not an action) — `await` ordering is already good
The adapter+device handshake (pure async waiting) already overlaps worker payload
generation. Do not "fix" this; just preserve the ordering when editing
`startGpuBackend`.

**Lead recommendation:** ship **A1 + A2** together (they are the real wins and
are naturally coupled to Workstream B's finer stages), then A4. Skip A3 unless
hardware profiling implicates `request_device`.

---

## Workstream B — Sub-stage progress channel (Rust → WASM → TS)

### Channel design
Add a JS callback that Rust calls to emit `(label: string, fraction: f32)` where
`fraction` is 0..1 progress *within the current stage*.

- **Rust side (`crates/brain-visualizer/src/lib.rs`):** Add a setter on
  `WasmGpuBackend` (and accept an optional callback on `create_staged`):
  ```rust
  // store: progress_cb: Option<js_sys::Function>
  pub fn set_progress_callback(&mut self, cb: js_sys::Function);
  ```
  Emit via `cb.call2(&JsValue::NULL, &JsValue::from_str(label), &JsValue::from_f64(frac))`.
  Because `create_staged` returns before the heavy stages, the cleanest path is:
  the **acquire** sub-progress (adapter/device/surface) is emitted from a callback
  passed *into* `create_staged` (since it owns `acquire_web`), and the
  **compile** sub-progress is emitted from `set_progress_callback` set on the
  returned backend before calling `startup_build_*`. `acquire_web`
  (`mod.rs:1126`) takes an optional `&js_sys::Function` and emits
  `"Requesting GPU adapter…" @ 0.1`, `"Requesting GPU device…" @ 0.4`,
  `"Configuring surface…" @ 0.7`. `build_sim`/`build_render` emit
  `"Compiling compute shaders…"` / `"Compiling render shaders…"` with fraction
  stepping per pipeline (n/total). Keep emits coarse (one per logical group) —
  per-pipeline microsteps don't repaint anyway since the call is synchronous.
- **Native build:** gate the callback plumbing behind `#[cfg(target_arch =
  "wasm32")]` or accept `Option` and skip on native so `cargo test` / examples
  (which call `acquire_native`/`new`) compile unchanged.
- **TS side (`main.ts`):** Define
  `onSubStage(label: string, fraction: number)` that maps the in-stage fraction
  onto the current stage's progress budget and calls `updateStartupOverlay({
  stage: label, progress: stageStart + fraction*(stageEnd-stageStart) })`. Pass it
  to `create_staged` and via `backend.set_progress_callback(...)` after creation.
  Because the heavy Rust call is synchronous, also see A2: to actually repaint,
  split the Rust-exposed compile into ≥2 awaited stages so the browser gets rAF
  gaps; the sub-stage labels then read as "Requesting GPU device…",
  "Compiling compute shaders…", "Compiling render shaders…".

### New stage weighting (replace the equal-10-slices scheme)
Current: `progressStart=54`, `progressEnd=96`, each of 10 stages = 4.2%
(`main.ts:592,606`). Replace the uniform `(index/stages.length)` math with a
per-stage weight table so the GPU acquire+compile stage owns the majority:

| Stage | Weight |
|---|---|
| Acquire GPU + core pipelines | **0.45** |
| Prepare network payload | 0.05 (usually already ready) |
| Stage prepared payload | 0.03 |
| Upload neuron buffers | 0.05 |
| Upload render mesh | 0.05 |
| Finalize render allocation | 0.02 |
| Generate morphology (buffer upload) | 0.05 |
| Bind network resources | 0.03 |
| Compile render pipelines | **0.20** |
| Create render targets | 0.07 |

Compute `beforeProgress`/`afterProgress` from cumulative weights × (96−54)+54
instead of `index/length`. Within the two heavy stages, the B-channel fraction
fills the gap so the bar moves continuously instead of freezing then snapping.
(Also relabel "Generate morphology" → "Upload morphology buffers" for honesty.)

### Exact emit points
- `acquire_web` (`mod.rs`): after `request_adapter` resolves, after
  `request_device` resolves, after `surface.configure`.
- `build_sim` (`pipelines.rs:127`): one emit before the compute group.
- `build_render` core (`pipelines.rs:233`, after A1 split): one emit before the
  render group.

---

## Workstream C — Panel UI (strict 3-row layout)

### New markup (`web/index.html`, replace the `#startup-panel` block ~164-178)
```html
<div id="startup-panel">
  <p id="startup-title">Brain Visualizer</p>
  <div id="startup-progress-track" aria-hidden="true">
    <div id="startup-progress-bar"></div>
  </div>
  <div id="startup-meta">
    <span id="startup-percent">0%</span>
    <span id="startup-stage">Starting…</span>
  </div>
</div>
```
**Delete elements:** `#startup-detail`, `#startup-steps` (the `step X/Y` span +
its wrapper), `#startup-elapsed` (`time`), `#startup-frames` (`frames`), and the
entire `<ul id="startup-timings">`.

### CSS deltas (the `<style>` block, ~13-139)
- Keep: `#startup-overlay` (+ `.ready` fade, `.failed`), `#startup-panel`
  (frosted), `#startup-title`, `#startup-progress-track`,
  `#startup-progress-bar` (turquoise→gold gradient `#4fd0c8 → #e7c06a`),
  `#startup-overlay.failed #startup-progress-bar`.
- **Delete:** `#startup-stage` (old multi-line styling), `#startup-detail`,
  `#startup-timings` + `#startup-timings li` + `:first-child` rules.
- **Change `#startup-meta`:** from `grid-template-columns: repeat(4, …)` to a
  two-item flex/grid with **percent left, stage right**:
  ```css
  #startup-meta {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    color: rgba(246, 242, 234, 0.56);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
  }
  #startup-meta #startup-stage {
    color: rgba(246, 242, 234, 0.78);
    text-align: right;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  ```
  (Stage now lives inside `#startup-meta` as the right cell; drop its old
  block-paragraph rule.)

### `updateStartupOverlay()` simplification (`main.ts:191-250`)
- **Drop from the `update` param type and the function body:** `detail`,
  `frames`, `stageIndex`, `totalStages`, `timings`, `backendMs`. Remove the DOM
  lookups + writes for `#startup-detail`, `#startup-frames`, `#startup-elapsed`,
  `#startup-steps`, `#startup-timings`. Keep `#startup-stage`, `#startup-percent`,
  `#startup-progress-bar`, and the overlay `.ready`/`.failed` toggles.
- **`StartupState` interface (129-141):** trim to `status`, `stage`, `progress`
  (+ keep `startedAtMs`/`elapsedMs` only if the test hook `__bvStartup` still
  needs them; the E2E hooks read `__bvStartup` and `__bvFrameCounter`). Verify
  what `web/tests` assert on `__bvStartup` before deleting fields (grep first).
- **`publishFrameCounter` (252-260):** keep updating `__bvFrameCounter`
  (tests/loop-alive probe rely on it) but **remove** the `#startup-frames` DOM
  write (element no longer exists).
- **`runStage` (591-617) and all `updateStartupOverlay` call sites:** remove
  `detail`, `stageIndex`, `totalStages`, `timings`, `backendMs` args. Replace the
  `timings` accumulation with the B-channel weighting. The `console.log` per-stage
  timing can stay (console only).

---

## Sequencing & file ownership

Shared hot files: **`main.ts`** (A2, A4, B, C all touch it) and
**`gpu/mod.rs`** (A1, A3, B) and **`pipelines.rs`** (A1, B). Sequence to avoid
churn:

1. **Workstream C first, alone** — pure UI/markup/`updateStartupOverlay`
   simplification. No Rust. Lands the new DOM ids that B will drive. Touches
   `index.html` + `main.ts`. (After this, grep tests for removed `__bvStartup`
   fields.)
2. **Workstream A1** — Rust-only split of `build_render` in `pipelines.rs` +
   `mod.rs` + the deferred-build call in `main.ts`'s rAF loop. Independent of B.
3. **Workstream B** — Rust callback plumbing (`lib.rs`, `mod.rs`,
   `pipelines.rs`) + `main.ts` weighting/sub-stage handler. Depends on C (new
   DOM) and benefits from A1's finer stage split.
4. **Workstream A2** — finalize the awaited-stage split + yields in `main.ts`;
   naturally completes alongside B.
5. **Workstream A4** — small `main.ts`/worker-warm change, independent; do last.

**Can run in parallel:** C (TS/HTML) and A1 (Rust) by two agents. **Must be
sequential:** B after C; A2 after B. A3 is optional and isolated.

### Cross-language contracts
No workstream changes `MorphSegment`, `MorphUniforms`, or the `VisualSettings`
Float32Array index — the B channel is a *new* one-way `(label, fraction)`
callback, additive. **Flag:** if B adds a param to `create_staged`, update the
`StagedGpuBackendConstructor` TS interface (`main.ts:159-176`) in lockstep, and
keep the `create()` fallback path (old pkg) working — the callback must be
`Option`/optional so a regenerated-vs-stale `.d.ts` mismatch can't break boot.

---

## Verification plan
- From `app/`: `cargo test` (covers pipeline-build unit tests in `pipelines.rs`,
  staged-startup, `from_flat_payload`). A1's `render_full` bloom-guard is the main
  correctness risk — ensure no `unwrap` on a not-yet-built pipeline.
- From `app/web/`: `npm run typecheck` (the `StagedGpuBackend*` interfaces and
  `updateStartupOverlay` signature changes must stay sound) and `npm test`
  (network-build-client tests for A4; any `__bvStartup` assertions for C).
- **No GPU on this box** — wall-clock can't be measured here. Build the wasm
  pkg (`wasm-pack`/the repo's build step) and run `cargo test` + the web gates;
  the user validates felt speed and bloom-on-first-frame on real hardware.

## Docs to update (at ship time)
- `architecture/web-frontend.md` — new 3-row startup overlay structure +
  `updateStartupOverlay` field set + the sub-stage progress weighting.
- `architecture/gpu-backend.md` — `build_render` core/deferred split (A1),
  `acquire_web`/`create_staged` progress-callback param, staged-stage list and
  weights, the relabeled "upload morphology buffers" stage.
- `architecture/dev-panel.md` — only if the removed diagnostics (`step`, `time`,
  `frames`, timings list) were documented there as a dev surface.
- `decisions/rendering.md` — record the "compile bloom/active pipelines lazily
  after first frame" decision (A1) if it constitutes a rationale shift.
- `decisions/dev-tooling.md` — record dropping the per-stage timings/diagnostics
  from the boot panel.
- `architecture/build-and-deploy.md` — only if any build config changes
  (expected: none).

## Open risks / decisions
1. **Bloom-first-frame (A1):** RESOLVED by user/orchestrator (2026-06-12) —
   deferring bloom by ~1 frame (≈16 ms) is acceptable (imperceptible). Proceed
   with the defer-bloom default. Fallback only if the implementer finds the gap
   is actually multiple frames or causes a visible flash: compile bloom in the
   "Compile render pipelines" stage but still defer the `*_active` variants.
2. **Async pipeline creation:** confirmed NOT available in wgpu 29's safe API;
   we are *not* attempting raw-`web_sys` pipeline plumbing. If the user
   specifically wants true parallel browser shader compilation, that's a separate
   larger spike (drop below wgpu) — flag and defer.
