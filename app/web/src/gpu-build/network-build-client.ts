import {
  validatePreparedNetworkPayload,
  type PreparedNetworkPayload,
  type PreparedNetworkRequest,
} from "./prepared-network";

type WorkerOut =
  | { type: "ready"; payload: PreparedNetworkPayload }
  | { type: "failed"; sequence: number; message: string };

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

  constructor(workerFactory: () => Worker = defaultWorkerFactory) {
    this.worker = workerFactory();
    this.worker.onmessage = (event: MessageEvent<WorkerOut>) => {
      this.handleMessage(event.data);
    };
    this.worker.onerror = (event) => {
      this.status = {
        kind: "failed",
        sequence: this.latestRequested,
        message: event.message || "network build worker failed",
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
    if (message.sequence !== this.latestRequested) return;
    this.readyPayload = null;
    this.status = {
      kind: "failed",
      sequence: message.sequence,
      message: message.message,
    };
  }
}

function defaultWorkerFactory(): Worker {
  return new Worker(new URL("./network-build-worker.ts", import.meta.url), { type: "module" });
}
