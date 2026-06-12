---
status:        shipped
owner:         unassigned
last_updated:  2026-06-12
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/data-model.md
  - architecture/gpu-backend.md
  - architecture/gpu-rendering.md
  - architecture/manifold.md
  - architecture/scaling.md
  - decisions/data-layout.md
  - decisions/manifold.md
  - decisions/rendering.md
  - decisions/scaling.md
---

# Stream D: Morphology Segment Scaling

## Mission

Remove the single-storage-buffer morphology segment ceiling so
`PRODUCT_MAX_N = 20_000` is not constrained by WebGPU's 128 MiB
`max_storage_buffer_binding_size`. Done means morphology segment storage,
active/recent compaction, tube drawing, and active-opacity tube drawing work
across segment chunks without CPU readback in the frame loop and without an
N-based dendrite-decoration throttle whose only purpose is avoiding the old
single-binding limit.

## Scope

In scope:

- Replace the flat `MorphBuffers.segment_buffer` branch segment binding with
  chunked segment resources using `ChunkLayout` or an equivalent shared
  primitive.
- Keep each segment chunk below adapter binding limits; 64 MiB chunk budget is
  the expected starting point unless code proves a better local policy.
- Split active/recent compaction and tube draws by segment chunk, with per-chunk
  segment buffer, active index buffer, selected counter, indirect draw args, and
  bind groups.
- After chunking is verified, remove or retarget `DECOR_FULL_N`,
  `DECOR_ZERO_N`, and `effective_decor_group_max` as binding-limit
  workarounds.

Out of scope:

- Moving generation off the main thread, CPU retirement, telemetry, settings
  schema redesign, region aesthetics, connectivity, spike dynamics, soma layout,
  or `MorphUniforms` changes unless a chunk uniform is required.

## Context Routes

- `docs/architecture/data-model.md`
- `docs/architecture/gpu-backend.md`
- `docs/architecture/gpu-rendering.md`
- `docs/architecture/manifold.md`
- `docs/architecture/scaling.md`
- `docs/decisions/scaling.md`
- `app/crates/brain-visualizer/src/buffers.rs`
- `app/crates/brain-visualizer/src/sim/gpu/resources.rs`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs`
- `app/crates/brain-visualizer/src/sim/gpu/pipelines.rs`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl`
- `app/crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `app/crates/brain-visualizer/src/sim/morphology.rs`

## Approach

1. Introduce a morphology segment chunk type containing one segment buffer,
   local segment count, active index buffer, active count, indirect draw args,
   compact uniform, and any selected-count profiler storage needed for that
   chunk.
2. Change `MorphBuffers` to hold `segment_chunks: Vec<_>` plus aggregate stats,
   while keeping soma spheres flat.
3. Prefer chunk-local indexing in compaction and render WGSL: scan local segment
   count, write local active indices, and render against the currently bound
   chunk.
4. Loop chunks in `render_full` for compaction, additive tube draw, and
   active-opacity tube draw. Preserve GPU-driven indirect draws and avoid
   per-frame buffer or bind group creation.
5. Only after chunk allocation and render smoke pass, replace tests/docs that
   assert high-N decoration goes to zero with tests/docs proving any remaining
   high-N reduction is explicit product policy, not a hidden storage-binding
   workaround.

## Exit Gate

- `cd app && cargo test`
- `cd app/web && npm run typecheck`
- `cd app/web && npm test`
- Host tests for chunk math above 128 MiB / 48 B, empty input, and last-partial
  chunks.
- Resource/layout assertion that every chunk binding is under the chosen budget
  and adapter limit.
- Shader/pipeline validation compiles the changed morphology WGSL and bind
  groups.
- `cd app && cargo run --example render_check` remains nonblank and exercises
  the compacted morphology path.
- A high-segment-count smoke proves allocation chooses multiple chunks without
  creating one oversized segment buffer.

## Handoff Notes

Land this before the worker-prepared payload serializes morphology buffers in
the rebuild-responsiveness plan. Keep one implementer on this plan at a time;
`resources.rs`, `mod.rs`, `pipelines.rs`, and morphology WGSL are high-conflict.

## Migration Notes

Migrated current-state facts into:

- `architecture/data-model.md`: shared chunk math now covers morphology segment storage as well as SoA fields.
- `architecture/gpu-backend.md`: `render_full` compacts/draws morphology per segment chunk; `init_morph_resources` allocates persistent chunk-local resources.
- `architecture/gpu-rendering.md`: active/recent compaction and both tube passes use chunk-local indices and chunk-local indirect args.
- `architecture/manifold.md`: `MorphSegment` stays a flat 48 B generator output; GPU upload chunking does not change the layout or generator contract.
- `architecture/scaling.md`: high-N storage pressure is handled by chunked segment bindings, not by neuron-count decoration throttling.

Migrated decisions/tradeoffs into:

- `decisions/data-layout.md`: large storage bindings use byte-budgeted `ChunkLayout`; morphology binds one segment chunk at a time.
- `decisions/rendering.md`: active/recent compaction and no-readback indirect draws are per chunk.
- `decisions/scaling.md`: chunk segment storage rather than throttling decoration to fit one binding.
- `decisions/manifold.md`: bushy decoration remains self-owned and per-neuron capped, not N-throttled.

Verification migrated/recorded: host chunk tests cover >128 MiB / 48 B layouts, empty input, last partial chunks, adapter-limit budgeting, and product-scale multi-chunk selection; `render_check` validates pipeline creation and compacted morphology rendering on llvmpipe.
