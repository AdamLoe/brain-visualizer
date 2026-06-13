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
});
