// Phase 0 — Throwaway Benchmark Spike
// Measures LIF neuron throughput: GPU (wgpu native) + CPU (rayon).
// Uses the exact BV22 hash primitive from decisions.md.
// NOT production code — will be deleted/archived after numbers are captured.

use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicI32, Ordering};
use rayon::prelude::*;

// ---------------------------------------------------------------------------
// BV22 hash — locked 32-bit hash (decisions.md). Identical to WGSL version.
// ---------------------------------------------------------------------------
#[inline(always)]
fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

/// Mix neuron_id + synapse_index + salt then hash. Matches WGSL mix_key.
#[inline(always)]
fn mix_key(neuron_id: u32, synapse_index: u32, salt: u32) -> u32 {
    let k = neuron_id
        .wrapping_add(synapse_index.wrapping_mul(0x9e3779b9))
        .wrapping_add(salt.wrapping_mul(0x6c62272e));
    hash32(k)
}

// ---------------------------------------------------------------------------
// Constants (BV19: S = 2^12 = 4096)
// ---------------------------------------------------------------------------
const S: i32 = 4096;
const LEAK: f32 = 0.95;
const THRESH: f32 = 1.0;
const RESET_V: f32 = 0.0;
const SCALE_INV: f32 = 1.0 / S as f32;
const WEIGHT_FP: i32 = (0.05 * S as f32) as i32; // ≈205

// ---------------------------------------------------------------------------
// WGSL shaders
// ---------------------------------------------------------------------------

const INTEGRATE_SHADER: &str = r#"
struct Params {
    n: u32,
    k: u32,
    tick: u32,
    scatter_stride_x: u32,
};

@group(0) @binding(0) var<storage, read_write> v: array<f32>;
@group(0) @binding(1) var<storage, read_write> I: array<atomic<i32>>;
@group(0) @binding(2) var<storage, read_write> last_spike: array<u32>;
@group(0) @binding(3) var<storage, read_write> spike_list: array<u32>;
@group(0) @binding(4) var<storage, read_write> spike_count: atomic<u32>;
@group(0) @binding(5) var<uniform> params: Params;

const LEAK: f32 = 0.95;
const THRESH: f32 = 1.0;
const RESET_V: f32 = 0.0;
const SCALE_INV: f32 = 1.0 / 4096.0;
// Benchmark external drive: inject 0.06 per tick to every 20th neuron
// so the network maintains ~5% firing rate. This is a benchmark-only
// approximation of biological thalamic drive.
const I_EXT_FP: i32 = 245;  // 0.06 * 4096 ≈ 245
const DRIVE_STEP: u32 = 20u;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n { return; }
    // Add ambient drive to every DRIVE_STEP-th neuron
    var ext: i32 = 0i;
    if (idx % DRIVE_STEP) == 0u { ext = I_EXT_FP; }
    let i_val = atomicLoad(&I[idx]) + ext;
    atomicStore(&I[idx], 0i);
    let current = f32(i_val) * SCALE_INV;
    let new_v = v[idx] * LEAK + current;
    if new_v >= THRESH {
        let slot = atomicAdd(&spike_count, 1u);
        if slot < params.n {
            spike_list[slot] = idx;
        }
        v[idx] = RESET_V;
        last_spike[idx] = 0x80000000u | (params.tick & 0x00FFFFFFu);
    } else {
        v[idx] = new_v;
    }
}
"#;

const SCATTER_SHADER: &str = r#"
struct Params {
    n: u32,
    k: u32,
    tick: u32,
    scatter_stride_x: u32,
};

@group(0) @binding(0) var<storage, read_write> I: array<atomic<i32>>;
@group(0) @binding(1) var<storage, read> spike_list: array<u32>;
@group(0) @binding(2) var<storage, read> spike_count_buf: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;

const WEIGHT_FP: i32 = 205;

fn hash32(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u;
    x = x * 0x7feb352du;
    x ^= x >> 15u;
    x = x * 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn mix_key(neuron_id: u32, synapse_index: u32, salt: u32) -> u32 {
    let k2 = neuron_id + synapse_index * 0x9e3779b9u + salt * 0x6c62272eu;
    return hash32(k2);
}

// 2D dispatch: event_idx = gid.x + gid.y * params.scatter_stride_x * 64
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sc = spike_count_buf[0];
    let total_events = sc * params.k;
    let event_idx = gid.x + gid.y * (params.scatter_stride_x * 64u);
    if event_idx >= total_events { return; }
    let spike_slot = event_idx / params.k;
    let synapse_j = event_idx % params.k;
    let src = spike_list[spike_slot];
    let tgt = mix_key(src, synapse_j, 1u) % params.n;
    atomicAdd(&I[tgt], WEIGHT_FP);
}
"#;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuParams {
    n: u32,
    k: u32,
    tick: u32,
    scatter_stride_x: u32, // for 2D scatter dispatch: see SCATTER_SHADER
}

fn bgl_entry_storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bgl_entry_uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

// ---------------------------------------------------------------------------
// GPU inner benchmark — allocate once, reuse across ticks
// ---------------------------------------------------------------------------
fn run_gpu_bench_inner(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    limits: &wgpu::Limits,
    n: usize,
    k: u32,
    ticks: usize,
) {
    use wgpu::util::DeviceExt;

    let n_u32 = n as u32;
    let max_wg = limits.max_compute_workgroups_per_dimension as u64;
    let integrate_wg = ((n as u64) + 255) / 256;
    // scatter: worst case total workgroups = ceil(N*K / 64)
    // Use 2D dispatch to work around maxComputeWorkgroupsPerDimension.
    // X = min(total_wg, max_wg), Y = ceil(total_wg / scatter_wg_x)
    let scatter_total_wg = ((n as u64 * k as u64) + 63) / 64;
    let scatter_wg_x = scatter_total_wg.min(max_wg) as u32;
    let scatter_wg_y = ((scatter_total_wg + scatter_wg_x as u64 - 1) / scatter_wg_x as u64) as u32;

    if integrate_wg > max_wg {
        println!(
            "N={:<8} K={:<4} SKIPPED: integrate needs {} workgroups > max {}",
            n, k, integrate_wg, max_wg
        );
        return;
    }
    if scatter_wg_y > max_wg as u32 {
        println!(
            "N={:<8} K={:<4} SKIPPED: scatter 2D dispatch ({} × {}) Y exceeds max {}",
            n, k, scatter_wg_x, scatter_wg_y, max_wg
        );
        return;
    }

    // --- Allocate buffers (once) ---
    // Seed v: 5% of neurons above threshold (fire immediately tick 0),
    // rest distributed below.
    let v_data: Vec<f32> = (0..n)
        .map(|i| if i % 20 == 0 { 1.1f32 } else { (i as f32 * 0.047) % 0.9 })
        .collect();
    // Seed I buffer with ongoing drive for driven neurons so they refire ~every 20 ticks.
    // drive = 0.06 * S = 245 per tick; pre-charge with 20 ticks worth.
    let drive_fp_gpu: i32 = (0.06 * S as f32) as i32; // 245
    let i_data: Vec<i32> = (0..n)
        .map(|i| if i % 20 == 0 { drive_fp_gpu * 20 } else { 0 })
        .collect();
    let ls_data = vec![0u32; n];
    let sl_data = vec![0u32; n];
    let zero = [0u32; 1];
    let params_init = GpuParams { n: n_u32, k, tick: 0, scatter_stride_x: scatter_wg_x };

    macro_rules! storage_buf {
        ($label:expr, $data:expr, $extra:expr) => {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some($label),
                contents: $data,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | $extra,
            })
        };
    }

    let buf_v  = storage_buf!("v",          bytemuck::cast_slice(&v_data),  wgpu::BufferUsages::empty());
    let buf_i  = storage_buf!("I",          bytemuck::cast_slice(&i_data),  wgpu::BufferUsages::empty());
    let buf_ls = storage_buf!("last_spike", bytemuck::cast_slice(&ls_data), wgpu::BufferUsages::empty());
    let buf_sl = storage_buf!("spike_list", bytemuck::cast_slice(&sl_data), wgpu::BufferUsages::empty());
    let buf_sc = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("spike_count"),
        contents: bytemuck::cast_slice(&zero),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    });
    let buf_sc_copy = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("spike_count_copy"),
        contents: bytemuck::cast_slice(&zero),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let buf_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("params"),
        contents: bytemuck::bytes_of(&params_init),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // --- Shaders ---
    let int_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("integrate"),
        source: wgpu::ShaderSource::Wgsl(INTEGRATE_SHADER.into()),
    });
    let scat_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scatter"),
        source: wgpu::ShaderSource::Wgsl(SCATTER_SHADER.into()),
    });

    // --- Bind group layouts ---
    let int_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("int_bgl"),
        entries: &[
            bgl_entry_storage(0, false),
            bgl_entry_storage(1, false),
            bgl_entry_storage(2, false),
            bgl_entry_storage(3, false),
            bgl_entry_storage(4, false),
            bgl_entry_uniform(5),
        ],
    });
    let scat_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scat_bgl"),
        entries: &[
            bgl_entry_storage(0, false),
            bgl_entry_storage(1, true),
            bgl_entry_storage(2, true),
            bgl_entry_uniform(3),
        ],
    });

    // --- Pipelines ---
    let int_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("int_pl"),
        bind_group_layouts: &[Some(&int_bgl)],
        immediate_size: 0,
    });
    let scat_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("scat_pl"),
        bind_group_layouts: &[Some(&scat_bgl)],
        immediate_size: 0,
    });

    let int_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("integrate"),
        layout: Some(&int_pl_layout),
        module: &int_mod,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let scat_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("scatter"),
        layout: Some(&scat_pl_layout),
        module: &scat_mod,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    // --- Bind groups ---
    let int_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("int_bg"),
        layout: &int_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buf_v.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: buf_i.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: buf_ls.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: buf_sl.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: buf_sc.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: buf_params.as_entire_binding() },
        ],
    });
    let scat_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scat_bg"),
        layout: &scat_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buf_i.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: buf_sl.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: buf_sc_copy.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: buf_params.as_entire_binding() },
        ],
    });

    // Warmup tick (outside the timed window)
    {
        queue.write_buffer(&buf_params, 0, bytemuck::bytes_of(&params_init));
        queue.write_buffer(&buf_sc, 0, bytemuck::cast_slice(&zero));
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut cp = enc.begin_compute_pass(&Default::default());
            cp.set_pipeline(&int_pipeline);
            cp.set_bind_group(0, &int_bg, &[]);
            cp.dispatch_workgroups(integrate_wg as u32, 1, 1);
        }
        enc.copy_buffer_to_buffer(&buf_sc, 0, &buf_sc_copy, 0, 4);
        {
            let mut cp = enc.begin_compute_pass(&Default::default());
            cp.set_pipeline(&scat_pipeline);
            cp.set_bind_group(0, &scat_bg, &[]);
            cp.dispatch_workgroups(scatter_wg_x, scatter_wg_y, 1);
        }
        queue.submit([enc.finish()]);
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    }

    // Reset to initial state before timing
    queue.write_buffer(&buf_v, 0, bytemuck::cast_slice(&v_data));
    queue.write_buffer(&buf_i, 0, bytemuck::cast_slice(&i_data));
    queue.write_buffer(&buf_ls, 0, bytemuck::cast_slice(&ls_data));
    queue.write_buffer(&buf_sc, 0, bytemuck::cast_slice(&zero));
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });

    // --- Timed loop ---
    let budget = Duration::from_secs(60);
    let start = Instant::now();
    let mut completed_ticks = 0usize;

    for tick in 0u32..ticks as u32 {
        if start.elapsed() > budget {
            let e = start.elapsed().as_secs_f64();
            let tps = completed_ticks as f64 / e;
            let syn_per_sec = n as f64 * 0.05 * k as f64 * completed_ticks as f64 / e;
            println!(
                "N={:<8} K={:<4} ticks={}/{} TIMEOUT  time={:.1}s  ticks/s={:.0}  synaptic_events/s={:.3}M  (5% fire est)",
                n, k, completed_ticks, ticks, e, tps, syn_per_sec / 1e6
            );
            return;
        }

        let params = GpuParams { n: n_u32, k, tick, scatter_stride_x: scatter_wg_x };
        queue.write_buffer(&buf_params, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&buf_sc, 0, bytemuck::cast_slice(&zero));

        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut cp = enc.begin_compute_pass(&Default::default());
            cp.set_pipeline(&int_pipeline);
            cp.set_bind_group(0, &int_bg, &[]);
            cp.dispatch_workgroups(integrate_wg as u32, 1, 1);
        }
        enc.copy_buffer_to_buffer(&buf_sc, 0, &buf_sc_copy, 0, 4);
        {
            let mut cp = enc.begin_compute_pass(&Default::default());
            cp.set_pipeline(&scat_pipeline);
            cp.set_bind_group(0, &scat_bg, &[]);
            cp.dispatch_workgroups(scatter_wg_x, scatter_wg_y, 1);
        }
        queue.submit([enc.finish()]);
        let _ = device.poll(wgpu::PollType::Poll);
        completed_ticks += 1;
    }
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });

    let elapsed = start.elapsed();
    let elapsed_s = elapsed.as_secs_f64();
    let tps = completed_ticks as f64 / elapsed_s;
    // Estimate events at 5% firing rate (biological estimate; no CPU readback of spike_count)
    let syn_per_sec = n as f64 * 0.05 * k as f64 * completed_ticks as f64 / elapsed_s;

    println!(
        "N={:<8} K={:<4} ticks={} time={:.2}s  ticks/s={:.0}  synaptic_events/s={:.3}M  (5% fire est)",
        n, k, ticks, elapsed_s, tps, syn_per_sec / 1e6
    );
}

// ---------------------------------------------------------------------------
// GPU benchmark entry
// ---------------------------------------------------------------------------
async fn run_gpu_bench() {
    let instance = wgpu::Instance::new(
        wgpu::InstanceDescriptor::new_without_display_handle(),
    );

    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
    {
        Ok(a) => a,
        Err(e) => {
            println!("=== GPU Benchmark ===");
            println!("No GPU adapter found: {e}");
            println!("(WSL2: likely no Vulkan ICD or compatible wgpu backend)");
            println!("Skipping GPU benchmark; CPU benchmark will still run.");
            return;
        }
    };

    let info = adapter.get_info();
    let features = adapter.features();
    let limits = adapter.limits();
    let has_ts = features.contains(wgpu::Features::TIMESTAMP_QUERY);

    println!("=== GPU Benchmark ===");
    println!("adapter={:?}", info.name);
    println!("type={:?}  backend={:?}  driver={:?}", info.device_type, info.backend, info.driver);
    println!("limits maxStorageBufferBindingSize={}", limits.max_storage_buffer_binding_size);
    println!("limits maxBufferSize={}", limits.max_buffer_size);
    println!("limits maxComputeWorkgroupsPerDimension={}", limits.max_compute_workgroups_per_dimension);
    println!("limits maxComputeInvocationsPerWorkgroup={}", limits.max_compute_invocations_per_workgroup);
    println!("limits maxComputeWorkgroupSizeX={}", limits.max_compute_workgroup_size_x);
    println!("timestamp_query={}", has_ts);
    println!();

    let required_limits = wgpu::Limits {
        max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
        max_buffer_size: limits.max_buffer_size,
        ..wgpu::Limits::default()
    };

    let (device, queue) = match adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("bench"),
            required_features: wgpu::Features::empty(),
            required_limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
    {
        Ok(dq) => dq,
        Err(e) => {
            println!("Failed to create GPU device: {e}. Skipping GPU benchmark.");
            return;
        }
    };

    // GPU N/K/ticks points
    let points: &[(usize, u32, usize)] = &[
        (100_000,   32, 1000),
        (500_000,   32, 500),
        (1_000_000, 32, 200),
        (5_000_000, 32, 100),
    ];

    for &(n, k, ticks) in points {
        run_gpu_bench_inner(&device, &queue, &limits, n, k, ticks);
    }

    // K=64 bonus point
    println!("--- K=64 point ---");
    run_gpu_bench_inner(&device, &queue, &limits, 500_000, 64, 500);
}

// ---------------------------------------------------------------------------
// CPU benchmark — rayon, event-driven, fixed-point AtomicI32 scatter
// ---------------------------------------------------------------------------
fn run_cpu_bench(n: usize, k: u32, ticks: usize, threads: usize) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("rayon pool");

    // Background drive: constant current added to ~5% of neurons each tick
    // so the network maintains a realistic ~5% firing rate for scatter load.
    // I_ext = enough to push a neuron at v=0 over threshold in ~20 ticks:
    // v grows as I_ext * (1 + LEAK + LEAK^2 + ...) = I_ext / (1 - LEAK).
    // For v >= 1.0: I_ext >= 1.0 * (1 - 0.95) = 0.05 per tick.
    // Use slightly higher drive: 0.06 to ensure sustained firing.
    let drive_fp: i32 = (0.06 * S as f32) as i32;
    // Drive period: inject into every 20th neuron
    let drive_step = 20usize;

    pool.install(|| {
        // SoA layout
        let v: Vec<f32> = vec![0.0f32; n];
        let i_buf: Vec<AtomicI32> = (0..n).map(|_| AtomicI32::new(0)).collect();

        // Pre-inject drive so first ticks have immediate firings
        for i in (0..n).step_by(drive_step) {
            i_buf[i].fetch_add(drive_fp * 20, Ordering::Relaxed);
        }

        let start = Instant::now();
        let mut total_synaptic_events: u64 = 0;

        for tick in 0u32..ticks as u32 {
            // Inject ambient drive before integrate
            for i in (0..n).step_by(drive_step) {
                i_buf[i].fetch_add(drive_fp, Ordering::Relaxed);
            }

            // Integrate + threshold in parallel. Use raw pointer writes for v
            // because rayon gives disjoint indices — no aliasing.
            let fired: Vec<u32> = (0..n)
                .into_par_iter()
                .filter_map(|idx| {
                    let i_val = i_buf[idx].swap(0, Ordering::Relaxed);
                    let cur = i_val as f32 * SCALE_INV;
                    let new_v = v[idx] * LEAK + cur;
                    // SAFETY: each idx is visited by exactly one rayon thread.
                    if new_v >= THRESH {
                        unsafe { *(v.as_ptr().add(idx) as *mut f32) = RESET_V; }
                        Some(idx as u32)
                    } else {
                        unsafe { *(v.as_ptr().add(idx) as *mut f32) = new_v; }
                        None
                    }
                })
                .collect();

            // Scatter: parallel over fired neurons, atomic fixed-point add
            total_synaptic_events += fired.len() as u64 * k as u64;
            fired.par_iter().for_each(|&src| {
                for j in 0..k {
                    let target = (mix_key(src, j, 1) as usize) % n;
                    i_buf[target].fetch_add(WEIGHT_FP, Ordering::Relaxed);
                }
            });

            let _ = (tick, &i_buf); // suppress unused warnings
        }

        let elapsed = start.elapsed();
        let elapsed_s = elapsed.as_secs_f64();
        let tps = ticks as f64 / elapsed_s;
        let syn_per_sec = total_synaptic_events as f64 / elapsed_s;
        let avg_fired = if ticks > 0 { total_synaptic_events / ticks as u64 / k as u64 } else { 0 };

        println!(
            "N={:<8} K={:<4} ticks={} time={:.2}s  ticks/s={:.0}  synaptic_events/s={:.3}M  avg_fired/tick={}",
            n, k, ticks, elapsed_s, tps, syn_per_sec / 1e6, avg_fired
        );
    });
}

fn run_all_cpu_benches() {
    let threads = rayon::current_num_threads();
    println!("\n=== CPU Benchmark ({} threads) ===", threads);

    let points: &[(usize, u32, usize)] = &[
        (10_000,   32, 2000),
        (50_000,   32, 1000),
        (100_000,  32, 500),
        (500_000,  32, 200),
    ];
    for &(n, k, ticks) in points {
        run_cpu_bench(n, k, ticks, threads);
    }

    println!("--- K=64 point ---");
    run_cpu_bench(50_000, 64, 1000, threads);
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------
fn main() {
    println!("Brain Visualizer — Phase 0 Benchmark");
    println!("Machine: WSL2, 20 cores, 31 GB RAM");
    println!("Date: 2026-06-03");
    println!();

    pollster::block_on(run_gpu_bench());
    run_all_cpu_benches();

    println!("\nDone.");
}
