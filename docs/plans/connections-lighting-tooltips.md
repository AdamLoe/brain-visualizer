---
status:        shipped
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - architecture/dev-panel.md
  - architecture/manifold.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
---

# Connections lighting redesign + instant dev-panel tooltips (v0.1.2)

## Mission

Reshape the "Connections" visual from a *traveling pulse along each neuron's
own tree* into *whole-connection lighting keyed to spikes*: when a neuron
fires, its actual synaptic connections light up instantly and fade out in sync
with the neuron's own glow (τ). Add two independent toggles — light a firing
neuron's **outgoing** ("next" / downstream) connections and/or its **incoming**
("past" / upstream) connections — and draw **all K** connections per neuron, not
a 5-branch subset. Remove the now-meaningless "Signal speed" and "Recent trail"
knobs, fix the "Curve" control so it visibly bends connections, and give the
dev panel custom tooltips that appear **immediately** on hover across basically
every control and metric. Ships as patch **0.1.2**. Done when the new model
renders correctly under the `render_check` example, all drift gates are green,
and the dev panel shows the new toggles + instant tooltips.

## Scope

**In scope**
- Replace the morphology pulse/trail mechanic with whole-connection, τ-synced
  spike lighting (downstream + upstream toggles).
- Draw every outgoing synapse per neuron (axon branches = K), raising the
  per-neuron segment cap accordingly.
- Fix the "Curve" (axon bow) control so its effect is clearly visible at the
  default scale.
- Remove the "Signal speed" + "Recent trail" panel sliders and repurpose their
  Float32Array indices (8, 9) for the two new toggles; bump the settings schema
  version 2 → 3.
- Custom instant-on tooltips for the dev panel (controls **and** Monitor/
  Dynamics metric rows), excluding self-evident items (section headers, ×/close).
- Version bump 0.1.1 → 0.1.2 (`web/package.json`, `crates/brain-visualizer/Cargo.toml`).
- Doc migration for every changed contract/behavior.

**Out of scope (cut line)**
- The retired ribbon / near-LOD / sphere passes stay retired (only the gated
  ribbon-uniform upload is touched to keep it compiling after the field rename).
- No change to connectivity math, LIF dynamics, or the hash determinism rule.
- Default **K stays 16** (user pre-authorized lowering it, but K is a runtime
  knob in the Network tab — no default change needed for correctness; revisit
  only if the all-K morphology reads as a hairball at default scale).
- Dendrites remain decorative resting structure (they are not inter-neuron
  "connections"); spike lighting applies to axon segments.

## Key design decisions (confirmed with user)

- **Fade:** whole connection lights instantly on the keyed spike, then fades
  using the *same* `exp(-tick_diff / glow_tau)` curve as the far-glow neuron
  dot. No separate lifetime/trail knob, no spatial pulse travel.
- **Default modes:** downstream ("next") **on**, upstream ("past") **off** —
  both toggleable in the panel.
- **Curve:** keep the slider; make the bow visibly bend.
- **Coverage:** draw **all K** outgoing connections per neuron.

## Approach

Three streams. Stream A is foundational (contract + shader + morphology) and
must land and pass gates **before** B and C, because B binds the renamed
settings keys and C documents the new contract. A is one tightly-coupled agent
(splitting it leaves the cross-language layout contract broken mid-flight).

### Stream A — contract, morphology data, shader (Rust + WGSL + TS store)

Owned files:
- `web/src/core/settings.ts` — repurpose index 8 → `connectionLightNext`,
  index 9 → `connectionLightPast` (both 0/1); defaults next=1, past=0; bump
  schema `version` sentinel 2 → 3; update `SavedDev`, `mergeOver`,
  `toFloat32Array`, `DEFAULT_SETTINGS`, header comment.
- `web/src/core/setting-metadata.ts` — rename the two keys in `SETTING_IMPACT`.
- `crates/brain-visualizer/src/sim/gpu/mod.rs` — `VisualSettings`: rename
  `connection_lifetime`/`connection_pulse_speed` → `connection_light_next: u32`
  / `connection_light_past: u32`; update `Default`, `from_slice` (indices 8/9
  via the `u(..)` reader), and the morph-uniform upload in `render_full`
  (pass the two toggles + `glow_tau`; drop `SIGNAL_SPEED_MULT`/pulse). Update
  the gated `DRAW_LEGACY_RIBBONS` ribbon-uniform upload to feed literal
  fallback constants for the removed tuning fields so the retired path still
  compiles.
- `crates/brain-visualizer/src/sim/morphology.rs` — `MorphSegment`: `_pad: u32`
  → `target_id: u32` (stays 48 B). `generate()`: `branches = k` (all targets,
  not `min(5, k)`); write each axon segment's `target_id = tgt_id`; dendrite
  `target_id = neuron_id` (self, unused). Derive the segment cap from K
  (worst case ≈ dendrites + `k * SEGS`) instead of the fixed `MAX_SEGS_PER_NEURON
  = 32`. **Curve fix:** raise `SEGS` for a smoother arc and increase the bow
  magnitude so `connection_curve_lift` reads at default scale (and/or widen the
  panel slider range in B).
- `crates/brain-visualizer/src/sim/gpu/resources.rs` — `MorphUniforms`:
  repurpose `signal_speed`/`lifetime` → `light_next: u32`/`light_past: u32` and
  add `glow_tau: f32` (consume a `_pad` slot; stays 144 B — keep the size
  assert). Size the morph segment buffer from the K-derived cap.
- `crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl` —
  matching `MorphSegment` (`target_id`) + `MorphUniforms` (`light_next`,
  `light_past`, `glow_tau`); new brightness model:
  `brightness = base + BOOST * lit`, where
  `lit = max( light_next && spiked(src) ? exp(-Δt_src/glow_tau) : 0,
              light_past && kind==axon && spiked(tgt) ? exp(-Δt_tgt/glow_tau) : 0 )`
  with `src = last_spike[neuron_id]`, `tgt = last_spike[target_id]`. Remove the
  pulse-front/band/life-fade block. Keep width, E/I + region color, curve.

Gate: `cargo test` green (incl. `wgsl_*_determinism`, `gpu_sim_dynamics`,
`MorphSegment == 48`, `MorphUniforms == 144`); `cargo run -p brain-visualizer
--example render_check` passes its pixel assertions.

### Stream B — dev panel (TS UI) — after A

Owned file: `web/src/ui/dev-panel.ts` (+ `dev-panel.css`).
- In the **Connections** section: delete the "Signal speed"
  (`connectionPulseSpeed`) and "Recent trail" (`connectionLifetime`) sliders;
  add two Off/On selects — "Light next (downstream)" → `connectionLightNext`,
  "Light past (upstream)" → `connectionLightPast`. Keep Connections on/off,
  Resting opacity, Width, and the (now-fixed) Curve slider.
- **Instant tooltips:** add a custom tooltip mechanism that shows with **zero
  delay** on hover and is not clipped by the panel's scroll container (a single
  body-appended floating element positioned on `mouseover`, hidden on
  `mouseout` — preferred over native `title=`, which has a ~1 s delay, and over
  a CSS `::after` that the scroll overflow would clip). Route the existing
  `tooltip` strings through it, then add tooltips to every remaining
  non-obvious control and to the Monitor/Dynamics metric rows (Spikes/tick,
  Branching ratio, Accum HW, pctFired windows, per-region, E/I, etc.). Skip
  self-evident items (section separators, the × button).

Gate: `npm run typecheck` clean; panel renders toggles + instant tooltips.

### Stream C — docs, version, final gates — after A & B

- Version: `web/package.json` + `crates/brain-visualizer/Cargo.toml` → 0.1.2.
- Doc migration (see Migration notes): `architecture/gpu-rendering.md`
  (morphology pass model), `architecture/dev-panel.md` (schema v3, index 8/9
  repurpose, new fields), `architecture/manifold.md` (all-K coverage + cap),
  `architecture/active-edges.md` (ribbon-uniform field rename note),
  `decisions/rendering.md` + `decisions/dev-tooling.md`, and the manifest
  `change-to-doc` / `drift-verification` slots if the contract list shifts.
- Final gates: `cargo test`, `npm run typecheck`, `npm test`, the
  `render_check` example, and a manual dev-server smoke confirming next-mode
  lighting + instant tooltips.

## Exit gate

1. `cd app && cargo test` — all green (determinism gates + layout-size asserts).
2. `cd app && cargo run -p brain-visualizer --example render_check` — passes.
3. `cd app/web && npm run typecheck && npm test` — clean.
4. Manual smoke (dev server): firing neurons light their outgoing connections,
   which fade with τ; the "Light past" toggle adds incoming-connection lighting;
   Curve visibly bends; "Signal speed"/"Recent trail" are gone; hovering any
   non-obvious control/metric shows a tooltip immediately.
5. Version reads 0.1.2 in both manifests; owning docs updated; this plan set to
   `shipped` with migration notes filled.

## Discipline rules

- The Float32Array index contract and the `MorphSegment` (48 B) /
  `MorphUniforms` (144 B) layouts are the #1 corruption risk — change Rust,
  WGSL, and TS in lockstep within Stream A and keep the size-assert tests.
- Schema version **must** bump 2 → 3 (repurposed indices); old saves are
  discarded by design — acceptable.
- No silent caps: if the K-derived segment cap is ever hit, keep the existing
  `eprintln!` drop log.

## Migration notes (complete)

Shipped as patch **0.1.2**. Version bumped 0.1.1 → 0.1.2 in
`app/web/package.json` and `app/crates/brain-visualizer/Cargo.toml` (workspace
`app/Cargo.toml` has no version field; all other `0.1.1` references in code are
historical "removed in 0.1.1" comments and were left intact).

Context migrated into current-state docs (this plan is now `okay_to_delete`):

- **`architecture/gpu-rendering.md`** — Morphology-pass section + render-pass-3
  description + the "with outward signal pulse" feature line rewritten to the
  whole-connection τ-synced model: instant light on spike, fade with the
  far-glow `exp(-tick_diff/glow_tau)`, `light_next` (downstream, default ON) /
  `light_past` (upstream, axon-only, default OFF) combined as a `max`; noted
  `target_id` in `MorphSegment`, the new `MorphUniforms` fields
  (`light_next`/`light_past`/`glow_tau`), all-K coverage, baked curve bow, and
  kept the 48 B / 144 B layout-contract warnings.
- **`architecture/dev-panel.md`** — Settings persistence contract bumped to
  schema `version: 3` (sentinel `!== 3` discarded, no migration); added an
  **Instant tooltips** section (`data-tip` + body-appended `.dp-tooltip` +
  delegated `document` listeners; `_buildTooltip`/`_attachTip`) and an
  "Update when" entry. Index 8/9 repurpose lives in the authoritative inline
  comments the doc points at (no literal index table transcribed, per rules).
- **`architecture/manifold.md`** — morphology-generation section: all-K axon
  branches (one arbor per synaptic target), the K-derived cap
  (`max_segs_per_neuron`, replacing fixed `MAX_SEGS_PER_NEURON = 32`),
  `AXON_SEGS_PER_BRANCH = 6` + `BOW_GAIN` curve bow, the `target_id` field
  (trailing `_pad` repurposed), and that `path_len` no longer drives timing.
- **`architecture/active-edges.md`** — "Reviving the path" caveat: the gated
  `RibbonUniforms` upload now writes literal fallback constants for the removed
  `connection_lifetime`/`connection_pulse_speed` (lifetime/pulse_speed) fields;
  ribbon path stays retired. Fixed the stale `See also` knob list.
- **`decisions/rendering.md`** — added three decisions: whole-connection
  τ-synced lighting (replacing traveling pulse), draw-all-K coverage, and the
  visible-curve fix; reworded the existing "morphology supersedes ribbon"
  decision off "outward signal pulse"/"wavefront".
- **`decisions/dev-tooling.md`** — added the instant-tooltip decision (native
  `title=` ~1 s delay too slow; CSS `::after` clipped by scroll container);
  bumped the persistence decision to `version: 3`.
- **`decisions/manifold.md`** — (owned-doc drift) reworded the morphology
  rationale off the retired pulse and fixed the dead `MAX_SEGS_PER_NEURON`
  code anchor → `max_segs_per_neuron`.
- **`_meta/manifest.md`** — no edit needed: its `change-to-doc` /
  `drift-verification` slots reference the still-valid `MorphSegment` layout,
  `VisualSettings` Float32Array contract, and `DRAW_LEGACY_*` flags — none of
  the now-changed concrete symbol names.

Final gates (all green): `cargo test` (78 unit + determinism + dynamics +
48 B/144 B layout asserts), `cargo run --example render_check`,
`npm run typecheck`, `npm test` (22 passed).

## See also

- `docs/plans/index.md` — where live plans land.
- `architecture/gpu-rendering.md`, `architecture/dev-panel.md`,
  `architecture/manifold.md` — owning docs.
- `_meta/manifest.md` — change-to-doc + drift gates.
