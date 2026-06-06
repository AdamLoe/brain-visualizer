// V2 Phase A: metrics reduction pass.
//
// One thread per neuron. Read-only over neuron state (last_spike + v); this pass
// MUST NOT mutate any simulation buffer so determinism is preserved bit-for-bit.
// All outputs are accumulated via atomicAdd into a fixed-slot u32 storage buffer
// that the CPU reads back asynchronously (non-blocking map) once per ~15 ticks.
//
// Slot layout (METRICS_SLOT_COUNT = 32 u32; matches MetricsSlots in mod.rs):
//   0  spikes_this_tick          (has_spiked && tick_diff(now,last)==0)
//   1  input_spikes              (region==0 of the above)
//   2  assoc_spikes              (region==1)
//   3  output_spikes             (region==2)
//   4  e_spikes                  (ei==0 of the above)
//   5  i_spikes                  (ei==1)
//   6  pct_fired_100ms count     (tick_diff<=6)
//   7  pct_fired_500ms count     (tick_diff<=30)
//   8  pct_fired_2s    count     (tick_diff<=120)
//   9  voltage_sum_lo            (fixed-point, see below)
//   10 voltage_sum_hi            (carry of slot 9)
//   11 refractory_blocked        (unused this phase; always 0)
//   12..15 reserved
//   16..31 voltage_histogram[16] (counts; CPU divides by n -> fraction)
//
// WGSL has no atomic<f32>, so voltage is accumulated as a fixed-point u32.
// We offset by the clamp lo (-0.5) to keep the value non-negative, then scale
// by VOLT_FP_SCALE = 1024: q = u32((clamp(v,-0.5,1.5) + 0.5) * 1024) in [0,2048].
// To stay overflow-safe up to N = 10M (max sum ≈ 2.048e10 > u32 max 4.29e9),
// the accumulation is split into a lo/hi pair: every neuron atomicAdds q into
// slot 9, and when that add wraps past 0xFFFFFFFF it carries +1 into slot 10.
// The CPU recombines: mean_v = ((hi*2^32 + lo) / VOLT_FP_SCALE) / n - 0.5.

@group(0) @binding(0) var<storage, read> last_spike: array<u32>;
@group(0) @binding(1) var<storage, read> v: array<f32>;
@group(0) @binding(2) var<storage, read_write> metrics: array<atomic<u32>>;

struct MetricsUniforms {
    current_tick: u32,  // most-recent completed tick (self.tick - 1, 24-bit)
    n: u32,
    volt_lo: f32,       // -0.5
    volt_hi: f32,       //  1.5
    volt_scale: f32,    // 1024.0
    histo_bins: u32,    // 16
    _pad: vec2<u32>,
}
@group(1) @binding(0) var<uniform> u: MetricsUniforms;

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

// Add `q` into the 64-bit lo/hi accumulator at slots 9/10. atomicAdd returns the
// previous value; if prev + q wraps (prev > 0xFFFFFFFF - q) we carry into hi.
fn add_voltage_fp(q: u32) {
    let prev = atomicAdd(&metrics[9], q);
    if prev > (0xFFFFFFFFu - q) {
        atomicAdd(&metrics[10], 1u);
    }
}

@compute @workgroup_size(256)
fn reduce_metrics(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= u.n { return; }

    let packed = last_spike[i];

    // ── spike-timing windows ────────────────────────────────────────────────
    if has_spiked(packed) {
        let last_tick = packed & TICK_MASK;
        let dt = tick_diff(u.current_tick, last_tick);
        let ntype = neuron_type(packed);
        let region = ntype >> 2u;   // 0=Input,1=Assoc,2=Output
        let ei = ntype & 1u;        // 1=inhibitory

        if dt == 0u {
            atomicAdd(&metrics[0], 1u); // spikes_this_tick
            // region breakdown
            if region == 0u {
                atomicAdd(&metrics[1], 1u);
            } else if region == 1u {
                atomicAdd(&metrics[2], 1u);
            } else {
                atomicAdd(&metrics[3], 1u);
            }
            // E/I breakdown
            if ei == 0u {
                atomicAdd(&metrics[4], 1u);
            } else {
                atomicAdd(&metrics[5], 1u);
            }
        }
        // pct-fired windows (≈60fps assumption: 6/30/120 ticks ≈ 100ms/500ms/2s).
        if dt <= 6u   { atomicAdd(&metrics[6], 1u); }
        if dt <= 30u  { atomicAdd(&metrics[7], 1u); }
        if dt <= 120u { atomicAdd(&metrics[8], 1u); }
    }

    // ── membrane voltage (sum + histogram) ──────────────────────────────────
    let vi = clamp(v[i], u.volt_lo, u.volt_hi);
    let q = u32((vi - u.volt_lo) * u.volt_scale); // [0, (hi-lo)*scale]
    add_voltage_fp(q);

    // Histogram: bin = clamp(floor((v-lo)/(hi-lo) * bins), 0, bins-1).
    let span = u.volt_hi - u.volt_lo;
    let bf = floor((vi - u.volt_lo) / span * f32(u.histo_bins));
    let bin = u32(clamp(bf, 0.0, f32(u.histo_bins) - 1.0));
    atomicAdd(&metrics[16u + bin], 1u);
}
