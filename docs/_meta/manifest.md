# Agent-docs manifest — brain_visualizer

App-specific bindings for the global agent-docs kit. The generic skills and
rules in `~/.claude/agent-docs/v1/` read the slots below; everything
app-specific lives here, nothing generic does.

```yaml
agent_docs_version: v1
repo_name: brain_visualizer — hardware-adaptive spiking-neural-network visualizer (WebGPU/WASM)
code_root: app
```

> **Roots.** Agent-docs v1 fixes the docs root at `docs/`. The repo root holds
> only `docs/`, `app/`, `README.md`, and dotfiles; **all source lives under
> `app/`**, so `code_root: app`. Every code anchor in the docs is relative to it
> (e.g. `crates/brain-visualizer/src/sim/gpu/mod.rs`,
> `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl`, `web/src/main.ts`).
> Cargo runs from `app/` (workspace root); npm runs from `app/web/`. WGSL files
> have named functions/consts — point to those, not line numbers.

## Slot: decisions-domains

`scope`, `backends`, `data-layout`, `connectivity`, `dynamics`, `manifold`,
`rendering`, `scaling`, `interaction`, `profiling`, `dev-tooling`.
(Authoritative list: `ls docs/decisions/`.)

## Slot: drift-gates

Per-commit gates (full detail in
[`../agent-context/testing-how-to.md`](../agent-context/testing-how-to.md)):

- `cargo test` — host unit tests + the determinism gates
  (`crates/brain-visualizer/tests/wgsl_hash_determinism.rs`, `crates/brain-visualizer/tests/wgsl_target_determinism.rs`) +
  `crates/brain-visualizer/tests/gpu_sim_dynamics.rs`. Runs under llvmpipe headless.
- `npm run typecheck` — `tsc --noEmit` over `web/`.
- `npm test` — vitest unit tests.
- `npm run test:e2e` — Playwright e2e (browser; not headless-CI by default).

## Slot: change-to-doc

Consult before declaring a commit done. "If you changed X → update Y."

| If you changed… | Update… |
|---|---|
| `crates/brain-visualizer/src/manifold/*` (icosphere, gyrify, placement), `crates/brain-visualizer/src/manifold/regions.rs` | `architecture/manifold.md`, `decisions/manifold.md` |
| `crates/brain-visualizer/src/sim/morphology.rs` (`MorphSegment` layout, the Prim-tree axon arbor in `generate`, `MorphSphereInstance` layout/`emit_soma_spheres`) | `architecture/manifold.md`, `architecture/gpu-rendering.md` (both morphology sub-passes), and the WGSL structs in `render_morphology.wgsl` |
| `crates/brain-visualizer/src/buffers.rs`, the packed-word / mask helpers (`crates/brain-visualizer/src/sim/backend.rs`, `integrate.wgsl`) | `architecture/data-model.md`, `decisions/data-layout.md` |
| `crates/brain-visualizer/src/connectivity/*` (hash, spatial grid, `target`/`weight`, the heavy-tailed `ReachParams`/`long_offset_component` long-range branch + `salt::REACH_*`), `hash.wgsl`, and the WGSL `target_neuron` twin in `scatter.wgsl` (`long_range_frac`/`max_reach` in `ConnectUniforms`) | `architecture/connectivity.md`, `decisions/connectivity.md`, regenerate/verify the determinism gates |
| `crates/brain-visualizer/src/sim/cpu/core.rs`, `integrate.wgsl`, `scatter.wgsl`, `stimulate.wgsl` (LIF math, heterogeneity, weight norm, input modes) | `architecture/simulation.md`, `decisions/dynamics.md` |
| `crates/brain-visualizer/src/sim/backend.rs` (`SimBackend`, `SimConfig`, `TickStats`), the `VisualSettings` Float32Array contract | `architecture/simulation.md`, `architecture/web-frontend.md`, `architecture/dev-panel.md` (the settings index) |
| `crates/brain-visualizer/src/sim/gpu/mod.rs` (frame graph, pass ordering, `DRAW_LEGACY_*`, readback) | `architecture/gpu-backend.md`; `architecture/gpu-rendering.md` if a render pass changes |
| `crates/brain-visualizer/src/sim/gpu/{pipelines,resources}.rs` (buffers, pipelines, bind groups) | `architecture/gpu-backend.md` |
| `crates/brain-visualizer/src/sim/gpu/shaders/render_*.wgsl`, `bloom.wgsl`, `frustum_cull.wgsl`, `draw_indirect.wgsl` | `architecture/gpu-rendering.md`, `decisions/rendering.md` |
| `crates/brain-visualizer/src/sim/gpu/shaders/{emit_edges,render_ribbon}.wgsl` (retired ribbon path) | `architecture/active-edges.md` |
| `crates/brain-visualizer/src/sim/cpu/*`, `web/src/cpu/cpu-worker.ts`, `web/src/cpu/cpu-renderer.ts`, the `cpu-threads` feature | `architecture/cpu-backend.md`, `decisions/backends.md` |
| `web/src/main.ts`, `camera.ts`, `controls.ts`, `renderer.ts`, `types.ts`, `sonification.ts` | `architecture/web-frontend.md`, `decisions/interaction.md` |
| `web/src/ui/dev-panel.ts`, `settings.ts`, `setting-metadata.ts` (persistence, impact table) | `architecture/dev-panel.md`, `decisions/dev-tooling.md` |
| `web/src/core/morph-config.ts` (`MorphologyConfig`, `MORPH_DESCRIPTORS`, `bv2_morph_v1`), `set_morphology_config` (`lib.rs` / `sim/gpu/mod.rs`), `MorphologyConfig`/`GeneratorConfig`/`RenderQualityConfig`/`LightingConfig` in `sim/morphology.rs` | `architecture/dev-panel.md`, `architecture/manifold.md`, `architecture/gpu-rendering.md`, `decisions/dev-tooling.md`, `decisions/manifold.md` |
| `web/src/render/profiler.ts`, `hud.ts`, `crates/brain-visualizer/src/profiler.rs`, `metrics.wgsl`, the metrics readback | `architecture/profiling.md`, `decisions/profiling.md` |
| `crates/brain-visualizer/src/sim/scaler.rs`, `crates/brain-visualizer/src/gpu_limits.rs`, tier presets in `web/src/core/types.ts` | `architecture/scaling.md`, `decisions/scaling.md` |
| `web/package.json`, `web/*` (vite/vitest/playwright/tsconfig), `crates/brain-visualizer/Cargo.toml`, COOP/COEP shim, `crates/brain-visualizer/examples/*`, `crates/brain-visualizer/tests/*` | `architecture/build-and-deploy.md`, `agent-context/testing-how-to.md` |
| A new/removed/re-routed architecture doc | `architecture/index.md`, `_meta/ownership.json` if ownership changes |
| A new/removed decisions domain | `decisions/index.md`, this manifest's `decisions-domains` slot |
| Repository directory layout | `repository-layout.md` |

## Slot: drift-verification (high-risk surfaces)

The doc-fix sweep verifies `path → symbol` pointers still resolve and
spot-checks these against code:

- The `MorphSegment` field order/size (48 B, branch-only; Rust `crates/brain-visualizer/src/sim/morphology.rs` ↔ WGSL
  `render_morphology.wgsl`) and any edge-event layout.
- The `MorphSphereInstance` field order/size (32 B, soma-only; Rust `crates/brain-visualizer/src/sim/morphology.rs → MorphSphereInstance` ↔ the WGSL sphere struct in `render_morphology.wgsl`).
- `MorphUniforms` size (192 B; Rust `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms` ↔ WGSL `render_morphology.wgsl`; must update both sides atomically). Includes the lighting/brightness fields `resting_brightness`/`active_boost` and the true-opacity fields `active_opacity`/`inactive_opacity_floor` (the latter two repurposed from the former trailing `_pad4`/`_pad5` — size unchanged at 192 B).
- The `VisualSettings` Float32Array index contract (`web/src/core/settings.ts` ↔
  `crates/brain-visualizer/src/sim/gpu/mod.rs`), including the heavy-tailed-reach
  indices `longRangeReachFrac`/`maxReachCells` that `set_visual_settings` packs
  (as integers over `REACH_FRAC_DEN`) into the `ConnectUniforms` `long_range_frac`/`max_reach`
  slots (Rust `resources.rs` ↔ WGSL `scatter.wgsl`, still 32 B).
- The packed `last_spike` masks and the locked hash constants (Rust ↔ WGSL).
- The `DRAW_LEGACY_*` guard flags (which passes are live vs retired).
- `DEFAULT_CONFIG` scale (`web/src/core/types.ts`).

## Notes

- The generic kit (authoring rules, coding-style, repo-rules, orchestrating)
  lives at `~/.claude/agent-docs/v1/`; workflow commands are global skills. This
  manifest is the only app-specific binding the kit reads.
- `ownership.md` is a thin pointer; the owner data is `_meta/ownership.json`.
