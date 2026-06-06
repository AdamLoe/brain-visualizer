// Integrate + threshold pass (phase 2, architecture §4/§5, phase-2 spec).
//
// One thread per neuron. Leaky integrate-and-fire with excitability gain,
// ambient drive for input-region neurons (BV17), refractory period, and a
// silent-start-safe packed `last_spike` (BV21). Fired ids are appended to a
// spike list via an atomic counter; the count drives the indirect scatter.
//
// V2 Phase C: this shader is PREPENDED with pipelines::HASH_WGSL (hash32 /
// mix_key) so per-neuron heterogeneity and the poisson input mode can draw
// deterministic per-neuron / per-tick randomness from the locked BV22 hash.
// Three new tunables are added (heterogeneity, weight_norm_factor, input_mode),
// each defaulting to a value that reproduces pre-V2 dynamics BIT-FOR-BIT:
//   - heterogeneity=0   -> every `*_i` term collapses to the global constant.
//   - weight_norm_factor=1.0 (the K=16 default for sqrt_k / k normalization).
//   - input_mode=0 (constant) -> today's `current += i_ext` for input region.

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
    // ─── V2 Phase C ───────────────────────────────────────────────────────────
    seed_lo: u32,           // BV22 connectivity seed (for per-neuron hash draws)
    heterogeneity: f32,     // [0,1] per-neuron parameter spread; 0 => homogeneous
    weight_norm_factor: f32,// K-invariant recurrent scale; 1.0 at K=16 default
    input_mode: u32,        // 0=constant 1=poisson 2=pulsed 3=cursor 4=scripted 5=off
    _pad: vec2<u32>,        // pad to 64 B (mirror IntegrateUniforms in resources.rs)
}
@group(1) @binding(0) var<uniform> u: Uniforms;

// ─── V2 Phase C: heterogeneity salts (distinct from scatter's 0x1..0x4) ───────
const SALT_THRESH: u32     = 0x00000010u;
const SALT_LEAK: u32       = 0x00000011u;
const SALT_REFRAC: u32     = 0x00000012u;
const SALT_INPUT_SENS: u32 = 0x00000013u;
const SALT_WEIGHT_SCALE: u32 = 0x00000014u;
const SALT_POISSON: u32    = 0x00000015u;

// Per-parameter symmetric spread fractions (full effect at heterogeneity=1).
const THRESH_SPREAD: f32     = 0.3;
const LEAK_SPREAD: f32       = 0.1;
const REFRAC_SPREAD: f32     = 0.5;
const INPUT_SENS_SPREAD: f32 = 0.3;
const WEIGHT_SPREAD: f32     = 0.3;

// Pulsed input-mode burst shape (input_mode==2): drive ON for the first
// PULSE_WIDTH ticks of each PULSE_PERIOD-tick cycle.
const PULSE_PERIOD: u32 = 120u;
const PULSE_WIDTH: u32  = 20u;

// rand in [0,1) for neuron `id`, distinguished by `salt`.
fn hrand(id: u32, salt: u32) -> f32 {
    return f32(mix_key(u.seed_lo, id, 0u, salt)) / 4294967296.0;
}
// symmetric spread in [-1,1] for neuron `id`, param `salt`.
fn hspread(id: u32, salt: u32) -> f32 {
    return (hrand(id, salt) - 0.5) * 2.0;
}

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

    // ─── V2 Phase C: per-neuron heterogeneity ────────────────────────────────
    // Each `*_i` is the global constant perturbed by a deterministic per-neuron
    // hash draw scaled by `heterogeneity` (het). At het==0 the `* het` term
    // vanishes EXACTLY, so every `*_i` equals the global constant / 1.0 and the
    // dynamics are bit-identical to pre-V2.
    let het = u.heterogeneity;
    let threshold_i  = u.threshold * (1.0 + hspread(i, SALT_THRESH) * het * THRESH_SPREAD);
    let leak_i       = clamp(u.leak_decay * (1.0 + hspread(i, SALT_LEAK) * het * LEAK_SPREAD), 0.0, 0.999);
    let refrac_f     = f32(u.refractory_ticks) * (1.0 + hspread(i, SALT_REFRAC) * het * REFRAC_SPREAD);
    let refractory_i = u32(max(0.0, round(refrac_f)));
    let input_sens_i = 1.0 + hspread(i, SALT_INPUT_SENS) * het * INPUT_SENS_SPREAD;
    let weight_scale_i = 1.0 + hspread(i, SALT_WEIGHT_SCALE) * het * WEIGHT_SPREAD;

    // Accumulated synaptic current (fixed-point i32 -> f32) scaled by the
    // effective recurrent-coupling knob, plus ambient drive. synaptic_scale is
    // a documented integration-side tuning that leaves the locked weight rule
    // and fixed_point_scale (4096) untouched; it sets how many coincident
    // presynaptic spikes are needed to cross threshold (biological realism).
    // V2 Phase C: weight_norm_factor (K-invariant; 1.0 at K=16) and the
    // per-neuron weight_scale_i (1.0 at het=0) multiply the recurrent term
    // BEFORE i_ext is added, so neither touches the ambient input drive.
    var current = (f32(I[i]) / u.fixed_point_scale) * u.synaptic_scale
                  * u.weight_norm_factor * weight_scale_i;
    if is_input_region {
        // V2 Phase C: input_mode selects how input-region neurons are driven.
        // Mode 0 (constant) reproduces today's behavior at input_sens_i==1.
        switch u.input_mode {
            case 0u: { // constant
                current += u.i_ext * input_sens_i;
            }
            case 1u: { // poisson: i_ext reinterpreted as a per-tick spike prob.
                let r = f32(mix_key(u.seed_lo, i, u.tick, SALT_POISSON)) / 4294967296.0;
                if r < u.i_ext {
                    current += input_sens_i;
                }
            }
            case 2u: { // pulsed: periodic burst (PULSE_WIDTH on / PULSE_PERIOD).
                if (u.tick % PULSE_PERIOD) < PULSE_WIDTH {
                    current += u.i_ext * input_sens_i;
                }
            }
            case 3u: { /* cursor_only: no ambient drive (stimulate() only). */ }
            case 4u: { // scripted: TODO — placeholder, treat as constant for now.
                current += u.i_ext * input_sens_i;
            }
            default: { /* 5=off, or unknown: no drive. */ }
        }
    }

    // Leaky integration with excitability gain mapping [0,1] -> [0.5, 2.0].
    let gain = 0.5 + u.excitability * 1.5;
    let new_v = v[i] * leak_i + current * gain;
    v[i] = new_v;
    I[i] = 0;

    // Threshold check with absolute refractory period.
    let refractory_ok = !has_spiked(packed) || tick_diff(u.tick, last_tick) > refractory_i;
    if new_v >= threshold_i && refractory_ok {
        let idx = atomicAdd(&spike_count, 1u);
        spike_list[idx] = i;
        v[i] = u.reset_potential;
        last_spike[i] = HAS_SPIKED_MASK | (ntype << 24u) | (u.tick & TICK_MASK);
    }
}
