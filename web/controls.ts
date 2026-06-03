// Control stubs (BV12, BV14, BV15, BV16). Phase 1: functions exist and are
// callable from the console / wired to DOM later. Speed changes the tick rate
// (visible in the profiler dump); brain state sets excitability; backend/tier
// changes are rare-path restart requests (no real restart yet).

import type {
  AppConfig,
  BackendKind,
  BrainState,
  SpeedPreset,
  Tier,
} from "./types";

// Brain state → excitability (BV15). Locked values.
export const BRAIN_STATES: Record<BrainState, number> = {
  deep_sleep: 0.1,
  relaxed: 0.3,
  focused: 0.55,
  hyperstimulated: 0.8,
  seizure: 1.0,
};

// Callback the host registers so controls can request a backend/tier restart
// (full teardown + restart, same seed — BV16). Phase 1: logs only.
type RestartFn = (config: AppConfig) => void;

export class Controls {
  constructor(
    private config: AppConfig,
    private onRestart: RestartFn,
  ) {}

  setSpeed(preset: SpeedPreset): void {
    this.config.speed = preset;
    console.log(`[controls] speed = ${preset}`);
  }

  setBrainState(state: BrainState): void {
    this.config.excitability = BRAIN_STATES[state];
    console.log(
      `[controls] brain state = ${state} (excitability ${this.config.excitability})`,
    );
  }

  setBackend(kind: BackendKind): void {
    if (kind === this.config.backend) return;
    this.config.backend = kind;
    console.log(`[controls] backend = ${kind} → restart (same seed)`);
    this.onRestart(this.config); // BV16 full restart (stub)
  }

  setTier(tier: Tier): void {
    if (tier === this.config.tier) return;
    this.config.tier = tier;
    console.log(`[controls] tier = ${tier} → restart`);
    this.onRestart(this.config);
  }
}

// ticksThisFrame: BV14 speed → ticks-per-frame mapping. Quarter = 1 tick every
// 4 frames, half = 1 every 2, normal = 1, double = 2. Pure & exported for use
// in the rAF loop and (potential) unit tests.
export function ticksThisFrame(
  speed: SpeedPreset,
  frameCounter: number,
): number {
  switch (speed) {
    case "quarter":
      return frameCounter % 4 === 0 ? 1 : 0;
    case "half":
      return frameCounter % 2 === 0 ? 1 : 0;
    case "normal":
      return 1;
    case "double":
      return 2;
  }
}
