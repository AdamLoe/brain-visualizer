# Repo rules

## When does this apply

You're committing, staging, or about to do something hard to reverse. The
generic repo discipline is in the global kit
([`~/agent-docs/v1/rules/repo-rules.md`](~/agent-docs/v1/rules/repo-rules.md));
below is what's specific to this repo.

## App-specific

- **Run the gates before committing.** The current showcase-stabilization gate
  is `cargo test -p brain-visualizer`,
  `cargo run -p brain-visualizer --example render_check`,
  `cargo run -p brain-visualizer --example morph_view`,
  `npm run typecheck`, `npm test`, and `npm run build` — see
  [`testing-how-to.md`](testing-how-to.md).
- **Commit code + docs together.** A change to an owned surface updates its
  architecture doc in the same PR ([`maintaining-docs.md`](maintaining-docs.md)).
- **Keep the project extractable.** The visualizer is developed inside the
  `adamloe.com` repo for now and will be extracted as a folder move — all
  visualizer source, docs, build files, and the COOP/COEP shim stay
  self-contained under the project root. Don't reach into site-level files.
- **Don't lock llvmpipe perf numbers into docs** as if they were GPU benchmarks
  ([`testing-how-to.md`](testing-how-to.md)).
- **When touching first-load defaults, reset flows, or build wiring, also
  verify the production preview path.** If the environment cannot supply a real
  WebGPU adapter, report that as an environment blocker rather than claiming
  visual acceptance.
- Generated artifacts (`pkg/`, `dist/`, `target/`, `node_modules/`,
  `test-results/`) are not committed.

## See also

- [`~/agent-docs/v1/rules/repo-rules.md`](~/agent-docs/v1/rules/repo-rules.md) — generic rules.
- [`testing-how-to.md`](testing-how-to.md), [`maintaining-docs.md`](maintaining-docs.md), [`index.md`](index.md).
