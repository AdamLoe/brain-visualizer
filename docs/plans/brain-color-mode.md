---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - architecture/gpu-rendering.md
  - architecture/web-frontend.md
  - decisions/dev-tooling.md
  - decisions/rendering.md
---

# Brain color mode

> User goal from the visual product polish phase: add `Color by = Brain` where
> inactive neurons and everything else are pink, while firing neurons,
> firing connections, and active morphology segments are blue.

## Mission

Add a new live color mode that reads as a single brain-themed activity view:
resting structure is pink and current firing/activity is blue. Done when the
Rendering tab exposes `Color by = Brain`, the TypeScript -> Rust
`VisualSettings` contract still sends `colorBy` through index 18 without
renumbering, and every live render path that contributes visible color honors
the same pink/resting and blue/active semantics.

## Grounding

Authoritative code paths checked on 2026-06-09:

- `web/src/core/settings.ts`: `colorBy` is already a persisted public setting
  and serializes through Float32Array index 18. `SETTINGS_LENGTH` is 26; do not
  grow or renumber the layout for this work.
- `web/src/ui/dev-panel.ts`: the Rendering tab hard-codes `Color by` options
  0..5 and the Debug View hard-codes matching labels. `colorBy` is marked live
  by `web/src/core/setting-metadata.ts`.
- `crates/brain-visualizer/src/sim/gpu/mod.rs`: `VisualSettings::from_slice`
  ingests index 18 into `visual.color_by`; `render_full` writes that value into
  `RenderUniforms.color_by` for `render_far.wgsl` and `MorphUniforms.color_by`
  for `render_morphology.wgsl`.
- `render_far.wgsl`: live neuron billboards use `color_for(mode, ..., glow)`,
  but the resting contribution is currently hard-coded gray in `fs_main`. Brain
  mode must change both active color and resting color here.
- `render_morphology.wgsl`: live morphology tubes and soma spheres use
  `branch_base_color` / `soma_base_color`, then material/lighting functions and
  the additive + true-opacity active passes. Brain mode must override both the
  inactive material tint and the active packet/soma tint.
- `render_manifold.wgsl`: optional surface pass has its own dark tint and no
  current `color_by` field. Its 16-byte uniform tail has padding that can carry
  `color_by` without changing the struct size.
- Retired color paths still exist in `render_ribbon.wgsl`,
  `render_cylinder.wgsl`, and `render_sphere.wgsl`, but `DRAW_LEGACY_RIBBONS`,
  `DRAW_LEGACY_CYLINDERS`, and `DRAW_LEGACY_NEAR_SPHERES` are false in
  `gpu/mod.rs`. They are not implementation targets unless those debug flags
  are intentionally revived.

## Exact semantic proposal

Use `colorBy = 6` for `Brain`.

Lead decision on 2026-06-09: Brain should become the default selected color
mode once implemented, not only a new option.

Follow-up lead decision on 2026-06-09: the traveling active segment and firing
core should be the light-blue focus. Other active-adjacent parts may be bluish,
but the whole connection should not become uniformly blue for its full glow
lifetime.

Second follow-up lead decision on 2026-06-09: Brain mode respects `surface =
Off`; selecting Brain must not force the optional manifold/context surface on.

Palette:

- `BRAIN_REST_PINK = vec3(1.0, 0.18, 0.54)`
- `BRAIN_ACTIVE_BLUE = vec3(0.08, 0.56, 1.0)` as the starting point; tune toward
  a light-blue core and softer bluish halo during visual acceptance if needed.

Mode semantics:

- Brain mode is a selected `Color by` mode only. It does not globally override
  Region, E/I, Spike age, Voltage, Activity, or Identity.
- Brain mode does not force hidden/off layers on. `connectionLayer = Off`,
  `surface = Off`, and `neuronVisibility = Active only` still behave as their
  own settings specify.
- Inactive/resting visible neurons are pink. In `render_far.wgsl`, this means
  the resting billboard contribution must use pink instead of the current gray
  when `u.color_by == 6u`.
- Firing neuron cores are light blue. In `render_far.wgsl`, spike, flash, and
  core center should use the active blue; surrounding glow can be bluish rather
  than fully saturated so the core remains the brightest activity cue.
- Resting morphology tubes and soma spheres are pink. This includes dendrites,
  axons, branch kind `0`, and any inactive soma sphere material; the existing
  hard-coded dendrite blue in `branch_base_color(kind == 0u)` must not survive
  Brain mode.
- Active morphology uses blue on the traveling active packet/segment using the
  shader's existing activity sources (`impulse_packet` and
  `impulse_segment_activity`). The rest of the same connection may pick up a
  weaker bluish cast from existing glow/activity math, but it should not turn
  uniformly blue for the whole glow lifetime. Soma spheres use a light-blue core
  for current firing energy (`glow + flash + core`) with a softer bluish
  surround.
- The optional manifold surface is pink when shown in Brain mode. When surface
  is off, no surface is drawn.
- Bloom remains downstream of color and should bloom the resulting blue/pink
  brightness; do not add a bloom-specific Brain branch.

## Implementation plan

### Stream 1 — UI and settings contract

Owned files:

- `web/src/core/settings.ts`
- `web/src/ui/dev-panel.ts`
- `web/src/core/setting-metadata.ts` only if comments/metadata need the new
  label clarified
- New per-feature test file under `web/src/ui/`, e.g.
  `brain-color-mode.test.ts`

Steps:

1. Add option `{ value: 6, label: "Brain" }` to the Rendering tab `Color by`
   selector.
2. Add `"Brain"` to the Debug View `COLOR_BY_LABELS`.
3. Keep `SETTINGS_LENGTH = 26`; keep `colorBy` at index 18; keep persistence
   schema version unless implementation discovers old saved values need a
   deliberate reset. Existing saved values 0..5 remain valid, and saved value 6
   should round-trip naturally.
4. Update comments that enumerate color modes in `settings.ts` if touched.
5. Add focused Vitest coverage that checks the selector/debug label inventory
   or extracted label helper if the implementation extracts one. Also assert
   `toFloat32Array({ ...DEFAULT_SETTINGS, colorBy: 6 })[18] === 6`.

Gate:

- From `app/web`: `npm test -- --run src/ui/brain-color-mode.test.ts` or the
  narrow equivalent if the test lands beside existing dev-panel tests.
- From `app/web`: `npm run typecheck`.

### Stream 2 — Render-uniform threading

Owned files:

- `crates/brain-visualizer/src/sim/gpu/mod.rs`
- `crates/brain-visualizer/src/sim/gpu/resources.rs`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`

Steps:

1. No ingestion shape change for `VisualSettings::from_slice`; only update
   enum comments / JSON-review docs to include mode `6=brain`.
2. `RenderUniforms` and `MorphUniforms` already carry `color_by`; update their
   comments to include mode 6.
3. Repurpose one `ManifoldUniforms` padding slot to `color_by: u32`, and mirror
   that in `render_manifold.wgsl`'s `Uniforms`. The size should remain one
   mat4 plus one 16-byte tail block.
4. In `render_full`, populate the manifold uniform's new `color_by` field with
   `self.visual.color_by`.

Gate:

- From `app`: `cargo test -p brain-visualizer sim::gpu::resources::tests::render_uniform_size_aligned`.
- If the exact module path differs, run the narrow `resources.rs` layout test
  target that covers `RenderUniforms` / `ManifoldUniforms` alignment.

### Stream 3 — Shader color paths

Owned files:

- `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`

Steps:

1. In each live shader, define shared local constants for the exact palette
   values above. Keep them shader-local unless a shader include mechanism
   already exists; do not introduce a new shader-preprocessing system for two
   constants.
2. `render_far.wgsl`: add mode 6 to `color_for` so active neuron color is blue.
   In `fs_main`, branch the resting contribution on `u.color_by == 6u` so
   resting/inactive neurons are pink instead of gray. Keep opacity and
   visibility math unchanged.
3. `render_morphology.wgsl`: make `branch_base_color` and `soma_base_color`
   return pink for mode 6 before normal region/E/I/kind handling. Then branch
   the tube and soma fragment color composition so active packet/soma activity
   mixes toward `BRAIN_ACTIVE_BLUE`, rather than white or identity/region
   material. Use the existing packet/segment/soma activity scalars; do not add
   new firing-state buffers.
4. Keep the true-opacity active passes color-matched to the additive passes.
   Their alpha/depth behavior is not part of this feature and should remain
   unchanged.
5. `render_manifold.wgsl`: when `u.color_by == 6u`, replace the dark surface
   base tint with pink before applying `surface_opacity` / `surface_mode`.
6. Do not update `render_ribbon.wgsl`, `render_cylinder.wgsl`, or
   `render_sphere.wgsl` in this stream unless implementation first flips the
   corresponding `DRAW_LEGACY_*` flag to true for product use.

Gate:

- From `app`: `cargo test -p brain-visualizer sim::gpu::pipelines::tests::render_shaders_present`.
- A focused render artifact/manual check: run the existing lightweight render
  check or `morph_view` capture with `colorBy=6`, then inspect that inactive
  visible structure is pink and active packets/somas are blue. Store any
  generated artifact outside docs unless a later implementation task explicitly
  asks to check it in.

## Ownership and sequencing

Likely implementation owner should take these files together because the UI
enum and shader enum must land atomically:

- `web/src/core/settings.ts`
- `web/src/ui/dev-panel.ts`
- `web/src/ui/brain-color-mode.test.ts` or equivalent per-feature test
- `crates/brain-visualizer/src/sim/gpu/mod.rs`
- `crates/brain-visualizer/src/sim/gpu/resources.rs`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_far.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl`
- `crates/brain-visualizer/src/sim/gpu/shaders/render_manifold.wgsl`

Sequencing constraints:

- Do not run concurrently with any stream editing
  `render_morphology.wgsl`; the visual polish hub already marks that shader as
  single-owner.
- Coordinate with the settings overhaul stream before touching
  `settings.ts` / `dev-panel.ts`, because that stream may also restructure the
  color selector and Debug View labels.
- Coordinate with active-opacity work before touching
  `render_morphology.wgsl`, because Brain mode must preserve active-pass alpha
  semantics while changing hue.

## Docs and tests impacted

Docs to update when implementation ships:

- `architecture/dev-panel.md`: add `Color by = Brain` to the setting contract
  / UI description.
- `architecture/web-frontend.md`: mention the new public color mode only if it
  documents the current settings surface.
- `architecture/gpu-rendering.md`: document the Brain color branch across
  far-glow, morphology tubes/somas, and optional surface.
- `decisions/rendering.md`: record the pink/resting and blue/activity semantic
  if it becomes an accepted product decision.
- `decisions/dev-tooling.md`: update only if the UI enum/persistence policy is
  discussed there.

Tests to add or adjust:

- New focused Vitest test for `colorBy=6` serialization and UI label coverage.
- Existing Rust layout tests should continue to pass; add a size assertion only
  if `ManifoldUniforms` changes in a way not already covered.
- Existing shader-presence tests should be sufficient for compile-time inclusion
  of WGSL files; the color behavior itself needs a visual artifact/manual check
  unless a small shader-string invariant test is judged useful.

## Lead questions

1. Is the proposed palette saturated enough, or should the implementation use a
   softer anatomical pink and a less cyan electrical blue?

## Deferrals

- No source implementation in this planning task.
- No `VisualSettings` layout growth, renumbering, or persistence migration.
- No changes to parked CPU/WebGL renderer paths.
- No changes to retired ribbon, cylinder, or near-sphere shaders unless those
  paths are deliberately revived.
- No change to simulation activity semantics, spike packing, glow decay, or
  active-opacity alpha/depth behavior.
- No broad full-suite gate per stream; run only the narrow gates above during
  implementation, with the visual polish hub owning any later consolidated gate.

## Exit gate

- `Color by = Brain` is visible in the Rendering tab and Debug View.
- `colorBy=6` round-trips through persistence and Float32Array index 18.
- Far neuron billboards, morphology tubes/somas, and optional surface all obey
  pink-resting / blue-active semantics.
- A targeted capture or manual browser check confirms the visible color
  behavior with active firing.
- Durable docs listed above are updated before this plan is marked shipped.

## Migration Notes

Migrated on 2026-06-11 into `architecture/dev-panel.md`,
`architecture/gpu-rendering.md`, and `decisions/rendering.md`. Current-state
docs record `colorBy = 6`, Brain as the default, pink resting/inactive
structure, blue firing cores/active packets, preserved index 18, optional
surface tinting, and the rule that Brain mode respects `surface = Off`.

`okay_to_delete` remains `false` only because the visual-product-polish hub is
retaining all six stream plans until the real-WebGPU browser smoke blocker is
cleared or waived.
