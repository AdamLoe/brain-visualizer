import { describe, expect, test } from "vitest";

import { applyMobileConfig } from "./mobile-config";
import { DEFAULT_CONFIG, type AppConfig } from "./types";

describe("mobile config", () => {
  test("does not persist a heavier scale than the accepted default", () => {
    const config: AppConfig = {
      ...DEFAULT_CONFIG,
      n: 10_000,
      k: 64,
      tier: "max",
      regionAssignmentMode: "anterior-posterior-prototype",
    };

    applyMobileConfig(config);

    expect(config.n).toBe(DEFAULT_CONFIG.n);
    expect(config.k).toBe(DEFAULT_CONFIG.k);
    expect(config.tier).toBe(DEFAULT_CONFIG.tier);
    expect(config.regionAssignmentMode).toBe(DEFAULT_CONFIG.regionAssignmentMode);
  });

  test("preserves a user scale that is already lighter than the accepted default", () => {
    const config: AppConfig = {
      ...DEFAULT_CONFIG,
      n: 2_000,
    };

    applyMobileConfig(config);

    expect(config.n).toBe(2_000);
  });
});
