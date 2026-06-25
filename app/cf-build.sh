#!/usr/bin/env bash
# Cloudflare Pages deploy build — COMPILE-ALL path.
#
# Cloudflare rebuilds everything from source on every deploy: Rust→WASM via
# wasm-pack, then the Vite bundle. NOTHING is prebuilt or committed — pkg/ and
# dist/ stay gitignored. `npm run build` (in app/web/) already chains
# wasm-pack → tsc --noEmit → vite build and emits the content-hashed bundle to
# app/web/dist/ (index-*.js, index-*.css, *_bg-*.wasm under dist/assets/).
# Vite copies app/web/public/_headers to dist/_headers so the static host can
# set COOP/COEP/CORP + frame-ancestors natively.
#
# Cloudflare Pages settings:
#   Root directory:          (repo root — leave blank)
#   Build command:           bash app/cf-build.sh
#   Build output directory:  app/web/dist
#
# Preview the exact production bundle locally:
#   bash app/cf-build.sh && ( cd app/web && npx vite preview )
#   # or: python3 -m http.server 5184 -d app/web/dist
set -euo pipefail

APP="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # app/
WEB="$APP/web"

# Rust: Cloudflare's build image ships no Rust toolchain, so bootstrap rustup on
# CI. There is no rust-toolchain.toml in this repo, so pin the channel here
# (matches the sibling fluid-simulation build). On a dev box cargo is already on
# PATH and this block is skipped.
if ! command -v cargo >/dev/null 2>&1; then
  echo "==> Installing Rust (rustup)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --profile minimal --default-toolchain 1.95.0
  export PATH="$HOME/.cargo/bin:$PATH"
fi

# Ensure the wasm target is present (no-op if already installed).
rustup target add wasm32-unknown-unknown 2>/dev/null || true

# wasm-pack: prebuilt-binary installer (fast, reliable on CI). Needs Rust
# present, so it runs after the rustup bootstrap above.
if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "==> Installing wasm-pack…"
  curl -sSf https://rustwasm.github.io/wasm-pack/installer/init.sh | sh
  export PATH="$HOME/.cargo/bin:$PATH"
fi

echo "==> Building bundle (wasm-pack + tsc --noEmit + vite build)…"
( cd "$WEB" && npm ci && npm run build )

echo "==> Done — output in $WEB/dist"
ls -la "$WEB/dist" "$WEB/dist/assets"
