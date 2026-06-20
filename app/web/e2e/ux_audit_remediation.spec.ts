import { expect, test, type Page } from "@playwright/test";

const REQUIRE_WEBGPU_VISUAL = process.env.BV_REQUIRE_WEBGPU_VISUAL === "1";

interface AdapterState {
  gpuPresent: boolean;
  hasAdapter: boolean;
  description: string | null;
}

interface CanvasEvidence {
  width: number;
  height: number;
  sampleWidth: number;
  sampleHeight: number;
  nonBlackRatio: number;
  varianceLuma: number;
}

async function collectAdapter(page: Page): Promise<AdapterState> {
  return page.evaluate(async () => {
    if (!("gpu" in navigator)) {
      return { gpuPresent: false, hasAdapter: false, description: null };
    }
    try {
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) return { gpuPresent: true, hasAdapter: false, description: null };
      const adapterWithInfo = adapter as GPUAdapter & {
        requestAdapterInfo?: () => Promise<{ vendor?: string; architecture?: string; description?: string }>;
        info?: { vendor?: string; architecture?: string; description?: string };
      };
      const info = typeof adapterWithInfo.requestAdapterInfo === "function"
        ? await adapterWithInfo.requestAdapterInfo()
        : adapterWithInfo.info;
      return {
        gpuPresent: true,
        hasAdapter: true,
        description: (info?.description ?? [info?.vendor, info?.architecture].filter(Boolean).join(" / ")) || null,
      };
    } catch {
      return { gpuPresent: true, hasAdapter: false, description: null };
    }
  });
}

async function waitForReady(page: Page): Promise<void> {
  await page.waitForFunction(
    () => (window as unknown as { __bvStartup?: { status?: string } }).__bvStartup?.status === "ready",
    { timeout: 45_000 },
  );
  await page.waitForFunction(
    () => {
      const overlay = document.getElementById("startup-overlay");
      return overlay?.classList.contains("ready") === true;
    },
    { timeout: 5_000 },
  );
}

async function collectCanvasEvidence(page: Page): Promise<CanvasEvidence> {
  return page.evaluate(async () => {
    const canvas = document.getElementById("brain-canvas") as HTMLCanvasElement | null;
    if (!canvas) throw new Error("brain-canvas not found");
    const image = new Image();
    image.src = canvas.toDataURL("image/png");
    await image.decode();

    const sampleCanvas = document.createElement("canvas");
    const sampleWidth = Math.min(180, Math.max(1, image.naturalWidth));
    const sampleHeight = Math.min(120, Math.max(1, image.naturalHeight));
    sampleCanvas.width = sampleWidth;
    sampleCanvas.height = sampleHeight;
    const ctx = sampleCanvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) throw new Error("2d sample context unavailable");
    ctx.drawImage(image, 0, 0, sampleWidth, sampleHeight);
    const pixels = ctx.getImageData(0, 0, sampleWidth, sampleHeight).data;

    let count = 0;
    let nonBlack = 0;
    let sum = 0;
    let sumSq = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      const luma = 0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2];
      count++;
      sum += luma;
      sumSq += luma * luma;
      if (luma > 3) nonBlack++;
    }
    const mean = sum / count;
    return {
      width: canvas.width,
      height: canvas.height,
      sampleWidth,
      sampleHeight,
      nonBlackRatio: +(nonBlack / count).toFixed(4),
      varianceLuma: +Math.max(0, sumSq / count - mean * mean).toFixed(3),
    };
  });
}

async function setDevPanelSlider(page: Page, label: string, value: string): Promise<void> {
  await page.evaluate(
    ({ label, value }) => {
      const rows = Array.from(document.querySelectorAll<HTMLElement>("#dev-panel .dp-ctrl-row"));
      const row = rows.find((candidate) =>
        candidate.querySelector(".dp-ctrl-label")?.textContent?.trim() === label
      );
      if (!row) throw new Error(`row not found: ${label}`);
      const wrap = row.nextElementSibling as HTMLElement | null;
      const slider = wrap?.querySelector<HTMLInputElement>('input[type="range"]');
      if (!slider) throw new Error(`range not found: ${label}`);
      slider.value = value;
      slider.dispatchEvent(new Event("input", { bubbles: true }));
      slider.dispatchEvent(new Event("change", { bubbles: true }));
    },
    { label, value },
  );
}

test("BV-UX-AUDIT-001/006 real WebGPU boot screenshots are visibly nonblank on desktop and mobile-ish viewports", async ({
  page,
}, testInfo) => {
  test.setTimeout(120_000);
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

  for (const viewport of [
    { name: "desktop", width: 1280, height: 720 },
    { name: "mobile-ish", width: 390, height: 844 },
  ]) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
    const adapter = await collectAdapter(page);
    if (!adapter.hasAdapter) {
      const message = `Real WebGPU adapter unavailable for ${viewport.name} ${viewport.width}x${viewport.height}; adapter=${JSON.stringify(adapter)}`;
      console.log(`[BV-UX-AUDIT-001] ${message}`);
      if (REQUIRE_WEBGPU_VISUAL) expect(adapter.hasAdapter, message).toBe(true);
      return;
    }

    await waitForReady(page);
    await page.waitForTimeout(500);
    const screenshotPath = testInfo.outputPath(`bv-ux-audit-001-${viewport.name}.png`);
    await page.screenshot({ path: screenshotPath, fullPage: true });
    const evidence = await collectCanvasEvidence(page);
    console.log(
      `[BV-UX-AUDIT-001] ${viewport.name} viewport=${viewport.width}x${viewport.height} ` +
      `sample=${evidence.sampleWidth}x${evidence.sampleHeight} screenshot=${screenshotPath} ` +
      `nonBlackRatio=${evidence.nonBlackRatio} varianceLuma=${evidence.varianceLuma}`,
    );

    expect(evidence.width, `${viewport.name} canvas width`).toBeGreaterThan(0);
    expect(evidence.height, `${viewport.name} canvas height`).toBeGreaterThan(0);
    expect(evidence.nonBlackRatio, `${viewport.name} canvas should not be effectively black`).toBeGreaterThan(0.01);
    expect(evidence.varianceLuma, `${viewport.name} canvas should have visible variation`).toBeGreaterThan(0.5);
  }

  expect(consoleErrors, `console/page errors during boot/render:\n${consoleErrors.join("\n")}`).toHaveLength(0);
});

test("BV-UX-AUDIT-002/007 forced structural rollback restores controls and app-owned storage", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 });
  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => typeof (window as unknown as { __bvForceStructuralRollback?: unknown }).__bvForceStructuralRollback === "function",
    { timeout: 20_000 },
  );
  await page.waitForFunction(
    () => {
      const startup = (window as unknown as { __bvStartup?: { status?: string } }).__bvStartup;
      return startup?.status === "ready" || startup?.status === "failed";
    },
    { timeout: 30_000 },
  ).catch(() => undefined);
  await page.evaluate(() => {
    const overlay = document.getElementById("startup-overlay");
    overlay?.classList.add("ready");
    overlay?.classList.remove("failed");
    if (overlay instanceof HTMLElement) overlay.style.pointerEvents = "none";
  });

  await page.locator("#settings-toggle").click();
  await expect(page.locator("#dev-panel")).toHaveClass(/dp--open/);
  await page.locator('#dev-panel .dp-tab[data-tab-id="network"]').click();

  await setDevPanelSlider(page, "Long-range fraction", "0.42");
  await expect(page.locator('#dev-panel input[aria-label="Long-range fraction"]')).toHaveValue("0.42");

  await page.evaluate(() => {
    (window as unknown as { __bvForceStructuralRollback: (reason?: string) => void })
      .__bvForceStructuralRollback("BV-UX-AUDIT-002 forced failure");
  });

  await expect(page.locator('#dev-panel input[aria-label="Long-range fraction"]')).toHaveValue("0.14");
  const persisted = await page.evaluate(() => JSON.parse(localStorage.getItem("bv2_settings_v2") ?? "{}"));
  expect(persisted.dev?.longRangeReachFrac).toBe(0.14);
  const rollback = await page.evaluate(
    () => (window as unknown as { __bvRollbackState?: { reason?: string } }).__bvRollbackState,
  );
  expect(rollback?.reason).toContain("BV-UX-AUDIT-002");

  await page.locator('#dev-panel .dp-tab[data-tab-id="morphology"]').click();
  await setDevPanelSlider(page, "Dendrite twigs", "2");
  await page.locator("#dev-panel .dp-action-btn", { hasText: "Rebuild Morphology" }).click();
  await expect(page.locator('#dev-panel input[aria-label="Dendrite twigs"]')).toHaveValue("2");

  await page.evaluate(() => {
    (window as unknown as { __bvForceStructuralRollback: (reason?: string) => void })
      .__bvForceStructuralRollback("BV-UX-AUDIT-007 forced morphology failure");
  });

  await expect(page.locator('#dev-panel input[aria-label="Dendrite twigs"]')).toHaveValue("1");
  const morphPersisted = await page.evaluate(() => JSON.parse(localStorage.getItem("bv2_morph_v2") ?? "{}"));
  expect(morphPersisted.config?.generator?.dendriteTwigCount).toBe(1);
});

test("BV-UX-AUDIT-003 startup failure actions are readable at narrow width and reset storage", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 700 });
  await page.addInitScript(() => {
    localStorage.setItem("bv2_config_v2", "{\"bad\":true}");
    localStorage.setItem("bv2_settings_v2", "{\"bad\":true}");
    localStorage.setItem("bv2_morph_v2", "{\"bad\":true}");
  });
  await page.goto("/?bv_force_startup_failure=1", { waitUntil: "networkidle", timeout: 30_000 });
  await expect(page.locator("#startup-overlay")).toHaveClass(/failed/);
  await expect(page.locator("#startup-reset-storage")).toBeVisible();
  await expect(page.locator("#startup-load-defaults")).toBeVisible();
  await expect(page.locator("#startup-retry")).toBeVisible();

  const stageFits = await page.locator("#startup-stage").evaluate((el) => {
    const rect = el.getBoundingClientRect();
    return rect.width <= window.innerWidth && el.scrollWidth <= el.clientWidth + 1;
  });
  expect(stageFits).toBe(true);

  await page.locator("#startup-reset-storage").click();
  const remaining = await page.evaluate(() => [
    localStorage.getItem("bv2_config_v2"),
    localStorage.getItem("bv2_settings_v2"),
    localStorage.getItem("bv2_morph_v2"),
  ]);
  expect(remaining).toEqual([null, null, null]);
});

test("BV-UX-AUDIT-004/005/006 keyboard access, tab semantics, focus help, narrow clamp, and mobile diagnostics policy", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 });
  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => (window as unknown as Record<string, unknown>).__bvFrameCounter !== undefined,
    { timeout: 20_000 },
  );

  await expect(page.locator("#settings-toggle")).toHaveAttribute("aria-label", /settings/i);
  await expect(page.locator("#pause-toggle")).toHaveAttribute("aria-label", /pause/i);

  await page.locator("#settings-toggle").focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("#dev-panel")).toHaveClass(/dp--open/);
  await expect(page.locator(".dp-close")).toBeFocused();

  const selectedTab = page.locator('#dev-panel .dp-tab[aria-selected="true"]');
  await expect(selectedTab).toHaveAttribute("role", "tab");
  await expect(page.locator("#dev-panel .dp-tabbar")).toHaveAttribute("role", "tablist");

  await page.locator('#dev-panel .dp-tab[data-tab-id="monitor"]').focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.locator('#dev-panel .dp-tab[data-tab-id="dynamics"]')).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("End");
  await expect(page.locator('#dev-panel .dp-tab[data-tab-id="storage"]')).toHaveAttribute("aria-selected", "true");

  await page.locator('#dev-panel .dp-tab[data-tab-id="network"]').click();
  await page.locator('#dev-panel input[aria-label="Excitability"]').focus();
  await expect(page.locator(".dp-tooltip")).toBeVisible();

  await page.setViewportSize({ width: 500, height: 700 });
  const canvasBox = await page.locator("#brain-canvas").boundingBox();
  expect(canvasBox?.width ?? 0).toBeGreaterThan(0);

  await page.locator(".dp-close").click();
  await expect(page.locator("#settings-toggle")).toBeFocused();

  await page.setViewportSize({ width: 390, height: 700 });
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForFunction(
    () => (window as unknown as { __bvDiagnosticsPolicy?: string }).__bvDiagnosticsPolicy === "unsupported-mobile",
    { timeout: 20_000 },
  );
  await expect(page.locator("#settings-toggle")).toBeHidden();
  await expect(page.locator("#dev-panel")).toHaveCount(0);
});
