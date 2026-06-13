/**
 * Unit tests for the boot stall watchdog + timing recorder (boot speed/
 * observability). The stall decision is a pure function so it can be tested with
 * a fake clock and no DOM/timers; `startBootWatchdog` is exercised with injected
 * `now`/`warn` to assert the >2s warn fires exactly once per stuck step.
 */

import { afterEach, describe, expect, test } from "vitest";
import {
  BOOT_STALL_THRESHOLD_MS,
  evaluateStall,
  getBootTimings,
  recordBootTiming,
  startBootWatchdog,
  type StallState,
} from "./boot-timings";

afterEach(() => {
  // Reset the globalThis-backed timings array between tests (the same object
  // exposed as window.__bvBootTimings in the browser).
  (globalThis as unknown as { __bvBootTimings?: unknown }).__bvBootTimings = [];
});

describe("evaluateStall", () => {
  test("first sample seeds state and never warns", () => {
    const d = evaluateStall(null, { label: "Acquire GPU", percent: 54 }, 1000, 2000);
    expect(d.warning).toBeNull();
    expect(d.next.lastLabel).toBe("Acquire GPU");
    expect(d.next.unchangedSinceMs).toBe(1000);
    expect(d.next.warned).toBe(false);
  });

  test("a changed label or percent resets the stall clock (no warn)", () => {
    const seed: StallState = {
      lastLabel: "Acquire GPU",
      lastPercent: 54,
      unchangedSinceMs: 0,
      warned: false,
    };
    // 5s later but the PERCENT moved → not stalled, clock resets.
    const d = evaluateStall(seed, { label: "Acquire GPU", percent: 60 }, 5000, 2000);
    expect(d.warning).toBeNull();
    expect(d.next.unchangedSinceMs).toBe(5000);
  });

  test("warns once when label AND percent are unchanged past the threshold", () => {
    const seed: StallState = {
      lastLabel: "Prepare network payload",
      lastPercent: 25,
      unchangedSinceMs: 0,
      warned: false,
    };
    const same = { label: "Prepare network payload", percent: 25 };

    // 1.5s: under threshold → no warning yet.
    const d1 = evaluateStall(seed, same, 1500, 2000);
    expect(d1.warning).toBeNull();

    // 2.5s: over threshold → warn, and mark warned.
    const d2 = evaluateStall(seed, same, 2500, 2000);
    expect(d2.warning).toEqual({ label: "Prepare network payload", percent: 25 });
    expect(d2.next.warned).toBe(true);

    // Still stuck a poll later → no repeat warning (warned latch).
    const d3 = evaluateStall(d2.next, same, 3000, 2000);
    expect(d3.warning).toBeNull();
  });
});

describe("startBootWatchdog", () => {
  test("fires a single >2s warning for a genuinely stuck step", () => {
    let clock = 0;
    const warnings: string[] = [];
    // A step that never changes: label + percent are constant.
    const wd = startBootWatchdog(
      () => ({ label: "Prepare network payload", percent: 25 }),
      {
        thresholdMs: BOOT_STALL_THRESHOLD_MS,
        intervalMs: 1,
        now: () => clock,
        warn: (m) => warnings.push(m),
      },
    );

    // Drive several poll ticks across the threshold by advancing the fake clock.
    // setInterval callbacks run on real timers, so flush them by spinning the
    // event loop after each clock bump.
    return (async () => {
      for (const t of [500, 1500, 2500, 3500]) {
        clock = t;
        await new Promise((r) => setTimeout(r, 2));
      }
      wd.stop();
      expect(warnings.length).toBe(1);
      expect(warnings[0]).toContain("stalled >2s");
      expect(warnings[0]).toContain("at 25%");
    })();
  });

  test("never warns when the step keeps moving", async () => {
    let clock = 0;
    let percent = 25;
    const warnings: string[] = [];
    const wd = startBootWatchdog(
      () => ({ label: "Prepare network payload", percent }),
      { thresholdMs: 2000, intervalMs: 1, now: () => clock, warn: (m) => warnings.push(m) },
    );
    for (const t of [1000, 2000, 3000, 4000]) {
      clock = t;
      percent += 5; // the % advances every poll → never stalled
      await new Promise((r) => setTimeout(r, 2));
    }
    wd.stop();
    expect(warnings).toHaveLength(0);
  });
});

describe("recordBootTiming / getBootTimings", () => {
  test("accumulates rows on window.__bvBootTimings", () => {
    recordBootTiming("Load WASM module", 12.34);
    recordBootTiming("payload: axon", 410.7);
    const rows = getBootTimings();
    expect(rows).toEqual([
      { stage: "Load WASM module", ms: 12.3 },
      { stage: "payload: axon", ms: 410.7 },
    ]);
  });
});
