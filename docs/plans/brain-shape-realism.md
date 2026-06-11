---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - architecture/web-frontend.md
  - decisions/manifold.md
  - decisions/rendering.md
  - decisions/interaction.md
---

# Brain shape realism

## Mission

Improve the procedural brain manifold so the first-read shape is more
recognizably brain-like without breaking the asset-free, deterministic,
beauty-first build. Done means the lead has chosen what "realistic" means for
this product pass, implementation can proceed from code-accurate generator
levers, and each stream has narrow gates/artifacts that catch shape, placement,
rendering, and interaction regressions without running the full suite per
stream.

## Grounding

Authoritative code is `app/crates/brain-visualizer/src/manifold/*`. Older plan
language should be treated as historical if it disagrees with those files.

The current generator is a compact, deterministic pipeline:

1. `icosphere.rs -> icosphere` builds a watertight subdivided unit sphere.
   Default `ManifoldParams::new` uses subdivision level 5.
2. `mod.rs -> brain_outer_radius` maps each direction to a star-convex
   brain-shaped envelope with `BRAIN_AXES = [0.92, 0.78, 1.30]`, lobe/fullness
   terms, ventral flattening, temporal fullness, a dorsal midline fissure term,
   rear taper, and a final radius-scale clamp of `0.55..1.35`.
3. `gyrify.rs -> gyrify` applies three independent OpenSimplex radial
   displacement fields over the shaped surface: coarse lumps
   (`freq=0.8`, `amp=0.12`), gyri (`freq=1.5`, `amp=0.15`), and sulci
   (`freq=4.0`, `amp=0.05`). The comment in `mod.rs` still says two octaves,
   but the code uses three.
4. `mod.rs -> place_neurons` samples uniform random directions inside the same
   smooth envelope. About 92% of neurons sit in a cortical shell from roughly
   `0.72..1.0` of local envelope radius; about 8% are interior fill from roughly
   `0.25..0.70`. Placement does not follow the post-gyrification folds.
5. `regions.rs -> assign_regions` ignores the anterior-posterior axis argument
   and assigns Input/Association/Output by deterministic hash-shuffle at
   30/40/30. Regions are currently not contiguous anatomical lobes.
6. `SpatialGrid::build` derives a uniform AABB grid from neuron positions
   (`DEFAULT_GRID_DIM = 16`). Connectivity, stimulation, and morphology all
   consume this grid or the placed positions.

Current tests already guard determinism, region split, elongated/fissure
envelope, neurons staying inside the smooth envelope, shell bias, spatial-grid
occupancy, and connectivity target range. These tests describe the current
contract, not necessarily the desired realism target.

## Current levers

**Topology and resolution.** `icosphere.rs` owns mesh topology and subdivision.
This guarantees a watertight mesh, but the shape remains star-convex after
radial deformation. A true separated longitudinal cleft, overhang, or underside
fold cannot be represented without changing topology or adding a non-radial
deformation pass.

**Coarse envelope.** `BRAIN_AXES`, `brain_outer_radius`, `ellipsoid_radius`,
`gaussian`, and `smoothstep` are the strongest silhouette levers. They can make
the object read more like a whole brain: stronger bilateral hemispheres, less
football-like symmetry, flatter ventral side, occipital/frontal asymmetry,
temporal lobes, and a clearer dorsal longitudinal fissure. Because placement
uses the same envelope, changes here affect both visible surface and neuron
positions.

**Fold field.** `GyrifyParams` controls fold scale and amplitude but the current
field is isotropic 3D noise along the outward direction. This produces organic
roughness, not anatomically directed sulci. Realism levers include directional
masking, surface-zone masks, explicit sulcus grooves, lower random amplitude in
the fissure/ventral regions, and separating "major sulci" from fine texture.

**Placement.** `place_neurons` can keep the existing smooth shell, move neurons
closer to the folded surface, add hemisphere/lobe density masks, or avoid deep
fissure/ventral voids. This is a product and simulation decision because
placement changes spatial-grid occupancy, local connectivity, stimulation
footprint, and morphology roots/sockets.

**Regions.** `assign_regions` is intentionally spatially random today. Making
regions anatomical would change the visible propagation story and the ambient
input distribution. Treat spatial region realism as a separate decision from
surface realism.

**Render/readability.** `render_manifold.wgsl` is a flat dark optional surface;
the live product visual is mostly neuron glow plus morphology. If surface is
off or too dim, manifold surface realism may only be legible through the neuron
cloud. Surface shading, depth, opacity, and color-mode streams can make a
better shape visible, but they should not be prerequisites for the geometry
pass unless the lead wants a surface-first brain.

**Interaction/framing.** `web/src/main.ts` uses `MANIFOLD_SPHERE_RADIUS = 1.4`
for cursor stimulation hits. `Camera` starts at distance 3.0 and clamps zoom to
`0.5..10.0`. A larger AP extent, deeper folds, or stronger temporal lobes can
break click hit behavior or first-frame composition even when Rust geometry is
valid.

## Strategy options

### Option A: silhouette-first envelope refinement

Keep the current icosphere, star-convex envelope, smooth-shell placement, random
regions, and procedural/no-asset decision. Tune and restructure
`brain_outer_radius` so the object reads as a brain before fine folds are
considered.

Likely changes:

- Refactor the envelope into named anterior/posterior/dorsal/ventral/temporal
  masks so the shape is tunable without a single dense formula.
- Strengthen bilateral hemisphere fullness and midline dorsal indentation while
  ensuring the clamp does not hide intended anatomy.
- Improve frontal/occipital asymmetry and ventral flattening.
- Add tests around measured radii, fissure depth, hemisphere balance, and max
  radius bound.

Pros: lowest risk, easy to gate, preserves all current simulation behavior.
Cons: folds may still read as generic noise, not cortex.

### Option B: structured sulci over isotropic noise

Keep the envelope but add deterministic anatomical fold masks: explicit
longitudinal fissure reinforcement, a few broad sulcal bands, and lower
amplitude random noise where it creates unrealistic spikes.

Likely changes:

- Split `gyrify` into coarse silhouette lumps, named major-groove terms, and
  fine random fold texture.
- Add hemisphere-aware masks from normalized coordinates, but keep no atlas or
  external mesh.
- Gate fold amplitude so sulci do not put the mesh outside the expected
  interaction/framing bounds.

Pros: much better cortical read without changing placement/regions. Cons:
radial grooves can look like surface scratches unless paired with surface
visibility/shading or neuron placement that hints at fold depth.

### Option C: folded-shell-aware neuron placement

Place neurons relative to the post-gyrification or matching fold field instead
of only the smooth envelope, so the neuron cloud follows sulcal/gyri depth.

Likely changes:

- Expose a deterministic `fold_radius(dir, seed, params)` helper shared by
  surface generation and placement.
- Keep neurons inside the folded outer radius with a cortical shell depth
  distribution.
- Decide whether sulci are dense surfaces, grooves with fewer neurons, or only
  visual indentations.

Pros: neuron cloud reinforces the new brain shape even with the surface pass
dim/off. Cons: changes spatial-grid occupancy, connectivity locality,
stimulation density, and morphology geometry; requires stronger gates.

### Option D: anatomical region placement

Make Input/Association/Output spatially contiguous or lobe-like along the
anterior-posterior axis or named masks.

Pros: could make activity propagation read as sensory-to-association-to-output
across the visible brain. Cons: intentionally changes current behavior; ambient
drive would become spatially clustered and could dominate the product feel.
This should not be bundled into the first geometry pass without lead approval.

### Option E: external anatomical mesh

Replace or guide the procedural mesh with an imported brain asset.

Pros: highest anatomical fidelity. Cons: conflicts with the current
asset-free/deterministic decision, adds licensing/bundling/normalization work,
and does not automatically solve neuron placement or regions. Defer unless the
lead explicitly changes the product constraint.

## Recommended first pass

Choose **Option A plus Option B plus a scoped version of Option C**:

- Preserve procedural/no-asset generation, icosphere topology, hash-shuffled
  regions, and existing public settings.
- Improve the coarse envelope first: clearer hemispheres, more believable
  frontal/occipital proportions, temporal fullness, ventral flattening, and a
  stable dorsal longitudinal fissure.
- Add a small number of structured major-groove terms in `gyrify` or a new
  helper.
- Make neuron placement follow the same folded radius/helper so the neuron cloud
  reinforces the surface shape. Keep region assignment hash-shuffled.

This is now a broader first pass than the initial recommendation. It should ship
with stronger gates because folded placement can affect spatial-grid occupancy,
connectivity locality, stimulation density, and morphology roots.

Lead decision on 2026-06-09: the realism target is all three of the obvious
axes: a more recognizable whole-brain silhouette, clearer cortical
folding/gyrification, and stronger hemispheres/sulci/fissure. Treat those as
first-pass acceptance criteria.

Follow-up lead decision on 2026-06-09: neuron placement should follow folds in
this realism pass. The implementation must therefore gate placement, spatial
grid occupancy, connectivity locality, stimulation density, and morphology
effects instead of treating the work as surface-only.

Second follow-up lead decision on 2026-06-09: regions can remain random /
hash-shuffled in this pass. Do not bundle anatomical region placement with the
shape/fold/placement work.

## Implementation streams

### Stream 1: product target and references

Owned files: this plan only until implementation starts.

Decide what realism means before tuning code. The implementation worker should
collect two to five visual references or lead-approved descriptors and convert
them into measurable acceptance points:

- Whole-brain silhouette from the initial camera angle.
- Dorsal/top read with two hemispheres and a longitudinal fissure.
- Side read with flatter underside and plausible frontal/occipital proportion.
- Fold density: subtle cortex texture vs visibly deep sulci.
- Whether the surface mesh must be visible by default or the neuron cloud alone
  is allowed to carry the shape.

Narrow gate: written acceptance bullets approved in this plan or the hub before
code changes begin.

### Stream 2: envelope refinement

Likely owned files:

- `app/crates/brain-visualizer/src/manifold/mod.rs`
- New per-feature tests in the same module or a focused manifold test file if
  the generator is split.

Work:

1. Refactor `brain_outer_radius` into named mask helpers only as much as needed
   to make tuning reviewable.
2. Tune `BRAIN_AXES` and envelope terms against the chosen product target.
3. Keep a single shared envelope for surface and placement.
4. Add/adjust host tests for AP/lateral/dorsal ratios, fissure contrast,
   ventral flattening, max radius, and no escaped neurons.
5. Record any accepted silhouette trade-offs in `decisions/manifold.md` at
   migration time.

Narrow gates:

- `cd app && cargo test -p brain-visualizer manifold::`
- A small generated metrics artifact or logged table of sampled directional
  radii for top/side/front/rear/fissure directions.

### Stream 3: structured fold field

Likely owned files:

- `app/crates/brain-visualizer/src/manifold/gyrify.rs`
- Possibly `app/crates/brain-visualizer/src/manifold/mod.rs` if major-groove
  masks need envelope coordinates.
- Focused tests for deterministic fold bounds.

Work:

1. Fix stale comments so they say three octaves or the new field structure.
2. Separate random texture from named major-groove terms.
3. Add deterministic, coordinate-derived grooves only where the product target
   needs them; avoid a large atlas-like grammar in the first pass.
4. Keep fold displacement bounded relative to local envelope radius.
5. Decide whether `GyrifyParams` stays the only public config or whether new
   protected constants are clearer. Do not expose new UI controls in this pass
   unless the settings stream explicitly asks for them.

Narrow gates:

- `cd app && cargo test -p brain-visualizer manifold::gyrify`
- Host assertion that folded radii stay within the approved min/max band across
  a fixed seed set.
- Visual artifact from the existing native render harness or a new focused
  manifold artifact, not a full browser e2e.

### Stream 4: placement and spatial grid audit

Likely owned files:

- `app/crates/brain-visualizer/src/manifold/mod.rs`
- Possibly `app/crates/brain-visualizer/src/manifold/gyrify.rs` for shared
  fold-radius helpers.
- Focused tests around shell ratio, grid occupancy, and connectivity target
  range.

Work:

1. Create one shared deterministic fold-radius path so
   surface and placement cannot drift.
2. Update `place_neurons` so cortical-shell placement follows the folded
   envelope while preserving deterministic seed behavior and intentional shell
   depth distribution.
3. Re-check spatial-grid occupancy and max cell occupancy; deeper fissures or
   lobes can create sparse cells and clumps.
4. Re-check connectivity locality and stimulation radius against the new density
   distribution.

Narrow gates:

- `cd app && cargo test -p brain-visualizer placement_is_cortical_shell_biased shell_bias_still_populates_spatial_grid connectivity_rule_remains_deterministic_and_in_range`
- Add one new per-feature test proving neurons stay inside the folded envelope
  and shell depth distribution remains intentional.

### Stream 5: region non-change

Likely owned files:

- None expected for implementation. `regions.rs` should remain untouched unless
  implementation discovers a test fixture needs a comment update.

Work:

1. Keep hash-shuffled regions for this pass.
2. Do not make Input/Association/Output spatial/anatomical as part of the shape
   realism stream.
3. If anatomical regions are wanted later, plan them as a separate behavior
   change with propagation and ambient-input acceptance criteria.

Narrow gates:

- `cd app && cargo test -p brain-visualizer region_split_approx_30_40_30`

### Stream 6: render and interaction audit

Likely owned files if geometry changes expose issues:

- `app/crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`
- `app/crates/brain-visualizer/src/sim/gpu/resources.rs`
- `app/crates/brain-visualizer/src/sim/gpu/mod.rs`
- `app/web/src/main.ts`
- `app/web/src/render/camera.ts`

Work:

1. Measure the new maximum radius across generated surface vertices and neuron
   positions. Keep it under the current `MANIFOLD_SPHERE_RADIUS = 1.4`, or
   deliberately update the interaction radius.
2. Verify first-frame composition at the current camera distance 3.0.
3. Decide whether the existing flat optional surface is enough for the new
   folds. If folds need lighting to read, coordinate with the rendering/color
   streams rather than hiding geometry risk inside the manifold pass.
4. Keep render changes out of the first geometry PR unless the improved shape is
   invisible without them.

Narrow gates:

- A fixed-seed visual artifact from at least front/side/top or equivalent
  camera views.
- Manual/browser smoke only for first-frame framing and cursor stimulation if
  radius/camera constants change.
- No full Playwright suite per stream.

## Artifacts

Prefer small, repeatable artifacts over broad gates:

- Directional radius table for `seed=1` and one alternate seed:
  AP, anterior, posterior, lateral, dorsal, ventral, temporal, fissure-midline.
- Surface/neuron max-radius summary proving interaction bounds are known.
- 3-view visual capture: first camera, dorsal/top-ish, lateral/side-ish.
- Folded-placement grid summary: occupied cells, max occupancy, and shell-depth
  histogram.

`examples/morph_view.rs` may be reused if it already captures enough of the
full visual. If not, add a focused manifold artifact harness during
implementation rather than overloading morphology acceptance.

## Lead questions

1. Is "realistic" primarily the whole-brain silhouette, cortical folds, clear
   hemispheres/fissure, anatomical region placement, or all of these?
2. Should the initial homepage view show a surface-first brain, or is it
   acceptable for the neuron/morphology cloud to carry most of the shape?
3. Should neurons follow sulci/gyri depth, or should folds remain a surface
   context layer around the smooth cortical shell?
4. Should Input/Association/Output stay spatially random, or should they become
   anatomical/lobe-like even if this changes propagation behavior?
5. Are external anatomical mesh assets still off the table?
6. What is the maximum acceptable stylization: biologically plausible cortex, or
   a recognizable artistic brain sculpture?

## Sequencing and collisions

- Run Stream 1 before code. The rest depends on those product answers.
- Stream 2 and Stream 3 both touch manifold shape and should be sequenced by one
  implementation owner or merged as one small geometry pass.
- Stream 4 must run after any envelope/fold changes because grid occupancy and
  connectivity depend on placement.
- Stream 5 is a separate behavior decision; do not combine it with the first
  silhouette/fold pass.
- Stream 6 runs after geometry changes, unless a render artifact proves the
  shape cannot be judged without surface shading changes.
- Per the polish hub, avoid concurrent work that also changes shared render,
  settings, or morphology files. This plan should not touch
  `app/crates/brain-visualizer/src/sim/morphology.rs` unless folded placement
  creates a confirmed morphology attachment defect.

## Exit gate

- Product target answered well enough to choose among Options A-E.
- The accepted implementation pass has a clear owned-file set and no ambiguous
  overlap with active settings/color/dendrite/defaults streams.
- Narrow host tests pass for changed manifold behavior.
- Fixed-seed visual artifacts show the shape from at least three useful views.
- Interaction/framing bounds are known; if changed, cursor stimulation and first
  camera view are manually smoked.
- At ship time, current-state facts migrate to `architecture/manifold.md`,
  render/interaction facts migrate to `architecture/gpu-rendering.md` and
  `architecture/web-frontend.md` if touched, and durable trade-offs migrate to
  `decisions/manifold.md`, `decisions/rendering.md`, or
  `decisions/interaction.md`.

## Deferrals

- External mesh import and licensing.
- Anatomical atlas/region mapping.
- True non-star-convex cortical topology or a separated hemisphere mesh.
- UI controls for manifold shape tuning.
- Full-suite gates per stream; reserve broad drift gates for the consolidated
  implementation wave.

## Migration notes

Migrated on 2026-06-11 into `architecture/manifold.md`,
`architecture/gpu-rendering.md`, and `decisions/manifold.md`. Current-state docs
record the refined envelope, shared deterministic `FoldField`, structured major
grooves, folded neuron placement, hash-random regions, retained star-convex
radial topology, and verification metrics: `dorsal_mid=0.4303`,
`fissure_mid=0.4545`, `max_surface=1.2407`, `max_neuron=1.2389`,
`occupied_cells=1409`, and `max_cell_occupancy=43`.

`okay_to_delete` remains `false` only because the visual-product-polish hub is
retaining all six stream plans until the real-WebGPU browser smoke blocker is
cleared or waived.

## See also

- `docs/plans/visual-product-polish-phase-hub.md`
- `docs/architecture/manifold.md`
- `docs/decisions/manifold.md`
