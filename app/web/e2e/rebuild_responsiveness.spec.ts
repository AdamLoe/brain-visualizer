import { expect, test, type Page } from "@playwright/test";

interface PreparedBuildStatus {
  kind: "idle" | "preparing" | "ready" | "failed";
  sequence?: number;
  message?: string;
}

declare global {
  interface Window {
    __bvFrameCounter?: number;
    __bvNetworkBuildStatus?: PreparedBuildStatus;
    __bvRequestPreparedNetworkSmoke?: (request: {
      n?: number;
      k?: number;
      seed?: number;
    }) => number;
  }
}

test("frame counter advances while high-N worker prepare is in flight", async ({ page }) => {
  await page.goto("/", { waitUntil: "networkidle", timeout: 30_000 });
  await page.waitForFunction(() => window.__bvFrameCounter !== undefined, { timeout: 20_000 });
  await page.waitForFunction(
    () => typeof window.__bvRequestPreparedNetworkSmoke === "function",
    { timeout: 20_000 },
  );

  const sequence = await page.evaluate(() => {
    if (!window.__bvRequestPreparedNetworkSmoke) {
      throw new Error("prepared-network smoke hook unavailable");
    }
    return window.__bvRequestPreparedNetworkSmoke({
      n: 20_000,
      k: 16,
      seed: 0xdecafbad,
    });
  });

  await page.waitForFunction(
    (seq) => window.__bvNetworkBuildStatus?.kind === "preparing" &&
      window.__bvNetworkBuildStatus.sequence === seq,
    sequence,
    { timeout: 5_000 },
  );

  const startFrame = await frameCounter(page);
  await page.waitForFunction(
    (start) => (window.__bvFrameCounter ?? 0) - start >= 5,
    startFrame,
    { timeout: 5_000 },
  );
  const advanced = (await frameCounter(page)) - startFrame;
  const statusDuringSample = await page.evaluate(() => window.__bvNetworkBuildStatus ?? null);

  expect(advanced).toBeGreaterThanOrEqual(5);
  expect(statusDuringSample?.kind).toBe("preparing");
  expect(statusDuringSample?.sequence).toBe(sequence);
});

async function frameCounter(page: Page): Promise<number> {
  return page.evaluate(() => window.__bvFrameCounter ?? 0);
}
