// Shared TypeScript types mirroring the Rust enums (src/sim/backend.rs).
// Phase 1: thin definitions so controls/main can be wired and typecheck.

export type SpeedPreset = "quarter" | "half" | "normal" | "double";
export type BackendKind = "gpu" | "cpu";
export type Tier = "basic" | "low" | "balanced" | "max";
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
  excitability: number;
  // Live sim speed in ticks/sec (1–60), driven by the dev-panel Speed slider.
  // The `speed` SpeedPreset above is the legacy frame-counter knob (controls.ts);
  // the GPU rAF loop uses this numeric value via `targetTicksPerSec`.
  ticksPerSec: number;
}

// V2 Phase 0 (beauty-first): default to 10k/16 — the beauty target.
// Tiers and the adaptive scaler are demoted to opt-in (Phase F re-arms them).
// V2 Phase F: tier "low" = 10k/16 — matches the default N so the highlighted
// tier button is consistent with what actually boots.
// Morphology (beauty-first): scaled DOWN to ~1200 neurons so each can be drawn
// as real procedural morphology (soma + dendrite tree + axon arbor) and read
// unmistakably as neurons. Sim stays K=16 (dynamics unchanged).
export const DEFAULT_CONFIG: AppConfig = {
  n: 1_200,
  k: 16,
  seed: 0x5eed5eed,
  tier: "low",
  speed: "normal",
  backend: "gpu",
  excitability: 0.71, // boot default = BRAIN_STATES.hyperstimulated (matches the live lerp seed)
  ticksPerSec: 30, // default 30 ticks/sec (matches the rAF-loop default)
};

// ─── AppConfig persistence (0.1.1) ────────────────────────────────────────────
// Mirrors the settings.ts pattern: a versioned localStorage key, version-gate →
// defaults on mismatch, field-by-field `?? base` merge, try/catch around storage.
// Persists ONLY user-chosen scaling/runtime knobs (n, k, tier, backend, speed,
// excitability). `seed` is a fixed constant and is NOT persisted; no runtime
// counters are persisted. On missing/parse-error/version-mismatch → DEFAULT_CONFIG.
const CONFIG_LS_KEY = "bv2_config_v1";

/** Subset of AppConfig persisted to localStorage (no seed, no runtime counters). */
interface SavedConfig {
  version: 1;
  n: number;
  k: number;
  tier: Tier;
  backend: BackendKind;
  speed: SpeedPreset;
  excitability: number;
  ticksPerSec: number;
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
      n:            parsed.n            ?? base.n,
      k:            parsed.k            ?? base.k,
      tier:         parsed.tier         ?? base.tier,
      backend:      parsed.backend      ?? base.backend,
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
      n: c.n,
      k: c.k,
      tier: c.tier,
      backend: c.backend,
      speed: c.speed,
      excitability: c.excitability,
      ticksPerSec: c.ticksPerSec,
    };
    localStorage.setItem(CONFIG_LS_KEY, JSON.stringify(saved));
  } catch {
    // localStorage unavailable (private browsing, quota, etc.) — silent.
  }
}
