# Brain Visualizer — Current Plan

_Source of truth for the visualizer's active direction. Last updated: 2026-06-03._

## What this is
An interactive 3D spiking neural network rendered as a glowing, firing patch of
cortex — the homepage centerpiece for adamloe.com, and a standalone systems/perf
showcase in its own right.

## Status / relationship to adamloe.com
- **Currently developed inside the `adamloe.com` repo** for convenience; will be
  **extracted into its own project** later. Keep it self-contained (own docs,
  own build) so extraction is a folder move.
- The site embeds it as the homepage effect (site decision D5). Name sits as
  text above it; the viz is a decoupled centerpiece (site decision D8).

## Orchestrator handoff
Use this file as the entry point, then read `decisions.md`, `architecture.md`,
and the active phase file before building. Do not re-litigate locked BV
decisions unless implementation proves one false; add a dated amendment instead.

Build in phase order. Phase 0 is a throwaway measurement spike; phase 1 creates
all module boundaries and stubs; later phases fill them in. Keep the project
extractable: all visualizer-specific source, docs, build files, assets, and
service-worker setup stay under `brain-visualizer/`.

Manual tier selection ships first (BV3), but the adaptive scaler may resize
`N`, `K`, and render resolution **within the selected tier** to stay near the
frame budget. Auto-picking a tier for the device is future work.

## Direction (see `decisions.md` for the locked calls)
- From-scratch, point-neuron (LIF) SNN, peak-optimized, hardware-adaptive (BV1).
- Two backends for CPU-vs-GPU comparison: WebGPU/`wgpu` (data-parallel) and
  WebGL2 + `rayon` active-list simulation (event-driven; SIMD after profiling)
  (BV4). CPU simulation runs in a dedicated coordinator Web Worker that owns the
  rayon worker pool and writes SharedArrayBuffer state for the main thread to
  render (BV24).
- Procedural connectivity (BV6), LOD rendering (BV7), three tiers (BV3),
  first-class profiling (BV8) + a small corner HUD.
- **Tuned for self-organized criticality / neuronal avalanches** with an
  excitability slider (BV9); named brain-state presets (Deep sleep → Seizure)
  label the axis (BV15).
- **Slight interactivity, "pretty toy" framing** (BV10): cursor stimulation,
  click-drag orbit + scroll zoom. Click-to-inspect is post-MVP. No scripted
  wake-up intro — natural input→center→output propagation from a silent start is
  the intro.
- **Sonification**, muted by default (BV11). Backend choice via a top-right
  toggle; side-by-side race deferred (BV12).
- **Simulation speed controls** (¼×/½×/1×/2×) in the top-left (BV14).
- **Cortical manifold = procedurally folded brain surface** (gyri + sulci);
  neuron count may be reduced to preserve brain-like shape (BV13).
- **Mobile:** runs, but scaled down (small render size, single backend, far LOD
  only) — not a big effort.
- Full engineering design: **`architecture.md`**.

## Implementation Phases
Detailed plans in `phases/`. Build GPU-first (visual confidence early);
CPU backend is a comparison feature added later. All architectural hooks
scaffolded in phase 1 even if not yet functional.

| Phase | Name | Key outcome |
|-------|------|-------------|
| 0 | Benchmark spike | Real throughput numbers; validate tier ceilings |
| 1 | Foundation | Full scaffold: interfaces, manifold, connectivity, profiler, all stubs |
| 2 | GPU sim core | LIF running on GPU; correct dynamics visible in console |
| 3 | GPU rendering | Brain lights up; camera; cursor stimulation |
| 4 | Near LOD | Zoom-in spheres + cylinders via GPU indirect draw |
| 5 | Controls | Brain states button group, speed controls, backend toggle UI |
| 6 | CPU backend | rayon active-list sim; bit-identical connectivity; WebGL2 render |
| 7 | Polish | Sound, HUD final pass, SOC tuning, mobile, 10M disclaimer |

## Open Questions (technical)
1. Sim-accurate conduction delay on any tier, or visual-only everywhere?
2. Exact N where connectivity switches from store-once to regenerate?
3. ~~CPU backend threading topology~~ — **resolved (BV24):** dedicated CPU sim
   coordinator worker + rayon pool; main thread renders and handles input.
4. ~~Cortical-manifold source~~ — **resolved (BV13):** procedurally folded
   brain surface (gyri/sulci), from scratch, neuron count flexible.
5. Gyrification algorithm details: subdivide-and-noise approach, resolution
   of the surface mesh, and how neurons are placed on/near the surface.
6. ~~Backend switch mid-sim~~ — **resolved (BV16):** full restart, same seed.
7. ~~Region topology (input/output)~~ — **resolved (BV17):** posterior =
   input (thalamic `I_ext` drive), central = association, anterior = output.
   E/I balance handles dissipation; no special output sink.
8. ~~Per-tier K~~ — **resolved (BV18):** K scales with tier (16–32 / 32–64 /
   64–128); adaptive scaler adjusts K alongside N within a tier.
9. ~~GPU/CPU hash primitive~~ — **resolved (BV22):** use a WGSL-friendly 32-bit
   integer hash with golden vectors, not `u64` PCG.
