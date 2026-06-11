---
status:        active
owner:         unassigned
last_updated:  2026-06-09
okay_to_delete: false
long_lived:    false
owning_docs:
  - architecture/manifold.md
  - architecture/gpu-rendering.md
  - decisions/manifold.md
  - decisions/rendering.md
---

# Organic soma redesign

## Mission

Make the soma read as a living cell body rather than "a sphere with cylinders
poking in and out." Chosen direction: **organic geometry + blend, where the
deformation is driven by the cell's own connections** — think of a sphere being
**stretched out in the directions of its processes** (trunk/axon root + dendrite
roots), with the branch roots faired into the bulges so there's no hard
sphere↔cylinder seam. The shape is therefore *meaningful*, not random noise: a
neuron pulled hard in one direction by a thick trunk reads that way. Done when a
close-up soma looks like a continuous membrane stretched toward where its
processes leave, with no visible intersection seam, holding up across the three
`morph_view` camera distances. This plan consumes the shared process-root/socket
contract: it should not invent a separate root-direction calculation from the
axon trunk plan.

## Scope

In scope:

- The soma geometry/instance: `crates/brain-visualizer/src/sim/morphology.rs →
  emit_soma_spheres` / `MorphSphereInstance`, and the soma render sub-pass
  `render_morphology.wgsl → vs_sphere / fs_sphere` (+ the active variant
  `fs_sphere_active`).
- Connection-driven soma shape: the UV sphere is **stretched/bulged toward the
  root directions of its processes** (the trunk dominates; dendrite/axon roots
  contribute), deterministic, no mesh assets. The displacement field is a function
  of the per-soma set of process-root directions (and ideally their thickness), so
  the soma needs access to that data through the shared process-root/socket
  contract.
- Seam treatment where trunk/dendrite roots meet the soma (visual fairing — the
  bulge toward a root naturally helps, plus root flare on the tube side / a shared
  blend region).

Out of scope: the axon trunk/branch geometry itself (its own plan — but the
trunk-meets-soma junction is the shared boundary; coordinate), connectivity,
opacity layer, far-LOD billboards, and top-few-root deformation beyond the
dominant trunk unless trunk-only deformation fails visual review.

## Approach

The look is chosen; the remaining preflight is the data path and layout:

1. **Resolve the shared process-root/socket contract first.** Use the dominant trunk
   direction/radius/weight emitted by the axon-root calculation. Do not make
   `vs_sphere` scan `MorphSegment` for near-soma roots; that pushes indexing and
   ownership complexity into the render shader.
2. **Bake compact soma deformation data.** Preferred path: extend
   `MorphSphereInstance` if the dominant-root fields stay compact and
   16-aligned; otherwise add a parallel one-record-per-soma deformation buffer.
   Either way, update Rust + WGSL layout together and keep size asserts. The
   first implementation should carry one dominant direction plus strength/radius,
   not an arbitrary list of roots.
   Plain version: today's soma draw only receives center/radius/neuron id. To
   stretch the soma toward the trunk, the sphere shader must also receive "pull
   this soma in direction D with strength S." Store that per-soma data directly
   instead of asking the shader to inspect branch segments and infer it.
3. **Implement** trunk-dominant displacement: stretch/bulge the UV sphere along
   the dominant root direction, weighted by root thickness so the trunk visually
   pulls the cell body.
4. **Junction fairing** — the stretch toward the root already softens the seam;
   finish it so the roots read as growing out of the membrane, not punched into it.

## First pass implemented

The first organic soma pass widens `MorphSphereInstance` in place from 32 B to
48 B. The appended 16-byte block carries one dominant `root_dir` plus one
bounded `root_pull` strength baked from the existing host-side `ProcessRoot`.
`emit_soma_spheres` consumes the same descriptor produced for the axon trunk;
the shader does not scan `MorphSegment` or invent a second root-direction rule.

`render_morphology.wgsl → vs_sphere` deforms the generated UV sphere before the
existing spike pulse scale: vertices facing the dominant root get a forward pull
and radial bulge, side vertices get a small shoulder, and the opposite side is
slightly compressed. This is intentionally trunk-dominant first-pass fairing;
top-few-root deformation and incoming dendrite sockets remain deferred.

## Exit gate

- `cargo test` green; if `MorphSphereInstance` changed, the 48 B (or new) size
  assert and the Rust↔WGSL layout match are updated together.
- `examples/render_check.rs` still passes (active-opacity proof, region colors,
  non-black) and `morph_view` frames show a seam-free organic soma.
- `architecture/manifold.md` (soma instance) + `architecture/gpu-rendering.md`
  (soma sub-pass) + `decisions/rendering.md` updated.
- The soma deformation uses the same dominant root descriptor as the axon trunk;
  no duplicate root-direction convention exists in shader code.

## Open questions

- How strong the stretch — subtle ovoid lean, or a clearly pulled, pear-like body?
  (Worth a reference image before the implementer starts.)
- If trunk-only deformation looks too simple in review frames, add top-few root
  deformation as a follow-up rather than expanding the first implementation.

## See also

- `architecture/manifold.md` — `MorphSphereInstance` 48 B contract + `emit_soma_spheres`.
- `architecture/gpu-rendering.md` — soma sphere sub-pass, active-opacity layer.
- `docs/plans/morphology-process-root-contract.md` — shared root/socket and
  budget contract.
- `docs/plans/axon-trunk-and-root-like-branches.md` — produces the dominant trunk
  root consumed here.
