// connection-layer.test.ts — Stream D focused tests
//
// Tests for connectionLayer settings metadata, default, and enum bounds.
// Verifies mode 0/1/2 semantics as represented in the settings contract.
// Isolated from dev-panel.test.ts which covers broader descriptor/preset coverage.

import { describe, expect, test } from "vitest";

import { SETTING_IMPACT } from "../core/setting-metadata";
import {
  DEFAULT_SETTINGS,
  SETTINGS_LENGTH,
  toFloat32Array,
} from "../core/settings";

// ── connectionLayer: index contract ──────────────────────────────────────────

describe("connectionLayer index contract", () => {
  test("connectionLayer is at index 17 of the Float32Array", () => {
    // Any non-default value so we can distinguish it from neighbours.
    const s = { ...DEFAULT_SETTINGS, connectionLayer: 2 };
    const arr = toFloat32Array(s);
    expect(arr[17]).toBe(2);
  });

  test("changing connectionLayer does not shift any other index", () => {
    const base = toFloat32Array(DEFAULT_SETTINGS);
    const other = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 0 });

    for (let i = 0; i < SETTINGS_LENGTH; i++) {
      if (i === 17) continue; // connectionLayer index — expect it to differ
      expect(other[i]).toBeCloseTo(base[i], 6);
    }
  });

  test("Float32Array length remains 26 after connectionLayer change", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 2 });
    expect(arr.length).toBe(26);
  });
});

// ── connectionLayer: default value ───────────────────────────────────────────

describe("connectionLayer default", () => {
  test("default is 1 (Active/recent only)", () => {
    expect(DEFAULT_SETTINGS.connectionLayer).toBe(1);
  });

  test("default is serialised as 1 at index 17", () => {
    const arr = toFloat32Array(DEFAULT_SETTINGS);
    expect(arr[17]).toBe(1);
  });
});

// ── connectionLayer: valid mode enum values ───────────────────────────────────

describe("connectionLayer mode enum bounds", () => {
  test("mode 0 (Off) serialises to 0 at index 17", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 0 });
    expect(arr[17]).toBe(0);
  });

  test("mode 1 (Active/recent) serialises to 1 at index 17", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 1 });
    expect(arr[17]).toBe(1);
  });

  test("mode 2 (Resting debug) serialises to 2 at index 17", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 2 });
    expect(arr[17]).toBe(2);
  });
});

// ── connectionLayer: impact metadata ─────────────────────────────────────────

describe("connectionLayer setting impact", () => {
  test("connectionLayer impact is 'live' (no network rebuild needed)", () => {
    expect(SETTING_IMPACT.connectionLayer).toBe("live");
  });
});

// ── connectionLayer: mode 0 is a true off state ───────────────────────────────
// Semantics: mode 0 must not be confused with a 'missing' value.
// The Rust renderer gates all morphology work on connection_layer != 0;
// this test confirms that mode 0 is distinct from the default 1.

describe("connectionLayer mode semantics", () => {
  test("mode 0 produces a different Float32Array than mode 1", () => {
    const off = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 0 });
    const on  = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 1 });
    expect(off[17]).not.toBe(on[17]);
  });

  test("mode 2 is distinct from both 0 and 1", () => {
    const off   = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 0 });
    const active = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 1 });
    const debug  = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 2 });
    expect(debug[17]).not.toBe(off[17]);
    expect(debug[17]).not.toBe(active[17]);
  });

  test("resting connections are hidden by default (morphRestingOpacity=0 + mode 1)", () => {
    // The 'fully hidden by default' requirement for resting connections:
    // morphRestingOpacity must be 0 at default (only active pulses visible),
    // and the default mode must be 1 (active/recent), not 0 (off) or 2 (all).
    expect(DEFAULT_SETTINGS.morphRestingOpacity).toBe(0.0);
    expect(DEFAULT_SETTINGS.connectionLayer).toBe(1);
  });
});
