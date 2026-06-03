# Phase 5 — Controls & Brain States UI

_All controls that were stubs become real UI. The visitor can interact with
the simulation without opening the console._

## Done when
- Brain states button group is visible and functional; switching states
  visibly changes activity within ~1–2 seconds.
- Speed controls (¼×/½×/1×/2×) work; slow motion makes individual
  wavefronts visible.
- Backend toggle UI is visible. GPU is active; CPU is visibly disabled until
  phase 6. Selecting an implemented backend triggers a full restart and the same
  network reappears.
- Excitability slider (if exposed separately) updates live.
- All controls work on mobile (touch events, appropriately sized tap targets).

## UI layout

```
┌─────────────────────────────────────────────────────────────┐
│  [¼×] [½×] [1×] [2×]          [GPU | CPU]  (top bar)       │
│                                                              │
│                                                              │
│              (brain — full bleed)                            │
│                                                              │
│                                                              │
│  [deep sleep][relaxed][focused][hyper][seizure]  (bottom)   │
└─────────────────────────────────────────────────────────────┘
```

- Top-left: speed controls (button group, single-select).
- Top-right: backend toggle (two-state: GPU / CPU). In phase 5, CPU is disabled
  with a tooltip/label such as "CPU backend lands in phase 6".
- Bottom-center: brain state button group (single-select, `focused` default).
- Everything else is the brain, full bleed.
- No other chrome.

## HTML / CSS

```html
<body>
  <div id="controls-top">
    <div id="speed-group" class="btn-group">
      <button data-speed="quarter">¼×</button>
      <button data-speed="half">½×</button>
      <button data-speed="normal" class="active">1×</button>
      <button data-speed="double">2×</button>
    </div>
    <div id="backend-toggle" class="btn-group">
      <button data-backend="gpu" class="active">GPU</button>
      <button data-backend="cpu">CPU</button>
    </div>
  </div>

  <canvas id="canvas"></canvas>

  <div id="controls-bottom">
    <div id="brain-state-group" class="btn-group">
      <button data-state="deep_sleep">deep sleep</button>
      <button data-state="relaxed">relaxed</button>
      <button data-state="focused" class="active">focused</button>
      <button data-state="hyperstimulated">hyper</button>
      <button data-state="seizure">seizure</button>
    </div>
  </div>
</body>
```

CSS approach: controls use `position: fixed` so the canvas is always
full-bleed underneath. Button groups: pill shape, dark semi-transparent
background (`rgba(0,0,0,0.4)`), light text, active state highlighted with
a region-color-inspired accent. No borders on the canvas element.

Minimum touch target: 44×44 CSS pixels (iOS HIG). On mobile, stack the
brain state buttons 2–3 per row if they don't fit on one line.

## `web/controls.ts` — full implementation

```typescript
export const BRAIN_STATES: Record<BrainState, number> = {
  deep_sleep:      0.10,
  relaxed:         0.30,
  focused:         0.55,
  hyperstimulated: 0.80,
  seizure:         1.00,
};

// Smooth transition: lerp excitability toward target over ~30 frames
// to avoid jarring instant jump (especially deep_sleep → seizure)
let targetExcitability = BRAIN_STATES.focused;
let currentExcitability = BRAIN_STATES.focused;
const EXCITABILITY_LERP = 0.08;

export function tickExcitability(): number {
  currentExcitability += (targetExcitability - currentExcitability) * EXCITABILITY_LERP;
  return currentExcitability;
}

export function setBrainState(state: BrainState): void {
  targetExcitability = BRAIN_STATES[state];
  document.querySelectorAll('#brain-state-group button').forEach(b =>
    b.classList.toggle('active', (b as HTMLElement).dataset.state === state)
  );
}

export function setSpeed(preset: SpeedPreset): void {
  config.speed = preset;
  document.querySelectorAll('#speed-group button').forEach(b =>
    b.classList.toggle('active', (b as HTMLElement).dataset.speed === preset)
  );
}

export function setBackend(kind: BackendKind): void {
  if (!backendAvailable(kind)) {
    showToast(`${kind.toUpperCase()} backend is not available yet`);
    return;
  }
  // Full restart: teardown → reinitialize with same seed
  config.backend = kind;
  backend.destroy();
  backend = createBackend(config);   // same seed → same network
  document.querySelectorAll('#backend-toggle button').forEach(b =>
    b.classList.toggle('active', (b as HTMLElement).dataset.backend === kind)
  );
}
```

Call `tickExcitability()` each frame in the rAF loop and pass the result to
`backend.tick()`. This gives smooth transitions between brain states.

## Event wiring (in `web/main.ts`)

```typescript
document.querySelectorAll('#brain-state-group button').forEach(btn =>
  btn.addEventListener('click', () =>
    setBrainState((btn as HTMLElement).dataset.state as BrainState))
);

document.querySelectorAll('#speed-group button').forEach(btn =>
  btn.addEventListener('click', () =>
    setSpeed((btn as HTMLElement).dataset.speed as SpeedPreset))
);

document.querySelectorAll('#backend-toggle button').forEach(btn =>
  btn.addEventListener('click', () =>
    setBackend((btn as HTMLElement).dataset.backend as BackendKind))
);
```

## Backend restart sequence (BV16)

```typescript
async function restartWithBackend(kind: BackendKind): Promise<void> {
  // 1. Halt rAF loop
  cancelAnimationFrame(rafHandle);

  // 2. Destroy current backend (releases GPU buffers / terminates workers)
  await backend.destroy();

  // 3. Reinitialize with same config and seed
  config.backend = kind;
  backend = await createBackend(config);  // same config.seed

  // 4. Restart loop
  rafHandle = requestAnimationFrame(rafLoop);
}
```

The restart is fast enough (< 500ms for GPU init) that no loading spinner
is needed, but the canvas will be black briefly — acceptable.

## Mobile considerations
- Touch `touchstart`/`touchmove`: single finger → orbit (same as left drag),
  two-finger pinch → zoom.
- Stimulation on mobile: single finger hover is impossible. Use a gentle
  ambient stimulation on the region closest to the center of the screen,
  or skip cursor stimulation on mobile entirely and rely on ambient I_ext.
  Decision: **skip cursor stimulation on mobile**; the brain is still active
  via ambient drive.
- Viewport: `<meta name="viewport" content="width=device-width, initial-scale=1">`.
  Canvas resizes to `window.innerWidth × window.innerHeight` on resize event.
- Tier: mobile defaults to Low tier regardless of what the scaler suggests
  (single backend, no rayon, WebGPU only).

## Adaptive scaler (activate in this phase)
In phase 1 the scaler was a stub. In phase 5 activate it:
```typescript
// After each profiler dump (1/sec):
const frameBudgetMs = 14;  // 60fps with 2ms headroom
const canResize = performance.now() - lastResizeMs > 3000;

if (canResize && profiler.frameP95 > frameBudgetMs && config.n > N_MIN[config.tier]) {
  config.n = Math.max(N_MIN[config.tier], Math.floor(config.n * 0.9));
  lastResizeMs = performance.now();
  backend.resize(config);
}

if (canResize && profiler.frameP95 < frameBudgetMs * 0.7 && config.n < N_MAX[config.tier]) {
  config.n = Math.min(N_MAX[config.tier], Math.floor(config.n * 1.1));
  lastResizeMs = performance.now();
  backend.resize(config);
}
```
Grow only when comfortable headroom; shrink quickly when over budget.

Scaler rules:
- resize at most once every 3 seconds;
- never resize while a backend restart or device-loss recovery is in progress;
- shrink on p95 over budget, grow only after sustained p95 headroom;
- allow render-resolution scale to shrink before reducing `N` on visually rich
  tiers;
- treat near LOD, HDR/bloom, sound, debug overlays, and GPU timings as optional
  cost centers that can be disabled before reducing the core simulation.

Controls should call public backend/config methods only. They must not recreate
GPU resources directly; resize/restart/render-mode changes flow through the
resource lifecycle described in phase 1.

The scaler is not the deferred auto-tier heuristic. It may resize `N`, `K`, and
render resolution within the currently selected Low/Balanced/Max tier, but it
must not silently change the selected tier. The visitor's manual tier choice
remains the visible mode; auto-picking a tier on page load is future work.
