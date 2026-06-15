---
status:        active
owner:         adamg
last_updated:  2026-06-15
---

# Simulation model

The neural dynamics: a leaky integrate-and-fire (LIF) spiking network whose
per-tick evolution runs in the GPU backend. This doc owns the *dynamics* (the
math, the knobs, the energy source) and the *boundary contract* (what crosses
wasm↔JS). It does not own how the GPU schedules the passes or how the network
is wired.

## What it owns

- The LIF neuron model + per-tick step: `crates/brain-visualizer/src/sim/gpu/shaders/integrate.wgsl → integrate`.
- Locked LIF constants (leak, threshold, reset, refractory): `crates/brain-visualizer/src/sim/gpu/mod.rs` (`LEAK_DECAY`/`THRESHOLD`/`RESET_POTENTIAL`/`REFRACTORY_TICKS`).
- Excitability / gain — the single slider that sweeps silent→critical→seizure (the `gain = 0.5 + excitability * 1.5` mapping in `integrate.wgsl`).
- Region ambient drive: `i_ext` injected into input-region neurons as the *sole* external energy. Region assignment is `crates/brain-visualizer/src/manifold/regions.rs → assign_regions`; the input-region test is `(ntype >> 2) == 0` in `integrate.wgsl`.
- E/I assignment: `crates/brain-visualizer/src/sim/backend.rs → neuron_type_byte`.
- Per-neuron heterogeneity (threshold/leak/refractory/input_sensitivity/weight_scale spread): the `hspread` / `SALT_*` / `*_SPREAD` block in `integrate.wgsl`.
- Weight normalization (none|sqrt_k|k): `crates/brain-visualizer/src/sim/gpu/mod.rs → weight_norm_factor`, consumed as `weight_norm_factor` in `integrate.wgsl`.
- Input modes (constant|poisson|pulsed|cursor_only|scripted|off): the `switch u.input_mode` in `integrate.wgsl`.
- Cursor stimulation (spatial-hash current bump): `crates/brain-visualizer/src/sim/gpu/shaders/stimulate.wgsl → stimulate`.
- The `SimBackend` trait + the web/stats boundary types: `crates/brain-visualizer/src/sim/backend.rs → SimBackend, SimConfig, TickStats`.
- The live settings + metrics contract crossing wasm↔JS: `crates/brain-visualizer/src/sim/gpu/mod.rs → VisualSettings` (the flat `Float32Array`) and `GpuBackend::metrics_snapshot` (the flat metrics array).

## What it does NOT own

- GPU pass orchestration, buffers, double-buffering, indirect dispatch → [`gpu-backend.md`](gpu-backend.md).
- The synapse target rule + weight values (`scatter.wgsl`, `connectivity::target`/`weight`) → [`connectivity.md`](connectivity.md).
- The SoA buffer layout + fixed-point current packing → [`data-model.md`](data-model.md).
- Metrics *meaning*, reduction, and the non-blocking readback state machine → [`profiling.md`](profiling.md).
- Neuron geometry / glow / morphology (anything visual) → the rendering docs.

## The per-tick step

One thread per neuron, every tick (`integrate.wgsl → integrate`):

1. **Leaky integrate.** `v = v*leak + current*gain`, where `current` is the
   swapped-in fixed-point synaptic accumulator scaled to f32 (`I/fixed_point_scale`)
   times `synaptic_scale`, times the K-invariant `weight_norm_factor`, times the
   per-neuron `weight_scale`; then, for input-region neurons only, plus the ambient
   drive selected by `input_mode`.
2. **Threshold + refractory.** Fire iff `v >= threshold` AND the neuron is out of
   its absolute refractory window. On fire: append id to the spike list via an
   atomic counter (this count drives the indirect scatter — see
   [`gpu-backend.md`](gpu-backend.md)), reset `v`, and repack `last_spike`.
3. **Scatter** (separate pass) reads the spike list and accumulates weighted
   current into targets — see [`connectivity.md`](connectivity.md).

The synaptic accumulator is consumed (zeroed) by integrate each tick; scatter
re-fills it for the next.

### Energy flow — the silent→posterior→anterior story

`i_ext` into the input region is the **only** external energy. There is no special
sink: dissipation is the global E/I balance plus the leak. A mild anterior (+Z)
feed-forward bias on a fraction of excitatory synapses (in the connectivity rule)
gives activity a direction. Start silent (v=0 everywhere); ambient drive lights the
input region; activity propagates through association to output. Production
region membership is assigned **uniformly at random over the volume**, not
spatially blocked — the input/assoc/output split is functional (who gets drive,
who relays), not a contiguous anatomical lobe. The internal anterior-posterior
region prototype is a build-time assignment mode only; it does not change this
drive path, connectivity, or LIF constants. See [`connectivity.md`](connectivity.md)
for the bias and [`manifold.md`](manifold.md) for the assignment modes.

## Dynamics knobs (the live tuning surface)

These cross the boundary live (no network rebuild) and are read fresh each tick:

| Knob | Where | Effect |
|---|---|---|
| `excitability` | `tick(ticks, excitability)` arg | global gain `[0,1] → [0.5, 2.0]`; the silent↔critical↔seizure slider; product default `0.10` |
| `i_ext` | `VisualSettings` idx 12 | ambient drive magnitude into input region; product default `0.014` |
| `synaptic_scale` | `VisualSettings` idx 13 | recurrent coupling strength (how many coincident inputs cross threshold) |
| `heterogeneity` | `VisualSettings` idx 14 | per-neuron parameter spread `[0,1]`; clean product default `0.50`; `0` ⇒ homogeneous |
| `weight_normalization` | `VisualSettings` idx 21 | `0=none 1=sqrt_k 2=k` |
| `input_mode` | `VisualSettings` idx 22 | drive shape (see below) |

The product defaults (`excitability=0.10`, `i_ext=0.014`, `n=6000`) produce a
quiet network where cascades are visible as individual propagating signals
rather than a saturated blur — beauty/readability first. See
[`../decisions/dynamics.md`](../decisions/dynamics.md) for the rationale.

**Invariant — neutral baseline.** `heterogeneity=0`, `weight_normalization` at
the K=16 baseline, and `input_mode=0` (constant) each reproduce pre-V2 dynamics
**bit-for-bit**. At het=0 every `*_i` term collapses to the global constant; at
K=16 both `sqrt_k` and `k` give factor `1.0` (`weight_norm_factor` is relative
to `K_REF`); `input_mode=0` is the plain `current += i_ext`. This is a
regression/bisect baseline, not the clean product default: the current clean
default is `heterogeneity=0.50`.

### Per-neuron heterogeneity

Each neuron's threshold/leak/refractory/input_sensitivity/weight_scale is the
global constant times `(1 + spread(id) * heterogeneity * SPREAD)`, where
`spread(id)` is a deterministic symmetric draw from `hash32(seed, id, salt)` (the
locked connectivity hash, prepended as `HASH_WGSL`). Determinism: same seed → same
network → same per-neuron parameters across runs and backends. See
[`../decisions/dynamics.md`](../decisions/dynamics.md).

### Input modes

`constant` (today's ambient `i_ext`), `poisson` (`i_ext` reinterpreted as a
per-tick spike probability, drawn per-neuron-per-tick from the hash),
`pulsed` (periodic burst), `cursor_only` (no ambient — only `stimulate()` drives),
`scripted` (placeholder, currently constant), `off`. Only input-region neurons see
any of these; association/output neurons are driven purely recurrently.

### Cursor stimulation

`stimulate(pos, radius, current)` queues a current bump consumed at the next tick
start (`stimulate.wgsl`). It finds neurons inside the sphere via a bounded
brute-force over the spatial-grid CSR cells overlapping the sphere's bounding box
— cheap and exact, no per-neuron cell-id upload. The bump is fixed-point and adds
to the same accumulator scatter writes.

## The boundary contract (wasm↔JS)

Three things cross:

- **In, config:** `SimConfig` (n, k, seed, i_ext, …) builds the network once.
- **In, live settings:** a flat `Float32Array` parsed by
  `VisualSettings::from_slice`. **Length-tolerant** — indices past the array
  fall back to defaults, so the contract can grow without breaking old callers.
  Indices are the canonical order; the JS source of truth is
  `web/src/core/settings.ts`. Removed settings keep reserved/default-written
  slots instead of shifting the array. Contract tests lock the full default
  TypeScript array, Rust index mapping, tombstoned zero slots, and the
  quarantined default-written slots.
- **Out, stats/metrics:** `tick()` returns `TickStats` (a coarse per-batch
  throughput summary — note `spikes` is approximated as `last_tick_count * ticks`,
  see the no-readback policy in [`gpu-backend.md`](gpu-backend.md)); the rich
  per-tick metrics come from `metrics_snapshot()` as a flat array. Meaning and
  reduction live in [`profiling.md`](profiling.md).

The named symbols `RenderState` / `RenderState::Gpu` also cross at the
render boundary (raw GPU buffer handles, zero readback).

## Update when

- The LIF step changes (any term in `integrate.wgsl → integrate`, including the
  gain mapping or refractory rule).
- A locked LIF constant changes.
- A `VisualSettings` index is added/reordered (must match `web/src/core/settings.ts`).
- An input mode is added or `weight_norm_factor` modes change.
- The `SimBackend` trait or `TickStats`/`SimConfig` shape changes.
- The neutral-baseline invariant (het=0 / K=16 / constant == pre-V2) is touched.
- The product default for a dynamics setting changes.

## See also

- [`gpu-backend.md`](gpu-backend.md) — pass orchestration, indirect dispatch, readback policy.
- [`connectivity.md`](connectivity.md) — synapse target rule, weights, anterior bias.
- [`data-model.md`](data-model.md) — SoA layout, `last_spike` packing, fixed-point current.
- [`profiling.md`](profiling.md) — metrics meaning + non-blocking readback.
- [`../decisions/dynamics.md`](../decisions/dynamics.md) — SOC target, region/energy model, heterogeneity, normalization, input modes.
- [`../decisions/scope.md`](../decisions/scope.md) — why from-scratch LIF, why this scale.
- [`../agent-context/maintaining-docs.md`](../agent-context/maintaining-docs.md)
