---
status:        long_lived
owner:         adamg
last_updated:  2026-06-04
okay_to_delete: false
long_lived:    true
owning_docs:   [architecture/*, decisions/*]
---

# Future roadmap & rejected ideas

The one long-lived plan. It holds two kinds of durable context that don't fit a
current-state architecture doc or a per-domain decision: **deferred work** we
might do, and **ideas we considered and rejected** (with the reason, so they're
not re-proposed). When a deferred item is taken on, move it into a real plan;
when one is killed, move it to the rejected table.

## Deferred / possible future work

### Sim & model
- **GLIF neuron model** (Allen-style adaptation terms) upgrading LIF. See
  [`../decisions/scope.md`](../decisions/scope.md) (LIF-first).
- **Hybrid connectivity**: procedural local + sparse stored long-range "highway"
  edges. Today's rule is local-only — [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Sim-accurate conduction delay** (delay ring buffer) instead of visual-only.
- **Synaptic plasticity / STDP** so the network visibly learns.

### Other engines to stack on the SNN
- N-body physics layout; GPU particle signal field; live in-browser
  forward-pass / training. All deferred from the SNN engine choice
  ([`../decisions/scope.md`](../decisions/scope.md)).

### Rendering / UX
- Region labels / anatomical overlays on zoom.
- **Side-by-side CPU/GPU "race"** — both backends on one seed, throughput
  counters racing. The backend toggle ships first
  ([`../decisions/backends.md`](../decisions/backends.md)).
- Reviving a near-LOD connection visual (ribbon or cylinder) is gated behind the
  `DRAW_LEGACY_*` flags — [`../architecture/active-edges.md`](../architecture/active-edges.md).

### Scaling / control
- **Smart within-tier auto-scaling** — a gentle, hysteretic, stall-aware
  replacement for the auto-scaler that was pulled in 0.1.1
  ([`../decisions/scaling.md`](../decisions/scaling.md)). It must: decide on the
  **average** frame time, not p95 (a one-frame resize stall is invisible to p95
  and caused an unbounded grow loop); and make resize **cheap** — skip the
  render-pipeline recompile by splitting GPU buffer-resize out of
  `build_render_pipelines` so a within-tier N change doesn't full-teardown.
  The dormant `scalerDecide` / `scaler.rs` stub is the seed
  ([`../architecture/scaling.md`](../architecture/scaling.md)).
- **Auto-tier selection heuristic** per device on load — distinct from the
  within-tier scaler above ([`../architecture/scaling.md`](../architecture/scaling.md)).
- **CPU backend revival** to feature-parity (heterogeneity, weight norm, input
  modes, a connection visual) — or formal retirement. It is parked today
  ([`../architecture/cpu-backend.md`](../architecture/cpu-backend.md),
  [`../decisions/backends.md`](../decisions/backends.md)).

## Considered and rejected

| Idea | Reason rejected | Permanent / deferred |
|---|---|---|
| Biophysically detailed (multi-compartment, ion-channel) neurons | ~1000× cost/neuron, needs a supercomputer, and the detail is invisible at this scale; point/LIF reproduces the look. | Deferred (revisit only on a drastic scope change) |
| Graphics engine (three.js / Babylon) | Conflicts with the from-scratch, peak-performance framing; we hand-write shaders + pipelines, `wgpu` is a thin binding. | Permanent |
| Real connectome data (HCP tractography, etc.) | Large, messy, licensing-encumbered; tiny visual payoff over procedural distance-decay wiring. | Deferred |
| "Machine score" / shareable benchmark + leaderboard | It's a "pretty toy with slight interactivity," not a competitive product; throughput still shows in the HUD. | Deferred |
| Avalanche trace mode (highlight a cascade path in a distinct color) | Visual-complexity scope creep over the core glow. | Deferred |
| Spectral overlay / live FFT of population activity | Turns a toy into a dashboard; the HUD covers perf. | Deferred |
| Damage / lesioning mode | Out of scope for now. | Deferred |
| Scripted "wake up" intro (seed spike, cortex loads dark) | Natural posterior→anterior propagation produces the effect for free. | Permanent — see [`../decisions/interaction.md`](../decisions/interaction.md) |

## See also

- [`index.md`](index.md) — plans landing + lifecycle.
- [`../decisions/index.md`](../decisions/index.md) — the choices these alternatives were weighed against.
