// Shared TypeScript types mirroring the Rust enums (src/sim/backend.rs).

export type SpeedPreset = "quarter" | "half" | "normal" | "double";
export type BackendKind = "gpu";
export type Tier = "basic" | "low" | "balanced" | "max";
export type RegionAssignmentMode = "hash-random" | "anterior-posterior-prototype";
export type BrainState =
  | "deep_sleep"
  | "relaxed"
  | "focused"
  | "hyperstimulated"
  | "seizure";

// Per-frame sim statistics (mirrors Rust TickStats).
export interface TickStats {
  tickCount: number;
  spikes: number;
  synapticEvents: number;
  tickMs: number;
}

export const ZERO_STATS: TickStats = {
  tickCount: 0,
  spikes: 0,
  synapticEvents: 0,
  tickMs: 0,
};

// Runtime config the harness reads (subset of Rust SimConfig relevant to JS).
export interface AppConfig {
  n: number;
  k: number;
  seed: number;
  tier: Tier;
  speed: SpeedPreset;
  backend: BackendKind;
  regionAssignmentMode: RegionAssignmentMode;
  excitability: number;
  // Live sim speed in ticks/sec (1–60), driven by the dev-panel Speed slider.
  // The `speed` SpeedPreset above is the legacy frame-counter knob (controls.ts);
  // the GPU rAF loop uses this numeric value via `targetTicksPerSec`.
  ticksPerSec: number;
}

export const PRODUCT_MAX_N = 20_000;

export function clampNeuronCount(n: number): number {
  const value = Number.isFinite(n) ? Math.round(n) : DEFAULT_CONFIG.n;
  return Math.max(1, Math.min(PRODUCT_MAX_N, value));
}

// v0.5.x high-scale baseline: 6000 neurons, low excitability for calm first load.
// The public tier controls remain explicit opt-in presets; the default app
// config is the accepted-default source of truth for first load.
export const DEFAULT_CONFIG: AppConfig = {
  n: 6_000,
  k: 16,
  seed: 0x5eed5eed,
  tier: "low",
  speed: "normal",
  backend: "gpu",
  regionAssignmentMode: "hash-random",
  excitability: 0.10, // boot default = BRAIN_STATES.deep_sleep (calm first load)
  ticksPerSec: 30, // default 30 ticks/sec (matches the rAF-loop default)
};

// ─── AppConfig persistence (0.1.1) ────────────────────────────────────────────
// Mirrors the settings.ts pattern: a versioned localStorage key, version-gate →
// defaults on mismatch, field-by-field `?? base` merge, try/catch around storage.
// Persists ONLY user-chosen scaling/runtime knobs (n, k, tier, backend, speed,
// excitability). `seed` is a fixed constant and is NOT persisted; no runtime
// counters are persisted. On missing/parse-error/version-mismatch → DEFAULT_CONFIG.
export const CONFIG_LS_KEY = "bv2_config_v2";

/** Subset of AppConfig persisted to localStorage (no seed, no runtime counters). */
interface SavedConfig {
  version: 1;
  n: number;
  k: number;
  tier: Tier;
  backend?: string;
  regionAssignmentMode?: string;
  speed: SpeedPreset;
  excitability: number;
  ticksPerSec: number;
}

function normalizeBackend(_backend: unknown): BackendKind {
  return "gpu";
}

export function normalizeRegionAssignmentMode(value: unknown): RegionAssignmentMode {
  return value === "anterior-posterior-prototype"
    ? "anterior-posterior-prototype"
    : "hash-random";
}

/** Load persisted config merged over DEFAULT_CONFIG. Returns a full AppConfig;
 *  on missing key / parse error / version mismatch returns {...DEFAULT_CONFIG}. */
export function loadConfig(): AppConfig {
  try {
    const raw = localStorage.getItem(CONFIG_LS_KEY);
    if (!raw) return { ...DEFAULT_CONFIG };
    const parsed = JSON.parse(raw) as Partial<SavedConfig> & { version?: number };
    if (parsed.version !== 1) return { ...DEFAULT_CONFIG };
    const base = DEFAULT_CONFIG;
    return {
      ...base,
      n:            clampNeuronCount(parsed.n ?? base.n),
      k:            parsed.k            ?? base.k,
      tier:         parsed.tier         ?? base.tier,
      backend:      normalizeBackend(parsed.backend ?? base.backend),
      regionAssignmentMode: normalizeRegionAssignmentMode(parsed.regionAssignmentMode ?? base.regionAssignmentMode),
      speed:        parsed.speed        ?? base.speed,
      excitability: parsed.excitability ?? base.excitability,
      ticksPerSec:  parsed.ticksPerSec  ?? base.ticksPerSec,
    };
  } catch {
    return { ...DEFAULT_CONFIG };
  }
}

/** Persist the user-chosen subset of AppConfig to localStorage (silent on error). */
export function saveConfig(c: AppConfig): void {
  try {
    const saved: SavedConfig = {
      version: 1,
      n: clampNeuronCount(c.n),
      k: c.k,
      tier: c.tier,
      backend: c.backend,
      regionAssignmentMode: normalizeRegionAssignmentMode(c.regionAssignmentMode),
      speed: c.speed,
      excitability: c.excitability,
      ticksPerSec: c.ticksPerSec,
    };
    localStorage.setItem(CONFIG_LS_KEY, JSON.stringify(saved));
  } catch {
    // localStorage unavailable (private browsing, quota, etc.) — silent.
  }
}

/** Clear persisted AppConfig and return a fresh default snapshot. */
export function resetConfig(): AppConfig {
  try {
    localStorage.removeItem(CONFIG_LS_KEY);
  } catch {
    // localStorage unavailable (private browsing, quota, etc.) — silent.
  }
  return { ...DEFAULT_CONFIG };
}
