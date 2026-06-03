// Entry point: load the WASM module, generate the manifold, wire controls /
// camera / renderer, and run the rAF + tick loop.
// Phase 3: renderer reads live sim state; cursor hover injects stimulation;
// camera orbit/zoom wired to pointer/wheel/touch events.

import init, {
  init_manifold,
  log_cross_origin_isolation,
} from "../pkg/brain_visualizer.js";
import { Camera } from "./camera";
import { Controls, ticksThisFrame } from "./controls";
import { Profiler } from "./profiler";
import { Renderer } from "./renderer";
import { DEFAULT_CONFIG, ZERO_STATS, type AppConfig } from "./types";

// Cursor stimulation constants (BV10 / phase-3 spec).
const STIM_RADIUS = 0.15;   // world units
const STIM_CURRENT = 0.3;   // biological mV, converted to fixed-point by backend

// Manifold bounding sphere radius (neurons lie at ~r=1.0 on the folded surface;
// add a margin for the gyrification deformation).
const MANIFOLD_SPHERE_RADIUS = 1.4;

async function boot(): Promise<void> {
  // 1. Load WASM.
  await init();

  // 2. COOP/COEP check.
  const isolated = (globalThis as { crossOriginIsolated?: boolean })
    .crossOriginIsolated === true;
  log_cross_origin_isolation(isolated);

  const config: AppConfig = { ...DEFAULT_CONFIG };

  // 3. Generate manifold.
  const neuronCount = init_manifold(config.n, config.seed >>> 0);
  console.log(`[main] manifold generated: ${neuronCount} neurons placed`);

  // 4. Canvas + renderer.
  const canvas = document.getElementById("brain-canvas") as HTMLCanvasElement;
  resizeCanvas(canvas);
  const renderer = new Renderer(canvas);
  await renderer.init();

  // 5. Camera + input.
  const camera = new Camera();
  camera.setAspect(canvas.width / canvas.height);

  canvas.addEventListener("pointerdown", (e) => {
    camera.onPointerDown(e.clientX, e.clientY);
  });
  window.addEventListener("pointerup", () => camera.onPointerUp());

  canvas.addEventListener("pointermove", (e) => {
    const isOrbit = camera.onPointerMove(e.clientX, e.clientY, e.buttons);
    if (!isOrbit) {
      // Hover (no button held) → cursor stimulation (BV10).
      handleStimulate(e, canvas, camera);
    }
  });

  canvas.addEventListener(
    "wheel",
    (e) => { e.preventDefault(); camera.onWheel(e.deltaY); },
    { passive: false },
  );

  // Touch events (one-finger orbit, two-finger pinch zoom).
  canvas.addEventListener("touchstart", (e) => {
    e.preventDefault();
    camera.onTouchStart(e.touches);
  }, { passive: false });
  canvas.addEventListener("touchmove", (e) => {
    e.preventDefault();
    camera.onTouchMove(e.touches);
  }, { passive: false });
  canvas.addEventListener("touchend", () => camera.onPointerUp());

  window.addEventListener("resize", () => {
    resizeCanvas(canvas);
    camera.setAspect(canvas.width / canvas.height);
  });

  // 6. Profiler.
  const profiler = new Profiler(config.backend, config.tier, config.n, config.k);

  // 7. Controls.
  const controls = new Controls(config, (cfg) => {
    console.log(`[main] restart requested (stub): ${JSON.stringify(cfg)}`);
    profiler.setConfig(cfg.backend, cfg.tier, cfg.n, cfg.k);
  });
  (window as unknown as { brainControls: Controls }).brainControls = controls;

  // 8. rAF + tick loop.
  let frameCounter = 0;
  let lastTimestamp = performance.now();
  let tickCount = 0;

  function rafLoop(timestamp: DOMHighResTimeStamp): void {
    const ticks = ticksThisFrame(config.speed, frameCounter);
    // Phase 3: GPU sim ticks are currently driven from native side only;
    // the WASM path stubs tick() → zero stats (full wasm sim wiring is phase 6).
    void ticks;
    void config.excitability;
    const stats = ZERO_STATS;

    tickCount += ticks;
    profiler.recordFrame(timestamp, timestamp - lastTimestamp, stats);
    profiler.maybeDump(timestamp);

    // Phase 3: render with camera MVP. The wasm backend's render() is stubbed
    // (no GPU device from browser yet — GPU device wiring is a browser-only
    // manual TODO). The renderer shows clear-black until the wasm GpuBackend
    // is wired to the browser's WebGPU context in a future phase.
    renderer.render(camera, tickCount);

    frameCounter++;
    lastTimestamp = timestamp;
    requestAnimationFrame(rafLoop);
  }
  requestAnimationFrame(rafLoop);

  console.log("[main] rAF loop started (phase-3 render wired; GPU backend wasm bridge = browser TODO)");
}

/**
 * Cursor stimulation: unproject pointer to world ray, intersect the manifold
 * bounding sphere, call backend.stimulate() at the hit point (BV10).
 *
 * `backend` is the wasm GpuBackend (once the wasm GPU bridge is wired —
 * currently a placeholder for the browser-integration phase).
 */
function handleStimulate(
  e: PointerEvent,
  canvas: HTMLCanvasElement,
  camera: Camera,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  backend?: any,
): void {
  // CSS pixel dimensions for unprojection.
  const rect = canvas.getBoundingClientRect();
  const cssX = e.clientX - rect.left;
  const cssY = e.clientY - rect.top;
  const cssW = rect.width;
  const cssH = rect.height;

  const { origin, dir } = camera.unproject(cssX, cssY, cssW, cssH);

  // Ray–sphere intersection: manifold bounding sphere at origin, r=MANIFOLD_SPHERE_RADIUS.
  const hit = raySphereIntersect(origin, dir, [0, 0, 0], MANIFOLD_SPHERE_RADIUS);
  if (!hit) return;

  // Call stimulate if the backend is available.
  if (backend && typeof backend.stimulate === "function") {
    backend.stimulate(hit, STIM_RADIUS, STIM_CURRENT);
  }
}

/**
 * Ray–sphere intersection. Returns the nearest hit point in world space,
 * or null if the ray misses.
 */
function raySphereIntersect(
  origin: [number, number, number],
  dir: [number, number, number],
  center: [number, number, number],
  radius: number,
): [number, number, number] | null {
  const ox = origin[0] - center[0];
  const oy = origin[1] - center[1];
  const oz = origin[2] - center[2];
  const a = dir[0]*dir[0] + dir[1]*dir[1] + dir[2]*dir[2];
  const b = 2 * (ox*dir[0] + oy*dir[1] + oz*dir[2]);
  const c = ox*ox + oy*oy + oz*oz - radius*radius;
  const disc = b*b - 4*a*c;
  if (disc < 0) return null;
  const t = (-b - Math.sqrt(disc)) / (2*a);
  if (t < 0) return null;
  return [origin[0]+dir[0]*t, origin[1]+dir[1]*t, origin[2]+dir[2]*t];
}

function resizeCanvas(canvas: HTMLCanvasElement): void {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(canvas.clientWidth * dpr) || 800;
  canvas.height = Math.floor(canvas.clientHeight * dpr) || 600;
}

boot().catch((e) => console.error("[main] boot failed:", e));
