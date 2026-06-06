---
status:        active
owner:         adamg
last_updated:  2026-06-04
---

# Cortical Manifold

Procedural generation of a brain-shaped surface, placement of neurons on/in
that surface, assignment of cortical region class to each neuron, and the
host-side morphology geometry that gives each neuron a visible soma + dendrite
tree + axon arbor. All of this runs on the CPU at `initialize()` time; the
resulting buffers are uploaded once to the GPU and remain static for the
life of the network.

## What it owns

- `Manifold` + `ManifoldParams` — `crates/brain-visualizer/src/manifold/mod.rs → Manifold`
- Icosphere mesh generation — `crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`
- Gyrification noise — `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, gyrify`
- Neuron volume placement — `crates/brain-visualizer/src/manifold/mod.rs → place_neurons`
- Region assignment — `crates/brain-visualizer/src/manifold/regions.rs → RegionKind, assign_regions`
- Spatial grid — `crates/brain-visualizer/src/manifold/mod.rs → DEFAULT_GRID_DIM`
- Anterior–posterior axis constant — `crates/brain-visualizer/src/manifold/mod.rs → ANTERIOR_POSTERIOR_AXIS`
- Per-neuron morphology geometry — `crates/brain-visualizer/src/sim/morphology.rs → Morphology, MorphSegment, generate`
- Manifold surface render shader — `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`
- Morphology render shader — `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`

## What it does NOT own

- The packing of region class into the per-neuron type byte — [`data-model.md`](data-model.md) owns that encoding; see `crates/brain-visualizer/src/sim/backend.rs → region_code, neuron_type_byte`.
- The ambient `I_ext` drive that input-region neurons receive each tick — [`simulation.md`](simulation.md) owns the drive math.
- The GPU render pipeline setup and buffer binding — [`gpu-rendering.md`](gpu-rendering.md).

## Surface generation pipeline

The five-step pipeline runs synchronously in `crates/brain-visualizer/src/manifold/mod.rs → Manifold::generate`:

1. **Icosphere subdivision** (`crates/brain-visualizer/src/manifold/icosphere.rs → icosphere`): starts
   from a 12-vertex/20-face base icosahedron and subdivides `levels` times, each
   pass splitting every triangle into four and projecting midpoints back to the
   unit sphere. Level 5 (the default) yields a near-uniform mesh. Vertex count
   follows `10 * 4^level + 2`; a CI test guards this formula.

2. **Gyrification** (`crates/brain-visualizer/src/manifold/gyrify.rs → gyrify`): two octaves of
   OpenSimplex noise displace each unit-sphere vertex along its outward normal
   (normal = position on a unit sphere). Large-scale noise (gyri, ridges) uses
   `gyri_freq ≈ 1.5` at `gyri_amp ≈ 15%` of radius; fine-scale noise (sulci,
   folds) uses `sulci_freq ≈ 4.0` at `sulci_amp ≈ 5%`. The two octaves use
   independent noise instances (different seeds derived from the network seed)
   so they decorrelate. Total radius variation stays within roughly ±20%, keeping
   all neurons inside the unit volume. See `crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams`
   for all tunable defaults.

3. **Neuron placement** (`crates/brain-visualizer/src/manifold/mod.rs → place_neurons`): neurons are
   placed with uniform volume density inside a sphere of radius 1.0 using
   spherical coordinates (cos θ uniform, φ uniform, r ∝ cbrt(uniform)). The
   function signature accepts vertices/faces but does not use them — placement is
   purely volumetric, not surface-pinned. This is the current code behavior.
   All randomness comes from the `hash32` function keyed on `seed ^ (i * salt)`.

4. **Region assignment** (`crates/brain-visualizer/src/manifold/regions.rs → assign_regions`): the
   30/40/30 split (Input/Association/Output) is applied by shuffling neuron
   indices with a deterministic hash and slicing the result, so the split is
   spatially random rather than spatially blocked. The `_axis` parameter is
   accepted but unused — the code comment and the old anterior–posterior intent
   are preserved as `ANTERIOR_POSTERIOR_AXIS` in `mod.rs`, but the actual
   assignment is hash-driven. The region fractions (30/40/30) are tested by
   `crates/brain-visualizer/src/manifold/mod.rs → region_split_approx_30_40_30`.

5. **Spatial grid** (`crates/brain-visualizer/src/manifold/mod.rs`): after placement, a uniform integer
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

`crates/brain-visualizer/src/sim/morphology.rs → generate` builds a flat list of `MorphSegment` records,
one per line segment, at `initialize()` time. Each neuron gets:

- **Dendrites** (kind 0): 3–5 primary branches, each bifurcating once, radiating
  outward from the soma. Reach is randomized per branch in the
  `DENDRITE_REACH_LO..DENDRITE_REACH_HI` band.
- **Axon arbor** (kind 1): one branch per **all K** outgoing connections (not a
  5-branch subset), each routing toward a real synaptic target drawn from the
  same connectivity rule the sim uses — so the drawn axons match the actual
  synapses the renderer lights. Each branch is a curved poly-line of
  `AXON_SEGS_PER_BRANCH` (6) segments; arc height scales with the live
  `connection_curve_lift` setting and is amplified by a `BOW_GAIN` so the bow
  reads at the default lift (and straightens fully at lift 0). Each axon segment
  records its `target_id`; dendrites record `target_id = neuron_id` (self).

`MorphSegment` is 48 bytes, std430, 16-aligned. The field order is a hard
contract between Rust and WGSL — see `crates/brain-visualizer/src/sim/morphology.rs → MorphSegment`
and the matching WGSL struct in `render_morphology.wgsl`. The final 16-byte row
is `neuron_id, path_len, kind, target_id` (the trailing `_pad` slot was
repurposed to `target_id`).

`path_len` is the cumulative path length from the soma to endpoint `a`, retained
in the struct but no longer driving render timing (the connection-lighting model
keys off spikes, not path position — see [`gpu-rendering.md`](gpu-rendering.md)).
A test in `crates/brain-visualizer/src/sim/morphology.rs → segment_layout_is_48_bytes` guards the size.
All hash inputs use `crates/brain-visualizer/src/connectivity/hash.rs → mix_key, hash32` with salts
defined in `crates/brain-visualizer/src/sim/morphology.rs → salt` — disjoint from connectivity salts so
morphology draws never collide with target/weight draws.

The buffer cap is `n * max_segs_per_neuron(k)`, where the per-neuron cap is
derived from K (`crates/brain-visualizer/src/sim/morphology.rs → max_segs_per_neuron`:
`DENDRITE_MAX + k * AXON_SEGS_PER_BRANCH + slack`) rather than a fixed constant,
since coverage now scales with K. If the cap is hit, the excess is counted in
`Morphology::dropped` and printed; no silent truncation.

## Rendering

**Manifold surface** (`crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`): a flat dark
fill pass drawn from the folded mesh vertices before neuron glows, so the brain
shape reads through the glow layer. Gated by the `surface` setting
(0=off, 1=dim, 2=normal); when off the pass is skipped entirely on the CPU side.
Controlled by `surface_opacity` and `surface_mode` uniforms.

**Neuron morphology** (`crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`): instanced
draw, one instance per `MorphSegment`, 6 vertices per instance forming a
camera-facing tapered quad. The `last_spike` storage buffer is read per instance
to drive whole-connection spike lighting (no traveling pulse): when a neuron
fires its connections light instantly and fade with the same
`exp(-tick_diff/glow_tau)` curve as the far-glow dot. Resting structure is drawn
at `base_brightness` (the `morph_resting_opacity` setting); setting it to zero
hides resting structure and shows only lit connections. Axon color is E/I-tinted
by default or region-tinted when `color_by == 0`. Additive blending, no depth
write, bloom-friendly. The lighting model (downstream/upstream toggles, τ fade)
is owned by [`gpu-rendering.md`](gpu-rendering.md).

The `crates/brain-visualizer/examples/morph_view.rs` harness exercises the full pipeline: N=1200/K=16,
250 warm-up ticks, three camera distances, plus a `morph_resting_opacity=0` frame
to verify the lit-connections-only look.

## Update when

- `place_neurons` is changed to surface-pinned or barycentric placement (the doc
  currently describes volumetric placement — the code wins).
- `assign_regions` is changed to use the anterior–posterior axis for spatial
  blocking instead of the current hash-shuffle.
- `MorphSegment` field order or size changes (update the layout table above and
  cross-check `render_morphology.wgsl`).
- `GyrifyParams` defaults change (the gyri/sulci frequencies and amplitudes).
- `max_segs_per_neuron` or the segment-cap policy changes, or axon coverage stops being all-K.
- The `surface` or `connection_layer` setting semantics change.

## See also

- [`../decisions/manifold.md`](../decisions/manifold.md) — why procedural generation over mesh assets
- [`data-model.md`](data-model.md) — type-byte packing, region codes, `last_spike` word layout
- [`simulation.md`](simulation.md) — ambient `I_ext` drive on input-region neurons
- [`gpu-rendering.md`](gpu-rendering.md) — pipeline wiring, buffer binding, render pass order
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
