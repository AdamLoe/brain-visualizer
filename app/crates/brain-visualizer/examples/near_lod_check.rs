//! Phase-4 near-LOD headless verification harness.
//!
//! Verifies the close/far render paths after the near-LOD retirement:
//!   1. At close camera distance (< 0.8 world units): the near-LOD sphere/cylinder
//!      path is now RETIRED (DRAW_LEGACY_NEAR_SPHERES / DRAW_LEGACY_CYLINDERS both
//!      false), so 0 neuron AND 0 synapse instances are emitted; the soft
//!      billboards (render_far.wgsl) render the bodies → non-black pixels.
//!   2. At far camera distance (> 1.5 world units): near-LOD is skipped (instance
//!      counts stay 0 / passes not encoded).
//!   3. Clamp/overflow counters are 0 at these sizes.
//!
//! Run: `cargo run --release --example near_lod_check`

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    pollster::block_on(run());
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
async fn run() {
    use brain_visualizer::sim::backend::{SimBackend, SimConfig};
    use brain_visualizer::sim::gpu::GpuBackend;

    const N: usize = 3_000;
    const K: usize = 16;
    const WIDTH: u32 = 256;
    const HEIGHT: u32 = 256;
    const WARM_TICKS: u32 = 300;

    // --- 1. Acquire device ---
    let ctx = match GpuBackend::acquire_native().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP near_lod_check: {e}");
            return;
        }
    };

    eprintln!("[near_lod_check] device acquired");

    // --- 2. Build backend ---
    let config = SimConfig {
        n: N,
        k: K,
        ..SimConfig::default()
    };
    let mut backend = GpuBackend::new(ctx, config.clone());
    backend.set_i_ext(0.040);
    backend.set_synaptic_scale(0.03);
    backend.initialize(&config);

    // --- 3. Build render pipelines ---
    let color_format = wgpu::TextureFormat::Rgba8Unorm;
    backend.build_render_pipelines(color_format);
    backend.resize_render_targets(WIDTH, HEIGHT);

    // --- 4. Create offscreen colour texture ---
    let color_tex = backend.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen-color-near"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: color_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color_tex.create_view(&Default::default());

    // --- 5. Warm up simulation ---
    println!("[near_lod_check] warming up {WARM_TICKS} ticks at focused …");
    for _ in 0..WARM_TICKS {
        backend.tick(1, 0.55);
    }
    let stats = backend.tick(1, 0.55);
    println!(
        "[near_lod_check] post-warmup: spikes/tick = {}",
        stats.spikes
    );
    assert!(
        stats.spikes > 0,
        "Expected some spikes after warm-up at focused"
    );

    // --- 6. Test: FAR camera (distance = 3.0 > 1.5) → near-LOD must be skipped ---
    println!("[near_lod_check] FAR camera test (distance=3.0, should skip near-LOD) …");
    let far_dist = 3.0f32;
    let (cr, cu) = camera_vectors(0.3, 0.4);
    let far_eye = orbit_eye(0.3, 0.4, far_dist);
    let far_mvp = camera_mvp(0.3, 0.4, far_dist, WIDTH as f32 / HEIGHT as f32);

    // Set far distance — near-LOD should NOT run.
    backend.set_lod_camera_distance(far_dist);
    backend.render_full(
        &color_view,
        &far_mvp,
        cr,
        cu,
        100.0,
        0.012,
        far_eye,
        far_dist,
    );

    let far_stats = backend.near_lod_stats();
    println!(
        "[near_lod_check] FAR: emitted_neurons={}, emitted_synapses={}",
        far_stats.emitted_neuron_instances, far_stats.emitted_synapse_instances,
    );
    // At FAR distance the near-LOD run_near_lod == false so profiler_staging is never
    // populated by the current frame. Stats should be default (0 from init, or from a
    // prior frame). We just assert that the frame didn't crash and no overflow.
    assert_eq!(
        far_stats.neuron_overflow, 0,
        "unexpected overflow at far dist"
    );
    assert_eq!(
        far_stats.synapse_overflow, 0,
        "unexpected overflow at far dist"
    );
    println!("[near_lod_check] FAR camera test: PASS (no crash, no overflow)");

    // --- 7. Test: CLOSE camera (distance = 0.3 < 0.8) → near-LOD must run ---
    println!("[near_lod_check] CLOSE camera test (distance=0.3, near-LOD active) …");
    let close_dist = 0.3f32;
    let close_eye = orbit_eye(0.3, 0.1, close_dist);
    let close_mvp = camera_mvp(0.3, 0.1, close_dist, WIDTH as f32 / HEIGHT as f32);
    let (cr2, cu2) = camera_vectors(0.3, 0.1);

    backend.set_lod_camera_distance(close_dist);
    backend.render_full(
        &color_view,
        &close_mvp,
        cr2,
        cu2,
        100.0,
        0.012,
        close_eye,
        close_dist,
    );

    let close_stats = backend.near_lod_stats();
    println!(
        "[near_lod_check] CLOSE: emitted_neurons={}, emitted_synapses={}, \
         neuron_overflow={}, synapse_overflow={}",
        close_stats.emitted_neuron_instances,
        close_stats.emitted_synapse_instances,
        close_stats.neuron_overflow,
        close_stats.synapse_overflow,
    );

    // UX fix (near-LOD / shadow line): the near-LOD faceted icosphere body is now
    // retired too (DRAW_LEGACY_NEAR_SPHERES=false in gpu/mod.rs), mirroring the
    // cylinder retirement. The soft additive billboards (render_far.wgsl) are the
    // body visual at ALL camera distances. With near-LOD fully gated off, the
    // cull_neurons compute never runs, so 0 emitted neuron instances is now the
    // EXPECTED, correct result at close zoom. The non-black pixel assertion below
    // still verifies the close-up scene renders (via the billboards).
    assert_eq!(
        close_stats.emitted_neuron_instances, 0,
        "near-LOD spheres are retired (DRAW_LEGACY_NEAR_SPHERES=false); expected 0 \
         neuron instances, got {}",
        close_stats.emitted_neuron_instances
    );
    // Legacy near-LOD cylinder connections were already retired in V2 (Phase E)
    // via DRAW_LEGACY_CYLINDERS=false — the active-edge ribbon renderer (Phase D)
    // is the one connection visual — so 0 emitted synapse instances is expected.
    assert_eq!(
        close_stats.emitted_synapse_instances, 0,
        "legacy cylinders are retired in V2 (Phase E); expected 0 synapse \
         instances, got {}",
        close_stats.emitted_synapse_instances
    );

    // With N=3k and MAX_NEAR=32768 / MAX_SYN=262144 there must be no overflow.
    assert_eq!(
        close_stats.neuron_overflow, 0,
        "neuron overflow at small N — capacity too small?"
    );
    assert_eq!(
        close_stats.synapse_overflow, 0,
        "synapse overflow at small N — capacity too small?"
    );

    // --- 8. Read back pixels and assert non-black ---
    let pixels = readback_rgba(backend.device(), backend.queue(), &color_tex, WIDTH, HEIGHT).await;
    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    let mut non_black = 0u32;
    for chunk in pixels.chunks(4) {
        let (r, g, b) = (chunk[0], chunk[1], chunk[2]);
        if r > 2 || g > 2 || b > 2 {
            non_black += 1;
        }
    }
    let total = WIDTH * HEIGHT;
    let frac = non_black as f32 / total as f32;

    println!(
        "[near_lod_check] non-black pixels: {non_black}/{total} ({:.2}%)",
        frac * 100.0
    );

    assert!(
        non_black > 0,
        "All pixels are black — sphere/cylinder render produced nothing"
    );
    println!("[near_lod_check] non-black pixels present: PASS");

    println!("=== near_lod_check PASSED ===");
    println!(
        "[near_lod_check] summary: close_dist={close_dist}, neurons={}, synapses={}, \
         overflow_n={}, overflow_s={}, non_black={}/{total}",
        close_stats.emitted_neuron_instances,
        close_stats.emitted_synapse_instances,
        close_stats.neuron_overflow,
        close_stats.synapse_overflow,
        non_black,
    );
    println!(
        "[near_lod_check] FAR test: near-LOD skipped at dist={far_dist} \
         (counts 0 → no-op) — PASS"
    );
}

// ─── Camera helpers (same as render_check.rs) ────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn orbit_eye(azimuth: f32, elevation: f32, distance: f32) -> [f32; 3] {
    let cp = elevation.cos();
    [
        distance * cp * azimuth.sin(),
        distance * elevation.sin(),
        distance * cp * azimuth.cos(),
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn camera_mvp(azimuth: f32, elevation: f32, distance: f32, aspect: f32) -> [f32; 16] {
    let eye = orbit_eye(azimuth, elevation, distance);
    let center = [0.0f32; 3];
    let up = [0.0f32, 1.0, 0.0];
    let proj = perspective(50.0f32.to_radians(), aspect, 0.01, 100.0);
    let view = look_at(eye, center, up);
    mat4_mul(proj, view)
}

#[cfg(not(target_arch = "wasm32"))]
fn camera_vectors(azimuth: f32, elevation: f32) -> ([f32; 3], [f32; 3]) {
    let cp = elevation.cos();
    let eye = [cp * azimuth.sin(), elevation.sin(), cp * azimuth.cos()];
    let right = [azimuth.cos(), 0.0, -azimuth.sin()];
    let eye_norm = vec3_norm(eye);
    let cam_up = vec3_cross(right, [-eye_norm[0], -eye_norm[1], -eye_norm[2]]);
    (right, vec3_norm(cam_up))
}

#[cfg(not(target_arch = "wasm32"))]
fn vec3_norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

#[cfg(not(target_arch = "wasm32"))]
fn vec3_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fovy / 2.0).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        (far + near) * nf,
        -1.0,
        0.0,
        0.0,
        2.0 * far * near * nf,
        0.0,
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn look_at(eye: [f32; 3], center: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let z = vec3_norm([eye[0] - center[0], eye[1] - center[1], eye[2] - center[2]]);
    let x = vec3_norm(vec3_cross(up, z));
    let y = vec3_cross(z, x);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [
        x[0],
        y[0],
        z[0],
        0.0,
        x[1],
        y[1],
        z[1],
        0.0,
        x[2],
        y[2],
        z[2],
        0.0,
        -dot(x, eye),
        -dot(y, eye),
        -dot(z, eye),
        1.0,
    ]
}

#[cfg(not(target_arch = "wasm32"))]
fn mat4_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            out[c * 4 + r] = s;
        }
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
async fn readback_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let bytes_per_row = ((width * 4 + 255) / 256) * 256;
    let buffer_size = (bytes_per_row * height) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pixel-readback-near"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
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
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let row_start = (row * bytes_per_row) as usize;
        out.extend_from_slice(&data[row_start..row_start + (width * 4) as usize]);
    }
    drop(data);
    staging.unmap();
    out
}
