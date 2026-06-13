import type { RegionAssignmentMode } from "../core/types";

export const PREPARED_NETWORK_VERSION = 1;

export interface PreparedNetworkRequest {
  sequence: number;
  n: number;
  k: number;
  seed: number;
  regionAssignmentMode: RegionAssignmentMode;
  visualSettings: Float32Array;
  morphConfigJson: string;
}

export interface PreparedNetworkPayload {
  version: number;
  sequence: number;
  n: number;
  k: number;
  seed: number;
  regionAssignmentMode: RegionAssignmentMode;
  gridDim: number;
  gridCellSize: number;
  droppedCount: number;
  positions: Float32Array;
  regionCodes: Uint8Array;
  gridMin: Float32Array;
  gridCellStart: Uint32Array;
  gridCellNeurons: Uint32Array;
  vertices: Float32Array;
  faces: Uint32Array;
  segmentEndpoints: Float32Array;
  segmentPathLen: Float32Array;
  segmentNeuronIds: Uint32Array;
  segmentKinds: Uint32Array;
  segmentTargetIds: Uint32Array;
  sphereGeometry: Float32Array;
  sphereNeuronIds: Uint32Array;
  sphereKinds: Uint32Array;
  statsJson: string;
  paramsJson: string;
  visualSettings: Float32Array;
  morphConfigJson: string;
}

export function validatePreparedNetworkPayload(payload: PreparedNetworkPayload): void {
  if (payload.version !== PREPARED_NETWORK_VERSION) {
    throw new Error(`prepared payload version ${payload.version} != ${PREPARED_NETWORK_VERSION}`);
  }
  if (!Number.isInteger(payload.n) || payload.n <= 0) throw new Error("prepared payload N invalid");
  if (!Number.isInteger(payload.k) || payload.k <= 0) throw new Error("prepared payload K invalid");
  if (
    payload.regionAssignmentMode !== "hash-random"
    && payload.regionAssignmentMode !== "anterior-posterior-prototype"
  ) {
    throw new Error("prepared payload regionAssignmentMode invalid");
  }
  if (!Number.isInteger(payload.gridDim) || payload.gridDim <= 0) throw new Error("prepared payload gridDim invalid");
  if (!Number.isFinite(payload.gridCellSize) || payload.gridCellSize <= 0) throw new Error("prepared payload gridCellSize invalid");
  expectLen("positions", payload.positions.length, payload.n * 3);
  expectLen("regionCodes", payload.regionCodes.length, payload.n);
  expectLen("gridMin", payload.gridMin.length, 3);
  expectLen("gridCellStart", payload.gridCellStart.length, payload.gridDim ** 3 + 1);
  expectLen("gridCellNeurons", payload.gridCellNeurons.length, payload.n);
  if (payload.vertices.length === 0 || payload.vertices.length % 3 !== 0) {
    throw new Error("prepared payload vertices length invalid");
  }
  if (payload.faces.length === 0 || payload.faces.length % 3 !== 0) {
    throw new Error("prepared payload faces length invalid");
  }
  const segmentCount = payload.segmentPathLen.length;
  expectLen("segmentEndpoints", payload.segmentEndpoints.length, segmentCount * 8);
  expectLen("segmentNeuronIds", payload.segmentNeuronIds.length, segmentCount);
  expectLen("segmentKinds", payload.segmentKinds.length, segmentCount);
  expectLen("segmentTargetIds", payload.segmentTargetIds.length, segmentCount);
  const sphereCount = payload.sphereNeuronIds.length;
  expectLen("sphereGeometry", payload.sphereGeometry.length, sphereCount * 8);
  expectLen("sphereKinds", payload.sphereKinds.length, sphereCount);
  if (payload.gridCellStart[0] !== 0 || payload.gridCellStart[payload.gridCellStart.length - 1] !== payload.n) {
    throw new Error("prepared payload grid CSR does not span 0..N");
  }
  for (let i = 1; i < payload.gridCellStart.length; i++) {
    if (payload.gridCellStart[i - 1] > payload.gridCellStart[i]) {
      throw new Error("prepared payload grid CSR offsets are not monotonic");
    }
  }
  for (const code of payload.regionCodes) {
    if (code > 2) throw new Error(`prepared payload region code ${code} invalid`);
  }
}

export function preparedNetworkTransferList(payload: PreparedNetworkPayload): Transferable[] {
  return [
    payload.positions.buffer,
    payload.regionCodes.buffer,
    payload.gridMin.buffer,
    payload.gridCellStart.buffer,
    payload.gridCellNeurons.buffer,
    payload.vertices.buffer,
    payload.faces.buffer,
    payload.segmentEndpoints.buffer,
    payload.segmentPathLen.buffer,
    payload.segmentNeuronIds.buffer,
    payload.segmentKinds.buffer,
    payload.segmentTargetIds.buffer,
    payload.sphereGeometry.buffer,
    payload.sphereNeuronIds.buffer,
    payload.sphereKinds.buffer,
    payload.visualSettings.buffer,
  ];
}

function expectLen(name: string, actual: number, expected: number): void {
  if (actual !== expected) {
    throw new Error(`prepared payload ${name} length ${actual} != ${expected}`);
  }
}
