import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Run only the pure-logic tests that don't require a browser.
    // Test files live alongside the source they test (*.test.ts here in src/web).
    include: ["**/*.test.ts"],
    environment: "node",
    // Disable browser APIs we don't use in the pure tests;
    // controls.ts accesses document/window only in the DOM-touching paths
    // (setBrainState, showToast, etc.) which are NOT called from tests.
    globals: false,
  },
});
