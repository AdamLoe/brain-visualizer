/**
 * GPU-free contract test for the boot overlay's "% only when real" label rule
 * (boot speed/observability).
 *
 * Drives the REAL `runGpuStartup` orchestration with a stubbed staged backend
 * and a REAL `NetworkBuildClient` backed by a controllable MockWorker — the same
 * seam `boot-sequencer.test.ts` uses. The worker emits a REALISTIC CONTINUOUS
 * cadence of payload-progress ticks (the now-continuous morphology sub-progress),
 * and the test asserts:
 *   1. the "Prepare network payload" label climbs through intermediate percents
 *      (real sub-progress is surfaced), and
 *   2. NO stage that lacks a real progress fraction ever shows an appended
 *      "NN%" — i.e. there is no SYNTHETIC within-step percent. Only the
 *      payload stage (continuous fraction) and the GPU-acquire sub-stages may
 *      append a percent; every other stage shows its bare label.
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

interface OverlaySample {
  stage?: string;
  progress?: number;
}

const PAYLOAD_LABEL = "Prepare network payload";

async function flush(): Promise<void> {
  for (let i = 0; i < 12; i++) await Promise.resolve();
}

/** The label of any stage that does NOT carry a real sub-progress fraction.
 * Such a stage must NEVER have an appended "NN%" (no synthetic within-step %). */
function hasAppendedPercent(stage: string | undefined): boolean {
  return stage !== undefined && /\s\d+%$/.test(stage);
}

describe("boot overlay label contract — % only when real", () => {
  test("payload label climbs through continuous percents; non-progress stages never show a fake %", async () => {
    const worker = new MockWorker();
    const client = new NetworkBuildClient(() => worker as unknown as Worker);
    const samples: OverlaySample[] = [];

    const startPromise = runGpuStartup({
      factory: {
        create: async () => fakeStagedBackend(),
        // Acquire resolves immediately so we reach the payload stage while the
        // worker is mid-generation. We deliberately do NOT call the onSubStage
        // callback here, so the acquire stage shows only its bare label — the
        // real GPU path would tick it, but the contract under test is that a
        // stage WITHOUT a fraction never invents one.
        create_staged: async () => fakeStagedBackend(),
      },
      networkClient: client,
      requestStartupNetwork: () => {
        client.request(request(1));
        return 1;
      },
      updateOverlay: (u) => samples.push({ ...u }),
      nextFrame: () => Promise.resolve(),
      stagePreparedPayload: () => {},
      log: () => {},
    });

    await flush();

    // Realistic continuous cadence: the now-continuous morphology sub-progress
    // emits many fractions across the 0.25..0.85 band, not just the four named
    // boundaries.
    const cadence = [0.05, 0.12, 0.18, 0.27, 0.36, 0.49, 0.61, 0.73, 0.85, 0.93];
    for (const f of cadence) {
      worker.emit({ type: "progress", sequence: 1, stage: "prepare-payload", phase: "x", fraction: f });
      await flush();
    }
    worker.emit({ type: "ready", payload: payload(1) });
    await startPromise;

    // 1. The payload label climbed through several distinct intermediate
    //    percents and ended at 100%.
    const payloadPercents = samples
      .map((s) => s.stage?.match(/^Prepare network payload (\d+)%$/)?.[1])
      .filter((m): m is string => m !== undefined)
      .map(Number);
    expect(new Set(payloadPercents).size).toBeGreaterThan(3);
    expect(Math.max(...payloadPercents)).toBe(100);
    // Monotonic non-decreasing — never snaps backward.
    for (let i = 1; i < payloadPercents.length; i++) {
      expect(payloadPercents[i]).toBeGreaterThanOrEqual(payloadPercents[i - 1]);
    }

    // 2. No stage OTHER than the payload stage carries an appended "NN%". The
    //    bare stage labels ("Upload neuron buffers", "Compile render
    //    pipelines", …) must show no synthetic within-step percent.
    for (const s of samples) {
      if (s.stage?.startsWith(PAYLOAD_LABEL)) continue;
      expect(
        hasAppendedPercent(s.stage),
        `stage "${s.stage}" must not show a synthetic within-step %`,
      ).toBe(false);
    }
  });
});
