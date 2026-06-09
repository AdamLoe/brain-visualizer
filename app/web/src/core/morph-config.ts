// web/core/morph-config.ts — v0.3.1 Morphology Configuration Exposure
//
// A SMALL, morphology-LOCAL descriptor-driven config surface (orchestrator
// decision D2). Deliberately separate from the bespoke `VisualizerSettings`
// plumbing in settings.ts: morphology generator/render-quality/lighting params
// are NOT part of the frozen Float32Array contract. They cross the WASM boundary
// as a JSON string via a dedicated `set_morphology_config(json)` entry point.
//
// Source of truth: the "Config Contract" table in
// docs/plans/v0.3.1-morph-config.md. The serialized JSON shape is nested
// `{ generator, renderQuality, lighting }` with camelCase keys — both Rust and
// TS verify against that table, not against each other.
//
// Persistence: versioned localStorage key "bv2_morph_v1", version sentinel = 1.
// On version mismatch → ignore saved data, use defaults (no migration), mirroring
// the loadSettings/saveSettings/resetSettings pattern in settings.ts.

import type { SettingImpact } from "./setting-metadata";

// ─── Nested config shape (mirrors the contract's JSON layout) ─────────────────

export interface MorphGeneratorConfig {
  baseRadius:                  number;
  dendritePrimaryMin:          number;
  dendritePrimarySpan:         number;
  dendriteReachLo:             number;
  dendriteReachHi:             number;
  axonStopFraction:            number;
  axonRootRadiusFraction:      number;
  axonCurveLift:               number;
  socketCountMin:              number;
  socketCountMax:              number;
  socketRadiusLo:              number;
  socketRadiusHi:              number;
  socketTipPreference:         number;
  trunkLengthFraction:         number;
  twigRadiusFraction:          number;
  taperCurve:                  number;
  dendriteMidRadiusFraction:   number;
  dendriteTipRadiusFraction:   number;
  treeScoreCurvature:          number;
  treeScoreDensity:            number;
  treeScoreDegree:             number;
  relaxLerp:                   number;
  relaxRepel:                  number;
  relaxWindow:                 number;
  edgeSubsegments:             number;
}

export interface MorphRenderQualityConfig {
  tubeSides:    number;
  sphereSlices: number;
  sphereStacks: number;
}

export interface MorphLightingConfig {
  lightDirX:         number;
  lightDirY:         number;
  lightDirZ:         number;
  ambient:           number;
  diffuseIntensity:  number;
  rimIntensity:      number;
  rimPower:          number;
  restingBrightness: number;
  activeBoost:       number;
}

export interface MorphologyConfig {
  generator:     MorphGeneratorConfig;
  renderQuality: MorphRenderQualityConfig;
  lighting:      MorphLightingConfig;
}

// ─── Defaults (every default from the contract table) ─────────────────────────

export const DEFAULT_MORPH_CONFIG: MorphologyConfig = {
  generator: {
    baseRadius:                0.006,
    dendritePrimaryMin:        3,
    dendritePrimarySpan:       2,
    dendriteReachLo:           0.035,
    dendriteReachHi:           0.058,
    axonStopFraction:          0.85,
    axonRootRadiusFraction:    0.66,
    axonCurveLift:             0.15,
    socketCountMin:            2,
    socketCountMax:            4,
    socketRadiusLo:            0.008,
    socketRadiusHi:            0.018,
    socketTipPreference:       0.78,
    trunkLengthFraction:       0.32,
    twigRadiusFraction:        0.16,
    taperCurve:                2.1,
    dendriteMidRadiusFraction: 0.6,
    dendriteTipRadiusFraction: 0.3,
    treeScoreCurvature:        0.5,
    treeScoreDensity:          0.5,
    treeScoreDegree:           0.7,
    relaxLerp:                 0.25,
    relaxRepel:                0.15,
    relaxWindow:               3,
    edgeSubsegments:           3,
  },
  renderQuality: {
    tubeSides:    6,
    sphereSlices: 8,
    sphereStacks: 6,
  },
  lighting: {
    lightDirX:         -0.352,
    lightDirY:          0.553,
    lightDirZ:          0.755,
    ambient:            0.55,
    diffuseIntensity:   0.35,
    rimIntensity:       0.30,
    rimPower:           2.0,
    restingBrightness:  0.20,
    activeBoost:        1.8,
  },
};

// ─── Descriptors (one entry per control, driven straight from the contract) ───
// applyKind:
//   "uniform"        → live uniform write (lighting + brightness)
//   "regenerate"     → needs regenerate_morphology (generator params)
//   "pipeline-rebuild" → needs render-pipeline rebuild (renderQuality)
// The dev panel renders one row per descriptor — no hand-written ~36 rows.

export type MorphGroup = "generator" | "renderQuality" | "lighting";
export type MorphApplyKind = "uniform" | "regenerate" | "pipeline-rebuild";

export interface MorphDescriptor {
  /** Nested path, e.g. "generator.baseRadius". Also the JSON key path. */
  jsonPath:  string;
  group:     MorphGroup;
  label:     string;
  type:      "number" | "int";
  min:       number;
  max:       number;
  step:      number;
  default:   number;
  /** Dev-panel impact dot color/label (setting-metadata.ts). */
  impact:    SettingImpact;
  applyKind: MorphApplyKind;
  tooltip:   string;
}

export const MORPH_DESCRIPTORS: readonly MorphDescriptor[] = [
  // ── generator (regenerate; red renderer-rebuild dot) ───────────────────────
  { jsonPath: "generator.baseRadius",                group: "generator", label: "Base radius",              type: "number", min: 0.004, max: 0.010, step: 0.0005, default: 0.006, impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Base cell/body scale that cascades into branch and sphere size." },
  { jsonPath: "generator.dendritePrimaryMin",        group: "generator", label: "Dendrite primary min",     type: "int",    min: 2,     max: 5,     step: 1,      default: 3,     impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Minimum number of primary dendrites." },
  { jsonPath: "generator.dendritePrimarySpan",       group: "generator", label: "Dendrite primary span",    type: "int",    min: 1,     max: 4,     step: 1,      default: 2,     impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Random span added to the primary dendrite count." },
  { jsonPath: "generator.dendriteReachLo",           group: "generator", label: "Dendrite reach lo",        type: "number", min: 0.020, max: 0.050, step: 0.001,  default: 0.035, impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Lower bound of dendrite reach range." },
  { jsonPath: "generator.dendriteReachHi",           group: "generator", label: "Dendrite reach hi",        type: "number", min: 0.040, max: 0.080, step: 0.001,  default: 0.058, impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Upper bound of dendrite reach range." },
  { jsonPath: "generator.axonStopFraction",          group: "generator", label: "Axon stop fraction",       type: "number", min: 0.60,  max: 0.98,  step: 0.01,   default: 0.85,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "How close terminal axons stop before their targets." },
  { jsonPath: "generator.axonRootRadiusFraction",    group: "generator", label: "Axon root radius frac",    type: "number", min: 0.40,  max: 0.90,  step: 0.01,   default: 0.66,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Axon root radius as a fraction of base radius." },
  { jsonPath: "generator.axonCurveLift",             group: "generator", label: "Axon curve lift",          type: "number", min: 0.0,   max: 0.40,  step: 0.01,   default: 0.15,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Axon bow/lift; also influenced by the live Curve setting." },
  { jsonPath: "generator.socketCountMin",            group: "generator", label: "Socket count min",         type: "int",    min: 1,     max: 4,     step: 1,      default: 2,     impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Minimum dendrite/socket anchor density." },
  { jsonPath: "generator.socketCountMax",            group: "generator", label: "Socket count max",         type: "int",    min: 1,     max: 6,     step: 1,      default: 4,     impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Maximum dendrite/socket anchor density." },
  { jsonPath: "generator.socketRadiusLo",            group: "generator", label: "Socket radius lo",         type: "number", min: 0.004, max: 0.016, step: 0.0005, default: 0.008, impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Lower bound of socket placement radius." },
  { jsonPath: "generator.socketRadiusHi",            group: "generator", label: "Socket radius hi",         type: "number", min: 0.010, max: 0.030, step: 0.0005, default: 0.018, impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Upper bound of socket placement radius." },
  { jsonPath: "generator.socketTipPreference",       group: "generator", label: "Socket tip preference",    type: "number", min: 0.50,  max: 1.0,   step: 0.01,   default: 0.78,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Bias of sockets toward branch tips." },
  { jsonPath: "generator.trunkLengthFraction",       group: "generator", label: "Trunk length fraction",    type: "number", min: 0.15,  max: 0.50,  step: 0.01,   default: 0.32,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Where the shared trunk node ends relative to mean target distance." },
  { jsonPath: "generator.twigRadiusFraction",        group: "generator", label: "Twig radius fraction",     type: "number", min: 0.08,  max: 0.35,  step: 0.01,   default: 0.16,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Axon tip (twig) radius floor as a fraction of trunk radius." },
  { jsonPath: "generator.taperCurve",                group: "generator", label: "Taper curve",              type: "number", min: 1.0,   max: 3.5,   step: 0.1,    default: 2.1,   impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Taper exponent along branches." },
  { jsonPath: "generator.dendriteMidRadiusFraction", group: "generator", label: "Dendrite mid radius frac", type: "number", min: 0.30,  max: 0.90,  step: 0.01,   default: 0.6,   impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Dendrite mid-section radius fraction." },
  { jsonPath: "generator.dendriteTipRadiusFraction", group: "generator", label: "Dendrite tip radius frac", type: "number", min: 0.10,  max: 0.60,  step: 0.01,   default: 0.3,   impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Dendrite tip radius fraction." },
  { jsonPath: "generator.treeScoreCurvature",        group: "generator", label: "Tree curvature weight",   type: "number", min: 0.0,   max: 2.0,   step: 0.05,   default: 0.5,   impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Axon-tree attach score: penalty for sharp bends (smoother forks)." },
  { jsonPath: "generator.treeScoreDensity",          group: "generator", label: "Tree density weight",     type: "number", min: 0.0,   max: 2.0,   step: 0.05,   default: 0.5,   impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Axon-tree attach score: penalty for crowding near existing branches." },
  { jsonPath: "generator.treeScoreDegree",           group: "generator", label: "Tree degree weight",      type: "number", min: 0.0,   max: 2.0,   step: 0.05,   default: 0.7,   impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Axon-tree attach score: soft 2-3-child fork tendency (no hard cap)." },
  { jsonPath: "generator.relaxLerp",                 group: "generator", label: "Relax pull strength",     type: "number", min: 0.0,   max: 0.8,   step: 0.01,   default: 0.25,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Relaxation pull of internal nodes toward parent+children mean." },
  { jsonPath: "generator.relaxRepel",                group: "generator", label: "Relax repel strength",    type: "number", min: 0.0,   max: 0.8,   step: 0.01,   default: 0.15,  impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Relaxation repulsion of nearby branches (spreads forks)." },
  { jsonPath: "generator.relaxWindow",               group: "generator", label: "Relax window depth",      type: "int",    min: 0,     max: 6,     step: 1,      default: 3,     impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Ancestor-window depth relaxed per attach." },
  { jsonPath: "generator.edgeSubsegments",           group: "generator", label: "Edge subsegments",        type: "int",    min: 1,     max: 4,     step: 1,      default: 3,     impact: "renderer-rebuild", applyKind: "regenerate", tooltip: "Bezier samples per axon-tree edge (curvature smoothness)." },

  // ── renderQuality (pipeline-rebuild; red renderer-rebuild dot) ─────────────
  { jsonPath: "renderQuality.tubeSides",    group: "renderQuality", label: "Tube sides",    type: "int", min: 3, max: 12, step: 1, default: 6, impact: "renderer-rebuild", applyKind: "pipeline-rebuild", tooltip: "Tube tessellation quality (sides per tube cross-section)." },
  { jsonPath: "renderQuality.sphereSlices", group: "renderQuality", label: "Sphere slices", type: "int", min: 5, max: 16, step: 1, default: 8, impact: "renderer-rebuild", applyKind: "pipeline-rebuild", tooltip: "Soma sphere longitudinal tessellation." },
  { jsonPath: "renderQuality.sphereStacks", group: "renderQuality", label: "Sphere stacks", type: "int", min: 4, max: 12, step: 1, default: 6, impact: "renderer-rebuild", applyKind: "pipeline-rebuild", tooltip: "Soma sphere latitudinal tessellation." },

  // ── lighting (uniform; live green dot) ─────────────────────────────────────
  { jsonPath: "lighting.lightDirX",         group: "lighting", label: "Light dir X",        type: "number", min: -1.0, max: 1.0, step: 0.01, default: -0.352, impact: "live", applyKind: "uniform", tooltip: "Light direction X (re-normalized CPU-side)." },
  { jsonPath: "lighting.lightDirY",         group: "lighting", label: "Light dir Y",        type: "number", min: -1.0, max: 1.0, step: 0.01, default:  0.553, impact: "live", applyKind: "uniform", tooltip: "Light direction Y." },
  { jsonPath: "lighting.lightDirZ",         group: "lighting", label: "Light dir Z",        type: "number", min: -1.0, max: 1.0, step: 0.01, default:  0.755, impact: "live", applyKind: "uniform", tooltip: "Light direction Z." },
  { jsonPath: "lighting.ambient",           group: "lighting", label: "Ambient",            type: "number", min:  0.20, max: 0.90, step: 0.01, default: 0.55, impact: "live", applyKind: "uniform", tooltip: "Ambient lighting term." },
  { jsonPath: "lighting.diffuseIntensity",  group: "lighting", label: "Diffuse intensity",  type: "number", min:  0.0,  max: 1.0,  step: 0.01, default: 0.35, impact: "live", applyKind: "uniform", tooltip: "Diffuse (Lambert) lighting intensity." },
  { jsonPath: "lighting.rimIntensity",      group: "lighting", label: "Rim intensity",      type: "number", min:  0.0,  max: 1.0,  step: 0.01, default: 0.30, impact: "live", applyKind: "uniform", tooltip: "Rim/fresnel highlight intensity." },
  { jsonPath: "lighting.rimPower",          group: "lighting", label: "Rim power",          type: "number", min:  1.0,  max: 6.0,  step: 0.1,  default: 2.0,  impact: "live", applyKind: "uniform", tooltip: "Rim highlight falloff exponent." },
  { jsonPath: "lighting.restingBrightness", group: "lighting", label: "Resting brightness", type: "number", min:  0.0,  max: 1.0,  step: 0.01, default: 0.20, impact: "live", applyKind: "uniform", tooltip: "Brightness of resting (non-active) structure." },
  { jsonPath: "lighting.activeBoost",       group: "lighting", label: "Active boost",       type: "number", min:  0.0,  max: 4.0,  step: 0.05, default: 1.8,  impact: "live", applyKind: "uniform", tooltip: "Brightness multiplier on active structure." },
] as const;

// ─── Nested path get/set helpers ──────────────────────────────────────────────

/** Read a descriptor's value out of a config (e.g. "generator.baseRadius"). */
export function getMorphValue(cfg: MorphologyConfig, jsonPath: string): number {
  const [group, key] = jsonPath.split(".") as [MorphGroup, string];
  return (cfg[group] as unknown as Record<string, number>)[key];
}

/** Return a copy of `cfg` with the descriptor's value replaced. */
export function setMorphValue(
  cfg: MorphologyConfig,
  jsonPath: string,
  value: number,
): MorphologyConfig {
  const [group, key] = jsonPath.split(".") as [MorphGroup, string];
  return {
    ...cfg,
    [group]: { ...(cfg[group] as unknown as Record<string, number>), [key]: value },
  };
}

// ─── Persistence (mirrors loadSettings/saveSettings/resetSettings) ─────────────

export const MORPH_CONFIG_LS_KEY = "bv2_morph_v1";
const LS_KEY = MORPH_CONFIG_LS_KEY;
const MORPH_VERSION = 1;

interface SavedMorphConfig {
  version: 1;
  config: MorphologyConfig;
}

/** Load from localStorage. Returns defaults on version mismatch or parse error. */
export function loadMorphConfig(): MorphologyConfig {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return structuredClone(DEFAULT_MORPH_CONFIG);
    const parsed = JSON.parse(raw) as { version?: number; config?: MorphologyConfig };
    if (parsed.version !== MORPH_VERSION || !parsed.config) {
      return structuredClone(DEFAULT_MORPH_CONFIG);
    }
    // Merge saved over defaults per-group so a missing field falls back to default.
    return {
      generator:     { ...DEFAULT_MORPH_CONFIG.generator,     ...parsed.config.generator },
      renderQuality: { ...DEFAULT_MORPH_CONFIG.renderQuality, ...parsed.config.renderQuality },
      lighting:      { ...DEFAULT_MORPH_CONFIG.lighting,      ...parsed.config.lighting },
    };
  } catch {
    return structuredClone(DEFAULT_MORPH_CONFIG);
  }
}

/** Persist the morphology config to localStorage. */
export function saveMorphConfig(cfg: MorphologyConfig): void {
  try {
    const saved: SavedMorphConfig = { version: MORPH_VERSION, config: cfg };
    localStorage.setItem(LS_KEY, JSON.stringify(saved));
  } catch {
    // localStorage unavailable (private browsing, quota, etc.) — silent.
  }
}

/** Clear the persisted morphology config. Returns the defaults. */
export function resetMorphConfig(): MorphologyConfig {
  try { localStorage.removeItem(LS_KEY); } catch { /* ignore */ }
  return structuredClone(DEFAULT_MORPH_CONFIG);
}

/** Serialize to the JSON string the WASM `set_morphology_config(json)` expects. */
export function morphConfigToJson(cfg: MorphologyConfig): string {
  return JSON.stringify(cfg);
}
