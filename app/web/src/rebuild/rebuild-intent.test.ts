import { describe, expect, test } from "vitest";

import { DEFAULT_MORPH_CONFIG, morphConfigToJson } from "../core/morph-config";
import { DEFAULT_SETTINGS, type VisualizerSettings } from "../core/settings";
import {
  morphConfigRequiresPreparedNetwork,
  settingsRequirePreparedNetwork,
} from "./rebuild-intent";

function settings(overrides: Partial<VisualizerSettings>): VisualizerSettings {
  return { ...DEFAULT_SETTINGS, ...overrides };
}

describe("rebuild intent classification", () => {
  test("routes curve and reach settings to prepared network rebuilds", () => {
    expect(settingsRequirePreparedNetwork(
      settings({ connectionCurveLift: 0.15 }),
      settings({ connectionCurveLift: 0.25 }),
    )).toBe(true);
    expect(settingsRequirePreparedNetwork(
      settings({ longRangeReachFrac: 0.14 }),
      settings({ longRangeReachFrac: 0.25 }),
    )).toBe(true);
    expect(settingsRequirePreparedNetwork(
      settings({ maxReachCells: 14 }),
      settings({ maxReachCells: 12 }),
    )).toBe(true);
  });

  test("keeps uniform-only settings on the immediate update path", () => {
    expect(settingsRequirePreparedNetwork(
      settings({ glowTau: 10 }),
      settings({ glowTau: 12 }),
    )).toBe(false);
  });

  test("routes generator config changes to prepared network rebuilds", () => {
    const next = structuredClone(DEFAULT_MORPH_CONFIG);
    next.generator.baseRadius = 0.008;

    expect(morphConfigRequiresPreparedNetwork(
      morphConfigToJson(DEFAULT_MORPH_CONFIG),
      morphConfigToJson(next),
    )).toBe(true);
  });

  test("keeps render-quality and lighting config changes immediate", () => {
    const renderQuality = structuredClone(DEFAULT_MORPH_CONFIG);
    renderQuality.renderQuality.tubeSides = 8;
    const lighting = structuredClone(DEFAULT_MORPH_CONFIG);
    lighting.lighting.activeBoost = 2.5;

    expect(morphConfigRequiresPreparedNetwork(
      morphConfigToJson(DEFAULT_MORPH_CONFIG),
      morphConfigToJson(renderQuality),
    )).toBe(false);
    expect(morphConfigRequiresPreparedNetwork(
      morphConfigToJson(DEFAULT_MORPH_CONFIG),
      morphConfigToJson(lighting),
    )).toBe(false);
  });
});
