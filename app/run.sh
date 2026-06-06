#!/usr/bin/env bash
#
# Rebuild the wasm crate and serve the visualizer. Runs in the foreground:
# Ctrl-C to stop, then re-run to rebuild + restart (handy after a plan finishes).
#
#   ./run.sh            dev build  (fast, unoptimized wasm)  + vite dev
#   ./run.sh release    optimized wasm                       + vite dev
#
set -euo pipefail

# Resolve to app/web regardless of where this is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/web"

# Free the dev ports first — Vite silently falls back to 5174 if 5173 is held by
# a stale server, which then desyncs the Playwright e2e suite (hard-coded :5173).
echo ">> freeing ports 5173/5174 (if held)"
fuser -k 5173/tcp 5174/tcp 2>/dev/null || true

# Pick the wasm build profile.
if [[ "${1:-}" == "release" ]]; then
  echo ">> building wasm (release)"
  npm run wasm
else
  echo ">> building wasm (dev)"
  npm run wasm:dev
fi

echo ">> serving on http://localhost:5173/  (Ctrl-C to stop)"
exec npx vite
