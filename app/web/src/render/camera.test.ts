import { describe, expect, it } from "vitest";
import { Camera } from "./camera";

describe("Camera pan target", () => {
  it("translates the eye by the expected screen-space delta", () => {
    const camera = new Camera();
    camera.setAspect(16 / 9);

    const eyeBefore = camera.eye();
    const right = camera.cameraRight();
    const up = camera.cameraUp();

    const dx = 24;
    const dy = -12;
    const viewportHeight = 900;
    const worldPerPixel = (2 * 3.0 * Math.tan((50 * Math.PI) / 360)) / viewportHeight;
    const expected = [
      (-right[0] * dx + up[0] * dy) * worldPerPixel,
      (-right[1] * dx + up[1] * dy) * worldPerPixel,
      (-right[2] * dx + up[2] * dy) * worldPerPixel,
    ];

    camera.pan(dx, dy, 1600, viewportHeight);

    const eyeAfter = camera.eye();
    expect(eyeAfter[0] - eyeBefore[0]).toBeCloseTo(expected[0], 10);
    expect(eyeAfter[1] - eyeBefore[1]).toBeCloseTo(expected[1], 10);
    expect(eyeAfter[2] - eyeBefore[2]).toBeCloseTo(expected[2], 10);
  });

  it("keeps hover separate from drag and supports resetTarget", () => {
    const camera = new Camera();
    camera.setAspect(1);

    expect(camera.onPointerMove(10, 10, 0, 800, 600)).toBe(false);

    const eyeBefore = camera.eye();
    camera.onPointerDown(10, 10, 2, false);
    expect(camera.onPointerMove(30, 18, 2, 800, 600)).toBe(true);

    const eyeAfterPan = camera.eye();
    expect(
      Math.abs(eyeAfterPan[0] - eyeBefore[0]) +
      Math.abs(eyeAfterPan[1] - eyeBefore[1]) +
      Math.abs(eyeAfterPan[2] - eyeBefore[2]),
    ).toBeGreaterThan(0);

    camera.resetTarget();
    const eyeAfterReset = camera.eye();
    expect(eyeAfterReset[0]).toBeCloseTo(eyeBefore[0], 10);
    expect(eyeAfterReset[1]).toBeCloseTo(eyeBefore[1], 10);
    expect(eyeAfterReset[2]).toBeCloseTo(eyeBefore[2], 10);
  });
});
