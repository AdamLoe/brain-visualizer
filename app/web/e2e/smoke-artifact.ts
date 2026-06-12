import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

export type SmokeStatus = "passed" | "failed" | "skipped";

export interface AdapterSmokeFields {
  gpuPresent: boolean;
  hasAdapter: boolean;
  adapterDescription: string | null;
  adapterVendor: string | null;
  adapterArchitecture: string | null;
}

export interface StartupSmokeFields {
  status: string | null;
  stage: string | null;
  progress: number | null;
  elapsedMs: number | null;
  backendMs: number | null;
  frames: number | null;
  timingCount: number;
  timings: Array<{ name: string; ms: number }>;
}

export interface CanvasSmokeFields {
  sampled: boolean;
  width: number;
  height: number;
  sampleCount: number;
  meanLuma: number | null;
  varianceLuma: number | null;
  minLuma: number | null;
  maxLuma: number | null;
  nonBlackRatio: number | null;
  error: string | null;
}

export interface FrameHealthSmokeFields {
  sampleDurationMs: number;
  framesAdvanced: number;
  fpsFromCounter: number;
  profilerFps: number | null;
  frameAvgMs: number | null;
  frameP95Ms: number | null;
}

export interface RealHardwareSmokeArtifact {
  schemaVersion: 1;
  status: SmokeStatus;
  requireWebGpu: boolean;
  reason: string | null;
  baseURL: string | undefined;
  browserName: string;
  generatedAt: string;
  adapter: AdapterSmokeFields;
  startup: StartupSmokeFields;
  canvas: CanvasSmokeFields;
  frameHealth: FrameHealthSmokeFields | null;
  screenshotPath: string;
}

export async function writeSmokeArtifact(
  path: string,
  artifact: RealHardwareSmokeArtifact,
): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, `${JSON.stringify(artifact, null, 2)}\n`, "utf8");
}
