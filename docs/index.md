# Documentation index

This tree is the canonical, current-state snapshot of the brain_visualizer
stack, optimized for LLM consumption: start small, route by task, load only the
subtree that matches the work. It follows **agent-docs v1** — the generic
authoring rules and the workflow/maintenance commands live in the global kit
(`~/.claude/agent-docs/v1/` and the global skills); this tree holds only what's
specific to **this** app.

If you are a fresh AI chat: run **`/fresh-chat`**. It routes you here, then to
[`overview.md`](overview.md), then to the smallest matching subtree.

## How to use this tree

- **System overview** — [`overview.md`](overview.md). Read at chat start for
  shape (and the "current state vs old docs" gotchas).
- **File inventory** — [`repository-layout.md`](repository-layout.md). Read only
  when you need to find where code lives.
- **Architecture** describes **what currently IS**. Route through
  [`architecture/index.md`](architecture/index.md).
- **Decisions** explain **why** current choices exist. Route through
  [`decisions/index.md`](decisions/index.md).
- **Agent-context** docs are procedural ("when working on X, do Y"). Route
  through [`agent-context/index.md`](agent-context/index.md).
- **Plans** stage multi-step work. Route through
  [`plans/index.md`](plans/index.md).
- **Ownership** is data in [`_meta/ownership.json`](_meta/ownership.json);
  [`ownership.md`](ownership.md) explains it. **App bindings** for the global kit
  are [`_meta/manifest.md`](_meta/manifest.md).

## Global routing

| Need | Read |
|---|---|
| System at a glance (+ current-state gotchas) | [`overview.md`](overview.md) |
| Where files live | [`repository-layout.md`](repository-layout.md) |
| Current subsystem facts | [`architecture/index.md`](architecture/index.md) |
| Design rationale | [`decisions/index.md`](decisions/index.md) |
| Workflow / coding / testing / doc-upkeep guardrails | [`agent-context/index.md`](agent-context/index.md) |
| Run the app or an example | [`agent-context/dev-loop.md`](agent-context/dev-loop.md) |
| App bindings for the global kit (change→doc table, drift gates) | [`_meta/manifest.md`](_meta/manifest.md) |
| Canonical owner for a concept | [`_meta/ownership.json`](_meta/ownership.json) |
| Plan status rules or active plans | [`plans/index.md`](plans/index.md) |

## See also

- [`overview.md`](overview.md)
- [`repository-layout.md`](repository-layout.md)
- [`ownership.md`](ownership.md)
