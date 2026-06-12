import { expect, test, type Page } from "@playwright/test";
import {
  type AdapterSmokeFields,
  type CanvasSmokeFields,
  type FrameHealthSmokeFields,
  type StartupSmokeFields,
  writeSmokeArtifact,
} from "./smoke-artifact";

interface ProfilerConsoleSnapshot {
  fps?: number;
  frame_ms_avg?: number;
  frame_ms_p95?: number;
}

interface StartupWindowState {
  status?: string;
  stage?: string;
  progress?: number;
  elapsedMs?: number;
  backendMs?: number;
  frames?: number;
  timings?: Array<{ name?: string; ms?: number }>;
}

const REQUIRE_WEBGPU = process.env.BV_REQUIRE_WEBGPU === "1";
const FRAME_SAMPLE_MS = 2_000;

test("real hardware smoke writes startup, adapter, canvas, and frame artifacts", async ({
  browserName,
  page,
  baseURL,
}, testInfo) => {
  const profilerSnapshots: ProfilerConsoleSnapshot[] = [];
  page.on("console", (msg) => {
    const text = msg.text();
    if (!text.startsWith("{")) return;
    try {
      const parsed = JSON.parse(text) as ProfilerConsoleSnapshot;
      if (
        typeof parsed.fps === "number" ||
        typeof parsed.frame_ms_p95 === "number"
      ) {
        profilerSnapshots.push(parsed);
      }
    } catch {
      // Non-profiler JSON logs are ignored.
    }
  });

  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => (window as unknown as Record<string, unknown>).__bvFrameCounter !== undefined,
    { timeout: 20_000 },
  );

  const adapter = await collectAdapter(page);
  const terminalStartup = await waitForStartupTerminal(page);
  const startup = normalizeStartup(terminalStartup);

  const screenshotPath = testInfo.outputPath("real-hardware-smoke.png");
  await page.screenshot({ path: screenshotPath, fullPage: true });

  if (!adapter.hasAdapter) {
    const reason =
      "WebGPU adapter unavailable; set BV_REQUIRE_WEBGPU=1 to fail this smoke instead of recording an environment skip.";
    const canvas = await collectCanvasEvidence(page);
    const artifactPath = testInfo.outputPath("real-hardware-smoke.json");
    await writeSmokeArtifact(artifactPath, {
      schemaVersion: 1,
      status: REQUIRE_WEBGPU ? "failed" : "skipped",
      requireWebGpu: REQUIRE_WEBGPU,
      reason,
      baseURL,
      browserName,
      generatedAt: new Date().toISOString(),
      adapter,
      startup,
      canvas,
      frameHealth: null,
      screenshotPath,
    });
    console.log(`[SMOKE-ARTIFACT] ${artifactPath}`);

    if (REQUIRE_WEBGPU) {
      expect(
        adapter.hasAdapter,
        `${reason} Artifact: ${artifactPath}`,
      ).toBe(true);
    }
    return;
  }

  const canvas = await collectCanvasEvidence(page);
  if (startup.status !== "ready") {
    const artifactPath = testInfo.outputPath("real-hardware-smoke.json");
    await writeSmokeArtifact(artifactPath, {
      schemaVersion: 1,
      status: "failed",
      requireWebGpu: REQUIRE_WEBGPU,
      reason: `startupStatus=${startup.status ?? "unknown"}`,
      baseURL,
      browserName,
      generatedAt: new Date().toISOString(),
      adapter,
      startup,
      canvas,
      frameHealth: null,
      screenshotPath,
    });
    console.log(`[SMOKE-ARTIFACT] ${artifactPath}`);
    expect(startup.status, `Startup should reach ready with a WebGPU adapter. Artifact: ${artifactPath}`).toBe("ready");
    return;
  }

  const frameHealth = await collectFrameHealth(page, profilerSnapshots);
  const frameFloorMet = frameHealth.framesAdvanced >= 10 && frameHealth.fpsFromCounter > 5;
  const canvasNonblank = canvas.sampled &&
    (canvas.nonBlackRatio ?? 0) > 0.01 &&
    (canvas.varianceLuma ?? 0) > 0.5;
  const status = frameFloorMet && canvasNonblank ? "passed" : "failed";
  const reason = status === "passed"
    ? null
    : `frameFloorMet=${frameFloorMet} canvasNonblank=${canvasNonblank}`;

  const artifactPath = testInfo.outputPath("real-hardware-smoke.json");
  await writeSmokeArtifact(artifactPath, {
    schemaVersion: 1,
    status,
    requireWebGpu: REQUIRE_WEBGPU,
    reason,
    baseURL,
    browserName,
    generatedAt: new Date().toISOString(),
    adapter,
    startup,
    canvas,
    frameHealth,
    screenshotPath,
  });
  console.log(`[SMOKE-ARTIFACT] ${artifactPath}`);

  expect(frameFloorMet, `Frame health floor failed. Artifact: ${artifactPath}`).toBe(true);
  expect(canvasNonblank, `Canvas nonblank/variance check failed. Artifact: ${artifactPath}`).toBe(true);
});

async function collectAdapter(page: Page): Promise<AdapterSmokeFields> {
  return page.evaluate(async () => {
    const gpuPresent = "gpu" in navigator;
    if (!gpuPresent) {
      return {
        gpuPresent: false,
        hasAdapter: false,
        adapterDescription: null,
        adapterVendor: null,
        adapterArchitecture: null,
      };
    }

    try {
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) {
        return {
          gpuPresent,
          hasAdapter: false,
          adapterDescription: null,
          adapterVendor: null,
          adapterArchitecture: null,
        };
      }
      const adapterWithInfo = adapter as GPUAdapter & {
        requestAdapterInfo?: () => Promise<{
          vendor?: string;
          architecture?: string;
          description?: string;
        }>;
        info?: {
          vendor?: string;
          architecture?: string;
          description?: string;
        };
      };
      const info = typeof adapterWithInfo.requestAdapterInfo === "function"
        ? await adapterWithInfo.requestAdapterInfo()
        : adapterWithInfo.info;
      const vendor = info?.vendor ?? null;
      const architecture = info?.architecture ?? null;
      const description = info?.description ?? [vendor, architecture].filter(Boolean).join(" / ");
      return {
        gpuPresent,
        hasAdapter: true,
        adapterDescription: description || null,
        adapterVendor: vendor,
        adapterArchitecture: architecture,
      };
    } catch {
      return {
        gpuPresent,
        hasAdapter: false,
        adapterDescription: null,
        adapterVendor: null,
        adapterArchitecture: null,
      };
    }
  });
}

async function waitForStartupTerminal(page: Page): Promise<StartupWindowState | null> {
  try {
    await page.waitForFunction(
      () => {
        const startup = (window as unknown as { __bvStartup?: StartupWindowState }).__bvStartup;
        return startup?.status === "ready" || startup?.status === "failed";
      },
      { timeout: 30_000 },
    );
  } catch {
    // The frame counter is already alive; the artifact records the latest state.
  }
  return page.evaluate(
    () => (window as unknown as { __bvStartup?: StartupWindowState }).__bvStartup ?? null,
  );
}

function normalizeStartup(startup: StartupWindowState | null): StartupSmokeFields {
  const timings = (startup?.timings ?? [])
    .filter((timing) => typeof timing.name === "string" && typeof timing.ms === "number")
    .map((timing) => ({ name: timing.name as string, ms: timing.ms as number }));
  return {
    status: startup?.status ?? null,
    stage: startup?.stage ?? null,
    progress: typeof startup?.progress === "number" ? startup.progress : null,
    elapsedMs: typeof startup?.elapsedMs === "number" ? startup.elapsedMs : null,
    backendMs: typeof startup?.backendMs === "number" ? startup.backendMs : null,
    frames: typeof startup?.frames === "number" ? startup.frames : null,
    timingCount: timings.length,
    timings,
  };
}

async function collectFrameHealth(
  page: Page,
  profilerSnapshots: ProfilerConsoleSnapshot[],
): Promise<FrameHealthSmokeFields> {
  const start = await frameCounter(page);
  const startedAt = Date.now();
  await page.waitForTimeout(FRAME_SAMPLE_MS);
  const sampleDurationMs = Date.now() - startedAt;
  const end = await frameCounter(page);
  const latest = profilerSnapshots.at(-1);
  return {
    sampleDurationMs,
    framesAdvanced: end - start,
    fpsFromCounter: +(((end - start) / sampleDurationMs) * 1000).toFixed(1),
    profilerFps: typeof latest?.fps === "number" ? latest.fps : null,
    frameAvgMs: typeof latest?.frame_ms_avg === "number" ? latest.frame_ms_avg : null,
    frameP95Ms: typeof latest?.frame_ms_p95 === "number" ? latest.frame_ms_p95 : null,
  };
}

async function frameCounter(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as unknown as { __bvFrameCounter?: number }).__bvFrameCounter ?? 0,
  );
}

async function collectCanvasEvidence(page: Page): Promise<CanvasSmokeFields> {
  return page.evaluate(async () => {
    const canvas = document.getElementById("brain-canvas") as HTMLCanvasElement | null;
    if (!canvas) {
      return emptyCanvasEvidence("brain-canvas not found");
    }
    try {
      const dataUrl = canvas.toDataURL("image/png");
      const image = new Image();
      image.src = dataUrl;
      await image.decode();
      const sampleCanvas = document.createElement("canvas");
      const width = Math.min(160, Math.max(1, image.naturalWidth));
      const height = Math.min(90, Math.max(1, image.naturalHeight));
      sampleCanvas.width = width;
      sampleCanvas.height = height;
      const ctx = sampleCanvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return emptyCanvasEvidence("2d sample context unavailable", canvas.width, canvas.height);
      ctx.drawImage(image, 0, 0, width, height);
      const pixels = ctx.getImageData(0, 0, width, height).data;
      let count = 0;
      let sum = 0;
      let sumSq = 0;
      let min = Number.POSITIVE_INFINITY;
      let max = Number.NEGATIVE_INFINITY;
      let nonBlack = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        const luma = 0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2];
        count++;
        sum += luma;
        sumSq += luma * luma;
        min = Math.min(min, luma);
        max = Math.max(max, luma);
        if (luma > 3) nonBlack++;
      }
      const mean = sum / count;
      const variance = Math.max(0, sumSq / count - mean * mean);
      return {
        sampled: true,
        width: canvas.width,
        height: canvas.height,
        sampleCount: count,
        meanLuma: +mean.toFixed(3),
        varianceLuma: +variance.toFixed(3),
        minLuma: +min.toFixed(3),
        maxLuma: +max.toFixed(3),
        nonBlackRatio: +(nonBlack / count).toFixed(4),
        error: null,
      };
    } catch (error) {
      return emptyCanvasEvidence(error instanceof Error ? error.message : String(error), canvas.width, canvas.height);
    }

    function emptyCanvasEvidence(
      error: string,
      width = 0,
      height = 0,
    ): CanvasSmokeFields {
      return {
        sampled: false,
        width,
        height,
        sampleCount: 0,
        meanLuma: null,
        varianceLuma: null,
        minLuma: null,
        maxLuma: null,
        nonBlackRatio: null,
        error,
      };
    }
  });
}
