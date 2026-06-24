import { describe, expect, test } from "vitest";
import { NetworkBuildClient } from "./network-build-client";
import type { PreparedNetworkPayload, PreparedNetworkRequest } from "./prepared-network";

class MockWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  posted: unknown[] = [];

  postMessage(message: unknown): void {
    this.posted.push(message);
  }

  terminate(): void {}

  emit(message: unknown): void {
    this.onmessage?.({ data: message } as MessageEvent);
  }

  emitError(event: Partial<ErrorEvent>): void {
    this.onerror?.(event as ErrorEvent);
  }
}

function request(sequence: number): PreparedNetworkRequest {
  return {
    sequence,
    n: 2,
    k: 1,
    seed: sequence,
    regionAssignmentMode: "hash-random",
    visualSettings: new Float32Array(26),
    morphConfigJson: "{}",
  };
}

function payload(sequence: number): PreparedNetworkPayload {
  return {
    version: 1,
    sequence,
    n: 2,
    k: 1,
    seed: sequence,
    regionAssignmentMode: "hash-random",
    gridDim: 1,
    gridCellSize: 1,
    droppedCount: 0,
    positions: new Float32Array([0, 0, 0, 1, 1, 1]),
    regionCodes: new Uint8Array([0, 1]),
    gridMin: new Float32Array([0, 0, 0]),
    gridCellStart: new Uint32Array([0, 2]),
    gridCellNeurons: new Uint32Array([0, 1]),
    vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
    faces: new Uint32Array([0, 1, 2]),
    segmentEndpoints: new Float32Array(),
    segmentPathLen: new Float32Array(),
    segmentNeuronIds: new Uint32Array(),
    segmentKinds: new Uint32Array(),
    segmentTargetIds: new Uint32Array(),
    sphereGeometry: new Float32Array([0, 0, 0, 0.1, 0, 1, 0, 0, 1, 1, 1, 0.1, 1, 0, 0, 0]),
    sphereNeuronIds: new Uint32Array([0, 1]),
    sphereKinds: new Uint32Array([2, 2]),
    statsJson: "{}",
    paramsJson: "{}",
    visualSettings: new Float32Array(26),
    morphConfigJson: "{}",
  };
}

describe("NetworkBuildClient", () => {
  test("drops stale worker results and exposes only the latest payload", () => {
    const worker = new MockWorker();
    const client = new NetworkBuildClient(() => worker as unknown as Worker);

    client.request(request(1));
    client.request(request(2));
    worker.emit({ type: "ready", payload: payload(1) });
    expect(client.consumeReady()).toBeNull();
    expect(client.currentStatus()).toEqual({ kind: "preparing", sequence: 2 });

    worker.emit({ type: "ready", payload: payload(2) });
    expect(client.currentStatus()).toEqual({ kind: "ready", sequence: 2 });
    expect(client.consumeReady()?.sequence).toBe(2);
    expect(client.currentStatus()).toEqual({ kind: "idle" });
  });

  test("drops stale failures", () => {
    const worker = new MockWorker();
    const client = new NetworkBuildClient(() => worker as unknown as Worker);

    client.request(request(3));
    client.request(request(4));
    worker.emit({ type: "failed", sequence: 3, message: "old" });
    expect(client.currentStatus()).toEqual({ kind: "preparing", sequence: 4 });
  });

  test("surfaces a non-empty message when worker.onerror has none", () => {
    const worker = new MockWorker();
    const client = new NetworkBuildClient(() => worker as unknown as Worker);

    client.request(request(7));
    worker.emitError({ message: "", filename: "", lineno: 0 });
    const status = client.currentStatus();
    expect(status.kind).toBe("failed");
    if (status.kind === "failed") {
      expect(status.message.length).toBeGreaterThan(0);
      expect(status.message).toContain("out of memory");
    }
  });

  test("flags a non-latest failed sequence as stale so it does not roll back", () => {
    const worker = new MockWorker();
    const client = new NetworkBuildClient(() => worker as unknown as Worker);

    client.request(request(8));
    client.request(request(9));
    // A leftover failure from the superseded request 8 is stale; the latest
    // request 9 is not.
    expect(client.isStaleFailure(8)).toBe(true);
    expect(client.isStaleFailure(9)).toBe(false);
  });

  test("delivers progress ticks for the latest request and drops stale ones", () => {
    const worker = new MockWorker();
    const client = new NetworkBuildClient(() => worker as unknown as Worker);
    const seen: number[] = [];
    client.onProgress((p) => seen.push(p.fraction));

    client.request(request(5));
    client.request(request(6));
    worker.emit({ type: "progress", sequence: 5, stage: "prepare-payload", phase: "old", fraction: 0.5 });
    worker.emit({ type: "progress", sequence: 6, stage: "prepare-payload", phase: "Growing morphology", fraction: 0.85 });
    expect(seen).toEqual([0.85]);

    client.onProgress(null);
    worker.emit({ type: "progress", sequence: 6, stage: "prepare-payload", phase: "done", fraction: 1 });
    expect(seen).toEqual([0.85]);
  });
});
