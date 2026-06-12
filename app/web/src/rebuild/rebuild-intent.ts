import type { MorphologyConfig } from "../core/morph-config";
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

export function morphConfigRequiresPreparedNetwork(
  appliedJson: string,
  incomingJson: string,
): boolean {
  const applied = parseMorphConfig(appliedJson);
  const incoming = parseMorphConfig(incomingJson);
  if (applied === null || incoming === null) return false;
  return JSON.stringify(applied.generator) !== JSON.stringify(incoming.generator);
}

function parseMorphConfig(json: string): MorphologyConfig | null {
  try {
    return JSON.parse(json) as MorphologyConfig;
  } catch {
    return null;
  }
}
