# Dev loop

## When does this apply

You want to run the visualizer, or run an example to see a change working. The
build/deploy mechanics are owned by
[`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md);
this is the short "how do I drive it" recipe.

## Run the app

The JS project lives in `app/web/` — **run npm from there**:

```bash
cd app/web
npm install                 # first time only
fuser -k 5173/tcp 5174/tcp  # EVERY restart: free the port(s) first (see below)
npm run dev                 # wasm-pack (dev) builds ../crates/brain-visualizer + vite, COOP/COEP set
```

**Free port 5173 on every restart — not just the first time.** Vite serves on
5173 by default but silently falls back to the next free port (5174, …) if it's
taken, printing the chosen URL. A leftover `npm run dev` from an earlier session
(or one that didn't shut down cleanly) keeps 5173 held, so your new server lands
on 5174 — and the Playwright e2e suite, which hard-codes `localhost:5173`, would
then drive the *stale* server. Because stale holders are easy to leave behind,
do the kill **before every `npm run dev`**, not only the first run:

```bash
ss -tlnp | grep -E ':517[34]'   # what (if anything) is listening
fuser -k 5173/tcp 5174/tcp      # kill the holder(s) — Linux/WSL
```

`vite.config.ts` changes (e.g. `server.fs.allow`, headers, plugins) are **not
hot-applied** — a config edit also requires a full dev-server restart, so the
same kill-then-`npm run dev` sequence applies.

`npm run dev` rebuilds the wasm package and serves the harness with
cross-origin isolation. The app boots GPU-only at the beauty-first
default. The hidden dev panel
opens via `?dev=1`, the backtick key, or the bottom-right affordance
([`../architecture/dev-panel.md`](../architecture/dev-panel.md)).

## Run an example (offline, no browser)

Cargo runs from the workspace root `app/`:

```bash
cd app
cargo run --release --example soc_sweep    # criticality sweep
cargo run --release --example morph_view   # morphology geometry
```

The examples run natively under llvmpipe — the fastest way to confirm sim or
shader behavior. See
[`testing-how-to.md`](testing-how-to.md) for the full gate list.

## Browser-only checks

Canvas appears, `crossOriginIsolated === true`, the profiler dumps one JSON
line/sec, speed presets change tick rate — these can only be confirmed in a real
browser (not headless).

## See also

- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
- [`../architecture/web-frontend.md`](../architecture/web-frontend.md) — what the app shell does.
- [`testing-how-to.md`](testing-how-to.md), [`index.md`](index.md).
