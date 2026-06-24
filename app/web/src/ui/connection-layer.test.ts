// connection-layer.test.ts — Stream D focused tests
//
// Tests for connectionLayer settings metadata, default, and enum bounds.
// Verifies mode 0/1 semantics and mode-2 compatibility normalization.
// Isolated from dev-panel.test.ts which covers broader descriptor/preset coverage.

import { describe, expect, test } from "vitest";

import { SETTING_IMPACT } from "../core/setting-metadata";
import {
  DEFAULT_SETTINGS,
  SETTINGS_LENGTH,
  SETTINGS_LS_KEY,
  loadSettings,
  toFloat32Array,
} from "../core/settings";

function installMemoryLocalStorage(): void {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
    },
  });
}

// ── connectionLayer: index contract ──────────────────────────────────────────

describe("connectionLayer index contract", () => {
  test("connectionLayer is at index 17 of the Float32Array", () => {
    // Any non-default value so we can distinguish it from neighbours.
    const s = { ...DEFAULT_SETTINGS, connectionLayer: 0 };
    const arr = toFloat32Array(s);
    expect(arr[17]).toBe(0);
  });

  test("changing connectionLayer does not shift any other index", () => {
    const base = toFloat32Array(DEFAULT_SETTINGS);
    const other = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 0 });

    for (let i = 0; i < SETTINGS_LENGTH; i++) {
      if (i === 17) continue; // connectionLayer index — expect it to differ
      expect(other[i]).toBeCloseTo(base[i], 6);
    }
  });

  test("Float32Array length remains 27 after connectionLayer change", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 2 });
    expect(arr.length).toBe(27);
  });
});

// ── connectionLayer: default value ───────────────────────────────────────────

describe("connectionLayer default", () => {
  test("default is 2 (Until arrival)", () => {
    expect(DEFAULT_SETTINGS.connectionLayer).toBe(2);
  });

  test("default is serialised as 2 at index 17", () => {
    const arr = toFloat32Array(DEFAULT_SETTINGS);
    expect(arr[17]).toBe(2);
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

  test("mode 2 (Until arrival) serialises to 2 at index 17", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 2 });
    expect(arr[17]).toBe(2);
  });
});

// ── connectionLayer: impact metadata ─────────────────────────────────────────

describe("connectionLayer setting impact", () => {
  test("connectionLayer impact is 'live' (no network rebuild needed)", () => {
    expect(SETTING_IMPACT.connectionLayer).toBe("live");
  });

  test("arrivalHoldTicks impact is 'live' (compaction timing only)", () => {
    expect(SETTING_IMPACT.arrivalHoldTicks).toBe("live");
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

  test("mode 2 is distinct from off and active/recent", () => {
    const off   = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 0 });
    const active = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 1 });
    const untilArrival  = toFloat32Array({ ...DEFAULT_SETTINGS, connectionLayer: 2 });
    expect(untilArrival[17]).not.toBe(off[17]);
    expect(untilArrival[17]).not.toBe(active[17]);
  });

  test("resting connections are hidden by default (morphRestingOpacity=0 + mode 2)", () => {
    // Idle structure stays hidden: morphRestingOpacity must be 0 at default (only
    // fired arbors show, via the until-arrival subdued visibility), and the default
    // mode must be 2 (until-arrival), not 0 (off).
    expect(DEFAULT_SETTINGS.morphRestingOpacity).toBe(0.0);
    expect(DEFAULT_SETTINGS.connectionLayer).toBe(2);
  });

  test("persisted mode 2 loads as until-arrival mode", () => {
    installMemoryLocalStorage();
    localStorage.setItem(SETTINGS_LS_KEY, JSON.stringify({
      version: 5,
      public: {
        glowTau: DEFAULT_SETTINGS.glowTau,
        connectionLayer: 2,
        colorBy: DEFAULT_SETTINGS.colorBy,
        neuronVisibility: DEFAULT_SETTINGS.neuronVisibility,
      },
      dev: {
        neuronVisualRadius: DEFAULT_SETTINGS.neuronVisualRadius,
        activeNeuronRadiusBoost: DEFAULT_SETTINGS.activeNeuronRadiusBoost,
        inactiveNeuronOpacity: DEFAULT_SETTINGS.inactiveNeuronOpacity,
        voltageGlowStrength: DEFAULT_SETTINGS.voltageGlowStrength,
        connectionVisualWidth: DEFAULT_SETTINGS.connectionVisualWidth,
        connectionCurveLift: DEFAULT_SETTINGS.connectionCurveLift,
        connectionLightNext: DEFAULT_SETTINGS.connectionLightNext,
        iExt: DEFAULT_SETTINGS.iExt,
        synapticScale: DEFAULT_SETTINGS.synapticScale,
        heterogeneity: DEFAULT_SETTINGS.heterogeneity,
        morphRestingOpacity: DEFAULT_SETTINGS.morphRestingOpacity,
        weightNormalization: DEFAULT_SETTINGS.weightNormalization,
        inputMode: DEFAULT_SETTINGS.inputMode,
        longRangeReachFrac: DEFAULT_SETTINGS.longRangeReachFrac,
        maxReachCells: DEFAULT_SETTINGS.maxReachCells,
        arrivalHoldTicks: DEFAULT_SETTINGS.arrivalHoldTicks,
      },
    }));
    expect(loadSettings().connectionLayer).toBe(2);
  });

  test("arrivalHoldTicks serialises after existing VisualSettings slots", () => {
    const arr = toFloat32Array({ ...DEFAULT_SETTINGS, arrivalHoldTicks: 42 });
    expect(arr[26]).toBe(42);
    expect(DEFAULT_SETTINGS.arrivalHoldTicks).toBe(30);
  });
});
