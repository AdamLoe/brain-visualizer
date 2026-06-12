//! BV22 determinism gate: prove the WGSL `hash32`/`mix_key` produce
//! **bit-identical** output to the Rust implementation for the golden vectors.
//!
//! Runs a real wgpu compute shader natively. Under WSL2 there is no real GPU,
//! so this falls back to llvmpipe (software Vulkan) — which the phase-0 bench
//! proved works. The shader source is the exact embedded `hash.wgsl` plus a
//! tiny harness kernel; if Rust and GPU disagree, GPU sim must not proceed
//! (per BV22).
//!
//! If no adapter is available at all (not even llvmpipe), the test is skipped
//! with a clear message rather than failing — but in this environment llvmpipe
//! is present.

mod common;

use brain_visualizer::connectivity::hash::{hash32, mix_key};
use brain_visualizer::sim::gpu::pipelines::HASH_WGSL;
use wgpu::util::DeviceExt;

/// Golden inputs: (seed_lo, neuron_id, synapse_j, salt). Covers each axis plus
/// representative combined values.
const GOLDEN: &[(u32, u32, u32, u32)] = &[
    (0, 0, 0, 0),
    (1, 0, 0, 0),
    (0, 1, 0, 0),
    (0, 0, 1, 0),
    (0, 0, 0, 1),
    (42, 1000, 7, 3),
    (0xdead_beef, 123, 45, 6),
    (0x5eed_5eed, 99999, 31, 4),
    (0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff),
];

const HARNESS: &str = r#"
struct Input { seed: u32, id: u32, j: u32, salt: u32 };
@group(0) @binding(0) var<storage, read> inputs: array<Input>;
@group(0) @binding(1) var<storage, read_write> out_hash: array<u32>;
@group(0) @binding(2) var<storage, read_write> out_mix: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&inputs)) { return; }
    let inp = inputs[i];
    // hash32 applied to the raw seed field, plus full mix_key.
    out_hash[i] = hash32(inp.seed);
    out_mix[i] = mix_key(inp.seed, inp.id, inp.j, inp.salt);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuInput {
    seed: u32,
    id: u32,
    j: u32,
    salt: u32,
}

#[test]
fn wgsl_matches_rust_for_golden_vectors() {
    pollster::block_on(run());
}

async fn run() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    let Some(adapter) =
        common::request_native_adapter_or_skip("wgsl_hash_determinism", &instance).await
    else {
        return;
    };
    eprintln!("[wgsl-determinism] adapter = {:?}", adapter.get_info().name);

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("hash-determinism"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("request_device");

    let count = GOLDEN.len();
    let inputs: Vec<GpuInput> = GOLDEN
        .iter()
        .map(|&(seed, id, j, salt)| GpuInput { seed, id, j, salt })
        .collect();

    let in_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("inputs"),
        contents: bytemuck::cast_slice(&inputs),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out_bytes = (count * 4) as u64;
    let make_out = |label| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: out_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    };
    let out_hash = make_out("out_hash");
    let out_mix = make_out("out_mix");
    let make_staging = |label| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: out_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    let stage_hash = make_staging("stage_hash");
    let stage_mix = make_staging("stage_mix");

    // Module = embedded BV22 hash.wgsl + the harness kernel.
    let source = format!("{HASH_WGSL}\n{HARNESS}");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("hash+harness"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl"),
        entries: &[
            storage_entry(0, true),
            storage_entry(1, false),
            storage_entry(2, false),
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("pipeline"),
        layout: Some(&pl),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: in_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: out_hash.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: out_mix.as_entire_binding(),
            },
        ],
    });

    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut cp = enc.begin_compute_pass(&Default::default());
        cp.set_pipeline(&pipeline);
        cp.set_bind_group(0, &bg, &[]);
        let groups = ((count as u32) + 63) / 64;
        cp.dispatch_workgroups(groups.max(1), 1, 1);
    }
    enc.copy_buffer_to_buffer(&out_hash, 0, &stage_hash, 0, out_bytes);
    enc.copy_buffer_to_buffer(&out_mix, 0, &stage_mix, 0, out_bytes);
    queue.submit([enc.finish()]);

    let gpu_hash = read_u32(&device, &stage_hash, count).await;
    let gpu_mix = read_u32(&device, &stage_mix, count).await;

    for (idx, &(seed, id, j, salt)) in GOLDEN.iter().enumerate() {
        let r_hash = hash32(seed);
        let r_mix = mix_key(seed, id, j, salt);
        assert_eq!(
            gpu_hash[idx], r_hash,
            "hash32 mismatch @ {idx}: gpu=0x{:08x} rust=0x{:08x} (seed=0x{seed:08x})",
            gpu_hash[idx], r_hash
        );
        assert_eq!(
            gpu_mix[idx], r_mix,
            "mix_key mismatch @ {idx}: gpu=0x{:08x} rust=0x{:08x} ({seed},{id},{j},{salt})",
            gpu_mix[idx], r_mix
        );
        eprintln!(
            "[ok] ({seed:#010x},{id},{j},{salt}) hash32=0x{r_hash:08x} mix_key=0x{r_mix:08x}"
        );
    }
    eprintln!("[wgsl-determinism] PASS: {count} golden vectors matched on GPU and Rust");
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
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

async fn read_u32(device: &wgpu::Device, staging: &wgpu::Buffer, count: usize) -> Vec<u32> {
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().expect("map channel").expect("map ok");
    let data = slice.get_mapped_range();
    let out: Vec<u32> = bytemuck::cast_slice(&data)[..count].to_vec();
    drop(data);
    staging.unmap();
    out
}
