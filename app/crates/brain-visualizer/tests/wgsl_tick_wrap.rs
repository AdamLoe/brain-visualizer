//! WGSL 24-bit tick-wrap gates for production shader helpers.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use brain_visualizer::sim::backend::{HAS_SPIKED_MASK, TICK_MASK};
use brain_visualizer::sim::gpu::pipelines::{
    HASH_WGSL, INTEGRATE_WGSL, METRICS_WGSL, RENDER_FAR_WGSL,
};
use wgpu::util::DeviceExt;

const TICK_CASES: usize = 5;

#[test]
fn production_wgsl_tick_diff_wraps_like_rust() {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Some(adapter) =
            common::request_native_adapter_or_skip("wgsl_tick_wrap", &instance).await
        else {
            return;
        };
        let (device, queue) = request_device(&adapter).await;

        let integrate = format!("{HASH_WGSL}\n{INTEGRATE_WGSL}");
        assert_eq!(
            run_tick_diff_harness(&device, &queue, "integrate_tick_diff", &integrate).await,
            expected_tick_cases()
        );
        assert_eq!(
            run_tick_diff_harness(&device, &queue, "metrics_tick_diff", METRICS_WGSL).await,
            expected_tick_cases()
        );
        let render_far = format!("{HASH_WGSL}\n{RENDER_FAR_WGSL}");
        assert_eq!(
            run_tick_diff_harness(&device, &queue, "render_far_tick_diff", &render_far).await,
            expected_tick_cases()
        );
    });
}

#[test]
fn metrics_windows_count_spikes_across_tick_wrap() {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Some(adapter) =
            common::request_native_adapter_or_skip("wgsl_metrics_tick_windows", &instance).await
        else {
            return;
        };
        let (device, queue) = request_device(&adapter).await;
        let metrics = run_metrics_wrap_harness(&device, &queue).await;

        assert_eq!(metrics[0], 1, "spikes_this_tick");
        assert_eq!(metrics[1], 1, "input spikes");
        assert_eq!(metrics[4], 1, "excitatory spikes");
        assert_eq!(metrics[6], 2, "100ms window across wrap");
        assert_eq!(metrics[7], 2, "500ms window across wrap");
        assert_eq!(metrics[8], 3, "2s window across wrap");
    });
}

async fn request_device(adapter: &wgpu::Adapter) -> (wgpu::Device, wgpu::Queue) {
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("tick-wrap"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("request_device")
}

fn expected_tick_cases() -> Vec<u32> {
    vec![1, 5, 0x0080_0000, 0x007F_FFFF, 0x007F_FFFF]
}

async fn run_tick_diff_harness(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    production_source: &str,
) -> Vec<u32> {
    const HARNESS: &str = r#"
@group(2) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn check_tick_diff() {
    out[0] = tick_diff(0x00000000u, 0x00ffffffu);
    out[1] = tick_diff(0x00000003u, 0x00fffffeu);
    out[2] = tick_diff(0x00800000u, 0x00000000u);
    out[3] = tick_diff(0x007fffffu, 0x00000000u);
    out[4] = tick_diff(0x00000000u, 0x00800001u);
}
"#;
    let source = format!("{production_source}\n{HARNESS}");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tick_diff_out"),
        size: (TICK_CASES * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = staging_buffer(device, (TICK_CASES * 4) as u64);
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("tick_diff_bgl"),
        entries: &[storage_entry(0, false)],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("tick_diff_layout"),
        bind_group_layouts: &[None, None, Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("tick_diff_pipeline"),
        layout: Some(&layout),
        module: &module,
        entry_point: Some("check_tick_diff"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tick_diff_bg"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: out.as_entire_binding(),
        }],
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut cp = enc.begin_compute_pass(&Default::default());
        cp.set_pipeline(&pipeline);
        cp.set_bind_group(2, &bg, &[]);
        cp.dispatch_workgroups(1, 1, 1);
    }
    enc.copy_buffer_to_buffer(&out, 0, &staging, 0, (TICK_CASES * 4) as u64);
    queue.submit([enc.finish()]);
    read_u32(device, &staging, TICK_CASES).await
}

async fn run_metrics_wrap_harness(device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u32> {
    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct MetricsUniforms {
        current_tick: u32,
        n: u32,
        volt_lo: f32,
        volt_hi: f32,
        volt_scale: f32,
        histo_bins: u32,
        _pad: [u32; 2],
    }

    let n = 5u32;
    let last_spike = [
        HAS_SPIKED_MASK | (0u32 << 24) | 3,
        HAS_SPIKED_MASK | (5u32 << 24) | TICK_MASK,
        HAS_SPIKED_MASK | (8u32 << 24) | 0x00ff_ffe0,
        HAS_SPIKED_MASK | (8u32 << 24) | 0x00ff_feff,
        0,
    ];
    let voltages = [0.0f32; 5];
    let metrics_zero = [0u32; 32];
    let uniforms = MetricsUniforms {
        current_tick: 3,
        n,
        volt_lo: -0.5,
        volt_hi: 1.5,
        volt_scale: 1024.0,
        histo_bins: 16,
        _pad: [0; 2],
    };

    let last_spike_buf = storage_init(device, "last_spike", bytemuck::cast_slice(&last_spike));
    let v_buf = storage_init(device, "v", bytemuck::cast_slice(&voltages));
    let metrics_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("metrics"),
        contents: bytemuck::cast_slice(&metrics_zero),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("metrics_uniform"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let staging = staging_buffer(device, 32 * 4);

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("metrics_wrap"),
        source: wgpu::ShaderSource::Wgsl(METRICS_WGSL.into()),
    });
    let g0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("metrics_g0"),
        entries: &[
            storage_entry(0, true),
            storage_entry(1, true),
            storage_entry(2, false),
        ],
    });
    let g1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("metrics_g1"),
        entries: &[uniform_entry(0)],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("metrics_layout"),
        bind_group_layouts: &[Some(&g0), Some(&g1)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("metrics_pipeline"),
        layout: Some(&layout),
        module: &module,
        entry_point: Some("reduce_metrics"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("metrics_bg0"),
        layout: &g0,
        entries: &[
            bind_entry(0, &last_spike_buf),
            bind_entry(1, &v_buf),
            bind_entry(2, &metrics_buf),
        ],
    });
    let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("metrics_bg1"),
        layout: &g1,
        entries: &[bind_entry(0, &uniform_buf)],
    });

    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut cp = enc.begin_compute_pass(&Default::default());
        cp.set_pipeline(&pipeline);
        cp.set_bind_group(0, &bg0, &[]);
        cp.set_bind_group(1, &bg1, &[]);
        cp.dispatch_workgroups(1, 1, 1);
    }
    enc.copy_buffer_to_buffer(&metrics_buf, 0, &staging, 0, 32 * 4);
    queue.submit([enc.finish()]);
    read_u32(device, &staging, 32).await
}

fn storage_init(device: &wgpu::Device, label: &str, contents: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::STORAGE,
    })
}

fn staging_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
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

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

fn bind_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
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
