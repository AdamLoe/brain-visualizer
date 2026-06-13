import "./controls.css";
// Phase 5 — Controls & Brain States UI (BV12, BV14, BV15, BV16).
// Replaces the Phase 1 stubs with real UI wiring, smooth excitability lerp,
// backend-availability gating, and the adaptive scaler activation.

import type {
  AppConfig,
  BackendKind,
  BrainState,
  SpeedPreset,
  Tier,
} from "../core/types";
import { PRODUCT_MAX_N, saveConfig } from "../core/types";

// ── Brain state → excitability (BV15) ────────────────────────────────────────
// Locked values per spec. Keys match the HTML data-state attributes.
export const BRAIN_STATES: Record<BrainState, number> = {
  deep_sleep:      0.10,
  relaxed:         0.30,
  focused:         0.63,
  hyperstimulated: 0.71,
  seizure:         1.00,
};

// ── Excitability smooth lerp ──────────────────────────────────────────────────
// Avoids jarring instant jumps (deep_sleep → seizure takes ~30 frames to cross).
let _targetExcitability  = BRAIN_STATES.hyperstimulated;
let _currentExcitability = BRAIN_STATES.hyperstimulated;
const EXCITABILITY_LERP  = 0.08; // fraction per frame

/**
 * Advance the excitability lerp one step toward the target and return the
 * new current value. Call once per rAF frame; pass the result to backend.tick().
 *
 * Pure enough to unit-test: the only side-effect is mutating the two
 * module-level variables (which are observable via getCurrentExcitability /
 * setExcitabilityForTest).
 */
export function tickExcitability(): number {
  _currentExcitability += (_targetExcitability - _currentExcitability) * EXCITABILITY_LERP;
  return _currentExcitability;
}

/** Read current excitability (useful for tests). */
export function getCurrentExcitability(): number {
  return _currentExcitability;
}

/**
 * Set the excitability target directly (UX overhaul — used by devPanel sim handler).
 * Clamped to [0, 1].  The existing lerp in tickExcitability() smoothly approaches
 * the new target over subsequent frames.
 */
export function setExcitabilityTarget(v: number): void {
  _targetExcitability = Math.max(0, Math.min(1, v));
}

/** Forcibly set both target and current (for test reset). */
export function setExcitabilityForTest(current: number, target: number): void {
  _currentExcitability = current;
  _targetExcitability  = target;
}

/**
 * Seed both target and current excitability at boot from the persisted config,
 * so a reload restores the user's last excitability without a visible ramp from
 * the hardcoded default. (0.1.1: brain-state/excitability persistence.)
 */
export function seedExcitability(v: number): void {
  const clamped = Math.max(0, Math.min(1, v));
  _currentExcitability = clamped;
  _targetExcitability  = clamped;
}

// ── Speed → ticks-per-frame mapping (BV14) ────────────────────────────────────
// Pure function: quarter=1 tick every 4 frames, half=every 2, normal=1, double=2.
// Exported for use in the rAF loop and unit tests.
export function ticksThisFrame(speed: SpeedPreset, frameCounter: number): number {
  switch (speed) {
    case "quarter": return frameCounter % 4 === 0 ? 1 : 0;
    case "half":    return frameCounter % 2 === 0 ? 1 : 0;
    case "normal":  return 1;
    case "double":  return 2;
  }
}

// ── Toast notification ───────────────────────────────────────────────────────
let _toastTimer: ReturnType<typeof setTimeout> | null = null;

export function showToast(msg: string, durationMs = 2500): void {
  const el = document.getElementById("toast");
  if (!el) return;
  el.textContent = msg;
  el.classList.add("visible");
  if (_toastTimer !== null) clearTimeout(_toastTimer);
  _toastTimer = setTimeout(() => {
    el.classList.remove("visible");
    _toastTimer = null;
  }, durationMs);
}

// ── DOM helpers ──────────────────────────────────────────────────────────────
export function setActiveButton(groupSelector: string, matchAttr: string, value: string): void {
  document.querySelectorAll(`${groupSelector} button`).forEach((b) => {
    (b as HTMLElement).classList.toggle(
      "active",
      (b as HTMLElement).dataset[matchAttr] === value,
    );
  });
}

// ── setBrainState ─────────────────────────────────────────────────────────────
export function setBrainState(state: BrainState): void {
  _targetExcitability = BRAIN_STATES[state];
  setActiveButton("#brain-state-group", "state", state);
}

// ── setSpeed ─────────────────────────────────────────────────────────────────
export function setSpeed(preset: SpeedPreset, config: AppConfig): void {
  config.speed = preset;
  saveConfig(config); // 0.1.1: persist so the choice survives a reload
  setActiveButton("#speed-group", "speed", preset);
}

// ── setBackend ────────────────────────────────────────────────────────────────
// Triggers the full restart sequence when the backend is available.
export async function setBackend(
  kind: BackendKind,
  config: AppConfig,
  restartFn: (kind: BackendKind) => Promise<void>,
): Promise<void> {
  if (kind === config.backend) return;
  // Update DOM active state immediately; the restart is quick.
  setActiveButton("#backend-toggle", "backend", kind);
  await restartFn(kind);
  saveConfig(config);
}

// ── setTier ───────────────────────────────────────────────────────────────────
export function setTier(
  tier: Tier,
  config: AppConfig,
  restartFn: (kind: BackendKind) => Promise<void>,
): void {
  if (tier === config.tier) return;
  config.tier = tier;
  saveConfig(config); // 0.1.1: persist the user's tier so it survives a reload
  // Tier change requires restart with same backend to allocate new N/K.
  void restartFn(config.backend);
}

// ── V2 Phase F: Tier → (n, k) preset map ────────────────────────────────────
// Legacy control facade only; the dev panel no longer exposes high-N tier UI.
// Presets remain below the product cap so old callers cannot request >20k.
export interface TierPreset { n: number; k: number; }
export const TIER_PRESETS: Record<Tier, TierPreset> = {
  basic:    { n:   2_000, k: 16 },
  low:      { n:  10_000, k: 16 },
  balanced: { n:  15_000, k: 16 },
  max:      { n: PRODUCT_MAX_N, k: 16 },
};

// ── Adaptive scaler (BV1 / Phase 5 activation) ───────────────────────────────
// Per-tier N bounds (must stay within tier — never auto-change tier).
export const N_MIN: Record<Tier, number> = {
  basic:    50,
  low:      1_000,
  balanced: 5_000,
  max:      10_000,
};
export const N_MAX: Record<Tier, number> = {
  basic:    1_000,
  low:      10_000,
  balanced: 15_000,
  max:      PRODUCT_MAX_N,
};

const FRAME_BUDGET_MS   = 14;    // 60 fps with ~2 ms headroom
const SCALER_COOLDOWN_MS = 3000; // at most one resize per 3 s
const SCALER_SHRINK_RATE = 0.9;  // −10% per step
const SCALER_GROW_RATE   = 1.1;  // +10% per step
const HEADROOM_FACTOR    = 0.7;  // grow only when p95 < 70% of budget

/** Action returned by scalerDecide (pure, testable). */
export type ScalerAction =
  | { kind: "none" }
  | { kind: "shrink_n"; newN: number }
  | { kind: "grow_n";   newN: number };

/**
 * Pure scaler decision function.
 *
 * Inputs:
 *   p95Ms          - frame-time 95th-percentile in milliseconds
 *   currentN       - current neuron count
 *   tier           - currently selected tier (Low / Balanced / Max)
 *   timeSinceResizeMs - ms since last resize (enforce cooldown)
 *   duringRestart  - true while a backend restart or device-loss recovery is in progress
 *
 * Returns a ScalerAction.  Caller applies newN to config and calls backend.resize().
 *
 * The scaler operates ONLY within the selected tier's [N_MIN, N_MAX] range.
 * It never changes tier.  Tier-changing is always an explicit user action.
 *
 * The doc's priority order for reducing cost before cutting N:
 *   1. Disable near-LOD (cull cost)
 *   2. Reduce render resolution (future: scale canvas DPR)
 *   3. Reduce N
 * In this implementation N is the only knob available without a full restart
 * (near-LOD and resolution reduction are Phase 7 optional cost centres).
 * The shrink action is still labelled shrink_n so Phase 7 can slot in higher-
 * priority actions before it.
 */
export function scalerDecide(
  p95Ms: number,
  currentN: number,
  tier: Tier,
  timeSinceResizeMs: number,
  duringRestart: boolean,
): ScalerAction {
  if (duringRestart) return { kind: "none" };
  if (timeSinceResizeMs < SCALER_COOLDOWN_MS) return { kind: "none" };

  if (p95Ms > FRAME_BUDGET_MS && currentN > N_MIN[tier]) {
    const newN = Math.min(
      N_MAX[tier],
      Math.max(N_MIN[tier], Math.floor(currentN * SCALER_SHRINK_RATE)),
    );
    return { kind: "shrink_n", newN };
  }

  if (p95Ms < FRAME_BUDGET_MS * HEADROOM_FACTOR && currentN < N_MAX[tier]) {
    const newN = Math.min(
      N_MAX[tier],
      Math.max(N_MIN[tier], Math.floor(currentN * SCALER_GROW_RATE)),
    );
    return { kind: "grow_n", newN };
  }

  return { kind: "none" };
}

// ── Mobile detection ──────────────────────────────────────────────────────────
/**
 * Returns true if running on a mobile/touch device.
 * Mobile defaults: Low tier, skip cursor stimulation (BV10 / Phase 5).
 */
export function isMobile(): boolean {
  return /Mobi|Android/i.test(navigator.userAgent) || window.innerWidth < 768;
}

// ── Controls class (backwards-compat facade) ──────────────────────────────────
// Kept so any code that imported `Controls` from phase 1 still compiles.
// setSpeed/setBrainState/setBackend/setTier now delegate to the module-level
// functions above; main.ts wires DOM click handlers directly instead.
type RestartFn = (config: AppConfig) => void;

export class Controls {
  constructor(
    private config: AppConfig,
    private onRestart: RestartFn,
  ) {}

  setSpeed(preset: SpeedPreset): void {
    setSpeed(preset, this.config);
    console.log(`[controls] speed = ${preset}`);
  }

  setBrainState(state: BrainState): void {
    setBrainState(state);
    console.log(
      `[controls] brain state = ${state} (target excitability ${BRAIN_STATES[state]})`,
    );
  }

  setBackend(kind: BackendKind): void {
    if (kind === this.config.backend) return;
    this.config.backend = kind;
    saveConfig(this.config); // 0.1.1: persist user's backend choice
    setActiveButton("#backend-toggle", "backend", kind);
    console.log(`[controls] backend = ${kind} → restart (same seed)`);
    this.onRestart(this.config);
  }

  setTier(tier: Tier): void {
    if (tier === this.config.tier) return;
    this.config.tier = tier;
    saveConfig(this.config); // 0.1.1: persist user's tier choice
    console.log(`[controls] tier = ${tier} → restart`);
    this.onRestart(this.config);
  }
}
