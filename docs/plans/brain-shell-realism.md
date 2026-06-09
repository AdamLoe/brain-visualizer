---
status:        draft
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - decisions/manifold.md
---

# Brain shell realism

## Mission

Make the procedural brain shell read as more anatomically convincing: **lumpier
outer surface** and a **bigger split down the middle** (a deeper longitudinal
fissure between the hemispheres). Done when the silhouette and surface look more
brain-like under owner review, the neuron cloud still follows the shell with no
drift, and `architecture/manifold.md` reflects the tuned envelope/gyrification.

## Scope

**In scope** — the host-side envelope and noise shaping:

- **Lumpier outside:** increase gyrification amplitude and/or add a lower-
  frequency "lumpiness" octave in
  `crates/brain-visualizer/src/manifold/gyrify.rs → gyrify` / `GyrifyParams`.
- **Bigger middle split:** strengthen the dorsal-midline indentation term (the
  longitudinal fissure) in
  `crates/brain-visualizer/src/manifold/mod.rs → brain_outer_radius`.

**Out of scope** — any external mesh asset (stays deferred in
`future_roadmap.md`); region geography changes; neuron placement strategy beyond
what automatically follows the envelope.

## Locked design decisions

- **Procedural, asset-free.** Tune the existing envelope + OpenSimplex octaves;
  do not introduce a mesh asset or texture pipeline.
- **Shared envelope is the safety net.** Both the surface mesh and
  `place_neurons` consume the same `brain_outer_radius`, so deepening the
  fissure and adding lumpiness automatically moves the neuron cloud too — the
  two views cannot drift. Keep it that way; do not fork a separate placement
  envelope.

## Approach

Small, iterative, visual-acceptance-driven:

1. Deepen the dorsal midline indentation in `brain_outer_radius`; tune width and
   depth so the hemispheres separate convincingly without pinching the volume in
   half.
2. Add lumpiness: either raise `gyri_amp` / add a coarse low-freq octave for
   large bulges, keeping `sulci_*` for fine folds. Watch that neurons still sit
   in the cortical shell band, not floating off the displaced surface.
3. Iterate against owner review at default scale; expose any new knob through
   the existing manifold params if live tuning helps.

## Exit gate

- `cargo test` green (icosphere vertex-count formula test, region-split test
  unaffected).
- Owner visual-acceptance review: lumpier surface + deeper central fissure read
  as more brain-like; neuron cloud still hugs the shell.
- `architecture/manifold.md` (surface generation pipeline) reflects any changed
  `GyrifyParams` defaults or the strengthened fissure term; update
  `decisions/manifold.md` only if the rationale for procedural shaping shifts.

## Migration notes (filled in at ship time)

Route changed `GyrifyParams` defaults and the fissure-term change into
`architecture/manifold.md → Surface generation pipeline`. Per the manifest
drift-verification list, `GyrifyParams` defaults are a tracked surface — keep the
doc's stated frequencies/amplitudes accurate.

## Implementation detail

Concrete, code-grounded recipe for the implementing agent. All paths are under
`app/`; cargo runs from `app/`. The two edit targets are
`crates/brain-visualizer/src/manifold/mod.rs → brain_outer_radius` (fissure) and
`crates/brain-visualizer/src/manifold/gyrify.rs → GyrifyParams, gyrify` (lumpiness).
Both are pure host functions consumed by `Manifold::generate`; nothing GPU/WASM
needs to change.

### Cross-language / determinism contracts to keep intact

- **No binary layout touched.** Neither `brain_outer_radius` nor `gyrify` feeds a
  Rust↔WGSL struct. The drift-verification surfaces in `_meta/manifest.md`
  (`MorphSegment`, `MorphSphereInstance`, `MorphUniforms`, `VisualSettings`,
  hash constants) are all out of scope — do not edit any `.wgsl` or
  `resources.rs`/`morphology.rs` layout. The change is host-side geometry only.
- **Single shared envelope.** `brain_outer_radius` is the locked safety net: it
  is called by `brain_surface_point` (mesh, via `Manifold::generate` line ~93)
  AND by `place_neurons` (line ~159). Make the fissure change *only* inside
  `brain_outer_radius` so the mesh and the neuron cloud move together. Do **not**
  add a placement-only fissure term in `place_neurons`.
- **Determinism namespace.** `gyrify` derives its two noise seeds as `seed` and
  `seed.wrapping_add(0x9e37_79b9)`. A third (lumpiness) octave, if added, MUST
  use a *new* distinct constant offset (e.g. `seed.wrapping_add(0x85eb_ca6b)`,
  a different odd 32-bit constant) so the three fields decorrelate and stay
  reproducible. Do not reuse the existing two seeds. This keeps the
  `gyrify::deterministic_for_seed` and `mod.rs::deterministic` tests green.
- **`gyrify` domain note.** `gyrify` calls `normalize(p)` and samples noise in
  the *unit-direction* domain, restoring magnitude via
  `base_radius = length(p)`. The new lumpiness octave must follow the same
  pattern — sample at `n * lump_freq`, scale displacement by
  `base_radius * lump_amp` — so it composes with the brain envelope identically
  to the existing octaves (works for both a raw sphere and the shaped shell).

### Edit order (small, reversible, test after each)

**Step 1 — Deepen the longitudinal fissure (`brain_outer_radius`).**
The current term (mod.rs ~line 129) is:
```
let fissure =
    0.22 * gaussian(x.abs(), 0.00, 0.18) * smoothstep(-0.05, 0.85, y) * gaussian(z, 0.10, 0.65);
```
This is subtracted from `scale`. To make the split bigger and read more
convincingly:
- Increase the **depth** coefficient (`0.22`) toward ~`0.30–0.34` for a deeper
  cleft. Stay below the point where the `scale` `.clamp(0.55, 1.35)` floor
  saturates at the midline — verify the clamp is not pinning by checking
  `brain_outer_radius(normalize([0.0, 1.0, 0.0]))` stays comfortably above
  `0.55 * ellipsoid`. The mission explicitly warns: separate the hemispheres
  "without pinching the volume in half," so do not let the midline radius
  collapse to the clamp floor.
- Optionally **narrow** the `x.abs()` gaussian sigma (`0.18` → ~`0.14–0.16`) so
  the fissure is a tighter cleft rather than a broad dish, and/or **extend** its
  anterior–posterior reach by widening the `gaussian(z, 0.10, 0.65)` sigma so
  the split runs more of the length of the dorsal surface.
- The existing test `mod.rs::envelope_is_elongated_with_midline_fissure` asserts
  `lateral_top > midline_top + 0.08`. Deepening the fissure *increases* this
  gap, so the test still passes; consider tightening the `+ 0.08` margin upward
  (e.g. `+ 0.12`) to lock in the new, deeper fissure as a regression guard.
  Leave the `ap > dorsal` / `ap > lateral` asserts untouched.
- Re-check `neurons_stay_inside_brain_envelope` and
  `placement_is_cortical_shell_biased`: because placement multiplies the same
  `outer_radius` by `depth`, a deeper fissure shrinks the local envelope at the
  midline but does not change the *ratio* `shell_depth_ratio`, so both tests
  should remain green. Run them to confirm rather than assume.

**Step 2 — Add lumpiness (`gyrify` / `GyrifyParams`).**
Pick ONE of two approaches; the third-octave approach is preferred because it
adds large bulges without inflating the fine-fold band:
- *Preferred — add a coarse low-freq octave.* Add two fields to `GyrifyParams`:
  `lump_freq: f64` (≈ `0.8`, below `gyri_freq = 1.5`) and `lump_amp: f32`
  (≈ `0.10–0.14`). In `Default`, set them. In `gyrify`, construct a third
  `OpenSimplex::new(seed.wrapping_add(0x85eb_ca6b))`, sample at `n * lump_freq`,
  and add `l * params.lump_amp * params.radius` into the `radius` expression
  alongside the existing `g` and `s` terms. This gives big slow bulges (overall
  lumpier silhouette) while `sulci_*` keeps the fine folds.
- *Alternative — just raise `gyri_amp`.* Bump `gyri_amp` from `0.15` toward
  `0.18–0.22`. Simpler, but coarsens at the existing `gyri_freq` and risks
  reading as noisier folds rather than anatomical lumps; prefer the new octave.

**Step 3 — Update the gyrify amplitude-band test.** The test
`gyrify::produces_folds_not_a_smooth_sphere` asserts `min > 0.70 && max < 1.30`
(i.e. total amplitude ≤ `0.30`). If you add a `lump_amp` octave or raise
`gyri_amp` such that `gyri_amp + sulci_amp + lump_amp` exceeds `0.30`, this test
will fail at its band check. Widen the band assertion to match the new
worst-case envelope (`1 ± (gyri_amp + sulci_amp + lump_amp) + eps`) and update
the inline comment that currently reads `(0.15+0.05)`. Keep the
`max - min > 0.05` "not smooth" assertion as-is. This is the only existing test
that hard-codes the amplitude defaults.

**Step 4 — Iterate visually.** Run the native render harness to eyeball the
silhouette (see gate commands). Tune the four numbers (fissure depth/width,
`lump_amp`/`lump_freq`) against owner review at default scale. If live tuning is
wanted, the scope allows surfacing a new knob through existing manifold params,
but `GyrifyParams` is not currently wired to the dev-panel `MorphologyConfig`
(that channel is morphology-only per `architecture/dev-panel.md`), so prefer
leaving the new fields as compile-time defaults unless the owner asks for a
slider — adding a UI channel is a larger change outside this plan's scope.

### Gate commands (run from `app/`)

```
# Fast inner loop — manifold host tests only:
cargo test -p brain-visualizer manifold

# Full host gate (determinism + sim dynamics, llvmpipe headless):
cargo test -p brain-visualizer

# Native render smoke (offscreen render + morphology + bloom):
cargo run -p brain-visualizer --example render_check
```
The exit gate's `cargo test` is satisfied by `cargo test -p brain-visualizer`.
The icosphere vertex-count formula test and the `region_split_approx_30_40_30`
test are unaffected by either edit (neither touches `icosphere` or
`assign_regions`). No `npm` gate is required — no `web/` or WGSL file changes.

### Docs to update at ship time (per `_meta/manifest.md` change-to-doc)

Editing `manifold/*` requires updating `architecture/manifold.md` and
`decisions/manifold.md`:
- `architecture/manifold.md → Surface generation pipeline` step 2
  (envelope/fissure) and step 3 (gyrification): update the stated fissure
  strength and the gyri/sulci/lump frequencies+amplitudes. `GyrifyParams`
  defaults are a tracked drift-verification surface — keep the numbers exact.
  If a `lump_*` octave is added, the "two octaves of OpenSimplex" wording in
  step 3 (and the gyrify.rs module doc-comment at the top of the file) becomes
  "three octaves" — update both.
- `decisions/manifold.md`: only if the procedural-shaping rationale shifts. A
  pure retune (deeper fissure, extra octave, same asset-free approach) does NOT
  change the decision, so likely leave it untouched and note that in the
  migration commit.

## See also

- The app's [`index.md`](index.md) — where live plans land.
- [`../architecture/manifold.md`](../architecture/manifold.md) — envelope shaping + gyrification pipeline.
- [`../decisions/manifold.md`](../decisions/manifold.md) — procedural-over-mesh rationale.
- [`future_roadmap.md`](future_roadmap.md) — real mesh asset stays deferred.
