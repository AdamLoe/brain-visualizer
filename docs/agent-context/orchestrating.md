# Orchestrating

## When does this apply

You're driving a large multi-stream effort across sub-agents (an audit, a
migration, a broad doc sweep) rather than a single focused change. The operating
manual is **generic** and lives in the global kit.

## App notes

- The offline examples and the determinism gates
  ([`testing-how-to.md`](testing-how-to.md)) are what sub-agents use to verify
  sim/shader work without a browser — point them there.
- When fanning out doc work, give each agent its ownership boundary from
  [`../_meta/ownership.json`](../_meta/ownership.json) and the authoring rules
  ([`maintaining-docs.md`](maintaining-docs.md)) so leaf docs stay consistent.

## See also

- [`~/agent-docs/v1/rules/orchestrating.md`](~/agent-docs/v1/rules/orchestrating.md) — the generic operating manual.
- [`../plans/index.md`](../plans/index.md) — where multi-step work is staged.
- [`index.md`](index.md) — agent-context router.
