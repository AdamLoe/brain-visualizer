---
status:        active
owner:         adamg
last_updated:  2026-06-17
---

# Cortical Manifold

The manifold subsystem builds the static brain geometry used by simulation and
rendering: a folded cortical surface, shell-biased neuron placement, per-neuron
region assignment, the spatial grid, and host-generated morphology geometry.
Generation is CPU work. Browser startup and structural rebuilds prepare the
payload in the module worker where feasible; WebGPU upload remains on the main
thread. The uploaded buffers are static until the network is rebuilt.

## What It Owns

- Manifold generation and placement —
  `crates/brain-visualizer/src/manifold/mod.rs → Manifold, ManifoldParams, Manifold::generate, Manifold::generate_with_progress, brain_outer_radius, brain_surface_point, folded_outer_radius, place_neurons, DEFAULT_GRID_DIM, ANTERIOR_POSTERIOR_AXIS`
- Icosphere mesh generation —
  `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`
- Structured fold field —
  `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, FoldField, gyrify, gyrify_with_field`
- Region assignment —
  `crates/brain-visualizer/src/manifold/regions.rs → RegionKind, RegionAssignmentMode, assign_regions, assign_regions_with_mode`
- Prepared-network payload validation —
  `crates/brain-visualizer/src/sim/gpu/mod.rs → PreparedNetworkBuild::prepare, PreparedNetworkBuild::from_flat_payload`
- Morphology generation and contracts —
  `crates/brain-visualizer/src/sim/morphology.rs → Morphology, MorphologyParams, MorphologyConfig, GeneratorConfig::apply_to, MorphologyStats, MorphSegment, MorphSphereInstance, ProcessRoot, generate, generate_with_progress, emit_soma_spheres, build_incoming_view, emit_incoming_dendrites, adaptive_subsegments, long_range_waypoints, effective_decor_group_max, segment_cap, max_segs_per_neuron`
- Manifold and morphology render shaders as consumers of those buffers —
  `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`,
  `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`

It does not own the packed type-byte layout, ambient input-region drive, render
pass ordering, or GPU bind-group setup; those live in
[`data-model.md`](data-model.md), [`simulation.md`](simulation.md),
[`gpu-rendering.md`](gpu-rendering.md), and [`gpu-backend.md`](gpu-backend.md).

## Surface And Placement Pipeline

`Manifold::generate` and `Manifold::generate_with_progress` build the surface,
fold field, neuron positions, region labels, and spatial grid from deterministic
inputs. The progress variant reports coarse internal stage boundaries so the
worker-prepared boot overlay can advance during CPU generation.

The surface starts as an icosphere, then `brain_outer_radius` and
`brain_surface_point` turn each direction into a deterministic brain envelope.
The envelope is shared by the visible mesh and the placement sampler, so surface
shape and neuron cloud cannot drift apart. `FoldField` adds structured sulci and
gyri; `gyrify_with_field` applies that same field to mesh vertices while
`place_neurons` uses it to bias samples into the folded cortical shell with a
small deterministic interior fill.

Region assignment defaults to `RegionAssignmentMode::HashRandom`: neuron indices
are deterministically shuffled and sliced into the input/association/output
split. The anterior-posterior prototype mode is opt-in through the hidden
dev-panel network control; it ranks neurons along `ANTERIOR_POSTERIOR_AXIS` with
deterministic jitter, but is not the product default. The split and default-mode
contract are gated by `cargo test` via
`region_split_approx_30_40_30`, `default_region_assignment_mode_is_hash_random`,
and `prototype_region_assignment_mode_is_opt_in`.

The spatial grid is a uniform integer grid over generated positions. Its
dimension is owned by `DEFAULT_GRID_DIM`; consumers should point to that symbol
instead of copying the cell count.

Worker-prepared startup/rebuilds cross JS/WASM as explicit flat arrays for
positions, region codes, surface mesh, morphology fields, soma fields, and the
spatial-grid CSR. `PreparedNetworkBuild::from_flat_payload` validates metadata,
counts, region-code range, face indices, CSR monotonicity, and one grid entry per
neuron before replacing GPU resources.

## Region Encoding Invariant

`RegionKind` is host-only until upload. `region_code` and `neuron_type_byte` in
`crates/brain-visualizer/src/sim/backend.rs` pack it into the neuron type byte,
and `integrate.wgsl` treats the input-region code as the ambient-drive selector.
Do not reorder or reinterpret `RegionKind` without updating those symbols and
the shader together. The full byte layout is owned by
[`data-model.md`](data-model.md).

## Neuron Morphology Geometry

`generate` builds a flat branch segment list plus one soma instance per neuron.
`generate_with_progress` wraps the same generator with throttled progress
callbacks and `MorphologyTimings` for boot observability. `MorphologyConfig`
layers dev-panel generator, render-quality, and lighting knobs over
`MorphologyParams::locked_default`; generator changes rebuild morphology instead
of flowing through the `VisualSettings` Float32Array.

Incoming dendrites are generated from the real reverse view of production
`target_with_cell`, not from decorative hashes. Raw non-self incoming synapses
are grouped into deterministic socket groups for visible dendrite geometry.
Target-owned internal dendrite roots/forks stay self-lit; real presynaptic
terminal leaves carry the source id as their activity owner. Bushy decorative
branchlets and twigs are explicitly self-owned and bounded by
`effective_decor_group_max`, so they cannot invent presynaptic activity.

Each source neuron also gets a `ProcessRoot` descriptor and a single axon arbor.
The protected soma-root to first-fork trunk is emitted before the tree grows.
The remaining tree is built by a deterministic Prim-like greedy attach loop with
local relaxation; leaves are unique non-self targets. Internal trunk/fork edges
carry the source id so shared paths stay source-lit, and only terminal leaf edges
carry the real target id. Width is computed bottom-up from synaptic weight using
the area-preserving rule described in
[`../decisions/manifold.md`](../decisions/manifold.md).

`adaptive_subsegments` controls deterministic CPU path sampling from edge
length, curvature, and long-range classification. The renderer then bends each
uploaded segment into a short multi-ring tube in-shader, so smoothness comes from
both host-side path sampling and render-side curved tube tessellation without
widening `MorphSegment`. `long_range_waypoints` routes visually long leaf edges
through deterministic bowed waypoints that are clamped to generation-time brain
bounds. Morphology does not read a per-synapse long-range flag; connectivity
exposes the chosen target id, and routing remains a visual concern.

`MorphSegment` is the branch-only Rust/WGSL layout contract, and
`MorphSphereInstance` is the soma-only layout contract. `MorphSphereInstance`
includes `root_dir` / `root_pull`, consumed by
`crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl →
vs_sphere` for axon-root soma pull. Their literal byte sizes are gated by
`cargo test` through `segment_layout_is_48_bytes` and
`sphere_instance_layout_is_48_bytes`; do not copy the field order here. GPU
upload chunks branch storage when needed via the rendering resource layer, but
chunking does not change the generator's flat output or these layout contracts.

`MorphologyStats` is the review/debug surface for generated facts: incoming
profile, segment cap/utilization, dropped counts, fork-degree/depth/radius
signals, unique-target coverage, and phase timings. Use it for artifacts instead
of scraping logs or duplicating formulas in docs.

## Exposed Vs Protected Parameters

`MorphologyConfig` exposes bounded generator shape controls through
`web/src/core/morph-config.ts → MORPH_DESCRIPTORS` and the hidden dev panel.
The WASM entry point
`crates/brain-visualizer/src/lib.rs → WasmGpuBackend::set_morphology_config`
passes the config to
`crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::set_morphology_config`.

Allocation budgets, waypoint/routing thresholds, and `salt::*` constants are
protected. `GeneratorConfig::apply_to` re-locks them at apply time so the UI
cannot move buffer-cap policy, hidden determinism namespaces, or seed
reproducibility. Removed morphology-control payload fields are normalized by the
web/Rust config loaders rather than revived as settings.

## Rendering Consumers

`render_manifold.wgsl` draws the optional folded surface before additive neuron
glow. `render_morphology.wgsl` consumes branch chunks and soma instances through
the render pipelines owned by [`gpu-rendering.md`](gpu-rendering.md). The tube
pass draws compacted active/recent branch instances as shader-built curved
multi-ring tubes; the soma pass draws one shader-built sphere per neuron. Both
use `last_spike`, path length, material noise, and `MorphUniforms` lighting
values, but pass order, blend/depth policy, compaction, bloom, active-opacity, and
the until-arrival visibility modes (including the `reveal_on_arrival` front-gate)
are owned by the rendering docs.

## Update When

- Surface envelope, fold-field, placement, region assignment, or spatial-grid
  generation changes.
- `PreparedNetworkBuild` changes the flat payload validation or reconstructed
  manifold/morphology facts.
- `MorphologyParams`, `MorphologyConfig`, exposed/protected config boundaries,
  or morphology progress/timing surfaces change.
- Incoming dendrite ownership, axon tree generation, width policy, adaptive
  subdivision, waypoint routing, decoration ownership, segment cap policy, or
  `MorphologyStats` changes.
- `MorphSegment` or `MorphSphereInstance` layout changes; cross-check Rust,
  WGSL, and the `cargo test` layout assertions.
- `surface` or `connection_layer` setting semantics change in a way that affects
  how generated manifold/morphology data is consumed.

## See Also

- [`../decisions/manifold.md`](../decisions/manifold.md)
- [`data-model.md`](data-model.md) — type-byte packing and `last_spike`
- [`simulation.md`](simulation.md) — input-region drive and dynamics
- [`gpu-rendering.md`](gpu-rendering.md) — render passes, compaction, opacity
- [`gpu-backend.md`](gpu-backend.md) — resource upload and frame graph
- [`dev-panel.md`](dev-panel.md) — hidden morphology controls and persistence
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
