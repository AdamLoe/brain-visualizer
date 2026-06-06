# Plans

Working coordination docs for multi-step work on this app. Plans are **not**
canonical architecture: once work ships, the architecture and decisions docs get
updated and the plan is deleted.

The **lifecycle rules, status-frontmatter spec, and ship-time migration
workflow are generic** (agent-docs v1) and live in the kit:
[`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md).
New plans start from the kit skeleton:
[`~/.claude/agent-docs/v1/plan-template.md`](~/.claude/agent-docs/v1/plan-template.md).

## What lives here

Active and recently-shipped plans live in this directory alongside this landing
doc. This index does not maintain an inventory (it would rot) — list the
directory to see what's live:

```
ls docs/plans/
```

The V1 phase plans and the V2 beauty-first plan have shipped; their durable
content was migrated into `architecture/` and `decisions/` and their history
lives in git. The one remaining file is [`future_roadmap.md`](future_roadmap.md)
— the long-lived home for deferred work and considered-and-rejected ideas;
everything else should reach `shipped + okay_to_delete: true` and be deleted.

## Routing

| Need | Read |
|---|---|
| Create a new plan | [`~/.claude/agent-docs/v1/plan-template.md`](~/.claude/agent-docs/v1/plan-template.md) |
| Plan lifecycle / status-metadata rules | [`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md) |
| The doc-update workflow a shipped plan triggers | [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) |

## See also

- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`../index.md`](../index.md) — global router.
