// Entry point (Consolidation): WASM load, startup progress, controls event
// wiring, rAF + tick loop with excitability lerp, LOD plumbing.
// (0.1.1: runtime auto-scaling removed — N is fixed at startup / user-driven.)
// Consolidation closes OD11: WasmGpuBackend wires the wgpu canvas surface so
// GPU sim + render run from the rAF loop without any JS-side WebGPU objects.

import init, {
  WasmGpuBackend,
  log_cross_origin_isolation,
} from "../../crates/brain-visualizer/pkg/brain_visualizer.js";
import { Camera } from "./render/camera";
import {
  Controls,
  isMobile,
  seedExcitability,
  setExcitabilityTarget,
  showToast,
  tickExcitability,
} from "./ui/controls";
import { CpuRenderer } from "./cpu/cpu-renderer";
import { CornerHud } from "./ui/hud";
import { Profiler } from "./render/profiler";
import { Renderer } from "./render/renderer";
import {
  ZERO_STATS,
  clampNeuronCount,
  loadConfig,
  saveConfig,
  type AppConfig,
  type BackendKind,
} from "./core/types";
import { getSettings, parseMetrics, subscribe, toFloat32Array } from "./core/settings";
import { loadMorphConfig, morphConfigToJson } from "./core/morph-config";
import { DevPanel } from "./ui/dev-panel"; // V2 Phase A / Phase B

// v0.3.1: morphology-config WASM entry point. The Rust agent adds
// `set_morphology_config(json: &str)` to WasmGpuBackend in parallel; until the
// pkg .d.ts is regenerated the method is not on the generated type, so we declare
// the expected signature here and cast at the single call site (no `any`).
// TODO(v0.3.1): drop this shim once the regenerated pkg exports the method.
interface MorphCapableBackend {
  set_morphology_config(json: string): void;
}

// Cursor stimulation constants (BV10 / phase-3 spec).
const STIM_RADIUS  = 0.15;  // world units
const STIM_CURRENT = 0.3;   // biological mV → fixed-point in backend

// Manifold bounding sphere radius (neurons live at ~r=1.0 on the folded
// surface; add a margin for gyrification deformation).
const MANIFOLD_SPHERE_RADIUS = 1.4;

type StartupStatus = "loading" | "ready" | "failed";

interface StartupState {
  status: StartupStatus;
  stage: string;
  progress: number;
  frames: number;
  startedAtMs: number;
  backendMs?: number;
}

const BOOT_STARTED_AT_MS = performance.now();
let startupState: StartupState = {
  status: "loading",
  stage: "Starting renderer...",
  progress: 0,
  frames: 0,
  startedAtMs: BOOT_STARTED_AT_MS,
};

function updateStartupOverlay(update: {
  status?: StartupStatus;
  stage?: string;
  progress?: number;
  frames?: number;
  backendMs?: number;
}): void {
  startupState = {
    ...startupState,
    ...update,
    progress: clampProgress(update.progress ?? startupState.progress),
  };
  const w = window as unknown as { __bvStartup: StartupState };
  w.__bvStartup = { ...startupState };

  const overlay = document.getElementById("startup-overlay");
  const stage = document.getElementById("startup-stage");
  const bar = document.getElementById("startup-progress-bar");
  const percent = document.getElementById("startup-percent");
  const frames = document.getElementById("startup-frames");
  if (overlay) {
    overlay.classList.toggle("ready", startupState.status === "ready");
    overlay.classList.toggle("failed", startupState.status === "failed");
  }
  if (stage) stage.textContent = startupState.stage;
  if (bar) bar.style.width = `${Math.round(startupState.progress)}%`;
  if (percent) percent.textContent = startupState.status === "failed"
    ? "failed"
    : `${Math.round(startupState.progress)}%`;
  if (frames) frames.textContent = String(startupState.frames);
}

function publishFrameCounter(frameCounter: number): void {
  (window as unknown as { __bvFrameCounter: number }).__bvFrameCounter = frameCounter;
  startupState = { ...startupState, frames: frameCounter };
  (window as unknown as { __bvStartup: StartupState }).__bvStartup = { ...startupState };
  if (startupState.status !== "ready") {
    const frames = document.getElementById("startup-frames");
    if (frames) frames.textContent = String(startupState.frames);
  }
}

function clampProgress(progress: number): number {
  return Math.max(0, Math.min(100, progress));
}

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => resolve());
  });
}

async function boot(): Promise<void> {
  updateStartupOverlay({ stage: "Loading WASM module...", progress: 8 });
  let startupFrameCounter = 0;
  let startupRafHandle = 0;
  const startupRafLoop = (): void => {
    startupFrameCounter++;
    publishFrameCounter(startupFrameCounter);
    startupRafHandle = requestAnimationFrame(startupRafLoop);
  };
  startupRafHandle = requestAnimationFrame(startupRafLoop);

  // 1. Load WASM.
  await init();
  updateStartupOverlay({ stage: "Checking browser isolation...", progress: 20 });

  // 2. COOP/COEP check.
  const isolated = (globalThis as { crossOriginIsolated?: boolean })
    .crossOriginIsolated === true;
  log_cross_origin_isolation(isolated);
  updateStartupOverlay({ stage: "Loading saved configuration...", progress: 28 });

  // 3. Mobile detection — apply full mobile profile (Phase 7 / BV spec):
  //    Low tier, GPU only, 0.75×DPR render res, no near-LOD, no stim.
  const mobile = isMobile();
  // 0.1.1: restore the user's last-used config from localStorage (n/k/tier/
  // backend/speed/excitability). Mobile override is applied AFTER load.
  const config: AppConfig = loadConfig();
  // 0.1.1: seed the excitability lerp from the persisted config so a reload
  // restores the user's last brain-state/excitability (no ramp from default).
  seedExcitability(config.excitability);
  if (mobile) {
    config.tier = "low";
    config.n    = 10_000;  // Mobile profile remains below the 20k product cap.
    config.k    = 16;
    config.backend = "gpu"; // GPU only on mobile (no rayon workers overhead)
    console.log("[main] mobile detected → Low tier (N=10k K=16, GPU only, 0.75×DPR)");
    saveConfig(config); // persist the mobile-forced profile so it survives reload
  }
  updateStartupOverlay({
    stage: `Preparing canvas for N=${config.n} K=${config.k}...`,
    progress: 38,
  });

  // 4. Canvas + renderer. The wasm backend creates the only live WebGPU
  // context; the JS Renderer is passive before backend readiness.
  const canvas = document.getElementById("brain-canvas") as HTMLCanvasElement;
  // Mobile: render at 0.75× DPR (Phase 7 mobile profile).
  resizeCanvas(canvas, mobile ? 0.75 : 1.0);
  const renderer = new Renderer(canvas);
  await renderer.init();
  updateStartupOverlay({ stage: "Wiring interaction controls...", progress: 46 });

  // 5. Camera + input.
  const camera = new Camera();
  camera.setAspect(canvas.width / canvas.height);

  canvas.addEventListener("pointerdown", (e) => {
    camera.onPointerDown(e.clientX, e.clientY, e.buttons, e.shiftKey);
    if (e.button === 2 || (e.button === 0 && e.shiftKey)) {
      e.preventDefault();
    }
    if (canvas.setPointerCapture) {
      canvas.setPointerCapture(e.pointerId);
    }
  });
  window.addEventListener("pointerup", () => camera.onPointerUp());

  canvas.addEventListener("pointermove", (e) => {
    const rect = canvas.getBoundingClientRect();
    const isDrag = camera.onPointerMove(e.clientX, e.clientY, e.buttons, rect.width, rect.height, e.shiftKey);
    if (isDrag) {
      e.preventDefault();
      return;
    }
    // Skip cursor stimulation on mobile (BV10 amendment / Phase 5).
    if (!mobile) {
      // Queue stimulation for the next rAF turn rather than calling the wasm
      // backend directly here — direct calls from event handlers can re-enter a
      // live &mut borrow on WasmGpuBackend and cause a wasm-bindgen panic.
      const stim = computeStimulation(e, canvas, camera);
      if (stim) pendingStim = stim;
    }
  });

  canvas.addEventListener(
    "wheel",
    (e) => { e.preventDefault(); camera.onWheel(e.deltaY); },
    { passive: false },
  );

  canvas.addEventListener("contextmenu", (e) => e.preventDefault());

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

  window.addEventListener("keydown", (e) => {
    if (e.key !== "r" && e.key !== "R") return;
    const target = e.target as HTMLElement | null;
    const tag = target?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;
    camera.resetTarget();
  });

  // Pending resize / stimulate: set by DOM event handlers, consumed at the TOP of
  // the next rAF turn.  Never call gpuBackend methods with &mut self directly from
  // event handlers — doing so while the rAF loop holds a &mut borrow on
  // WasmGpuBackend triggers the wasm-bindgen "recursive use of an object"
  // reentrancy panic.  The rAF loop is the single owner of the backend; all
  // mutations must flow through it.
  let pendingResize: { w: number; h: number } | null = null;
  let pendingStim: { x: number; y: number; z: number; radius: number; current: number } | null = null;
  // V2 Phase 0: settings push flag.  Set by the subscribe callback; flushed at
  // the top of rafLoop alongside pendingResize to avoid &mut reentrancy.
  let pendingSettingsPush = true; // push once immediately after backend ready
  // V2 Phase B: brain-reset flag.  Kept as no-op stub (UX round 2 removed the
  // pending UI, but the flag is harmless and keeps the flush path intact).
  let pendingBrainReset = false;
  // UX round 2: network rebuild flag — set by onNetwork handler, flushed at top
  // of rafLoop (same &mut discipline as pendingResize / pendingBrainReset).
  let pendingNetworkRebuild = false;
  // v0.3.1: morphology-config JSON to push, set by the dev-panel morph handlers
  // (live uniform edits AND the explicit Rebuild). Flushed via set_morphology_config
  // in the rafLoop &mut-discipline block. Latest-wins (a single JSON snapshot).
  let pendingMorphConfig: string | null = morphConfigToJson(loadMorphConfig());

  window.addEventListener("resize", () => {
    // UX overhaul: when the settings panel is open, the canvas occupies only the
    // left portion of the viewport.  Account for the panel width so the canvas
    // does not overflow behind the open drawer.
    const panelOpen = devPanel
      ? (typeof (devPanel as unknown as { isOpen: unknown }).isOpen === "function"
          ? (devPanel as unknown as { isOpen(): boolean }).isOpen()
          : false)
      : false;
    const panelWidth = panelOpen
      ? ((DevPanel as unknown as { PANEL_WIDTH_PX?: number }).PANEL_WIDTH_PX ?? 360)
      : 0;
    const dprScale = mobile ? 0.75 : 1.0;
    const dpr = (window.devicePixelRatio || 1) * dprScale;
    const targetCssW = window.innerWidth - panelWidth;
    canvas.style.width  = `${targetCssW}px`;
    canvas.style.height = `${window.innerHeight}px`;
    canvas.width  = Math.floor(targetCssW * dpr) || 800;
    canvas.height = Math.floor(window.innerHeight * dpr) || 600;
    camera.setAspect(canvas.width / canvas.height);
    // Schedule resize — applied at the start of the next rAF turn (not inline).
    pendingResize = { w: canvas.width, h: canvas.height };
  });

  // 7. Profiler.
  const profiler = new Profiler(config.backend, config.tier, config.n, config.k);

  // 7b. Public corner HUD: still always-on, independent of the hidden dev panel.
  const cornerHud = new CornerHud();

  // UX round 2: time-based ticks/sec target — declared early so the devPanel
  // closure below can reference it without a block-scope violation.
  // 0.1.1: restore the user's last-used sim speed from the persisted config so a
  // reload keeps the chosen ticks/sec instead of snapping back to the default.
  let targetTicksPerSec = config.ticksPerSec;

  // Pause state: when true, the rAF loop renders (camera still orbits, glow
  // still decays toward steady state) but issues zero sim ticks. The tick
  // accumulator is drained each paused frame so resuming doesn't burst.
  let paused = false;

  // 7d. Dev panel (V2 Phase A / Phase B). Desktop-only: skip on mobile so the
  //     public UI stays clean on small screens. ?dev=1 / backtick / gear button.
  const devPanel: DevPanel | null = mobile ? null : new DevPanel({
    n:            config.n,
    k:            config.k,
    seed:         config.seed >>> 0,
    excitability: config.excitability,
    tps:          targetTicksPerSec,
  });

  // V2 Phase B: wire brain-reset apply handler to the dev panel.
  // The handler sets a flag consumed at the top of rafLoop; never calls the
  // backend directly (would re-enter a live &mut on WasmGpuBackend).
  if (devPanel) {
    devPanel.setApplyHandlers({
      onBrainReset: () => {
        if (gpuBackend !== null) {
          pendingBrainReset = true;
        }
      },
    });

    // UX round 2: wire sim handlers (excitability, speed-tps, network rebuild).
    // onExcitability: delegates to setExcitabilityTarget; existing lerp smoothly approaches.
    // onSpeed: sets targetTicksPerSec (1–60); time-based accumulator uses it next frame.
    // onNetwork: deferred rebuild — sets pendingNetworkRebuild flag flushed at rafLoop top
    //   (same &mut discipline as all other backend mutations).
    if (typeof (devPanel as unknown as { setSimHandlers: unknown }).setSimHandlers === "function") {
      (devPanel as unknown as {
        setSimHandlers(h: {
          onExcitability(v: number): void;
          onSpeed(tps: number): void;
          onNetwork(p: { n: number; k: number; seed: number }): void;
          onConfigReset?(config: AppConfig): void;
        }): void;
      }).setSimHandlers({
        onExcitability(v: number): void {
          setExcitabilityTarget(v);
          // 0.1.1: persist so the chosen excitability survives a reload.
          config.excitability = v;
          saveConfig(config);
        },
        onSpeed(tps: number): void {
          targetTicksPerSec = Math.max(1, Math.min(60, Math.round(tps)));
          // 0.1.1: persist so the chosen sim speed survives a reload.
          config.ticksPerSec = targetTicksPerSec;
          saveConfig(config);
        },
        onNetwork(p: { n: number; k: number; seed: number }): void {
          config.n    = clampNeuronCount(p.n);
          config.k    = p.k;
          config.seed = p.seed >>> 0;
          saveConfig(config); // 0.1.1: persist user-chosen N/K so it survives reload
          pendingNetworkRebuild = true;
          pendingSettingsPush   = true;
        },
        onConfigReset(defaultConfig: AppConfig): void {
          config.n = defaultConfig.n;
          config.k = defaultConfig.k;
          config.seed = defaultConfig.seed >>> 0;
          config.tier = defaultConfig.tier;
          config.speed = defaultConfig.speed;
          config.backend = defaultConfig.backend;
          config.excitability = defaultConfig.excitability;
          config.ticksPerSec = defaultConfig.ticksPerSec;
          targetTicksPerSec = defaultConfig.ticksPerSec;
          seedExcitability(defaultConfig.excitability);
        },
      });
    }

    // v0.3.1: wire morphology-config apply handlers. Both paths stash the JSON in
    // pendingMorphConfig (latest-wins); the rafLoop flushes it via
    // set_morphology_config under the same &mut discipline. The Rust side diffs
    // the config and runs the narrowest update (uniform / regenerate / pipeline).
    if (typeof (devPanel as unknown as { setMorphHandlers: unknown }).setMorphHandlers === "function") {
      (devPanel as unknown as {
        setMorphHandlers(h: {
          onMorphLive(json: string): void;
          onMorphRebuild(json: string): void;
        }): void;
      }).setMorphHandlers({
        onMorphLive(json: string): void {
          pendingMorphConfig = json;
        },
        onMorphRebuild(json: string): void {
          pendingMorphConfig = json;
        },
      });
    }

    // UX overhaul: shrink canvas when settings panel opens, restore when it closes.
    // Done via pendingResize so the resize is applied at the top of the next rAF
    // turn (same &mut discipline as all other backend mutations).
    if (typeof (devPanel as unknown as { onVisibilityChange: unknown }).onVisibilityChange === "function") {
      (devPanel as unknown as {
        onVisibilityChange(cb: (open: boolean) => void): void;
      }).onVisibilityChange((open: boolean) => {
        const panelWidth = (DevPanel as unknown as { PANEL_WIDTH_PX?: number }).PANEL_WIDTH_PX ?? 360;
        const targetCssW = open
          ? window.innerWidth - panelWidth
          : window.innerWidth;
        const dprScale = mobile ? 0.75 : 1.0;
        const dpr = (window.devicePixelRatio || 1) * dprScale;
        canvas.style.width  = `${targetCssW}px`;
        canvas.style.height = `${window.innerHeight}px`;
        canvas.width  = Math.floor(targetCssW * dpr) || 800;
        canvas.height = Math.floor(window.innerHeight * dpr) || 600;
        camera.setAspect(canvas.width / canvas.height);
        pendingResize = { w: canvas.width, h: canvas.height };
      });
    }
  }

  // 8. Controls (facade kept for console access).
  const controls = new Controls(config, (cfg) => {
    console.log(`[main] restart requested (stub): ${JSON.stringify(cfg)}`);
    profiler.setConfig(cfg.backend, cfg.tier, cfg.n, cfg.k);
  });
  (window as unknown as { brainControls: Controls }).brainControls = controls;
  // Expose restartWithBackend for console-driven backend switching (UX round 2).
  (window as unknown as { _restartWithBackend: typeof restartWithBackend })._restartWithBackend = restartWithBackend;

  // 9. Wire DOM click handlers — UX overhaul: speed/tier/brain-state removed from
  //    main view; their handlers now live in the settings panel (devPanel sim handlers
  //    above). Settings-toggle wiring lives in the dev panel.

  // Pause toggle (bottom-center transport). Freezes sim ticks; rendering and
  // camera orbit continue. Available on mobile too (no &mut interaction — it
  // only flips a JS flag the rAF loop reads).
  const pauseBtn = document.getElementById("pause-toggle");
  if (pauseBtn) {
    pauseBtn.addEventListener("click", () => {
      paused = !paused;
      pauseBtn.textContent = paused ? "▶" : "⏸";
      pauseBtn.title = paused ? "Resume simulation" : "Pause simulation";
      pauseBtn.setAttribute("aria-pressed", String(paused));
    });
  }

  // Settings / gear toggle (UX overhaul). Opens/closes the dev panel.
  // Hidden on mobile (panel is desktop-only).
  const settingsBtn = document.getElementById("settings-toggle");
  if (settingsBtn && !mobile && devPanel) {
    settingsBtn.addEventListener("click", () => {
      if (typeof (devPanel as unknown as { toggle: unknown }).toggle === "function") {
        (devPanel as unknown as { toggle(): void }).toggle();
      } else {
        // Fallback: use the internal _toggle alias (_setOpen) if toggle() not yet landed.
        (devPanel as unknown as { _toggle(): void })._toggle?.();
      }
    });
  } else if (settingsBtn && mobile) {
    (settingsBtn as HTMLElement).style.display = "none";
  }

  // 10. Restart sequence state.
  let rafHandle = 0;
  let duringRestart = false;

  // V2 Phase 0: GLOW_TAU and POINT_RADIUS are now sourced from VisualSettings
  // (pushed to the backend via update_settings).  The render_frame call no
  // longer accepts them as positional arguments.

  // Sim tuning constants (Phase 2 locked values, verified by SOC sweep).
  // Shared by both GPU and CPU backends so the two backends run identical dynamics.
  const SIM_I_EXT    = 0.055;
  const SIM_SYN_SCALE = 0.03;
  // Aliases used by their respective backend startup paths.
  const GPU_I_EXT    = SIM_I_EXT;
  const GPU_SYN_SCALE = SIM_SYN_SCALE;

  // GPU backend instance. Created once at boot; recreated on backend switch or
  // tier change. null during init or when CPU backend is active.
  let gpuBackend: WasmGpuBackend | null = null;

  // Phase 6 CPU backend coordinator (BV24). Owns the worker + the SoA views the
  // WebGL2 CpuRenderer draws. Tuning matches examples/cpu_check.rs / sim_check.rs.
  const cpu = new CpuCoordinator(canvas, SIM_I_EXT, SIM_SYN_SCALE);

  /**
   * Create (or recreate) the wasm GPU backend for the current config.
   * The GpuBackend owns the wgpu canvas surface; the Renderer wrapper stays
   * passive so startup never creates a duplicate canvas/device context.
   */
  async function startGpuBackend(): Promise<void> {
    const backendStartedAt = performance.now();
    try {
      updateStartupOverlay({ stage: "Preparing WebGPU backend...", progress: 58 });
      await nextAnimationFrame();
      updateStartupOverlay({ stage: "Creating WebGPU device and pipelines...", progress: 68 });
      // WasmGpuBackend.create() acquires the browser WebGPU device, creates a
      // wgpu surface from the canvas, configures it, builds all pipelines,
      // uploads the manifold, and returns a ready-to-use backend.
      gpuBackend = await WasmGpuBackend.create(
        canvas,
        config.n,
        config.k,
        config.seed >>> 0,
        GPU_I_EXT,
        GPU_SYN_SCALE,
      ) as WasmGpuBackend;
      // Boot-apply: push every persisted config surface once backend is ready.
      // AppConfig already constructed this backend and seeded JS loop state;
      // visual settings and morphology config cross by explicit backend calls.
      pendingSettingsPush = true;
      pendingMorphConfig = morphConfigToJson(loadMorphConfig());
      const backendMs = performance.now() - backendStartedAt;
      updateStartupOverlay({
        stage: "Backend ready. Rendering first frame...",
        progress: 94,
        backendMs,
      });
      console.log(`[main] WasmGpuBackend created in ${backendMs.toFixed(1)}ms`);
    } catch (e) {
      console.error("[main] GPU backend creation failed:", e);
      showToast("WebGPU init failed — check browser support");
      const message = e instanceof Error ? e.message : String(e);
      updateStartupOverlay({
        status: "failed",
        stage: `WebGPU startup failed: ${message}`,
        progress: 100,
      });
      gpuBackend = null;
    }
  }

  // V2 Phase 0: subscribe to settings changes.  Set a flag (never call the
  // backend directly from the callback — it may fire while rafLoop holds &mut).
  subscribe(() => { pendingSettingsPush = true; });

  /**
   * BV16 restart sequence: cancel rAF, tear down the current backend, reinit the
   * other one with the SAME seed (identical network). The black-canvas gap
   * during teardown+reinit is acceptable per the spec.
   *
   * Both GPU (WasmGpuBackend) and CPU (CpuCoordinator) paths are real.
   */
  async function restartWithBackend(kind: BackendKind): Promise<void> {
    if (duringRestart) return;
    duringRestart = true;
    cancelAnimationFrame(rafHandle);

    console.log(`[main] restart → backend=${kind} seed=0x${config.seed.toString(16)}`);
    const prev = config.backend;
    config.backend = kind;
    profiler.setConfig(kind, config.tier, config.n, config.k);

    // Tear down previous backend.
    if (prev === "cpu") cpu.destroy();
    if (prev === "gpu" && gpuBackend) {
      gpuBackend.destroy();
      gpuBackend = null;
    }

    // Start new backend.
    if (kind === "cpu") {
      try {
        await cpu.start(config);
      } catch (e) {
        console.warn("[main] CPU backend start failed, reverting to GPU:", e);
        showToast("CPU backend failed to start");
        config.backend = "gpu";
        kind = "gpu";
      }
    }
    if (kind === "gpu") {
      await startGpuBackend();
    }

    duringRestart = false;
    rafHandle = requestAnimationFrame(rafLoop);
  }

  // UX round 2: time-based ticks/sec scheduling (replaces frame-count multiplier).
  // targetTicksPerSec: declared earlier (near devPanel) so closures can reference it.
  // tickAccumulator: fractional carry-over between frames for sub-integer rates.
  let tickAccumulator = 0.0;

  // UX overhaul: running max ticksPerSec (for panel SysInfo.maxTicksPerSec).
  let maxTicksPerSec = 0;

  // 12. rAF + tick loop.
  let frameCounter = startupFrameCounter;
  let lastTimestamp = performance.now();
  let tickCount = 0;
  let firstReadyFrameSeen = false;

  function rafLoop(timestamp: DOMHighResTimeStamp): void {
    // ── Flush deferred mutations BEFORE any backend call ────────────────────
    // All mutations from DOM event handlers must be applied here (not inline in
    // the handlers) to avoid re-entering a &mut borrow on WasmGpuBackend.
    if (pendingResize !== null) {
      if (gpuBackend) {
        gpuBackend.resize(pendingResize.w, pendingResize.h);
      }
      pendingResize = null;
    }
    // V2 Phase B: brain reset — now a no-op stub (UX round 2 removed the UI).
    if (pendingBrainReset && gpuBackend) {
      pendingBrainReset = false;
      // No-op: brain-reset pending UI removed; network rebuilds go via pendingNetworkRebuild.
    }
    // UX round 2: deferred network rebuild (N/K/seed change from Network tab).
    // Never call reinitialize directly from the onNetwork handler — use this
    // deferred flag to avoid &mut reentrancy on WasmGpuBackend.
    if (pendingNetworkRebuild && gpuBackend) {
      pendingNetworkRebuild = false;
      gpuBackend.reinitialize(
        config.n,
        config.k,
        config.seed >>> 0,
        getSettings().iExt,
        getSettings().synapticScale,
      );
      // Re-push all settings so visual knobs apply to the fresh network.
      pendingSettingsPush = true;
      pendingMorphConfig = morphConfigToJson(loadMorphConfig());
      console.log(`[main] network rebuild: n=${config.n} k=${config.k} seed=0x${config.seed.toString(16)}`);
    }
    // V2 Phase 0: push settings to the backend when changed (or on first frame
    // after backend creation).
    if (pendingSettingsPush && gpuBackend) {
      gpuBackend.update_settings(toFloat32Array(getSettings()));
      pendingSettingsPush = false;
    }
    // v0.3.1: flush morphology-config JSON to the backend. The Rust side diffs
    // current-vs-new and chooses uniform-only / regenerate / pipeline-rebuild.
    // TODO(v0.3.1): set_morphology_config exists after the parallel wasm rebuild;
    // the method is declared on MorphCapableBackend below so typecheck passes now.
    if (pendingMorphConfig !== null && gpuBackend) {
      (gpuBackend as unknown as MorphCapableBackend).set_morphology_config(pendingMorphConfig);
      pendingMorphConfig = null;
    }
    if (pendingStim !== null) {
      const { x, y, z, radius, current } = pendingStim;
      pendingStim = null;
      if (config.backend === "gpu" && gpuBackend) {
        gpuBackend.stimulate(x, y, z, radius, current);
      } else if (config.backend === "cpu") {
        cpu.stimulate(x, y, z, radius, current);
      }
    }
    // ────────────────────────────────────────────────────────────────────────

    // UX round 2: time-based tick scheduling.
    // dtSec is clamped to 50 ms (20 fps floor) to avoid spiral-of-death on slow frames.
    // If the frame took >50 ms, we skip ticks entirely and drain the accumulator so
    // we don't burst on recovery (same spirit as the old >20 ms guard).
    const frameMs = timestamp - lastTimestamp;
    const dtSec   = Math.min(frameMs / 1000, 0.05); // clamp: max 50 ms of sim per frame
    let ticks = 0;
    if (paused) {
      tickAccumulator = 0; // frozen: drain so resume doesn't burst
    } else if (frameMs <= 50) {
      tickAccumulator += dtSec * targetTicksPerSec;
      ticks = Math.floor(tickAccumulator);
      tickAccumulator -= ticks;
    } else {
      tickAccumulator = 0; // drain on very long frames to avoid burst on recovery
    }

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
      // GPU path: tick + render via WasmGpuBackend (OD11 closed — bridge wired).
      if (gpuBackend) {
        if (ticks > 0) {
          const spikes = gpuBackend.tick(ticks, excitability);
          stats = {
            tickCount: ticks,
            spikes: spikes,
            synapticEvents: Math.round(spikes * config.k),
            tickMs: 0,
          };
        }
        const dist = camera.cameraDistance();
        gpuBackend.set_lod_camera_distance(dist);

        const mvp   = camera.mvpMatrix();
        const right = camera.cameraRight();
        const up    = camera.cameraUp();
        const eye   = camera.eye();
        // V2 Phase 0: glow_tau and point_radius are sourced from VisualSettings
        // inside the backend (set via update_settings); no longer passed here.
        gpuBackend.render_frame(
          mvp,
          right[0], right[1], right[2],
          up[0],    up[1],    up[2],
          eye[0],   eye[1],   eye[2],
          dist,
        );
        if (!firstReadyFrameSeen) {
          firstReadyFrameSeen = true;
          const totalMs = performance.now() - BOOT_STARTED_AT_MS;
          updateStartupOverlay({ status: "ready", stage: "Ready", progress: 100 });
          console.log(`[main] first GPU frame rendered after ${totalMs.toFixed(1)}ms`);
        }
      } else {
        // gpuBackend not yet ready (still initializing); visible startup state is
        // handled by the DOM overlay so this does not claim the canvas context.
        renderer.render(camera, tickCount);
      }
    }

    profiler.recordFrame(timestamp, timestamp - lastTimestamp, stats);
    const dumped = profiler.maybeDump(timestamp);

    // HUD and dev-panel monitor updates run once per second.
    // (0.1.1: runtime auto-scaling removed — N is fixed at startup / user-driven.)
    if (dumped) {
      const snap = profiler.getLastSnapshot();
      if (snap) {
        cornerHud.update({
          fps: snap.fps,
          n: config.n,
          backend: config.backend,
          synapticEventsPerSec: snap.synapticEventsPerSec,
        });
      }

      // Dev panel — update Monitor tab metrics + SysInfo once per second (V2 Phase A / UX overhaul).
      // Passes Metrics (GPU readback) + SysInfo (n, k, fps, ticksPerSec, maxTicksPerSec).
      // Only compute when panel is open; guard avoids unnecessary work.
      if (devPanel && config.backend === "gpu" && gpuBackend) {
        if (snap) {
          if (snap.ticksPerSec > maxTicksPerSec) maxTicksPerSec = snap.ticksPerSec;
        }
        if (devPanel.isOpen()) {
          const panelUpdate = (devPanel as unknown as {
            update(m: ReturnType<typeof parseMetrics>, sys?: {
              n: number; k: number; fps: number;
              ticksPerSec: number; maxTicksPerSec: number;
            }): void;
          }).update;
          if (typeof panelUpdate === "function") {
            panelUpdate.call(devPanel, parseMetrics(gpuBackend.metrics()), snap ? {
              n: config.n,
              k: config.k,
              fps: snap.fps,
              ticksPerSec: snap.ticksPerSec,
              maxTicksPerSec,
            } : undefined);
          }
        }
      }
    }

    frameCounter++;
    // Expose frame counter on window for integration tests (E2E can poll this to
    // confirm the rAF loop is alive without relying on visual output).
    publishFrameCounter(frameCounter);
    lastTimestamp = timestamp;
    rafHandle = requestAnimationFrame(rafLoop);
  }

  updateStartupOverlay({ stage: "Starting animation loop...", progress: 52 });
  cancelAnimationFrame(startupRafHandle);
  frameCounter = startupFrameCounter;
  rafHandle = requestAnimationFrame(rafLoop);
  void startGpuBackend();

  console.log("[main] Consolidation ready — OD11 GPU bridge wired (WasmGpuBackend); rAF started before async GPU init");
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

    this.worker = new Worker(new URL("./cpu/cpu-worker.ts", import.meta.url), { type: "module" });
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
 * Unproject the pointer and intersect the manifold bounding sphere, returning
 * stimulation params if the ray hits. Returns null on miss.
 * Call site queues the result for the next rAF turn to avoid re-entering a live
 * &mut borrow on WasmGpuBackend (wasm-bindgen reentrancy panic).
 */
function computeStimulation(
  e: PointerEvent,
  canvas: HTMLCanvasElement,
  camera: Camera,
): { x: number; y: number; z: number; radius: number; current: number } | null {
  const rect = canvas.getBoundingClientRect();
  const cssX = e.clientX - rect.left;
  const cssY = e.clientY - rect.top;
  const { origin, dir } = camera.unproject(cssX, cssY, rect.width, rect.height);
  const hit = raySphereIntersect(origin, dir, [0, 0, 0], MANIFOLD_SPHERE_RADIUS);
  if (!hit) return null;
  return { x: hit[0], y: hit[1], z: hit[2], radius: STIM_RADIUS, current: STIM_CURRENT };
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

boot().catch((e) => {
  console.error("[main] boot failed:", e);
  const message = e instanceof Error ? e.message : String(e);
  updateStartupOverlay({
    status: "failed",
    stage: `Startup failed: ${message}`,
    progress: 100,
  });
});
