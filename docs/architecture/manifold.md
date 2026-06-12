---
status:        active
owner:         adamg
last_updated:  2026-06-12
---

# Cortical Manifold

Procedural generation of a brain-shaped surface, placement of neurons in a
cortical-shell-biased brain volume, assignment of cortical region class to each
neuron, and the
host-side morphology geometry that gives each neuron a visible soma + dendrite
tree + axon arbor. All of this is pure CPU preparation; browser startup and
structural rebuilds prepare the payload in a dedicated module worker where
feasible, while WebGPU upload still happens on the main thread. The resulting
buffers are uploaded once to the GPU and remain static for the life of the
network.

## What it owns

- `Manifold` + `ManifoldParams` — `crates/brain-visualizer/src/manifold/mod.rs → Manifold`
- Icosphere mesh generation — `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`
- Structured fold field / gyrification — `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, FoldField, gyrify_with_field`
- Brain-envelope shaping primitive — `crates/brain-visualizer/src/manifold/mod.rs → brain_outer_radius, brain_surface_point`
- Neuron volume placement — `crates/brain-visualizer/src/manifold/mod.rs → place_neurons`
- Region assignment — `crates/brain-visualizer/src/manifold/regions.rs → RegionKind, RegionAssignmentMode, assign_regions, assign_regions_with_mode`
- Spatial grid — `crates/brain-visualizer/src/manifold/mod.rs → DEFAULT_GRID_DIM`
- Anterior–posterior axis constant — `crates/brain-visualizer/src/manifold/mod.rs → ANTERIOR_POSTERIOR_AXIS`
- Per-neuron morphology geometry — `crates/brain-visualizer/src/sim/morphology.rs → Morphology, MorphSegment, MorphSphereInstance, generate, emit_soma_spheres`
- Manifold surface render shader — `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`
- Morphology render shader — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- Prepared-network payload validation — `crates/brain-visualizer/src/sim/gpu/mod.rs →
  PreparedNetworkBuild`, which reconstructs `Manifold`, `SpatialGrid`,
  `MorphSegment`, and `MorphSphereInstance` from explicit typed arrays.

## What it does NOT own

- The packing of region class into the per-neuron type byte — [`data-model.md`](data-model.md) owns that encoding; see `crates/brain-visualizer/src/sim/backend.rs → region_code, neuron_type_byte`.
- The ambient `I_ext` drive that input-region neurons receive each tick — [`simulation.md`](simulation.md) owns the drive math.
- The GPU render pipeline setup and buffer binding — [`gpu-rendering.md`](gpu-rendering.md).

## Surface generation pipeline

The six-step pipeline runs synchronously in `crates/brain-visualizer/src/manifold/mod.rs → Manifold::generate`:

1. **Icosphere subdivision** (`crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`): starts
   from a 12-vertex/20-face base icosahedron and subdivides `levels` times, each
   pass splitting every triangle into four and projecting midpoints back to the
   unit sphere. Level 5 (the default) yields a near-uniform mesh. Vertex count
   follows `10 * 4^level + 2`; a CI test guards this formula.

2. **Brain-envelope shaping** (`crates/brain-visualizer/src/manifold/mod.rs → brain_outer_radius`):
   each icosphere direction is converted into a reusable host-side brain
   envelope. The envelope is star-convex and deterministic: an elongated
   ellipsoid base is modulated by lobe-like anterior/posterior fullness, a
   ventral flattening term, temporal-side fullness, and a dorsal midline
   indentation that reads as the longitudinal fissure. The fissure is a deep
   cleft: it subtracts `0.32 × gaussian(|x|, 0, 0.15) × smoothstep(-0.05, 0.85, y)
   × gaussian(z, 0.10, 0.78)` from the radius scale, so the hemispheres separate
   along most of the dorsal length without the `scale.clamp(0.55, 1.35)` floor
   pinching the volume at the midline. This same primitive is used by both the
   surface mesh and neuron placement, so those two views of the arena cannot
   drift.

3. **Structured fold field** (`crates/brain-visualizer/src/manifold/gyrify.rs →
   FoldField`): the surface pass creates one deterministic `FoldField` from
   `GyrifyParams` and the network seed, then uses it for both surface vertices
   and neuron placement. The field combines bounded OpenSimplex texture with
   explicit cortical masks and major-groove terms so folds read as organized
   sulci/gyri rather than isotropic noise. `gyrify_with_field` applies the
   field radially to the shaped envelope and keeps the folded radius inside the
   interaction/framing bounds guarded by tests. The visual-polish verification
   log for the current pass recorded folded radii `dorsal_mid = 0.4303` and
   `fissure_mid = 0.4545`, with `max_surface = 1.2407` and `max_neuron =
   1.2389`.

4. **Folded-shell neuron placement** (`crates/brain-visualizer/src/manifold/mod.rs →
   place_neurons`): neurons are sampled from the same deterministic hash
   namespace as the rest of the app. Direction comes from spherical coordinates
   (cos theta uniform, phi uniform), then the shared `FoldField` supplies the
   local folded outer radius for that direction. Depth is shell-biased rather
   than full-volume uniform: most neurons are placed in the outer cortical band
   of the **folded** envelope, with a small deterministic interior-fill fraction
   (~8%) so the cloud keeps some depth instead of becoming a perfectly hollow
   rind. This makes the neuron cloud follow the visible folds even when the
   optional surface pass is off.

5. **Region assignment** (`crates/brain-visualizer/src/manifold/regions.rs → assign_regions`): the
   default 30/40/30 split (Input/Association/Output) is applied by shuffling
   neuron indices with a deterministic hash and slicing the result, so the split
   is spatially random rather than spatially blocked. `ManifoldParams` defaults
   to `RegionAssignmentMode::HashRandom`; all production/browser construction
   paths use that default. An internal opt-in prototype
   (`assign_regions_with_mode` with `RegionAssignmentMode::AnteriorPosteriorPrototype`)
   uses `ANTERIOR_POSTERIOR_AXIS` to rank neurons by projection plus
   deterministic jitter, keeping exact 30/40/30 counts while making input
   posterior-biased, output anterior-biased, and association central. The region
   fractions and default/prototype routing are tested by
   `crates/brain-visualizer/src/manifold/mod.rs → region_split_approx_30_40_30`,
   `default_region_assignment_mode_is_hash_random`, and
   `prototype_region_assignment_mode_is_opt_in`.

6. **Spatial grid** (`crates/brain-visualizer/src/manifold/mod.rs`): after placement, a uniform integer
   grid (`DEFAULT_GRID_DIM = 16`, giving 4096 cells) is built over the neuron
   positions for O(1) neighborhood lookup during connectivity generation and
   cursor stimulation. The current folded-placement verification recorded
   `occupied_cells = 1409` and `max_cell_occupancy = 43`.

For worker-prepared browser startup/rebuilds, the same generated facts cross the
JS/WASM boundary as explicit flat arrays: positions, region codes, surface
vertices/faces, and the spatial-grid CSR (`min`, `cell_size`, `dim`,
`cell_start`, `cell_neurons`). `PreparedNetworkBuild::from_flat_payload`
validates metadata/count agreement, region-code range, face indices, CSR
span/monotonicity, and one grid entry per neuron before WebGPU upload can
replace resources.

## Region encoding invariant

`RegionKind` is the host-side enum. Before upload to the GPU it is packed into
bits [3:2] of the 7-bit neuron type byte by `crates/brain-visualizer/src/sim/backend.rs → region_code,
neuron_type_byte`. The integer codes are: Input=0, Association=1, Output=2.
The integrate shader reads `(type >> 2) == 0` to identify input-region neurons
for ambient drive. **Do not reorder the `RegionKind` variants** without updating
`region_code` and the integrate shader. The full byte layout is owned by
[`data-model.md`](data-model.md).

## Neuron morphology geometry

`crates/brain-visualizer/src/sim/morphology.rs → generate` builds a flat list of
`MorphSegment` records, one per line segment, at `initialize()` time. The
generator is driven by `MorphologyParams` and emits a matching `MorphologyStats`
profile so review artifacts can read facts without scraping logs. The shipped
default is `MorphologyParams::locked_default`; a dev-panel-supplied
`MorphologyConfig` layers its generator fields over that locked default via
`crates/brain-visualizer/src/sim/morphology.rs → GeneratorConfig::apply_to` /
`MorphologyConfig::to_params` before regeneration (see Exposed vs protected
parameters below).

Each neuron gets deterministic target-owned incoming dendrites plus a single
**Prim-like axon tree grown by greedy attach + local relaxation**, not K
independent sin-bow curves and not the old hand-tuned trunk→cluster→twig fan.

- **Incoming dendrites** (kind 0): generated from the build-time reverse view of
  production `target_with_cell`, not from decorative local hashes. Every
  non-self raw incoming synapse is stored in `Morphology::incoming_synapses` and
  sorted by `Morphology::incoming_ranges`; duplicate `(source,target,socket)`
  records aggregate into `Morphology::incoming_socket_groups` by summed absolute
  weight. The renderer draws every unique incoming socket group at the default
  N=1200/K=16 scale. Geometry is target-owned (`neuron_id = target_id`) but no
  longer emits one long shared bucket stem. Instead, each target organizes
  incoming groups into bounded soma-surface root collars, close first forks, and
  per-group child/terminal branches. Root collars start just outside the soma
  surface, first forks default around `1.45 * base_radius`, tangential curvature
  is controlled by `dendriteCurveTightness`, and individual group spacing keeps
  dense incoming groups legible. Shared internal roots/forks carry `target_id =
  neuron_id`; source-specific terminal leaves carry `target_id = source_id` so
  they can use the presynaptic source's activity. Terminal leaves are emitted
  from the stored socket inward toward the soma, so packet direction reads
  synapse-to-soma.
- **Axon tree** (kind 1): one leaf per unique non-self target (resolved from the
  same incoming socket groups, ordered by target id). Each source neuron first
  gets one host-side `ProcessRoot` descriptor containing the soma
  center, dominant direction, soma-surface root point, first-fork point, root
  radius, summed root weight, and unique target count. The emitted arbor roots at
  `ProcessRoot::soma_root`, then emits a protected source-lit trunk to
  `ProcessRoot::first_fork` before any real target branch can begin. The greedy
  attach loop cannot attach leaves directly to the soma root or split the
  root→first-fork edge, and relaxation holds both the root and descriptor
  first-fork fixed. Single-target neurons use this same descriptor trunk before
  their terminal target edge.

  After that fixed trunk, the tree grows by **Prim-like greedy attach**: each
  iteration adds the one globally-best `(leaf, attach-point)` edge, scored by
  `distance + tree_score_curvature·curvature + tree_score_density·density +
  tree_score_degree·degree`. An attach point may **split an existing edge**,
  inserting an internal node — this is where shared trunks and forks emerge.
  After each attach, a `relax_window`-deep pass pulls internal nodes toward the
  mean of parent+children (`relax_lerp`) and repels nearby branches
  (`relax_repel`); protected trunk nodes and the fixed leaf sockets are never
  moved. All draws use `mix_key`/`hash32` with the `salt::TREE_*` constants,
  disjoint from connectivity and dendrite draws; tie-breaks are by
  `(leaf target_id, node index)` so generation is fully seed-reproducible.
- **Lighting `target_id` contract** (preserved): every internal trunk/fork edge
  carries the **source** neuron id, so shared paths stay source-lit; only a leaf
  edge carries the **real** target id, so the renderer can light the actual
  synaptic endpoint. The single-target fast path emits no source-`target_id`
  axon segment.

**Width rule — area-preserving, bottom-up.** After the tree is final, each
leaf's weight is the sum of `connectivity::weight(id, j, src_type)` over the
draws that resolved to it, taken as `unsigned_abs().max(1)` so inhibitory
(negative) weights still give a visible positive-width twig. A bottom-up pass
(descending depth, deterministic order) sums each node's subtree weight, and
internal nodes use
`radius = R_trunk · √(subtree_weight / total_weight)`, floored at
`R_trunk · twig_radius_fraction`. `R_trunk = base_radius · axon_root_radius_fraction`
and the locked root fraction is `0.90`. Terminal leaf nodes are set directly to
the twig floor so every terminal edge has a fair trunk-to-tip taper, including
single-target arbors. The protected trunk carries 100% of the weight at full
radius; each fork sheds its children's share. The √ (Murray/Rall,
area-preserving) keeps trunks substantial and the thinnest twig still visible —
see [`../decisions/manifold.md`](../decisions/manifold.md).

**Soft fork degree.** The `tree_score_degree` term makes a node resist its
3rd/4th child (penalty monotonic in current child count), biasing toward 2-3-way
forks. It is a *tendency*, not a hard cap; relaxation then spreads siblings into
fork-like geometry. `MorphologyStats::cluster_count_histogram` (now the
fork-degree histogram, index 5 = "5+") plus `tree_depth_max/mean` and
`radius_bands` carry the review evidence.

Source-type bytes are built from the same region+seed contract used for
production connectivity, so morphology target resolution matches the sim's real
`target_with_cell` rule. Self targets are filtered out of the stored incoming
view. Duplicate targets are retained as raw incoming records, then aggregated
only for visible socket groups and axon leaves by summed absolute weight.
Unique-target coverage is the acceptance target, and the shared arbor is
budgeted with named segment classes plus slack rather than an opaque fixed cap.

`MorphSegment` is the **branch-only** contract: 48 bytes, std430, 16-aligned. It carries two endpoints (`a`, `b`), `radius_a`, `radius_b`, `neuron_id`, `path_len`, `kind` (0=dendrite, 1=axon), and `target_id`. Field order is a hard Rust ↔ WGSL contract — see `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment` and the matching WGSL struct in `render_morphology.wgsl`. The size assert is `crates/brain-visualizer/src/sim/morphology.rs → segment_layout_is_48_bytes`. GPU upload chunks the flat list into multiple segment storage bindings when needed; chunking does not change this record layout or the generator's flat output. Prepared rebuild payloads do not serialize this struct as guessed raw bytes: they carry explicit field arrays (`segment_endpoints`, `segment_path_len`, owner/kind/target ids), and Rust reconstructs canonical `MorphSegment` values before upload.

`MorphSphereInstance` is the **soma-only** contract: 48 bytes, 16-aligned. One instance per neuron, emitted at `initialize()` time by `crates/brain-visualizer/src/sim/morphology.rs → emit_soma_spheres` from the neuron position arrays plus the matching host-side `ProcessRoot` descriptor. Fields: `center: [f32; 3]`, `radius: f32` (= `params::R0`), `neuron_id: u32`, `kind: u32` (= 2 for soma), `_pad0`, `_pad1`, `root_dir: [f32; 3]`, and `root_pull: f32`. `root_dir/root_pull` carry the dominant axon root direction and bounded deformation strength consumed by `render_morphology.wgsl → vs_sphere`; neurons with no unique outgoing target get zero pull. The size assert is `crates/brain-visualizer/src/sim/morphology.rs → sphere_instance_layout_is_48_bytes`.

`path_len` is the cumulative path length from the branch root to endpoint `a`:
incoming source-specific dendrite leaves root at their stored socket and travel
inward, shared dendrite stems root at their aggregate branch point, and axons
root at `ProcessRoot::soma_root`.
The generator computes it from the emitted sampled chain distance, and sibling
branches start from their parent's endpoint path instead of accumulating across
unrelated siblings. The morphology renderer uses it again in v0.3.3 by adding a
local segment interpolant (`t * length(b-a)`) to recover per-fragment path
position for the traveling packet in `render_morphology.wgsl`. All hash inputs
use `crates/brain-visualizer/src/connectivity/hash.rs → mix_key, hash32` with
salts defined in `crates/brain-visualizer/src/sim/morphology.rs → salt` so
morphology draws stay disjoint from connectivity target/weight draws.

**Adaptive edge subdivision.** Every emitted edge is tessellated by
`crates/brain-visualizer/src/sim/morphology.rs → adaptive_subsegments(edge_len,
curvature, is_long_range, params)`, which is length- and curvature-aware:
`subs = clamp(ceil(edge_len / max_len) + round(curvature · curvature_subsegment_boost),
min_subsegments, edge_subsegments_max)`. `max_len` is
`long_range_max_segment_length` for long-range hops and `max_segment_length`
otherwise — long-range uses the *smaller* max so long fibers get more spatial
samples for readable pulse motion. `curvature` is `|bend| / edge_len` from the
salt-seeded bend vector, so the count is fully deterministic (no
runtime/camera/float-reduction input). The locked defaults are
`max_segment_length = 0.05`, `long_range_max_segment_length = 0.025`,
`curvature_subsegment_boost = 2.0`, `edge_subsegments_max = EDGE_SUBSEGMENTS_MAX`,
`min_subsegments = 1`. The legacy flat `edge_subsegments` knob is retained as a
floor/legacy control (it still drives a dev-panel slider) but is no longer the
sole subdivision control. `MorphSegment.path_len` cumulative continuity is
preserved across the adaptively-tessellated chain.

**Long-range axon waypoint routing.** A long axon leaf no longer crosses the
volume as one giant span. `crates/brain-visualizer/src/sim/morphology.rs →
long_range_waypoints` inserts 1–3 deterministic intermediate waypoints (terminal
socket excluded), each placed on the parent→socket chord then bowed outward from
the brain-volume centroid (`4·t·(1−t)` so the bow peaks mid-edge and vanishes at
the anchors) plus salt-seeded lateral detours (`salt::TREE_BEND` /
`salt::TREE_SPLIT`). The leaf is then emitted as `waypoints.len() + 1` bowed
Bezier hops, each with its own bend and its own adaptive subdivision. An edge is
classified "visually long" by **world distance** — the leaf chord parent→socket
exceeding `long_range_chord_cells · grid.cell_size` (see
[`connectivity.md`](connectivity.md) for why distance, not a connectivity flag).
Waypoints are pure visual route geometry: the leaf still carries the real target
id and the connectivity target/weight rule is untouched (guarded by
`deterministic_with_long_range_waypoints` and `waypoints_preserve_leaf_target_identity`).
Locked params: `long_range_chord_cells = 3.0`, `long_range_max_waypoints = 3`
(mirrored by `LONG_RANGE_MAX_WAYPOINTS`), `long_range_waypoint_span = 0.20`,
`long_range_lateral_offset = 0.12`.

**Bushy local dendrite decoration.** Beyond the real presynaptic incoming
geometry, `emit_incoming_dendrites` grows a bushy local arbor off each incoming
group's tip: secondary branchlets (outward processes with curl + 3D splay) and
terminal twigs (a thin splayed brush, each twig with its own salt-seeded curl so
curvature varies per branch rather than one bow per edge), with a trunk→twig
radius taper (`dendrite_twig_radius_fraction ≈ 0.18` of base). The new locked
controls are `dendrite_branchlet_count/length_fraction/radius_fraction`,
`dendrite_twig_count/length_fraction/radius_fraction`, `dendrite_twig_curl`, and
`dendrite_decor_group_max`; new salts are `salt::DENDRITE_BRANCHLET`,
`DENDRITE_TWIG`, `DENDRITE_TWIG_CURL`.

**Activity-owner invariant (contract).** Real presynaptic incoming leaves keep
`kind == 0`, `neuron_id` = the receiving neuron, `target_id` = the source id, and
`path_len = 0` — the renderer derives their activity from `target_id`. Decorative
branchlets and twigs are **self-owned** (`target_id == neuron_id`) and never
invent target ids, so they light with the neuron and can never corrupt
presynaptic semantics (guarded by
`bushy_dendrite_decorations_preserve_presynaptic_owner_rule`).

**Decoration cap.** Dendrite decoration density is bounded by
`crates/brain-visualizer/src/sim/morphology.rs → effective_decor_group_max`,
which clamps the configured group count to `DENDRITE_DECOR_GROUP_MAX` but does
not ramp down with N. High-N storage pressure is handled by GPU segment chunking
owned by [`scaling.md`](scaling.md) / [`gpu-rendering.md`](gpu-rendering.md), not
by silently changing the generated morphology. Three of the new dendrite controls
are user-exposed as generator controls (regenerate `applyKind`):
`dendriteBranchletCount`, `dendriteTwigCount`, `dendriteDecorGroupMax`
(runtime-clamped to compile-time maxes); the rest stay locked. The dev-panel
wiring is owned by [`dev-panel.md`](dev-panel.md).

The buffer cap is `n * max_segs_per_neuron(k)`, where the per-neuron cap is
`dendrite_budget + trunk_cluster_budget + k·terminal_twig_budget + cap_slack`
(`crates/brain-visualizer/src/sim/morphology.rs → segment_cap, max_segs_per_neuron`)
rather than a fixed constant. The Prim tree emits roughly `2k` edges, and each
edge's subsegment count is set by the adaptive rule (see below), bounded above by
the descriptor-max `EDGE_SUBSEGMENTS_MAX`; a long-range leaf can additionally
route through up to `LONG_RANGE_MAX_WAYPOINTS + 1` hops, so the per-target budget
is sized for `(LONG_RANGE_MAX_WAYPOINTS + 1) · EDGE_SUBSEGMENTS_MAX` — the cap
never under-allocates regardless of edge length, waypoint count, or where the
live subdivision sliders sit. If the cap is hit, the excess is counted in
`Morphology::dropped` and printed; no silent truncation.

`MorphologyStats` reports the reverse incoming profile used by review artifacts:
raw incoming count, unique visible socket groups, mean/p99/max raw in-degree,
mean/p99/max visible groups per target, incoming capped/dropped counts, storage
bytes for raw/range/group vectors, total segment count, cap, p99/max segments
per neuron, and total dropped count.

## Exposed vs protected parameters

`MorphologyParams` (`crates/brain-visualizer/src/sim/morphology.rs → MorphologyParams`)
splits into two classes:

- **Exposed (tunable):** the generator shape fields — branch counts, reach,
  socket placement, radius/taper fractions, the tree-grammar knobs
  (`tree_score_curvature/density/degree`, `relax_lerp/repel/window`,
  the legacy floor `edge_subsegments`), bounded straight-subdivision controls
  (`maxSegmentLength`, `longRangeMaxSegmentLength`,
  `curvatureSubsegmentBoost`, `edgeSubsegmentsMax`, `minSubsegments`), and
  three bushy-decoration controls
  (`dendriteBranchletCount`, `dendriteTwigCount`, `dendriteDecorGroupMax`,
  runtime-clamped to their compile-time maxes). These are surfaced to the hidden dev
  panel as a `MorphologyConfig` (see [`dev-panel.md`](dev-panel.md)) and reach
  the backend through the WASM entry point
  `crates/brain-visualizer/src/lib.rs → WasmGpuBackend::set_morphology_config` →
  `crates/brain-visualizer/src/sim/gpu/mod.rs → GpuBackend::set_morphology_config`,
  which deserializes the config (serde, camelCase), diffs it against the current
  config, and runs the narrowest update — generator changes trigger a full
  morphology regeneration. This path is separate from the `VisualSettings`
  Float32Array; see [`../decisions/manifold.md`](../decisions/manifold.md).
- **Protected (never exposed):** the four allocation/safety budgets
  (`dendrite_budget`, `trunk_cluster_budget`, `terminal_twig_budget`,
  `cap_slack`), waypoint counts/routing thresholds, and the `salt::*` hash constants. These are re-locked by
  `GeneratorConfig::apply_to` even when a config is applied, so the buffer cap
  and determinism namespace cannot be moved from the UI. Exposing them risks
  silent truncation/OOM or breaking seed reproducibility, with no visual upside.

The older dead `dendritePrimaryMin` / `dendritePrimarySpan` controls are no
longer in the exposed config surface. The current target-owned incoming
dendrite generator's effective placement controls are `socketCount*`,
`socketRadius*`, `socketTipPreference`, `dendritePrimaryRootCount`,
`dendriteForkDistance`, `dendriteCurveTightness`,
`dendriteMidRadiusFraction`, `dendriteTipRadiusFraction`, and
`dendriteGroupSpacing`. Old persisted morphology payloads that still contain
removed fields are accepted and normalized away by the web loader/Rust JSON
deserializer. The duplicate `generator.axonCurveLift` descriptor is also not
exposed; `connectionCurveLift` is the user-facing curve control.

Long-range axon leaves use deterministic waypoint routing when the leaf chord
exceeds the locked distance threshold. Waypoints are clamped to the same
generation-time inner brain bounds before the emitted straight subsegments are
sampled, and the route endpoints/control points are clamped for those routed
leaves. This keeps long projection polylines inside the brain volume without
shrinking rendered radii or adding curved shader geometry.

## Rendering

**Manifold surface** (`crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`): a flat dark
fill pass drawn from the folded mesh vertices before neuron glows, so the brain
shape reads through the glow layer. Gated by the `surface` setting
(0=off, 1=dim, 2=normal); when off the pass is skipped entirely on the CPU side.
Controlled by `surface_opacity` and `surface_mode` uniforms.

**Neuron morphology** (`crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`):
two separate draw sub-passes, both additive blend, no depth write, bloom-friendly.
The *tube sub-pass* (`vs_main / fs_main`) draws one shader-generated tapered cylinder
per `MorphSegment` — 36 vertices per instance. The *soma sphere sub-pass*
(`vs_sphere / fs_sphere`) draws one UV-sphere per `MorphSphereInstance` — 288
vertices per instance. Both sub-passes read `last_spike`, but they now use it
in different ways: the soma sphere shares the far-body soma pulse timing
(`glow` + short `flash` + white-core lift), while the tube pass turns
`path_len + local_segment_t * length(b-a)` into a moving source-owned packet.
The packet's travel lifetime is independent of `glow_tau`; glow decay affects
soma/legacy afterglow only.
Axons carry the full outward packet; dendrites get only a weak near-soma echo so
the renderer does not imply false outgoing dendrite signaling. A simple ambient + diffuse + rim
lighting model is applied via `MorphUniforms` (192 B, shared by both
sub-passes); the lighting/brightness defaults are the dev-panel-tunable
`MorphologyConfig` lighting group (`crates/brain-visualizer/src/sim/morphology.rs → LightingConfig`), not hardcoded
shader constants. Resting structure is drawn at the config-owned
`resting_brightness`; branch and soma color get low-amplitude deterministic
procedural material variation from world position, normal, `path_len`, `kind`,
and `neuron_id`, with no texture assets or new layout fields. The full lighting
model and pass order are owned by
[`gpu-rendering.md`](gpu-rendering.md).

The `crates/brain-visualizer/examples/morph_view.rs` harness exercises the full pipeline: N=1200/K=16,
250 warm-up ticks, three camera distances, plus a `morph_resting_opacity=0` frame
to verify the lit-connections-only look. Its artifact JSON snapshots the full
`MorphologyConfig` (all three groups) and emits a stronger-active-brightness
variant to `/tmp/morph_view_active_bright_stats.json` for the tuning pass. In
the visual-product-polish consolidated gate, the default artifact reported
`segment_count=103526`, `dropped_count=0`, `incoming_dropped_count=0`,
`segment_cap_per_neuron=296`, `segment_cap=355200`, p99/max segments
`159/242`, and incoming visible groups mean/p99/max `10.356667/29/45`.
Converted frames were nonblank and showed close soma-proximal dendrite
branching.

## Update when

- `brain_outer_radius` / `brain_surface_point` changes in a way that materially
  alters the silhouette or placement volume.
- `place_neurons` changes away from the current shell-biased envelope sampler.
- `assign_regions` is changed to use the anterior–posterior axis for spatial
  blocking instead of the current hash-shuffle.
- `MorphologyParams`, `MorphologyStats`, or the source-type bytes contract
  change.
- The adaptive-subdivision rule (`adaptive_subsegments`), the long-range
  classification/waypoint routing (`long_range_waypoints`, `LONG_RANGE_MAX_WAYPOINTS`),
  the bushy local decoration grammar, or `effective_decor_group_max` change.
- The activity-owner invariant (presynaptic leaf `kind/neuron_id/target_id/path_len`
  vs self-owned decoration) changes.
- The exposed-vs-protected split changes (a field is added to/removed from the
  `MorphologyConfig` generator surface, or a budget/salt becomes exposed), or the
  `set_morphology_config` apply path changes.
- `MorphSegment` field order or size changes (update the layout contract description above and cross-check `render_morphology.wgsl`).
- `MorphSphereInstance` field order or size changes (update the contract description above and cross-check the WGSL sphere struct).
- `GyrifyParams` defaults change (the gyri/sulci frequencies and amplitudes).
- `max_segs_per_neuron` or the segment-cap policy changes, or unique-target
  terminal coverage stops being the rendered contract.
- The `surface` or `connection_layer` setting semantics change.

## See also

- [`../decisions/manifold.md`](../decisions/manifold.md) — why procedural generation over mesh assets
- [`data-model.md`](data-model.md) — type-byte packing, region codes, `last_spike` word layout
- [`simulation.md`](simulation.md) — ambient `I_ext` drive on input-region neurons
- [`gpu-rendering.md`](gpu-rendering.md) — pipeline wiring, buffer binding, render pass order
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
