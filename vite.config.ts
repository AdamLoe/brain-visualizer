import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// COOP/COEP headers enable SharedArrayBuffer / WASM threads in the dev server
// and preview (the coi-serviceworker shim covers static hosts like GitHub
// Pages that can't set headers — see public/coi-serviceworker.js).
const crossOriginIsolation = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
  server: { headers: crossOriginIsolation },
  preview: { headers: crossOriginIsolation },
  // The CPU coordinator worker (web/cpu-worker.ts) dynamically imports the wasm
  // pkg, which forces code-splitting; ES module workers are required for that
  // (the default IIFE worker format cannot code-split). Module workers also
  // carry crossOriginIsolated into the worker for SharedArrayBuffer (BV24).
  worker: { format: "es", plugins: () => [wasm(), topLevelAwait()] },
  // The wasm-pack `pkg/` output is referenced directly by web/main.ts.
  optimizeDeps: { exclude: ["brain_visualizer"] },
});
