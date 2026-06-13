import { describe, expect, it } from "vitest";
import { formatSubStageLabel, mapSubStageProgress } from "./boot-overlay";

describe("formatSubStageLabel", () => {
  it("appends the within-stage percent to the label", () => {
    expect(formatSubStageLabel("Prepare network payload", 0)).toBe(
      "Prepare network payload 0%",
    );
    expect(formatSubStageLabel("Prepare network payload", 0.42)).toBe(
      "Prepare network payload 42%",
    );
    expect(formatSubStageLabel("Prepare network payload", 1)).toBe(
      "Prepare network payload 100%",
    );
  });

  it("rounds to whole percent", () => {
    expect(formatSubStageLabel("Growing morphology", 0.851)).toBe("Growing morphology 85%");
    expect(formatSubStageLabel("Growing morphology", 0.855)).toBe("Growing morphology 86%");
  });

  it("clamps out-of-range fractions to [0,1]", () => {
    expect(formatSubStageLabel("Requesting GPU adapter…", -0.5)).toBe(
      "Requesting GPU adapter… 0%",
    );
    expect(formatSubStageLabel("Requesting GPU adapter…", 1.7)).toBe(
      "Requesting GPU adapter… 100%",
    );
  });

  it("renders the real worker payload phase fractions as climbing labels", () => {
    // Mirrors the Rust prepare_with_progress phase emit() fractions.
    const phases: Array<[string, number]> = [
      ["Folding manifold", 0.15],
      ["Assigning source types", 0.25],
      ["Growing morphology", 0.85],
      ["Emitting soma spheres", 1.0],
    ];
    const labels = phases.map(([phase, f]) => formatSubStageLabel(phase, f));
    expect(labels).toEqual([
      "Folding manifold 15%",
      "Assigning source types 25%",
      "Growing morphology 85%",
      "Emitting soma spheres 100%",
    ]);
  });
});

describe("mapSubStageProgress", () => {
  it("maps fraction onto the [start, end] band", () => {
    expect(mapSubStageProgress(0, 54, 96)).toBe(54);
    expect(mapSubStageProgress(1, 54, 96)).toBe(96);
    expect(mapSubStageProgress(0.5, 54, 96)).toBe(75);
  });

  it("clamps out-of-range fractions to the band", () => {
    expect(mapSubStageProgress(-1, 54, 96)).toBe(54);
    expect(mapSubStageProgress(2, 54, 96)).toBe(96);
  });

  it("is monotonic across increasing fractions", () => {
    let prev = -Infinity;
    for (const f of [0, 0.15, 0.25, 0.85, 1]) {
      const p = mapSubStageProgress(f, 54, 60);
      expect(p).toBeGreaterThanOrEqual(prev);
      prev = p;
    }
  });
});
