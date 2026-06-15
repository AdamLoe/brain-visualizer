/**
 * Brain Visualizer — End-to-end integration tests (Playwright).
 *
 * Tests are written to handle the headless-WebGPU limitation in this WSL2
 * environment gracefully.  WebGPU is present on localhost but requestAdapter()
 * returns null when no real GPU adapter is available.  Tests that require a
 * live WebGPU device are gated with a runtime check and emit a clear skip
 * message rather than failing silently.
 *
 * The critical regression tests (smoke/boot, resize reentrancy) do not require
 * a WebGPU adapter.
 */

import { test, expect, type Page } from "@playwright/test";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Boot the app and wait for the wasm module to load + rAF loop to start.
 * Returns { errors, consoleLogs } collected during navigation.
 */
async function bootApp(page: Page): Promise<{
  errors: string[];
  consoleLogs: string[];
}> {
  const errors: string[] = [];
  const consoleLogs: string[] = [];

  page.on("pageerror", (err) => errors.push(err.message));
  page.on("console", (msg) => {
    consoleLogs.push(`[${msg.type()}] ${msg.text()}`);
    // Escalate console.error to the errors list for easy assertion.
    if (msg.type() === "error") {
      errors.push(`console.error: ${msg.text()}`);
    }
  });

  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });

  // Wait for the wasm init log that fires after manifold generation.
  await page.waitForFunction(
    () =>
      (window as unknown as Record<string, unknown>).__bvFrameCounter !== undefined,
    { timeout: 20_000 },
  );

  return { errors, consoleLogs };
}

/**
 * Returns the current rAF frame counter exposed on window.__bvFrameCounter.
 */
async function getFrameCounter(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as unknown as { __bvFrameCounter?: number }).__bvFrameCounter ?? 0,
  );
}

/**
 * Wait until at least `minFrames` new frames have been rendered.
 */
async function waitForFrames(
  page: Page,
  minFrames: number,
  timeoutMs = 10_000,
): Promise<number> {
  const start = await getFrameCounter(page);
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const current = await getFrameCounter(page);
    if (current - start >= minFrames) return current - start;
    await page.waitForTimeout(100);
  }
  throw new Error(
    `Timed out waiting for ${minFrames} frames (got ${(await getFrameCounter(page)) - start})`,
  );
}

async function setDevPanelSlider(
  page: Page,
  label: string,
  value: string,
): Promise<string> {
  return page.evaluate(
    ({ label, value }) => {
      const rows = Array.from(document.querySelectorAll<HTMLElement>("#dev-panel .dp-ctrl-row"));
      const row = rows.find((candidate) => {
        const text = candidate.querySelector(".dp-ctrl-label")?.textContent?.trim();
        return text === label;
      });
      if (!row) throw new Error(`Dev-panel slider row not found: ${label}`);

      const wrap = row.nextElementSibling as HTMLElement | null;
      const slider = wrap?.querySelector<HTMLInputElement>('input[type="range"]');
      if (!slider) throw new Error(`Dev-panel range input not found: ${label}`);

      slider.value = value;
      slider.dispatchEvent(new Event("input", { bubbles: true }));
      slider.dispatchEvent(new Event("change", { bubbles: true }));
      return slider.value;
    },
    { label, value },
  );
}

/**
 * Check WebGPU adapter availability at runtime.
 * Returns { gpuPresent, hasAdapter, adapterDescription }.
 */
async function checkWebGpu(page: Page) {
  return page.evaluate(async () => {
    const gpuPresent = "gpu" in navigator;
    if (!gpuPresent) {
      return { gpuPresent: false, hasAdapter: false, adapterDescription: null };
    }
    let adapter = null;
    let adapterDescription: string | null = null;
    try {
      adapter = await navigator.gpu.requestAdapter();
      if (adapter) {
        const info = await adapter.requestAdapterInfo();
        adapterDescription = `${info.vendor ?? ""} / ${info.architecture ?? ""}`.trim();
      }
    } catch (_e) {
      // requestAdapter may throw when no backend is available.
    }
    return { gpuPresent, hasAdapter: !!adapter, adapterDescription };
  });
}

// ---------------------------------------------------------------------------
// Test 1 — Smoke / boot (regression test for the reentrancy panic)
// ---------------------------------------------------------------------------

test("smoke: wasm loads and rAF loop starts without uncaught errors", async ({
  page,
}) => {
  const { errors } = await bootApp(page);

  // --- Critical regression: the "recursive use of an object" panic must not appear.
  const reentrancyErrors = errors.filter((e) =>
    e.includes("recursive use of an object"),
  );
  expect(
    reentrancyErrors,
    "wasm-bindgen reentrancy panic fired — the deferred-resize fix may be broken",
  ).toHaveLength(0);

  // The rAF loop must be running.
  const frames1 = await getFrameCounter(page);
  await waitForFrames(page, 5);
  const frames2 = await getFrameCounter(page);
  expect(frames2).toBeGreaterThan(frames1);

  // No uncaught JavaScript exceptions.
  const jsErrors = errors.filter(
    (e) =>
      !e.startsWith("console.error:") ||
      // GPU init failure is expected when no adapter is available; not a bug.
      !(
        e.includes("GPU backend creation failed") ||
        e.includes("No available adapters") ||
        e.includes("webgpu not available") ||
        e.includes("Failed to load resource")
      ),
  );
  // Allow GPU-init failures silently (no adapter in this WSL2 environment).
  const nonGpuErrors = jsErrors.filter(
    (e) =>
      !e.includes("GPU backend creation failed") &&
      !e.includes("No available adapters") &&
      !e.includes("webgpu not available") &&
      !e.includes("Failed to load resource"),
  );
  expect(
    nonGpuErrors,
    `Unexpected page errors:\n${nonGpuErrors.join("\n")}`,
  ).toHaveLength(0);
});

// ---------------------------------------------------------------------------
// Test 2 — WebGPU init (gated on adapter availability)
// ---------------------------------------------------------------------------

test("webgpu: navigator.gpu is present on localhost; adapter gated", async ({
  page,
}) => {
  const { errors: _errors } = await bootApp(page);

  const gpu = await checkWebGpu(page);

  // navigator.gpu MUST be present on localhost (origin check passes).
  expect(
    gpu.gpuPresent,
    "navigator.gpu not found on localhost — Chrome may need WebGPU flags",
  ).toBe(true);

  if (!gpu.hasAdapter) {
    console.log(
      "[SKIP-REASON] WebGPU adapter unavailable in this environment " +
        "(WSL2 without a real GPU — SwiftShader Vulkan present but Dawn " +
        "returns no adapters via requestAdapter()). " +
        "WebGPU-device-dependent assertions are skipped. " +
        "Device-dependent WebGPU assertions are skipped.",
    );
    // Not a test failure — environment limitation.
    return;
  }

  // If an adapter IS available, confirm the WasmGpuBackend initialised.
  console.log(`[OK] WebGPU adapter: ${gpu.adapterDescription}`);
  await waitForFrames(page, 10);

  // No reentrancy panic.
  const gpuErrors = await page.evaluate(() =>
    (window as unknown as { __bvErrors?: string[] }).__bvErrors ?? [],
  );
  const reentrancy = gpuErrors.filter((e: string) =>
    e.includes("recursive use of an object"),
  );
  expect(reentrancy).toHaveLength(0);
});

// ---------------------------------------------------------------------------
// Test 3 — Resize regression (directly reproduces the reported bug)
// ---------------------------------------------------------------------------

test("resize: viewport resize while frames run does NOT trigger borrow panic", async ({
  page,
}) => {
  const errors: string[] = [];
  const reentrancyErrors: string[] = [];

  // Capture reentrancy panics specifically.
  page.on("pageerror", (err) => {
    errors.push(err.message);
    if (err.message.includes("recursive use of an object")) {
      reentrancyErrors.push(err.message);
    }
  });
  page.on("console", (msg) => {
    if (
      msg.type() === "error" &&
      msg.text().includes("recursive use of an object")
    ) {
      reentrancyErrors.push(`console.error: ${msg.text()}`);
    }
  });

  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => (window as unknown as Record<string, unknown>).__bvFrameCounter !== undefined,
    { timeout: 20_000 },
  );

  // Let the rAF loop run for several frames first.
  await waitForFrames(page, 10);

  // Now fire rapid viewport resizes while frames are actively rendering.
  // This is the exact scenario that triggered the "recursive use of an object"
  // panic — a resize event being dispatched while the wasm backend holds a
  // &mut borrow via tick() or render_frame().
  const sizes = [
    { width: 1280, height: 720 },
    { width: 800, height: 600 },
    { width: 1024, height: 768 },
    { width: 640, height: 480 },
    { width: 1920, height: 1080 },
    { width: 1200, height: 900 },
  ];
  for (const size of sizes) {
    await page.setViewportSize(size);
    // Give the rAF loop a chance to process the pending resize.
    await page.waitForTimeout(50);
  }

  // Wait for several more frames to confirm the loop is still alive.
  await waitForFrames(page, 10);

  // THE KEY ASSERTION: the reentrancy panic must not have fired.
  expect(
    reentrancyErrors,
    `wasm-bindgen reentrancy panic detected:\n${reentrancyErrors.join("\n")}\n\n` +
      "The deferred-resize fix ensures resize() is called at the start of the\n" +
      "next rAF turn, not inline from the DOM event handler.",
  ).toHaveLength(0);

  // The rAF loop must still be alive after all the resizes.
  const framesBefore = await getFrameCounter(page);
  await waitForFrames(page, 5);
  const framesAfter = await getFrameCounter(page);
  expect(framesAfter).toBeGreaterThan(framesBefore);
});

// ---------------------------------------------------------------------------
// Test 4 — Controls: public buttons and dev-panel simulation sliders
// ---------------------------------------------------------------------------

test("controls: current public and simulation controls toggle without errors", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (err) => errors.push(err.message));

  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => (window as unknown as Record<string, unknown>).__bvFrameCounter !== undefined,
    { timeout: 20_000 },
  );

  const gpu = await checkWebGpu(page);
  if (!gpu.hasAdapter) {
    console.log(
      "[SKIP-REASON] WebGPU adapter unavailable; controls assertions require " +
        "startup to reach the interactive runtime.",
    );
    return;
  }

  // Wait for the loop to start.
  await waitForFrames(page, 5);

  // UX overhaul: the old top-level brain-state/speed groups are intentionally
  // removed from the public page. Their live controls now live in the hidden
  // dev panel as Excitability and Speed sliders.
  await expect(page.locator("#brain-state-group")).toHaveCount(0);
  await expect(page.locator("#speed-group")).toHaveCount(0);

  const pauseBtn = page.locator("#pause-toggle");
  await expect(pauseBtn).toBeVisible();
  await pauseBtn.click();
  await expect(pauseBtn).toHaveAttribute("aria-pressed", "true");
  await pauseBtn.click();
  await expect(pauseBtn).toHaveAttribute("aria-pressed", "false");

  await page.locator("#settings-toggle").click();
  await expect(page.locator("#dev-panel")).toHaveClass(/dp--open/);
  await page.locator('#dev-panel .dp-tab[data-tab-id="network"]').click();

  for (const excitability of ["0.10", "0.30", "0.63", "0.71", "1.00"]) {
    expect(Number(await setDevPanelSlider(page, "Excitability", excitability))).toBeCloseTo(
      Number(excitability),
      2,
    );
    await page.waitForTimeout(50);
  }

  for (const speed of ["1", "15", "30", "60"]) {
    expect(Number(await setDevPanelSlider(page, "Speed (ticks/sec)", speed))).toBe(
      Number(speed),
    );
    await page.waitForTimeout(50);
  }

  // Confirm no crashes from button interactions.
  const criticalErrors = errors.filter(
    (e) => !e.includes("GPU backend creation failed"),
  );
  expect(
    criticalErrors,
    `Unexpected errors after button interactions:\n${criticalErrors.join("\n")}`,
  ).toHaveLength(0);

  // rAF loop must still be running.
  await waitForFrames(page, 3);
});
