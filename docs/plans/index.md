# Plans

Working coordination docs for multi-step work on this app. Plans are **not**
canonical architecture: once work ships, the architecture and decisions docs get
updated and the plan is deleted.

The **lifecycle rules, status-frontmatter spec, and ship-time migration
workflow are generic** (agent-docs v1) and live in the kit:
[`~/.agentdocs/plan-lifecycle.md`](~/.agentdocs/plan-lifecycle.md).
New plans start from the kit skeleton:
[`~/.agentdocs/plan-template.md`](~/.agentdocs/plan-template.md).

## What lives here

Active and recently-shipped plans live in this directory alongside this landing
doc. This index does not maintain an inventory (it would rot) — list the
directory to see what's live:

```
ls docs/plans/
```

The V1 phase plans, the V2 beauty-first plan, the v0.3–v0.5 visual roadmap, and
the branching-tree / heavy-tailed-reach / active-opacity / brain-shell plans have
all shipped; their durable content was migrated into `architecture/` and
`decisions/` and their history lives in git. [`future_roadmap.md`](future_roadmap.md)
is the one long-lived plan: the home for deferred work and
considered-and-rejected ideas. Other files in this directory should be
active/recent coordination docs that eventually reach
`shipped + okay_to_delete: true` or `abandoned + okay_to_delete: true`.

## Routing

| Need | Read |
|---|---|
| Create a new plan | [`~/.agentdocs/plan-template.md`](~/.agentdocs/plan-template.md) |
| Plan lifecycle / status-metadata rules | [`~/.agentdocs/plan-lifecycle.md`](~/.agentdocs/plan-lifecycle.md) |
| Deferred / rejected roadmap items | [`future_roadmap.md`](future_roadmap.md) |
| The doc-update workflow a shipped plan triggers | [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) |

## See also

- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`../index.md`](../index.md) — global router.
