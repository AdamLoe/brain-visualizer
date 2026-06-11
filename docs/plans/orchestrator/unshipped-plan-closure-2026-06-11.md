---
status:        shipped
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: true
long_lived:    false
owning_docs: []
---

# Unshipped plan closure — orchestration hub

Disposable tracker for the 2026-06-11 request: finish every plan in
`docs/plans/` that is not shipped yet.

## Scope

Included by default:

- Active, non-long-lived plans in `docs/plans/`.
- Superseded active plans that need closure rather than new implementation.
- Active hubs whose body says their work is complete but whose frontmatter still
  says `active`.

Excluded by default unless the lead says otherwise:

- `future_roadmap.md` because it is explicitly `long_lived: true`.
- New roadmap implementation that is not already staged in an active plan.

## Current read

No plan currently looks like a fresh implementation stream. The active set is
mostly code/docs-done work waiting on final visual/manual acceptance or stale
frontmatter cleanup.

## Assumptions Applied

- `future_roadmap.md` stayed out of scope because it is `long_lived: true`.
- The request to finish all unshipped plans was treated as approval to use native
  visual artifacts plus browser smoke as acceptance evidence for plans that were
  waiting on manual visual/reload approval.
- `visual-product-polish-phase-hub.md` stayed out of scope because it was already
  `status: shipped`; its remaining `okay_to_delete: false` real-WebGPU caveat
  was not part of the "not shipped yet" request.

## Lead questions

- Does `future_roadmap.md` stay out of this run?
- Is it acceptable to prioritize the finished visual default over preserving
  every previously planned setting/control?
- Should shipped-but-not-deletable plans, especially
  `visual-product-polish-phase-hub.md`, be included in this cleanup?

## Streams

| Stream | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|
| Inventory | plans | done | 10 active non-long-lived plans; 1 active long-lived roadmap; 1 shipped non-deletable plan | none | none |
| Corrective visual acceptance | opacity, dendrites, reload | done | `render_check`, `morph_view`, artifact inspection, and server-backed Playwright passed with environment-gated WebGPU device assertions | none | none |
| Migration sanity | architecture/decisions | done | owning docs mention v2 storage keys, 48 B soma instances, continuous opacity, target-owned incoming dendrites, and tombstoned dead settings | none | none |
| Closure edits | plan frontmatter/status | done | active non-long-lived plans flipped to `shipped + okay_to_delete: true`; closure notes added for blockers/supersessions | none | none |

## Gate policy

Per-stream gates stay narrow. Final closure uses the manifest drift gates where
practical: `cargo test -p brain-visualizer`, `npm run typecheck`, `npm test`,
and native examples for render/morphology acceptance. Browser e2e may be
environment-limited.

## Verification Evidence

- `cargo run -p brain-visualizer --example morph_view` passed and regenerated
  `/tmp/morph_0.rgba` through `/tmp/morph_3.rgba`,
  `/tmp/morph_active_bright.rgba`, and stats JSON. N=1200/K=16 produced
  174,633 segments, 0 dropped; N=6000/K=16 produced 866,167 segments, 0 dropped.
- `cargo run -p brain-visualizer --example render_check` passed, including the
  active-opacity continuous low/small/high check and active/recent compaction
  check (`5097 / 866167`, 0.59%, at low-firing default).
- `cargo test -p brain-visualizer` passed, including the long GPU dynamics gate
  and WGSL hash/target determinism tests.
- `npm run typecheck` passed.
- `npm test` passed: 5 files, 56 tests.
- `npm run test:e2e` failed without a server (`ERR_CONNECTION_REFUSED`) after an
  unsandboxed rerun confirmed Chromium could launch; the proper server-backed
  gate `npm run test:e2e:server` passed with 4 passed and 1 expected
  CPU-backend skip. WebGPU adapter device assertions were gated by the WSL2
  environment.
