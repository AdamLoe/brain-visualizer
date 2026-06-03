# Phase 2 — GPU Simulation Core

_The LIF simulation runs correctly on the GPU. Nothing renders yet — just
correct neuron dynamics, verified via console profiler output._

## Done when
- `profiler.maybeDump()` shows non-zero `spikes_per_sec` and
  `synaptic_events_per_sec` at N=100k on the GPU backend.
- Firing rate at `focused` excitability preset is in the 5–20 Hz range
  (biologically plausible).
- `seizure` preset produces synchronized bursting (very high spike rate).
- `deep_sleep` preset produces near-silence (< 1 Hz mean firing rate).
- Switching speed from ½× to 2× visibly changes how fast spike rate evolves
  (console numbers change).
- No i32 overflow: running at `seizure` for 60s produces no negative spike
  rates or NaN membrane potentials.
- No CPU readback is required during normal ticking. Scatter dispatch size is
  driven by a GPU-written indirect dispatch buffer.
- GPU pass timings are available when `timestamp-query` exists and are resolved
  asynchronously without stalling the frame loop.

## Shaders

### `src/sim/gpu/shaders/integrate.wgsl`
```wgsl
// Bindings (one bind group per chunk if chunked):
@group(0) @binding(0) var<storage, read_write> v: array<f32>;
@group(0) @binding(1) var<storage, read_write> last_spike: array<u32>; // bit31 valid, [30:24]=type, [23:0]=tick
@group(0) @binding(2) var<storage, read_write> I: array<i32>;
@group(0) @binding(3) var<storage, read_write> spike_list: array<u32>;
@group(0) @binding(4) var<storage, read_write> spike_count: atomic<u32>;

struct Uniforms {
    tick: u32,
    n: u32,
    leak_decay: f32,
    threshold: f32,
    reset_potential: f32,
    refractory_ticks: u32,
    i_ext: f32,          // ambient drive for input-region neurons
    excitability: f32,   // global gain multiplier [0,1] → actual range [0.5, 2.0]
    fixed_point_scale: f32,  // 4096.0
}
@group(1) @binding(0) var<uniform> u: Uniforms;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TYPE_MASK: u32 = 0x7F000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;

fn neuron_type(packed: u32) -> u32 {
    return (packed & TYPE_MASK) >> 24u;
}

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}

fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

@compute @workgroup_size(256)
fn integrate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= u.n { return; }

    let packed = last_spike[i];
    let ntype = neuron_type(packed);
    let last_tick = packed & TICK_MASK;
    let is_input_region = (ntype >> 2u) == 0u;  // region bits

    // Apply ambient drive to input-region neurons
    var current = f32(I[i]) / u.fixed_point_scale;
    if is_input_region {
        current += u.i_ext;
    }

    // Leaky integration with excitability gain
    let gain = 0.5 + u.excitability * 1.5;  // maps [0,1] → [0.5, 2.0]
    v[i] = v[i] * u.leak_decay + current * gain;
    I[i] = 0;

    // Threshold check with refractory
    let refractory_ok = !has_spiked(packed) || tick_diff(u.tick, last_tick) > u.refractory_ticks;
    if v[i] >= u.threshold && refractory_ok {
        let idx = atomicAdd(&spike_count, 1u);
        spike_list[idx] = i;
        v[i] = u.reset_potential;
        last_spike[i] = HAS_SPIKED_MASK | (ntype << 24u) | (u.tick & TICK_MASK);
    }
}
```

### `src/sim/gpu/shaders/scatter.wgsl`
```wgsl
@group(0) @binding(0) var<storage, read> spike_list: array<u32>;
@group(0) @binding(1) var<storage, read> spike_count_buf: array<u32>; // [0] = count
@group(0) @binding(2) var<storage, read_write> I_next: array<atomic<i32>>;
@group(0) @binding(3) var<storage, read> last_spike: array<u32>; // for source type

struct ConnectUniforms {
    n: u32,
    k: u32,
    fixed_point_scale: f32,
}
@group(1) @binding(0) var<uniform> cu: ConnectUniforms;

// Must match Rust hash32() exactly (BV22).
fn hash32(x_in: u32) -> u32 {
    var x = x_in;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn mix_key(seed_lo: u32, neuron_id: u32, synapse_j: u32, salt: u32) -> u32 {
    return hash32(
        seed_lo ^
        (neuron_id * 0x9e3779b1u) ^
        (synapse_j * 0x85ebca6bu) ^
        (salt * 0xc2b2ae35u)
    );
}

fn target_neuron(src: u32, synapse_j: u32) -> u32 {
    // Integer-only: same algorithm as Rust connectivity::target()
    // BV22 hash of (seed, src, synapse_j, salt) → index into local spatial bucket.
    // Production uses the phase 1 spatial rule, including mild anterior bias for
    // excitatory targets. Modulo-N is allowed only as a debug fallback.
    let h = mix_key(0x12345678u, src, synapse_j, 0u);
    return h % cu.n;
}

fn synapse_weight(src: u32, synapse_j: u32, src_type: u32) -> i32 {
    let h = mix_key(0x12345678u, src, synapse_j, 1u);
    // Excitatory: positive fixed-point weight
    // Inhibitory (type bit 0 = 1): negative
    let mag = i32(h & 0x0FFFu) + 1024;  // range [1024, 5119] in fixed-point
    if (src_type & 1u) == 1u { return -mag / 2; }  // inhibitory: smaller magnitude
    return mag;
}

fn neuron_type(packed: u32) -> u32 {
    return (packed >> 24u) & 0x7Fu;
}

@compute @workgroup_size(64)
fn scatter(@builtin(global_invocation_id) gid: vec3<u32>) {
    let event_idx = gid.x;
    let count = spike_count_buf[0];
    let total_events = count * cu.k;
    if event_idx >= total_events { return; }

    let spike_idx = event_idx / cu.k;
    let synapse_j = event_idx - spike_idx * cu.k;
    let src = spike_list[spike_idx];
    let src_type = neuron_type(last_spike[src]);
    let tgt = target_neuron(src, synapse_j);
    let w = synapse_weight(src, synapse_j, src_type);

    // Phase 2 may start with atomicAdd plus high-water overflow instrumentation.
    // Production must either prove a per-tier current bound or replace this with
    // saturating accumulation via atomic compare-exchange.
    atomicAdd(&I_next[tgt], w);
}
```

**Hash determinism:** WGSL has no native `u64`; use the BV22 `u32` hash above.
The Rust and WGSL implementations must pass the phase 1 golden-vector tests
before relying on them here.

## Dispatch sequence (per tick)

```
1. Reset spike_count to 0 (write via staging buffer or indirect clear).
2. dispatch integrate  — workgroup(256), groups = ceil(N/256)
3. barrier
4. write indirect dispatch args from spike_count on the GPU
5. dispatch scatter indirect — workgroup(64), groups = ceil(spike_count * K / 64)
   Use `dispatchWorkgroupsIndirect` driven by the GPU-side spike count so the
   CPU never stalls waiting for a readback.
6. barrier
7. swap I / I_next (or double-buffer pointer flip in uniforms)
```

**Indirect scatter dispatch:** write a `DispatchIndirect` buffer as part of the
integrate pass (a separate small shader or appended to integrate) that computes
`ceil(spike_count * K / 64)` on the GPU. The main JS loop then calls
`dispatchWorkgroupsIndirect` with that buffer — zero CPU stall.

### `src/sim/gpu/shaders/write_scatter_dispatch.wgsl`
```wgsl
@group(0) @binding(0) var<storage, read> spike_count_buf: array<u32>;
@group(0) @binding(1) var<storage, read_write> dispatch_args: array<u32>;

struct ConnectUniforms {
    n: u32,
    k: u32,
    fixed_point_scale: f32,
}
@group(1) @binding(0) var<uniform> cu: ConnectUniforms;

@compute @workgroup_size(1)
fn main() {
    let total_events = spike_count_buf[0] * cu.k;
    let groups = (total_events + 63u) / 64u;
    dispatch_args[0] = groups;
    dispatch_args[1] = 1u;
    dispatch_args[2] = 1u;
}
```

## GPU command encoding discipline

For each rAF frame, use one command encoder for all sim passes due that frame
and the render pass added in later phases. Do not submit after every pass unless
profiling proves it is necessary. Pass boundaries already provide the ordering
needed for storage-buffer dependencies.

Normal frames may write:
- small uniform buffers (`tick`, `excitability`, speed/timing options);
- clear/reset buffers (`spike_count`, optional debug counters);
- command buffers.

Normal frames must not recreate:
- storage buffers;
- bind groups;
- pipelines;
- staging/readback buffers;
- render targets.

Structural changes (`resize`, tier switch, backend restart, device loss) own
all resource recreation and then refresh dependent bind groups in one place.

## Optional GPU timing

If `timestamp-query` is supported, wrap these pass groups:

1. integrate;
2. indirect-dispatch write + scatter;
3. optional stimulation/delay;
4. render once phase 3 exists.

Resolve into a small pool of staging buffers and map asynchronously. If no
staging buffer is available, skip this frame's timing result rather than
blocking. Timing must never be required for correctness.

## Buffer allocation

Use `ChunkedBuffer` from phase 1. For phase 2, likely a single chunk suffices
(N ≤ 1M = 4 MB per f32 field). Multi-chunk path must compile and not panic even
if it only exercises one chunk. The scatter shader must accept a chunk index
into a bind group array.

Allocate at startup with `SimConfig.n`. On tier change, call `backend.resize()`
which reallocates. Profiler tracks current allocation size.

After every resize:
1. recreate all size-dependent buffers;
2. rewrite static initial state;
3. recreate bind groups that reference recreated buffers;
4. reset spike/current buffers;
5. restart from silent state with the same seed.

Do not try to preserve mid-run membrane/current state across resize. The same
seed preserves the meaningful comparison while avoiding expensive readbacks and
state-shape conversion.

## LIF parameters (lock these; adjust only via excitability gain)
```
leak_decay       = 0.95   (≈ 50ms time constant at 1ms/tick)
threshold        = 1.0    (normalized)
reset_potential  = 0.0
refractory_ticks = 5      (5ms absolute refractory)
i_ext            = 0.02   (ambient drive for input-region neurons; tune during phase)
fixed_point_scale= 4096   (S = 2^12, BV19)
```

E/I ratio: 80% excitatory, 20% inhibitory. Assign at neuron creation time
via `hash32(neuron_id ^ seed_lo) % 5 == 0` → inhibitory.

## Tier parameters (update with phase 0 benchmark results)
| Tier     | N       | K    |
|----------|---------|------|
| Low      | 50k     | 16   |
| Balanced | 200k    | 32   |
| Max      | 1M      | 64   |

## Correctness checks (implement as debug-mode assertions)
- After integrate pass: `mean(v)` should be in [-0.5, 1.5].
- Spike rate: if > 80% neurons fire in one tick → excitability bug; log warning.
- i32 overflow guard: log max absolute accumulated current per run; shipped
  tiers require either a proven per-tick current/fan-in bound or saturating
  accumulation. `K * MAX_WEIGHT_FP` is not sufficient because it is out-degree,
  not worst-case target fan-in.
