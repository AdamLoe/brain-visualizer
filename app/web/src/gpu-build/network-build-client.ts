import {
  validatePreparedNetworkPayload,
  type PreparedNetworkPayload,
  type PreparedNetworkRequest,
} from "./prepared-network";

type WorkerOut =
  | { type: "ready"; payload: PreparedNetworkPayload }
  | { type: "progress"; sequence: number; stage: "prepare-payload"; phase: string; fraction: number }
  | { type: "failed"; sequence: number; message: string };

/** Real payload-build progress for the current request (latest-wins). */
export interface PreparedNetworkProgress {
  sequence: number;
  phase: string;
  /** 0..1 over the whole payload build. */
  fraction: number;
}

export type PreparedNetworkStatus =
  | { kind: "idle" }
  | { kind: "preparing"; sequence: number }
  | { kind: "ready"; sequence: number }
  | { kind: "failed"; sequence: number; message: string };

export class NetworkBuildClient {
  private readonly worker: Worker;
  private latestRequested = 0;
  private status: PreparedNetworkStatus = { kind: "idle" };
  private readyPayload: PreparedNetworkPayload | null = null;
  private progressListener: ((progress: PreparedNetworkProgress) => void) | null = null;

  constructor(workerFactory: () => Worker = defaultWorkerFactory) {
    this.worker = workerFactory();
    this.worker.onmessage = (event: MessageEvent<WorkerOut>) => {
      this.handleMessage(event.data);
    };
    this.worker.onerror = (event) => {
      this.status = {
        kind: "failed",
        sequence: this.latestRequested,
        message: workerErrorMessage(event),
      };
    };
    // Boot-load overhaul (A4): warm the worker's WASM instance immediately on
    // construction so its instantiate overlaps the main-thread renderer init +
    // GPU handshake, instead of serializing in front of the first `prepare`.
    this.warm();
  }

  /** Ask the worker to instantiate its WASM module now (idempotent, fire-and-forget). */
  warm(): void {
    this.worker.postMessage({ type: "warm" });
  }

  request(request: PreparedNetworkRequest): void {
    this.latestRequested = Math.max(this.latestRequested, request.sequence);
    this.readyPayload = null;
    this.status = { kind: "preparing", sequence: request.sequence };
    this.worker.postMessage({ type: "prepare", request }, [request.visualSettings.buffer]);
  }

  currentStatus(): PreparedNetworkStatus {
    return this.status;
  }

  /**
   * A `failed` sequence is stale once a later request has superseded it. The
   * rafLoop uses this to refuse rolling back an already-applied newer build on
   * a leftover failure from an abandoned request.
   */
  isStaleFailure(sequence: number): boolean {
    return sequence !== this.latestRequested;
  }

  failLatestForTesting(message: string): void {
    if (this.latestRequested <= 0) return;
    this.readyPayload = null;
    this.status = {
      kind: "failed",
      sequence: this.latestRequested,
      message,
    };
  }

  /**
   * Subscribe to real payload-build progress for the latest request. Additive:
   * existing consumers that never call this are unaffected. Only ticks whose
   * `sequence` matches the latest request are delivered (latest-wins).
   */
  onProgress(listener: ((progress: PreparedNetworkProgress) => void) | null): void {
    this.progressListener = listener;
  }

  consumeReady(): PreparedNetworkPayload | null {
    const payload = this.readyPayload;
    this.readyPayload = null;
    if (payload !== null) {
      this.status = { kind: "idle" };
    }
    return payload;
  }

  destroy(): void {
    this.worker.terminate();
  }

  private handleMessage(message: WorkerOut): void {
    if (message.type === "ready") {
      if (message.payload.sequence !== this.latestRequested) return;
      validatePreparedNetworkPayload(message.payload);
      this.readyPayload = message.payload;
      this.status = { kind: "ready", sequence: message.payload.sequence };
      return;
    }
    if (message.type === "progress") {
      if (message.sequence !== this.latestRequested) return;
      this.progressListener?.({
        sequence: message.sequence,
        phase: message.phase,
        fraction: message.fraction,
      });
      return;
    }
    if (message.sequence !== this.latestRequested) return;
    this.readyPayload = null;
    this.status = {
      kind: "failed",
      sequence: message.sequence,
      message: message.message,
    };
  }
}

/**
 * `worker.onerror` fires with an empty `event.message` for many uncaught worker
 * crashes (e.g. WASM out-of-memory at high N), which would otherwise surface as
 * a blank toast. Fall back to the source location, then a generic OOM-leaning
 * message, so the failure is never silent.
 */
function workerErrorMessage(event: ErrorEvent): string {
  if (event.message) return event.message;
  if (event.filename) {
    return `network build worker crashed at ${event.filename}:${event.lineno}`;
  }
  return "network build worker crashed (likely out of memory at this N)";
}

function defaultWorkerFactory(): Worker {
  return new Worker(new URL("./network-build-worker.ts", import.meta.url), { type: "module" });
}
