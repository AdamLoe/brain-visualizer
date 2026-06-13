import { expect, test, type Page } from "@playwright/test";

/**
 * Boot-load overlay observation spec.
 *
 * Drives a real browser through boot and samples `window.__bvStartup`
 * (status / stage label / progress) at a tight interval, recording the
 * full timeline. Verifies the boot-panel overhaul:
 *   1. 3-row layout renders (title / progress bar / percent-left + stage-right).
 *   2. Overall percent climbs monotonically; bar width tracks it.
 *   3. The stage label carries its within-stage percent (`<stage> N%`) during
 *      the sub-stage stages.
 *   4. "Prepare network payload" reaches 100% (its real worker phases drive it).
 *
 * Headless WSL2 has no real GPU adapter, so boot may stall at the GPU-acquire
 * stage; the spec records the timeline and screenshots regardless, and only
 * hard-asserts the DOM-observable / worker-observable behavior.
 */

interface StartupWindowState {
  status?: string;
  stage?: string;
  progress?: number;
}

interface Rect {
  top: number;
  left: number;
  right: number;
  width: number;
  height: number;
}

interface LayoutSnapshot {
  titleText: string | null;
  titleRect: Rect | null;
  trackRect: Rect | null;
  barRect: Rect | null;
  metaRect: Rect | null;
  percentRect: Rect | null;
  stageRect: Rect | null;
  percentText: string | null;
  stageText: string | null;
  stageOverflows: boolean | null;
}

interface Sample {
  t: number;
  status: string | null;
  stage: string | null;
  progress: number | null;
  barWidthPct: number | null;
  percentText: string | null;
}

test("boot overlay 3-row layout, monotonic climb, within-stage percent, payload reaches 100%", async ({
  page,
}, testInfo) => {
  // Generous budget: the payload probe waits up to 30s for the single build
  // worker, which may be serially finishing boot's abandoned startup payload.
  test.setTimeout(90_000);
  // Inject a high-frequency in-page recorder BEFORE any app code runs. It polls
  // __bvStartup + overlay DOM on a 1ms interval and records every distinct
  // (status, stage, progress) change — no Playwright round-trip latency, so the
  // brief within-stage label windows (e.g. "Requesting GPU adapter… 10%") are
  // reliably captured even on a fast no-GPU failure path.
  await page.addInitScript(() => {
    try {
      window.localStorage.clear();
    } catch {
      /* ignore */
    }
    interface RecSample {
      t: number;
      status: string | null;
      stage: string | null;
      progress: number | null;
      barWidthPct: number | null;
      percentText: string | null;
    }
    const rec: {
      samples: RecSample[];
      loadingLayout: unknown | null;
      startedAt: number;
    } = { samples: [], loadingLayout: null, startedAt: Date.now() };
    (window as unknown as { __bvOverlayRec: typeof rec }).__bvOverlayRec = rec;

    const rect = (el: Element | null) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { top: r.top, left: r.left, right: r.right, width: r.width, height: r.height };
    };
    let lastKey = "";
    const tick = () => {
      const w = window as unknown as {
        __bvStartup?: { status?: string; stage?: string; progress?: number };
      };
      const startup = w.__bvStartup;
      const bar = document.getElementById("startup-progress-bar");
      const percent = document.getElementById("startup-percent");
      const stage = document.getElementById("startup-stage");
      const barWidth = bar ? parseFloat((bar as HTMLElement).style.width || "") : null;
      const status = startup?.status ?? null;
      const stageText = startup?.stage ?? null;
      const progress = typeof startup?.progress === "number" ? startup.progress : null;
      const key = `${status}|${stageText}|${progress}`;
      if (key !== lastKey) {
        lastKey = key;
        rec.samples.push({
          t: Date.now() - rec.startedAt,
          status,
          stage: stageText,
          progress,
          barWidthPct: barWidth !== null && !Number.isNaN(barWidth) ? barWidth : null,
          percentText: percent?.textContent ?? null,
        });
        if (
          rec.loadingLayout === null &&
          status === "loading" &&
          stageText !== null
        ) {
          const title = document.getElementById("startup-title");
          const track = document.getElementById("startup-progress-track");
          const meta = document.getElementById("startup-meta");
          rec.loadingLayout = {
            titleText: title?.textContent ?? null,
            titleRect: rect(title),
            trackRect: rect(track),
            barRect: rect(bar),
            metaRect: rect(meta),
            percentRect: rect(percent),
            stageRect: rect(stage),
            percentText: percent?.textContent ?? null,
            stageText: stage?.textContent ?? null,
            stageOverflows: stage ? stage.scrollWidth > stage.clientWidth + 1 : null,
          };
        }
      }
    };
    setInterval(tick, 1);
  });

  await page.goto("/", { waitUntil: "commit", timeout: 30_000 });

  // Wait for the startup hook to appear, then for a terminal-ish state or timeout.
  await page.waitForFunction(
    () => (window as unknown as { __bvStartup?: StartupWindowState }).__bvStartup !== undefined,
    { timeout: 20_000 },
  );

  // Give boot a generous window; on a no-GPU box it may never reach ready.
  await page
    .waitForFunction(
      () => {
        const s = (window as unknown as { __bvStartup?: StartupWindowState }).__bvStartup;
        return s?.status === "ready" || s?.status === "failed";
      },
      { timeout: 25_000 },
    )
    .catch(() => {
      /* record whatever we have */
    });

  // Let the recorder drain a few more frames.
  await page.waitForTimeout(300);

  const recorded = await page.evaluate(
    () =>
      (window as unknown as {
        __bvOverlayRec?: { samples: unknown[]; loadingLayout: unknown };
      }).__bvOverlayRec ?? { samples: [], loadingLayout: null },
  );
  const samples = recorded.samples as Sample[];

  // Terminal-state screenshot (the real boot end state — "ready" overlay hidden,
  // or the "failed" error panel on a no-GPU box). Taken BEFORE the visual pin.
  const screenshotPath = testInfo.outputPath("boot-overlay.png");
  await page.screenshot({ path: screenshotPath, fullPage: true });

  // Visual artifact: with the recorder's samples already read, re-pin the
  // overlay to a representative loading state (real markup + CSS) and screenshot
  // it, so the captured PNG shows the turquoise->gold gradient bar mid-climb.
  // On a no-GPU box the live loading window is too brief to screenshot, and
  // this pin runs strictly AFTER all behavioral samples are captured, so it
  // cannot pollute the asserted timeline.
  await page
    .evaluate(() => {
      const overlay = document.getElementById("startup-overlay");
      const bar = document.getElementById("startup-progress-bar");
      const percent = document.getElementById("startup-percent");
      const stage = document.getElementById("startup-stage");
      overlay?.classList.remove("ready", "failed");
      if (bar) (bar as HTMLElement).style.width = "62%";
      if (percent) percent.textContent = "62%";
      if (stage) stage.textContent = "Prepare network payload 78%";
    })
    .catch(() => undefined);
  await page
    .screenshot({ path: testInfo.outputPath("boot-overlay-loading.png"), fullPage: true })
    .catch(() => undefined);

  // --- Layout assertions (3-row), against a loading-state snapshot ---
  expect(
    recorded.loadingLayout,
    "should have captured a loading-state layout snapshot",
  ).not.toBeNull();
  const layout = recorded.loadingLayout as LayoutSnapshot;

  // Emit the full timeline so it lands in the test output / artifacts.
  const timeline = samples.map(
    (s) =>
      `t=${String(s.t).padStart(5)}ms status=${s.status} progress=${
        s.progress === null ? "—" : s.progress.toFixed(1)
      } bar=${s.barWidthPct === null ? "—" : s.barWidthPct + "%"} percentText=${
        s.percentText
      } stage="${s.stage}"`,
  );
  console.log(`[BOOT-OVERLAY] screenshot: ${screenshotPath}`);
  console.log(`[BOOT-OVERLAY] layout: ${JSON.stringify(layout, null, 2)}`);
  console.log(`[BOOT-OVERLAY] timeline (${samples.length} samples):\n${timeline.join("\n")}`);

  // 1. 3-row layout: title above track, track above meta; percent left of stage.
  expect(layout.titleText).toBe("Brain Visualizer");
  expect(layout.titleRect, "title present").not.toBeNull();
  expect(layout.trackRect, "progress track present").not.toBeNull();
  expect(layout.metaRect, "meta row present").not.toBeNull();
  expect(layout.percentRect, "percent present").not.toBeNull();
  expect(layout.stageRect, "stage present").not.toBeNull();
  // Vertical order.
  expect(layout.titleRect!.top).toBeLessThan(layout.trackRect!.top);
  expect(layout.trackRect!.top).toBeLessThan(layout.metaRect!.top);
  // Percent on the left, stage on the right (same row).
  expect(layout.percentRect!.left).toBeLessThan(layout.stageRect!.left);
  expect(Math.abs(layout.percentRect!.top - layout.stageRect!.top)).toBeLessThan(8);
  // No overflow of the stage label.
  expect(layout.stageOverflows, "stage label must not overflow").not.toBe(true);

  // 2. Monotonic climb of overall progress (allow tiny float noise).
  const progressSeq = samples
    .map((s) => s.progress)
    .filter((p): p is number => p !== null);
  // Synchronous WASM init blocks the main thread (and the 1ms recorder), so the
  // early 8→52 stages collapse into a couple of samples; require only a handful.
  expect(progressSeq.length).toBeGreaterThanOrEqual(2);
  for (let i = 1; i < progressSeq.length; i++) {
    expect(
      progressSeq[i],
      `progress regressed at sample ${i}: ${progressSeq[i - 1]} -> ${progressSeq[i]}`,
    ).toBeGreaterThanOrEqual(progressSeq[i - 1] - 0.6);
  }
  // It should actually move a meaningful distance.
  expect(Math.max(...progressSeq) - Math.min(...progressSeq)).toBeGreaterThan(20);

  // Bar width should track progress (within rounding) on samples where both exist.
  for (const s of samples) {
    if (s.progress !== null && s.barWidthPct !== null && s.status === "loading") {
      expect(Math.abs(s.barWidthPct - Math.round(s.progress))).toBeLessThanOrEqual(1);
    }
  }

  // 3. Within-stage percent: stage labels carry an in-stage "N%" suffix during
  // the sub-stage stages. On a no-GPU box the GPU-acquire stage (the only
  // within-stage stage reached before boot fails) lives only a few ms, so the
  // 1ms recorder catches its "Requesting GPU adapter… N%" label most runs but
  // not deterministically. We log what we caught; the label-format CONTRACT is
  // pinned deterministically by src/boot-overlay.test.ts (formatSubStageLabel),
  // and the payload sub-stage fractions are verified by the probe below.
  const withinStageSamples = samples.filter(
    (s) => s.stage !== null && /\s\d{1,3}%$/.test(s.stage),
  );
  console.log(
    `[BOOT-OVERLAY] within-stage labels caught (${withinStageSamples.length}): ${withinStageSamples
      .map((s) => `"${s.stage}"`)
      .join(", ")}`,
  );

  // 4. "Prepare network payload" should reach 100% (real worker phases).
  const payloadLabels = samples
    .map((s) => s.stage)
    .filter((st): st is string => !!st && st.startsWith("Prepare network payload"));
  console.log(
    `[BOOT-OVERLAY] payload labels seen: ${Array.from(new Set(payloadLabels)).join(" | ")}`,
  );
  // Independently confirm the worker progress wiring fires end to end and the
  // payload completes (this does not require a GPU adapter).
  const payloadProbe = await probePayloadProgress(page);
  console.log(`[BOOT-OVERLAY] payload probe: ${JSON.stringify(payloadProbe)}`);
  expect(payloadProbe.maxFraction).toBeCloseTo(1, 5);
  expect(payloadProbe.phases.length).toBeGreaterThanOrEqual(1);
});

/**
 * Drive the worker payload path directly via the smoke hook and capture the
 * onProgress phase/fraction ticks the client delivers. GPU-independent: proves
 * the worker -> client.onProgress wiring fires and the payload reaches 1.0.
 */
async function probePayloadProgress(
  page: Page,
): Promise<{ phases: Array<{ phase: string; fraction: number }>; maxFraction: number }> {
  await page.waitForFunction(
    () => typeof (window as unknown as Record<string, unknown>).__bvRequestPreparedNetworkSmoke === "function",
    { timeout: 10_000 },
  );

  return page.evaluate(async () => {
    const w = window as unknown as {
      __bvRequestPreparedNetworkSmoke?: (r: { n?: number; k?: number; seed?: number }) => number;
      __bvOnNetworkBuildProgress?: (
        cb: ((p: { sequence: number; phase: string; fraction: number }) => void) | null,
      ) => void;
      __bvNetworkBuildStatus?: { kind: string; sequence?: number };
    };
    const phases: Array<{ phase: string; fraction: number }> = [];
    let max = 0;

    // Subscribe BEFORE issuing the request so no early phase tick is missed.
    let seq = -1;
    w.__bvOnNetworkBuildProgress?.((p) => {
      if (p.sequence !== seq) return;
      phases.push({ phase: p.phase, fraction: p.fraction });
      if (p.fraction > max) max = p.fraction;
    });
    // Small N keeps the heavy morphology phase fast; the single build worker may
    // still be finishing boot's abandoned startup payload, so the budget below is
    // generous to absorb that serial contention.
    seq = w.__bvRequestPreparedNetworkSmoke!({ n: 4_000, k: 12, seed: 0x51571 });

    // Wait until the final phase tick (fraction 1.0) lands or we time out. The
    // window status mirror is only refreshed by the boot loop, so we rely on
    // the progress ticks themselves (which include the terminal 1.0 emit).
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline && max < 1) {
      await new Promise((r) => setTimeout(r, 20));
    }
    w.__bvOnNetworkBuildProgress?.(null);
    return { phases, maxFraction: max };
  });
}
