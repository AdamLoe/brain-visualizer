import { expect, test, type Page } from "@playwright/test";
import { inflateSync } from "node:zlib";

const REQUIRE_WEBGPU_VISUAL = process.env.BV_REQUIRE_WEBGPU_VISUAL !== "0";
const WEBGPU_BROWSER_MODE =
  process.env.BV_WEBGPU_BROWSER_MODE ?? (REQUIRE_WEBGPU_VISUAL ? "hardware" : "software");

interface AdapterState {
  gpuPresent: boolean;
  hasAdapter: boolean;
  description: string | null;
}

interface CanvasEvidence {
  width: number;
  height: number;
  sampleX: number;
  sampleY: number;
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

function paeth(a: number, b: number, c: number): number {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  if (pa <= pb && pa <= pc) return a;
  return pb <= pc ? b : c;
}

function collectScreenshotEvidence(png: Buffer): CanvasEvidence {
  const signature = "89504e470d0a1a0a";
  if (png.subarray(0, 8).toString("hex") !== signature) throw new Error("screenshot is not PNG");

  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idat: Buffer[] = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.subarray(offset + 4, offset + 8).toString("ascii");
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
      if (data[12] !== 0) throw new Error("interlaced PNG screenshots are unsupported");
    } else if (type === "IDAT") {
      idat.push(Buffer.from(data));
    } else if (type === "IEND") {
      break;
    }
    offset += 12 + length;
  }
  if (bitDepth !== 8 || (colorType !== 2 && colorType !== 6)) {
    throw new Error(`unsupported PNG format bitDepth=${bitDepth} colorType=${colorType}`);
  }

  const channels = colorType === 6 ? 4 : 3;
  const stride = width * channels;
  const inflated = inflateSync(Buffer.concat(idat));
  const recon = new Uint8Array(stride * height);
  let src = 0;
  for (let y = 0; y < height; y++) {
    const filter = inflated[src++];
    const row = y * stride;
    const prev = row - stride;
    for (let x = 0; x < stride; x++) {
      const raw = inflated[src++];
      const left = x >= channels ? recon[row + x - channels] : 0;
      const up = y > 0 ? recon[prev + x] : 0;
      const upLeft = y > 0 && x >= channels ? recon[prev + x - channels] : 0;
      let value: number;
      if (filter === 0) value = raw;
      else if (filter === 1) value = raw + left;
      else if (filter === 2) value = raw + up;
      else if (filter === 3) value = raw + Math.floor((left + up) / 2);
      else if (filter === 4) value = raw + paeth(left, up, upLeft);
      else throw new Error(`unsupported PNG filter ${filter}`);
      recon[row + x] = value & 0xff;
    }
  }

  const sampleX = Math.floor(width * 0.2);
  const sampleY = Math.floor(height * 0.2);
  const sampleWidth = Math.max(1, Math.floor(width * 0.6));
  const sampleHeight = Math.max(1, Math.floor(height * 0.6));

  let count = 0;
  let nonBlack = 0;
  let sum = 0;
  let sumSq = 0;
  for (let y = sampleY; y < sampleY + sampleHeight; y++) {
    for (let x = sampleX; x < sampleX + sampleWidth; x++) {
      const i = (y * width + x) * channels;
      const luma = 0.2126 * recon[i] + 0.7152 * recon[i + 1] + 0.0722 * recon[i + 2];
      count++;
      sum += luma;
      sumSq += luma * luma;
      if (luma > 3) nonBlack++;
    }
  }
  const mean = sum / count;
  return {
    width,
    height,
    sampleX,
    sampleY,
    sampleWidth,
    sampleHeight,
    nonBlackRatio: +(nonBlack / count).toFixed(4),
    varianceLuma: +Math.max(0, sumSq / count - mean * mean).toFixed(3),
  };
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
  if (REQUIRE_WEBGPU_VISUAL) {
    expect(
      WEBGPU_BROWSER_MODE,
      "Strict visual proof must run with BV_WEBGPU_BROWSER_MODE=hardware; software mode is only for non-strict local checks",
    ).toBe("hardware");
  }
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
    const png = await page.locator("#brain-canvas").screenshot({ path: screenshotPath });
    const evidence = collectScreenshotEvidence(png);
    console.log(
      `[BV-UX-AUDIT-001] ${viewport.name} viewport=${viewport.width}x${viewport.height} ` +
      `sample=center-${evidence.sampleWidth}x${evidence.sampleHeight}@${evidence.sampleX},${evidence.sampleY} ` +
      `screenshot=${screenshotPath} ` +
      `nonBlackRatio=${evidence.nonBlackRatio} varianceLuma=${evidence.varianceLuma}`,
    );

    expect(evidence.width, `${viewport.name} canvas width`).toBeGreaterThan(0);
    expect(evidence.height, `${viewport.name} canvas height`).toBeGreaterThan(0);
    expect(evidence.nonBlackRatio, `${viewport.name} canvas should not be effectively black`).toBeGreaterThan(0.01);
    expect(evidence.varianceLuma, `${viewport.name} canvas should have visible variation`).toBeGreaterThan(0.5);
  }

  expect(consoleErrors, `console/page errors during boot/render:\n${consoleErrors.join("\n")}`).toHaveLength(0);
});

test("BV-UX-AUDIT-002/007 prepared-network failure rolls back controls, storage, and reload state", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 });
  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => typeof (window as unknown as { __bvFailLatestPreparedNetworkForTesting?: unknown }).__bvFailLatestPreparedNetworkForTesting === "function",
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
  const pendingPersisted = await page.evaluate(() => JSON.parse(localStorage.getItem("bv2_settings_v2") ?? "{}"));
  expect(pendingPersisted.dev?.longRangeReachFrac).not.toBe(0.42);

  await page.evaluate(() => {
    (window as unknown as { __bvFailLatestPreparedNetworkForTesting: (message?: string) => void })
      .__bvFailLatestPreparedNetworkForTesting("BV-UX-AUDIT-002 forced prepared-network failure");
  });

  await expect(page.locator('#dev-panel input[aria-label="Long-range fraction"]')).toHaveValue("0.14");
  const persisted = await page.evaluate(() => JSON.parse(localStorage.getItem("bv2_settings_v2") ?? "{}"));
  expect(persisted.dev?.longRangeReachFrac).toBe(0.14);
  const rollback = await page.evaluate(
    () => (window as unknown as { __bvRollbackState?: { reason?: string } }).__bvRollbackState,
  );
  expect(rollback?.reason).toContain("BV-UX-AUDIT-002");

  await page.reload({ waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(
    () => typeof (window as unknown as { __bvFailLatestPreparedNetworkForTesting?: unknown }).__bvFailLatestPreparedNetworkForTesting === "function",
    { timeout: 20_000 },
  );
  await page.evaluate(() => {
    const overlay = document.getElementById("startup-overlay");
    overlay?.classList.add("ready");
    overlay?.classList.remove("failed");
    if (overlay instanceof HTMLElement) overlay.style.pointerEvents = "none";
  });
  await page.locator("#settings-toggle").click();
  await page.locator('#dev-panel .dp-tab[data-tab-id="network"]').click();
  await expect(page.locator('#dev-panel input[aria-label="Long-range fraction"]')).toHaveValue("0.14");
  const reloadedPersisted = await page.evaluate(() => JSON.parse(localStorage.getItem("bv2_settings_v2") ?? "{}"));
  expect(reloadedPersisted.dev?.longRangeReachFrac).toBe(0.14);
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
