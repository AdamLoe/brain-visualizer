# System overview

**Brain Visualizer** is a hardware-adaptive spiking-neural-network sculpture: a
living patch of cortex where point/LIF neurons sit on a procedurally folded
brain surface, are wired by a deterministic local hash rule, and are simulated
in real time so signals visibly propagate through the structure. It is built
from scratch — own shaders, kernels, data layout — with `wgpu` as a thin GPU
binding, **not** a graphics engine. Beauty and dynamics readability come before
neuron count.

It is the homepage centerpiece for `adamloe.com`, developed inside that repo for
now but kept self-contained so extraction is a folder move.

## Shape

```
┌──────────────────────────────────────────────────────────────┐
│  Browser tab                                                  │
│                                                               │
│  web/ (TypeScript, Vite)                                      │
│    main.ts ── rAF loop ── wasm bridge ── camera / controls    │
│      │            │           │              dev-panel / HUD  │
│      │            │           ▼                               │
│      │            │     Rust/WASM crate (crates/brain-visualizer/src/)                │
│      │            │       GpuBackend ── WebGPU device         │
│      │            │         per tick:  integrate → indirect   │
│      │            │           scatter → stimulate/metrics      │
│      │            │         per frame: manifold → far glow →   │
│      │            │           morphology → bloom               │
│      │            ▼                                            │
│      └──► canvas (WebGPU). State stays GPU-resident; no rAF    │
│           readback (metrics via async Idle/Pending staging).  │
└──────────────────────────────────────────────────────────────┘
   Parked: a CPU/WebGL2 backend (rayon active-list sim in a worker,
   SharedArrayBuffer bridge) — code kept, unwired in V2.
```

The network starts **silent**; ambient drive into the posterior input region
ramps activity that propagates through the cortex. There is no scripted intro —
the physics is the intro.

## The current state (read this before assuming the old docs)

The shipped build (**V2**) diverged from earlier plans in ways the architecture
docs capture but a casual reader may not expect:

- **GPU-only.** The CPU/WebGL2 backend is parked — code kept, but GPU is forced
  at boot and the backend toggle is hidden. See
  [architecture/cpu-backend.md](architecture/cpu-backend.md).
- **The live connection/neuron visual is the per-neuron *morphology* renderer**
  (`crates/brain-visualizer/src/sim/morphology.rs` → `render_morphology.wgsl`). The active-edge **ribbon**
  pass, the near-LOD **cylinder** pass, and the near-LOD **sphere** pass are all
  retired behind `DRAW_LEGACY_*` flags in `crates/brain-visualizer/src/sim/gpu/mod.rs` — present but not
  drawn. See [architecture/gpu-rendering.md](architecture/gpu-rendering.md) and
  [architecture/active-edges.md](architecture/active-edges.md).
- **Regions are assigned uniformly at random over the volume**, not as
  contiguous anterior/posterior lobes; directionality comes from the
  excitatory feed-forward bias in the connectivity rule. See
  [architecture/manifold.md](architecture/manifold.md).
- **Default scale is small** (`DEFAULT_CONFIG` ≈ 1.2k neurons / K=16 in
  `web/src/core/types.ts`) — beauty-first. High-N tiers and the adaptive scaler exist but
  are gated. See [architecture/scaling.md](architecture/scaling.md).

## Hard-to-grep facts

- The GPU/CPU paths must stay **bit-identical** on connectivity: the 32-bit hash
  and `target`/`weight` rule are implemented once in Rust and once in WGSL and
  gated by `crates/brain-visualizer/tests/wgsl_*_determinism.rs`.
- WGSL has **no f32 atomics** → synaptic current accumulates in fixed-point i32
  (scale S = 4096). WGSL has **no portable point size** → glow is quad
  billboards, never `@builtin(point_size)`.
- The headless/WSL2 dev box has **no real GPU**; `cargo test` and the examples
  run under **llvmpipe** (software Vulkan) — correctness only, never a perf
  benchmark.
- Two cross-language layout contracts are the main corruption risks:
  `MorphSegment` (Rust ↔ WGSL) and the `VisualSettings` Float32Array index
  (`web/src/core/settings.ts` ↔ `crates/brain-visualizer/src/sim/gpu/mod.rs`).

## Where to go next

- Subsystem facts: [architecture/index.md](architecture/index.md).
- Design rationale: [decisions/index.md](decisions/index.md).
- Where files live: [repository-layout.md](repository-layout.md).
- Run it: [agent-context/dev-loop.md](agent-context/dev-loop.md).

## See also

- [index.md](index.md) — global router.
- [repository-layout.md](repository-layout.md) — file inventory.
