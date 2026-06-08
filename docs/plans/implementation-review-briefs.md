---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - plans/*
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - architecture/web-frontend.md
  - architecture/dev-panel.md
  - architecture/profiling.md
  - decisions/manifold.md
  - decisions/rendering.md
  - decisions/interaction.md
  - decisions/dev-tooling.md
---

# Implementation Review Briefs

## Mission

Give future agents narrow briefs for creating detailed implementation plans
before code work starts. These briefs capture the orchestration decisions that
are easy to miss in a fresh chat: v0.3.2 is critical, v0.3.2/v0.3.3 share one
shader/config implementation surface, hidden dev-panel presets are in scope for
v0.4.0, and soft spatial regions are backlog-only.

Agents using this doc should produce a concrete implementation plan or PR
handoff for their stream. They should not mark subjective visual work accepted;
Adam remains the visual approver under
[`visual-acceptance-contract.md`](visual-acceptance-contract.md).

## Shared startup context

Every implementation-planning agent should read:

- [`00-roadmap-index.md`](00-roadmap-index.md)
- [`visual-acceptance-contract.md`](visual-acceptance-contract.md)
- [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md)
- the relevant versioned plan below
- the owning architecture and decision docs listed in that plan's frontmatter

Each agent deliverable should include:

- owned files and files that must not be touched;
- proposed tests, harness captures, and artifact JSON additions;
- exact config/layout migrations, if any;
- doc migration notes for architecture/decisions;
- explicit blockers that require Adam's visual review.

## Agent A - v0.3.0 Shape / Placement Detail Plan

Read:

- [`v0.3.0-brain-shaped-arena.md`](v0.3.0-brain-shaped-arena.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/connectivity.md`](../architecture/connectivity.md)
- [`../architecture/simulation.md`](../architecture/simulation.md)
- [`../decisions/manifold.md`](../decisions/manifold.md)

Owns:

- `crates/brain-visualizer/src/manifold/*`
- tests/harness changes needed to prove placement determinism, grid occupancy,
  and shape captures

Do not own:

- region reassignment;
- connectivity algorithm changes;
- mesh asset import;
- morphology branch grammar.

Detailed plan must answer:

- What exact host-side shape primitive makes both shell mesh and placement use
  the same brain envelope?
- How does the cortical-shell sampler avoid solid-volume glow and hollow-shell
  thinness?
- What deterministic hashes/salts are used for shell depth and sparse interior
  fill?
- What grid-occupancy and dynamics smoke checks prove the placement did not
  break target locality or default activity?
- What shell-only and neuron-cloud-only artifacts will be produced?

## Agent B - v0.3.1 Curved Morphology Detail Plan

Read:

- [`v0.3.1-curved-chain-morphsegments.md`](v0.3.1-curved-chain-morphsegments.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../decisions/manifold.md`](../decisions/manifold.md)
- [`../decisions/rendering.md`](../decisions/rendering.md)

Owns:

- `crates/brain-visualizer/src/sim/morphology.rs`
- morphology budget/cap tests
- `morph_view` stats additions if needed

Do not own:

- `MorphSegment` field layout changes;
- `render_morphology.wgsl` material or pulse semantics;
- retired ribbon/cylinder paths.

Detailed plan must answer:

- What `BranchCursor` / `GrowthState` helper will emit sampled curved chains?
- How are dendrite, shared axon, and terminal twig behaviors parameterized
  without exposing every knob?
- How is cumulative `path_len` verified along emitted chains?
- What is the expected segment count, cap usage, tube vertex count, and dropped
  segment behavior at default `N`/`K`?
- Which review frames prove close-up curvature without far-view hairballing?

## Agent C - v0.3.2 / v0.3.3 Shared Shader-Config Detail Plan

Read:

- [`v0.3.2-neuron-material-texture-pass.md`](v0.3.2-neuron-material-texture-pass.md)
- [`v0.3.3-soma-pulse-and-traveling-impulses.md`](v0.3.3-soma-pulse-and-traveling-impulses.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/manifold.md`](../architecture/manifold.md)
- [`../architecture/data-model.md`](../architecture/data-model.md)
- [`../decisions/rendering.md`](../decisions/rendering.md)
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)

Owns:

- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl`
- `MorphUniforms` / `MorphologyConfig` changes if needed
- shader/config preflight output

Do not own:

- `MorphSegment` layout changes;
- simulation dynamics or delayed synaptic delivery;
- public presets or public UI controls.

Detailed plan must answer before code starts:

- Can material and pulse ship with existing `MorphUniforms` fields?
- If not, what is the combined Rust/WGSL layout migration, including size
  asserts and padding?
- Which fields are live uniform writes, renderer-rebuild controls, or hidden
  preset defaults?
- What cheap deterministic material functions are used for soma and tube
  surfaces?
- What exact `last_spike` / `path_len` timing model drives soma pulse and
  traveling impulses?
- How is old whole-arbor instant lighting reduced without making pulses too
  faint?
- What shader-cost budget is acceptable after v0.3.1's segment count?
- What artifact set proves material polish and moving pulse both pass?

## Agent D - v0.3.4 Camera/Pan Detail Plan

Read:

- [`v0.3.4-right-click-pan-camera-target.md`](v0.3.4-right-click-pan-camera-target.md)
- [`../architecture/web-frontend.md`](../architecture/web-frontend.md)
- [`../decisions/interaction.md`](../decisions/interaction.md)

Owns:

- `web/src/render/camera.ts`
- `web/src/main.ts` input handling
- frontend tests/manual check notes for orbit, pan, zoom, stimulation

Do not own:

- picking/selection;
- backend or WASM API changes;
- public settings UI.

Detailed plan must answer:

- What camera target representation preserves current orbit semantics?
- How is right-drag pan distinguished from left-drag orbit and hover
  stimulation?
- How are context-menu suppression and Shift-left fallback handled?
- What minimal recenter/clamp behavior prevents losing the brain offscreen?

## Agent E - v0.4.0 Defaults / Hidden Presets Detail Plan

Read:

- [`v0.4.0-cohesive-visual-defaults.md`](v0.4.0-cohesive-visual-defaults.md)
- [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/profiling.md`](../architecture/profiling.md)
- [`../decisions/dev-tooling.md`](../decisions/dev-tooling.md)
- [`../decisions/rendering.md`](../decisions/rendering.md)

Owns:

- hidden dev-panel review preset workflow;
- accepted defaults manifest fill-in;
- localStorage/reset behavior for new visual and morphology settings;
- review harness artifact JSON updates.

Do not own:

- public preset manager;
- runtime auto-scaling;
- new product UI beyond the hidden dev panel.

Detailed plan must answer:

- Where do default/performance/hero preset payloads live?
- How does `accepted-default` stay identical to clean first-load defaults?
- How are preset ids written into artifact JSON?
- Which controls stay visible, move to advanced, or get tombstoned?
- How are old localStorage profiles prevented from corrupting accepted defaults?
- Which final settings move into architecture/decisions when shipped?

## Agent F - v0.5.0 Stabilization Detail Plan

Read:

- [`v0.5.0-showcase-stabilization.md`](v0.5.0-showcase-stabilization.md)
- [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md)
- [`visual-acceptance-contract.md`](visual-acceptance-contract.md)
- [`../architecture/build-and-deploy.md`](../architecture/build-and-deploy.md)
- [`../architecture/profiling.md`](../architecture/profiling.md)
- [`../agent-context/testing-how-to.md`](../agent-context/testing-how-to.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)

Owns:

- production build/preview verification;
- first-load and reset-to-default verification;
- final artifact ledger;
- docs/future-roadmap cleanup.

Do not own:

- new visual features;
- education mode;
- CPU backend revival;
- soft spatial regions unless a new plan explicitly reopens them.

Detailed plan must answer:

- What exact commands and browser checks prove production readiness?
- How does v0.5.0 verify the accepted defaults manifest without re-tuning?
- What final artifact set is recorded, and where?
- Which shipped plan context is migrated into architecture/decisions?
- Which plans can be marked `shipped + okay_to_delete: true`?

## See also

- [`00-roadmap-index.md`](00-roadmap-index.md)
- [`visual-acceptance-contract.md`](visual-acceptance-contract.md)
- [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md)
- [`future_roadmap.md`](future_roadmap.md)
