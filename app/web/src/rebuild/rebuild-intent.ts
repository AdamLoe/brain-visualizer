import type { VisualizerSettings } from "../core/settings";

const STRUCTURAL_SETTING_KEYS = [
  "connectionCurveLift",
  "longRangeReachFrac",
  "maxReachCells",
] as const satisfies readonly (keyof VisualizerSettings)[];

export function settingsRequirePreparedNetwork(
  previous: VisualizerSettings,
  next: VisualizerSettings,
): boolean {
  return STRUCTURAL_SETTING_KEYS.some((key) => previous[key] !== next[key]);
}
