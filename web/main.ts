// Entry point: load the WASM module, generate the manifold, wire controls /
// camera / renderer, and run the rAF + tick loop (phase-1 doc §"rAF loop").
// Ticks are stubbed (backend.tick returns zeros) so the profiler dumps zeros
// for sim counters; FPS / frame-time are real.

import init, {
  init_manifold,
  log_cross_origin_isolation,
} from "../pkg/brain_visualizer.js";
import { Camera } from "./camera";
import { Controls, ticksThisFrame } from "./controls";
import { Profiler } from "./profiler";
import { Renderer } from "./renderer";
import { DEFAULT_CONFIG, ZERO_STATS, type AppConfig } from "./types";

async function boot(): Promise<void> {
  // 1. Load WASM (also installs the panic hook via the #[wasm_bindgen(start)]).
  await init();

  // 2. COOP/COEP / SharedArrayBuffer check (phase-1 startup log).
  const isolated = (globalThis as { crossOriginIsolated?: boolean })
    .crossOriginIsolated === true;
  log_cross_origin_isolation(isolated);

  const config: AppConfig = { ...DEFAULT_CONFIG };

  // 3. Generate the manifold (real geometry; nothing drawn yet).
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
  canvas.addEventListener("pointerdown", (e) =>
    camera.onPointerDown(e.clientX, e.clientY),
  );
  window.addEventListener("pointerup", () => camera.onPointerUp());
  canvas.addEventListener("pointermove", (e) =>
    camera.onPointerMove(e.clientX, e.clientY),
  );
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      camera.onWheel(e.deltaY);
    },
    { passive: false },
  );
  window.addEventListener("resize", () => {
    resizeCanvas(canvas);
    camera.setAspect(canvas.width / canvas.height);
  });

  // 6. Profiler.
  const profiler = new Profiler(
    config.backend,
    config.tier,
    config.n,
    config.k,
  );

  // 7. Controls (backend/tier restart is a stub log in phase 1).
  const controls = new Controls(config, (cfg) => {
    console.log(`[main] restart requested (stub): ${JSON.stringify(cfg)}`);
    profiler.setConfig(cfg.backend, cfg.tier, cfg.n, cfg.k);
  });
  // Expose for console poking (phase-1 doc: callable from console).
  (window as unknown as { brainControls: Controls }).brainControls = controls;

  // 8. rAF + tick loop.
  let frameCounter = 0;
  let lastTimestamp = performance.now();

  function rafLoop(timestamp: DOMHighResTimeStamp): void {
    const ticks = ticksThisFrame(config.speed, frameCounter);
    // Backend tick is stubbed in phase 1 → zeroed stats.
    void ticks;
    void config.excitability;
    const stats = ZERO_STATS;

    profiler.recordFrame(timestamp, timestamp - lastTimestamp, stats);
    profiler.maybeDump(timestamp);

    // camera.viewProjection() will feed the real renderer in phase 3.
    void camera.viewProjection();
    renderer.render();

    frameCounter++;
    lastTimestamp = timestamp;
    requestAnimationFrame(rafLoop);
  }
  requestAnimationFrame(rafLoop);

  console.log("[main] rAF loop started (render = clear-only stub)");
}

function resizeCanvas(canvas: HTMLCanvasElement): void {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(canvas.clientWidth * dpr) || 800;
  canvas.height = Math.floor(canvas.clientHeight * dpr) || 600;
}

boot().catch((e) => console.error("[main] boot failed:", e));
