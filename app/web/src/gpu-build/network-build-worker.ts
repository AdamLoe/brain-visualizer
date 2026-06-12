import init, * as wasm from "../../../crates/brain-visualizer/pkg/brain_visualizer.js";
import {
  preparedNetworkTransferList,
  validatePreparedNetworkPayload,
  type PreparedNetworkPayload,
  type PreparedNetworkRequest,
} from "./prepared-network";

type WorkerIn = { type: "prepare"; request: PreparedNetworkRequest };
type WorkerOut =
  | { type: "ready"; payload: PreparedNetworkPayload }
  | { type: "failed"; sequence: number; message: string };

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerIn>) => void) | null;
  postMessage(message: WorkerOut, transfer?: Transferable[]): void;
}

interface WasmPreparedNetwork {
  version(): number;
  n(): number;
  k(): number;
  seed(): number;
  grid_dim(): number;
  grid_cell_size(): number;
  dropped_count(): number;
  positions(): Float32Array;
  region_codes(): Uint8Array;
  vertices(): Float32Array;
  faces(): Uint32Array;
  grid_min(): Float32Array;
  grid_cell_start(): Uint32Array;
  grid_cell_neurons(): Uint32Array;
  segment_endpoints(): Float32Array;
  segment_path_len(): Float32Array;
  segment_neuron_ids(): Uint32Array;
  segment_kinds(): Uint32Array;
  segment_target_ids(): Uint32Array;
  sphere_geometry(): Float32Array;
  sphere_neuron_ids(): Uint32Array;
  sphere_kinds(): Uint32Array;
  stats_json(): string;
  params_json(): string;
}

interface WasmGpuBuildModule {
  prepare_network_payload(
    n: number,
    k: number,
    seed: number,
    visualSettings: Float32Array,
    morphConfigJson: string,
  ): WasmPreparedNetwork;
}

let wasmReady: Promise<void> | null = null;
const workerScope = self as unknown as WorkerScope;

workerScope.onmessage = (event: MessageEvent<WorkerIn>) => {
  if (event.data.type !== "prepare") return;
  void prepare(event.data.request);
};

async function prepare(request: PreparedNetworkRequest): Promise<void> {
  try {
    wasmReady ??= init().then(() => undefined);
    await wasmReady;
    const module = wasm as unknown as WasmGpuBuildModule;
    const prepared = module.prepare_network_payload(
      request.n,
      request.k,
      request.seed >>> 0,
      request.visualSettings,
      request.morphConfigJson,
    );
    const payload: PreparedNetworkPayload = {
      version: prepared.version(),
      sequence: request.sequence,
      n: prepared.n(),
      k: prepared.k(),
      seed: prepared.seed() >>> 0,
      gridDim: prepared.grid_dim(),
      gridCellSize: prepared.grid_cell_size(),
      droppedCount: prepared.dropped_count(),
      positions: prepared.positions(),
      regionCodes: prepared.region_codes(),
      gridMin: prepared.grid_min(),
      gridCellStart: prepared.grid_cell_start(),
      gridCellNeurons: prepared.grid_cell_neurons(),
      vertices: prepared.vertices(),
      faces: prepared.faces(),
      segmentEndpoints: prepared.segment_endpoints(),
      segmentPathLen: prepared.segment_path_len(),
      segmentNeuronIds: prepared.segment_neuron_ids(),
      segmentKinds: prepared.segment_kinds(),
      segmentTargetIds: prepared.segment_target_ids(),
      sphereGeometry: prepared.sphere_geometry(),
      sphereNeuronIds: prepared.sphere_neuron_ids(),
      sphereKinds: prepared.sphere_kinds(),
      statsJson: prepared.stats_json(),
      paramsJson: prepared.params_json(),
      visualSettings: new Float32Array(request.visualSettings),
      morphConfigJson: request.morphConfigJson,
    };
    validatePreparedNetworkPayload(payload);
    workerScope.postMessage(
      { type: "ready", payload } satisfies WorkerOut,
      preparedNetworkTransferList(payload),
    );
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    workerScope.postMessage({ type: "failed", sequence: request.sequence, message } satisfies WorkerOut);
  }
}
