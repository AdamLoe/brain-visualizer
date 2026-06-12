import { describe, expect, test } from "vitest";

import { ESTIMATED_METRIC_LABELS } from "./dev-panel";
import { CornerHud } from "./hud";

class FakeElement {
  id = "";
  textContent = "";
  style: { cssText?: string; display?: string } = {};
}

describe("estimated metric labels", () => {
  test("dev-panel labels derived metrics as estimates", () => {
    expect(ESTIMATED_METRIC_LABELS.synapticEventsPerSec).toBe("Syn. events/sec (est.)");
    expect(ESTIMATED_METRIC_LABELS.cascadeSizeNow).toBe("Cascade size now (approx)");
  });

  test("corner HUD labels synaptic events per second as estimated", () => {
    const appended: FakeElement[] = [];
    const previousDocument = globalThis.document;
    Object.defineProperty(globalThis, "document", {
      configurable: true,
      value: {
        body: {
          appendChild: (el: FakeElement) => { appended.push(el); },
        },
        createElement: () => new FakeElement(),
      },
    });

    try {
      const hud = new CornerHud();
      hud.update({
        fps: 60,
        n: 1200,
        backend: "gpu",
        synapticEventsPerSec: 1_200_000,
      });

      expect(appended).toHaveLength(1);
      expect(appended[0].textContent).toContain("syn/s est: 1.2M");
    } finally {
      Object.defineProperty(globalThis, "document", {
        configurable: true,
        value: previousDocument,
      });
    }
  });
});
