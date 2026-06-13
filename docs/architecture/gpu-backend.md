---
status:        active
owner:         adamg
last_updated:  2026-06-13
---

# GPU backend / frame graph

The clock-driven WebGPU compute backend. It owns the *order* in which compute
and render passes are encoded each tick/frame, the lifecycle of the persistent
GPU buffer set, and the non-blocking staging-readback state machine. It is the
only live sim backend (see
[`../decisions/backends.md`](../decisions/backends.md)).

The one job: drive every neuron forward one tick entirely on the GPU with **no
CPU readback in the loop and no per-frame allocation**, and hand the resulting
GPU-resident state straight to the render passes.

## What it owns

- `GpuBackend` — the backend + frame graph driver — `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend`.
- The per-tick compute pass *ordering* in `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::tick`
  (the `SimBackend::tick` impl).
- The per-frame render pass *ordering* in `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::render_full`
  (`render` is the thin far-only wrapper).
- The async metrics readback state machine — `crates/brain-visualizer/src/sim/gpu/mod.rs → MetricsReadState`,
  `GpuBackend::update_metrics`.
- The persistent resource set + dirty/rebuild bookkeeping —
  `crates/brain-visualizer/src/sim/gpu/resources.rs → GpuResources` (`bind_groups_dirty`,
  `refresh_bind_groups`, `resize_render_targets`, the `init_*` / `resize_neurons`
  allocators, `destroy`).
- Pipeline + bind-group-layout construction — `crates/brain-visualizer/src/sim/gpu/pipelines.rs →
  GpuPipelines` (`build`, `build_render`, `build_near_lod`), `GpuLayouts`.
- Device/queue acquisition — `GpuBackend::acquire_web` / `acquire_native`,
  `GpuContext`.
- Browser startup staging — `crates/brain-visualizer/src/lib.rs →
  WasmGpuBackend::create_staged` and the `startup_*` methods drive
  `GpuBackend::begin_initialize`, the staged resource upload helpers, and
  `finish_initialize`.
- Worker-prepared network ingestion — `crates/brain-visualizer/src/sim/gpu/mod.rs →
  PreparedNetworkBuild` validates/reconstructs flat CPU payloads, and
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend::startup_begin_prepared_network`
  / `apply_prepared_network` enter the same main-thread WebGPU upload/resource
  path as direct builds.
- The standing guardrail constants `DRAW_LEGACY_CYLINDERS`,
  `DRAW_LEGACY_NEAR_SPHERES`, `DRAW_LEGACY_RIBBONS` that gate retired passes out
  of the graph, and `crates/brain-visualizer/src/sim/gpu/pipelines.rs →
  DRAW_LEGACY_ALL_SEGMENTS` that bypasses morphology compaction.

## What it does NOT own

- The LIF/dynamics math inside `integrate` / `scatter` (leak, threshold, weight
  norm, heterogeneity, the spatial `target_neuron` rule) — [`simulation.md`](simulation.md).
- The visual logic of each render pass (billboards, manifold, morphology, bloom)
  — [`gpu-rendering.md`](gpu-rendering.md).
- The active-edge / ribbon emit+ring internals — [`active-edges.md`](active-edges.md).
- What the metrics slots *mean* and how JS parses them — [`profiling.md`](profiling.md).
- Buffer field layouts and SoA neuron storage shapes — [`data-model.md`](data-model.md).

This doc owns **where** in the frame graph each of those passes is encoded; the
sibling docs own what each pass does internally.

## Per-tick compute frame graph

`tick(ticks, excitability)` records one command encoder for the whole batch and
submits it once. Per tick, in fixed order (`crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::tick`):

1. **apply pending config** — `stim_pending` is `take`n once before the loop; the
   stimulate dispatch runs only on `tick_idx == 0` (`stimulate.wgsl → stimulate`,
   1 workgroup). It writes `i_current` for the *same* parity integrate will read.
2. **uniforms** — the integrate uniform is rewritten each tick (cheap; carries the
   tick counter and live knobs); `spike_count` is zeroed via `write_buffer`.
3. **integrate** — `integrate.wgsl → integrate`, `dispatch_workgroups(ceil(n/256))`.
   Threshold-crossers append themselves to `spike_list` via `atomicAdd(spike_count)`.
4. **write_scatter_dispatch** — `write_scatter_dispatch.wgsl → main`, 1 workgroup.
   Reads `spike_count` *on the GPU*, computes `ceil(spike_count·K / 64)` into
   `dispatch_args`. This is the count→scan→scatter seam.
5. **scatter** — `scatter.wgsl → scatter` via
   `dispatch_workgroups_indirect(dispatch_args, 0)`. Each thread routes one
   spike·synapse to a target cell and `atomicAdd`s fixed-point current into
   `I_next`.
6. **(optional) emit_edges** — `emit_edges.wgsl`, gated behind `do_emit`
   (`DRAW_LEGACY_RIBBONS && connection_layer != 0`). Default-off ⇒ skipped
   entirely. See [`active-edges.md`](active-edges.md).
7. **swap** — flip `self.parity ^= 1` and `self.tick = self.tick.wrapping_add(1)`.

After the loop (once per batch), stats are staged and the metrics state machine
is driven (below).

### THE load-bearing invariant: GPU-driven indirect scatter

`spike_count` is produced by integrate and consumed by `write_scatter_dispatch`
and `scatter` **without ever crossing to the CPU**. The CPU must NEVER map
`spike_count` to size the next pass — the indirect `dispatch_args` buffer is the
only thing that sizes scatter. Mapping it would force a sync stall and break the
no-readback policy. Pass boundaries (separate compute passes) provide the
ordering guarantee integrate→write→scatter rely on; there is no explicit barrier.

Workgroup sizes are pinned per pass: integrate/metrics 256, scatter 64,
write_scatter_dispatch / stimulate 1. The `div_ceil(.., 256)` group count for
integrate and the `/64` divisor in `write_scatter_dispatch.wgsl` must stay in
lockstep with those.

## Per-frame render frame graph

`render_full` records and submits one render encoder (`crates/brain-visualizer/src/sim/gpu/mod.rs →
GpuBackend::render_full`). Every pass reads GPU-resident state directly; no
upload of per-instance data. Order:

1. **bloom routing decision** — when `visual.bloom_strength <= 0` (default),
   `scene_view` *is* the surface `target_view` (validated direct path). Only when
   bloom is on AND all bloom pipelines/targets exist does the scene render into
   the offscreen HDR target.
2. **(optional) manifold surface pass** — gated by `visual.surface != 0`; clears
   color+depth so later passes load on top.
3. **far-LOD glow pass** — clears color (unless the surface pass already did),
   additive, no depth.
4. **active/recent compaction compute + morphology tube pass** — when `connection_layer != 0`. For each morphology segment chunk, the compaction compute (`compact_morph_segments.wgsl`: `reset` 1wg → `compact` ⌈chunk_segs/64⌉wg → `write_args` 1wg) writes that chunk's `active_draw_args`; the tube pass then binds each chunk and `draw_indirect`s over its compacted active/recent subset (additive, no depth) via `render_morphology.wgsl → vs_main`. Instance counts are GPU-decided per chunk — no CPU readback sizes these draws. See [`gpu-rendering.md`](gpu-rendering.md) for the selection predicate.
5. **morphology soma sphere pass** — when `connection_layer != 0`; additive, no depth. One UV-sphere per neuron via `render_morphology.wgsl → vs_sphere`. Uses `render_soma_spheres` pipeline (`crates/brain-visualizer/src/sim/gpu/pipelines.rs → GpuPipelines`), reusing the same `last_spike` and `morph_uniform` buffers from the tube pass.
6. **active-opacity tube + soma passes** — when `connection_layer != 0` and the active-opacity guard is on; depth-tested **alpha** blend (not additive), layered over the additive morphology passes so firing geometry genuinely occludes. The active-tube pass owns the depth `Clear(1.0)`; the active-soma pass `Load`s it. See [`gpu-rendering.md`](gpu-rendering.md) for the opacity model and skip-at-zero guard.
7. **(retired) ribbon pass** — behind `DRAW_LEGACY_RIBBONS`.
8. **(retired) near-LOD passes** — cull_neurons → (cull_synapses) → write_indirect
   → sphere draw → (cylinder draw), all behind `DRAW_LEGACY_NEAR_SPHERES`.
9. **(optional) bloom post** — bright → blur_h → blur_v → composite into the
   surface.

Per-frame uniform uploads use `queue.write_buffer` into buffers the bind groups
already reference, so no bind-group rebuild is needed for a uniform change. See
[`gpu-rendering.md`](gpu-rendering.md) for each pass's visual semantics.

### Retired-pass guardrails

Three passes are kept in code but gated OUT of the live graph so they never
double-draw connections or re-introduce old visual bugs:

| Const | Default | Gates out |
|---|---|---|
| `DRAW_LEGACY_CYLINDERS` | `false` | near-LOD straight-cylinder synapses (cull_synapses + cylinder draw) |
| `DRAW_LEGACY_NEAR_SPHERES` | `false` | the faceted near-LOD icosphere body + the whole near-LOD branch (`run_near_lod`) |
| `DRAW_LEGACY_RIBBONS` | `false` | the Phase-D ribbon emit (`do_emit`) + ribbon render pass |

The morphology pass is the one connection renderer; billboards are the body
visual at all distances. Flip a const to `true` only to debug the old geometry.

## Resource lifecycle

All large buffers are **persistent across frames**. Allocation happens only on a
*structural* change, never in the rAF loop:

- **tier resize / network rebuild** — `GpuBackend::initialize` (called by
  `resize`) runs the `GpuResources::resize_neurons` + `init_render_resources` +
  `init_near_lod_resources` + `init_edge_resources` + `init_morph_resources`
  allocators, then `refresh_bind_groups`. `init_morph_resources` splits the
  generated branch segments into `MorphSegmentChunk` resources using
  `morph_segment_chunk_layout` (64 MiB default budget, further capped by the
  adapter's storage-binding limit), allocates chunk-local compaction buffers
  (`active_segment_indices`, `active_segment_count`, `active_draw_args`,
  `compact_uniform`, plus profiler `active_selected` / `selected_staging`), and
  allocates the flat soma sphere instance buffer (`sphere_instances` via
  `crates/brain-visualizer/src/sim/morphology.rs → emit_soma_spheres`). The tube
  and compaction bind groups are one pair per segment chunk; the sphere bind
  group stays flat and shares the 192 B `MorphUniforms` buffer
  (`crates/brain-visualizer/src/sim/gpu/resources.rs → MorphBuffers / MorphUniforms`).
- **render-target resize** — `resize_render_targets`, guarded: it recreates the
  depth + bloom textures **only when width/height actually changed** (the
  `changed` check in `crates/brain-visualizer/src/sim/gpu/resources.rs → GpuResources::resize_render_targets`).
- **backend restart / device-loss** — re-acquire context, rebuild pipelines,
  re-`initialize`.
- **direct curve-lift / generator fallback** — `regenerate_morphology` rebuilds
  only the morph buffers + refreshes bind groups. Browser UI routes structural
  curve/reach/generator changes through worker-prepared payloads first; this
  direct path remains for internal/native callers.
- **worker-prepared network rebuild** — `PreparedNetworkBuild` carries the CPU
  manifold, placement, spatial grid, morphology segments, soma instances, and
  metadata produced away from the main thread. The payload is GPU-agnostic and
  flat; `GpuBackend::initialize_prepared` still runs `resize_neurons`,
  `init_render_resources`, `init_near_lod_resources`, `init_edge_resources`,
  `GpuResources::init_morph_resources_from_prepared`, and `finish_initialize`
  on the main thread. Segment chunking remains an upload/resource policy inside
  `GpuResources`; the worker never encodes chunk layout or creates WebGPU
  resources.

### Browser startup staging

The browser does not call the monolithic `WasmGpuBackend::create()` during normal
boot. `web/src/main.ts → startGpuBackend` requests a worker-prepared payload,
uses `WasmGpuBackend::create_staged()` to acquire WebGPU and construct the core
compute backend, then calls explicit startup stages with a browser frame yield
between each call:

1. `startup_begin_prepared_network` → validates the flat worker payload,
   reconstructs `PreparedNetworkBuild`, stores it as `NetworkBuildState`, and
   applies the prepared visual/morph config without running generator work.
2. `startup_upload_neuron_buffers` → `GpuResources::resize_neurons`.
3. `startup_upload_render_resources` → `GpuResources::init_render_resources`.
4. `startup_allocate_lod_edge_resources` →
   `init_near_lod_resources` + `init_edge_resources`.
5. `startup_upload_morphology` → `GpuResources::init_morph_resources_from_prepared`.
6. `startup_finish_network` → `refresh_bind_groups`, `write_connect_uniform`,
   and the per-network runtime-state reset.
7. `startup_build_render_pipelines` → `GpuBackend::build_render_pipelines`.
8. `startup_resize_render_targets` → `GpuBackend::resize_render_targets`.

This staging does **not** move WebGPU ownership off the main thread and does not
make individual Rust stages preemptible. It lets the DOM loading overlay paint
and report measured per-stage timings between structural allocation blocks. The
same upload helpers are now also the boundary used by worker-prepared network
payloads after startup. The rAF loop must not receive the staged
`WasmGpuBackend` until all startup stages complete.

### bind_groups_dirty rebuild rule

Any buffer recreation that feeds a bind group **must** mark
`GpuResources::bind_groups_dirty = true` (every `init_*` / `resize_*` does). The
frame loop calls `GpuBackend::ensure_bind_groups` at the top of `tick`, which
calls `refresh_bind_groups` only when dirty, then clears the flag. The rule:
**recreate a buffer → set the dirty flag → the next tick rebuilds every dependent
bind group.** `refresh_bind_groups` rebuilds *all* bind groups (it early-outs and
clears the flag if the core neuron/grid/sim buffers don't exist yet), so a stale
bind group pointing at a freed buffer can never reach a dispatch.

## Async non-blocking readback state machine

Shared shape for the two instrumentation readbacks (metrics + edge_emitted).
Driven by `GpuBackend::update_metrics`, modelled in
[`profiling.md`](profiling.md):

- **Idle** — safe to issue: zero `metrics_buf`, dispatch the read-only
  `metrics.wgsl → reduce_metrics` pass, `copy_buffer_to_buffer` → `metrics_staging`,
  submit, `map_async` (the callback flips a shared `Arc<AtomicBool>`), → **Pending**.
- **Pending** — a map is in flight and `metrics_staging` is mapped/locked. **NEVER
  `copy_buffer_to_buffer` into staging while Pending** — that is the corruption bug
  the code comments warn about. When the `AtomicBool` resolves, copy the mapped
  slots into `metrics_cpu`, `unmap`, → **Idle**.

`update_metrics` always calls `device.poll(PollType::Poll)` (non-blocking):
progresses the map on native, harmless no-op on wasm (the browser progresses it
between frames). Issuance is throttled by `METRICS_ISSUE_INTERVAL` ticks so the
COPY+MAP round-trip is not per-tick.

### wasm vs native readback asymmetry

On wasm/WebGPU `device.poll(Wait)` is a documented no-op, so a *blocking* map can
never resolve and would leave staging permanently mapped — poisoning the next
encoder. Therefore the **blocking** readbacks (`read_stats`, `read_u32`,
`read_near_lod_stats`, the per-batch `stats_staging` / `edge_emitted` copies) are
all `#[cfg(not(target_arch = "wasm32"))]` and return 0 on wasm. The
metrics/edge_emitted *non-blocking* state machine works identically on both.
`debug_dynamics_snapshot` / `readback` are explicit stalls for crates/brain-visualizer/tests/debug only —
never on the hot path.

## Standing guardrails

- **No CPU readback in the rAF loop.** Indirect dispatch sizes scatter;
  the morphology tube passes size their `draw_indirect` from GPU-written
  `active_draw_args` (compaction selection never crosses to the CPU); metrics
  use the non-blocking state machine; blocking readbacks (including the
  morphology selected-segment count) are native-only instrumentation off the
  rAF path.
- **No per-frame buffer / bind-group / pipeline / texture creation.** Everything
  is allocated on structural change only. (The bloom pass builds 4 tiny per-frame
  bind groups when bloom is *on* — the one documented exception, cheap and opt-in.)

See [`../decisions/backends.md`](../decisions/backends.md) for why GPU-only /
clock-driven.

## Update when

- A compute pass is added/removed/reordered in `tick` (update the frame-graph
  list + workgroup-size note).
- A render pass is added/removed/reordered in `render_full` (update the render
  order list), including the morphology compaction compute that precedes the
  tube passes or its `draw_indirect` wiring.
- A new persistent resource set or `init_*` allocator is added (update the
  lifecycle + dirty-flag section).
- The readback state machine gains a state or a new staged buffer.
- A `DRAW_LEGACY_*` guard is added, removed, or flipped to default-on.
- The indirect-scatter contract changes (e.g. spike_count consumed differently).

## See also

- [`simulation.md`](simulation.md) — LIF/dynamics math inside integrate/scatter
- [`gpu-rendering.md`](gpu-rendering.md) — visual logic of the render passes
- [`active-edges.md`](active-edges.md) — edge/ribbon emit + ring internals
- [`profiling.md`](profiling.md) — metrics meaning + parseMetrics
- [`data-model.md`](data-model.md) — buffer/field layouts
- [`../decisions/backends.md`](../decisions/backends.md) — GPU-only / clock-driven rationale
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
