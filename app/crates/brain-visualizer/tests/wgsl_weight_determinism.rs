//! Determinism gate for the production WGSL `synapse_weight` helper.
//!
//! The target gate proves the spatial wiring rule. This focused gate proves the
//! fixed-point E/I weight rule uses the same scale and hash path in Rust and WGSL.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use brain_visualizer::connectivity::{self, weight, FIXED_POINT_SCALE};
use brain_visualizer::sim::backend::{self, SimConfig};
use brain_visualizer::sim::gpu::pipelines::{HASH_WGSL, SCATTER_WGSL, WRITE_SCATTER_DISPATCH_WGSL};
use brain_visualizer::sim::gpu::resources::ConnectUniforms;
use wgpu::util::DeviceExt;

const HARNESS: &str = r#"
struct WeightCase {
    src: u32,
    j: u32,
    source_type: u32,
    _pad: u32,
};
@group(2) @binding(0) var<storage, read> cases: array<WeightCase>;
@group(2) @binding(1) var<storage, read_write> out_weight: array<i32>;

@compute @workgroup_size(64)
fn check(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= arrayLength(&cases) { return; }
    let c = cases[idx];
    out_weight[idx] = synapse_weight(c.src, c.j, c.source_type);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WeightCase {
    src: u32,
    j: u32,
    source_type: u32,
    _pad: u32,
}

#[test]
fn fixed_point_scale_and_connect_uniform_layout_match_contract() {
    assert_eq!(backend::FIXED_POINT_SCALE, connectivity::FIXED_POINT_SCALE);
    assert_eq!(SimConfig::default().fixed_point_scale, FIXED_POINT_SCALE);
    assert_eq!(std::mem::size_of::<ConnectUniforms>(), 32);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, n), 0);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, k), 4);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, fixed_point_scale), 8);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, seed_lo), 12);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, grid_dim), 16);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, long_range_frac), 20);
    assert_eq!(std::mem::offset_of!(ConnectUniforms, max_reach), 24);

    assert_connect_uniform_field_order(SCATTER_WGSL);
    assert_connect_uniform_field_order(WRITE_SCATTER_DISPATCH_WGSL);
}

#[test]
fn wgsl_synapse_weight_matches_rust_for_ei_inputs() {
    pollster::block_on(run_weight_gate());
}

async fn run_weight_gate() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Some(adapter) =
        common::request_native_adapter_or_skip("wgsl_weight_determinism", &instance).await
    else {
        return;
    };
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("weight-determinism"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("request_device");

    let cases = weight_cases();
    let rust_weights: Vec<i32> = cases
        .iter()
        .map(|c| weight(c.src, c.j, c.source_type as u8))
        .collect();

    let cu = ConnectUniforms {
        n: 1,
        k: 16,
        fixed_point_scale: FIXED_POINT_SCALE as f32,
        seed_lo: 0,
        grid_dim: 1,
        long_range_frac: 0,
        max_reach: 1,
        _pad: [0; 1],
    };

    let cu_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("connect_uniform"),
        contents: bytemuck::bytes_of(&cu),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let cases_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("weight_cases"),
        contents: bytemuck::cast_slice(&cases),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("out_weight"),
        size: (cases.len() * std::mem::size_of::<i32>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("weight_staging"),
        size: (cases.len() * std::mem::size_of::<i32>()) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let source = format!("{HASH_WGSL}\n{SCATTER_WGSL}\n{HARNESS}");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scatter-weight-harness"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let uniform_entry = wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let storage_entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let connect_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("connect_layout"),
        entries: &[uniform_entry],
    });
    let harness_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("weight_harness_layout"),
        entries: &[storage_entry(0, true), storage_entry(1, false)],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("weight_pipeline_layout"),
        bind_group_layouts: &[None, Some(&connect_layout), Some(&harness_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("weight_check"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("check"),
        compilation_options: Default::default(),
        cache: None,
    });

    fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        }
    }
    let connect_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("connect_group"),
        layout: &connect_layout,
        entries: &[entry(0, &cu_buf)],
    });
    let harness_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("weight_harness_group"),
        layout: &harness_layout,
        entries: &[entry(0, &cases_buf), entry(1, &out_buf)],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(1, &connect_group, &[]);
        pass.set_bind_group(2, &harness_group, &[]);
        pass.dispatch_workgroups(((cases.len() as u32) + 63) / 64, 1, 1);
    }
    encoder.copy_buffer_to_buffer(
        &out_buf,
        0,
        &staging,
        0,
        (cases.len() * std::mem::size_of::<i32>()) as u64,
    );
    queue.submit([encoder.finish()]);

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().expect("map").expect("map ok");
    let data = slice.get_mapped_range();
    let gpu_weights: Vec<i32> = bytemuck::cast_slice(&data)[..cases.len()].to_vec();
    drop(data);
    staging.unmap();

    let mut mismatches = 0;
    for (idx, (gpu, rust)) in gpu_weights.iter().zip(rust_weights.iter()).enumerate() {
        if gpu != rust {
            if mismatches < 8 {
                let c = cases[idx];
                eprintln!(
                    "mismatch (src={},j={},type={}): gpu={} rust={}",
                    c.src, c.j, c.source_type, gpu, rust
                );
            }
            mismatches += 1;
        }
    }
    assert_eq!(
        mismatches,
        0,
        "{mismatches}/{} WGSL synapse_weight results disagree with Rust weight()",
        cases.len()
    );
    eprintln!(
        "[wgsl-weight] PASS: {} E/I synapse weights matched GPU vs Rust",
        cases.len()
    );
}

fn weight_cases() -> Vec<WeightCase> {
    let mut cases = Vec::new();
    for src in [0, 1, 2, 7, 31, 255, 1024, 3999] {
        for j in 0..16 {
            for source_type in [0, 1, 2, 3, 4, 5] {
                cases.push(WeightCase {
                    src,
                    j,
                    source_type,
                    _pad: 0,
                });
            }
        }
    }
    cases
}

fn assert_connect_uniform_field_order(source: &str) {
    let start = source
        .find("struct ConnectUniforms")
        .expect("ConnectUniforms");
    let body = &source[start..source[start..].find('}').expect("ConnectUniforms end") + start];
    let mut cursor = 0;
    for field in [
        "n: u32",
        "k: u32",
        "fixed_point_scale: f32",
        "seed_lo: u32",
        "grid_dim: u32",
        "long_range_frac: u32",
        "max_reach: u32",
    ] {
        let relative = body[cursor..]
            .find(field)
            .unwrap_or_else(|| panic!("missing ConnectUniforms field {field}"));
        cursor += relative + field.len();
    }
}
