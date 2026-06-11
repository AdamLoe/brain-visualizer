---
status:        shipped
owner:         Kuhn
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/dev-panel.md
  - architecture/scaling.md
  - architecture/simulation.md
  - architecture/web-frontend.md
  - decisions/dev-tooling.md
  - decisions/dynamics.md
  - decisions/scaling.md
---

# Defaults and scale limits

## Mission

Implement the product-default tuning and scale cap from the visual-polish phase:

- `heterogeneity` defaults on at `0.50`.
- `glowTau` defaults to `10`.
- morphology `lighting.restingBrightness` defaults to `0.05`.
- maximum user/adaptive neuron count is limited to `20_000`.

Done when a clean first load, storage reset, accepted-default preset, backend
fallback defaults, and all UI/scaler caps agree, and persisted localStorage is
either migrated or deliberately reset per the lead decision.

This is a planning-only stream under
`docs/plans/visual-product-polish-phase-hub.md`. Source files under `app/` must
not be edited until implementation starts.

## Current Default And Max Map

| Surface | Current truth | Required target | Notes |
|---|---:|---:|---|
| `app/web/src/core/settings.ts -> DEFAULT_SETTINGS.glowTau` | `60.0` | `10.0` | Web clean first-load source for VisualSettings index `0`; reset and accepted-default preset clone this value. |
| `app/crates/brain-visualizer/src/sim/gpu/mod.rs -> VisualSettings::default().glow_tau` | `60.0` | `10.0` | Rust fallback for missing/short Float32Array and backend-created default before JS pushes settings. Must match TS. |
| `app/web/src/core/settings.ts -> DEFAULT_SETTINGS.heterogeneity` | `0.0` | `0.50` | Web clean first-load source for VisualSettings index `14`; stored under `bv2_settings_v1.dev.heterogeneity`. |
| `app/crates/brain-visualizer/src/sim/gpu/mod.rs -> VisualSettings::default().heterogeneity` | `0.0` | `0.50` | Rust fallback for missing index `14`; comment/docs currently frame `0` as neutral baseline. |
| `app/web/src/core/morph-config.ts -> DEFAULT_MORPH_CONFIG.lighting.restingBrightness` | `0.20` | `0.05` | Web clean first-load source for morphology JSON; `MORPH_DESCRIPTORS` also carries `default: 0.20` and must match. |
| `app/crates/brain-visualizer/src/sim/morphology.rs -> LightingConfig::default().resting_brightness` | `0.20` | `0.05` | Rust fallback for malformed/missing morphology JSON fields via serde defaults. Must match TS. |
| `app/web/src/core/types.ts -> DEFAULT_CONFIG.n` | `1_200` | unchanged unless lead says otherwise | Default scale is not the max; docs identify this as first-load scale truth. |
| `app/web/src/ui/dev-panel.ts -> N slider max` | `200_000` | `20_000` | Direct user input cap in Network tab. Also clamp number input and external sync behavior. |
| `app/web/src/ui/controls.ts -> TIER_PRESETS` | `balanced: 50_000`, `max: 200_000` | no preset above `20_000` | Legacy/control surface still exported, tested, and documented. Comments conflict with `DEFAULT_CONFIG` by saying low is 10k. |
| `app/web/src/ui/controls.ts -> N_MIN/N_MAX` | up to `1_200_000` | no `N_MAX` above `20_000`; `N_MIN <= N_MAX` per tier | Pure `scalerDecide` tests depend on these. |
| `app/crates/brain-visualizer/src/sim/scaler.rs -> TierRange::for_tier` | up to `1_000_000`; min tiers start at `10_000/50_000/200_000` | no `n_max` above `20_000`; no `n_min` above `20_000` | Dormant scaler proposal path, but docs and tests treat it as a scaling truth. |
| `app/crates/brain-visualizer/src/sim/backend.rs -> SimConfig::default()` | `n: 50_000`, `k: 32`, `tier: Balanced`, `i_ext: 0.06` | `n <= 20_000`; likely align with web fallback or a Rust-only safe fallback | Used by tests/native helpers and by wasm construction for unspecified fields. This currently conflicts with the new cap. |
| `app/crates/brain-visualizer/src/gpu_limits.rs -> GpuCaps` | hardware-derived millions-scale addressability | no product cap here unless adding a product cap field | This is adapter capacity, not user/product maximum. Keep product cap separate from hardware capability. |
| `app/web/src/main.ts -> loadConfig()/loadSettings()/loadMorphConfig()` boot | saved values win after version match | decision needed | Existing saved localStorage can preserve old high N, old `glowTau`, old `heterogeneity`, and old `restingBrightness` unless migrated/reset. |
| `app/crates/brain-visualizer/src/lib.rs -> WasmGpuBackend::new` | accepts JS `n/k` as given and fills rest from `SimConfig::default()` | validate or receive already-clamped values | Rust ingestion currently trusts JS for `n`; backend fallback default also exceeds the cap. |

Related non-authoritative docs that will drift after implementation:
`docs/overview.md`, `docs/architecture/web-frontend.md`,
`docs/architecture/scaling.md`, `docs/architecture/dev-panel.md`,
`docs/architecture/simulation.md`, `docs/decisions/scaling.md`,
`docs/decisions/dynamics.md`, and `docs/decisions/dev-tooling.md`.

## Conflicts To Resolve First

The repo currently has several conflicting sources of scale truth:

- `DEFAULT_CONFIG` says the clean first load is `1_200`, while
  `controls.ts` comments say low/default is `10_000`.
- `dev-panel.ts` lets the user type/slide up to `200_000`.
- `controls.ts` tier presets and scaler bounds go to `200_000` and
  `1_200_000`.
- Rust `TierRange` goes to `1_000_000`, and Rust `SimConfig::default()` starts at
  `50_000`.
- `gpu_limits.rs` correctly reports hardware capacity far above the new product
  cap, so implementation must avoid treating hardware capacity as the product
  maximum.

Start implementation with a source-of-truth cleanup phase: introduce or clearly
name one product max constant per language boundary, then make every user,
persistence, scaler, and fallback path consume that truth or explicitly document
why it is hardware-only.

## Persistence Migration Options

Lead decision on 2026-06-09: do not make saved browser settings a product
blocker for this phase. Use the simplest safe implementation; at minimum, clamp
saved `n` to the new product max. If implementation chooses to reset settings
or morphology versions, that is acceptable as long as the behavior is explicit
and tested.

Follow-up lead decision on 2026-06-09: retire high-N tier semantics from the UI
under the `20_000` cap. Do not preserve old tier labels that imply large-scale
targets.

### Option A: Version reset

Bump `bv2_settings_v1` schema version from `5` to `6` and `bv2_morph_v1`
sentinel from `1` to `2`; consider bumping `bv2_config_v1` from `1` to `2`.
On mismatch, existing code falls back to the new defaults. This is the simplest
way to make every saved browser see `heterogeneity=0.50`, `glowTau=10`,
`restingBrightness=0.05`, and capped default config.

Tradeoff: users lose all saved settings/config, including unrelated tuning.

### Option B: Selective migration and clamping

Keep keys, add migration logic:

- For `bv2_settings_v1` version `5`, if saved `glowTau` or `heterogeneity`
  equals the old default (`60` / `0`), rewrite to the new default; preserve
  non-default user edits.
- For `bv2_morph_v1` version `1`, if saved
  `lighting.restingBrightness === 0.20`, rewrite to `0.05`; preserve
  non-default edits.
- For `bv2_config_v1` version `1`, clamp saved `n` to `20_000` and adjust any
  saved tier/preset semantics that imply a larger N.

Tradeoff: more code and tests, but it avoids wiping deliberate user changes.

### Option C: Always clamp only scale, reset visual defaults

Bump settings and morph versions so visual defaults reset, but keep
`bv2_config_v1` at version `1` and clamp `parsed.n` during `loadConfig()`. This
is a pragmatic middle path if the lead wants new visual defaults to override old
localStorage but does not want unrelated runtime config erased.

Recommended implementation after the lead answer: choose Option A if it is the
least invasive in the current code, otherwise Option C. Option B is no longer
worth the complexity for this phase because saved-setting preservation is not a
lead priority.

## Implementation Sequencing

1. **Define product scale truth.**

   Add/norm a `20_000` product max in the web config/scaling surface and a Rust
   equivalent for backend fallback/scaler logic. Keep hardware capacity in
   `gpu_limits.rs` separate. Decide whether to export the web max from
   `types.ts` or from a scaling-specific module; current docs treat
   `types.ts -> DEFAULT_CONFIG` as scale-default truth and `controls.ts` as tier
   preset/bounds truth.

2. **Update web defaults and persistence.**

   Change `DEFAULT_SETTINGS.glowTau` to `10` and `heterogeneity` to `0.50`.
   Apply the chosen `bv2_settings_v1` migration/reset behavior. Verify
   `toFloat32Array()` still places those values at indices `0` and `14`; do not
   reorder or resize the Float32Array contract.

3. **Update morphology defaults and persistence.**

   Change `DEFAULT_MORPH_CONFIG.lighting.restingBrightness` to `0.05` and the
   matching `MORPH_DESCRIPTORS` row default to `0.05`. Apply the chosen
   `bv2_morph_v1` migration/reset behavior. Keep this out of VisualSettings;
   it is morphology JSON owned by `set_morphology_config()`.

4. **Cap web user and scaler inputs.**

   Clamp `loadConfig()` and any Network tab `onNetwork` path so saved or typed N
   cannot exceed `20_000`. Lower the dev-panel N slider max. Update
   `controls.ts` `TIER_PRESETS`, `N_MIN`, `N_MAX`, and scaler tests so no
   preset/grow action exceeds `20_000`.

5. **Align Rust fallbacks and scaler.**

   Update `VisualSettings::default()` and `LightingConfig::default()` to match
   web defaults. Lower `SimConfig::default().n` to at most `20_000`; preferably
   align it with web `DEFAULT_CONFIG.n` unless native examples/tests require a
   larger fixture. Update `TierRange::for_tier()` so proposal math never emits
   `n > 20_000` and no tier has `n_min > n_max`.

6. **Guard Rust ingestion.**

   Decide whether `WasmGpuBackend::new(n, k, ...)` should clamp/reject `n >
   20_000`, or whether JS clamping is the only runtime guard. For defense in
   depth, clamp or validate at the Rust WASM boundary and log the adjustment.

7. **Retire high-N tier UI semantics and refresh tests.**

   `HIDDEN_REVIEW_PRESETS["accepted-default"]` is derived from the three
   defaults and should update automatically. Review `performance-review` and
   `hero-review`: they intentionally override glow/resting brightness today
   (`glowTau` `50/72`, `restingBrightness` `0.16/0.07`), so decide whether they
   remain comparison presets or should be retuned around the new baseline.
   Remove or rename user-facing high-N tier language rather than compressing old
   high-scale tier names into the new cap.

8. **Doc migration after implementation.**

   Update architecture/decision docs listed in the frontmatter and the manifest
   change-to-doc table only after source changes ship. Do not mutate the phase
   hub from this stream except during orchestrator collision-map work.

## Narrow Gates

Run focused gates per implementation stream, not the full suite:

- Web defaults/persistence: `npm test -- dev-panel.test.ts` plus any new
  per-feature tests for `settings.ts`, `morph-config.ts`, and `types.ts`
  migrations.
- Web type contract: `npm run typecheck` from `app/web/` after touching
  TypeScript defaults or exported constants.
- Rust defaults/scaler: `cargo test -p brain-visualizer scaler` and a focused
  test for `VisualSettings::from_slice` fallback defaults if added.
- Rust morphology fallback: focused morphology config/default test if one exists
  or is added; otherwise a small per-feature Rust test for
  `LightingConfig::default()` / serde missing-field fallback.
- Manual/browser gate: clean localStorage first-load and saved-old-localStorage
  reload scenarios; inspect dev-panel readouts for `glowTau=10`,
  `heterogeneity=0.50`, `restingBrightness=0.05`, and N cap behavior.

## Lead Questions

- Should Rust `SimConfig::default().n` match web `DEFAULT_CONFIG.n = 1_200`, or
  use a Rust/native fallback such as `20_000` while web stays beauty-first?
- Should hidden `performance-review` and `hero-review` presets keep their
  current override values, or should they be regenerated from the new baseline?

## Deferrals

- Do not change `gpu_limits.rs` hardware-derived capacity unless a separate
  product-cap field is intentionally added; hardware capacity and product max
  are different concepts.
- Do not change shader heterogeneity math. Only the default setting changes;
  deterministic hash behavior and the `heterogeneity=0` baseline guarantee
  remain available when users set the slider to zero.
- Do not update architecture/decision docs until implementation ships.
- Do not run full drift gates per stream; reserve full verification for the
  visual-polish hub's consolidated gate.

## Migration Notes

Migrated on 2026-06-11 into `architecture/dev-panel.md`,
`architecture/web-frontend.md`, `architecture/scaling.md`,
`architecture/simulation.md`, `decisions/dev-tooling.md`,
`decisions/scaling.md`, and `decisions/dynamics.md`. Current-state docs record
`heterogeneity = 0.50`, `glowTau/glow_tau = 10`,
`restingBrightness/resting_brightness = 0.05`, `DEFAULT_CONFIG.n = 1200`,
`SimConfig::default().n = 1200`, product max `N = 20_000`, saved-N clamping,
and the deliberate non-bump of visual/morph localStorage versions.

`okay_to_delete` remains `false` only because the visual-product-polish hub is
retaining all six stream plans until the real-WebGPU browser smoke blocker is
cleared or waived.
