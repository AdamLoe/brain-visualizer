# Repository layout

One sentence per non-trivial directory. Use this to find where a thing lives
without grepping. Architecture docs explain what the things *are*; this doc maps
paths.

The repo **root** holds only `docs/`, `app/`, `README.md`, and dotfiles — all
source lives under `app/`. Doc code anchors are written relative to
`code_root: app` (e.g. `crates/brain-visualizer/src/lib.rs`, `web/src/main.ts`).
**Cargo runs from `app/`** (workspace root); **npm runs from `app/web/`**.

```
app/                         Workspace root — Cargo.toml here is the workspace manifest.
  crates/brain-visualizer/   The Rust crate (compiles to WASM; pure logic also host-tested).
    Cargo.toml               Crate manifest: [lib] cdylib+rlib.
    src/
      lib.rs                 wasm_bindgen entry points + host-callable build hooks.
      buffers.rs             SoA buffer layout math.
      profiler.rs            Perf ring buffer + per-second JSON dump.
      gpu_limits.rs          Adapter limits → derived caps.
      connectivity/          Procedural wiring: 32-bit hash, integer spatial grid,
                             target/weight pure functions (Rust/WGSL parity).
      manifold/              Cortical surface: icosphere, gyrification, region
                             assignment, neuron placement.
      sim/
        backend.rs           SimBackend trait, SimConfig, TickStats, shared bit/type helpers.
        scaler.rs            Adaptive scaler (within-tier feedback).
        morphology.rs        Per-neuron morphology geometry (MorphSegment), the live visual.
        gpu/                 Live GPU backend.
          mod.rs             GpuBackend: frame graph, pass ordering, readback.
          pipelines.rs       Pipeline + bind-group-layout construction.
          resources.rs       Persistent buffers, bind groups, dirty flags.
          shaders/*.wgsl     Compute (integrate, scatter, stimulate, metrics,
                             compact_morph_segments, write_scatter_dispatch) + render
                             (far, manifold, morphology, bloom).
    examples/                Offline host-verification harnesses (sim_check,
                             soc_sweep, render_check, morph_view), run via
                             `cargo run --example <name>`.
    tests/                   Rust integration tests: wgsl_hash_determinism,
                             wgsl_target_determinism, gpu_sim_dynamics.

  web/                       TypeScript app + JS project root (run npm here).
    package.json             npm scripts (wasm/dev/build/test); package-lock.json.
    index.html               Vite entry.
    vite.config.ts vitest.config.ts playwright.config.ts tsconfig.json   Tool configs (flat, conventional).
    src/
      main.ts                rAF loop, wasm bridge, orchestration.
      core/
        types.ts             AppConfig, tier presets, defaults.
        settings.ts          VisualSettings persistence + Float32Array contract.
        setting-metadata.ts  SETTING_IMPACT classification table.
      render/
        camera.ts            Orbital camera + pointer tracking.
        renderer.ts          WebGPU device/canvas setup.
        profiler.ts          Perf display (mirrors Rust Profiler).
      ui/
        controls.ts          Brain-state presets, scaler, GPU backend facade.
        dev-panel.ts         Hidden dev panel (gear icon).
        hud.ts               Public corner HUD (CornerHud).
    public/
      coi-serviceworker.js   COOP/COEP shim for GitHub Pages (Vite publicDir).
    e2e/                     Playwright specs (brain_visualizer.spec.ts).

docs/                        This documentation tree (agent-docs v1). See index.md.
```

## See also

- [`index.md`](index.md) — global router.
- [`architecture/index.md`](architecture/index.md) — what these dirs *do*.
- [`agent-context/dev-loop.md`](agent-context/dev-loop.md) — how to run them.
