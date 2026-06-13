import { describe, expect, test } from "vitest";

import {
  DEFAULT_MORPH_CONFIG,
  MORPH_CONFIG_LS_KEY,
  MORPH_DESCRIPTORS,
  getMorphValue,
  loadMorphConfig,
  morphConfigToJson,
  saveMorphConfig,
} from "../core/morph-config";
import {
  DEFAULT_SETTINGS,
  SETTINGS_LS_KEY,
  loadSettings,
  saveSettings,
  toFloat32Array,
} from "../core/settings";
import {
  CONFIG_LS_KEY,
  DEFAULT_CONFIG,
  PRODUCT_MAX_N,
  loadConfig,
  normalizeRegionAssignmentMode,
  saveConfig,
} from "../core/types";
import { HIDDEN_REVIEW_PRESETS } from "./dev-panel";

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

describe("HIDDEN_REVIEW_PRESETS", () => {
  test("accepted-default matches clean first-load defaults exactly", () => {
    expect(HIDDEN_REVIEW_PRESETS["accepted-default"].appConfig).toEqual(DEFAULT_CONFIG);
    expect(HIDDEN_REVIEW_PRESETS["accepted-default"].visualSettings).toEqual(DEFAULT_SETTINGS);
    expect(HIDDEN_REVIEW_PRESETS["accepted-default"].morphologyConfig).toEqual(DEFAULT_MORPH_CONFIG);
  });

  test("review-only variants stay separate from the accepted default payload", () => {
    expect(HIDDEN_REVIEW_PRESETS["performance-review"].visualSettings).not.toEqual(DEFAULT_SETTINGS);
    expect(HIDDEN_REVIEW_PRESETS["hero-review"].morphologyConfig).not.toEqual(DEFAULT_MORPH_CONFIG);
  });
});

describe("settings descriptors", () => {
  test("accepted product defaults are wired to settings and morphology", () => {
    expect(DEFAULT_SETTINGS.glowTau).toBe(10);
    expect(DEFAULT_SETTINGS.heterogeneity).toBe(0.50);
    expect(DEFAULT_MORPH_CONFIG.lighting.restingBrightness).toBe(0.0);

    const values = toFloat32Array(DEFAULT_SETTINGS);
    expect(values[0]).toBe(10);
    expect(values[14]).toBe(0.50);
  });

  test("morph descriptor defaults match DEFAULT_MORPH_CONFIG", () => {
    for (const descriptor of MORPH_DESCRIPTORS) {
      expect(descriptor.default).toBe(getMorphValue(DEFAULT_MORPH_CONFIG, descriptor.jsonPath));
    }
  });

  test("persisted app config clamps saved N to the product cap", () => {
    installMemoryLocalStorage();
    localStorage.setItem(CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      n: PRODUCT_MAX_N * 10,
      k: DEFAULT_CONFIG.k,
      tier: DEFAULT_CONFIG.tier,
      backend: DEFAULT_CONFIG.backend,
      speed: DEFAULT_CONFIG.speed,
      excitability: DEFAULT_CONFIG.excitability,
      ticksPerSec: DEFAULT_CONFIG.ticksPerSec,
    }));

    const loaded = loadConfig();
    expect(loaded.n).toBe(PRODUCT_MAX_N);

    saveConfig({ ...DEFAULT_CONFIG, n: PRODUCT_MAX_N * 2 });
    const saved = JSON.parse(localStorage.getItem(CONFIG_LS_KEY) ?? "{}") as { n?: number };
    expect(saved.n).toBe(PRODUCT_MAX_N);
  });

  test("stale CPU backend app config normalizes to GPU", () => {
    installMemoryLocalStorage();
    localStorage.setItem(CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      n: DEFAULT_CONFIG.n,
      k: DEFAULT_CONFIG.k,
      tier: DEFAULT_CONFIG.tier,
      backend: "cpu",
      speed: DEFAULT_CONFIG.speed,
      excitability: DEFAULT_CONFIG.excitability,
      ticksPerSec: DEFAULT_CONFIG.ticksPerSec,
    }));

    expect(loadConfig().backend).toBe("gpu");
  });

  test("app config defaults to hash-random region assignment", () => {
    installMemoryLocalStorage();
    expect(loadConfig().regionAssignmentMode).toBe("hash-random");
    expect(DEFAULT_CONFIG.regionAssignmentMode).toBe("hash-random");
    expect(normalizeRegionAssignmentMode("unknown")).toBe("hash-random");
  });

  test("app config persists prototype region assignment and normalizes stale values", () => {
    installMemoryLocalStorage();
    saveConfig({
      ...DEFAULT_CONFIG,
      regionAssignmentMode: "anterior-posterior-prototype",
    });
    expect(loadConfig().regionAssignmentMode).toBe("anterior-posterior-prototype");

    localStorage.setItem(CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      n: DEFAULT_CONFIG.n,
      k: DEFAULT_CONFIG.k,
      tier: DEFAULT_CONFIG.tier,
      backend: DEFAULT_CONFIG.backend,
      regionAssignmentMode: "spatial-lobes",
      speed: DEFAULT_CONFIG.speed,
      excitability: DEFAULT_CONFIG.excitability,
      ticksPerSec: DEFAULT_CONFIG.ticksPerSec,
    }));
    expect(loadConfig().regionAssignmentMode).toBe("hash-random");
  });

  test("duplicate generator axon curve control is hidden and default-only", () => {
    expect(MORPH_DESCRIPTORS.map((d) => d.jsonPath)).not.toContain("generator.axonCurveLift");

    installMemoryLocalStorage();
    localStorage.setItem(MORPH_CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      config: {
        generator: {
          ...DEFAULT_MORPH_CONFIG.generator,
          axonCurveLift: 0.37,
        },
      },
    }));

    const loaded = loadMorphConfig();

    expect(loaded.generator.axonCurveLift).toBe(DEFAULT_MORPH_CONFIG.generator.axonCurveLift);

    saveMorphConfig({
      ...loaded,
      generator: {
        ...loaded.generator,
        axonCurveLift: 0.37,
      },
    });
    const saved = JSON.parse(localStorage.getItem(MORPH_CONFIG_LS_KEY) ?? "{}") as {
      config?: { generator?: { axonCurveLift?: number } };
    };
    expect(saved.config?.generator?.axonCurveLift).toBe(DEFAULT_MORPH_CONFIG.generator.axonCurveLift);
  });

  test("legacy dendrite controls are dropped from persisted morph config", () => {
    installMemoryLocalStorage();
    localStorage.setItem(MORPH_CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      config: {
        generator: {
          ...DEFAULT_MORPH_CONFIG.generator,
          baseRadius: 0.007,
          dendritePrimaryMin: 99,
          dendritePrimarySpan: 99,
          dendriteReachLo: 99,
          dendriteReachHi: 99,
        },
      },
    }));

    const loaded = loadMorphConfig();

    expect(loaded.generator.baseRadius).toBe(0.007);
    expect("dendritePrimaryMin" in loaded.generator).toBe(false);
    expect("dendriteReachLo" in loaded.generator).toBe(false);
    const serialized = JSON.parse(morphConfigToJson(loaded)) as {
      generator?: Record<string, unknown>;
    };
    expect(serialized.generator?.dendritePrimaryRootCount).toBe(DEFAULT_MORPH_CONFIG.generator.dendritePrimaryRootCount);
    expect("dendritePrimaryMin" in (serialized.generator ?? {})).toBe(false);
    expect("dendritePrimarySpan" in (serialized.generator ?? {})).toBe(false);
    expect("dendriteReachLo" in (serialized.generator ?? {})).toBe(false);
    expect("dendriteReachHi" in (serialized.generator ?? {})).toBe(false);

    saveMorphConfig(loaded);
    const persisted = JSON.parse(localStorage.getItem(MORPH_CONFIG_LS_KEY) ?? "{}") as {
      config?: { generator?: Record<string, unknown> };
    };
    expect(persisted.config?.generator?.dendritePrimaryRootCount).toBe(DEFAULT_MORPH_CONFIG.generator.dendritePrimaryRootCount);
    expect("dendritePrimaryMin" in (persisted.config?.generator ?? {})).toBe(false);
    expect("dendritePrimarySpan" in (persisted.config?.generator ?? {})).toBe(false);
    expect("dendriteReachLo" in (persisted.config?.generator ?? {})).toBe(false);
    expect("dendriteReachHi" in (persisted.config?.generator ?? {})).toBe(false);
  });

  test("retired visual settings are not persisted or restored", () => {
    installMemoryLocalStorage();
    localStorage.setItem(SETTINGS_LS_KEY, JSON.stringify({
      version: 5,
      public: {
        glowTau: 88,
        bloomStrength: 0.7,
        surfaceOpacity: 0.25,
        connectionLayer: 1,
        colorBy: 4,
        neuronVisibility: 2,
        surface: 2,
      },
      dev: {
        pointRadius: 0.02,
        neuronVisualRadius: 0.011,
        activeNeuronRadiusBoost: 3,
        inactiveNeuronOpacity: 0.5,
        voltageGlowStrength: 1,
        connectionVisualWidth: 1.2,
        connectionCurveLift: 0.2,
        connectionLightNext: 1,
        iExt: 0.1,
        synapticScale: 0.07,
        heterogeneity: 0.4,
        morphRestingOpacity: 0.3,
        weightNormalization: 2,
        inputMode: 1,
        longRangeReachFrac: 0.25,
        maxReachCells: 9,
      },
    }));

    const loaded = loadSettings();

    expect(loaded.glowTau).toBe(88);
    expect(loaded.bloomStrength).toBe(DEFAULT_SETTINGS.bloomStrength);
    expect(loaded.pointRadius).toBe(DEFAULT_SETTINGS.pointRadius);
    expect(loaded.surfaceOpacity).toBe(DEFAULT_SETTINGS.surfaceOpacity);
    expect(loaded.surface).toBe(DEFAULT_SETTINGS.surface);

    saveSettings({
      ...loaded,
      bloomStrength: 0.7,
      pointRadius: 0.02,
      surfaceOpacity: 0.25,
      surface: 2,
    });
    const saved = JSON.parse(localStorage.getItem(SETTINGS_LS_KEY) ?? "{}") as {
      public?: Record<string, unknown>;
      dev?: Record<string, unknown>;
    };
    expect(saved.public).not.toHaveProperty("bloomStrength");
    expect(saved.public).not.toHaveProperty("surfaceOpacity");
    expect(saved.public).not.toHaveProperty("surface");
    expect(saved.dev).not.toHaveProperty("pointRadius");
  });

  test("dead and retired VisualSettings indices are tombstoned or default-written", () => {
    const settings = {
      ...DEFAULT_SETTINGS,
      pointRadius: 0.02,
      surfaceOpacity: 0.25,
      bloomStrength: 0.7,
      signalSource: 2,
      surface: 2,
      adaptiveScalerEnabled: 1,
    };
    const values = toFloat32Array(settings);

    expect(values.length).toBe(26);
    expect(values[1]).toBeCloseTo(DEFAULT_SETTINGS.pointRadius);
    expect(values[10]).toBe(0);
    expect(values[11]).toBe(DEFAULT_SETTINGS.surfaceOpacity);
    expect(values[16]).toBe(0);
    expect(values[20]).toBe(DEFAULT_SETTINGS.surface);
    expect(values[23]).toBe(0);
  });
});

describe("morphology subdivision controls", () => {
  const paths = [
    "generator.maxSegmentLength",
    "generator.longRangeMaxSegmentLength",
    "generator.curvatureSubsegmentBoost",
    "generator.edgeSubsegmentsMax",
    "generator.minSubsegments",
  ];

  test("DEFAULT_MORPH_CONFIG exposes straight subsegment controls", () => {
    expect(DEFAULT_MORPH_CONFIG.generator.maxSegmentLength).toBe(0.05);
    expect(DEFAULT_MORPH_CONFIG.generator.longRangeMaxSegmentLength).toBe(0.025);
    expect(DEFAULT_MORPH_CONFIG.generator.curvatureSubsegmentBoost).toBe(2.0);
    expect(DEFAULT_MORPH_CONFIG.generator.edgeSubsegmentsMax).toBe(4);
    expect(DEFAULT_MORPH_CONFIG.generator.minSubsegments).toBe(1);
  });

  test("subdivision descriptors match defaults and regenerate morphology", () => {
    for (const path of paths) {
      const descriptor = MORPH_DESCRIPTORS.find((d) => d.jsonPath === path);
      expect(descriptor).toBeDefined();
      expect(descriptor?.default).toBe(getMorphValue(DEFAULT_MORPH_CONFIG, path));
      expect(descriptor?.group).toBe("generator");
      expect(descriptor?.applyKind).toBe("regenerate");
    }
  });

  test("morphConfigToJson persists subdivision controls", () => {
    const modified = structuredClone(DEFAULT_MORPH_CONFIG);
    modified.generator.maxSegmentLength = 0.03;
    modified.generator.longRangeMaxSegmentLength = 0.018;
    modified.generator.curvatureSubsegmentBoost = 3.1;
    modified.generator.edgeSubsegmentsMax = 3;
    modified.generator.minSubsegments = 2;

    const parsed = JSON.parse(morphConfigToJson(modified)) as {
      generator?: Record<string, unknown>;
    };
    expect(parsed.generator?.maxSegmentLength).toBe(0.03);
    expect(parsed.generator?.longRangeMaxSegmentLength).toBe(0.018);
    expect(parsed.generator?.curvatureSubsegmentBoost).toBe(3.1);
    expect(parsed.generator?.edgeSubsegmentsMax).toBe(3);
    expect(parsed.generator?.minSubsegments).toBe(2);
  });
});

// ── Stream F: dendrite decoration config round-trip and defaults ──────────────

describe("Stream F dendrite decoration controls", () => {
  test("DEFAULT_MORPH_CONFIG has the three new decoration fields with correct defaults", () => {
    expect(DEFAULT_MORPH_CONFIG.generator.dendriteBranchletCount).toBe(1);
    expect(DEFAULT_MORPH_CONFIG.generator.dendriteTwigCount).toBe(1);
    expect(DEFAULT_MORPH_CONFIG.generator.dendriteDecorGroupMax).toBe(12);
  });

  test("MORPH_DESCRIPTORS contains entries for the three new decoration controls", () => {
    const paths = MORPH_DESCRIPTORS.map((d) => d.jsonPath);
    expect(paths).toContain("generator.dendriteBranchletCount");
    expect(paths).toContain("generator.dendriteTwigCount");
    expect(paths).toContain("generator.dendriteDecorGroupMax");
  });

  test("new decoration descriptor defaults match DEFAULT_MORPH_CONFIG", () => {
    const decor = MORPH_DESCRIPTORS.filter((d) =>
      ["generator.dendriteBranchletCount", "generator.dendriteTwigCount", "generator.dendriteDecorGroupMax"].includes(d.jsonPath)
    );
    expect(decor).toHaveLength(3);
    for (const d of decor) {
      expect(d.default).toBe(getMorphValue(DEFAULT_MORPH_CONFIG, d.jsonPath));
    }
  });

  test("all decoration descriptors are group=generator and applyKind=regenerate", () => {
    const decor = MORPH_DESCRIPTORS.filter((d) =>
      ["generator.dendriteBranchletCount", "generator.dendriteTwigCount", "generator.dendriteDecorGroupMax"].includes(d.jsonPath)
    );
    for (const d of decor) {
      expect(d.group).toBe("generator");
      expect(d.applyKind).toBe("regenerate");
    }
  });

  test("morphConfigToJson round-trip preserves new decoration fields", () => {
    const modified = structuredClone(DEFAULT_MORPH_CONFIG);
    modified.generator.dendriteBranchletCount = 0;
    modified.generator.dendriteTwigCount = 2;
    modified.generator.dendriteDecorGroupMax = 8;

    const json = morphConfigToJson(modified);
    const parsed = JSON.parse(json) as { generator?: Record<string, unknown> };
    expect(parsed.generator?.dendriteBranchletCount).toBe(0);
    expect(parsed.generator?.dendriteTwigCount).toBe(2);
    expect(parsed.generator?.dendriteDecorGroupMax).toBe(8);
  });

  test("loadMorphConfig applies defaults for missing new decoration fields (forward compat)", () => {
    installMemoryLocalStorage();
    // Simulate a saved config from before Stream F (new fields absent).
    const { dendriteBranchletCount: _b, dendriteTwigCount: _t, dendriteDecorGroupMax: _d, ...generatorWithout } =
      DEFAULT_MORPH_CONFIG.generator;
    localStorage.setItem(MORPH_CONFIG_LS_KEY, JSON.stringify({
      version: 1,
      config: {
        generator: {
          ...generatorWithout,
          baseRadius: 0.007,
        },
      },
    }));

    const loaded = loadMorphConfig();

    // Known fields should load.
    expect(loaded.generator.baseRadius).toBe(0.007);
    // Missing new fields should fall back to defaults.
    expect(loaded.generator.dendriteBranchletCount).toBe(DEFAULT_MORPH_CONFIG.generator.dendriteBranchletCount);
    expect(loaded.generator.dendriteTwigCount).toBe(DEFAULT_MORPH_CONFIG.generator.dendriteTwigCount);
    expect(loaded.generator.dendriteDecorGroupMax).toBe(DEFAULT_MORPH_CONFIG.generator.dendriteDecorGroupMax);
  });

  test("saveMorphConfig persists new decoration fields and loadMorphConfig restores them", () => {
    installMemoryLocalStorage();
    const modified = structuredClone(DEFAULT_MORPH_CONFIG);
    modified.generator.dendriteBranchletCount = 0;
    modified.generator.dendriteTwigCount = 2;
    modified.generator.dendriteDecorGroupMax = 4;

    saveMorphConfig(modified);
    const loaded = loadMorphConfig();

    expect(loaded.generator.dendriteBranchletCount).toBe(0);
    expect(loaded.generator.dendriteTwigCount).toBe(2);
    expect(loaded.generator.dendriteDecorGroupMax).toBe(4);
  });

  test("hero-review preset has richer decoration than default", () => {
    const hero = HIDDEN_REVIEW_PRESETS["hero-review"].morphologyConfig;
    // Hero should use max twigs and decoration for close-up screenshots.
    expect(hero.generator.dendriteTwigCount).toBeGreaterThan(
      DEFAULT_MORPH_CONFIG.generator.dendriteTwigCount
    );
    expect(hero.generator.dendriteDecorGroupMax).toBeGreaterThanOrEqual(
      DEFAULT_MORPH_CONFIG.generator.dendriteDecorGroupMax
    );
  });
});
