import {
  DEFAULT_CONFIG,
  type AppConfig,
} from "./types";

export function applyMobileConfig(config: AppConfig): void {
  config.tier = DEFAULT_CONFIG.tier;
  config.n = Math.min(config.n, DEFAULT_CONFIG.n);
  config.k = DEFAULT_CONFIG.k;
  config.backend = "gpu";
  config.regionAssignmentMode = DEFAULT_CONFIG.regionAssignmentMode;
}
