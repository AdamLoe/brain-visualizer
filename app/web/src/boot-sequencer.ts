/**
 * Boot sequencer (boot-load overhaul).
 *
 * The staged GPU-backend startup orchestration extracted out of `main.ts` so the
 * real `runStage` / `onSubStage` / network-payload-progress wiring can be
 * exercised in a GPU-free integration test (vitest), with the WebGPU backend and
 * the frame-yield stubbed. `main.ts` delegates to `runGpuStartup` so the live
 * boot path and the test path share one implementation — the test reproduces the
 * actual boot ordering instead of re-testing isolated helpers.
 *
 * The progress-listener lifecycle fix lives here: the network-payload progress
 * listener is attached BEFORE the GPU-acquire stage (right after the startup
 * payload request is fired), and the latest fraction is buffered, so the worker's
 * progress ticks — emitted while it races the GPU handshake — are not dropped
 * before the "Prepare network payload" stage becomes active. The payload stage
 * then replays the buffered fraction so the label climbs from where the worker
 * actually is rather than sitting at 0%.
 */

import { formatSubStageLabel, mapSubStageProgress } from "./boot-overlay";
import type {
  PreparedNetworkProgress,
  PreparedNetworkStatus,
} from "./gpu-build/network-build-client";
import type { PreparedNetworkPayload } from "./gpu-build/prepared-network";

/** Minimal network-client surface the sequencer depends on (subset of
 * NetworkBuildClient) so the test can drive a real client or a faithful fake. */
export interface NetworkClientLike {
  onProgress(
    listener: ((progress: PreparedNetworkProgress) => void) | null,
  ): void;
  consumeReady(): PreparedNetworkPayload | null;
  currentStatus(): PreparedNetworkStatus;
}

/** The staged WASM backend's startup methods. Each is feature-detected at the
 * call site so a stale generated pkg can't break boot. */
export interface StagedBackendLike {
  startup_upload_neuron_buffers(): void;
  startup_upload_render_resources(): void;
  startup_allocate_lod_edge_resources(): void;
  startup_upload_morphology(): void;
  startup_finish_network(): void;
  startup_build_render_pipelines(): void;
  startup_resize_render_targets(): void;
  set_progress_callback?(cb: (label: string, fraction: number) => void): void;
}

export interface BackendFactory {
  /** Old generated packages only expose this; returns a fully-initialized
   * backend with no further explicit stages available. */
  create(): Promise<unknown>;
  /** Boot-load overhaul: staged acquire with a sub-stage progress callback. */
  create_staged?(
    onSubStage: (label: string, fraction: number) => void,
  ): Promise<StagedBackendLike>;
}

export interface OverlayUpdate {
  stage?: string;
  progress?: number;
}

export interface RunGpuStartupArgs {
  factory: BackendFactory;
  networkClient: NetworkClientLike;
  /** Fire the early "startup" payload request and return its sequence. Must be
   * invoked by the sequencer BEFORE the GPU-acquire stage so worker generation
   * overlaps the handshake. */
  requestStartupNetwork(): number;
  /** Push overlay state (the same updater main.ts uses). */
  updateOverlay(update: OverlayUpdate): void;
  /** Yield to the next animation frame (stubbed in tests). */
  nextFrame(): Promise<void>;
  /** Stage prepared payload into the backend (the heavy begin_prepared_network
   * call). Kept as a callback so the wasm-typed glue stays in main.ts. */
  stagePreparedPayload(
    backend: StagedBackendLike,
    payload: PreparedNetworkPayload,
  ): void;
  /** Logger (defaults to console.log). */
  log?(message: string): void;
}

export interface RunGpuStartupResult {
  /** The staged backend if create_staged ran; the raw create() result
   * otherwise; null on failure. */
  backend: unknown;
  /** Whether the staged path ran to completion. */
  staged: boolean;
  /** The prepared payload (only on the staged path). */
  preparedPayload: PreparedNetworkPayload | null;
  /** Set when the backend failed to initialize. */
  error: unknown;
}

const PROGRESS_START = 54;
const PROGRESS_END = 96;

/**
 * Run the real staged GPU-backend startup sequence. Behaviour-preserving
 * extraction of `startGpuBackend` from `main.ts`, with the progress-listener
 * attached early so payload ticks emitted during the GPU-acquire stage are not
 * dropped.
 */
export async function runGpuStartup(
  args: RunGpuStartupArgs,
): Promise<RunGpuStartupResult> {
  const {
    factory,
    networkClient,
    requestStartupNetwork,
    updateOverlay,
    nextFrame,
    stagePreparedPayload,
  } = args;
  const log = args.log ?? ((m: string) => console.log(m));

  let stagedBackend: StagedBackendLike | null = null;
  let rawBackend: unknown = null;
  let startupPreparedPayload: PreparedNetworkPayload | null = null;

  const startupPrepareSequence = requestStartupNetwork();

  let stageBandStart = PROGRESS_START;
  let stageBandEnd = PROGRESS_START;

  const onSubStage = (label: string, fraction: number): void => {
    updateOverlay({
      stage: formatSubStageLabel(label, fraction),
      progress: mapSubStageProgress(fraction, stageBandStart, stageBandEnd),
    });
  };

  // Fix: attach the payload-progress listener NOW — before the GPU-acquire
  // stage — and buffer the latest fraction. The warmed worker races the GPU
  // handshake and emits its phase ticks (folding → source-types → morphology →
  // soma) while "Acquire GPU" is the active stage; if we waited to subscribe
  // until the "Prepare network payload" stage's run() (the old bug), those ticks
  // are dropped (no listener) and the label sits at 0% until the payload lands,
  // then jumps. We capture them here so the payload stage can replay the latest.
  let latestPayloadFraction = 0;
  let sawPayloadProgress = false;
  networkClient.onProgress((progress) => {
    if (progress.sequence !== startupPrepareSequence) return;
    if (progress.fraction > latestPayloadFraction) {
      latestPayloadFraction = progress.fraction;
    }
    sawPayloadProgress = true;
    // If the payload stage is the active band, drive the label live.
    if (payloadStageActive) onSubStage(PAYLOAD_LABEL, latestPayloadFraction);
  });

  const PAYLOAD_LABEL = "Prepare network payload";
  let payloadStageActive = false;

  async function waitForPreparedNetwork(
    sequence: number,
  ): Promise<PreparedNetworkPayload> {
    for (;;) {
      const ready = networkClient.consumeReady();
      if (ready !== null && ready.sequence === sequence) return ready;
      const status = networkClient.currentStatus();
      if (status.kind === "failed" && status.sequence === sequence) {
        throw new Error(status.message);
      }
      await nextFrame();
    }
  }

  type BackendStage = {
    name: string;
    weight: number;
    run(): void | Promise<void>;
  };

  const stages: BackendStage[] = [
    {
      name: "Acquire GPU + core pipelines",
      weight: 0.34,
      run: async () => {
        if (typeof factory.create_staged !== "function") {
          rawBackend = await factory.create();
          return;
        }
        stagedBackend = await factory.create_staged(onSubStage);
        if (typeof stagedBackend.set_progress_callback === "function") {
          stagedBackend.set_progress_callback(onSubStage);
        }
      },
    },
    {
      name: PAYLOAD_LABEL,
      weight: 0.18,
      run: async () => {
        payloadStageActive = true;
        // Replay the latest fraction the worker already reported during acquire
        // (or 0 if none yet) so the label starts where the worker actually is
        // rather than snapping back to 0%.
        onSubStage(PAYLOAD_LABEL, sawPayloadProgress ? latestPayloadFraction : 0);
        try {
          startupPreparedPayload = await waitForPreparedNetwork(
            startupPrepareSequence,
          );
        } finally {
          payloadStageActive = false;
          networkClient.onProgress(null);
        }
        onSubStage(PAYLOAD_LABEL, 1);
      },
    },
    {
      name: "Stage prepared payload",
      weight: 0.03,
      run: () => {
        if (stagedBackend === null || startupPreparedPayload === null) return;
        stagePreparedPayload(stagedBackend, startupPreparedPayload);
      },
    },
    {
      name: "Upload neuron buffers",
      weight: 0.05,
      run: () => stagedBackend?.startup_upload_neuron_buffers(),
    },
    {
      name: "Upload render mesh",
      weight: 0.05,
      run: () => stagedBackend?.startup_upload_render_resources(),
    },
    {
      name: "Finalize render allocation",
      weight: 0.02,
      run: () => stagedBackend?.startup_allocate_lod_edge_resources(),
    },
    {
      name: "Upload morphology buffers",
      weight: 0.05,
      run: () => stagedBackend?.startup_upload_morphology(),
    },
    {
      name: "Bind network resources",
      weight: 0.03,
      run: () => stagedBackend?.startup_finish_network(),
    },
    {
      name: "Compile render pipelines",
      weight: 0.16,
      run: () => stagedBackend?.startup_build_render_pipelines(),
    },
    {
      name: "Create render targets",
      weight: 0.07,
      run: () => stagedBackend?.startup_resize_render_targets(),
    },
  ];

  const totalWeight = stages.reduce((sum, s) => sum + s.weight, 0) || 1;
  const band = PROGRESS_END - PROGRESS_START;
  let cumulativeWeight = 0;

  const runStage = async (stage: BackendStage): Promise<boolean> => {
    stageBandStart = PROGRESS_START + (cumulativeWeight / totalWeight) * band;
    cumulativeWeight += stage.weight;
    stageBandEnd = PROGRESS_START + (cumulativeWeight / totalWeight) * band;
    updateOverlay({ stage: stage.name, progress: stageBandStart });
    await nextFrame();
    const started = performance.now();
    await stage.run();
    const ms = performance.now() - started;
    updateOverlay({ stage: stage.name, progress: stageBandEnd });
    log(`[startup] ${stage.name}: ${ms.toFixed(1)}ms`);
    return stagedBackend !== null;
  };

  try {
    updateOverlay({ stage: "Preparing WebGPU backend…", progress: PROGRESS_START });
    for (let i = 0; i < stages.length; i++) {
      const stillStaged = await runStage(stages[i]);
      if (i === 0 && !stillStaged) break;
    }
    return {
      backend: stagedBackend !== null ? stagedBackend : rawBackend,
      staged: stagedBackend !== null,
      preparedPayload: startupPreparedPayload,
      error: null,
    };
  } catch (e) {
    // Ensure the listener is detached on the error path too.
    networkClient.onProgress(null);
    return { backend: null, staged: false, preparedPayload: null, error: e };
  }
}
