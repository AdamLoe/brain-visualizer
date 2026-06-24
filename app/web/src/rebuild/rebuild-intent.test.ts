import { describe, expect, test } from "vitest";

import { DEFAULT_SETTINGS, type VisualizerSettings } from "../core/settings";
import { settingsRequirePreparedNetwork } from "./rebuild-intent";

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
});
