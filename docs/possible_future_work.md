# Brain Visualizer — Possible Future Work

_Ideas NOT in the current MVP. Promote to `current_plan.md` / `decisions.md` when
chosen; move to `denied_ideas.md` if rejected._

## Sim / model upgrades
- **GLIF neuron model** (Allen-style adaptation terms) upgrading LIF (BV5).
- **Hybrid connectivity:** procedural local + sparse stored long-range "highway"
  edges for cross-region realism (BV6 is local-only).
- **Sim-accurate conduction delay** (delay ring buffer) instead of visual-only.
- **Synaptic plasticity / STDP** so the network visibly learns over time.

## Other engines to stack onto the SNN
- N-body physics layout (network self-organizes).
- GPU particle signal field flowing along edges.
- Live forward-pass / in-browser training (the "it's learning" story).
- (All deferred from BV2's engine choice.)

## Rendering / UX
- **Richer neuron geometry** at near-LOD (dendrite hints, etc.) beyond the
  placeholder sphere/cylinder (BV7).
- Region labels / anatomical overlays on zoom.
- **Side-by-side CPU/GPU "race"** — both backends on the same seed rendered
  together, throughput counters racing (BV12 ships a top-right toggle first).

## Scaling / control
- **Auto-tier selection heuristic** per device: choose Low/Balanced/Max on page
  load or after a benchmark burst. This is separate from the MVP adaptive scaler,
  which may resize within the manually selected tier.
- Variants tied to name (NN spelling the name, etc.) — see the site-level docs;
  current site decision (D8) keeps the name decoupled from the viz.
