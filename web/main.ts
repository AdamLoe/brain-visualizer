// Entry point (Phase 7): WASM load, manifold generation, controls event wiring,
// rAF + tick loop with excitability lerp, LOD plumbing, adaptive scaler.
// Phase 7 adds: sonification engine, corner HUD, mobile scaling, 10M disclaimer.
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
  setActiveButton,
  setBrainState,
  setSpeed,
  showToast,
  tickExcitability,
  ticksThisFrame,
} from "./controls";
import { CpuRenderer } from "./cpu-renderer";
import { CornerHud } from "./hud";
import { Profiler } from "./profiler";
import { Renderer } from "./renderer";
import { SonificationEngine, deriveRegionFractions } from "./sonification";
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

  // 3. Mobile detection — apply full mobile profile (Phase 7 / BV spec):
  //    Low tier, GPU only, 0.75×DPR render res, no near-LOD, no sound, no stim.
  const mobile = isMobile();
  const config: AppConfig = { ...DEFAULT_CONFIG };
  if (mobile) {
    config.tier = "low";
    config.n    = 50_000;  // Low tier N (N≈50k / K=16 per spec)
    config.k    = 16;
    config.backend = "gpu"; // GPU only on mobile (no rayon workers overhead)
    console.log("[main] mobile detected → Low tier (N=50k K=16, GPU only, 0.75×DPR, no sound)");
  }

  // 4. Generate manifold.
  const neuronCount = init_manifold(config.n, config.seed >>> 0);
  console.log(`[main] manifold generated: ${neuronCount} neurons placed`);

  // 5. Canvas + renderer.
  const canvas = document.getElementById("brain-canvas") as HTMLCanvasElement;
  // Mobile: render at 0.75× DPR (Phase 7 mobile profile).
  resizeCanvas(canvas, mobile ? 0.75 : 1.0);
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
    resizeCanvas(canvas, mobile ? 0.75 : 1.0);
    camera.setAspect(canvas.width / canvas.height);
  });

  // 7. Profiler.
  const profiler = new Profiler(config.backend, config.tier, config.n, config.k);

  // 7b. Corner HUD (BV8 amendment — Phase 7). Bottom-right, updated 1/sec.
  //     Hidden on mobile (no debug overlays per spec).
  const hud = new CornerHud(false /* debug fields off by default */);
  if (mobile) hud.hide();

  // 7c. Sonification engine (BV11 — Phase 7). Disabled on mobile.
  const sonification = new SonificationEngine();

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
      // Phase 6: both backends are selectable. Belt-and-suspenders disabled
      // guard kept in case a build re-disables a button.
      if ((btn as HTMLButtonElement).disabled) {
        showToast(`${kind.toUpperCase()} backend is not available`);
        return;
      }
      if (kind !== config.backend) {
        setActiveButton("#backend-toggle", "backend", kind);
        void restartWithBackend(kind);
      }
    });
  });

  // Tier selector (BV3 / BV23 — Phase 7). Manual switch; auto-pick deferred.
  document.querySelectorAll("#tier-group button").forEach((btn) => {
    btn.addEventListener("click", () => {
      const tier = (btn as HTMLElement).dataset.tier as import("./types").Tier;
      if (tier !== config.tier) {
        config.tier = tier;
        setActiveButton("#tier-group", "tier", tier);
        console.log(`[controls] tier = ${tier} → restart`);
        void restartWithBackend(config.backend);
      }
    });
  });

  // Sound toggle (BV11 — Phase 7). Button is in the top bar.
  // Disabled on mobile (audio context is flaky on mobile per spec).
  const soundBtn = document.getElementById("sound-toggle");
  if (soundBtn && !mobile) {
    soundBtn.addEventListener("click", () => {
      if (sonification.enabled) {
        sonification.disable();
        soundBtn.textContent = "🔇";
        soundBtn.title = "Enable sound";
      } else {
        sonification.enable();
        soundBtn.textContent = "🔊";
        soundBtn.title = "Disable sound";
      }
    });
  } else if (soundBtn && mobile) {
    // Hide sound toggle on mobile.
    (soundBtn as HTMLElement).style.display = "none";
  }

  // 10. Restart sequence state.
  let rafHandle = 0;
  let duringRestart = false;

  // Phase 6 CPU backend coordinator (BV24). Owns the worker + the SoA views the
  // WebGL2 CpuRenderer draws. Tuning matches examples/cpu_check.rs / sim_check.rs.
  const CPU_I_EXT = 0.040;
  const CPU_SYN_SCALE = 0.03;
  const cpu = new CpuCoordinator(canvas, CPU_I_EXT, CPU_SYN_SCALE);

  /**
   * BV16 restart sequence: cancel rAF, tear down the current backend, reinit the
   * other one with the SAME seed (identical network). The black-canvas gap
   * during teardown+reinit is acceptable per the spec.
   *
   * CPU path is real here (spawns the coordinator worker + WebGL2 renderer); the
   * GPU wasm bridge remains a browser TODO (Phase 3 OD11).
   */
  async function restartWithBackend(kind: BackendKind): Promise<void> {
    if (duringRestart) return;
    duringRestart = true;
    cancelAnimationFrame(rafHandle);

    console.log(`[main] restart → backend=${kind} seed=0x${config.seed.toString(16)}`);
    const prev = config.backend;
    config.backend = kind;
    profiler.setConfig(kind, config.tier, config.n, config.k);

    if (prev === "cpu") cpu.destroy();
    // TODO (browser): if prev === "gpu", destroy wasm GpuBackend.

    if (kind === "cpu") {
      try {
        await cpu.start(config);
      } catch (e) {
        console.warn("[main] CPU backend start failed, reverting to GPU:", e);
        showToast("CPU backend failed to start");
        config.backend = "gpu";
        setActiveButton("#backend-toggle", "backend", "gpu");
      }
    }
    // TODO (browser): if kind === "gpu", re-create wasm GpuBackend (same seed).

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

    // Smooth excitability lerp (Phase 5). Passed to the backend's tick().
    const excitability = tickExcitability();
    tickCount += ticks;

    let stats = ZERO_STATS;

    if (config.backend === "cpu") {
      // CPU path: drive the coordinator worker + WebGL2 render (Phase 6).
      if (ticks > 0) cpu.tick(ticks, excitability);
      stats = cpu.lastStats();
      cpu.render(camera);
    } else {
      // GPU path: tick + render via the wasm GpuBackend (browser TODO bridge).
      // TODO (browser): wasmBackend.tick(ticks, excitability)
      // TODO (browser): wasmBackend.set_lod_camera_distance(camera.cameraDistance())
      // TODO (browser): wasmBackend.render_full(camera.eye(), camera.cameraDistance(), ...)
      void ticks;
      void excitability;
      void camera.cameraDistance();
      void camera.eye();
      renderer.render(camera, tickCount);
    }

    profiler.recordFrame(timestamp, timestamp - lastTimestamp, stats);
    const dumped = profiler.maybeDump(timestamp);

    // Adaptive scaler + HUD + sonification — all run once per second.
    if (dumped) {
      const p95 = profiler.getFrameP95();
      const timeSinceResize = timestamp - lastResizeMs;
      const action = scalerDecide(p95, config.n, config.tier, timeSinceResize, duringRestart);
      let scalerReason: string | undefined;

      if (action.kind === "shrink_n" || action.kind === "grow_n") {
        config.n = action.newN;
        lastResizeMs = timestamp;
        scalerReason = action.kind;
        profiler.setConfig(config.backend, config.tier, config.n, config.k);
        console.log(`[scaler] ${action.kind}: n=${config.n} (tier=${config.tier} p95=${p95.toFixed(1)}ms)`);
        // TODO (browser): wasmBackend.resize(config)
      }

      // Corner HUD — update from profiler snapshot (not per-frame).
      // Mobile: HUD is hidden (spec: no debug overlays on mobile).
      if (!mobile) {
        const snap = profiler.getLastSnapshot();
        if (snap) {
          hud.update({
            fps:                  snap.fps,
            n:                    config.n,
            backend:              config.backend,
            synapticEventsPerSec: snap.synapticEventsPerSec,
            scalerReason,
          });
        }
      }

      // Sonification — update at 1/sec from profiler stats, off the hot path.
      // Disabled on mobile (spec).
      if (!mobile && sonification.enabled) {
        const snap = profiler.getLastSnapshot();
        if (snap && snap.ticksPerSec > 0) {
          const fractions = deriveRegionFractions(
            snap.spikesPerSec,
            config.n,
            snap.ticksPerSec,
          );
          const total = snap.spikesPerSec / (config.n * snap.ticksPerSec);
          sonification.update(fractions, total);
        }
      }
    }

    frameCounter++;
    lastTimestamp = timestamp;
    rafHandle = requestAnimationFrame(rafLoop);
  }

  rafHandle = requestAnimationFrame(rafLoop);

  console.log("[main] Phase 7 ready — sonification, HUD, mobile profile applied; GPU bridge = browser TODO");
}

/**
 * CPU backend coordinator (Phase 6, BV24). Owns the dedicated sim Web Worker
 * (which owns the WASM instance + rayon pool + sim state) and the WebGL2
 * renderer on the main thread. The worker writes the SoA into the shared WASM
 * memory; this class builds Float32Array/Uint32Array views over that memory and
 * hands them to the CpuRenderer each frame (zero-copy, full upload).
 *
 * Compile/typecheck-only here — the runtime path needs a browser (no browser in
 * the build env). Falls back to single-threaded sim when cross-origin isolation
 * / WASM threads are unavailable (still correct, just slower).
 */
class CpuCoordinator {
  private worker: Worker | null = null;
  private renderer: CpuRenderer | null = null;
  private memory: WebAssembly.Memory | null = null;
  private neuronCount = 0;
  private vRenderPtr = 0;
  private lastSpikePtr = 0;
  private positionsPtr = 0;
  private tickCounter = 0;
  private spikesThisFrame = 0;

  constructor(
    private canvas: HTMLCanvasElement,
    private iExt: number,
    private synScale: number,
  ) {}

  async start(config: AppConfig): Promise<void> {
    const gl = this.canvas.getContext("webgl2");
    if (!gl) throw new Error("WebGL2 unavailable (CPU backend requires it)");
    this.renderer = new CpuRenderer(gl);

    this.worker = new Worker(new URL("./cpu-worker.ts", import.meta.url), { type: "module" });
    const ready = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("CPU worker init timeout")), 30000);
      this.worker!.onmessage = (ev: MessageEvent) => {
        const m = ev.data;
        if (m.type === "ready") {
          clearTimeout(timer);
          this.memory = m.memory;
          this.neuronCount = m.neuronCount;
          this.vRenderPtr = m.vRenderPtr;
          this.lastSpikePtr = m.lastSpikePtr;
          this.positionsPtr = m.positionsPtr;
          const pos = new Float32Array(this.memory!.buffer, this.positionsPtr, this.neuronCount * 3);
          this.renderer!.setPositions(pos, this.neuronCount);
          console.log(`[cpu] coordinator ready: N=${this.neuronCount} threaded=${m.threaded}`);
          resolve();
        } else if (m.type === "ticked") {
          this.spikesThisFrame = m.spikes;
          this.tickCounter = m.tick;
          this.vRenderPtr = m.vRenderPtr;
          this.lastSpikePtr = m.lastSpikePtr;
        }
      };
      this.worker!.onerror = (e) => { clearTimeout(timer); reject(e); };
    });
    this.worker.postMessage({
      type: "init",
      n: config.n,
      k: config.k,
      seed: config.seed >>> 0,
      iExt: this.iExt,
      synapticScale: this.synScale,
      requestedThreads: navigator.hardwareConcurrency || 4,
    });
    await ready;
  }

  tick(ticks: number, excitability: number): void {
    this.worker?.postMessage({ type: "tick", ticks, excitability });
  }

  stimulate(x: number, y: number, z: number, radius: number, current: number): void {
    this.worker?.postMessage({ type: "stim", x, y, z, radius, current });
  }

  render(camera: Camera): void {
    if (!this.renderer || !this.memory || this.neuronCount === 0) return;
    const vRender = new Float32Array(this.memory.buffer, this.vRenderPtr, this.neuronCount);
    const lastSpike = new Uint32Array(this.memory.buffer, this.lastSpikePtr, this.neuronCount);
    this.renderer.render(camera, this.tickCounter, vRender, lastSpike);
  }

  lastStats(): typeof ZERO_STATS {
    return {
      tickCount: 1,
      spikes: this.spikesThisFrame,
      synapticEvents: 0,
      tickMs: 0,
    };
  }

  destroy(): void {
    this.worker?.postMessage({ type: "destroy" });
    this.worker?.terminate();
    this.worker = null;
    this.renderer?.destroy();
    this.renderer = null;
    this.memory = null;
    this.neuronCount = 0;
  }
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

/**
 * Set canvas resolution from CSS size × DPR × scale.
 * Mobile profile uses scale=0.75 to reduce pixel fill cost (Phase 7 spec).
 * Desktop uses scale=1.0 (full DPR).
 */
function resizeCanvas(canvas: HTMLCanvasElement, dprScale = 1.0): void {
  const dpr = (window.devicePixelRatio || 1) * dprScale;
  canvas.width  = Math.floor(canvas.clientWidth  * dpr) || 800;
  canvas.height = Math.floor(canvas.clientHeight * dpr) || 600;
}

boot().catch((e) => console.error("[main] boot failed:", e));
