// web/setting-metadata.ts — V2 Phase B
//
// Single source of truth for setting impact classification.
// Every control in the dev panel (and any future UI) reads impact from here.
//
// Impact levels:
//   "live"             — takes effect immediately via update_settings (green dot)
//   "brain-reset"      — requires reinitialize() with same seed (yellow dot)
//   "renderer-rebuild" — requires a full pipeline rebuild (red dot)
//
// Classification follows the Phase B plan exactly:
//   live: all visual knobs + sim drive knobs + mode enums whose effect is
//         purely a shader/renderer concern with no structural network change.
//   brain-reset: heterogeneity (per-neuron param spread), weightNormalization
//                (changes connectivity scale), inputMode (changes drive pattern).
//   renderer-rebuild: none in Phase B — N/K/tier live in AppConfig, out of scope.

import type { VisualizerSettings } from "./settings";

// ── Impact types ─────────────────────────────────────────────────────────────

export type SettingImpact = "live" | "brain-reset" | "renderer-rebuild";

// ── Full classification table ─────────────────────────────────────────────────

export const SETTING_IMPACT: Record<keyof VisualizerSettings, SettingImpact> = {
  // ── Visual continuous knobs — all live ─────────────────────────────────────
  glowTau:                  "live",
  pointRadius:              "live",
  neuronVisualRadius:       "live",
  activeNeuronRadiusBoost:  "live",
  inactiveNeuronOpacity:    "live",
  voltageGlowStrength:      "live",
  connectionVisualWidth:    "live", // Morphology: branch-width multiplier (live)
  connectionCurveLift:      "renderer-rebuild", // Morphology: regenerates geometry on apply/release
  connectionLightNext:      "live",
  bloomStrength:            "live",
  surfaceOpacity:           "live",
  // ── Sim drive knobs — live ─────────────────────────────────────────────────
  iExt:                     "live",
  synapticScale:            "live",
  // ── Structural sim param — now live (UX round 2: integrated uniform read every tick) ──
  heterogeneity:            "live",
  // ── Morphology resting opacity — live ─────────────────────────────────────
  morphRestingOpacity:      "live",
  // ── Mode enums — live (renderer reads them from settings uniform) ──────────
  signalSource:             "live",
  connectionLayer:          "live",
  colorBy:                  "live",
  neuronVisibility:         "live",
  surface:                  "live",
  // ── Structural sim params — now live (UX round 2: integrated uniform read every tick) ──
  weightNormalization:      "live",
  inputMode:                "live",
  // ── index 23 reserved/inert — auto-scaling removed in 0.1.1 (contract kept) ──
  adaptiveScalerEnabled:    "live",
  // ── Heavy-tailed reach — brain-reset: changes target ids + generated geometry ──
  longRangeReachFrac:       "brain-reset",
  maxReachCells:            "brain-reset",
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/** CSS color string for an impact level. */
export function impactColor(impact: SettingImpact): string {
  switch (impact) {
    case "live":             return "rgba(100, 230, 140, 0.95)";   // green
    case "brain-reset":      return "rgba(240, 200,  60, 0.95)";   // yellow
    case "renderer-rebuild": return "rgba(255, 100,  80, 0.95)";   // red
  }
}

/** Short human label for an impact level. */
export function impactLabel(impact: SettingImpact): string {
  switch (impact) {
    case "live":             return "Live";
    case "brain-reset":      return "Brain reset";
    case "renderer-rebuild": return "Renderer rebuild";
  }
}
