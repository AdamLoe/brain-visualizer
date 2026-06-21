# Maintaining the docs

## When does this apply

You shipped a code change that touched a surface owned by a doc in this tree,
or you are about to write a "v2 added…" / "previously this was…" sentence into
an architecture doc. Stop and read this first.

The **enforceable authoring rules are generic** and live in the global kit:
[`~/.agentdocs/rules/authoring-rules.md`](~/.agentdocs/rules/authoring-rules.md).
This doc is the thin app-specific binding: it points at the rules and at the
slot-data the rules read from [`../_meta/manifest.md`](../_meta/manifest.md).

## The rules in one screen

- **Recoverability test.** If an agent could recover a sentence in ~30s by
  reading the named code file, it's transcription — delete it and leave a
  `path → symbol` pointer (symbol names, never line numbers). Keep only the
  map, invariants, gotchas, and the why.
- **Architecture describes what IS.** Rewrite in place when code changes; no
  dated/versioned framing. History lives in git.
- **Decisions are sectioned by domain, not by date.** Three mandatory fields
  (`Decision`, `Why`, `Applies to`); no `Date:` field. Drop superseded
  decisions — git keeps them.
- **One owner per concept.** [`../_meta/ownership.json`](../_meta/ownership.json)
  names it. Non-owners link, never redefine.
- **README is a short orientation, not the fact owner.** If README changes, keep
  it aligned with [`../index.md`](../index.md),
  [`../repository-layout.md`](../repository-layout.md), and the relevant
  architecture docs; durable runtime facts still belong under `docs/`.
- **No literal counts** unless a named gate fails when the number is wrong.
- **Each leaf ~1–2k tokens, one subsystem per file.** Split past ~2k.

## What changes trigger a doc update

Consult the **`change-to-doc` table** in the manifest before declaring a commit
done: [`../_meta/manifest.md`](../_meta/manifest.md). If your change touches a
surface and you're unsure which doc owns it, query
[`../_meta/ownership.json`](../_meta/ownership.json).

## Workflow when shipping

1. Make the code change; run the per-commit gates (manifest `drift-gates` slot —
   see [`testing-how-to.md`](testing-how-to.md)).
2. Rewrite the architecture doc(s) that own the touched surface — in place.
3. Add a decision entry to `decisions/<domain>.md` if the change embodies a new
   choice.
4. Commit code + docs together.

## See also

- [`~/.agentdocs/rules/authoring-rules.md`](~/.agentdocs/rules/authoring-rules.md) — the generic rules (authoritative).
- [`~/.agentdocs/agent-docs-guide.md`](~/.agentdocs/agent-docs-guide.md) — why the system is shaped this way.
- [`../_meta/manifest.md`](../_meta/manifest.md) — app slot-data (`change-to-doc`, `drift-gates`, `decisions-domains`).
- [`index.md`](index.md) — agent-context router.
