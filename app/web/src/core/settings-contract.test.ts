import { describe, expect, test } from "vitest";

import {
  DEFAULT_SETTINGS,
  METRICS_LAYOUT,
  METRICS_LENGTH,
  METRICS_SCALAR_COUNT,
  SETTINGS_LENGTH,
  VOLTAGE_HISTOGRAM_BINS,
  parseMetrics,
  toFloat32Array,
  type VisualizerSettings,
} from "./settings";

function expectFloatArrayClose(actual: Float32Array, expected: readonly number[]): void {
  expect(actual.length).toBe(expected.length);
  expected.forEach((value, index) => {
    expect(actual[index]).toBeCloseTo(value, 6);
  });
}

describe("VisualSettings Float32Array contract", () => {
  test("default settings serialize to the locked 27-slot layout", () => {
    expect(SETTINGS_LENGTH).toBe(27);
    expectFloatArrayClose(toFloat32Array(DEFAULT_SETTINGS), [
      10.0,
      0.004,
      0.004,
      2.0,
      1.0,
      0.0,
      0.80,
      0.15,
      1.0,
      0.0,
      0.0,
      1.0,
      0.014,
      0.03,
      0.50,
      0.0,
      0.0,
      1.0,
      6.0,
      0.0,
      0.0,
      1.0,
      0.0,
      0.0,
      0.14,
      14.0,
      30.0,
    ]);
  });

  test("reserved slots are zero-written and quarantined slots are default-written", () => {
    const settings: VisualizerSettings = {
      ...DEFAULT_SETTINGS,
      pointRadius: 0.02,
      bloomStrength: 0.7,
      surfaceOpacity: 0.25,
      signalSource: 4,
      surface: 2,
      adaptiveScalerEnabled: 1,
    };

    const values = toFloat32Array(settings);

    expect(values[1]).toBeCloseTo(DEFAULT_SETTINGS.pointRadius, 6);
    expect(values[9]).toBe(0);
    expect(values[10]).toBe(0);
    expect(values[11]).toBe(DEFAULT_SETTINGS.surfaceOpacity);
    expect(values[16]).toBe(0);
    expect(values[20]).toBe(DEFAULT_SETTINGS.surface);
    expect(values[23]).toBe(0);
  });
});

describe("metrics array contract", () => {
  test("scalar layout and histogram offset are locked", () => {
    expect(VOLTAGE_HISTOGRAM_BINS).toBe(16);
    expect(METRICS_SCALAR_COUNT).toBe(17);
    expect(METRICS_LENGTH).toBe(33);
    expect(METRICS_LAYOUT).toEqual([
      "spikesThisTick",
      "spikesPerSec",
      "meanFiringRateHz",
      "synapticEventsPerSec",
      "meanMembraneVoltage",
      "inputSpikes",
      "assocSpikes",
      "outputSpikes",
      "eSpikes",
      "iSpikes",
      "pctFired100ms",
      "pctFired500ms",
      "pctFired2s",
      "branchingRatio",
      "timeSinceLastLargeCascade",
      "refractoryBlockedAttempts",
      "currentAccumulatorHighWater",
    ]);
  });

  test("parseMetrics maps scalars and 16 histogram bins by index", () => {
    const data = new Float32Array(METRICS_LENGTH);
    data.forEach((_, index) => {
      data[index] = index + 1;
    });

    const parsed = parseMetrics(data);

    expect(parsed.spikesThisTick).toBe(1);
    expect(parsed.spikesPerSec).toBe(2);
    expect(parsed.meanFiringRateHz).toBe(3);
    expect(parsed.synapticEventsPerSec).toBe(4);
    expect(parsed.meanMembraneVoltage).toBe(5);
    expect(parsed.inputSpikes).toBe(6);
    expect(parsed.assocSpikes).toBe(7);
    expect(parsed.outputSpikes).toBe(8);
    expect(parsed.eSpikes).toBe(9);
    expect(parsed.iSpikes).toBe(10);
    expect(parsed.pctFired100ms).toBe(11);
    expect(parsed.pctFired500ms).toBe(12);
    expect(parsed.pctFired2s).toBe(13);
    expect(parsed.branchingRatio).toBe(14);
    expect(parsed.timeSinceLastLargeCascade).toBe(15);
    expect(parsed.refractoryBlockedAttempts).toBe(16);
    expect(parsed.currentAccumulatorHighWater).toBe(17);
    expect(parsed.voltageHistogram).toEqual([
      18, 19, 20, 21, 22, 23, 24, 25,
      26, 27, 28, 29, 30, 31, 32, 33,
    ]);
  });

  test("parseMetrics treats missing trailing entries as zero", () => {
    const parsed = parseMetrics(new Float32Array([5, 6]));

    expect(parsed.spikesThisTick).toBe(5);
    expect(parsed.spikesPerSec).toBe(6);
    expect(parsed.meanFiringRateHz).toBe(0);
    expect(parsed.voltageHistogram).toHaveLength(16);
    expect(parsed.voltageHistogram.every((value) => value === 0)).toBe(true);
  });
});
