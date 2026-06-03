# Brain Visualizer — Denied Ideas

_Ideas considered and rejected, with reason and whether permanent or deferred. If
revived, move back to `possible_future_work.md` with a note._

| Idea | Reason rejected | Permanent or deferred |
| --- | --- | --- |
| Biophysically detailed (multi-compartment, ion-channel) neurons | ~1000× cost/neuron, needs a supercomputer (Fugaku-class), and the detail is invisible at this scale. Point/LIF reproduces the look. | Deferred (not feasible in-browser; revisit only if scope changes drastically) |
| Graphics engine (three.js / Babylon) | Conflicts with the from-scratch, peak-performance goal (BV1); we hand-write shaders + pipelines. `wgpu` (a thin GPU-API binding) is used instead. | Permanent for this project's framing |
| Real connectome data (HCP tractography, etc.) | Large, messy, licensing-encumbered; tiny visual payoff over procedural distance-decay + small-world wiring. | Deferred |
| "Machine score" / shareable benchmark (your device sustained X synaptic events/sec, leaderboard) | Adam wants a "silly pretty toy with slight interactivity, nothing more" — not a competitive/benchmark product. The throughput numbers still live in the perf HUD (BV8). | Deferred (could revive if framing changes) |
| Avalanche trace mode (highlight cascade propagation path in distinct color) | Too much visual complexity; scope creep on top of the core glow rendering. | Deferred |
| Spectral overlay / live FFT of population activity showing frequency bands | Too much; turns a pretty toy into a dashboard. The HUD (BV8) covers perf already. | Deferred |
| Damage / lesioning mode (silence a region, watch compensation) | Too much for current scope. | Deferred |
| Scripted "wake up" intro (seed spike, cortex loads dark) | Replaced by natural propagation from input→center→output regions (BV10 amendment). Scripted version adds complexity for a one-time effect the physics already produces for free. | Permanent |
