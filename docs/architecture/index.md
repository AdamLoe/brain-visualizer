# Architecture index

Current-state subsystem docs. These describe what the system is now, not how it
changed over time. Load only the doc that matches your task; follow its
`See also` into `decisions/` for rationale.

## The model (what is simulated)

| Need | Read |
|---|---|
| Cortical surface generation (icosphere → gyrification → neuron placement), region assignment, per-neuron morphology geometry | [`manifold.md`](manifold.md) |
| SoA GPU buffer layout, the packed `last_spike` word (valid bit + type + tick), fixed-point current | [`data-model.md`](data-model.md) |
| Procedural hash connectivity (`target`/`weight`, spatial grid, per-tier K, feed-forward bias) | [`connectivity.md`](connectivity.md) |
| LIF dynamics, E/I balance, ambient drive, SOC tuning, heterogeneity, weight norm, input modes, stimulation; the sim boundary contract | [`simulation.md`](simulation.md) |

## GPU backend (the live path)

| Need | Read |
|---|---|
| The per-tick compute frame graph + render-pass ordering, buffer/pipeline lifecycle, indirect dispatch, async readback | [`gpu-backend.md`](gpu-backend.md) |
| Render scheme: far glow billboards, live morphology pass, bloom, visual mode enums | [`gpu-rendering.md`](gpu-rendering.md) |
| Deleted active-edge ribbon subsystem history; morphology superseded it | [`active-edges.md`](active-edges.md) |

## Frontend & tooling

| Need | Read |
|---|---|
| The TS app shell: `main.ts` orchestrator, wasm bridge, camera, controls, renderer, public HUD | [`web-frontend.md`](web-frontend.md) |
| The hidden dev panel: triggers, tabs, impact dots, settings persistence contract | [`dev-panel.md`](dev-panel.md) |
| Perf instrumentation: profiler + HUD, the GPU metrics reduction pass, async non-blocking readback | [`profiling.md`](profiling.md) |

## Cross-cutting

| Need | Read |
|---|---|
| Fixed-N scaling, difficulty tiers, the 20k product cap, adapter-limit caps | [`scaling.md`](scaling.md) |
| The retired CPU/WebGL2 backend boundary and stale-config normalization | [`cpu-backend.md`](cpu-backend.md) |
| Build pipeline (wasm-pack/vite), COOP/COEP, examples, test gates | [`build-and-deploy.md`](build-and-deploy.md) |

## See also

- [`../decisions/index.md`](../decisions/index.md) — why these are shaped this way.
- [`../_meta/ownership.json`](../_meta/ownership.json) — canonical owner per concept.
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) — doc-authoring rules.
