---
status:        active
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - plans/*
  - architecture/gpu-rendering.md
  - architecture/manifold.md
  - architecture/profiling.md
---

# Visual Acceptance Contract

Shared review contract for the v0.3-v0.5 visual roadmap. Individual plans may
add plan-specific captures, but they should not redefine what "accepted review
frames" means.

## Baseline config

- **Seed:** use the first-load default seed unless a plan says otherwise.
- **Scale:** use first-load default `N`/`K` for the core pass; add tier captures
  only when the plan changes tier behavior.
- **Profile:** start from empty localStorage, then record any review-only
  overrides in the artifact JSON.
- **Resolution:** use a fixed square offscreen size for native harness captures;
  use a real browser WebGPU viewport for final beauty/performance review.

The exact config must be recorded with each artifact. Do not rely on a screenshot
filename to imply the seed, tier, settings, or camera.

## Accepted defaults manifest

[`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md)
is the coordination artifact for accepted visual state. It records:

- first-load `AppConfig`, `VisualSettings`, `MorphologyConfig`, and render
  quality values;
- hidden dev-panel preset payloads for default, performance, and hero review;
- canonical camera presets for default, top, side, oblique, and close-neuron
  captures;
- artifact paths and the config JSON used to produce them.

Before an agent claims a visual plan is ready for subjective review, its artifact
JSON must state whether it used the current accepted defaults manifest, a
plan-specific override, or an experimental config. v0.4.0 fills the manifest;
v0.5.0 verifies production behavior against it.

## Capture set

Each visual plan should capture the smallest relevant subset of these views:

| View | Purpose |
|---|---|
| shell-only | proves the brain shape without glow doing the work |
| neuron-cloud-only | proves placement follows the accepted shape |
| morphology resting | proves close structure without activity distraction |
| active simulation | proves activity readability in the default composition |
| pulse sequence | proves soma flash and traveling impulse timing |
| low-tier default | proves reduced settings still look intentional |

Camera presets:

| Preset | Purpose |
|---|---|
| front/default | first screenshot / homepage impression |
| top | hemisphere and silhouette check |
| side | non-spherical profile check |
| oblique | general composition check |
| close-neuron | soma, branches, material, and pulse readability |

## Artifact handling

- Native harness artifacts may live under `/tmp` during review; the artifact JSON
  must list all image/video paths and the exact config.
- Large screenshots and videos do not need to be committed unless the repo
  already stores that artifact class.
- If a plan updates a harness, include the expected artifact names in that plan's
  exit gate.
- Browser screenshots/video are the final beauty review source. llvmpipe/native
  captures validate shader correctness and gross composition only.
- Artifact JSON must include: plan id, git commit or working-tree note, seed,
  `N`, `K`, tier, localStorage reset state, preset id if any, visual settings,
  morphology config, camera preset, viewport/resolution, and all output paths.
- If hidden dev-panel presets are used, the artifact JSON must identify the
  loaded preset. Do not infer preset identity from a filename.

## Pass / Fail

- **Brain shape passes** only if shell-only and neuron-cloud-only views read as a
  brain without color modes, bloom, or morphology hiding the silhouette.
- **Cortical placement passes** only if the neuron cloud reads as tissue near the
  surface, not a uniformly glowing solid volume.
- **Curved morphology passes** only if branches no longer read as straight sticks
  or symmetrical starbursts at close and medium distance.
- **Material polish passes** only if it improves close-up somas/branches without
  glitter, banding, temporal crawl, or washed-out color modes.
- **Pulse visuals pass** only if a firing event reads in motion as soma flash
  followed by a compact traveling impulse; whole-arbor instant glow must not be
  the dominant cue.
- **Pan passes** only if left-drag orbit is unchanged and right-drag pan moves
  predictably in screen space; Shift-left-drag must do the same if that fallback
  is implemented.
- **Cohesive defaults pass** only if a clean first-load page opens to the accepted
  look without dev-panel interaction.

Adam is the final visual approver. Agents should report artifacts and the
observed result; they should not mark subjective visual work accepted on their
own.

## See also

- [`00-roadmap-index.md`](00-roadmap-index.md)
- [`v0.3.0-brain-shaped-arena.md`](v0.3.0-brain-shaped-arena.md)
- [`v0.3.1-curved-chain-morphsegments.md`](v0.3.1-curved-chain-morphsegments.md)
- [`v0.3.2-neuron-material-texture-pass.md`](v0.3.2-neuron-material-texture-pass.md)
- [`v0.3.3-soma-pulse-and-traveling-impulses.md`](v0.3.3-soma-pulse-and-traveling-impulses.md)
- [`v0.4.0-cohesive-visual-defaults.md`](v0.4.0-cohesive-visual-defaults.md)
