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
lives in git. [`future_roadmap.md`](future_roadmap.md) is the one long-lived
plan: the home for deferred work and considered-and-rejected ideas. The active
v0.3-v0.5 visual roadmap is coordinated through
[`00-roadmap-index.md`](00-roadmap-index.md), and subjective visual review uses
[`visual-acceptance-contract.md`](visual-acceptance-contract.md). The accepted
look is tracked in
[`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md),
and detailed planning handoff briefs live in
[`implementation-review-briefs.md`](implementation-review-briefs.md). Other
files in this directory should be active/recent coordination docs that
eventually reach `shipped + okay_to_delete: true` or
`abandoned + okay_to_delete: true`.

## Routing

| Need | Read |
|---|---|
| Create a new plan | [`~/.claude/agent-docs/v1/plan-template.md`](~/.claude/agent-docs/v1/plan-template.md) |
| Plan lifecycle / status-metadata rules | [`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md) |
| Active v0.3-v0.5 visual roadmap | [`00-roadmap-index.md`](00-roadmap-index.md) |
| Visual review acceptance gates | [`visual-acceptance-contract.md`](visual-acceptance-contract.md) |
| Accepted first-load defaults / hidden review presets / artifact ledger | [`accepted-visual-defaults-manifest.md`](accepted-visual-defaults-manifest.md) |
| Detailed implementation-planning briefs for future agents | [`implementation-review-briefs.md`](implementation-review-briefs.md) |
| Deferred / rejected roadmap items | [`future_roadmap.md`](future_roadmap.md) |
| The doc-update workflow a shipped plan triggers | [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md) |

## See also

- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
- [`../index.md`](../index.md) — global router.
