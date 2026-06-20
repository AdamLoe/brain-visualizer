import { MORPH_CONFIG_LS_KEY } from "./core/morph-config";
import { SETTINGS_LS_KEY } from "./core/settings";
import { CONFIG_LS_KEY } from "./core/types";

const WEBGPU_GUIDANCE =
  "This experience needs WebGPU. Try current Chrome or Edge with hardware acceleration enabled.";

export const APP_OWNED_STORAGE_KEYS = [
  CONFIG_LS_KEY,
  SETTINGS_LS_KEY,
  MORPH_CONFIG_LS_KEY,
] as const;

export function hasWebGpuSupport(navigatorLike: Pick<Navigator, "gpu"> | object | undefined): boolean {
  return typeof navigatorLike === "object" && navigatorLike !== null && "gpu" in navigatorLike;
}

export function webGpuUnsupportedStage(): string {
  return `${WEBGPU_GUIDANCE} No CPU/WebGL fallback is available.`;
}

export function webGpuStartupFailureStage(): string {
  return `${WEBGPU_GUIDANCE} If it still fails, update graphics drivers or try another device.`;
}

export function resetAppOwnedStorage(storage: Pick<Storage, "removeItem"> = localStorage): void {
  for (const key of APP_OWNED_STORAGE_KEYS) {
    storage.removeItem(key);
  }
}

export type DiagnosticsPolicy = "desktop-supported" | "unsupported-mobile";

export function diagnosticsPolicyForViewport(width: number, mobileUserAgent: boolean): DiagnosticsPolicy {
  return mobileUserAgent || width < 768 ? "unsupported-mobile" : "desktop-supported";
}
