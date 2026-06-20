const WEBGPU_GUIDANCE =
  "This experience needs WebGPU. Try current Chrome or Edge with hardware acceleration enabled.";

export function hasWebGpuSupport(navigatorLike: Pick<Navigator, "gpu"> | object | undefined): boolean {
  return typeof navigatorLike === "object" && navigatorLike !== null && "gpu" in navigatorLike;
}

export function webGpuUnsupportedStage(): string {
  return `${WEBGPU_GUIDANCE} No CPU/WebGL fallback is available.`;
}

export function webGpuStartupFailureStage(): string {
  return `${WEBGPU_GUIDANCE} If it still fails, update graphics drivers or try another device.`;
}
