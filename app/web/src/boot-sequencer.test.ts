/**
 * GPU-free integration test for the staged boot sequencer.
 *
 * Drives the REAL `runGpuStartup` orchestration (the same one `main.ts` uses)
 * with a stubbed WebGPU backend (stage methods resolve immediately) and a REAL
 * `NetworkBuildClient` backed by a controllable MockWorker. This exercises the
 * actual `runStage` / `onSubStage` / `onProgress` wiring without a WebGPU
 * adapter, so it reproduces the boot-time ordering that the isolated helper
 * tests missed.
 *
 * Reproduction target: on the pre-fix code the worker (warmed early) emits its
 * payload-build progress ticks while the "Acquire GPU" stage is still active —
 * before the payload-progress listener was attached — so every tick is dropped
 * and the "Prepare network payload" label sits at 0% then jumps to 100%, never
 * climbing. These tests assert the label CLIMBS through an intermediate percent
 * in both orderings (worker still generating when the stage starts; worker
 * finished during acquire).
 */

import { describe, expect, test } from "vitest";
import { NetworkBuildClient } from "./gpu-build/network-build-client";
import { runGpuStartup, type StagedBackendLike } from "./boot-sequencer";
import type {
  PreparedNetworkPayload,
  PreparedNetworkRequest,
} from "./gpu-build/prepared-network";

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

/** A fake staged backend whose stage methods all resolve immediately. */
function fakeStagedBackend(): StagedBackendLike {
  return {
    startup_upload_neuron_buffers() {},
    startup_upload_render_resources() {},
    startup_allocate_lod_edge_resources() {},
    startup_upload_morphology() {},
    startup_finish_network() {},
    startup_build_render_pipelines() {},
    startup_resize_render_targets() {},
    set_progress_callback() {},
  };
}

/** Capture every overlay update so we can assert the label timeline. */
interface OverlaySample {
  stage?: string;
  progress?: number;
}

/**
 * Build a harness: a controllable acquire gate (so the test can decide whether
 * the GPU acquire finishes before or after the worker), a real client, and a
 * recorder of overlay samples. `nextFrame` is a microtask flush so the boot loop
 * is deterministic and fast.
 */
function harness(opts: {
  /** Resolves the create_staged acquire stage when the test calls it. */
  acquire: Promise<void>;
}) {
  const worker = new MockWorker();
  const client = new NetworkBuildClient(() => worker as unknown as Worker);
  const samples: OverlaySample[] = [];
  let startupSequence = 0;

  const startPromise = runGpuStartup({
    factory: {
      create: async () => fakeStagedBackend(),
      create_staged: async () => {
        await opts.acquire;
        return fakeStagedBackend();
      },
    },
    networkClient: client,
    requestStartupNetwork: () => {
      startupSequence = 1;
      client.request(request(1));
      return startupSequence;
    },
    updateOverlay: (u) => samples.push({ ...u }),
    nextFrame: () => Promise.resolve(),
    stagePreparedPayload: () => {},
    log: () => {},
  });

  return { worker, client, samples, startPromise };
}

/** Pull the percent out of "Prepare network payload NN%" labels in order. */
function payloadPercents(samples: OverlaySample[]): number[] {
  const out: number[] = [];
  for (const s of samples) {
    const m = s.stage?.match(/^Prepare network payload (\d+)%$/);
    if (m) out.push(Number(m[1]));
  }
  return out;
}

describe("boot sequencer — network payload progress", () => {
  test("label climbs through intermediate percents while the worker is still generating when the stage starts", async () => {
    // GPU acquire finishes immediately so the boot reaches the payload stage
    // while the worker is mid-generation.
    const h = harness({ acquire: Promise.resolve() });

    // Let acquire complete and the payload stage become active.
    await flush();

    // Worker emits real phase ticks (folding → source-types → morphology), then
    // finishes. These arrive while the payload stage is active.
    h.worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "folding manifold", fraction: 0.15 });
    h.worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "source types", fraction: 0.25 });
    h.worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "growing morphology", fraction: 0.85 });
    await flush();
    h.worker.emit({ type: "ready", payload: payload(1) });

    await h.startPromise;

    const percents = payloadPercents(h.samples);
    // Must climb through intermediate values, not jump 0 → 100.
    expect(percents).toContain(15);
    expect(percents).toContain(85);
    expect(Math.max(...percents)).toBe(100);
    expect(percents[percents.length - 1]).toBe(100);
  });

  test("label still climbs when the worker finishes (emits all progress) DURING the GPU-acquire stage", async () => {
    // The realistic fast-box ordering: the warmed worker races ahead and emits
    // ALL its progress + ready BEFORE the GPU acquire resolves and the payload
    // stage becomes active. The pre-fix code dropped these ticks (no listener
    // attached yet) and the label sat at 0% until the payload landed, then
    // jumped — reproduced as the "stuck at 0%" stall the user reported.
    let resolveAcquire!: () => void;
    const acquire = new Promise<void>((r) => (resolveAcquire = r));
    const h = harness({ acquire });

    await flush();
    // Worker storms ahead during acquire.
    h.worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "folding manifold", fraction: 0.15 });
    h.worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "source types", fraction: 0.25 });
    h.worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "growing morphology", fraction: 0.85 });
    h.worker.emit({ type: "ready", payload: payload(1) });
    await flush();

    // Only now does the GPU finish acquiring and boot advances to the payload
    // stage, which finds the payload already ready.
    resolveAcquire();
    await h.startPromise;

    const percents = payloadPercents(h.samples);
    // The stage must reflect the worker's real progress (85% reached during
    // acquire) and complete at 100% — never sit at 0%.
    expect(percents).toContain(85);
    expect(Math.max(...percents)).toBe(100);
    expect(percents[percents.length - 1]).toBe(100);
  });
});

/** Flush pending microtasks/promise continuations a few turns. */
async function flush(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}
