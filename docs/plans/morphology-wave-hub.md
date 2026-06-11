---
status:        active
owner:         orchestrator
last_updated:  2026-06-11
okay_to_delete: false
long_lived:    false
owning_docs: []
---

# Morphology wave — orchestration hub

Coordination hub for running three dependency-ordered plans as one wave:

1. [`high-scale-defaults-and-settings-prep.md`](high-scale-defaults-and-settings-prep.md) — **P1**
2. [`active-recent-morphology-rendering.md`](active-recent-morphology-rendering.md) — **P2**
3. [`realistic-branching-and-long-range-signals.md`](realistic-branching-and-long-range-signals.md) — **P3**

This doc is the map. The three plans are the territory. Update the streams
table from *observed* agent results + disk truth, never optimism.

## Operating parameters (from lead, 2026-06-11)

- **Checkpointing:** run all three straight through, P1→P2→P3. Only stop for a
  genuine blocker or a design fork I cannot resolve from code/precedent.
- **Commits:** none. Leave everything in the working tree; lead reviews and
  commits. (Repo default anyway.)
- **One GPU.** Headless WSL2 box runs `cargo test` / examples under llvmpipe
  (software Vulkan), single consumer. GPU-exercising gates run one at a time —
  never two GPU streams concurrently.

## Why serial, not parallel

All three plans touch the same hot files, so they cannot run concurrently:

| File | P1 | P2 | P3 |
|---|---|---|---|
| `crates/brain-visualizer/src/sim/morphology.rs` | tiny (LightingConfig::default) | stats exposure | **heavy** (generate rewrite) |
| `crates/brain-visualizer/src/sim/gpu/mod.rs` | defaults | **heavy** (render_full, compaction) | — |
| `render_morphology.wgsl` | — | indexed reads | pulse timing |
| `web/src/core/settings.ts` | defaults | connectionLayer modes | — |
| `web/src/ui/dev-panel.ts` | lighting→Appearance | connection-mode UI | descriptor grouping |
| `web/src/core/morph-config.ts` | lighting defaults | — | new descriptors |

`morphology.rs` is single-writer by plan rule. Order is therefore
**P1 → P2 → P3**, with limited intra-plan parallelism only between file-disjoint
streams that don't both hit the GPU gate.

## Verified anchors (recon 2026-06-11, all MATCH plan claims)

- DEFAULT_CONFIG: n=1200, k=16, excitability=0.71, ticksPerSec=30 (`types.ts`).
- DEFAULT_SETTINGS: iExt=0.055 (idx12), heterogeneity=0.50 (idx14),
  longRangeReachFrac=0.0 (idx24), maxReachCells=6 (idx25). SETTINGS_LENGTH=26.
- Rust `VisualSettings::default()`/`from_slice` mirror those index-for-index
  (`gpu/mod.rs`).
- morph lighting defaults: restingBrightness=0.05, activeBoost=1.8,
  activeOpacity=1.0, inactiveOpacityFloor=0.0 (`morph-config.ts` ↔ Rust
  `LightingConfig::default`). `morphRestingOpacity`=0.20 (settings idx15),
  `connectionLayer`=1 (settings idx17).
- `_buildAppearanceTab` (dev-panel:1382), `_buildMorphConfigRows` (:1542).
  MORPH_DESCRIPTORS carry `group` ("generator"/"renderQuality"/"lighting") +
  `applyKind` ("uniform"/"regenerate"/"pipeline-rebuild").
- Storage keys bv2_config_v1 / bv2_settings_v1 / bv2_morph_v1; all version-gate
  on mismatch and apply defaults.
- render_full: both additive + active opacity tube passes do
  `pass.draw(0..self.morph_tube_verts, 0..segs)` (segs = mb.segment_count).
- wgsl activity-owner rule confirmed: `select(neuron_id, target_id, kind==0 && target_id!=neuron_id)`.
- `generate` flat/static; `MorphologyTimings` = setup/incoming/dendrite/axon/finalize/total_ms.
- `MorphBuffers` = segment_buffer/segment_count/morph_uniform/sphere_buffer/sphere_count/params/stats.
- Indirect-draw infra EXISTS and is wired: `draw_indirect.wgsl`, `frustum_cull.wgsl` (Phase 4) — reusable for P2.
- `emit_bezier_path` exists; `edge_subsegments` default=3; MorphSegment 48B with path_len (asserted morphology.rs:2330).
- Pulse consts: AXON_IMPULSE_SPEED=0.018, DENDRITE_ECHO_SPEED=0.006, IMPULSE_WIDTH=0.028; impulse_travel/packet/segment_activity exist.

## Decisions log

- **D1** Order = P1→P2→P3, serial on shared files. (dependency + overlap matrix above)
- **D2** P1 flips n=6000 *now*, not deferred behind P2. We run the whole wave;
  any intermediate slowness on the old all-segment renderer is non-user-facing
  and P2 fixes it. Plan explicitly allows either order; source changes identical.
- **D3** No commits (lead). **D4** Straight through, checkpoint at end (lead).
- **D5** P1 ships as a SINGLE implement agent — it's small and its files
  (settings.ts, dev-panel.ts, gpu/mod.rs) would be a merge hazard if split.
- **D6** Storage strategy = version reset. Bump the three key version suffixes
  (`bv2_*_v1` → `bv2_*_v2`) so stale localStorage can't mask the new low-firing
  defaults on dev machines. (P1 plan: "Prefer version reset for this wave.")

## Streams table (observed state)

| Stream | Plan | Area | Status | Last observed fact | Next action | Blockers |
|---|---|---|---|---|---|---|
| P1 | 1 | defaults + dev-panel IA | **DONE** ✅ | all gates green: typecheck, 36/36 vitest, 103/103 cargo. n=6000/exc=0.10/iExt=0.014/LRF=0.14/maxReach=14; resting hidden (restingBrightness=0, morphRestingOpacity=0); lighting→Appearance; keys bumped →v2 | — | — |
| P2-AE | 2 | baseline instrumentation + gen cleanup | **DONE** ✅ | BASELINE: N=1200→112,702 segs; N=6000→692,994 segs (=draw instances ×2 passes). Gen dominated by axon (Prim O(K³), deferred). Low-risk cleanups applied, determinism gates PASS | — | — |
| P2-BC | 2 | active-seg buffers + compaction + indexed render | **DONE** ✅ | compaction selects 4,191/692,994 = **0.60%** at low-firing default (5,480 under stim). New compact_morph_segments.wgsl (reset→compact→write_args); both tube passes draw_indirect; no CPU readback; MorphSegment 48B preserved; legacy path behind DRAW_LEGACY_ALL_SEGMENTS. Determinism gates PASS, render_check PASS | — | — |
| P2-D | 2 | connectionLayer semantics/defaults | **DONE** ✅ | mode 0 skips ALL morphology (tubes+soma+compute, already gated); UI dropdown Off/Active-recent/Resting-debug in Appearance; mode2=const-gated debug (honest tooltip). 48 web + 103 cargo tests pass | — | — |
| P3-A | 3 | visual baseline + stats | **DONE** ✅ | BEFORE: N=6000 692,994 segs (cap 1.78M, 39% util, 0 dropped, max/neuron 2211). Long-range frac 0.14 vs 0.0: +8.9% segs, +188% max/neuron, +6.3x incoming time. Artifacts /tmp/morph_{0..3}.png. Problems: long-range=giant single arcs; dendrites=generic radial fans; no trunk→twig hierarchy | — | — |
| P3-BC | 3 | adaptive subdivision + waypoints | **DONE** ✅ | adaptive_subsegments (len+curvature, det); waypoints via world-dist heuristic (1-3 bowed, target identity preserved — tested). N=6000: 774,733 segs (+12%), max/neuron 2,215, 0 dropped, cap raised to 2.93M (140MB), util 26.5%. 2 new det tests pass, gates green, 48B layout intact | — | — |
| P3-D | 3 | richer local branching | **DONE** ✅ | bushy dendrites (branchlets/twigs/taper/curvature var); presynaptic owner rule preserved + tested; decorations self-owned, no fake target_ids. N=6000: 866,167 segs (+12%), max/neuron 2,231, util 24.7%, 0 dropped. Det gates + new tests pass. **FLAG:** single-storage-buffer 128MiB binding caps ~2.76M segs (~N=12000, pre-existing); decoration ramped to 0 by N=8000 to stay safe | — | — |
| P3-E | 3 | pulse motion (wgsl) | **DONE** ✅ | local/long-range pulse split (LR 2.5× faster/2.1× wider, LONG_RANGE_PATH=0.18); compact_morph synced (per-seg width/speed, headroom=width*4). render_check compaction still 0.59% (5,097/866,167), adequate headroom verified. No uniform; MorphUniforms 192B. Gates green | — | — |
| P3-F | 3 | config/presets | **DONE** ✅ | exposed 3 dendrite controls (branchletCount/twigCount/decorGroupMax, generator/regenerate, runtime-clamped to compile-time maxes); buffer-sized knobs left locked w/ rationale; hero-review preset extended; persistence forward-compat verified. +8 web tests. npm typecheck/56 tests + 107+3 cargo all green | — | — |
| END | — | consolidated gate | **DONE** ✅ | integrated tree GREEN: cargo build 0 warnings, 110 cargo tests, 56 vitest, render_check compaction 0.59%, morph_view 0 dropped (N=1200 & N=6000). e2e skipped (no headless browser) | — | — |
| DOC | — | doc migration (4 buckets) | **DONE** ✅ | R(gpu-rendering/backend/profiling/rendering-dec), M(manifold×2/connectivity×2), S(scaling×2), W(dev-panel/web-frontend/sim/dynamics/dev-tooling+manifest keys) — all migrated, no unresolved anchors, stale statements fixed | — | — |
| CLOSE | — | flip plans shipped+okay_to_delete | **DONE** ✅ | all 3 plans shipped+okay_to_delete:true | — | — |

## WAVE COMPLETE — 2026-06-11

All three plans implemented, integrated, gated green, and doc-migrated. No
commits — full diff in working tree for lead review. The three plans are flipped
to `shipped + okay_to_delete: true` (a later `/clear-plans` sweep, or the lead,
can delete them). This hub is the review hand-off; safe to delete after review.

**Result headlines:**
- P1: product baseline n=6000/k=16, low-firing (excit 0.10, iExt 0.014), heavy-tail
  reach on (frac 0.14 / reach 14), resting morphology hidden by default, lighting
  controls moved to Appearance, storage keys reset to v2.
- P2: morphology now draws only active/recent segments via GPU compaction +
  indirect draw — **0.59% of 866,167 segments drawn** at the default (was 100%).
  connectionLayer 3-mode enum (Off/Active-recent/Resting-debug).
- P3: adaptive subdivision, long-range bowed-waypoint axons (no more giant single
  spans), bushy dendrites with taper/curvature, local-vs-long-range pulse split so
  blue packets sweep long paths. 0 segments dropped at n=1200 and n=6000.
- End gate: cargo build 0 warnings, 110 cargo tests, 56 vitest, examples pass.

## Open questions for lead (raise at final checkpoint)

- **Q1 (visual / beauty call): whole-tree glow halo.** P2-BC's compaction lights
  only the moving *packet band* on active branches, deliberately NOT the old
  faint whole-tree halo that lit a fired neuron's entire arbor for the full glow
  lifetime. Selecting that halo would keep ~all segments and defeat scaling, and
  it overlaps the "resting" structure now hidden by default. Current default =
  packet-only. If you want some bounded whole-arbor afterglow on a fired neuron,
  that's a deliberate add (a Stream-D mode or a P3 pulse-timing choice), not a
  correctness gap. **Eyeball this in the running app.**
- **Q2 (scaling, deferred — out of wave scope): morphology storage-buffer
  ceiling.** Morphology segments are bound as ONE GPU storage buffer against the
  128 MiB `max_storage_buffer_binding_size` limit → hard cap ~2.76M segments
  (~N=12000). Pre-existing, not introduced by this wave. P3-D worked around it by
  ramping dendrite decoration to 0 by N=8000, so the N=6000 default and N=12000
  both stay safe, but full dendrite bushiness is throttled above N≈2400. Proper
  fix = chunk the segment buffer / multi-binding in `resources.rs`+`mod.rs`.
  Belongs to a future scaling plan, not these three. Decide if/when to schedule.
- **Note: synaptic_scale.** Low-firing only emerges with the visual-settings
  default synaptic_scale=0.03 (raw backend default is 1.0). The real app applies
  visual settings, so this is fine; flagging so it isn't a surprise in tests.

## Gate reference

- Web: `cd app/web && npm run typecheck`; `npm test` (focused file).
- Rust/GPU: `cargo test -p brain-visualizer` (llvmpipe); examples
  `render_check`, `morph_view`. One GPU consumer at a time.
- Consolidated end gate runs once after P3.
