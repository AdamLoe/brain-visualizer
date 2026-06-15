---
status:        active
owner:         adamg
last_updated:  2026-06-15
---

# Active Edges

Active-edge ribbon rendering is not part of the current runtime. There is no
live emit shader, ribbon shader, event struct, ring buffer, bind group, pipeline,
or frame/tick branch for ribbon events; git history is the archive.

The live connection visual is procedural morphology:
`crates/brain-visualizer/src/sim/gpu/shaders/render_morphology.wgsl` draws
spike-keyed soma and branch geometry, and
`crates/brain-visualizer/src/sim/gpu/shaders/compact_morph_segments.wgsl`
selects the active/recent segment set per frame.

## What It Owns

This doc intentionally owns only the statement above: there is no active ribbon
runtime surface. Current connection rendering facts live in
[`gpu-rendering.md`](gpu-rendering.md); connectivity targeting facts live in
[`connectivity.md`](connectivity.md).

## Update When

- A new active-edge runtime subsystem exists.
- Procedural morphology stops being the only connection visual.

## See Also

- [`gpu-rendering.md`](gpu-rendering.md) — live morphology connection renderer
- [`connectivity.md`](connectivity.md) — spatial-grid target algorithm
- [`../decisions/rendering.md`](../decisions/rendering.md)
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
