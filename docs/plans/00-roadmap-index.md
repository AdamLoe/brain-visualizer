---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - plans/*
  - architecture/*
  - decisions/*
---

# v0.3+ Visual Roadmap Orchestration Hub

## Mission

Turn the app from a technically impressive neural glow/simulation into a
recognizable, living brain-shaped visual experience. The immediate roadmap is
visual and interaction-first:

1. make the arena read as a real brain-shaped procedural sculpture,
2. make dendrites/axons grow as curved chains instead of straight-ish rods,
3. make neurons look organic through restrained material/lighting polish,
4. make spikes read as soma pulses plus traveling impulses,
5. make camera movement easier with right-drag panning.

This roadmap intentionally does **not** introduce an educational lesson system,
selection/picking, a dashboard UX, or a broader product pivot. Those can be
revisited only after the default visual feels alive.

## Current wave

**Wave 0 — docs/orchestration repair.** Finish this hub, recreate the deferred
roadmap, add the shared visual acceptance contract, add the accepted-defaults
manifest, and align the versioned plans before implementation agents start.

**Wave 1 — independent foundations.** v0.3.4 can run independently. v0.3.0 is
the visual-identity blocker and should complete before tuning morphology against
the final stage.

**Wave 2 — morphology and shader work.** v0.3.1 lands curved chains. v0.3.2
material polish and v0.3.3 pulse semantics are both critical and should be
planned as one coordinated shader/config implementation surface before either
mutates `render_morphology.wgsl`, `MorphUniforms`, or `MorphologyConfig`.

**Wave 3 — integration.** v0.4.0 accepts one cohesive default look, records it
in [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md),
and adds hidden dev-panel review presets. v0.5.0 then stabilizes first load,
production preview, docs, and review artifacts.

Soft spatial region territories are backlog work only; the active roadmap does
not carry a separate v0.4.1 plan.

## Stream Tracker

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Wave 0 | Docs/orchestration repair | Done | Docs coherence pass fixed v0.3.2/v0.3.3 wording from blocked to gated and found no remaining link/status contradictions in the listed roadmap files. | Keep this tracker current as implementation streams report. | None known. |
| v0.3.0 | Brain-shaped arena / cortical placement | Code done | Manifold worker implemented one shared procedural brain envelope for shell generation and placement, preserved hash-shuffled regions/connectivity semantics, passed `cargo test -p brain-visualizer` with 90 tests and `render_check` under llvmpipe. | Capture/review default, top, side, shell-only, neuron-cloud-only, and morphology-on frames. | Adam visual acceptance still required for subjective brain-shape pass. |
| v0.3.1 | Curved-chain morphology | Code done | Morphology worker fixed dendrites to sampled curved chains, made axon `path_len` branch-local, preserved `MorphSegment` 48 B and `MorphSphereInstance` 32 B layouts, added path/count regressions, passed `cargo test -p brain-visualizer`, and ran `morph_view` with 68,574 segments / 0 dropped. | Carry visual artifacts into later acceptance review and let shader owner use confirmed cumulative `path_len`. | Final subjective tuning depends on accepted v0.3.0 stage. |
| v0.3.2/v0.3.3 | Material polish plus soma/traveling pulse | Code done | Shared shader worker implemented v0.3.3 soma pulse/traveling axon packet and v0.3.2 deterministic asset-free material polish without changing `MorphSegment`, `MorphUniforms`, or `RenderUniforms`; `cargo test -p brain-visualizer` passed with 90 tests and `morph_view` produced `/tmp/morph_0..3.rgba`, `/tmp/morph_active_bright.rgba`, and stats JSON. | Feed artifacts/settings into v0.4.0 defaults integration. | Subjective pulse/material acceptance remains with Adam. |
| v0.3.4 | Right-click pan / camera target | Code done | Frontend worker added right-drag and Shift-left-drag pan, context-menu suppression, hover suppression during drag, `R` recenter, docs, and a camera pan unit test. `npm run typecheck` and `npm test` passed in `app/web` with 24/24 tests passing. | Run live browser pointer check before final acceptance. | Manual browser check remains for orbit, pan, context menu, hover suppression, wheel zoom, and mobile touch sanity. |
| v0.4.0 | Cohesive defaults / hidden review presets | Code done | Defaults worker added hidden `accepted-default`, `performance-review`, and `hero-review` presets; `accepted-default` clones `DEFAULT_CONFIG` (`n=1200`, `k=16`, `tier=\"low\"`), `DEFAULT_SETTINGS`, and `DEFAULT_MORPH_CONFIG`; reset restores the live default network; `npm run typecheck` and `npm test` passed with 26/26 tests. | Use accepted-default data and artifact manifest in v0.5.0 stabilization. | Adam visual approval of default/performance/hero looks remains pending. |
| v0.5.0 | Showcase stabilization | Code/build verified | Stabilization worker restored the public HUD, passed `cargo test -p brain-visualizer`, `render_check`, `morph_view`, `npm run typecheck`, `npm test`, and `npm run build`; production preview served with COOP/COEP headers and Chromium smoke showed the HUD/frame counter. | Run real WebGPU browser beauty pass on supported hardware and collect Adam visual approval. | Local Chromium had `navigator.gpu` but no adapter; Adam remains final visual approver. |

## Plan order

| Plan | Role | Dependency | Parallelization |
|---|---|---|---|
| [`v0.3.0-brain-shaped-arena.md`](v0.3.0-brain-shaped-arena.md) | Procedural brain-shaped arena + cortical shell placement | Wave 0 docs | Blocks visual identity |
| [`v0.3.1-curved-chain-morphsegments.md`](v0.3.1-curved-chain-morphsegments.md) | Curved chain morphology | v0.3.0 accepted stage | Do not overlap shader/config work |
| [`v0.3.2-neuron-material-texture-pass.md`](v0.3.2-neuron-material-texture-pass.md) | Morphology material polish | Shader/config preflight | Critical; share one shader/config owner with v0.3.3 |
| [`v0.3.3-soma-pulse-and-traveling-impulses.md`](v0.3.3-soma-pulse-and-traveling-impulses.md) | Soma pulse + traveling impulse | v0.3.1 accurate `path_len`, shader/config preflight | Critical; share one shader/config owner with v0.3.2 |
| [`v0.3.4-right-click-pan-camera-target.md`](v0.3.4-right-click-pan-camera-target.md) | Right-click pan | none | Independent frontend stream |
| [`v0.4.0-cohesive-visual-defaults.md`](v0.4.0-cohesive-visual-defaults.md) | Accepted default look + hidden review presets | v0.3.x accepted | Integration stream |
| [`v0.5.0-showcase-stabilization.md`](v0.5.0-showcase-stabilization.md) | Showcase hardening | v0.4.0 accepted defaults | Final stabilization |

## Suggested execution

**Ship v0.3.4 whenever convenient.** Right-click panning is isolated to the
frontend/camera path and does not depend on arena shape or morphology work. It
is low-risk and immediately improves navigation.

**Keep v0.3.0 procedural-first.** The current manifold decision prefers an
asset-free procedural shape. v0.3.0 should use procedural brain shaping and
cortical-shell placement. Real mesh assets stay in
[`future_roadmap.md`](future_roadmap.md) and are not a fallback path inside
v0.3.0.

**Keep v0.3.1 before v0.3.3.** Traveling impulses need meaningful cumulative
`path_len` along short curved segment chains. Adding impulse timing before the
curved-chain rewrite risks tuning against geometry that will be thrown away.

**Plan v0.3.2 and v0.3.3 as one shader/config surface.** Material polish and
pulse semantics both touch `render_morphology.wgsl` and may pressure
`MorphUniforms` / `MorphologyConfig`. They remain separate acceptance goals, but
one implementation owner should sequence the shader/config edits and record the
preflight before code changes.

**Build hidden dev-panel review presets in v0.4.0.** Default/performance/hero
variants are useful for review and screenshots. They belong in the hidden dev
panel and harness metadata, not in the public UI.

**Keep spatial region coherence in backlog.** Hard spatial bands risk
reintroducing the prior "three glowing slabs" failure mode. Any future region
geography should be soft, probabilistic, deterministic, and reviewed as a
dynamics change.

## Shader/config preflight

Before v0.3.2 or v0.3.3 implementation, the shared shader/config owner must
answer and record:

- Can the work avoid changing `MorphSegment`? The expected answer is yes.
- Can the work avoid changing `MorphUniforms`? Prefer fixed defaults or existing
  fields unless review proves controls are needed.
- If `MorphUniforms` changes, which Rust/WGSL fields and size asserts change
  atomically?
- Which new `MorphologyConfig` fields are live uniform writes, renderer-rebuild
  controls, or hidden-preset-only defaults?
- Can soma pulse and segment pulse share one timing model from `last_spike`?
- Can traveling impulse position derive only from `path_len` plus local segment
  interpolation?

Record the preflight in
[`implementation-review-briefs.md`](implementation-review-briefs.md) or in the
implementing PR before the shader branch starts. It does not need its own code
branch unless the answers force a layout/config change.

## File ownership map

| Surface | Primary plan owner | Collision rule |
|---|---|---|
| `crates/brain-visualizer/src/manifold/*` | v0.3.0 | Do not mix shape/placement work with region reassignment |
| `crates/brain-visualizer/src/sim/morphology.rs` | v0.3.1 | Coordinate with v0.3.2/v0.3.3 before changing segment semantics |
| `render_morphology.wgsl` | v0.3.2/v0.3.3 shared shader owner | Single owner at a time |
| `MorphUniforms` / `MorphologyConfig` | v0.3.2/v0.3.3 shared shader owner, v0.4.0 defaults owner | Preflight before adding fields |
| `render_far.wgsl` | v0.3.3 | Coordinate soma pulse timing with morphology shader |
| `web/src/render/camera.ts`, `web/src/main.ts` input handlers | v0.3.4 | Independent from GPU/render morphology work |
| `web/src/ui/dev-panel.ts`, settings metadata | v0.4.0 | Hidden review presets allowed; no public preset manager |

## Roadmap discipline

- Use [`visual-acceptance-contract.md`](visual-acceptance-contract.md) for
  subjective visual gates. llvmpipe validates correctness, not beauty or real GPU
  performance.
- Plans may mention versions. Architecture docs must not. When a plan ships,
  rewrite the owning architecture docs in place to describe what is true now.
- Decisions are updated by domain only when a new choice is actually accepted.
  Superseded decision text should be removed, not dated.
- Do not add runtime fallback toggles just to preserve the old look. History is
  in git; the live product should have one clear default path.
- No per-frame allocation in the rAF loop or GPU tick path.
- No CPU↔GPU readback in the render loop.
- Keep Rust/WGSL layout contracts byte-identical when touching shared structs.
- Keep the public UI shallow. New tuning controls belong in the hidden dev panel
  unless the default visitor clearly needs them.
- Hidden dev-panel review presets are allowed in v0.4.0. They must not become
  public modes or split the first-load default into a choice.

## Decision log

- **2026-06-08:** Recreate `future_roadmap.md`; broken links are not an accepted
  steady state.
- **2026-06-08:** v0.3.0 uses procedural brain shaping plus cortical-shell
  placement. Real mesh assets remain backlog-only and are not a fallback inside
  v0.3.0.
- **2026-06-08:** Add a shared visual acceptance contract. Individual plans link
  to it instead of inventing their own meaning for "accepted frames."
- **2026-06-08:** v0.3.2 is narrowed to material polish. It is not a broad
  texture/noise feature or an external asset pipeline.
- **2026-06-08:** Soft spatial region territories move to backlog only. No hard
  spatial slabs before the showcase is stable.
- **2026-06-08:** v0.3.2 remains critical, but v0.3.2/v0.3.3 shader and config
  edits are coordinated as one implementation surface.
- **2026-06-08:** v0.4.0 owns hidden dev-panel review presets and the accepted
  defaults manifest. Public presets remain out of scope.
- **2026-06-08:** The soft spatial region plan is moved to backlog
  (`future_roadmap.md`) instead of living as an active versioned plan.

## Review gates to answer later

- Does the procedural arena read as a brain in shell-only and neuron-cloud-only
  captures?
- Does cortical-shell placement avoid a glowing solid-volume look?
- Does v0.3.2 need any new uniform/config fields after hardcoded/subtle defaults
  are reviewed?
- Does v0.3.3 make motion legible without misleading upstream causality?
- Does the v0.4.0 hidden dev-panel preset implementation cleanly load
  default/performance/hero variants without leaking into public UI?
- Does the accepted defaults manifest contain enough exact config/camera/artifact
  data for v0.5.0 stabilization to reproduce the look?

## Deferred on purpose

Deferred work lives in [`future_roadmap.md`](future_roadmap.md). Do not grow this
hub into a second parking lot.

## See also

- [`future_roadmap.md`](future_roadmap.md)
- [`visual-acceptance-contract.md`](visual-acceptance-contract.md)
- [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md)
- [`implementation-review-briefs.md`](implementation-review-briefs.md)
- [`v0.3.0-brain-shaped-arena.md`](v0.3.0-brain-shaped-arena.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/web-frontend.md`](../architecture/web-frontend.md)
- [`../decisions/interaction.md`](../decisions/interaction.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
