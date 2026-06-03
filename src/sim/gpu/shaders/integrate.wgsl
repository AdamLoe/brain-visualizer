// Integrate + threshold pass (phase 2, architecture §4/§5, phase-2 spec).
//
// One thread per neuron. Leaky integrate-and-fire with excitability gain,
// ambient drive for input-region neurons (BV17), refractory period, and a
// silent-start-safe packed `last_spike` (BV21). Fired ids are appended to a
// spike list via an atomic counter; the count drives the indirect scatter.

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
    excitability: f32,   // global gain multiplier [0,1] -> actual range [0.5, 2.0]
    fixed_point_scale: f32,  // 4096.0
    synaptic_scale: f32, // effective recurrent-coupling scale (tuning knob; see GpuBackend)
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
    // Region bits live above the E/I bit. The type byte is (region<<2)|ei,
    // with Input region encoded as 0 so input-region neurons have ntype>>2==0.
    let is_input_region = (ntype >> 2u) == 0u;

    // Accumulated synaptic current (fixed-point i32 -> f32) scaled by the
    // effective recurrent-coupling knob, plus ambient drive. synaptic_scale is
    // a documented integration-side tuning that leaves the locked weight rule
    // and fixed_point_scale (4096) untouched; it sets how many coincident
    // presynaptic spikes are needed to cross threshold (biological realism).
    var current = (f32(I[i]) / u.fixed_point_scale) * u.synaptic_scale;
    if is_input_region {
        current += u.i_ext;
    }

    // Leaky integration with excitability gain mapping [0,1] -> [0.5, 2.0].
    let gain = 0.5 + u.excitability * 1.5;
    let new_v = v[i] * u.leak_decay + current * gain;
    v[i] = new_v;
    I[i] = 0;

    // Threshold check with absolute refractory period.
    let refractory_ok = !has_spiked(packed) || tick_diff(u.tick, last_tick) > u.refractory_ticks;
    if new_v >= u.threshold && refractory_ok {
        let idx = atomicAdd(&spike_count, 1u);
        spike_list[idx] = i;
        v[i] = u.reset_potential;
        last_spike[i] = HAS_SPIKED_MASK | (ntype << 24u) | (u.tick & TICK_MASK);
    }
}
