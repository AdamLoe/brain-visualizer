---
status:        draft
owner:         unassigned
last_updated:  2026-06-17
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
  - decisions/rendering.md
---

# Organic morphology geometry

## Outcome

The morphology visual should read as biologically inspired neuron branching
rather than angular generated linework. A viewer should see a smooth,
root-like branching structure growing from each active cell: recognizable as
neuronal soma, dendrite, and axon structure for an educational brain-firing
visualization, while still stylized enough to stay readable and performant.

The implementer may change the Rust/WGSL morphology layout contracts if that is
the cleanest way to achieve the visual target. Backward compatibility with old
morphology buffers or saved morphology payloads is not required; old git history
is available if a previous approach is worth mining.

## Scope

This stream owns the morphology generator and any data contract changes needed
to make branch structure smooth and visually mature:

- `crates/brain-visualizer/src/sim/morphology.rs`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl`
- morphology resource/pipeline code touched by a layout or draw-contract change
- focused morphology generator tests and `morph_view` review artifacts
- hidden morphology controls only if existing controls become misleading

In scope:

- Replace or substantially reshape the current straight-subsegment / Prim-like
  tree visual grammar if it is still producing basic angular branches.
- Permit a new segment representation, curve/control metadata, richer junction
  information, or a new generated primitive contract when justified by visual
  quality.
- Preserve deterministic generation for same seed/config.
- Preserve honest activity ownership: real outgoing axon paths and real
  incoming terminal ownership should remain connected to simulation facts.
- Keep the result asset-free and procedural.

Out of scope:

- No external mesh/texture asset pipeline.
- No anatomical atlas, exact cell-type reconstruction, or neuroscience-grade
  morphology model.
- No support for old 48 B `MorphSegment` compatibility if a better contract is
  chosen.
- No click selection, picking, labels, or education overlay.
- No change to simulation dynamics or synaptic delivery.
- No requirement to make resting morphology solid; that belongs to the active
  opacity stream and only applies to firing/active geometry.

## Context routes

Load these first:

- `docs/architecture/manifold.md`
- `docs/architecture/gpu-rendering.md`
- `docs/decisions/manifold.md`
- `docs/decisions/rendering.md`
- `docs/architecture/profiling.md` for `morph_view` artifact expectations
- `docs/architecture/dev-panel.md` only if morphology controls change

Relevant code anchors:

- `crates/brain-visualizer/src/sim/morphology.rs → generate,
  generate_with_progress, MorphologyParams, MorphologyConfig, MorphologyStats,
  MorphSegment, MorphSphereInstance, ProcessRoot, build_incoming_view,
  emit_incoming_dendrites, adaptive_subsegments, long_range_waypoints,
  segment_cap, max_segs_per_neuron`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl`
- `crates/brain-visualizer/src/sim/gpu/resources.rs → MorphUniforms,
  MorphSegmentChunk, MorphBuffers`
- `crates/brain-visualizer/examples/morph_view.rs`
- `crates/brain-visualizer/examples/render_check.rs`

## Open assumptions

- The visual target is biologically inspired and professor-demo credible, not a
  literal reconstruction of real neuron morphology.
- Smooth, visually appealing branch-root structure is more important than
  preserving the current segment representation.
- Old morphology buffer compatibility is disposable.
- Product scale remains beauty-first; do not solve high-N graceful degradation
  unless the new generator creates a new blocker at default scale.

## Acceptance / verification

The handoff should include visual and mechanical evidence:

- `cargo test` passes any changed Rust/WGSL layout assertions and focused
  morphology generator tests.
- `cargo run -p brain-visualizer --example morph_view` produces review artifacts
  showing smooth, organic branching from multiple camera distances.
- `cargo run -p brain-visualizer --example render_check` remains green.
- `MorphologyStats` shows no unexpected dropped segments at the accepted default
  config.
- A reviewer can inspect the produced images and confirm branches no longer read
  as basic angular polylines or obvious hand-tuned fans.
- Active packet motion still follows the visible active morphology paths.

If the implementation changes `MorphSegment`, `MorphSphereInstance`, or
`MorphUniforms`, update both Rust and WGSL atomically and refresh the layout
tests/docs listed in the owning docs.

## Handoff notes

This stream should run before the active-solid opacity stream when it changes
the morphology data contract. The opacity work depends on whatever branch
primitive, activity owner, packet path, and draw ordering this stream leaves
behind.

The main review risk is accepting a technically smoother version that still
looks like a generated angular graph. Visual review artifacts are not optional;
they are the acceptance surface.

## Migration notes (filled in at ship time)

Before marking this plan shipped, migrate durable facts into:

- `architecture/manifold.md` for the generator/data contract.
- `architecture/gpu-rendering.md` for render consumption of the new morphology
  primitive.
- `decisions/manifold.md` for the chosen branch grammar and rejected
  alternatives.
- `decisions/rendering.md` if shader primitives, packet path rendering, or
  morphology material choices change.
