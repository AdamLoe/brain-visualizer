---
status:        shipped
owner:         unassigned
last_updated:  2026-06-17
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - architecture/manifold.md
  - decisions/rendering.md
---

# Active morphology solid opacity

## Outcome

Firing/active soma, active trunks, and active synaptic morphology should read as
solid visible geometry rather than see-through glow. The solidity applies only
to firing/active geometry. Resting or inactive morphology should keep the
current low-emphasis/additive behavior unless the organic morphology stream
changes its baseline representation.

## Scope

This stream owns active-only opacity, depth, and compositing behavior:

- `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `crates/brain-visualizer/src/sim/gpu/pipelines.rs` for active pipeline state
- `crates/brain-visualizer/src/sim/gpu/resources.rs` only if opacity uniforms or
  morphology layout consumption changes
- focused render smoke coverage and artifact review

In scope:

- Make firing/active soma and active branch/trunk/synapse geometry genuinely
  occluding or visually solid.
- Preserve the active packet/activity semantics: active means spike/firing
  activity, not click selection.
- Keep resting/inactive geometry from becoming a dense opaque forest.
- Align with any new morphology data contract from
  `organic-morphology-geometry.md`.

Out of scope:

- Do not make all resting morphology solid.
- Do not add picking, selected-neuron state, or inspection UI.
- Do not revive deleted ribbon/cylinder connection renderers.
- Do not change simulation dynamics or spike timing to make opacity easier.
- Do not add public controls unless the existing hidden morph lighting controls
  are insufficient for review.

## Context routes

Load these first:

- `docs/architecture/gpu-rendering.md`
- `docs/architecture/gpu-backend.md`
- `docs/architecture/manifold.md`
- `docs/decisions/rendering.md`
- `docs/architecture/build-and-deploy.md` for render smoke commands

Relevant code anchors:

- `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full,
  VisualSettings`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
  fs_main_active / fs_sphere_active`
- `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl`
- `crates/brain-visualizer/src/sim/gpu/pipelines.rs →
  build_morph_pipelines, build_morph_active_pipelines`
- `crates/brain-visualizer/src/sim/morphology.rs → LightingConfig,
  MorphSegment, MorphSphereInstance`
- `crates/brain-visualizer/examples/render_check.rs`
- `crates/brain-visualizer/examples/morph_view.rs`

## Open assumptions

- Active solidity is a visual rendering rule, not a new simulation state.
- Only firing/active geometry should become solid.
- The organic morphology stream may change branch primitive/layout details, so
  this stream should avoid locking in assumptions about the old 48 B
  `MorphSegment` contract unless it runs first as a short-lived patch.
- If the active layer currently compiles lazily, first-frame fallback to the old
  additive look is acceptable only if it is not visible in normal startup review.

## Acceptance / verification

The handoff should include:

- `cargo test` for any Rust/WGSL layout, uniform, or render-contract changes.
- `cargo run -p brain-visualizer --example render_check` passing with active
  opacity coverage intact.
- `cargo run -p brain-visualizer --example morph_view` artifacts showing active
  soma/trunk/synapse geometry reading as solid rather than transparent.
- Review evidence that inactive/resting morphology does not become an opaque
  hairball.
- Review evidence that active packet motion remains visible after the solidity
  change.

If browser WebGPU is available, include a manual or Playwright smoke with
stimulation/activity visible in the live canvas. If the environment has no
WebGPU adapter, record that blocker and rely on native render artifacts.

## Handoff notes

Prefer to implement this after `organic-morphology-geometry.md` if that stream
changes the primitive or layout contract. If product urgency demands opacity
first, keep it deliberately narrow and expect a rebase when the generator
rework lands.

The main review risk is mistaking brightness for solidity. Additive brightness
alone is not enough; active geometry needs real occlusion or an equivalent
solid-looking compositing model that survives close-up screenshots.

## Migration notes (filled in at ship time)

Shipped durable context is captured in:

- `architecture/gpu-rendering.md` for active pass ordering, depth/alpha policy,
  shader behavior, and active-only meaning.
- `architecture/gpu-backend.md` for the frame graph order and deferred active
  pipeline build behavior.
- `decisions/rendering.md` for active-only true opacity, the firing-not-selection
  rule, the `LightingConfig` opacity knobs, and rejected additive-brightness /
  all-resting-opacity alternatives.

This implementation batch kept that active-opacity contract intact while moving
the morphology tube primitive to curved multi-ring rendering.
