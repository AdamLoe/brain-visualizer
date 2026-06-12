//! Phase-2 determinism gate: the WGSL `target_neuron` (production scatter rule)
//! must produce the SAME synapse targets as Rust `connectivity::target()` for a
//! real manifold grid. The scatter shader reimplements the spatial rule in
//! WGSL; this proves the two implementations agree bit-for-bit so the GPU and
//! CPU backends wire the same network (BV6/BV22).
//!
//! Skips (does not fail) if no wgpu adapter is available. Native-only.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use brain_visualizer::connectivity::{target, ReachParams};
use brain_visualizer::manifold::{Manifold, ManifoldParams};
use brain_visualizer::sim::backend::neuron_type_byte;
use brain_visualizer::sim::gpu::pipelines::{HASH_WGSL, SCATTER_WGSL};
use wgpu::util::DeviceExt;

const N: usize = 4_000;
const K: u32 = 32;
const SEED: u32 = 0x5eed_5eed;

// Heavy-tailed reach: exercise the long-range tail in the gate (not just the
// local path) so the Rust↔WGSL contract is proven WITH the branch enabled.
// long_range_frac = 64 / 256 = 25%; max_reach = 6 cells.
const LONG_RANGE_FRAC: u32 = 64;
const MAX_REACH: u32 = 6;

// Harness kernel: call the production target_neuron for each (i, j) pair and
// write the result. Reuses the real scatter.wgsl bindings + ConnectUniforms.
const HARNESS: &str = r#"
struct Pair { i: u32, j: u32 };
@group(2) @binding(0) var<storage, read> pairs: array<Pair>;
@group(2) @binding(1) var<storage, read_write> out_tgt: array<u32>;

@compute @workgroup_size(64)
fn check(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= arrayLength(&pairs) { return; }
    let p = pairs[idx];
    let src_type = neuron_type(last_spike[p.i]);
    out_tgt[idx] = target_neuron(p.i, p.j, src_type);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Pair {
    i: u32,
    j: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ConnectUniforms {
    n: u32,
    k: u32,
    fixed_point_scale: f32,
    seed_lo: u32,
    grid_dim: u32,
    long_range_frac: u32,
    max_reach: u32,
    _pad: [u32; 1],
}

#[test]
fn wgsl_target_matches_rust() {
    pollster::block_on(run());
}

async fn run() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Some(adapter) =
        common::request_native_adapter_or_skip("wgsl_target_determinism", &instance).await
    else {
        return;
    };
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffers_per_shader_stage =
        adapter.limits().max_storage_buffers_per_shader_stage;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("target-determinism"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("request_device");

    // Build a real manifold + grid.
    let m = Manifold::generate(&ManifoldParams::new(N, SEED));
    let grid = &m.spatial_grid;
    let cell_of_neuron = grid.cell_of_neuron_map();

    // Per-neuron type byte (E/I + region) packed into last_spike upper bits.
    let last_spike: Vec<u32> = (0..N as u32)
        .map(|i| {
            let t = neuron_type_byte(i, SEED, m.neuron_regions[i as usize]);
            (t as u32) << 24
        })
        .collect();

    // Rust reference targets.
    let mut pairs = Vec::new();
    let mut rust_tgt = Vec::new();
    for i in 0..N as u32 {
        let st = neuron_type_byte(i, SEED, m.neuron_regions[i as usize]);
        for j in 0..K {
            pairs.push(Pair { i, j });
            rust_tgt.push(target(
                i,
                j,
                grid,
                K as usize,
                SEED,
                st,
                ReachParams {
                    long_range_frac: LONG_RANGE_FRAC,
                    max_reach: MAX_REACH,
                },
            ));
        }
    }
    let count = pairs.len();

    // --- GPU buffers matching scatter.wgsl group(0) + harness group(2) ---
    let sb = |label, data: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: data,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let dummy = sb("dummy", bytemuck::cast_slice(&[0u32; 4]));
    let last_spike_buf = sb("last_spike", bytemuck::cast_slice(&last_spike));
    let cell_of_neuron_buf = sb("cell_of_neuron", bytemuck::cast_slice(&cell_of_neuron));
    let cell_start_buf = sb("cell_start", bytemuck::cast_slice(&grid.cell_start));
    let cell_neurons_buf = sb("cell_neurons", bytemuck::cast_slice(&grid.cell_neurons));
    let i_next = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("i_next"),
        size: (N * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let max_abs = sb("max_abs", bytemuck::cast_slice(&[0u32]));

    let cu = ConnectUniforms {
        n: N as u32,
        k: K,
        fixed_point_scale: 4096.0,
        seed_lo: SEED,
        grid_dim: grid.dim,
        long_range_frac: LONG_RANGE_FRAC,
        max_reach: MAX_REACH,
        _pad: [0; 1],
    };
    let cu_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("connect_uniform"),
        contents: bytemuck::bytes_of(&cu),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let pairs_buf = sb("pairs", bytemuck::cast_slice(&pairs));
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("out_tgt"),
        size: (count * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size: (count * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Module = hash + scatter (provides target_neuron/neuron_type) + harness.
    let source = format!("{HASH_WGSL}\n{SCATTER_WGSL}\n{HARNESS}");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scatter+harness"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let st = |b, ro| wgpu::BindGroupLayoutEntry {
        binding: b,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: ro },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let uni = |b| wgpu::BindGroupLayoutEntry {
        binding: b,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let g0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("g0"),
        entries: &[
            st(0, true),
            st(1, true),
            st(2, false),
            st(3, true),
            st(4, true),
            st(5, true),
            st(6, true),
            st(7, false),
        ],
    });
    let g1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("g1"),
        entries: &[uni(0)],
    });
    let g2 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("g2"),
        entries: &[st(0, true), st(1, false)],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pl"),
        bind_group_layouts: &[Some(&g0), Some(&g1), Some(&g2)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("check"),
        layout: Some(&pl),
        module: &module,
        entry_point: Some("check"),
        compilation_options: Default::default(),
        cache: None,
    });

    fn e(b: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding: b,
            resource: buf.as_entire_binding(),
        }
    }
    let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg0"),
        layout: &g0,
        entries: &[
            e(0, &dummy),
            e(1, &dummy),
            e(2, &i_next),
            e(3, &last_spike_buf),
            e(4, &cell_of_neuron_buf),
            e(5, &cell_start_buf),
            e(6, &cell_neurons_buf),
            e(7, &max_abs),
        ],
    });
    let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg1"),
        layout: &g1,
        entries: &[e(0, &cu_buf)],
    });
    let bg2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg2"),
        layout: &g2,
        entries: &[e(0, &pairs_buf), e(1, &out_buf)],
    });

    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut cp = enc.begin_compute_pass(&Default::default());
        cp.set_pipeline(&pipeline);
        cp.set_bind_group(0, &bg0, &[]);
        cp.set_bind_group(1, &bg1, &[]);
        cp.set_bind_group(2, &bg2, &[]);
        cp.dispatch_workgroups(((count as u32) + 63) / 64, 1, 1);
    }
    enc.copy_buffer_to_buffer(&out_buf, 0, &staging, 0, (count * 4) as u64);
    queue.submit([enc.finish()]);

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
    let gpu_tgt: Vec<u32> = bytemuck::cast_slice(&data)[..count].to_vec();
    drop(data);
    staging.unmap();

    let mut mismatches = 0;
    for idx in 0..count {
        if gpu_tgt[idx] != rust_tgt[idx] {
            if mismatches < 8 {
                let p = pairs[idx];
                eprintln!(
                    "mismatch (i={},j={}): gpu={} rust={}",
                    p.i, p.j, gpu_tgt[idx], rust_tgt[idx]
                );
            }
            mismatches += 1;
        }
    }
    assert_eq!(
        mismatches, 0,
        "{mismatches}/{count} WGSL target_neuron results disagree with Rust target()"
    );
    eprintln!("[wgsl-target] PASS: {count} (i,j) targets matched GPU vs Rust");
}
