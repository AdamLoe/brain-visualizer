# Agent context index

Procedural docs: when working on X, do Y, don't do Z.

## How to use this folder

Load only the procedural doc that matches the situation you're in. Each opens
with a "when does this apply" framing. Under **agent-docs v1** the generic
workflow discipline lives in the global kit
(`~/.claude/agent-docs/v1/rules/`); the docs below hold what's specific to
**this** app and link up to the matching global rule.

## Routing

| Situation | Read | Generic rule (global kit) |
|---|---|---|
| Editing Rust / WGSL / TypeScript here | [`coding-style.md`](coding-style.md) | `rules/coding-style.md` |
| Running the app / examples to see a change | [`dev-loop.md`](dev-loop.md) | — (app-specific) |
| Running tests, adding tests, triaging failures, drift gates | [`testing-how-to.md`](testing-how-to.md) | — (app-specific) |
| Git commits, things to never do | [`repo-rules.md`](repo-rules.md) | `rules/repo-rules.md` |
| Updating docs after a code change | [`maintaining-docs.md`](maintaining-docs.md) | `rules/authoring-rules.md` |
| Orchestrating multi-stream work via sub-agents | [`orchestrating.md`](orchestrating.md) | `rules/orchestrating.md` |
| Creating / shipping a plan | [`../plans/index.md`](../plans/index.md) | `rules/authoring-rules.md` §workflow |

## See also

- [`../index.md`](../index.md) — global router.
- [`../architecture/index.md`](../architecture/index.md) — the current-state facts these procedural docs reference.
- [`../_meta/manifest.md`](../_meta/manifest.md) — app slot-data the global rules plug into.
