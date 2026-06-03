// Entry point (Phase 5): WASM load, manifold generation, controls event wiring,
// rAF + tick loop with excitability lerp, LOD plumbing, adaptive scaler.
// Phase 3/4 render and stim paths are preserved; GPU bridge remains browser TODO.

import init, {
  init_manifold,
  log_cross_origin_isolation,
} from "../pkg/brain_visualizer.js";
import { Camera } from "./camera";
import {
  BRAIN_STATES,
  Controls,
  isMobile,
  scalerDecide,
  setBrainState,
  setSpeed,
  showToast,
  tickExcitability,
  ticksThisFrame,
} from "./controls";
import { Profiler } from "./profiler";
import { Renderer } from "./renderer";
import { DEFAULT_CONFIG, ZERO_STATS, type AppConfig, type BackendKind, type BrainState, type SpeedPreset } from "./types";

// Cursor stimulation constants (BV10 / phase-3 spec).
const STIM_RADIUS  = 0.15;  // world units
const STIM_CURRENT = 0.3;   // biological mV → fixed-point in backend

// Manifold bounding sphere radius (neurons live at ~r=1.0 on the folded
// surface; add a margin for gyrification deformation).
const MANIFOLD_SPHERE_RADIUS = 1.4;

async function boot(): Promise<void> {
  // 1. Load WASM.
  await init();

  // 2. COOP/COEP check.
  const isolated = (globalThis as { crossOriginIsolated?: boolean })
    .crossOriginIsolated === true;
  log_cross_origin_isolation(isolated);

  // 3. Mobile detection — default Low tier on mobile (Phase 5 / BV).
  const mobile = isMobile();
  const config: AppConfig = { ...DEFAULT_CONFIG };
  if (mobile) {
    config.tier = "low";
    console.log("[main] mobile detected → Low tier default");
  }

  // 4. Generate manifold.
  const neuronCount = init_manifold(config.n, config.seed >>> 0);
  console.log(`[main] manifold generated: ${neuronCount} neurons placed`);

  // 5. Canvas + renderer.
  const canvas = document.getElementById("brain-canvas") as HTMLCanvasElement;
  resizeCanvas(canvas);
  const renderer = new Renderer(canvas);
  await renderer.init();

  // 6. Camera + input.
  const camera = new Camera();
  camera.setAspect(canvas.width / canvas.height);

  canvas.addEventListener("pointerdown", (e) => {
    camera.onPointerDown(e.clientX, e.clientY);
  });
  window.addEventListener("pointerup", () => camera.onPointerUp());

  canvas.addEventListener("pointermove", (e) => {
    const isOrbit = camera.onPointerMove(e.clientX, e.clientY, e.buttons);
    // Skip cursor stimulation on mobile (BV10 amendment / Phase 5).
    if (!isOrbit && !mobile) {
      handleStimulate(e, canvas, camera);
    }
  });

  canvas.addEventListener(
    "wheel",
    (e) => { e.preventDefault(); camera.onWheel(e.deltaY); },
    { passive: false },
  );

  // Touch events: one-finger orbit, two-finger pinch zoom.
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

  // 7. Profiler.
  const profiler = new Profiler(config.backend, config.tier, config.n, config.k);

  // 8. Controls (facade kept for console access).
  const controls = new Controls(config, (cfg) => {
    console.log(`[main] restart requested (stub): ${JSON.stringify(cfg)}`);
    profiler.setConfig(cfg.backend, cfg.tier, cfg.n, cfg.k);
  });
  (window as unknown as { brainControls: Controls }).brainControls = controls;

  // 9. Wire DOM click handlers for all three button groups (Phase 5 task).
  document.querySelectorAll("#brain-state-group button").forEach((btn) => {
    btn.addEventListener("click", () => {
      const state = (btn as HTMLElement).dataset.state as BrainState;
      setBrainState(state);
      console.log(`[controls] brain state = ${state} (target excitability ${BRAIN_STATES[state]})`);
    });
  });

  document.querySelectorAll("#speed-group button").forEach((btn) => {
    btn.addEventListener("click", () => {
      const preset = (btn as HTMLElement).dataset.speed as SpeedPreset;
      setSpeed(preset, config);
      console.log(`[controls] speed = ${preset}`);
    });
  });

  document.querySelectorAll("#backend-toggle button").forEach((btn) => {
    btn.addEventListener("click", () => {
      const kind = (btn as HTMLElement).dataset.backend as BackendKind;
      // Disabled buttons (CPU in Phase 5) have the HTML `disabled` attribute;
      // belt-and-suspenders: also check availability in setBackend via toast path.
      if ((btn as HTMLButtonElement).disabled) {
        showToast("CPU backend is not available yet");
        return;
      }
      if (kind !== config.backend) {
        void restartWithBackend(kind);
      }
    });
  });

  // 10. Restart sequence state.
  let rafHandle = 0;
  let duringRestart = false;

  /**
   * BV16 restart sequence: cancel rAF, destroy backend, reinit same seed.
   * In Phase 5 only GPU is real; CPU shows disabled. The black-canvas gap
   * during teardown+reinit is acceptable per the spec.
   */
  async function restartWithBackend(kind: BackendKind): Promise<void> {
    if (duringRestart) return;
    duringRestart = true;
    cancelAnimationFrame(rafHandle);

    // Destroy current wasm backend (releases GPU buffers).
    // In Phase 5 the wasm GPU bridge is a browser TODO; when wired, this will
    // call wasmBackend.destroy() and re-create with same config.seed.
    console.log(`[main] restart → backend=${kind} seed=0x${config.seed.toString(16)}`);
    config.backend = kind;
    profiler.setConfig(kind, config.tier, config.n, config.k);

    // TODO (browser): destroy wasm GpuBackend, re-create with same seed.
    // backend = await WasmGpuBackend.create(config);

    duringRestart = false;
    rafHandle = requestAnimationFrame(rafLoop);
  }

  // 11. Adaptive scaler state.
  const SCALER_COOLDOWN = 3000;
  let lastResizeMs = -SCALER_COOLDOWN; // allow first check immediately

  // 12. rAF + tick loop.
  let frameCounter = 0;
  let lastTimestamp = performance.now();
  let tickCount = 0;

  function rafLoop(timestamp: DOMHighResTimeStamp): void {
    const ticks = ticksThisFrame(config.speed, frameCounter);

    // Smooth excitability lerp (Phase 5). Result passed to backend.tick().
    const excitability = tickExcitability();
    // TODO (browser): wasmBackend.tick(ticks, excitability)
    void ticks;
    void excitability;

    const stats = ZERO_STATS;
    tickCount += ticks;

    // Phase 4 LOD plumbing: update LOD distance before render each frame.
    // TODO (browser): wasmBackend.set_lod_camera_distance(camera.cameraDistance())
    // TODO (browser): wasmBackend.render_full(camera.eye(), camera.cameraDistance(), ...)
    void camera.cameraDistance();
    void camera.eye();

    profiler.recordFrame(timestamp, timestamp - lastTimestamp, stats);
    const dumped = profiler.maybeDump(timestamp);

    // Adaptive scaler: runs once per second (after each profiler dump).
    if (dumped) {
      const p95 = profiler.getFrameP95();
      const timeSinceResize = timestamp - lastResizeMs;
      const action = scalerDecide(p95, config.n, config.tier, timeSinceResize, duringRestart);

      if (action.kind === "shrink_n" || action.kind === "grow_n") {
        config.n = action.newN;
        lastResizeMs = timestamp;
        profiler.setConfig(config.backend, config.tier, config.n, config.k);
        console.log(`[scaler] ${action.kind}: n=${config.n} (tier=${config.tier} p95=${p95.toFixed(1)}ms)`);
        // TODO (browser): wasmBackend.resize(config)
      }
    }

    renderer.render(camera, tickCount);

    frameCounter++;
    lastTimestamp = timestamp;
    rafHandle = requestAnimationFrame(rafLoop);
  }

  rafHandle = requestAnimationFrame(rafLoop);

  console.log("[main] Phase 5 ready — controls wired, scaler active (GPU bridge = browser TODO)");
}

/**
 * Cursor stimulation: unproject pointer to world ray, intersect the manifold
 * bounding sphere, call backend.stimulate() at the hit point (BV10).
 * Skipped on mobile (Phase 5 decision: rely on ambient I_ext instead).
 */
function handleStimulate(
  e: PointerEvent,
  canvas: HTMLCanvasElement,
  camera: Camera,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  backend?: unknown,
): void {
  const rect = canvas.getBoundingClientRect();
  const cssX = e.clientX - rect.left;
  const cssY = e.clientY - rect.top;
  const { origin, dir } = camera.unproject(cssX, cssY, rect.width, rect.height);
  const hit = raySphereIntersect(origin, dir, [0, 0, 0], MANIFOLD_SPHERE_RADIUS);
  if (!hit) return;
  const b = backend as { stimulate?: (h: [number,number,number], r: number, c: number) => void } | undefined;
  if (b && typeof b.stimulate === "function") {
    b.stimulate(hit, STIM_RADIUS, STIM_CURRENT);
  }
}

/** Ray–sphere intersection. Returns nearest hit in world space or null. */
function raySphereIntersect(
  origin: [number, number, number],
  dir: [number, number, number],
  center: [number, number, number],
  radius: number,
): [number, number, number] | null {
  const ox = origin[0] - center[0], oy = origin[1] - center[1], oz = origin[2] - center[2];
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
  canvas.width  = Math.floor(canvas.clientWidth  * dpr) || 800;
  canvas.height = Math.floor(canvas.clientHeight * dpr) || 600;
}

boot().catch((e) => console.error("[main] boot failed:", e));
