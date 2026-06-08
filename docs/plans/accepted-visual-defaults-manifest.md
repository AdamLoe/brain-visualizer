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
| First-load default config | Pending v0.4.0 acceptance | v0.4.0 |
| Hidden dev-panel presets | Pending v0.4.0 implementation | v0.4.0 |
| Camera presets | Pending visual review harness update | v0.4.0 |
| Artifact ledger | Pending accepted captures | v0.4.0 / v0.5.0 |
| Production verification | Pending v0.5.0 | v0.5.0 |

## Baseline identity

Fill these when a look is accepted:

| Field | Value |
|---|---|
| Accepted by | TBD |
| Accepted working tree / commit | TBD |
| Review date | TBD |
| Source plan | `v0.4.0-cohesive-visual-defaults.md` |
| Default preset id | `accepted-default` |
| Performance preset id | `performance-review` |
| Hero preset id | `hero-review` |

## First-load default config

The accepted default must match a clean-profile first load. Do not let this
become a dev-only tuned state.

| Surface | Accepted value / source |
|---|---|
| Seed | TBD |
| Tier | TBD |
| `N` / `K` | TBD |
| `AppConfig` source | `web/src/core/types.ts -> DEFAULT_CONFIG` after v0.4.0 |
| `VisualSettings` payload | TBD artifact JSON |
| `MorphologyConfig` payload | TBD artifact JSON |
| Render quality config | TBD artifact JSON |
| localStorage starting state | Empty profile |
| Reset-to-default behavior | Must return to this config |

## Hidden dev-panel presets

These presets are hidden review tools. They must not appear in the public UI.

| Preset | Purpose | Required behavior |
|---|---|---|
| `accepted-default` | Public first-load look | Matches clean first-load defaults exactly |
| `performance-review` | Lower visual cost comparison | Reduces cost without changing semantics |
| `hero-review` | Screenshot/video capture | May increase cost; must stay dev/review-only if costly |

For each preset, record the exact JSON payload once implemented:

```text
preset_id:
  app_config: TBD
  visual_settings: TBD
  morphology_config: TBD
  render_quality: TBD
  notes: TBD
```

## Camera presets

Use stable camera presets for repeatable review. Fill exact target/eye/distance
values when the harness/browser capture path supports them.

| Preset | Purpose | Camera payload |
|---|---|---|
| default | first screenshot / homepage impression | TBD |
| top | hemisphere and silhouette check | TBD |
| side | non-spherical profile check | TBD |
| oblique | general composition check | TBD |
| close-neuron | soma, branches, material, pulse readability | TBD |

## Artifact ledger

Record accepted artifact paths here even if large files live outside the repo.

| Artifact | Preset | Camera | Source | Path |
|---|---|---|---|---|
| `01-default-first-load.png` | `accepted-default` | default | browser WebGPU | TBD |
| `02-top-brain-shape.png` | `accepted-default` | top | browser WebGPU | TBD |
| `03-side-brain-shape.png` | `accepted-default` | side | browser WebGPU | TBD |
| `04-close-neuron-material.png` | `accepted-default` | close-neuron | browser WebGPU | TBD |
| `05-soma-pulse-sequence.mp4` | `accepted-default` | close-neuron | browser WebGPU | TBD |
| `06-traveling-impulse-sequence.mp4` | `accepted-default` | close-neuron | browser WebGPU | TBD |
| `07-low-tier-default.png` | `performance-review` | default | browser WebGPU | TBD |
| `08-hero-dev-preset.png` | `hero-review` | oblique | browser WebGPU | TBD |

## Acceptance status

| Plan | Accepted artifact source | Status |
|---|---|---|
| v0.3.0 brain-shaped arena | TBD | Pending |
| v0.3.1 curved chain morphology | TBD | Pending |
| v0.3.2 material polish | TBD | Pending |
| v0.3.3 soma pulse / traveling impulse | TBD | Pending |
| v0.3.4 right-click pan | Manual/browser note | Pending |
| v0.4.0 cohesive defaults | This manifest | Pending |
| v0.5.0 stabilization | Production verification | Pending |

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
