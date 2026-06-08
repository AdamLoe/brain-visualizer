---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/gpu-rendering.md
  - architecture/dev-panel.md
  - architecture/profiling.md
  - decisions/rendering.md
  - decisions/dev-tooling.md
  - decisions/scaling.md
---

# Accepted Visual Defaults Manifest

## Mission

Hold the exact accepted visual state for the v0.3-v0.5 roadmap: first-load
defaults, hidden dev-panel review presets, camera presets, artifact paths, and
the config JSON that produced accepted screenshots/video.

This is a coordination artifact, not permanent architecture. v0.4.0 fills it
when the cohesive look is accepted; v0.5.0 verifies production behavior against
it; ship-time migration moves durable facts into architecture/decisions.

## Current status

| Area | Status | Owner |
|---|---|---|
| First-load default config | Implemented in code; visual acceptance pending Adam | v0.4.0 |
| Hidden dev-panel presets | Implemented in `app/web/src/ui/dev-panel.ts` | v0.4.0 |
| Camera presets | Partially filled from recent `morph_view` artifacts | v0.4.0 |
| Artifact ledger | Partially filled from recent `/tmp/morph_*` and `morph_view*.json` outputs | v0.4.0 / v0.5.0 |
| Production verification | `cargo test`, `render_check`, `morph_view`, `npm run typecheck`, `npm test`, and `npm run build` all passed on 2026-06-08; production preview served with COOP/COEP headers; Chromium booted the built page and restored the public HUD, but this environment still exposed no usable WebGPU adapter | v0.5.0 |

## Baseline identity

Fill these when a look is accepted:

| Field | Value |
|---|---|
| Accepted by | Pending Adam visual approval |
| Accepted working tree / commit | `a90f427` + dirty worktree |
| Review date | `2026-06-08` |
| Source plan | `v0.4.0-cohesive-visual-defaults.md` |
| Default preset id | `accepted-default` |
| Performance preset id | `performance-review` |
| Hero preset id | `hero-review` |

## First-load default config

The accepted default must match a clean-profile first load. Do not let this
become a dev-only tuned state.

| Surface | Accepted value / source |
|---|---|
| Seed | `0x5eed5eed` (`1592614637`) from `app/web/src/core/types.ts -> DEFAULT_CONFIG` |
| Tier | `low` from `app/web/src/core/types.ts -> DEFAULT_CONFIG` |
| `N` / `K` | `1200 / 16` from `app/web/src/core/types.ts -> DEFAULT_CONFIG` |
| `AppConfig` source | `app/web/src/core/types.ts -> DEFAULT_CONFIG` |
| `VisualSettings` payload | `app/web/src/core/settings.ts -> DEFAULT_SETTINGS` |
| `MorphologyConfig` payload | `app/web/src/core/morph-config.ts -> DEFAULT_MORPH_CONFIG` |
| Render quality config | `tubeSides: 6`, `sphereSlices: 8`, `sphereStacks: 6` from `DEFAULT_MORPH_CONFIG.renderQuality` |
| localStorage starting state | Empty profile |
| Reset-to-default behavior | Storage reset now reapplies these defaults and schedules a default `N/K/seed` rebuild |

### Current clean first-load payloads

`accepted-default` matches these values exactly by construction.

```json
app_config: {
  "n": 1200,
  "k": 16,
  "seed": 1592614637,
  "tier": "low",
  "speed": "normal",
  "backend": "gpu",
  "excitability": 0.71,
  "ticksPerSec": 30
}
visual_settings: {
  "glowTau": 60,
  "pointRadius": 0.004,
  "neuronVisualRadius": 0.004,
  "activeNeuronRadiusBoost": 2,
  "inactiveNeuronOpacity": 1,
  "voltageGlowStrength": 0,
  "connectionVisualWidth": 0.8,
  "connectionCurveLift": 0.15,
  "connectionLightNext": 1,
  "bloomStrength": 0.4,
  "surfaceOpacity": 1,
  "iExt": 0.055,
  "synapticScale": 0.03,
  "heterogeneity": 0,
  "morphRestingOpacity": 0.2,
  "signalSource": 0,
  "connectionLayer": 1,
  "colorBy": 0,
  "neuronVisibility": 0,
  "surface": 0,
  "weightNormalization": 1,
  "inputMode": 0,
  "adaptiveScalerEnabled": 0
}
morphology_config: {
  "generator": {
    "baseRadius": 0.006,
    "dendritePrimaryMin": 3,
    "dendritePrimarySpan": 2,
    "dendriteReachLo": 0.035,
    "dendriteReachHi": 0.058,
    "axonStopFraction": 0.85,
    "axonRootRadiusFraction": 0.66,
    "axonCurveLift": 0.15,
    "socketCountMin": 2,
    "socketCountMax": 4,
    "socketRadiusLo": 0.008,
    "socketRadiusHi": 0.018,
    "socketTipPreference": 0.78,
    "clusterMin": 2,
    "clusterMax": 5,
    "trunkRootSamples": 2,
    "clusterBranchSamples": 2,
    "terminalTwigSamples": 3,
    "trunkLengthFraction": 0.32,
    "clusterSplitFraction": 0.62,
    "rootRadiusFraction": 0.62,
    "clusterRadiusFraction": 0.44,
    "twigRadiusFraction": 0.16,
    "taperCurve": 2.1,
    "dendriteMidRadiusFraction": 0.6,
    "dendriteTipRadiusFraction": 0.3
  },
  "renderQuality": {
    "tubeSides": 6,
    "sphereSlices": 8,
    "sphereStacks": 6
  },
  "lighting": {
    "lightDirX": -0.352,
    "lightDirY": 0.553,
    "lightDirZ": 0.755,
    "ambient": 0.55,
    "diffuseIntensity": 0.35,
    "rimIntensity": 0.3,
    "rimPower": 2,
    "restingBrightness": 0.2,
    "activeBoost": 1.8
  }
}
```

## Hidden dev-panel presets

These presets are hidden review tools. They must not appear in the public UI.

| Preset | Purpose | Required behavior |
|---|---|---|
| `accepted-default` | Public first-load look | Matches clean first-load defaults exactly |
| `performance-review` | Lower visual cost comparison | Reduces cost without changing semantics |
| `hero-review` | Screenshot/video capture | May increase cost; must stay dev/review-only if costly |

Exact current payloads from `app/web/src/ui/dev-panel.ts -> HIDDEN_REVIEW_PRESETS`:

```json
accepted-default: {
  "app_config": {
    "n": 1200,
    "k": 16,
    "seed": 1592614637,
    "tier": "low",
    "speed": "normal",
    "backend": "gpu",
    "excitability": 0.71,
    "ticksPerSec": 30
  },
  "visual_settings_source": "DEFAULT_SETTINGS",
  "morphology_config_source": "DEFAULT_MORPH_CONFIG",
  "render_quality": { "tubeSides": 6, "sphereSlices": 8, "sphereStacks": 6 },
  "notes": "Exact clean first-load defaults."
}
performance-review: {
  "app_config": "same as accepted-default",
  "visual_settings_overrides": {
    "glowTau": 50,
    "connectionVisualWidth": 0.65,
    "bloomStrength": 0.2,
    "morphRestingOpacity": 0.14
  },
  "morphology_config_overrides": {
    "renderQuality": { "tubeSides": 4, "sphereSlices": 6, "sphereStacks": 4 },
    "lighting": {
      "ambient": 0.52,
      "diffuseIntensity": 0.3,
      "rimIntensity": 0.2,
      "rimPower": 1.8,
      "restingBrightness": 0.16,
      "activeBoost": 1.55
    }
  },
  "notes": "Lower-cost comparison preset with reduced bloom and tessellation."
}
hero-review: {
  "app_config": "same as accepted-default",
  "visual_settings_overrides": {
    "glowTau": 72,
    "connectionVisualWidth": 0.95,
    "bloomStrength": 0.65,
    "morphRestingOpacity": 0.24
  },
  "morphology_config_overrides": {
    "renderQuality": { "tubeSides": 8, "sphereSlices": 10, "sphereStacks": 8 },
    "lighting": {
      "ambient": 0.4,
      "diffuseIntensity": 0.55,
      "rimIntensity": 0.4,
      "rimPower": 2.5,
      "restingBrightness": 0.07,
      "activeBoost": 3
    }
  },
  "notes": "Screenshot-oriented review preset. Lighting split matches the current active-bright morph_view reference artifact."
}
```

## Camera presets

Use stable camera presets for repeatable review. Fill exact target/eye/distance
values when the harness/browser capture path supports them.

| Preset | Purpose | Camera payload |
|---|---|---|
| default | first screenshot / homepage impression | `az: 0.300`, `el: 0.400`, `dist: 3.000` from `/tmp/morph_view_stats.json` frame `0` |
| top | hemisphere and silhouette check | Pending browser/review harness capture |
| side | non-spherical profile check | Pending browser/review harness capture |
| oblique | general composition check | `az: 2.200`, `el: 0.200`, `dist: 3.000` from `/tmp/morph_view_stats.json` frame `1` |
| close-neuron | soma, branches, material, pulse readability | `az: 0.600`, `el: 0.300`, `dist: 0.900` from `/tmp/morph_view_stats.json` frame `2` |

## v0.5.0 production verification

Verification run completed on `2026-06-08` against the current `a90f427 + dirty
worktree` state:

- Native gates passed:
  `cargo test -p brain-visualizer`,
  `cargo run -p brain-visualizer --example render_check`,
  `cargo run -p brain-visualizer --example morph_view`
- Web gates passed:
  `npm run typecheck`,
  `npm test`,
  `npm run build`
- Production preview served the built `dist/` with
  `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`.
- Chromium/Playwright booted the built page, advanced frames, and showed the
  public HUD again after the stabilization fix.
- `navigator.gpu` was present, but `requestAdapter()` still returned no usable
  adapter in this environment. Final browser WebGPU beauty review and subjective
  visual acceptance therefore remain blocked on a real adapter plus Adam's sign-off.

## Artifact ledger

Record accepted artifact paths here even if large files live outside the repo.

| Artifact | Preset | Camera | Source | Path |
|---|---|---|---|---|
| `01-default-first-load.png` | `accepted-default` | default | native `morph_view` PNG conversion | `/tmp/morph_0.png` |
| `02-top-brain-shape.png` | `accepted-default` | top | Pending capture | TBD |
| `03-side-brain-shape.png` | `accepted-default` | side | Pending capture | TBD |
| `04-shell-only-brain-shape.png` | `accepted-default` | default/top/side | Pending shell-only capture | TBD |
| `05-neuron-cloud-only-placement.png` | `accepted-default` | default/top/side | Pending neuron-cloud-only capture | TBD |
| `06-close-neuron-material.png` | `accepted-default` | close-neuron | native `morph_view` PNG conversion | `/tmp/morph_2.png` |
| `07-soma-pulse-sequence.mp4` | `accepted-default` | close-neuron | Pending browser capture | TBD |
| `08-traveling-impulse-sequence.mp4` | `accepted-default` | close-neuron | Pending browser capture | TBD |
| `09-low-tier-default.png` | `performance-review` | default | Pending capture after preset rerun | TBD |
| `10-hero-dev-preset.png` | `hero-review` | oblique | Pending real image capture; current closest reference is active-bright morph harness stats only | TBD |
| `production-preview-smoke.png` | `accepted-default` | default | Chromium preview smoke on built `dist/`; environment had no WebGPU adapter | `/tmp/brain-visualizer-preview.png` |

## Acceptance status

| Plan | Accepted artifact source | Status |
|---|---|---|
| v0.3.0 brain-shaped arena | `render_check` plus pending top/side/shell-only/browser captures | Implementation complete; visual acceptance pending |
| v0.3.1 curved chain morphology | `/tmp/morph_0..3.rgba`, `/tmp/morph_view_stats.json` | Implementation complete; visual acceptance pending |
| v0.3.2 material polish | `/tmp/morph_0..3.rgba`, `/tmp/morph_active_bright.rgba`, `/tmp/morph_view_stats.json` | Implementation complete; visual acceptance pending |
| v0.3.3 soma pulse / traveling impulse | `/tmp/morph_active_bright.rgba`, `/tmp/morph_view_active_bright_stats.json`; pulse video still pending | Implementation complete; visual acceptance pending |
| v0.3.4 right-click pan | Manual/browser note | Implementation complete; live pointer check pending |
| v0.4.0 cohesive defaults | This manifest | Implementation complete; visual acceptance pending |
| v0.5.0 stabilization | Production gates + preview verification | Code/build verification complete; final WebGPU beauty pass and Adam approval pending |

## Migration notes

When v0.5.0 ships:

- Move current rendering/default behavior into `architecture/gpu-rendering.md`.
- Move hidden preset persistence/control facts into `architecture/dev-panel.md`.
- Move accepted rationale into `decisions/rendering.md` and
  `decisions/dev-tooling.md` only when it constrains future work.
- Mark this manifest `shipped` and `okay_to_delete: true` only after durable
  context is migrated.

## See also

- [`00-roadmap-index.md`](00-roadmap-index.md)
- [`visual-acceptance-contract.md`](visual-acceptance-contract.md)
- [`v0.4.0-cohesive-visual-defaults.md`](v0.4.0-cohesive-visual-defaults.md)
- [`v0.5.0-showcase-stabilization.md`](v0.5.0-showcase-stabilization.md)
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md)
- [`../architecture/dev-panel.md`](../architecture/dev-panel.md)
