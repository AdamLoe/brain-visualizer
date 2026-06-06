---
status:        active
owner:         adamg
last_updated:  2026-06-04
---

# Active-Edge Ribbon

One-line job: emit one curved, animated ribbon per firing neuron per tick,
drawn from source soma to its scatter target, so signal propagation is
visible at any zoom level with zero cost when disabled.

> **Current status:** The ribbon renderer and its emit compute pass are
> **retired** — both are gated behind `crates/brain-visualizer/src/sim/gpu/mod.rs → DRAW_LEGACY_RIBBONS`
> (`false` by default) and never run. Procedural neuron morphology
> (`render_morphology.wgsl`) is the active connection visual. The edge
> buffers are still allocated so the legacy path can be re-enabled for
> debugging by flipping that constant.

## What it owns

- `crates/brain-visualizer/src/sim/gpu/shaders/emit_edges.wgsl → emit_edges` — compute pass: one thread per firing neuron, emits one `EdgeEvent` into the ring.
- `crates/brain-visualizer/src/sim/gpu/shaders/render_ribbon.wgsl → vs_main, fs_main` — vertex + fragment: instanced ribbon geometry from `EdgeEvent` data.
- `crates/brain-visualizer/src/sim/gpu/resources.rs → EdgeEvent` — the 48-byte payload written at emit time and read at draw time.
- `crates/brain-visualizer/src/sim/gpu/resources.rs → EdgeBuffers, EDGE_CAP` — persistent ring buffer allocation.
- `crates/brain-visualizer/src/sim/gpu/mod.rs → DRAW_LEGACY_RIBBONS` — the compile-time gate.

## Invariant: scatter-mirror contract

The emit pass resolves each ribbon's target neuron using the **identical spatial-grid algorithm** as `scatter.wgsl` — same constants, same salt space, same target function — so the visual edge lands on the same neuron the scatter pass drove current to. This is marked by the `// MIRRORS scatter.wgsl` comment block in `emit_edges.wgsl → target_neuron`. If `scatter.wgsl`'s spatial-grid target algorithm ever changes, the mirror in `emit_edges.wgsl` must change in lockstep.

## How it worked (for revival)

Each `EdgeEvent` captures source and target world positions plus a per-edge `curve_seed` at emit time, so the ribbon render pass needs no neuron buffers at draw time. The vertex shader (`render_ribbon.wgsl → vs_main`) generates a camera-facing cubic Bézier strip with a traveling pulse and age-based fade entirely on the GPU; the fragment shader (`render_ribbon.wgsl → fs_main`) applies E/I tinting and soft cross-section falloff. Ring-buffer slot management and the visual budget cap are handled via `EdgeBuffers` and `EDGE_CAP`; see those symbols for details.

## Reviving the path

Flip `DRAW_LEGACY_RIBBONS = true` in `crates/brain-visualizer/src/sim/gpu/mod.rs`, then read `emit_edges.wgsl` and `render_ribbon.wgsl` directly for the geometry. **Caveat for the uniform upload:** `RibbonUniforms` still carries `lifetime` and `pulse_speed` fields, but the settings that once fed them (the retired traveling-pulse `connection_lifetime` / `connection_pulse_speed`) no longer exist — their Float32Array indices were repurposed to the morphology lighting toggles. The gated upload in `render_full` therefore writes **literal fallback constants** for those two fields just to keep the retired path compiling; a revival would need to wire real values (or new settings).

## Update when

- `EdgeEvent` field order or padding changes (must update both WGSL structs and the Rust repr simultaneously).
- The spatial-grid target algorithm in `scatter.wgsl` changes (the mirror in `emit_edges.wgsl → target_neuron` must be updated in lockstep).
- `EDGE_CAP` changes (affects buffer allocation size and maximum visual budget).
- The ribbon pass is un-retired.

## See also

- [`gpu-rendering.md`](gpu-rendering.md) — morphology renderer that replaced ribbons as the live connection visual
- [`gpu-backend.md`](gpu-backend.md) — frame graph, metrics readback, buffer lifecycle
- [`simulation.md`](simulation.md) — scatter pass and tick structure the emit pass hooks into
- [`connectivity.md`](connectivity.md) — spatial-grid target algorithm mirrored in `emit_edges.wgsl`
- [`dev-panel.md`](dev-panel.md) — live morphology knobs (width, curve lift, lighting toggles)
- [`../decisions/rendering.md`](../decisions/rendering.md) — ribbon superseded by morphology; active-connections-only default
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
