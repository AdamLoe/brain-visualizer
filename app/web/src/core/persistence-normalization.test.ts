import { describe, expect, test } from "vitest";

import {
  DEFAULT_MORPH_CONFIG,
  MORPH_CONFIG_LS_KEY,
  loadMorphConfig,
  saveMorphConfig,
} from "./morph-config";
import {
  DEFAULT_SETTINGS,
  SETTINGS_LS_KEY,
  loadSettings,
  saveSettings,
} from "./settings";
import {
  CONFIG_LS_KEY,
  DEFAULT_CONFIG,
  loadConfig,
  saveConfig,
  type AppConfig,
} from "./types";

function installMemoryLocalStorage(): Map<string, string> {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
    },
  });
  return store;
}

describe("AppConfig persistence normalization", () => {
  test("loadConfig clamps persisted runtime knobs and normalizes stale enums", () => {
    installMemoryLocalStorage();
    localStorage.setItem(CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      n: DEFAULT_CONFIG.n,
      k: 999,
      tier: "oversized",
      backend: "cpu",
      regionAssignmentMode: "spatial-lobes",
      speed: "warp",
      excitability: 2,
      ticksPerSec: -10,
    }));

    const loaded = loadConfig();

    expect(loaded.k).toBe(64);
    expect(loaded.tier).toBe(DEFAULT_CONFIG.tier);
    expect(loaded.backend).toBe("gpu");
    expect(loaded.regionAssignmentMode).toBe("hash-random");
    expect(loaded.speed).toBe(DEFAULT_CONFIG.speed);
    expect(loaded.excitability).toBe(1);
    expect(loaded.ticksPerSec).toBe(1);
  });

  test("saveConfig writes a normalized payload", () => {
    installMemoryLocalStorage();
    saveConfig({
      ...DEFAULT_CONFIG,
      k: -5,
      tier: "oversized",
      backend: "cpu",
      regionAssignmentMode: "spatial-lobes",
      speed: "warp",
      excitability: -2,
      ticksPerSec: 1000,
    } as unknown as AppConfig);

    const saved = JSON.parse(localStorage.getItem(CONFIG_LS_KEY) ?? "{}") as Record<string, unknown>;

    expect(saved.k).toBe(4);
    expect(saved.tier).toBe(DEFAULT_CONFIG.tier);
    expect(saved.backend).toBe("gpu");
    expect(saved.regionAssignmentMode).toBe("hash-random");
    expect(saved.speed).toBe(DEFAULT_CONFIG.speed);
    expect(saved.excitability).toBe(0);
    expect(saved.ticksPerSec).toBe(60);
  });
});

describe("VisualizerSettings persistence normalization", () => {
  test("loadSettings clamps finite out-of-range persisted settings", () => {
    installMemoryLocalStorage();
    localStorage.setItem(SETTINGS_LS_KEY, JSON.stringify({
      version: 5,
      public: {
        glowTau: -10,
        connectionLayer: 99,
        colorBy: 99,
        neuronVisibility: 9,
      },
      dev: {
        neuronVisualRadius: 10,
        activeNeuronRadiusBoost: -1,
        inactiveNeuronOpacity: 2,
        voltageGlowStrength: 9,
        connectionVisualWidth: -1,
        connectionCurveLift: 9,
        connectionLightNext: 7,
        iExt: 9,
        synapticScale: -2,
        heterogeneity: 3,
        morphRestingOpacity: -1,
        weightNormalization: 7,
        inputMode: 9,
        longRangeReachFrac: 2,
        maxReachCells: 99,
        arrivalHoldTicks: -10,
      },
    }));

    const loaded = loadSettings();

    expect(loaded.glowTau).toBe(1);
    expect(loaded.connectionLayer).toBe(1);
    expect(loaded.colorBy).toBe(DEFAULT_SETTINGS.colorBy);
    expect(loaded.neuronVisibility).toBe(DEFAULT_SETTINGS.neuronVisibility);
    expect(loaded.neuronVisualRadius).toBe(0.02);
    expect(loaded.activeNeuronRadiusBoost).toBe(1);
    expect(loaded.inactiveNeuronOpacity).toBe(1);
    expect(loaded.voltageGlowStrength).toBe(2);
    expect(loaded.connectionVisualWidth).toBe(0.1);
    expect(loaded.connectionCurveLift).toBe(0.5);
    expect(loaded.connectionLightNext).toBe(DEFAULT_SETTINGS.connectionLightNext);
    expect(loaded.iExt).toBe(0.3);
    expect(loaded.synapticScale).toBe(0);
    expect(loaded.heterogeneity).toBe(1);
    expect(loaded.morphRestingOpacity).toBe(0);
    expect(loaded.weightNormalization).toBe(DEFAULT_SETTINGS.weightNormalization);
    expect(loaded.inputMode).toBe(DEFAULT_SETTINGS.inputMode);
    expect(loaded.longRangeReachFrac).toBe(1);
    expect(loaded.maxReachCells).toBe(16);
    expect(loaded.arrivalHoldTicks).toBe(0);
  });

  test("saveSettings clamps values before writing localStorage", () => {
    installMemoryLocalStorage();
    saveSettings({
      ...DEFAULT_SETTINGS,
      glowTau: 999,
      iExt: -1,
      longRangeReachFrac: 2,
      maxReachCells: 2.4,
      arrivalHoldTicks: 999,
    });

    const saved = JSON.parse(localStorage.getItem(SETTINGS_LS_KEY) ?? "{}") as {
      public?: Record<string, unknown>;
      dev?: Record<string, unknown>;
    };

    expect(saved.public?.glowTau).toBe(200);
    expect(saved.dev?.iExt).toBe(0);
    expect(saved.dev?.longRangeReachFrac).toBe(1);
    expect(saved.dev?.maxReachCells).toBe(2);
    expect(saved.dev?.arrivalHoldTicks).toBe(180);
  });
});

describe("MorphologyConfig persistence normalization", () => {
  test("loadMorphConfig clamps persisted values to descriptor ranges", () => {
    installMemoryLocalStorage();
    localStorage.setItem(MORPH_CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      config: {
        generator: {
          baseRadius: 99,
          socketCountMin: 3.6,
          axonCurveLift: 0.37,
        },
        renderQuality: {
          tubeSides: 99,
          sphereStacks: 4.7,
        },
        lighting: {
          lightDirX: -8,
          ambient: -5,
          rimPower: 9,
        },
      },
    }));

    const loaded = loadMorphConfig();

    expect(loaded.generator.baseRadius).toBe(0.010);
    expect(loaded.generator.socketCountMin).toBe(4);
    expect(loaded.generator.axonCurveLift).toBe(DEFAULT_MORPH_CONFIG.generator.axonCurveLift);
    expect(loaded.renderQuality.tubeSides).toBe(12);
    expect(loaded.renderQuality.sphereStacks).toBe(5);
    expect(loaded.lighting.lightDirX).toBe(-1);
    expect(loaded.lighting.ambient).toBe(0.20);
    expect(loaded.lighting.rimPower).toBe(6);
  });

  test("saveMorphConfig persists a normalized config", () => {
    installMemoryLocalStorage();
    saveMorphConfig({
      ...DEFAULT_MORPH_CONFIG,
      generator: {
        ...DEFAULT_MORPH_CONFIG.generator,
        edgeSubsegments: 99,
        minSubsegments: 2.6,
      },
      renderQuality: {
        ...DEFAULT_MORPH_CONFIG.renderQuality,
        sphereSlices: 99,
      },
      lighting: {
        ...DEFAULT_MORPH_CONFIG.lighting,
        activeOpacity: 2,
      },
    });

    const saved = JSON.parse(localStorage.getItem(MORPH_CONFIG_LS_KEY) ?? "{}") as {
      config?: {
        generator?: Record<string, unknown>;
        renderQuality?: Record<string, unknown>;
        lighting?: Record<string, unknown>;
      };
    };

    expect(saved.config?.generator?.edgeSubsegments).toBe(4);
    expect(saved.config?.generator?.minSubsegments).toBe(3);
    expect(saved.config?.renderQuality?.sphereSlices).toBe(16);
    expect(saved.config?.lighting?.activeOpacity).toBe(1);
  });
});
