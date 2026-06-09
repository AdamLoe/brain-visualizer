---
status:        draft
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - decisions/rendering.md
---

# True opacity for active geometry

## Mission

Let **active (firing) neurons and their connections render as genuinely opaque**,
while inactive structure stays extremely see-through or invisible. Today the
whole morphology + glow path is additive blend with no depth write, which can
only make things *brighter*, never *solid* — so everything reads as uniformly
translucent, which the owner finds muddy. This plan introduces a depth-tested,
alpha-blended path for active geometry layered on top of the additive resting
layer, and skips drawing geometry whose opacity is effectively zero. Done when
active neurons visibly occlude what's behind them, the resting look is
unchanged-or-cleaner, and `architecture/gpu-rendering.md` + `decisions/rendering.md`
document the new pass model.

## Scope

**In scope:**

- A new **depth-tested, alpha-blended render path** for active geometry (soma
  spheres + morphology tubes), keyed off the same `last_spike` activity timing
  the rest of the renderer already uses ("active" = firing — **not** click
  selection; see Locked decisions).
- An opacity model where inactive geometry can go to near-zero / fully hidden
  and active geometry can reach **opacity 1**, as a config option.
- **Skip-draw at opacity 0** (bullet 6): when a layer's opacity is effectively
  zero, skip the pass on the CPU side (the same pattern the surface pass already
  uses), rather than drawing fully-transparent geometry.
- Resolving the interaction with bloom/HDR and the additive resting layer:
  draw order, depth buffer introduction, and how an opaque active pass composes
  with the soft-additive glow.

**Out of scope:** GPU picking / click-to-select (stays deferred per
`future_roadmap.md`); per-instance GPU culling of individual zero-opacity
neurons (the skip is at the *pass/layer* granularity unless an implementer finds
per-instance is cheap and clearly worth it); any `MorphSegment`/`MorphUniforms`
layout change beyond what the opacity model strictly needs.

## Locked design decisions

- **"Selected" = active/firing, no picking.** Bullet 7's "opaque if selected"
  is interpreted as *active*. This keeps the plan self-contained and avoids
  reviving the deferred picking subsystem. Design the opacity pass so a
  click-selected set *could* feed it later, but do not build picking here.
- **True opacity, not brighter-additive.** A genuine depth + alpha path was
  chosen over faking opacity with brightness, because additive blending
  physically cannot occlude. This is the source of the plan's risk: it breaks
  the renderer's current no-depth assumption.

## Approach

This is the **highest-risk** item of the four — additive-no-depth is baked into
the current pass order. Treat it as a real rendering design pass, not a quick
config edit.

1. **Decide the layering.** Most likely: keep the additive resting/glow layer
   as-is for the translucent background of inactive structure, and add a
   separate depth-tested alpha pass for active geometry that writes/tests depth
   so it occludes. Work out the pass order in `GpuBackend::render_full` and how
   it interacts with the existing bloom/HDR offscreen target.
2. **Opacity controls.** Add the inactive-opacity floor and active-opacity
   ceiling (default toward 1.0) as `VisualSettings` / `MorphologyConfig` knobs —
   check the `VisualSettings` Float32Array index contract carefully (it is a
   locked Rust ↔ TS contract).
3. **Skip-at-zero guards.** CPU-side pass skips mirroring the existing `surface`
   and `connection_layer` guards.
4. **Validate** on llvmpipe via `examples/render_check.rs` (extend it to assert
   an active neuron occludes geometry behind it).

## Exit gate

- `examples/render_check.rs` (extended) proves active geometry is opaque
  (occludes background) and that opacity-0 layers are skipped, on llvmpipe.
- `cargo test` green; `MorphUniforms`/`VisualSettings` layout asserts intact if
  touched.
- Bloom on and off both produce correct, non-regressed output.
- Owner visual-acceptance review: active reads solid, inactive reads
  near-invisible, no muddy uniform translucency.
- `architecture/gpu-rendering.md` (render pass order + a new opacity/depth
  section) and `decisions/rendering.md` document the model and the
  active-vs-selected and true-opacity-vs-additive trade-offs.

## Discipline rules

The `VisualSettings` Float32Array index contract
(`web/src/core/settings.ts` ↔ `crates/brain-visualizer/src/sim/gpu/mod.rs`) and
the `MorphUniforms` 192 B layout are locked cross-language contracts — any new
opacity field must update both sides atomically and keep the size asserts green.

## Migration notes (filled in at ship time)

Route the new pass + depth model into `architecture/gpu-rendering.md → Render
pass order`; route the additive-vs-true-opacity and active-vs-click-selected
decisions into `decisions/rendering.md`; add an `_meta/ownership.json` entry if
"opacity model" becomes a distinct owned concept.

## Implementation detail

Concrete, code-grounded plan for the implementing agent. All paths are relative
to `app/`. Read `crates/brain-visualizer/src/sim/gpu/mod.rs → render_full`,
`render_morphology.wgsl`, `pipelines.rs → build_morph_pipelines`, and
`resources.rs → MorphUniforms` before starting.

### Layering decision (resolves Approach bullet 1)

Keep the existing additive, no-depth passes **exactly as-is** for the resting /
background look (far-glow, morphology tubes, morphology somas). Add **two new
depth-tested, alpha-blended draws** — an *active tube* draw and an *active soma*
draw — that run immediately after their additive counterparts inside
`render_full`, write/test depth, and so genuinely occlude.

Why this composition is safe and minimal:

- The depth texture already exists and is allocated unconditionally on every
  resize at full resolution: `resources.rs → resize_render_targets` creates a
  `Depth32Float` `depth_view` with `RENDER_ATTACHMENT` usage. `render_full`
  already early-returns if `depth_view` is `None`, so the active pass can rely on
  it. **Reuse this same `depth_view`** — do not allocate a second depth target.
- The additive scene passes use `depth_stencil_attachment: None`, so they never
  touch depth. The active alpha passes are the ONLY writers/readers of depth in
  the morphology path. They must `LoadOp::Clear(1.0)` the depth on the first
  active pass of the frame (the active tube pass), then `LoadOp::Load` on the
  active soma pass. Do NOT reuse the manifold surface pass's depth clear: the
  surface pass is gated on `surface != 0` (off by default) and the near-LOD path
  is permanently off (`DRAW_LEGACY_NEAR_SPHERES = false`), so the active passes
  must own their own depth clear to be correct in the default configuration.
- Both passes render into `scene_view` (which is `hdr_view` when `bloom_on`, else
  `target_view`) with `LoadOp::Load`, identical to the existing morphology
  passes. Because they write into the same HDR target, bloom composes over them
  unchanged — bloom reads color only, never depth. This satisfies the "bloom on
  and off both correct" exit-gate requirement without any bloom-path edits.
- **Alpha blend choice:** use `wgpu::BlendState::ALPHA_BLENDING` (src=SrcAlpha,
  dst=OneMinusSrcAlpha) for the active passes, with `depth_write_enabled: true`
  and `depth_compare: Less` (mirroring the manifold pipeline at
  `pipelines.rs:835`). The active fragment returns `vec4(color, active_alpha)`
  where `active_alpha` ramps toward 1.0 with spike recency — so a freshly-fired
  neuron writes a near-opaque fragment AND a near-camera depth value, occluding
  the additive background behind it.

### Pass order in `render_full`

Final order (additions marked **NEW**):

1. surface pass (unchanged; `surface != 0`)
2. far-glow pass (unchanged; additive, no depth)
3. morphology tube pass (unchanged; additive, no depth) — resting/background
4. **NEW active-tube pass** — alpha + depth-write, `Clear(1.0)` depth
5. morphology soma pass (unchanged; additive, no depth) — resting/background
6. **NEW active-soma pass** — alpha + depth-test/write, `Load` depth
7. near-LOD passes (unchanged; permanently off)
8. bloom (unchanged)

Rationale for ordering the active draws after their additive siblings: the
additive resting layer lays down the soft glow; the opaque active layer then
punches solid geometry on top of it and into the depth buffer. Keeping the two
active draws adjacent (4 then 6, with 5 between) is fine because the soma pass
loads the depth the tube pass wrote, giving correct active-tube/active-soma
mutual occlusion. (Self-occlusion *within* the additive resting layer remains
deferred per the existing "additive/no-depth" decision — only the active layer
is depth-correct, which is exactly the mission.)

Guard the NEW passes behind the **same** `self.visual.connection_layer != 0`
condition as the existing morphology passes, AND behind the skip-at-zero guard
below.

### Skip-at-zero guards (Approach bullet 3)

Add a CPU-side guard `active_opaque_on` computed once near the top of
`render_full` alongside `bloom_on` / `draw_surface`:

```
let active_opaque_on = self.visual.connection_layer != 0
    && self.morph_config.lighting.active_opacity > 0.0
    && <active pipelines + bind groups present>;
```

- When `active_opacity == 0.0`, skip BOTH new passes entirely (no pass encoded,
  no depth clear) — the renderer is bit-for-bit the current additive look. This
  is the pass/layer-granularity skip the plan's bullet 6 calls for, mirroring the
  existing `draw_surface` and `connection_layer` guards.
- The inactive floor is enforced **inside the shader** (active fragments below an
  effective-opacity epsilon `discard`, exactly like the existing
  `if c.r+c.g+c.b < 0.002 { discard; }` lines), so zero-opacity *fragments* never
  write depth and never occlude. No per-instance CPU culling (explicitly out of
  scope).

### Opacity model — where the knobs live (Approach bullet 2)

**Do NOT grow the `VisualSettings` Float32Array.** It is full at length 24
(`settings.ts → SETTINGS_LENGTH = 24`, mirrored by `from_slice` indices 0–23).
Growing it touches the locked Rust↔TS index contract and the persistence schema
for no benefit. Instead put the two new knobs in the **morph-config-owned**
`LightingConfig` (`morphology.rs → LightingConfig`), which is the established,
contract-light path for morphology beauty knobs (it already owns
`resting_brightness` / `active_boost`, fed through `MorphUniforms`, not the
Float32Array). This is permitted by the Locked decision ("VisualSettings /
MorphologyConfig knobs") and is strictly lower risk.

Add two fields to `LightingConfig` (and its `Default`):

- `active_opacity: f32` (the active-opacity **ceiling**, default `1.0`).
- `inactive_opacity_floor: f32` (the inactive opacity **floor**, default `0.0`
  so inactive structure can go fully hidden in the active layer; the additive
  resting layer still shows it softly).

Plumb them through `MorphUniforms` by **repurposing existing reserved padding —
no size change**. `MorphUniforms` is locked at 192 B and the
`morph_layouts_locked` assert (`resources.rs:2428`) must stay green. The struct
has reserved `u32` pad slots (`_pad4`, `_pad5` in the final 16-B block, plus
`_pad3` and `_pad_a/_pad_b/_pad_c`). Convert `_pad4` and `_pad5` from `u32` to
`f32` and rename them to `active_opacity` / `inactive_opacity_floor` on BOTH
sides atomically:

- Rust: `resources.rs → MorphUniforms` (change the two field types/names; update
  the byte-map doc-comment lines `175`/`176`/`219`/`220` to name them instead of
  "pad"). Size and 16-B alignment are unchanged → the 192 B asserts stay green.
- WGSL: `render_morphology.wgsl → struct MorphUniforms` (lines 84–85: rename
  `_pad4`/`_pad5` to `active_opacity: f32` / `inactive_opacity_floor: f32`, and
  update the byte-offset comment block at lines 57). Offsets are identical, so no
  other WGSL struct field moves — this is NOT a reordering, only a type/name
  change on already-reserved trailing slots (the safe kind of MorphUniforms
  edit).
- Set them in the `MorphUniforms { … }` literal in `render_full` (~line 1306,
  next to `resting_brightness` / `active_boost`) from
  `self.morph_config.lighting`.

Because the additive (resting) tube/soma passes share this same uniform buffer
and ignore the new fields, they are unaffected.

### Shader work in `render_morphology.wgsl`

The two NEW alpha passes need their own fragment entry points so the existing
`fs_main` / `fs_sphere` (which `return vec4(c, 1.0)`) stay byte-identical for the
additive passes. Add `fs_main_active` and `fs_sphere_active` (vertex stages
`vs_main` / `vs_sphere` are reused unchanged):

- Compute an `active` strength from the same source the lighting already uses:
  for tubes, `activity = legacy + packet` (already computed in `fs_main`); for
  somas, `in.glow`/`in.flash`/`in.core`. Define
  `active_alpha = mix(u.inactive_opacity_floor, u.active_opacity, clamp(activity_signal, 0.0, 1.0))`.
- Return `vec4(color, active_alpha)` (color same as the additive path's `c`, but
  NOT pre-multiplied — ALPHA_BLENDING expects straight alpha).
- `if active_alpha < <epsilon> { discard; }` so resting/zero-opacity fragments
  write neither color nor depth (this is the in-shader inactive skip).

This keeps "active = firing" keyed off `last_spike` exactly as the additive
passes already do (Locked decision: active, not click-selection). The
click-selected feed remains a future hook — no picking is built.

### New pipelines in `pipelines.rs`

In `build_morph_pipelines` add two pipelines next to `render_morphology` /
`render_soma_spheres`:

- `render_morphology_active`: same module/layout as `render_morphology`, but
  `entry_point: "fs_main_active"`, `blend: Some(ALPHA_BLENDING)`, and
  `depth_stencil: Some(DepthStencilState { format: Depth32Float,
  depth_write_enabled: Some(true), depth_compare: Some(Less), .. })` (copy from
  the manifold pipeline at `pipelines.rs:835`). Reuse the SAME `tube_consts`
  override constants and the SAME bind-group layout
  (`render_morphology_bgl`) — bindings 0/1/2 are identical.
- `render_soma_spheres_active`: same as `render_soma_spheres` but
  `entry_point: "fs_sphere_active"`, alpha blend, depth-test/write as above,
  reusing `sphere_consts` and `render_soma_spheres_bgl` (bindings 3/4/5).

Add `Option<wgpu::RenderPipeline>` fields for both to the pipelines struct
(`pipelines.rs:75`-area) and `None`-init them (`pipelines.rs:122`-area). They are
built in the same call sites that already build the morph pipelines, so the
`set_morphology_config` render-quality-rebuild path (`mod.rs:551`) and initial
build both cover them with no extra wiring. Reuse the existing
`morph_tube_verts` / `morph_sphere_verts` draw counts for the new draws — the
override constants are identical, so the counts stay in lockstep automatically.

No new bind groups or buffers are needed: the active passes set the SAME bind
group as their additive sibling (`bg.render_morphology` / `bg.render_soma_spheres`).

### Validation — extend `examples/render_check.rs`

Add a step after the existing morphology check (~line 231) that proves
occlusion:

1. Enable `connection_layer = 2` and warm the sim so several neurons are firing.
   Set a non-zero `active_opacity` (default 1.0 is fine via the default config).
2. Render once and read back. Assert the active pass drew (non-black), as the
   morphology check already does.
3. **Occlusion assertion (the new proof):** render a second frame whose
   `MorphologyConfig` sets `inactive_opacity_floor = 0` and `active_opacity = 1`,
   then a third frame with the active layer skipped (`active_opacity = 0`). The
   active-on frame must have at least one pixel where a near, firing neuron's
   solid color replaced what the additive background showed in the
   active-skipped frame — i.e. for some pixel the active-on color differs in a
   way consistent with occlusion (e.g. the additive background contribution is
   suppressed: pick the brightest active-layer pixel and assert its color is
   dominated by a single in-front neuron's tint rather than the summed additive
   value). A pragmatic, deterministic check on llvmpipe: assert
   `max_channel(active_on) >= max_channel(active_skipped)` is NOT how additive
   behaves — instead assert that the **count of saturated/near-opaque pixels**
   (alpha-driven solid color) is `> 0` only when the active layer is on. Keep the
   assertion robust to llvmpipe dithering (use the same `> 2` channel epsilon the
   file already uses).
4. **Skip-at-zero assertion:** with `active_opacity = 0`, assert the frame equals
   the pure-additive baseline within tolerance (no depth-written occlusion), and
   that no panic occurs — proving the CPU-side pass skip.
5. Run with bloom on AND off around the occlusion check (the file already toggles
   `set_bloom_strength`), confirming both composite correctly.

If a `set_morphology_config` JSON entry point is the only way to set the new
fields from the example, drive it via `backend.set_morphology_config(json)` with
a minimal JSON blob `{"lighting":{"activeOpacity":1.0,"inactiveOpacityFloor":0.0}}`
(serde camelCase + `#[serde(default)]` fills the rest).

### Order of edits (minimizes broken intermediate states)

1. `morphology.rs → LightingConfig`: add the two fields + defaults (compiles
   alone; nothing reads them yet).
2. `resources.rs → MorphUniforms`: rename `_pad4`/`_pad5` to the two `f32`
   fields; update byte-map comments; confirm `morph_layouts_locked` still
   asserts 192 B (`cargo test` for that test).
3. `render_morphology.wgsl`: rename the matching WGSL pad fields; add
   `fs_main_active` / `fs_sphere_active`.
4. `pipelines.rs`: add the two active pipelines + struct fields + init.
5. `mod.rs → render_full`: set the two new uniform fields; compute
   `active_opaque_on`; encode the two NEW passes with the depth attachment.
6. `examples/render_check.rs`: add the occlusion + skip-at-zero assertions.
7. Docs: `architecture/gpu-rendering.md` (Render pass order + a new
   opacity/depth subsection; note the active layer is the only depth-correct
   one), `decisions/rendering.md` (active-vs-selected and
   true-opacity-vs-additive trade-offs, and that the knobs live in
   `LightingConfig` not the Float32Array). Per `_meta/manifest.md` change-to-doc,
   the `render_full`/pass-order change also touches `architecture/gpu-backend.md`
   if pass ordering is described there — verify and update if so.

### Gate commands (run from `app/`)

- `cargo test` — must stay green; specifically `morph_layouts_locked`,
  `segment_layout_is_48_bytes`, `sphere_instance_layout_is_32_bytes`, and the
  determinism gates (`wgsl_hash_determinism`, `wgsl_target_determinism`,
  `gpu_sim_dynamics`). Verified: the determinism gates hash only `hash.wgsl` and
  `scatter.wgsl` (BV22 hash output), NOT `render_morphology.wgsl` — so the shader
  edit here does NOT touch those expected hashes. A determinism-gate failure
  after this change would be a real regression, not an expected-hash update.
- `cargo run --release --example render_check` — the extended occlusion +
  skip-at-zero + bloom-on/off assertions, on llvmpipe (validates all WGSL via
  Naga at pipeline build).
- `cargo run --release --example near_lod_check` — confirm the depth-attachment
  changes didn't disturb the (off-by-default) near-LOD depth usage.
- `cd web && npm run typecheck && npm test` — only needed if the
  `VisualSettings` Float32Array is touched. With the chosen
  `LightingConfig`-only approach it is **not**, so these are a no-op sanity check
  unless a dev-panel control for the new knobs is added (out of scope for the
  render pass; the morph-config JSON path already carries them).

### Contract-integrity checklist (Discipline rules)

- `VisualSettings` Float32Array: **untouched** (length stays 24; no `settings.ts`
  or `from_slice` edit). This sidesteps the locked Rust↔TS index contract
  entirely.
- `MorphUniforms`: stays **192 B** — only two trailing reserved `u32` pads become
  `f32` fields, updated atomically in `resources.rs` AND
  `render_morphology.wgsl`. `morph_layouts_locked` must remain green.
- `MorphSegment` (48 B) / `MorphSphereInstance` (32 B): **untouched** (Scope:
  "any `MorphSegment`/`MorphUniforms` layout change beyond what the opacity model
  strictly needs" — the model needs neither beyond the pad repurpose).
- `DRAW_LEGACY_*` guards: **untouched**.

## See also

- The app's [`index.md`](index.md) — where live plans land.
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md) — current additive-no-depth pass order and bloom path.
- [`../decisions/rendering.md`](../decisions/rendering.md).
- [`future_roadmap.md`](future_roadmap.md) — Click-To-Inspect / Picking stays deferred; this plan deliberately avoids it.
