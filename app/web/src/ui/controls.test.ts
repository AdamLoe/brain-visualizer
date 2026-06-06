// Unit tests for pure control / scaler logic (Phase 5).
// Run with: npx vitest run  (or: npm test)
// No browser DOM required — pure logic only.

import { describe, it, expect, beforeEach } from "vitest";
import {
  ticksThisFrame,
  tickExcitability,
  setExcitabilityForTest,
  getCurrentExcitability,
  BRAIN_STATES,
  scalerDecide,
  N_MIN,
  N_MAX,
} from "./controls";

// ─────────────────────────────────────────────────────────────────────────────
// ticksThisFrame — BV14 speed preset → ticks mapping
// ─────────────────────────────────────────────────────────────────────────────
describe("ticksThisFrame", () => {
  it("quarter: fires exactly 1 tick on frames 0,4,8,12 and 0 on others", () => {
    expect(ticksThisFrame("quarter", 0)).toBe(1);
    expect(ticksThisFrame("quarter", 1)).toBe(0);
    expect(ticksThisFrame("quarter", 2)).toBe(0);
    expect(ticksThisFrame("quarter", 3)).toBe(0);
    expect(ticksThisFrame("quarter", 4)).toBe(1);
    expect(ticksThisFrame("quarter", 7)).toBe(0);
    expect(ticksThisFrame("quarter", 8)).toBe(1);
    expect(ticksThisFrame("quarter", 12)).toBe(1);
  });

  it("half: fires on every even frame, 0 on odd", () => {
    expect(ticksThisFrame("half", 0)).toBe(1);
    expect(ticksThisFrame("half", 1)).toBe(0);
    expect(ticksThisFrame("half", 2)).toBe(1);
    expect(ticksThisFrame("half", 99)).toBe(0);
    expect(ticksThisFrame("half", 100)).toBe(1);
  });

  it("normal: always 1 regardless of frameCounter", () => {
    for (const fc of [0, 1, 7, 100, 9999]) {
      expect(ticksThisFrame("normal", fc)).toBe(1);
    }
  });

  it("double: always 2 regardless of frameCounter", () => {
    for (const fc of [0, 1, 7, 100, 9999]) {
      expect(ticksThisFrame("double", fc)).toBe(2);
    }
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// tickExcitability — smooth lerp toward target (EXCITABILITY_LERP = 0.08)
// ─────────────────────────────────────────────────────────────────────────────
describe("tickExcitability", () => {
  const LERP = 0.08;

  beforeEach(() => {
    // Reset module state before each test.
    setExcitabilityForTest(BRAIN_STATES.focused, BRAIN_STATES.focused);
  });

  it("stays at current value when already at target", () => {
    const start = BRAIN_STATES.focused;
    setExcitabilityForTest(start, start);
    for (let i = 0; i < 10; i++) {
      const v = tickExcitability();
      expect(v).toBeCloseTo(start, 6);
    }
  });

  it("moves toward target by LERP fraction each frame", () => {
    const from = BRAIN_STATES.focused;   // 0.55
    const to   = BRAIN_STATES.seizure;   // 1.00
    setExcitabilityForTest(from, to);
    const v1 = tickExcitability();
    // After 1 frame: from + (to - from) * LERP
    expect(v1).toBeCloseTo(from + (to - from) * LERP, 6);
    const v2 = tickExcitability();
    expect(v2).toBeCloseTo(v1 + (to - v1) * LERP, 6);
  });

  it("converges to target within 200 frames", () => {
    const from = BRAIN_STATES.deep_sleep;  // 0.10
    const to   = BRAIN_STATES.seizure;     // 1.00
    setExcitabilityForTest(from, to);
    for (let i = 0; i < 200; i++) tickExcitability();
    // After 200 steps it should be within 0.001 of target
    const remaining = Math.abs(getCurrentExcitability() - to);
    expect(remaining).toBeLessThan(0.001);
  });

  it("converges from seizure back to deep_sleep", () => {
    const from = BRAIN_STATES.seizure;     // 1.00
    const to   = BRAIN_STATES.deep_sleep;  // 0.10
    setExcitabilityForTest(from, to);
    for (let i = 0; i < 200; i++) tickExcitability();
    expect(Math.abs(getCurrentExcitability() - to)).toBeLessThan(0.001);
  });

  it("never overshoots target (monotone approach)", () => {
    setExcitabilityForTest(0.1, 1.0);
    let prev = getCurrentExcitability();
    for (let i = 0; i < 100; i++) {
      const v = tickExcitability();
      expect(v).toBeGreaterThanOrEqual(prev);  // monotone increase
      expect(v).toBeLessThanOrEqual(1.0);       // never overshoots
      prev = v;
    }
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// scalerDecide — adaptive scaler decision (pure function)
// ─────────────────────────────────────────────────────────────────────────────
describe("scalerDecide", () => {
  const BUDGET = 14;         // ms
  const COOLDOWN = 3000;     // ms

  // Helper: enough time since last resize
  const LONG_AGO = COOLDOWN + 1;

  it("returns none during restart", () => {
    const action = scalerDecide(20, 50_000, "balanced", LONG_AGO, true);
    expect(action.kind).toBe("none");
  });

  it("returns none within cooldown window", () => {
    const action = scalerDecide(20, 50_000, "balanced", COOLDOWN - 1, false);
    expect(action.kind).toBe("none");
  });

  it("shrinks N when p95 is over budget", () => {
    const action = scalerDecide(BUDGET + 1, 50_000, "balanced", LONG_AGO, false);
    expect(action.kind).toBe("shrink_n");
    if (action.kind === "shrink_n") {
      expect(action.newN).toBe(Math.floor(50_000 * 0.9));
    }
  });

  it("grows N when p95 is well under budget (< 70%)", () => {
    const action = scalerDecide(BUDGET * 0.5, 50_000, "balanced", LONG_AGO, false);
    expect(action.kind).toBe("grow_n");
    if (action.kind === "grow_n") {
      expect(action.newN).toBe(Math.floor(50_000 * 1.1));
    }
  });

  it("returns none when p95 is in the 70–100% budget zone (no action)", () => {
    const p95InBand = BUDGET * 0.75;
    const action = scalerDecide(p95InBand, 50_000, "balanced", LONG_AGO, false);
    expect(action.kind).toBe("none");
  });

  it("clamps shrink to N_MIN for tier", () => {
    // Provide an N that is just above N_MIN so one step might undershoot.
    const justAboveMin = N_MIN.balanced + 1;
    const action = scalerDecide(BUDGET + 5, justAboveMin, "balanced", LONG_AGO, false);
    expect(action.kind).toBe("shrink_n");
    if (action.kind === "shrink_n") {
      expect(action.newN).toBeGreaterThanOrEqual(N_MIN.balanced);
    }
  });

  it("does not shrink below N_MIN", () => {
    const action = scalerDecide(BUDGET + 5, N_MIN.balanced, "balanced", LONG_AGO, false);
    expect(action.kind).toBe("none");  // already at floor
  });

  it("clamps grow to N_MAX for tier", () => {
    const justBelowMax = N_MAX.low - 1;
    const action = scalerDecide(0, justBelowMax, "low", LONG_AGO, false);
    expect(action.kind).toBe("grow_n");
    if (action.kind === "grow_n") {
      expect(action.newN).toBeLessThanOrEqual(N_MAX.low);
    }
  });

  it("does not grow above N_MAX", () => {
    const action = scalerDecide(0, N_MAX.max, "max", LONG_AGO, false);
    expect(action.kind).toBe("none");
  });

  it("never changes tier — low tier bounds respected", () => {
    // Below N_MIN.balanced = 30k → would not fire for balanced,
    // but for low tier 20k is within range.
    const action = scalerDecide(BUDGET + 5, 20_000, "low", LONG_AGO, false);
    if (action.kind === "shrink_n") {
      expect(action.newN).toBeGreaterThanOrEqual(N_MIN.low);
      expect(action.newN).toBeLessThanOrEqual(N_MAX.low);
    }
  });

  it("never changes tier — max tier bounds respected when growing", () => {
    const action = scalerDecide(0, 500_000, "max", LONG_AGO, false);
    if (action.kind === "grow_n") {
      expect(action.newN).toBeLessThanOrEqual(N_MAX.max);
    }
  });

  it("exactly at N_MIN with over-budget → none (cannot shrink further)", () => {
    const action = scalerDecide(BUDGET + 1, N_MIN.low, "low", LONG_AGO, false);
    expect(action.kind).toBe("none");
  });

  it("exactly at N_MAX with under-budget → none (cannot grow further)", () => {
    const action = scalerDecide(0, N_MAX.max, "max", LONG_AGO, false);
    expect(action.kind).toBe("none");
  });
});
