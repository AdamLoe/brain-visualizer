import { describe, expect, test } from "vitest";

import {
  hasWebGpuSupport,
  webGpuStartupFailureStage,
  webGpuUnsupportedStage,
} from "./boot-failure";

describe("WebGPU failure copy", () => {
  test("detects missing navigator.gpu without assuming a fallback", () => {
    expect(hasWebGpuSupport({})).toBe(false);
    expect(hasWebGpuSupport({ gpu: {} })).toBe(true);
  });

  test("unsupported browsers get visitor-grade guidance", () => {
    const message = webGpuUnsupportedStage();

    expect(message).toContain("WebGPU");
    expect(message).toContain("Chrome or Edge");
    expect(message).toContain("hardware acceleration");
    expect(message).toContain("No CPU/WebGL fallback");
  });

  test("startup failures avoid exposing raw diagnostics as the product message", () => {
    const message = webGpuStartupFailureStage();

    expect(message).toContain("WebGPU");
    expect(message).toContain("graphics drivers");
    expect(message).not.toContain("requestAdapter");
    expect(message).not.toContain("panic");
  });
});
