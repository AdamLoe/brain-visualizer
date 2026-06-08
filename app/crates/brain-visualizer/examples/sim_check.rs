//! Phase-2 native verification harness (architecture §"verification policy").
//!
//! Drives the REAL production `GpuBackend` on a native wgpu device (llvmpipe
//! under WSL2) and reports the dynamics numbers the phase is judged on:
//!   - non-zero spikes/synaptic-events at `focused`,
//!   - excitability sweep (deep_sleep -> focused -> seizure) firing rates,
//!   - no NaN / overflow under extended seizure,
//!   - max |accumulated current| vs the i32 range.
//!
//! Run: `cargo run --release --example sim_check`. llvmpipe is slow, so N and
//! tick counts are modest; 100k / full-tier numbers are a real-GPU manual TODO.

use brain_visualizer::sim::backend::{SimBackend, SimConfig};
use brain_visualizer::sim::gpu::GpuBackend;

const N: usize = 30_000;
const K: usize = 32;
/// Biological tick = 1 ms, so ticks/sec -> Hz directly.
const TICK_MS_BIO: f32 = 1.0;

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let ctx = match GpuBackend::acquire_native().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP sim_check: {e}");
            return;
        }
    };

    let base = SimConfig {
        n: N,
        k: K,
        ..SimConfig::default()
    };
    let mut backend = GpuBackend::new(ctx, base.clone());

    // Tuning (documented; leaves all locked BV values untouched):
    //  - i_ext = 0.040: just above the input-region firing cliff so input
    //    neurons seed activity from silence (config default 0.06 over-saturates).
    //  - synaptic_scale = 0.03: effective recurrent coupling. The phase-1 locked
    //    weights are strong vs threshold=1.0 (one input ~ suprathreshold), which
    //    makes the raw network bistable (silent or refractory-capped ~166 Hz).
    //    Scaling recurrent current to 0.03 requires many coincident inputs to
    //    fire (biological realism) and opens a graded 5-20 Hz band at focused.
    const I_EXT: f32 = 0.040;
    const SYN_SCALE: f32 = 0.03;
    backend.set_i_ext(I_EXT);
    backend.set_synaptic_scale(SYN_SCALE);
    backend.initialize(&base);

    println!("=== Phase 2 GPU sim check (device above) ===");
    println!("N={N} K={K}  i_ext={I_EXT:.3}  synaptic_scale={SYN_SCALE:.3}");

    if std::env::var("SWEEP").is_ok() {
        println!("--- synaptic_scale sweep at focused (excit=0.55, i_ext=0.040) ---");
        backend.set_i_ext(0.040);
        for &ss in &[0.005f32, 0.01, 0.02, 0.03, 0.05, 0.08, 0.12, 0.20, 0.5] {
            backend.set_synaptic_scale(ss);
            backend.initialize(&base);
            for _ in 0..200 {
                backend.tick(1, 0.55);
            }
            let mut sp: u64 = 0;
            let mut peak: u64 = 0;
            for _ in 0..400 {
                let s = backend.tick(1, 0.55);
                sp += s.spikes;
                peak = peak.max(s.spikes);
            }
            let sec = 400.0 * TICK_MS_BIO / 1000.0;
            let hz = sp as f32 / (N as f32 * sec);
            let pk = 100.0 * peak as f32 / N as f32;
            println!("  syn_scale={ss:.3} -> mean_rate={hz:7.2} Hz  peak_fire/tick={pk:.1}%");
        }
        backend.set_i_ext(base.i_ext);
        return;
    }

    // Warm-up: let input-region drive ramp the network out of silence.
    let warmup = 200;
    // Measurement window per preset.
    let measure = 600;

    // Excitability presets (BV15). Each: warm up from a fresh silent state,
    // then measure mean firing rate over `measure` ticks.
    let presets = [
        ("deep_sleep", 0.10f32),
        ("focused", 0.55f32),
        ("seizure", 1.00f32),
    ];

    let mut focused_spikes_per_s = 0.0;
    let mut focused_syn_per_s = 0.0;

    for (name, excit) in presets {
        // Fresh silent start with the same seed (resize re-inits state).
        backend.initialize(&base);

        // Warm-up (drive ramps activity in from input regions).
        for _ in 0..warmup {
            backend.tick(1, excit);
        }

        // Measure: tick one-at-a-time so each TickStats.spikes is that tick's
        // exact count (the batch multiply is exact when ticks==1).
        let mut total_spikes: u64 = 0;
        let mut total_syn: u64 = 0;
        let mut max_in_tick: u64 = 0;
        for _ in 0..measure {
            let s = backend.tick(1, excit);
            total_spikes += s.spikes;
            total_syn += s.synaptic_events;
            max_in_tick = max_in_tick.max(s.spikes);
        }

        // mean firing rate (Hz) = spikes / (N * seconds), seconds = ticks * 1ms.
        let seconds = measure as f32 * TICK_MS_BIO / 1000.0;
        let mean_rate_hz = total_spikes as f32 / (N as f32 * seconds);
        let spikes_per_s = total_spikes as f32 / seconds;
        let syn_per_s = total_syn as f32 / seconds;
        let pct_peak = 100.0 * max_in_tick as f32 / N as f32;

        println!(
            "[{name:>10}] excit={excit:.2}  mean_rate={mean_rate_hz:6.2} Hz  \
             spikes/s={spikes_per_s:11.0}  syn_events/s={syn_per_s:13.0}  \
             peak_fire/tick={pct_peak:5.1}%  max|I|_hw={}",
            backend.max_abs_current_hw
        );

        if name == "focused" {
            focused_spikes_per_s = spikes_per_s;
            focused_syn_per_s = syn_per_s;
            let (mean_v, frac) = backend.debug_dynamics_snapshot();
            println!(
                "             debug: mean_v={mean_v:.3} (want [-0.5,1.5])  \
                 fired_this_tick={:.2}% (warn if >80%)",
                frac * 100.0
            );
        }
    }

    // --- Overflow / NaN stress: extended seizure run ---
    println!("--- seizure overflow/NaN stress ---");
    backend.initialize(&base);
    let stress_ticks = 2000;
    for _ in 0..stress_ticks {
        backend.tick(1, 1.0);
    }
    let max_abs = backend.max_abs_current_hw;
    let i32_max = i32::MAX as u32;
    let headroom = i32_max as f64 / max_abs.max(1) as f64;
    println!(
        "seizure {stress_ticks} ticks: max|I|_fixed_point={max_abs}  \
         i32_max={i32_max}  headroom={headroom:.1}x"
    );

    // NaN / negative-rate check: read back v and last_spike, scan for NaN.
    let (nan_count, mean_v, spiked) = scan_membrane(&backend).await;
    println!(
        "membrane scan: NaN_count={nan_count}  mean_v={mean_v:.4}  \
         ever_spiked={spiked}/{N}"
    );

    // --- Verdicts ---
    println!("=== verdicts ===");
    println!(
        "focused dynamics non-zero: spikes/s={focused_spikes_per_s:.0} \
         syn/s={focused_syn_per_s:.0} -> {}",
        if focused_spikes_per_s > 0.0 && focused_syn_per_s > 0.0 {
            "PASS"
        } else {
            "FAIL"
        }
    );
    println!(
        "no NaN membrane: {}",
        if nan_count == 0 { "PASS" } else { "FAIL" }
    );
    println!(
        "no i32 overflow at seizure: {} (max|I|={max_abs}, i32_max={i32_max})",
        if max_abs < i32_max {
            "PASS (safe)"
        } else {
            "FAIL (overflow)"
        }
    );
}

/// Read back `v` and `last_spike` from the GPU render-state buffers and report
/// (nan_count, mean_v, ever_spiked_count). One-off debug readback (allowed).
async fn scan_membrane(backend: &GpuBackend) -> (usize, f32, usize) {
    use brain_visualizer::sim::backend::{has_spiked, RenderState};
    let rs = backend.render_state();
    let (v_buf, ls_buf, n) = match rs {
        RenderState::Gpu {
            v_buf,
            last_spike_buf,
            neuron_count,
            ..
        } => (v_buf, last_spike_buf, neuron_count),
        _ => return (0, 0.0, 0),
    };
    // We need a device handle; reuse the backend's via a fresh small readback.
    let device = backend.device();
    let queue = backend.queue();

    let bytes = (n * 4) as u64;
    let mk_stage = |label| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    let v_stage = mk_stage("v_stage");
    let ls_stage = mk_stage("ls_stage");
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(v_buf, 0, &v_stage, 0, bytes);
    enc.copy_buffer_to_buffer(ls_buf, 0, &ls_stage, 0, bytes);
    queue.submit([enc.finish()]);

    let v: Vec<f32> = read_mapped(device, &v_stage, n);
    let ls: Vec<u32> = read_mapped(device, &ls_stage, n);

    let mut nan = 0;
    let mut sum = 0.0f64;
    for &x in &v {
        if x.is_nan() {
            nan += 1;
        } else {
            sum += x as f64;
        }
    }
    let spiked = ls.iter().filter(|&&w| has_spiked(w)).count();
    (nan, (sum / n as f64) as f32, spiked)
}

fn read_mapped<T: bytemuck::Pod>(
    device: &wgpu::Device,
    staging: &wgpu::Buffer,
    count: usize,
) -> Vec<T> {
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().expect("map").expect("map ok");
    let data = slice.get_mapped_range();
    let out: Vec<T> = bytemuck::cast_slice(&data)[..count].to_vec();
    drop(data);
    staging.unmap();
    out
}
