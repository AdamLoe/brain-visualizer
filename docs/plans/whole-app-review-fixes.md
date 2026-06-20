---
status:        shipped
owner:         docs-maintenance
last_updated:  2026-06-20
okay_to_delete: true
long_lived:    false
owning_docs:
  - ../README.md
  - agent-context/maintaining-docs.md
  - agent-context/testing-how-to.md
  - architecture/build-and-deploy.md
  - architecture/connectivity.md
  - architecture/data-model.md
  - architecture/dev-panel.md
  - architecture/gpu-backend.md
  - architecture/gpu-rendering.md
  - architecture/scaling.md
  - architecture/simulation.md
  - architecture/web-frontend.md
  - decisions/backends.md
  - decisions/connectivity.md
  - decisions/dev-tooling.md
  - decisions/interaction.md
  - decisions/rendering.md
  - decisions/scaling.md
---

# Whole-app review fixes

## Mission

Retrospective plan for the whole-app review fixes shipped in commits `4c2f34a`,
`0fca549`, `029da96`, `0fd34ed`, `b4a4926`, and `6e37f6a`. Done means the
fixes are represented in canonical current-state docs, rationale lives in the
matching decisions docs, and this plan is disposable coordination history.

## Scope

In scope: persisted settings/config normalization, honest mobile/WebGPU failure
behavior, cursor stimulation grid bounds, Rust/WGSL weight determinism, lazy
bloom/deferred active pipelines, active morphology depth/alpha layering,
morphology rebuild batching, morphology visual-spike docs drift,
README/doc-maintenance drift, and final verification evidence.

Out of scope: new runtime behavior, new visual tuning, deleting shipped plans,
or changing the long-lived future roadmap.

## Shipped slices

- `4c2f34a` normalized persisted app, visual, and morphology config so stale
  values cannot mask current defaults or cross removed fields back into Rust.
- `0fca549` made mobile scale explicit and made missing/failing WebGPU a
  visitor-facing unsupported state, while fixing cursor stimulation to use the
  actual spatial-grid bounds.
- `029da96` locked WGSL `synapse_weight` against Rust `weight()` and documented
  the fixed-point/layout determinism gate.
- `0fd34ed` moved bloom and active morphology pipeline compilation off the first
  render path, kept bloom targets lazy, and restored real depth-tested alpha for
  active tubes/somas over the additive morphology layers.
- `b4a4926` batched morphology generator/render-quality edits behind the
  Rebuild Morphology button and refreshed README as a short runnable orientation.
- `6e37f6a` repaired morphology visual-spike docs drift in the data model.

## Exit gate

The shipped implementation commits carried their own code/test gates. This
retrospective docs slice is complete when:

- canonical docs contain each durable fact listed in Scope;
- this plan's `owning_docs` list names the migration targets;
- `git diff --check` passes;
- targeted `rg` / `sed` evidence proves both the plan and canonical docs carry
  the migrated facts;
- the docs slice is committed with a clean worktree afterward.

## Migration notes

Migration is complete; this plan is `okay_to_delete: true`.

- Persistence normalization lives in [`architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`architecture/web-frontend.md`](../architecture/web-frontend.md), and
  [`decisions/dev-tooling.md`](../decisions/dev-tooling.md).
- Mobile scale and WebGPU failure behavior live in
  [`architecture/scaling.md`](../architecture/scaling.md),
  [`architecture/web-frontend.md`](../architecture/web-frontend.md),
  [`decisions/backends.md`](../decisions/backends.md), and
  [`decisions/interaction.md`](../decisions/interaction.md).
- Cursor stimulation grid bounds live in
  [`architecture/simulation.md`](../architecture/simulation.md).
- Weight determinism lives in
  [`architecture/connectivity.md`](../architecture/connectivity.md),
  [`architecture/build-and-deploy.md`](../architecture/build-and-deploy.md),
  [`agent-context/testing-how-to.md`](../agent-context/testing-how-to.md), and
  [`decisions/connectivity.md`](../decisions/connectivity.md).
- Lazy bloom and deferred active-pipeline compilation live in
  [`architecture/gpu-backend.md`](../architecture/gpu-backend.md),
  [`architecture/gpu-rendering.md`](../architecture/gpu-rendering.md), and
  [`decisions/rendering.md`](../decisions/rendering.md).
- Active morphology layering/alpha lives in
  [`architecture/gpu-rendering.md`](../architecture/gpu-rendering.md) and
  [`decisions/rendering.md`](../decisions/rendering.md).
- Morphology rebuild batching lives in
  [`architecture/dev-panel.md`](../architecture/dev-panel.md),
  [`architecture/web-frontend.md`](../architecture/web-frontend.md), and
  [`decisions/dev-tooling.md`](../decisions/dev-tooling.md).
- README/doc drift policy lives in [`../README.md`](../../README.md),
  [`docs/index.md`](../index.md),
  [`repository-layout.md`](../repository-layout.md), and
  [`agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md).
- Morphology visual-spike docs drift lives in
  [`architecture/data-model.md`](../architecture/data-model.md),
  [`architecture/simulation.md`](../architecture/simulation.md), and
  [`architecture/gpu-rendering.md`](../architecture/gpu-rendering.md).
- Final gate expectations and the residual real-WebGPU-hardware risk live in
  [`architecture/build-and-deploy.md`](../architecture/build-and-deploy.md),
  [`agent-context/testing-how-to.md`](../agent-context/testing-how-to.md), and
  [`agent-context/repo-rules.md`](../agent-context/repo-rules.md).

## Residual risk

No docs blocker remains. Real browser/WebGPU beauty and performance acceptance
still depends on hardware with an actual WebGPU adapter; llvmpipe gates validate
correctness, not representative throughput.

## See also

- [`index.md`](index.md)
- [`future_roadmap.md`](future_roadmap.md)
- [`~/agent-docs/v1/plan-lifecycle.md`](~/agent-docs/v1/plan-lifecycle.md)
- The owning docs listed in the frontmatter.
