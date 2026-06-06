# Documentation ownership

The canonical owner map — "which one doc owns this concept" — is **structured
data**: [`_meta/ownership.json`](_meta/ownership.json) (agent-docs v1, concept →
owner doc + allowed referencers; no code anchors).

## How to use it

- **Finding an owner:** read `_meta/ownership.json`, match your concept. The
  `owner` is the doc you edit; every other doc *links* to it.
- **The rule itself** ("edit the owner; non-owners link, never redefine; move
  drift back to the owner") is generic and lives in the global authoring rules
  (`~/.claude/agent-docs/v1/rules/authoring-rules.md`, rule 1).
- **Adding owners:** when a concept gets a new owner or a cross-doc conflict
  appears, edit `_meta/ownership.json`.

This file is a thin pointer kept so `See also` links resolve. The data is the
JSON; the rules are global.

## See also

- [`_meta/ownership.json`](_meta/ownership.json) — the owner map (data).
- [`index.md`](index.md) — global router.
- [`architecture/index.md`](architecture/index.md) — what these own.
