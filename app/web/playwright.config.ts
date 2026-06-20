import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright E2E test configuration.
 *
 * Tests cover:
 * 1. Smoke/boot — wasm loads, no "recursive use of an object" panic.
 * 2. WebGPU init — adapter availability (gated: skipped when no adapter).
 * 3. Resize regression — the exact reentrancy bug the user hit.
 * 4. Controls — public buttons and dev-panel simulation sliders work without error.
 *
 * The dev server is assumed to be running at http://localhost:5173.
 * Set USE_WEBSERVER=1 to have Playwright start it automatically instead.
 */

const USE_WEBSERVER = process.env.USE_WEBSERVER === "1";
const REQUIRE_WEBGPU_VISUAL = process.env.BV_REQUIRE_WEBGPU_VISUAL !== "0";
const WEBGPU_BROWSER_MODE =
  process.env.BV_WEBGPU_BROWSER_MODE ?? (REQUIRE_WEBGPU_VISUAL ? "hardware" : "software");
const HEADLESS = process.env.BV_PLAYWRIGHT_HEADLESS === "1";

if (WEBGPU_BROWSER_MODE !== "hardware" && WEBGPU_BROWSER_MODE !== "software") {
  throw new Error(`BV_WEBGPU_BROWSER_MODE must be "hardware" or "software", got ${WEBGPU_BROWSER_MODE}`);
}

const webGpuArgs =
  WEBGPU_BROWSER_MODE === "hardware"
    ? [
        "--enable-unsafe-webgpu",
        "--ignore-gpu-blocklist",
        "--use-angle=vulkan",
        "--enable-features=Vulkan,WebGPU",
      ]
    : [
        "--enable-unsafe-swiftshader",
        "--use-angle=swiftshader",
        "--enable-features=Vulkan,WebGPU",
        "--use-vulkan=swiftshader",
      ];

export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  retries: 0,
  workers: 1, // serial — one browser at a time

  reporter: [["list"], ["json", { outputFile: "./e2e/results.json" }]],

  use: {
    baseURL: "http://localhost:5173",
    // Strict UX visual proof defaults to hardware mode. Non-strict local e2e
    // can set BV_REQUIRE_WEBGPU_VISUAL=0 to use the software flags below.
    headless: HEADLESS,
    launchOptions: {
      executablePath:
        process.env.CHROMIUM_PATH ||
        "/home/adamg/.cache/ms-playwright/chromium-1223/chrome-linux64/chrome",
      args: [
        "--no-sandbox",
        "--disable-gpu-sandbox",
        ...webGpuArgs,
      ],
    },
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  // Start the Vite dev server automatically if USE_WEBSERVER=1.
  webServer: USE_WEBSERVER
    ? {
        command: "npm run dev",
        url: "http://localhost:5173",
        reuseExistingServer: true,
        timeout: 120_000,
      }
    : undefined,
});
