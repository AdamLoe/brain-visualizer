# Decisions index

Rationale for current design choices, indexed by domain. Each doc states a
decision and the reason that still holds; superseded deliberation stays in git.
For the **what** (current behavior), follow the architecture doc a decision
constrains.

## Routing

| Need | Read |
|---|---|
| From-scratch / point-LIF / no graphics engine; SNN as the engine; beauty-first ~10k-scale | [`scope.md`](scope.md) |
| Two interchangeable backends, toggle UX, full-restart-same-seed switch, CPU coordinator worker, CPU parked for V2 | [`backends.md`](backends.md) |
| Packed `last_spike` word, fixed-point current scale (S=2^12) + overflow policy, CPU scatter atomics | [`data-layout.md`](data-layout.md) |
| Procedural no-edge-list connectivity, per-tier K, 32-bit hash (not u64 PCG) with golden vectors | [`connectivity.md`](connectivity.md) |
| Self-organized criticality target, region topology + ambient drive, heterogeneity determinism, weight norm, input modes | [`dynamics.md`](dynamics.md) |
| Procedurally folded brain surface, region assignment, per-neuron morphology geometry | [`manifold.md`](manifold.md) |
| LOD glow→bodies, active-connections-only-by-default, morphology supersedes ribbons/cylinders | [`rendering.md`](rendering.md) |
| Three tiers built+benchmarked, K as a per-tier scaler axis, 1M-default / 10M-gated honest adaptation | [`scaling.md`](scaling.md) |
| "Pretty toy" interaction model, natural start (no scripted intro), sonification opt-in, speed presets, brain-state presets | [`interaction.md`](interaction.md) |
| First-class profiling + corner HUD, periodic GPU reduction + async readback, cheap hot-loop counters | [`profiling.md`](profiling.md) |
| Hidden dev panel + impact-dot metadata, versioned localStorage, no preset manager | [`dev-tooling.md`](dev-tooling.md) |

## See also

- [`../index.md`](../index.md) — global router.
- [`../architecture/index.md`](../architecture/index.md) — what these constrain.
