---
status:        draft
owner:         adamg
last_updated:  2026-06-05
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/scaling.md
  - architecture/web-frontend.md
  - architecture/dev-panel.md
  - decisions/scaling.md
  - decisions/interaction.md
---

# Patch 0.1.1 — Remove runtime auto-scaling; persist app config

## Mission

The adaptive scaler is causing multi-second freezes. It fires `grow_n` once per
second whenever it sees frame-time headroom; each grow calls
`GpuBackend.reinitialize()` — a full teardown (rebuild manifold + connectivity,
reallocate every GPU buffer, recompile every render pipeline) that stalls a
single frame for seconds. Because the scaler decides on **p95** and that stall
is always exactly one outlier per 120-frame window, p95 never sees it, reads
"healthy", and grows again — an unbounded feedback loop (observed:
`frame_ms_avg` climbing 26 → 91 ms while `frame_ms_p95` stays pinned at 4.2 ms,
`fps` flapping 0.3 ↔ 240).

For this patch we **remove all runtime auto-scaling**. The network is generated
at a fixed N at startup and stays there; changing scale is a deliberate user
action (tier buttons / dev panel). Separately, fix the config-persistence bug:
the user-chosen scaling (and other `AppConfig` fields) is **never saved to
localStorage** today, so it resets on every reload.

Smart, gentle auto-scaling is explicitly deferred to a later UX-polish patch
(see `future_roadmap.md`) — this patch keeps the pure `scalerDecide` function and
its tests as a dormant seed for that work.

**Done when:** no `reinitialize`-on-scale path runs in the rAF loop; a reload
restores the last-used N/K/tier/backend/speed/brain-state; all gates green.

## Scope

**In scope**

1. **Remove auto-scaling wiring** (no behavior re-introduced):
   - `web/src/main.ts` — delete the `scalerDecide` call + `shrink_n`/`grow_n`
     handling + the `gpuBackend.reinitialize(...)`-on-scale block inside the
     `if (dumped)` section ([main.ts:525-545](../../app/web/src/main.ts#L525-L545)).
     Keep the HUD / sonification / dev-panel updates in that block.
   - `web/src/ui/dev-panel.ts` + `web/src/core/setting-metadata.ts` — remove the
     now-dead `adaptiveScalerEnabled` toggle from the panel.
   - `web/src/core/settings.ts` — leave the `adaptiveScalerEnabled` field at
     **Float32Array index 23 reserved/inert** (do NOT renumber — it's the
     fragile Rust↔TS `VisualSettings` contract). Comment it as reserved.
   - Keep `scalerDecide` + `ScalerAction` ([controls.ts:185-236](../../app/web/src/ui/controls.ts#L185-L236))
     and `web/src/ui/controls.test.ts` as dormant/tested code for the future
     re-arm. The Rust `scaler.rs` is already an unused stub — leave it.

2. **Persist `AppConfig` to localStorage**:
   - Add `saveConfig` / `loadConfig` (versioned key, e.g. `bv2_config_v1`,
     mirroring the `settings.ts` pattern: version gate → defaults on mismatch,
     field-by-field `?? base` merge). Persist `n`, `k`, `tier`, `backend`,
     `speed`, `excitability`. Do NOT persist `seed` (it's a fixed constant) or
     runtime counters.
   - `web/src/main.ts` — load persisted config at boot (merged over
     `DEFAULT_CONFIG`, respecting the mobile override) and save whenever an
     `AppConfig` field changes.
   - `web/src/ui/controls.ts` — `setTier` / `setBackend` / `setSpeed` (and the
     dev-panel N control, if any) call the new save after mutating `config`.

3. **Bump version** `0.1.0 → 0.1.1` in `app/crates/brain-visualizer/Cargo.toml`
   and `app/web/package.json`.

4. **Docs + roadmap**: update the owning docs (below) and add a
   `future_roadmap.md` entry: "smart auto-scaling (gentle, hysteretic, stall-
   aware — decide on avg not p95; cheap resize that skips pipeline recompile)."

**Out of scope**

- Re-introducing any automatic N change (deferred to the future UX patch).
- Making `reinitialize` cheap / splitting buffer-resize from pipeline-rebuild
  (only relevant once auto-scaling returns; note it in the roadmap).
- Migrating persisted localStorage from old keys (no migration; mismatch →
  defaults, consistent with `settings.ts`).

## Approach

Files overlap (`main.ts` and `controls.ts` are touched by both work items), so
implementation is **one coherent stream**, not a parallel fan-out. Parallelism
is in verification.

- **Stream 1 — Implement** (single agent, serial): items 1–3 above as one diff.
- **Stream 2 — Verify** (parallel, after implement): `npm run typecheck`,
  `cargo test`, `npm test`, and an adversarial diff review (did removing the
  scaler block leave the `if (dumped)` HUD/sono/panel updates intact? does the
  config round-trip survive reload? was index 23 left untouched? any dangling
  imports/constants?).
- **Stream 3 — Docs** (after verify): update owning docs + roadmap per the
  change→doc table.

## Exit gate

Run `cargo` from `app/`, `npm` from `app/web/`:

- `cargo test` — green (determinism + dynamics gates unaffected).
- `npm run typecheck` — green.
- `npm test` — green (incl. the retained `scalerDecide` tests in
  `controls.test.ts`).
- Manual/asserted: grep confirms no `grow_n` / `shrink_n` / `reinitialize`
  call remains in the rAF loop; `loadConfig`/`saveConfig` exist and are wired at
  boot + on every `AppConfig` mutation.
- Behavioral smoke (described, browser): change tier, reload → tier restored;
  console no longer emits `[scaler] grow_n` lines.

## Migration notes (filled in at ship time)

Route at ship time:

- **architecture/scaling.md** — runtime auto-scaler removed; N is fixed at
  startup, user-driven only. The `scalerDecide` pure fn / `scaler.rs` stub
  remain dormant.
- **architecture/web-frontend.md** — new `AppConfig` persistence (`loadConfig`
  at boot, `saveConfig` on mutation; `bv2_config_v1` key).
- **architecture/dev-panel.md** — `adaptiveScalerEnabled` toggle removed; index
  23 reserved.
- **decisions/scaling.md** — why runtime auto-scaling was pulled (p95-blind
  feedback loop + reinitialize stall) and deferred.
- **decisions/interaction.md** — scaling is now an explicit user action that
  persists.

## See also

- `docs/plans/index.md` — where live plans land.
- [`../architecture/scaling.md`](../architecture/scaling.md),
  [`../architecture/web-frontend.md`](../architecture/web-frontend.md).
- [`future_roadmap.md`](future_roadmap.md) — deferred smart-autoscaling work.
