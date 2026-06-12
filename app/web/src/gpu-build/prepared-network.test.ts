import { describe, expect, test } from "vitest";
import { validatePreparedNetworkPayload, type PreparedNetworkPayload } from "./prepared-network";

function payload(): PreparedNetworkPayload {
  return {
    version: 1,
    sequence: 1,
    n: 2,
    k: 1,
    seed: 7,
    gridDim: 1,
    gridCellSize: 1,
    droppedCount: 0,
    positions: new Float32Array([0, 0, 0, 1, 1, 1]),
    regionCodes: new Uint8Array([0, 2]),
    gridMin: new Float32Array([0, 0, 0]),
    gridCellStart: new Uint32Array([0, 2]),
    gridCellNeurons: new Uint32Array([0, 1]),
    vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
    faces: new Uint32Array([0, 1, 2]),
    segmentEndpoints: new Float32Array([0, 0, 0, 0.1, 1, 1, 1, 0.1]),
    segmentPathLen: new Float32Array([0]),
    segmentNeuronIds: new Uint32Array([0]),
    segmentKinds: new Uint32Array([1]),
    segmentTargetIds: new Uint32Array([1]),
    sphereGeometry: new Float32Array([
      0, 0, 0, 0.1, 0, 1, 0, 0.2,
      1, 1, 1, 0.1, 1, 0, 0, 0.1,
    ]),
    sphereNeuronIds: new Uint32Array([0, 1]),
    sphereKinds: new Uint32Array([2, 2]),
    statsJson: "{}",
    paramsJson: "{}",
    visualSettings: new Float32Array(26),
    morphConfigJson: "{}",
  };
}

describe("prepared network payload validation", () => {
  test("accepts a flat GPU-agnostic payload", () => {
    expect(() => validatePreparedNetworkPayload(payload())).not.toThrow();
  });

  test("rejects stale contract versions", () => {
    const p = payload();
    p.version = 2;
    expect(() => validatePreparedNetworkPayload(p)).toThrow(/version/);
  });

  test("rejects segment field length mismatch", () => {
    const p = payload();
    p.segmentKinds = new Uint32Array();
    expect(() => validatePreparedNetworkPayload(p)).toThrow(/segmentKinds/);
  });
});
