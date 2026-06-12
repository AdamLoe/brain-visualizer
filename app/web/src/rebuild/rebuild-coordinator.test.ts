import { describe, expect, test } from "vitest";

import {
  RebuildCoordinator,
  type RebuildBackendOps,
  type RebuildFrameInputs,
} from "./rebuild-coordinator";

function makeHarness(): {
  coordinator: RebuildCoordinator;
  calls: string[];
  backend: RebuildBackendOps;
  inputs: RebuildFrameInputs;
} {
  const calls: string[] = [];
  const coordinator = new RebuildCoordinator();
  const backend: RebuildBackendOps = {
    reinitialize(n, k, seed, iExt, synapticScale) {
      calls.push(`network:${n}:${k}:${seed}:${iExt}:${synapticScale}`);
    },
    updateSettings(settings) {
      calls.push(`settings:${settings[0]}`);
    },
    applyMorphConfig(json) {
      calls.push(`morph:${json}`);
    },
  };
  const inputs: RebuildFrameInputs = {
    visualSettings: () => new Float32Array([42]),
    simulationSettings: () => ({ iExt: 0.1, synapticScale: 0.2 }),
    morphConfigJson: () => "saved-morph",
  };
  return { coordinator, calls, backend, inputs };
}

describe("RebuildCoordinator", () => {
  test("coalesces multiple network requests and stages post-rebuild pushes", () => {
    const { coordinator, calls, backend, inputs } = makeHarness();

    const first = coordinator.requestNetwork({ n: 1000, k: 8, seed: 1 });
    const second = coordinator.requestNetwork({ n: 2000, k: 16, seed: 2 });

    expect(second).toBeGreaterThan(first);
    expect(coordinator.applyNext(backend, inputs)).toEqual({
      kind: "network",
      request: { n: 2000, k: 16, seed: 2 },
      sequence: second,
    });
    expect(coordinator.applyNext(backend, inputs).kind).toBe("settings");
    expect(coordinator.applyNext(backend, inputs).kind).toBe("morphology");
    expect(coordinator.applyNext(backend, inputs).kind).toBe("idle");
    expect(calls).toEqual([
      "network:2000:16:2:0.1:0.2",
      "settings:42",
      "morph:saved-morph",
    ]);
  });

  test("keeps an explicit latest morph config across a network rebuild", () => {
    const { coordinator, calls, backend, inputs } = makeHarness();

    coordinator.requestMorphConfig("older");
    coordinator.requestNetwork({ n: 3000, k: 12, seed: 3 });
    coordinator.requestMorphConfig("newer");

    coordinator.applyNext(backend, inputs);
    coordinator.applyNext(backend, inputs);
    coordinator.applyNext(backend, inputs);

    expect(calls).toEqual([
      "network:3000:12:3:0.1:0.2",
      "settings:42",
      "morph:newer",
    ]);
  });

  test("prioritizes a newer network request over staged follow-up work", () => {
    const { coordinator, calls, backend, inputs } = makeHarness();

    coordinator.requestNetwork({ n: 1000, k: 8, seed: 1 });
    coordinator.applyNext(backend, inputs);
    coordinator.requestNetwork({ n: 4000, k: 20, seed: 4 });
    coordinator.applyNext(backend, inputs);
    coordinator.applyNext(backend, inputs);
    coordinator.applyNext(backend, inputs);

    expect(calls).toEqual([
      "network:1000:8:1:0.1:0.2",
      "network:4000:20:4:0.1:0.2",
      "settings:42",
      "morph:saved-morph",
    ]);
  });
});
