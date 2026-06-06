import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright E2E test configuration.
 *
 * Tests cover:
 * 1. Smoke/boot — wasm loads, no "recursive use of an object" panic.
 * 2. WebGPU init — adapter availability (gated: skipped when no adapter).
 * 3. Resize regression — the exact reentrancy bug the user hit.
 * 4. Controls — brain-state + speed buttons work without error.
 * 5. CPU backend toggle (gated: requires WebGL2 + worker).
 *
 * The dev server is assumed to be running at http://localhost:5173.
 * Set USE_WEBSERVER=1 to have Playwright start it automatically instead.
 */

const USE_WEBSERVER = process.env.USE_WEBSERVER === "1";

export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  retries: 0,
  workers: 1, // serial — one browser at a time

  reporter: [["list"], ["json", { outputFile: "./e2e/results.json" }]],

  use: {
    baseURL: "http://localhost:5173",
    // WebGPU needs SwiftShader in headless mode (no real GPU in WSL2).
    // With DISPLAY=:0 (WSL2 X server), non-headless mode exposes navigator.gpu
    // on localhost even without a real GPU adapter.
    headless: false,
    launchOptions: {
      executablePath:
        process.env.CHROMIUM_PATH ||
        "/home/adamg/.cache/ms-playwright/chromium-1223/chrome-linux64/chrome",
      args: [
        "--no-sandbox",
        "--disable-gpu-sandbox",
        "--enable-unsafe-swiftshader",
        "--use-angle=swiftshader",
        "--enable-features=Vulkan,WebGPU",
        "--use-vulkan=swiftshader",
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
