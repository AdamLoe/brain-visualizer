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
  // The wasm-pack `pkg/` output is referenced directly by web/main.ts.
  optimizeDeps: { exclude: ["brain_visualizer"] },
});
