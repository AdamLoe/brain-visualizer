---
status:        active
owner:         adamg
last_updated:  2026-06-08
---

# Cortical Manifold

Procedural generation of a brain-shaped surface, placement of neurons in a
cortical-shell-biased brain volume, assignment of cortical region class to each
neuron, and the
host-side morphology geometry that gives each neuron a visible soma + dendrite
tree + axon arbor. All of this runs on the CPU at `initialize()` time; the
resulting buffers are uploaded once to the GPU and remain static for the
life of the network.

## What it owns

- `Manifold` + `ManifoldParams` — `crates/brain-visualizer/src/manifold/mod.rs → Manifold`
- Icosphere mesh generation — `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`
- Gyrification noise — `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, gyrify`
- Brain-envelope shaping primitive — `crates/brain-visualizer/src/manifold/mod.rs → brain_outer_radius, brain_surface_point`
- Neuron volume placement — `crates/brain-visualizer/src/manifold/mod.rs → place_neurons`
- Region assignment — `crates/brain-visualizer/src/manifold/regions.rs → RegionKind, assign_regions`
- Spatial grid — `crates/brain-visualizer/src/manifold/mod.rs → DEFAULT_GRID_DIM`
- Anterior–posterior axis constant — `crates/brain-visualizer/src/manifold/mod.rs → ANTERIOR_POSTERIOR_AXIS`
- Per-neuron morphology geometry — `crates/brain-visualizer/src/sim/morphology.rs → Morphology, MorphSegment, MorphSphereInstance, generate, emit_soma_spheres`
- Manifold surface render shader — `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`
- Morphology render shader — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`

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

3. **Gyrification** (`crates/brain-visualizer/src/manifold/gyrify.rs → gyrify`): three octaves of
   OpenSimplex noise displace each shaped envelope vertex along its outward
   direction. Coarse lumps use `lump_freq ≈ 0.8` at `lump_amp ≈ 12%` of local
   radius (large slow bulges → a lumpier silhouette); gyri/ridges use
   `gyri_freq ≈ 1.5` at `gyri_amp ≈ 15%`; fine sulci/folds use `sulci_freq ≈ 4.0`
   at `sulci_amp ≈ 5%`. The three octaves use independent noise instances —
   distinct seed offsets (`seed`, `+0x9e3779b9`, `+0x85ebca6b`) derived from the
   network seed — so they decorrelate. See
   `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams` for all tunable
   defaults (a tracked drift-verification surface).

4. **Neuron placement** (`crates/brain-visualizer/src/manifold/mod.rs → place_neurons`): neurons are
   sampled from the same deterministic hash namespace as the rest of the app.
   Direction comes from spherical coordinates (cos θ uniform, φ uniform), then
   the shared brain envelope supplies the local outer radius for that direction.
   Depth is shell-biased rather than full-volume uniform: most neurons are
   placed in the outer cortical band (`~0.72..1.0` of the local envelope
   radius), with a small deterministic interior-fill fraction (`~8%`) so the
   cloud keeps some depth instead of becoming a perfectly hollow rind.

5. **Region assignment** (`crates/brain-visualizer/src/manifold/regions.rs → assign_regions`): the
   30/40/30 split (Input/Association/Output) is applied by shuffling neuron
   indices with a deterministic hash and slicing the result, so the split is
   spatially random rather than spatially blocked. The `_axis` parameter is
   accepted but unused — the code comment and the old anterior–posterior intent
   are preserved as `ANTERIOR_POSTERIOR_AXIS` in `mod.rs`, but the actual
   assignment is hash-driven. The region fractions (30/40/30) are tested by
   `crates/brain-visualizer/src/manifold/mod.rs → region_split_approx_30_40_30`.

6. **Spatial grid** (`crates/brain-visualizer/src/manifold/mod.rs`): after placement, a uniform integer
   grid (`DEFAULT_GRID_DIM = 16`, giving 4096 cells) is built over the neuron
   positions for O(1) neighborhood lookup during connectivity generation and
   cursor stimulation.

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

Each neuron gets a deterministic arbor: a local dendrite tree plus a single
**Prim-like axon tree grown by greedy attach + local relaxation**, not K
independent sin-bow curves and not the old hand-tuned trunk→cluster→twig fan.

- **Dendrites** (kind 0): a stable local tree of primary branches and twigs
  around the soma, used as visible landing context for sockets. Each stem/twig
  is emitted as a short sampled cubic-Bezier chain rather than a single chord,
  so close-up branches curve through multiple `MorphSegment`s while keeping the
  same 48-byte layout. This block is unchanged by the tree rewrite — reach,
  count, and taper still come from named morphology budgets.
- **Axon tree** (kind 1): one leaf per unique non-self target (resolved by the
  production `target_with_cell` rule, deduped, ordered by target id). For a
  multi-target neuron the tree seeds a shared trunk node along the mean target
  direction, then grows by **Prim-like greedy attach**: each iteration adds the
  one globally-best `(leaf, attach-point)` edge, scored by
  `distance + tree_score_curvature·curvature + tree_score_density·density +
  tree_score_degree·degree`. An attach point may **split an existing edge**,
  inserting an internal node — this is where shared trunks and forks emerge.
  After each attach, a `relax_window`-deep pass pulls internal nodes toward the
  mean of parent+children (`relax_lerp`) and repels nearby branches
  (`relax_repel`); the soma root and the fixed leaf sockets are never moved. A
  single-target neuron takes the trivial fast path (root → one leaf, no shared
  root). All draws use `mix_key`/`hash32` with the `salt::TREE_*` constants,
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
`radius = R_trunk · √(subtree_weight / total_weight)`, floored at
`R_trunk · twig_radius_fraction`. `R_trunk = base_radius · axon_root_radius_fraction`.
The trunk carries 100% of the weight at full radius; each fork sheds its
children's share. The √ (Murray/Rall, area-preserving) keeps trunks substantial
and the thinnest twig still visible — see [`../decisions/manifold.md`](../decisions/manifold.md).

**Soft fork degree.** The `tree_score_degree` term makes a node resist its
3rd/4th child (penalty monotonic in current child count), biasing toward 2-3-way
forks. It is a *tendency*, not a hard cap; relaxation then spreads siblings into
fork-like geometry. `MorphologyStats::cluster_count_histogram` (now the
fork-degree histogram, index 5 = "5+") plus `tree_depth_max/mean` and
`radius_bands` carry the review evidence.

Source-type bytes are built from the same region+seed contract used for
production connectivity, so morphology target resolution matches the sim's real
`target_with_cell` rule. Duplicate and self targets are filtered before
generation, unique-target coverage is the acceptance target, and the shared
arbor is budgeted with named segment classes plus slack rather than an opaque
fixed cap.

`MorphSegment` is the **branch-only** contract: 48 bytes, std430, 16-aligned. It carries two endpoints (`a`, `b`), `radius_a`, `radius_b`, `neuron_id`, `path_len`, `kind` (0=dendrite, 1=axon), and `target_id`. Field order is a hard Rust ↔ WGSL contract — see `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment` and the matching WGSL struct in `render_morphology.wgsl`. The size assert is `crates/brain-visualizer/src/sim/morphology.rs → segment_layout_is_48_bytes`.

`MorphSphereInstance` is the **soma-only** contract: 32 bytes, 16-aligned. One instance per neuron, emitted at `initialize()` time by `crates/brain-visualizer/src/sim/morphology.rs → emit_soma_spheres` from the neuron position arrays. Fields: `center: [f32; 3]`, `radius: f32` (= `params::R0`), `neuron_id: u32`, `kind: u32` (= 2 for soma), `_pad0`, `_pad1`. The size assert is `crates/brain-visualizer/src/sim/morphology.rs → sphere_instance_layout_is_32_bytes`.

`path_len` is the cumulative path length from the soma to endpoint `a`. The
generator computes it from the emitted sampled chain distance, and sibling
branches start from their parent's endpoint path instead of accumulating across
unrelated siblings. The morphology renderer uses it again in v0.3.3 by adding a
local segment interpolant (`t * length(b-a)`) to recover per-fragment path
position for the traveling packet in `render_morphology.wgsl`. All hash inputs use
`crates/brain-visualizer/src/connectivity/hash.rs → mix_key, hash32` with salts
defined in `crates/brain-visualizer/src/sim/morphology.rs → salt` so
morphology draws stay disjoint from connectivity target/weight draws.

The buffer cap is `n * max_segs_per_neuron(k)`, where the per-neuron cap is
`dendrite_budget + trunk_cluster_budget + k·terminal_twig_budget + cap_slack`
(`crates/brain-visualizer/src/sim/morphology.rs → segment_cap, max_segs_per_neuron`)
rather than a fixed constant. The Prim tree emits roughly `2k` edges ×
`edge_subsegments` axon segments, so the per-target budget is sized for the
descriptor-max `edge_subsegments` (`EDGE_SUBSEGMENTS_MAX`) — the cap never
under-allocates regardless of where the live `edge_subsegments` slider sits. If
the cap is hit, the excess is counted in `Morphology::dropped` and printed; no
silent truncation.

## Exposed vs protected parameters

`MorphologyParams` (`crates/brain-visualizer/src/sim/morphology.rs → MorphologyParams`)
splits into two classes:

- **Exposed (tunable):** the generator shape fields — branch counts, reach,
  socket placement, radius/taper fractions, and the tree-grammar knobs
  (`tree_score_curvature/density/degree`, `relax_lerp/repel/window`,
  `edge_subsegments`). These are surfaced to the hidden dev
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
  `cap_slack`) and the `salt::*` hash constants. These are re-locked by
  `GeneratorConfig::apply_to` even when a config is applied, so the buffer cap
  and determinism namespace cannot be moved from the UI. Exposing them risks
  silent truncation/OOM or breaking seed reproducibility, with no visual upside.

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
to verify the lit-connections-only look. Its artifact JSON now snapshots the full
`MorphologyConfig` (all three groups) and emits a stronger-active-brightness
variant to `/tmp/morph_view_active_bright_stats.json` for the tuning pass.

## Update when

- `brain_outer_radius` / `brain_surface_point` changes in a way that materially
  alters the silhouette or placement volume.
- `place_neurons` changes away from the current shell-biased envelope sampler.
- `assign_regions` is changed to use the anterior–posterior axis for spatial
  blocking instead of the current hash-shuffle.
- `MorphologyParams`, `MorphologyStats`, or the source-type bytes contract
  change.
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
