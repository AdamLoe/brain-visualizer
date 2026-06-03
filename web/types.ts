// Shared TypeScript types mirroring the Rust enums (src/sim/backend.rs).
// Phase 1: thin definitions so controls/main can be wired and typecheck.

export type SpeedPreset = "quarter" | "half" | "normal" | "double";
export type BackendKind = "gpu" | "cpu";
export type Tier = "low" | "balanced" | "max";
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
}

export const DEFAULT_CONFIG: AppConfig = {
  n: 50_000,
  k: 32,
  seed: 0x5eed5eed,
  tier: "balanced",
  speed: "normal",
  backend: "gpu",
  excitability: 0.3, // "relaxed"
};
