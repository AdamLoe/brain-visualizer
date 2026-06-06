---
status:        shipped
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: true
long_lived:    false
owning_docs:
  - architecture/active-edges.md
  - architecture/cpu-backend.md
  - architecture/gpu-rendering.md
  - architecture/web-frontend.md
  - decisions/backends.md
  - decisions/scaling.md
  - decisions/dev-tooling.md
  - _meta/ownership.json
---

# Doc editorial cleanup (review-docs follow-up)

## Mission

A `review-docs` editorial pass found the tree is strong but carries three
classes of avoidable debt: (1) machine-generation artifacts, (2) full-fidelity
documentation of retired/parked code that never runs, and (3) plan-phase
vocabulary ("Phase F", "UX round 2") leaking into evergreen docs in violation
of authoring-rule 2. None are structural; all are trim-and-purge. Done = the
tree reads as timeless current-state, retired subsystems are stubbed to
map+why+revival-pointer, and no rendered artifact or undefined phase-reference
remains.

## Scope

**In scope** — five findings:

1. **Stray generation artifacts.** Remove the literal `</content>` /
   `</invoke>` trailers from `architecture/simulation.md`,
   `architecture/gpu-backend.md`, `decisions/scope.md`, `decisions/dynamics.md`.
2. **Trim retired/parked code.** Stub `architecture/active-edges.md` to
   map + why-retired + revival pointer + the mirror-constants gotcha; trim the
   disabled near-LOD sphere/cylinder geometry detail in
   `architecture/gpu-rendering.md`; trim the parked-path internals in
   `architecture/cpu-backend.md`.
3. **Purge phase vocabulary.** Replace every "Phase F" with the timeless
   *condition* it stands for, and "UX round 2" with the timeless fact, across
   `architecture/cpu-backend.md`, `architecture/scaling.md`,
   `architecture/dev-panel.md`, `decisions/backends.md`, `decisions/scaling.md`,
   `decisions/dev-tooling.md`.
4. **Uniform decisions frontmatter.** Drop the YAML frontmatter (incl. the
   banned `last_updated` date field) from the four decisions docs that carry it
   (`data-layout`, `connectivity`, `manifold`, `rendering`) so the cluster is
   uniformly frontmatter-free.
5. **Own the wasm boundary.** Give `architecture/web-frontend.md` an explicit
   wasm-boundary ownership section and register the concept in
   `_meta/ownership.json`.

**Out of scope** — architecture-cluster frontmatter (legitimately uses
`status:`); any code change; re-verifying code claims (that's
`check-docs-consistency-some`); deleting the retired *code* (kept, gated).

## Approach

Disjoint file-sets so streams never collide. Four sonnet subagents run the
meaty per-file rewrites in parallel; the orchestrator does the trivial
mechanical edits directly (also in parallel).

- **Stream A (subagent)** — `architecture/active-edges.md`: stub it (finding 2).
- **Stream B (subagent)** — `architecture/gpu-rendering.md`: trim disabled
  near-LOD detail (finding 2).
- **Stream C (subagent)** — `architecture/cpu-backend.md`: trim parked
  internals (finding 2) + Phase-F purge (finding 3).
- **Stream D (subagent)** — `architecture/web-frontend.md` +
  `_meta/ownership.json`: wasm-boundary ownership (finding 5).
- **Stream E (orchestrator, direct)** — stray-tag removal (finding 1, 4 files);
  Phase-F/UX purge in `scaling.md`, `dev-panel.md`, `decisions/backends.md`,
  `decisions/scaling.md`, `decisions/dev-tooling.md` (finding 3); frontmatter
  drop on the four decisions docs (finding 4).

## Exit gate

- `grep -rn '</content>\|</invoke>' docs/` → no matches.
- `grep -rn 'Phase F\|UX round 2' docs/` → no matches.
- `architecture/active-edges.md` is a stub (no Bézier/perp_dir geometry
  walkthrough), still links the morphology successor and names the
  `MIRRORS scatter.wgsl` locked-constants gotcha.
- The four decisions docs (`data-layout`, `connectivity`, `manifold`,
  `rendering`) have no YAML frontmatter.
- `_meta/ownership.json` parses and has a wasm-boundary concept owned by
  `architecture/web-frontend.md`.
- Every edited doc keeps its `See also` and owner pointers intact.

## Discipline rules

- Authoring rules apply: keep `path → symbol` pointers (never `path:line`),
  preserve every corruption-risk gotcha and cross-link, do not delete the
  *why*. Trimming means removing recoverable-from-code transcription, not
  removing map/invariant/gotcha/rationale.

## Migration notes (filled in at ship time)

This plan is a doc-only change — there is no architecture/decisions content to
migrate; the "migration" is the edits themselves landing. Shipped state:

- **Finding 1 (artifacts):** removed from `simulation.md`, `gpu-backend.md`,
  `decisions/scope.md`, `decisions/dynamics.md`. Exit-gate grep clean.
- **Finding 2 (retired-code trim):** `active-edges.md` stubbed 106→55 lines
  (kept the `MIRRORS scatter.wgsl` gotcha, `DRAW_LEGACY_RIBBONS` gate, revival
  pointer, morphology successor link); `gpu-rendering.md` near-LOD/cylinder
  detail compressed (live sections untouched); `cpu-backend.md` lightly trimmed.
- **Finding 3 (phase vocab):** all "Phase F" / "UX round 2" replaced with the
  timeless condition across the 6 named docs. Exit-gate grep clean.
- **Finding 4 (frontmatter):** dropped from `data-layout`, `connectivity`,
  `manifold`, `rendering`; decisions cluster now uniformly frontmatter-free.
- **Finding 5 (wasm boundary):** `web-frontend.md` now explicitly owns the
  `lib.rs` bridge mechanics; `_meta/ownership.json` gained the `wasm-boundary`
  concept (owner `architecture/web-frontend.md`). JSON validated.

All exit-gate assertions pass. `okay_to_delete: true` — safe for the next
`clear-plans` sweep.

## See also

- The app's [`index.md`](index.md) — live-plans landing.
- [`~/.claude/agent-docs/v1/plan-lifecycle.md`](~/.claude/agent-docs/v1/plan-lifecycle.md).
- [`~/.claude/agent-docs/v1/rules/authoring-rules.md`](~/.claude/agent-docs/v1/rules/authoring-rules.md) — the rules the edits must satisfy.
- The owning docs in the frontmatter.
