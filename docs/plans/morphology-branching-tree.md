---
status:        draft
owner:         adamg
last_updated:  2026-06-08
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
---

# Morphology branching tree (Prim-like + local relaxation)

## Mission

Replace the hand-tuned "trunk + 2–5 cluster fan" arbor generator in
`crates/brain-visualizer/src/sim/morphology.rs → generate` with a principled
**Prim-like tree growth + local relaxation** generator that produces shared
trunks forking out to targets, with branch **width driven by how much synaptic
weight still flows through each branch**. Goal: arbors that read as real
root/tree structures (smooth curvature, organic forks) instead of a flat fan,
and whose thickness encodes signal-carrying capacity. Done when the morphology
renderer draws these trees, the determinism gates stay green, and
`architecture/manifold.md` + `decisions/manifold.md` reflect the new grammar.

## Scope

**In scope** — the host-side generator only:

- Tree construction: greedy attach of the nearest unclaimed dendrite-socket
  target to the best existing branch node, scored by
  `distance + curvature_penalty + density_penalty + degree_penalty`.
- Local relaxation after each attach (pull internal nodes toward the average of
  parent+children, repel nearby branches), with the axon root and dendrite
  endpoints held fixed.
- Spline emission: each tree edge becomes a sampled Bézier chain →
  `MorphSegment` records (this is where smoothness / subsegment count lives —
  **bullet 2**).
- Width: `radius ∝ √(subtree_weight / total_weight)`, computed bottom-up after
  the tree is built (**bullet 3**).
- Soft fork-degree tendency toward 2–3 children per node (**bullet 4**).
- Promote the generator's magic constants to named `MorphologyConfig` dev-panel
  knobs.

**Out of scope** — does not change: the `MorphSegment` 48 B / `MorphSphereInstance`
32 B / `MorphUniforms` 192 B layout contracts (no new fields — `radius_a`/`radius_b`
already exist); the morphology *render* shader's lighting/packet model; the
connectivity `target`/`weight` rule (consumed, not changed). Width is **static**:
baked once at generation from synaptic weight, full stop — there is no runtime
width path in this plan or planned after it.

## Locked design decisions

These were settled with the owner; an implementer should not relitigate them
without checking back.

- **Width = downstream weight fraction, area-preserving.** Walk the tree
  bottom-up, sum the synaptic `weight(i, j, source_type)` of every target in
  each branch's subtree. `radius = R_trunk · √(subtree_weight / total_weight)`.
  Trunk carries 100% → full radius; each fork sheds its children's share. The
  √ (Murray/Rall, area-preserving) keeps trunks substantial and twigs visible.
  The owner's literal "90% weight → 90% width" linear model was considered and
  rejected as too wispy.
- **Soft fork degree, not a hard cap.** Bullet 4's "only 2, max 3" is a
  *tendency*: a `degree_penalty` term in the attach score makes a node resist
  its 3rd/4th child, and relaxation spreads siblings into fork-like geometry.
  No hard cap. If trunks visibly spray 5+ ways in review, the escalation is to
  convert `degree_penalty` into a hard cap that synthesizes intermediate split
  nodes — note this as the known fallback.
- **Deterministic, Rust, at init.** Generation stays CPU-side at
  `initialize()`. The whole app is seed-reproducible, so the greedy loop and
  relaxation **must use ordered structures and deterministic tie-breaks** — no
  `HashSet`/`HashMap` iteration order in the hot path, no float
  nondeterminism that diverges across runs. All random draws use the existing
  `mix_key`/`hash32` with `salt::*`, disjoint from connectivity draws.
- **Lighting `target_id` contract is preserved.** Internal trunk/fork edges
  carry the **source** neuron id in `target_id` (shared paths stay source-lit);
  only leaf edges (axon → the dendrite socket of a real target) carry the
  **real** target id. This matches the existing morphology lighting contract —
  do not break it.
- **Constants become dev-panel knobs.** The score weights (curvature, density,
  degree), the relaxation lerp/repel strengths and window depth, and the
  per-edge subsegment count all move into the exposed `MorphologyConfig`
  generator group (see `decisions/manifold.md` exposed-vs-protected split).
  The allocation budgets and `salt::*` stay protected.

## Approach

Roughly serial — this is one generator rewrite, not parallel streams. Suggested
decomposition for the implementing agent:

1. **Target set + sockets.** Reuse the existing unique-target resolution and
   the dendrite-socket anchoring (where an axon's targets physically land). The
   tree's leaf set = these socket positions, each tagged with its synaptic
   weight.
2. **Tree growth.** Implement the greedy attach with the four-term score over an
   ordered candidate set. Reuse the `SpatialGrid` for nearby-target queries
   where it helps; note that attach *nodes* are arbitrary points along existing
   edges, not just neurons, so a per-arbor local structure is needed (cheap —
   one neuron's tree is small).
3. **Relaxation.** Local ancestor-window pass after each attach; fixed roots and
   endpoints.
4. **Width pass.** Bottom-up subtree-weight sum → per-node radius → per-segment
   `radius_a`/`radius_b`.
5. **Spline emission.** Tree edges → sampled Bézier → `MorphSegment` list, with
   the subsegment-count knob driving curvature smoothness.
6. **Budget re-derivation.** Recompute `max_segs_per_neuron` against the new
   per-arbor segment counts; keep the no-silent-truncation `dropped` accounting.
   Default-scale-first (~1.2k neurons); cap/degrade gracefully at high-N tiers
   rather than blocking on them.
7. **Stats + harness.** Update `MorphologyStats` and the `examples/morph_view.rs`
   artifact so review can read fork counts, depth, width distribution without
   scraping logs.

## Exit gate

- `cargo test` green — including `crates/brain-visualizer/tests/*determinism*`
  (morphology generation must not perturb connectivity gates) and the
  `MorphSegment` 48 B layout assert.
- `examples/morph_view.rs` runs on llvmpipe and its artifact JSON shows shared
  trunks (segment count well below K × independent-twig baseline), 2–3-leaning
  fork degree, and √-scaled width spread.
- Owner visual-acceptance review per `visual-acceptance-contract.md`.
- `architecture/manifold.md` (morphology section) and `decisions/manifold.md`
  describe the Prim+relax grammar and the area-preserving width rule.

## Discipline rules

Do not add fields to `MorphSegment`/`MorphSphereInstance`/`MorphUniforms` —
this plan ships within the existing layout contracts (`radius_a`/`radius_b`
already carry width). If the design seems to need a new per-segment field, stop
and escalate rather than growing the layout.

## Migration notes (filled in at ship time)

Route the grammar (tree growth + relaxation + width rule) into
`architecture/manifold.md → Neuron morphology geometry`; route the
linear-vs-area-preserving and soft-vs-hard-fork trade-offs into
`decisions/manifold.md`; update the manifest `change-to-doc` row if the
generator surface changes.

## Implementation detail

Everything below is contained to `crates/brain-visualizer/src/sim/morphology.rs`
plus the two cross-language config surfaces (`morph-config.ts`, the descriptor
table) and `examples/morph_view.rs`. No render-shader, `MorphUniforms`,
`buffers.rs`, or connectivity-rule edit is required — `radius_a`/`radius_b`
already carry width, and the lighting contract is honoured purely by how we set
`target_id` (see step 6). `connectivity::weight` already exists
(`crates/brain-visualizer/src/connectivity/mod.rs → weight(i, j, source_type)`)
and is *consumed*, never changed.

### 0. What stays untouched (contract anchors)

- `MorphSegment` (48 B), `MorphSphereInstance` (32 B), `MorphUniforms` (192 B):
  no field add/reorder. The two layout asserts
  (`segment_layout_is_48_bytes`, `sphere_instance_layout_is_32_bytes`) must keep
  passing unchanged. `emit_bezier_path`, `cubic_bezier`, `bend_vector`,
  `target_socket`, `dir_from_hashes`, the vec helpers (`add/sub/scale/len/norm/
  cross/perp`), `unit`, `lerp`, `clamp01`, `build_source_types`,
  `emit_soma_spheres` are all reused as-is.
- The `salt` module stays the determinism namespace. Add new salts there
  (next free range `0x00A0_0005..`) for the tree-growth and relaxation draws; do
  NOT reuse the connectivity salts and do NOT touch `connectivity::salt`. New
  salts are protected (never exposed to the config), same as the existing four.
- The protected budget fields (`dendrite_budget`, `trunk_cluster_budget`,
  `terminal_twig_budget`, `cap_slack`) and `GeneratorConfig::apply_to`'s
  re-locking of them stay protected — the new knobs added below join the EXPOSED
  generator group only.

### 1. New config knobs — keep the 4-way contract aligned in one commit

The "constants become dev-panel knobs" decision touches FOUR files that must
move together (the manifest `morph-config.ts` row): the Rust `GeneratorConfig`
struct + its `from_params`/`apply_to` field lists + `MorphologyParams` +
`MorphologyParams::to_json`, AND the TS `MorphGeneratorConfig` interface +
`DEFAULT_MORPH_CONFIG.generator` + `MORPH_DESCRIPTORS`. The field names/shape are
verified against the contract table on each side independently, so add the new
fields with identical camelCase↔snake_case names and identical default values on
both sides in the same edit. Add these generator fields (names final):

- `tree_score_curvature` (f32, default ~0.5) — curvature term weight.
- `tree_score_density` (f32, default ~0.5) — density/repel term weight.
- `tree_score_degree` (f32, default ~0.7) — degree-penalty weight (soft fork).
- `relax_lerp` (f32, default ~0.25) — pull-to-mean strength per relax pass.
- `relax_repel` (f32, default ~0.15) — sibling/branch repulsion strength.
- `relax_window` (usize/int, default 3) — ancestor-window depth relaxed per
  attach.
- `edge_subsegments` (usize/int, default 3) — Bézier samples per tree edge
  (the curvature-smoothness knob; replaces the per-class
  `*_samples` only for the new axon tree path — see step 7 on the old fields).
- `trunk_radius` (f32; reuse `axon_root_radius_fraction` × `base_radius` — do
  NOT add a redundant field; `R_trunk` in the width formula = the existing axon
  root radius). Keep `axon_root_radius_fraction` as the trunk-radius source.

Because `GeneratorConfig` is `#[serde(default)]` per field, an old persisted
`bv2_morph_v1` blob missing the new keys still deserializes (falls back to the
new defaults) — but the TS persistence does a per-group merge over
`DEFAULT_MORPH_CONFIG.generator`, so bumping nothing else is fine; the saved blob
simply gains the new defaults. Update `to_params`'s round-trip test
(`locked_default_matches_current_constants`) for any new default values.

Decide per-field whether each old axon-class field
(`cluster_min/max`, `trunk_root_samples`, `cluster_branch_samples`,
`terminal_twig_samples`, `trunk_length_fraction`, `cluster_split_fraction`,
`root_radius_fraction`, `cluster_radius_fraction`, `twig_radius_fraction`)
survives. The Prim tree replaces the trunk→cluster→twig three-tier scheme, so
those specific knobs lose meaning. **Recommended:** keep the fields in the
struct (to avoid a churny multi-file contract break) but stop reading them in the
new axon path; OR remove them and their descriptors atomically across all four
surfaces. Resolve this with the owner if unsure — see open question. The
dendrite fields (`dendrite_*`, `socket_*`, `base_radius`, `taper_curve`,
`axon_stop_fraction`, `axon_curve_lift`) are unchanged and stay.

### 2. Per-arbor tree data structure (host-local, never uploaded)

Inside `generate`, for each neuron `i`, build a transient tree, then flatten it
to `MorphSegment`s. Do NOT add public structs to the layout; these are private
to the function/module:

```text
struct ArborNode { pos: [f32;3], parent: Option<usize>, children: Vec<usize>,
                   target_id: Option<u32>, weight: i64, radius: f32 }
```

Use index-keyed `Vec<ArborNode>` (NOT HashMap) so iteration is ordered and
deterministic — the locked "ordered structures, deterministic tie-breaks" rule.
Node 0 = the soma/axon-root (held fixed). One leaf per unique target. Internal
nodes are arbitrary points spawned during attach (step 3).

### 3. Target set + per-target weight (reuse existing, add weight)

Reuse the existing unique-target resolution verbatim (lines ~1034–1051): iterate
`j in 0..k`, `connectivity::target_with_cell(...)`, drop self, dedup with the
ordered `seen_targets`/`unique_targets` + `unique_targets.sort_unstable()`. This
preserves `emits_one_terminal_per_unique_target_under_real_source_types` and
`mixed_ei_source_types_match_target_with_cell` — keep that coverage invariant
(one terminal per unique non-self target).

NEW: accumulate per-unique-target weight. A unique target may be hit by several
`j` draws (duplicates). Sum `connectivity::weight(id, j, src_type)` over every
`j` that resolved to that target id (use `i32` from `weight`, accumulate into
`i64`; clamp negatives — inhibitory weights are negative — to their absolute
value or floor at a small positive so √ is real and a twig is never zero-width:
`w = weight.unsigned_abs().max(1)`). This is the leaf weight. Reuse the existing
duplicate accounting; just add the weight sum alongside it. Each leaf's socket
position comes from the existing `target_socket(seed_lo, id, plan, params)` —
that is the fixed leaf endpoint (held fixed in relaxation), and the existing
`terminal_socket_distance_bands` / `socket_reuse_bands` stat updates stay.

### 4. Tree growth (Prim-like greedy attach)

Seed the tree with node 0 at the soma. Maintain an ordered `Vec` of unattached
leaf indices (sorted by `target_id`, already deterministic). Loop until all
leaves attached:

- For each unattached leaf, find its best attach point over existing nodes AND
  along existing edges. Score =
  `distance + curvature*curvature_penalty + density*density_penalty +
   degree*degree_penalty`, weighted by the step-1 knobs. Curvature penalty =
  angle between the candidate parent's incoming direction and the new edge
  (use `dir·dir` / `acos`, or `1 - dot` to avoid trig — prefer the dot form for
  determinism and speed). Density penalty = proximity to already-placed nodes
  (reuse `SpatialGrid` only if it helps for cross-arbor queries; within one
  arbor a linear scan over the small node `Vec` is cheaper and simpler — a
  single neuron's tree is ≤ k leaves + internal nodes, k≈16). Degree penalty =
  monotonic increasing in the candidate parent's current `children.len()`
  (e.g. `children.len().saturating_sub(1)` so the 1st child is free, 2nd cheap,
  3rd+ progressively penalized) — this is the soft fork-degree tendency, NOT a
  hard cap.
- Attach the globally-best `(leaf, attach_point)` pair this iteration (Prim:
  one edge per step). If the attach point is mid-edge, split that edge by
  inserting an internal node at the projection point (this is where shared
  trunks/forks emerge). **Tie-break deterministically**: when scores are within
  an epsilon, break by `(leaf target_id, candidate node index)` ascending —
  never by `Vec`/iteration accident, never float-equality-dependent.

All random draws here (e.g. any jitter on the split point) use
`mix_key(seed_lo, id, <leaf or node index>, salt::TREE_*)` with the new salts —
disjoint from connectivity and from the dendrite salts.

### 5. Relaxation (local, fixed endpoints)

After each attach (or after each batch — pick one and document it; per-attach is
simplest and matches the plan), run `relax_window`-deep passes over the
just-touched ancestor chain: move each INTERNAL node toward the mean of
`parent + children` by `relax_lerp`, then apply `relax_repel` away from nearby
nodes. Node 0 (axon root) and all leaf nodes (sockets) are **held fixed** —
never moved. Determinism: process nodes in ascending index order; repulsion
neighbour set is the ordered node `Vec`, not a spatial-hash iteration. No
float-nondeterministic reductions (sum in a fixed index order).

### 6. Width pass (bottom-up, area-preserving) — the locked rule

After the tree is final, compute `total_weight` = sum of all leaf weights
(reuse the step-3 accumulation; this is the trunk's 100%). Walk bottom-up
(process nodes in reverse topological / descending-by-depth order; since parents
have lower indices than children if you always push children after parents,
a simple reverse-index pass works — assert/guarantee that ordering): each node's
`subtree_weight` = its own leaf weight (0 for internal) + Σ children
`subtree_weight`. Then
`radius = R_trunk * sqrt(subtree_weight as f32 / total_weight as f32)`, where
`R_trunk = base_radius * axon_root_radius_fraction` (existing field). Clamp to a
small floor (e.g. `R_trunk * twig_radius_fraction` or a fixed `1e-4`) so the
thinnest twig stays visible and `radius > 0` (the test asserts
`radius_a > 0 && radius_b > 0`). The √ is the locked area-preserving
(Murray/Rall) choice — do NOT substitute the linear model.

### 7. Spline emission (tree edges → MorphSegment)

For each tree edge (parent→child), emit a sampled cubic-Bézier chain via the
existing `emit_bezier_path`, with `samples = edge_subsegments` (the new knob),
`r0 = parent.radius`, `r3 = child.radius`, taper via existing `taper_curve`,
and control points built with the existing `bend_vector` (curve magnitude scaled
by `axon_curve_lift`, as the current axon path does). `path_len` start for a
child edge = the parent edge's end path (carry the cumulative path forward per
node, exactly as the current code threads `root_path_end`/`cluster_path_end`)
so `path_lengths_match_parent_branch_endpoints` keeps passing.

**Lighting `target_id` contract (LOCKED):** internal edges (both endpoints are
trunk/fork nodes, child has children or is not a leaf) carry `target_id = id`
(the SOURCE neuron) → shared paths stay source-lit. Only a LEAF edge (child is a
socket/terminal node) carries `target_id = leaf.target_id` (the real target).
This is exactly the existing rule (root/cluster use `id`; terminal twig uses
`plan.target_id`) — preserve `kind = 1` for all axon edges and
`kind = 0`/`target_id = id` for dendrites (dendrites are UNCHANGED — keep the
current dendrite block verbatim; this plan only rewrites the axon arbor). The
test `emits_one_terminal_per_unique_target_under_real_source_types` asserts both
that every unique target appears once as a leaf `target_id` AND that
source-`target_id` shared segments exist — the tree satisfies both by
construction.

Single-target fast path: when `unique_count == 1`, keep emitting a direct twig
with NO shared-root segment (the test
`single_target_path_emits_direct_twig_without_shared_root` asserts no
`target_id == id` axon segment exists for that neuron). In the tree model this is
the trivial tree (root → one leaf, one edge, leaf edge carries the real target).
Make sure no internal `target_id == id` axon segment is emitted in that case.

### 8. Budget re-derivation (no silent truncation)

The new per-arbor segment count = (edges) × `edge_subsegments`. Edges ≈
(leaves + internal split nodes); internal nodes ≤ leaves − 1 for a binary-ish
tree, so edges ≈ 2·k worst case. Update `MorphologyParams::segment_cap(k)` and
the named budgets so
`cap = dendrite_budget + trunk_cluster_budget + k*terminal_twig_budget +
cap_slack` still bounds the new grammar. Concretely: keep `dendrite_budget` as
DENDRITE_MAX (dendrites unchanged); set `trunk_cluster_budget` /
`terminal_twig_budget` so the total covers `~2k edges × edge_subsegments` at the
default `edge_subsegments`. Because `edge_subsegments` is now an EXPOSED knob but
the budgets are PROTECTED, the cap must be derived from the *default*
`edge_subsegments` with enough `cap_slack` headroom for the knob's max (the
descriptor max, e.g. 4) — OR clamp the effective subsegment count used for
buffer-cap purposes. Simplest: size the budget for the descriptor-max
`edge_subsegments` so the cap never under-allocates regardless of the slider.
Keep the existing `dropped` accounting and the `eprintln!` cap-hit log; default
scale (~1.2k neurons / k=16) must show `dropped == 0`
(`generates_segments_for_every_neuron`, `emits_one_terminal...` assert
`m.dropped == 0`). Update `max_segs_per_neuron`/`segment_cap` and the
`dendrite_segment_budget_matches_sampled_branch_grammar` test if the dendrite
budget math is touched (it should not be — dendrites are untouched).

### 9. Stats + harness

Extend `MorphologyStats` so review reads tree shape without log-scraping. Add
(and wire into `to_json`, the `stats_json_contains_core_fields` test, and the
fields list): `fork_degree_histogram: [u32; 6]` (count of internal nodes by
child count, index 5 = "5+", the soft-fork evidence), `tree_depth_max: u32` /
`tree_depth_mean: f32` (shared-trunk evidence), and a width-spread summary
(e.g. `radius_bands: [u32; 4]` or min/mean/max radius). Keep the existing
`cluster_count_histogram` field name OR rename it — if renamed, update the
`to_json` format string, the example artifact parser, and the test; prefer
repurposing `cluster_count_histogram` → fork histogram to avoid a struct churn,
but document the rename in the migration note. The `examples/morph_view.rs`
artifact already serializes `morph_stats.to_json()` (line ~543) — bump
`ARTIFACT_TAG` from `"0.2.1"` and the `*_0.2.1_stats.json` path to the new
version so the reviewer can diff old vs new, and confirm the new stat fields
appear in the artifact JSON. The shared-trunk acceptance signal = total segment
count well below the `k × independent-twig` baseline; that falls out of
`segment_count` already in the artifact.

### Order of edits

1. Add new salts + new `GeneratorConfig`/`MorphologyParams` fields +
   `from_params`/`apply_to`/`to_params`/`to_json` + TS interface/defaults/
   descriptors (all four surfaces, one commit) — then `cargo test` +
   `npm run typecheck` to prove the contract still round-trips before touching
   generation logic.
2. Replace ONLY the axon-arbor block of `generate` (lines ~1031–1339) with
   the tree build → relax → width → spline pipeline (steps 2–7). Leave the
   dendrite block (lines ~917–1029) and the unique-target resolution verbatim.
3. Re-derive budgets / `segment_cap` (step 8).
4. Extend `MorphologyStats` + `morph_view` artifact (step 9).
5. Update `architecture/manifold.md` (Neuron morphology geometry +
   max_segs/budget paragraph) and `decisions/manifold.md` ("Shared arbor"
   decision → Prim+relax grammar; add the area-preserving width rule and the
   soft-vs-hard fork trade-off). Update the manifest `change-to-doc`
   `morph-config.ts` row only if the exposed field set changes (it does).

### Gate commands (run from `app/`)

- `cargo test -p brain-visualizer` — runs the in-file morphology determinism +
  coverage tests (`deterministic_for_same_seed`, `seed_changes_morphology`,
  `emits_one_terminal_per_unique_target_under_real_source_types`,
  `single_target_path_emits_direct_twig_without_shared_root`,
  `path_lengths_match_parent_branch_endpoints`, the 48 B / 32 B layout asserts)
  AND the connectivity determinism gates
  (`tests/wgsl_hash_determinism.rs`, `tests/wgsl_target_determinism.rs`) — these
  must stay green to prove morphology draws did not perturb the connectivity hash
  namespace.
- `cargo run --example morph_view` — produces `/tmp/morph_view_stats.json`
  (and the bumped versioned path). Inspect: shared trunks (low `segment_count`
  vs baseline), 2–3-leaning `fork_degree_histogram`, √-scaled width spread,
  `dropped == 0`.
- `cd web && npm run typecheck && npm test` — TS contract (interface +
  descriptors + defaults) compiles and the morph-config round-trip vitest passes.

## See also

- The app's [`index.md`](index.md) — where live plans land.
- [`../architecture/manifold.md`](../architecture/manifold.md) — current arbor grammar.
- [`../architecture/gpu-rendering.md`](../architecture/gpu-rendering.md) — morphology pass that draws the result.
- [`../decisions/manifold.md`](../decisions/manifold.md) — exposed-vs-protected param split.
- [`heavy-tailed-synapse-reach.md`](heavy-tailed-synapse-reach.md) — coupled: long-range synapses produce long terminal axons this generator must route.
