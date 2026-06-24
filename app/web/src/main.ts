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
import { CornerHud } from "./ui/hud";
import { Profiler } from "./render/profiler";
import { Renderer } from "./render/renderer";
import { RebuildCoordinator } from "./rebuild/rebuild-coordinator";
import { settingsRequirePreparedNetwork } from "./rebuild/rebuild-intent";
import {
  NetworkBuildClient,
  type PreparedNetworkProgress,
  type PreparedNetworkStatus,
} from "./gpu-build/network-build-client";
import type { PreparedNetworkPayload } from "./gpu-build/prepared-network";
import { runGpuStartup, type StagedBackendLike } from "./boot-sequencer";
import {
  logBootSummary,
  recordBootTiming,
  startBootWatchdog,
  type BootWatchdog,
} from "./boot-timings";
import {
  ZERO_STATS,
  clampNeuronCount,
  loadConfig,
  saveConfig,
  type AppConfig,
  type BackendKind,
  type RegionAssignmentMode,
} from "./core/types";
import { getSettings, parseMetrics, replaceSettings, saveSettings, subscribe, toFloat32Array } from "./core/settings";
import { loadMorphConfig, morphConfigToJson, saveMorphConfig, type MorphologyConfig } from "./core/morph-config";
import { DevPanel } from "./ui/dev-panel"; // V2 Phase A / Phase B
import {
  diagnosticsPolicyForViewport,
  hasWebGpuSupport,
  resetAppOwnedStorage,
  webGpuStartupFailureStage,
  webGpuUnsupportedStage,
} from "./boot-failure";
import { applyMobileConfig } from "./core/mobile-config";

// v0.3.1: morphology-config WASM entry point. The Rust agent adds
// `set_morphology_config(json: &str)` to WasmGpuBackend in parallel; until the
// pkg .d.ts is regenerated the method is not on the generated type, so we declare
// the expected signature here and cast at the single call site (no `any`).
// TODO(v0.3.1): drop this shim once the regenerated pkg exports the method.
interface MorphCapableBackend {
  set_morphology_config(json: string): void;
}

interface PreparedNetworkCapableBackend {
  startup_begin_prepared_network(
    version: number,
    n: number,
    k: number,
    seed: number,
    visualSettings: Float32Array,
    morphConfigJson: string,
    positions: Float32Array,
    regionCodes: Uint8Array,
    gridMin: Float32Array,
    gridCellSize: number,
    gridDim: number,
    gridCellStart: Uint32Array,
    gridCellNeurons: Uint32Array,
    vertices: Float32Array,
    faces: Uint32Array,
    segmentEndpoints: Float32Array,
    segmentPathLen: Float32Array,
    segmentNeuronIds: Uint32Array,
    segmentKinds: Uint32Array,
    segmentTargetIds: Uint32Array,
    sphereGeometry: Float32Array,
    sphereNeuronIds: Uint32Array,
    sphereKinds: Uint32Array,
    droppedCount: number,
  ): void;
  apply_prepared_network(
    version: number,
    n: number,
    k: number,
    seed: number,
    visualSettings: Float32Array,
    morphConfigJson: string,
    positions: Float32Array,
    regionCodes: Uint8Array,
    gridMin: Float32Array,
    gridCellSize: number,
    gridDim: number,
    gridCellStart: Uint32Array,
    gridCellNeurons: Uint32Array,
    vertices: Float32Array,
    faces: Uint32Array,
    segmentEndpoints: Float32Array,
    segmentPathLen: Float32Array,
    segmentNeuronIds: Uint32Array,
    segmentKinds: Uint32Array,
    segmentTargetIds: Uint32Array,
    sphereGeometry: Float32Array,
    sphereNeuronIds: Uint32Array,
    sphereKinds: Uint32Array,
    droppedCount: number,
  ): void;
}

interface BrainVisualizerTestHooks {
  __bvFrameCounter?: number;
  __bvStartup?: StartupState;
  __bvNetworkBuildStatus?: PreparedNetworkStatus;
  __bvDiagnosticsPolicy?: "desktop-supported" | "unsupported-mobile";
  __bvRollbackState?: { reason: string; settings: Readonly<ReturnType<typeof getSettings>>; config: AppConfig };
  __bvForceStructuralRollback?: (reason?: string) => void;
  __bvFailLatestPreparedNetworkForTesting?: (message?: string) => void;
  __bvRequestPreparedNetworkSmoke?: (request: {
    n?: number;
    k?: number;
    seed?: number;
    regionAssignmentMode?: RegionAssignmentMode;
  }) => number;
  // Test-only: subscribe to real worker payload-build progress ticks for the
  // latest request. Additive proxy to NetworkBuildClient.onProgress so e2e can
  // observe the worker -> client progress wiring without a GPU adapter.
  __bvOnNetworkBuildProgress?: (
    listener: ((progress: PreparedNetworkProgress) => void) | null,
  ) => void;
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
  elapsedMs: number;
}

interface StagedGpuBackend extends WasmGpuBackend {
  startup_build_manifold(): void;
  startup_upload_neuron_buffers(): void;
  startup_upload_render_resources(): void;
  startup_allocate_lod_edge_resources(): void;
  startup_upload_morphology(): void;
  startup_finish_network(): void;
  startup_build_render_pipelines(): void;
  startup_resize_render_targets(): void;
  // Boot-load overhaul (A1): compile bloom + *_active pipelines lazily one
  // frame after the first render. Inherited from WasmGpuBackend; the call site
  // still feature-detects at runtime so a stale pkg without it can't break boot.
  build_deferred_render_pipelines(): void;
  // Boot-load overhaul (B): install a (label, fraction) sub-stage progress
  // callback. Inherited from WasmGpuBackend; the call site feature-detects too.
  set_progress_callback(cb: (label: string, fraction: number) => void): void;
}

interface StagedGpuBackendConstructor {
  create(
    canvas: HTMLCanvasElement,
    n: number,
    k: number,
    seed: number,
    iExt: number,
    synapticScale: number,
  ): Promise<WasmGpuBackend>;
  create_staged?(
    canvas: HTMLCanvasElement,
    n: number,
    k: number,
    seed: number,
    iExt: number,
    synapticScale: number,
    // Boot-load overhaul (B): optional (label, fraction) progress callback for
    // the GPU-acquire sub-stages. Optional so a stale .d.ts can't break boot.
    progress?: (label: string, fraction: number) => void,
  ): Promise<StagedGpuBackend>;
}

const BOOT_STARTED_AT_MS = performance.now();
/** The live dev-only boot stall watchdog (module-scoped so both the "Ready"
 * path and the top-level boot().catch can clear its interval). */
let activeBootWatchdog: BootWatchdog | null = null;
let startupRecoveryActionsWired = false;
let startupState: StartupState = {
  status: "loading",
  stage: "Starting…",
  progress: 0,
  frames: 0,
  startedAtMs: BOOT_STARTED_AT_MS,
  elapsedMs: 0,
};

function updateStartupOverlay(update: {
  status?: StartupStatus;
  stage?: string;
  progress?: number;
}): void {
  const elapsedMs = performance.now() - startupState.startedAtMs;
  startupState = {
    ...startupState,
    ...update,
    elapsedMs,
    progress: clampProgress(update.progress ?? startupState.progress),
  };
  const w = window as unknown as { __bvStartup: StartupState };
  w.__bvStartup = { ...startupState };

  const overlay = document.getElementById("startup-overlay");
  const stage = document.getElementById("startup-stage");
  const bar = document.getElementById("startup-progress-bar");
  const percent = document.getElementById("startup-percent");
  if (overlay) {
    overlay.classList.toggle("ready", startupState.status === "ready");
    overlay.classList.toggle("failed", startupState.status === "failed");
  }
  if (stage) stage.textContent = startupState.stage;
  if (bar) bar.style.width = `${Math.round(startupState.progress)}%`;
  if (percent) percent.textContent = startupState.status === "failed"
    ? "failed"
    : `${Math.round(startupState.progress)}%`;
}

function publishFrameCounter(frameCounter: number): void {
  (window as unknown as { __bvFrameCounter: number }).__bvFrameCounter = frameCounter;
  startupState = { ...startupState, frames: frameCounter };
  (window as unknown as { __bvStartup: StartupState }).__bvStartup = { ...startupState };
}

function clampProgress(progress: number): number {
  return Math.max(0, Math.min(100, progress));
}

function wireStartupRecoveryActions(): void {
  if (startupRecoveryActionsWired) return;
  startupRecoveryActionsWired = true;
  const resetStorage = document.getElementById("startup-reset-storage") as HTMLButtonElement | null;
  const loadDefaults = document.getElementById("startup-load-defaults") as HTMLButtonElement | null;
  const retry = document.getElementById("startup-retry") as HTMLButtonElement | null;

  resetStorage?.addEventListener("click", () => {
    resetAppOwnedStorage();
    updateStartupOverlay({
      status: "failed",
      stage: "Saved app settings were reset. Reload defaults or retry startup.",
      progress: 100,
    });
  }, { once: true });

  loadDefaults?.addEventListener("click", () => {
    resetAppOwnedStorage();
    window.location.assign(window.location.pathname);
  });

  retry?.addEventListener("click", () => {
    window.location.reload();
  });
}

function setDiagnosticsPolicy(policy: "desktop-supported" | "unsupported-mobile"): void {
  (window as unknown as BrainVisualizerTestHooks).__bvDiagnosticsPolicy = policy;
}

/** Parse the morphology stats JSON and record its MorphTimer sub-phase ms into
 * __bvBootTimings (prefixed so they group under the payload stage). Best effort:
 * a bad/empty stats string is silently ignored — observability must not break
 * boot. */
function recordPayloadSubPhaseTimings(statsJson: string): void {
  try {
    const stats = JSON.parse(statsJson) as { timings?: Record<string, number> };
    const t = stats.timings;
    if (!t) return;
    const phases: Array<[string, keyof typeof t]> = [
      ["payload: incoming view", "incoming_ms"],
      ["payload: dendrite", "dendrite_ms"],
      ["payload: axon", "axon_ms"],
    ];
    for (const [label, key] of phases) {
      const ms = t[key];
      if (typeof ms === "number") recordBootTiming(label, ms);
    }
  } catch {
    // ignore — observability only
  }
}

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => resolve());
  });
}

async function boot(): Promise<void> {
  wireStartupRecoveryActions();
  if (new URLSearchParams(window.location.search).get("bv_force_startup_failure") === "1") {
    throw new Error("forced startup failure");
  }
  // Boot observability: record per-step wall-clock ms into __bvBootTimings.
  // `markPhaseA(name)` records the time since the previous mark, attributing it
  // to the step that just finished. Phase-B (boot-sequencer) stages and the
  // worker payload sub-phases feed the same array via recordBootTiming.
  let phaseAMark = performance.now();
  const markPhaseA = (stage: string): void => {
    const now = performance.now();
    recordBootTiming(stage, now - phaseAMark);
    phaseAMark = now;
  };
  // DEV-only stall watchdog: warn if the overlay step (label + %) is unchanged
  // for > 2s, the product signal that "a boot step taking > 2s is wrong". Cheap
  // (one interval); stopped at "Ready". `import.meta.env.DEV` is true under Vite
  // dev / vitest and false in production builds. Read defensively because the
  // project's tsconfig doesn't pull in `vite/client`'s ImportMeta typing.
  const viteEnv = (import.meta as unknown as { env?: { DEV?: boolean } }).env;
  if (viteEnv?.DEV) {
    activeBootWatchdog = startBootWatchdog(() => ({
      label: startupState.stage,
      percent: Math.round(startupState.progress),
    }));
  }

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
  // Boot-load overhaul (A4): construct the network-build worker as early as
  // possible (right after the main module's WASM init, before canvas/renderer
  // setup) so its own WASM instantiate — kicked off by the constructor's warm()
  // — overlaps the main-thread renderer init and the GPU handshake. The main
  // module's WASM bytes are already cached, so the worker's fetch is a cache hit.
  const networkBuildClient = new NetworkBuildClient();
  markPhaseA("Load WASM module");
  updateStartupOverlay({ stage: "Checking browser isolation...", progress: 20 });

  // 2. COOP/COEP check.
  const isolated = (globalThis as { crossOriginIsolated?: boolean })
    .crossOriginIsolated === true;
  log_cross_origin_isolation(isolated);
  markPhaseA("Check browser isolation");
  updateStartupOverlay({ stage: "Loading saved configuration...", progress: 28 });

  // 3. Mobile detection: lower DPR and disable stim on phones, without
  // increasing the accepted default neuron count.
  const mobile = isMobile();
  setDiagnosticsPolicy(diagnosticsPolicyForViewport(window.innerWidth, mobile));
  // 0.1.1: restore the user's last-used config from localStorage (n/k/tier/
  // backend/speed/excitability). Stale CPU backend saves normalize to GPU in
  // loadConfig(); the mobile override is applied AFTER load.
  const config: AppConfig = loadConfig();
  // 0.1.1: seed the excitability lerp from the persisted config so a reload
  // restores the user's last brain-state/excitability (no ramp from default).
  seedExcitability(config.excitability);
  if (mobile) {
    applyMobileConfig(config);
    console.log(`[main] mobile detected -> ${config.tier} tier (N=${config.n} K=${config.k}, 0.75xDPR)`);
    saveConfig(config);
  }
  markPhaseA("Load saved configuration");
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
  markPhaseA("Prepare canvas + renderer");
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
  const rebuildCoordinator = new RebuildCoordinator();
  let nextNetworkBuildSequence = 1;
  let lastReportedNetworkBuildFailure = 0;
  let appliedMorphConfigJson = morphConfigToJson(loadMorphConfig());
  let lastSettingsSnapshot = getSettings();
  let lastAppliedConfig: AppConfig = { ...config };
  rebuildCoordinator.requestSettingsPush();
  rebuildCoordinator.requestMorphConfig(appliedMorphConfigJson);

  // Sim tuning constants (Phase 2 locked values, verified by SOC sweep).
  const SIM_I_EXT    = 0.055;
  const SIM_SYN_SCALE = 0.03;
  const GPU_I_EXT    = SIM_I_EXT;
  const GPU_SYN_SCALE = SIM_SYN_SCALE;

  // GPU backend instance. Created once at boot; recreated on tier change.
  // null only while init is in flight.
  let gpuBackend: WasmGpuBackend | null = null;

  function publishNetworkBuildStatus(): void {
    (window as unknown as BrainVisualizerTestHooks).__bvNetworkBuildStatus =
      networkBuildClient.currentStatus();
  }

  function requestPreparedNetwork(
    reason: string,
    morphConfigJson = morphConfigToJson(loadMorphConfig()),
    visualSettings = toFloat32Array(getSettings()),
    requestConfig: AppConfig = config,
  ): number {
    const sequence = nextNetworkBuildSequence++;
    networkBuildClient.request({
      sequence,
      n: requestConfig.n,
      k: requestConfig.k,
      seed: requestConfig.seed >>> 0,
      regionAssignmentMode: requestConfig.regionAssignmentMode,
      visualSettings,
      morphConfigJson,
    });
    publishNetworkBuildStatus();
    console.log(`[main] network prepare requested (${reason}): seq=${sequence} n=${requestConfig.n} k=${requestConfig.k} seed=0x${(requestConfig.seed >>> 0).toString(16)}`);
    return sequence;
  }

  (window as unknown as BrainVisualizerTestHooks).__bvRequestPreparedNetworkSmoke = (request) => {
    return requestPreparedNetwork("smoke", morphConfigToJson(loadMorphConfig()), toFloat32Array(getSettings()), {
      ...config,
      n: clampNeuronCount(request.n ?? config.n),
      k: request.k ?? config.k,
      seed: (request.seed ?? config.seed) >>> 0,
      regionAssignmentMode: request.regionAssignmentMode ?? config.regionAssignmentMode,
    });
  };

  (window as unknown as BrainVisualizerTestHooks).__bvOnNetworkBuildProgress = (listener) => {
    networkBuildClient.onProgress(listener);
  };
  (window as unknown as BrainVisualizerTestHooks).__bvFailLatestPreparedNetworkForTesting = (message = "forced prepared-network failure") => {
    networkBuildClient.failLatestForTesting(message);
    publishNetworkBuildStatus();
  };

  /**
   * Create (or recreate) the wasm GPU backend for the current config.
   * Startup uses the staged WASM API so the loading overlay can advance from
   * real completed work and the browser can paint between expensive setup
   * blocks.
   */
  async function startGpuBackend(): Promise<void> {
    const backendStartedAt = performance.now();
    if (!hasWebGpuSupport(navigator)) {
      console.error("[main] WebGPU unavailable: navigator.gpu is missing");
      showToast("WebGPU is required for this experience");
      updateStartupOverlay({
        status: "failed",
        stage: webGpuUnsupportedStage(),
        progress: 100,
      });
      gpuBackend = null;
      return;
    }
    // Boot-load overhaul: the staged-startup orchestration lives in
    // `boot-sequencer.ts` (runGpuStartup) so the real runStage / onSubStage /
    // onProgress wiring can be exercised in a GPU-free integration test. The
    // sequencer fires the early "startup" payload request itself (before the
    // GPU-acquire stage) and attaches the payload-progress listener up front, so
    // worker ticks emitted while it races the GPU handshake aren't dropped.
    const ctor = WasmGpuBackend as unknown as StagedGpuBackendConstructor;

    const result = await runGpuStartup({
      factory: {
        create: () => ctor.create(
          canvas,
          config.n,
          config.k,
          config.seed >>> 0,
          GPU_I_EXT,
          GPU_SYN_SCALE,
        ),
        create_staged: typeof ctor.create_staged === "function"
          ? (onSubStage) =>
              ctor.create_staged!(
                canvas,
                config.n,
                config.k,
                config.seed >>> 0,
                GPU_I_EXT,
                GPU_SYN_SCALE,
                onSubStage,
              ) as unknown as Promise<StagedBackendLike>
          : undefined,
      },
      networkClient: networkBuildClient,
      requestStartupNetwork: () =>
        requestPreparedNetwork("startup", appliedMorphConfigJson),
      updateOverlay: (update) => updateStartupOverlay(update),
      nextFrame: () => {
        // Publish status each poll turn so __bvNetworkBuildStatus tracks the
        // in-flight payload build during boot (as the old inline wait did).
        publishNetworkBuildStatus();
        return nextAnimationFrame();
      },
      stagePreparedPayload: (backend, payload) => {
        (backend as unknown as PreparedNetworkCapableBackend).startup_begin_prepared_network(
          payload.version,
          payload.n,
          payload.k,
          payload.seed >>> 0,
          payload.visualSettings,
          payload.morphConfigJson,
          payload.positions,
          payload.regionCodes,
          payload.gridMin,
          payload.gridCellSize,
          payload.gridDim,
          payload.gridCellStart,
          payload.gridCellNeurons,
          payload.vertices,
          payload.faces,
          payload.segmentEndpoints,
          payload.segmentPathLen,
          payload.segmentNeuronIds,
          payload.segmentKinds,
          payload.segmentTargetIds,
          payload.sphereGeometry,
          payload.sphereNeuronIds,
          payload.sphereKinds,
          payload.droppedCount,
        );
        appliedMorphConfigJson = payload.morphConfigJson;
        lastSettingsSnapshot = getSettings();
        // Observability: fold the worker's MorphTimer sub-phase ms (already in
        // the payload stats) into the boot summary so the dominant payload phase
        // is itemized (incoming/dendrite/axon) — the seconds the user sees as the
        // "Prepare network payload" stage, now broken down.
        recordPayloadSubPhaseTimings(payload.statsJson);
      },
      log: (message) => console.log(message),
      recordTiming: (stage, ms) => recordBootTiming(stage, ms),
    });

    if (result.error !== null) {
      const e = result.error;
      console.error("[main] GPU backend creation failed:", e);
      showToast("WebGPU startup failed");
      updateStartupOverlay({
        status: "failed",
        stage: webGpuStartupFailureStage(),
        progress: 100,
      });
      gpuBackend = null;
      return;
    }

    gpuBackend = result.backend as WasmGpuBackend | null;
    rebuildCoordinator.requestSettingsPush();
    rebuildCoordinator.requestMorphConfig(morphConfigToJson(loadMorphConfig()));
    const backendMs = performance.now() - backendStartedAt;
    updateStartupOverlay({
      stage: "Rendering first frame…",
      progress: 98,
    });
    console.log(`[main] WasmGpuBackend startup completed in ${backendMs.toFixed(1)}ms`);
  }

  const gpuStartupPromise = startGpuBackend();

  window.addEventListener("resize", () => {
    setDiagnosticsPolicy(diagnosticsPolicyForViewport(window.innerWidth, mobile));
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
    const targetCssW = Math.max(1, window.innerWidth - panelWidth);
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
    regionAssignmentMode: config.regionAssignmentMode,
    excitability: config.excitability,
    tps:          targetTicksPerSec,
  });

  if (devPanel) {
    // UX round 2: wire sim handlers (excitability, speed-tps, network rebuild).
    // onExcitability: delegates to setExcitabilityTarget; existing lerp smoothly approaches.
    // onSpeed: sets targetTicksPerSec (1–60); time-based accumulator uses it next frame.
    // onNetwork: worker-prepared rebuild — the worker builds the payload, and
    // rafLoop applies the latest ready payload under the same &mut discipline as
    // all other backend mutations.
    if (typeof (devPanel as unknown as { setSimHandlers: unknown }).setSimHandlers === "function") {
      (devPanel as unknown as {
        setSimHandlers(h: {
          onExcitability(v: number): void;
          onSpeed(tps: number): void;
          onNetwork(p: { n: number; k: number; seed: number; regionAssignmentMode: RegionAssignmentMode }): void;
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
        onNetwork(p: { n: number; k: number; seed: number; regionAssignmentMode: RegionAssignmentMode }): void {
          requestPreparedNetwork("network controls", morphConfigToJson(loadMorphConfig()), toFloat32Array(getSettings()), {
            ...config,
            n: clampNeuronCount(p.n),
            k: p.k,
            seed: p.seed >>> 0,
            regionAssignmentMode: p.regionAssignmentMode,
          });
        },
        onConfigReset(defaultConfig: AppConfig): void {
          config.n = defaultConfig.n;
          config.k = defaultConfig.k;
          config.seed = defaultConfig.seed >>> 0;
          config.tier = defaultConfig.tier;
          config.speed = defaultConfig.speed;
          config.backend = defaultConfig.backend;
          config.regionAssignmentMode = defaultConfig.regionAssignmentMode;
          config.excitability = defaultConfig.excitability;
          config.ticksPerSec = defaultConfig.ticksPerSec;
          targetTicksPerSec = defaultConfig.ticksPerSec;
          seedExcitability(defaultConfig.excitability);
        },
      });
    }

    // v0.3.1: wire morphology-config apply handlers. Both paths queue JSON in
    // the rebuild coordinator (latest-wins); the rafLoop flushes it via
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
          rebuildCoordinator.requestMorphConfig(json);
        },
        // Rebuild Morphology = geometry, rebuilt in-place. A generator change
        // only regenerates axon-tree geometry (Rust set_morphology_config →
        // regenerate_morphology), never network topology — topology changes go
        // through Regenerate Network. So this always takes the cheap in-place
        // path, never a worker network prepare.
        onMorphRebuild(json: string): void {
          rebuildCoordinator.requestMorphConfig(json);
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
          ? Math.max(1, window.innerWidth - panelWidth)
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
  // Expose restartWithBackend for console-driven GPU restarts.
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
      pauseBtn.setAttribute("aria-label", paused ? "Resume simulation" : "Pause simulation");
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

  // V2 Phase 0: subscribe to settings changes.  Set a flag (never call the
  // backend directly from the callback — it may fire while rafLoop holds &mut).
  subscribe((settings) => {
    if (settingsRequirePreparedNetwork(lastSettingsSnapshot, settings)) {
      requestPreparedNetwork(
        "structural settings",
        morphConfigToJson(loadMorphConfig()),
        toFloat32Array(settings),
      );
      return;
    }
    lastSettingsSnapshot = { ...settings };
    rebuildCoordinator.requestSettingsPush();
  });

  /** Restart the GPU backend after a tier/reset-style config change. */
  async function restartWithBackend(kind: BackendKind): Promise<void> {
    if (duringRestart) return;
    duringRestart = true;
    cancelAnimationFrame(rafHandle);

    console.log(`[main] restart → backend=${kind} seed=0x${config.seed.toString(16)}`);
    config.backend = "gpu";
    profiler.setConfig("gpu", config.tier, config.n, config.k);

    if (gpuBackend) {
      gpuBackend.destroy();
      gpuBackend = null;
    }

    await startGpuBackend();

    duringRestart = false;
    rafHandle = requestAnimationFrame(rafLoop);
  }

  function rollbackStructuralState(reason: string): void {
    Object.assign(config, lastAppliedConfig);
    replaceSettings({ ...lastSettingsSnapshot });
    saveConfig(config);
    if (devPanel && typeof (devPanel as unknown as { setInitialValues: unknown }).setInitialValues === "function") {
      (devPanel as unknown as {
        setInitialValues(opts: {
          n: number; k: number; seed: number; regionAssignmentMode: RegionAssignmentMode;
          excitability: number; tps: number;
        }): void;
      }).setInitialValues({
        n: config.n,
        k: config.k,
        seed: config.seed >>> 0,
        regionAssignmentMode: config.regionAssignmentMode,
        excitability: config.excitability,
        tps: config.ticksPerSec,
      });
    }
    if (devPanel && typeof (devPanel as unknown as { rollbackMorphologyConfig: unknown }).rollbackMorphologyConfig === "function") {
      (devPanel as unknown as { rollbackMorphologyConfig(json: string): void })
        .rollbackMorphologyConfig(appliedMorphConfigJson);
    }
    (window as unknown as BrainVisualizerTestHooks).__bvRollbackState = {
      reason,
      settings: { ...lastSettingsSnapshot },
      config: { ...config },
    };
  }
  (window as unknown as BrainVisualizerTestHooks).__bvForceStructuralRollback = (reason = "forced rollback") => {
    rollbackStructuralState(reason);
  };

  function applyPreparedNetworkPayload(payload: PreparedNetworkPayload): void {
    if (!gpuBackend) return;
    (gpuBackend as unknown as PreparedNetworkCapableBackend).apply_prepared_network(
      payload.version,
      payload.n,
      payload.k,
      payload.seed >>> 0,
      payload.visualSettings,
      payload.morphConfigJson,
      payload.positions,
      payload.regionCodes,
      payload.gridMin,
      payload.gridCellSize,
      payload.gridDim,
      payload.gridCellStart,
      payload.gridCellNeurons,
      payload.vertices,
      payload.faces,
      payload.segmentEndpoints,
      payload.segmentPathLen,
      payload.segmentNeuronIds,
      payload.segmentKinds,
      payload.segmentTargetIds,
      payload.sphereGeometry,
      payload.sphereNeuronIds,
      payload.sphereKinds,
      payload.droppedCount,
    );
    config.n = payload.n;
    config.k = payload.k;
    config.seed = payload.seed >>> 0;
    config.regionAssignmentMode = payload.regionAssignmentMode;
    appliedMorphConfigJson = payload.morphConfigJson;
    lastSettingsSnapshot = getSettings();
    lastAppliedConfig = { ...config };
    saveSettings({ ...lastSettingsSnapshot });
    saveAppliedMorphConfigJson(appliedMorphConfigJson);
    saveConfig(config);
    rebuildCoordinator.requestSettingsPush();
    rebuildCoordinator.requestMorphConfig(morphConfigToJson(loadMorphConfig()));
    profiler.setConfig(config.backend, config.tier, payload.n, payload.k);
    console.log(
      `[main] prepared network applied: seq=${payload.sequence} n=${payload.n} k=${payload.k} seed=0x${payload.seed.toString(16)} segments=${payload.segmentPathLen.length}`,
    );
  }

  function saveAppliedMorphConfigJson(json: string): void {
    try {
      saveMorphConfig(JSON.parse(json) as MorphologyConfig);
    } catch {
      console.warn("[main] applied morphology config was not valid JSON; localStorage not updated");
    }
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
  // Boot-load overhaul (A1): true once the deferred render pipelines (bloom +
  // *_active morphology) have been compiled, one frame after the first frame.
  let deferredPipelinesBuilt = false;

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
    if (gpuBackend) {
      const preparedPayload = networkBuildClient.consumeReady();
      if (preparedPayload !== null) {
        try {
          applyPreparedNetworkPayload(preparedPayload);
          publishNetworkBuildStatus();
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          console.error("[main] prepared network apply failed:", error);
          rollbackStructuralState(`apply failed: ${message}`);
          showToast(`Network rebuild failed: ${message}`);
        }
      }
    }
    const networkBuildStatus = networkBuildClient.currentStatus();
    publishNetworkBuildStatus();
    if (
      networkBuildStatus.kind === "failed"
      && networkBuildStatus.sequence !== lastReportedNetworkBuildFailure
    ) {
      lastReportedNetworkBuildFailure = networkBuildStatus.sequence;
      console.error(`[main] network prepare failed: seq=${networkBuildStatus.sequence}: ${networkBuildStatus.message}`);
      rollbackStructuralState(`prepare failed: ${networkBuildStatus.message}`);
      showToast(`Network prepare failed: ${networkBuildStatus.message}`);
    }
    if (gpuBackend && rebuildCoordinator.hasPendingWork()) {
      try {
        const rebuildStep = rebuildCoordinator.applyNext({
          reinitialize(n, k, seed, iExt, synapticScale): void {
            gpuBackend!.reinitialize(n, k, seed, iExt, synapticScale);
          },
          updateSettings(settings): void {
            gpuBackend!.update_settings(settings);
          },
          applyMorphConfig(json): void {
            (gpuBackend! as unknown as MorphCapableBackend).set_morphology_config(json);
          },
        }, {
          visualSettings: () => toFloat32Array(getSettings()),
          simulationSettings: () => {
            const settings = getSettings();
            return {
              iExt: settings.iExt,
              synapticScale: settings.synapticScale,
            };
          },
          morphConfigJson: () => morphConfigToJson(loadMorphConfig()),
        });
        if (rebuildStep.kind === "network") {
          const request = rebuildStep.request;
          console.log(`[main] network rebuild: n=${request.n} k=${request.k} seed=0x${request.seed.toString(16)}`);
        } else if (rebuildStep.kind === "morphology") {
          appliedMorphConfigJson = rebuildStep.json;
          saveAppliedMorphConfigJson(rebuildStep.json);
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        console.error("[main] rebuild apply failed:", error);
        rollbackStructuralState(`rebuild failed: ${message}`);
        showToast(`Rebuild failed: ${message}`);
      }
    }
    if (pendingStim !== null) {
      const { x, y, z, radius, current } = pendingStim;
      pendingStim = null;
      if (gpuBackend) {
        gpuBackend.stimulate(x, y, z, radius, current);
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
      const mvp   = camera.mvpMatrix();
      const right = camera.cameraRight();
      const up    = camera.cameraUp();
      const eye   = camera.eye();
      const dist  = camera.cameraDistance();
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
        // Boot is done: stop the stall watchdog and emit the one clean
        // __bvBootTimings summary table (Phase-A + Phase-B + payload sub-phases).
        recordBootTiming("First GPU frame", totalMs - phaseAMark);
        activeBootWatchdog?.stop();
        activeBootWatchdog = null;
        logBootSummary();
      } else if (!deferredPipelinesBuilt) {
        // Boot-load overhaul (A1): compile the deferred render pipelines (bloom +
        // true-opacity *_active morphology variants) one frame AFTER the first
        // frame painted, so those shader compiles stay off the critical path.
        // render_full guards every bloom/active access with is_some(), so the
        // first frame renders correctly without them; bloom/active appear ~1
        // frame later (imperceptible). No-op on the create() fallback path,
        // which already built everything.
        deferredPipelinesBuilt = true;
        const deferrable = gpuBackend as unknown as {
          build_deferred_render_pipelines?: () => void;
        };
        if (typeof deferrable.build_deferred_render_pipelines === "function") {
          try {
            deferrable.build_deferred_render_pipelines();
          } catch (e) {
            console.error("[main] deferred render pipeline build failed:", e);
          }
        }
      }
    } else {
      // gpuBackend not yet ready (still initializing); visible startup state is
      // handled by the DOM overlay so this does not claim the canvas context.
      renderer.render(camera, tickCount);
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

  markPhaseA("Wire interaction controls");
  updateStartupOverlay({ stage: "Starting animation loop...", progress: 52 });
  cancelAnimationFrame(startupRafHandle);
  frameCounter = startupFrameCounter;
  rafHandle = requestAnimationFrame(rafLoop);
  void gpuStartupPromise;

  console.log("[main] Consolidation ready — OD11 GPU bridge wired (WasmGpuBackend); rAF started before async GPU init");
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
  wireStartupRecoveryActions();
  activeBootWatchdog?.stop();
  activeBootWatchdog = null;
  const message = e instanceof Error ? e.message : String(e);
  updateStartupOverlay({
    status: "failed",
    stage: `Startup failed: ${message}`,
    progress: 100,
  });
});
