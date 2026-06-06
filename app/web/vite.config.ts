import { defineConfig, type Plugin } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { spawn } from "node:child_process";
import { resolve } from "node:path";

// Rebuild the Rust crate to wasm whenever a .rs source or Cargo.toml changes,
// then trigger a full browser reload. This keeps the wasm pkg/ output in sync
// during `npm run dev` without a separate cargo-watch process. Rebuilds are
// debounced and serialized so a burst of saves collapses into one build.
// This config lives at app/web/; the Rust crate is a sibling at app/crates/.
const crateDir = resolve(__dirname, "../crates/brain-visualizer");

function wasmHotRebuild(): Plugin {
  const srcDir = resolve(crateDir, "src");
  const cargoToml = resolve(crateDir, "Cargo.toml");
  let building = false;
  let queued = false;
  let timer: NodeJS.Timeout | null = null;

  return {
    name: "wasm-hot-rebuild",
    apply: "serve",
    configureServer(server) {
      server.watcher.add([srcDir, cargoToml]);

      const build = () => {
        if (building) {
          queued = true;
          return;
        }
        building = true;
        server.config.logger.info("[wasm] rebuilding…", { timestamp: true });
        const proc = spawn(
          "wasm-pack",
          ["build", crateDir, "--target", "web", "--dev"],
          { cwd: __dirname, stdio: "inherit" },
        );
        proc.on("exit", (code) => {
          building = false;
          if (code === 0) {
            server.config.logger.info("[wasm] rebuilt — reloading", {
              timestamp: true,
            });
            server.ws.send({ type: "full-reload", path: "*" });
          } else {
            server.config.logger.error(`[wasm] build failed (exit ${code})`, {
              timestamp: true,
            });
          }
          if (queued) {
            queued = false;
            build();
          }
        });
      };

      const onChange = (file: string) => {
        if (!file.endsWith(".rs") && file !== cargoToml) return;
        if (timer) clearTimeout(timer);
        timer = setTimeout(build, 150);
      };

      server.watcher.on("change", onChange);
      server.watcher.on("add", onChange);
      server.watcher.on("unlink", onChange);
    },
  };
}

// COOP/COEP headers enable SharedArrayBuffer / WASM threads in the dev server
// and preview (the coi-serviceworker shim covers static hosts like GitHub
// Pages that can't set headers — see web/public/coi-serviceworker.js).
const crossOriginIsolation = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

export default defineConfig({
  // index.html and public/ sit here in src/web/ (the default root/publicDir),
  // so no overrides are needed.
  plugins: [wasmHotRebuild(), wasm(), topLevelAwait()],
  // Vite's workspace-root autodetect stops at app/web/ (the lockfile lives
  // there and there's no JS-workspace marker above it), so the sibling
  // app/crates/.../pkg/ that main.ts imports the wasm from is outside the
  // default fs.allow and gets served as 403. Widen the allow-list to the
  // shared parent app/ so the wasm pkg/ is reachable over /@fs/.
  server: { headers: crossOriginIsolation, fs: { allow: [resolve(__dirname, "..")] } },
  preview: { headers: crossOriginIsolation },
  // The CPU coordinator worker (web/cpu-worker.ts) dynamically imports the wasm
  // pkg, which forces code-splitting; ES module workers are required for that
  // (the default IIFE worker format cannot code-split). Module workers also
  // carry crossOriginIsolated into the worker for SharedArrayBuffer (BV24).
  worker: { format: "es", plugins: () => [wasm(), topLevelAwait()] },
  // The wasm-pack `pkg/` output is referenced directly by web/main.ts.
  optimizeDeps: { exclude: ["brain_visualizer"] },
});
