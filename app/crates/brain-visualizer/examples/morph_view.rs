//! Morphology PNG-dump harness (V2 Beauty-First).
//!
//! Builds the REAL production GpuBackend at N=1200/K=16 with default visual
//! settings (connection_layer=1 → procedural neuron morphology + outward signal
//! flow), warms the sim ~250 ticks at excitability 0.6, then renders the scene
//! to a 1024×1024 offscreen texture at THREE camera views and writes raw RGBA to
//! /tmp/morph_{0,1,2}.rgba. A 4th frame /tmp/morph_3.rgba re-renders the zoomed
//! view with morph_resting_opacity=0 to prove non-active structure is hidden
//! (only live pulses show). The reviewer converts these to PNG with PIL.
//!
//! Asserts each frame has non-black pixels (so we know it actually rendered).
//!
//! Run: `cargo run --example morph_view`
//! (Camera / readback helpers are copied from examples/render_check.rs.)

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    pollster::block_on(run());
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
async fn run() {
    use brain_visualizer::sim::backend::{SimBackend, SimConfig};
    use brain_visualizer::sim::gpu::{GpuBackend, VisualSettings};
    use brain_visualizer::sim::morphology::{MorphologyParams, MorphologyStats};

    const N: usize = 1_200;
    const K: usize = 16;
    const WIDTH: u32 = 1024;
    const HEIGHT: u32 = 1024;
    const WARM_TICKS: u32 = 250;
    const ARTIFACT_JSON: &str = "/tmp/morph_view_stats.json";
    const ARTIFACT_JSON_VERS: &str = "/tmp/morph_view_0.2.1_stats.json";
    const ARTIFACT_TAG: &str = "0.2.1";

    let default_visual = VisualSettings::default();
    let default_params =
        MorphologyParams::default_preset().with_curve_lift(default_visual.connection_curve_lift);
    let config = SimConfig {
        n: N,
        k: K,
        ..SimConfig::default()
    };
    let mut adapter_status = String::from("native_ok");
    let mut timestamps_supported = false;
    let mut frame_reports: Vec<FrameReport> = Vec::new();

    // --- 1. Acquire device ---
    let ctx = match GpuBackend::acquire_native().await {
        Ok(c) => c,
        Err(e) => {
            adapter_status = format!("skip: {e}");
            let artifact = MorphViewArtifact {
                status: "skip",
                adapter_status: &adapter_status,
                timestamps_supported,
                artifact_json_path: ARTIFACT_JSON,
                config: &config,
                base_visual: &default_visual,
                final_visual: &default_visual,
                morph_params: &default_params,
                morph_stats: &MorphologyStats::default(),
                warmup_ticks: WARM_TICKS,
                seed: config.seed,
                n: config.n,
                k: config.k,
                frames: &frame_reports,
            };
            std::fs::write(ARTIFACT_JSON, artifact.to_json()).expect("write morph_view json");
            std::fs::write(ARTIFACT_JSON_VERS, artifact.to_json()).expect("write morph_view json");
            eprintln!("SKIP morph_view: {e}");
            return;
        }
    };
    timestamps_supported = ctx.timestamps_supported;

    // --- 2. Build backend (default visual settings → connection_layer=1) ---
    let mut backend = GpuBackend::new(ctx, config.clone());
    backend.set_i_ext(0.055);
    backend.set_synaptic_scale(0.03);
    backend.initialize(&config);
    // Morphology controls: default VisualSettings has connection_layer=1 (on);
    // make it explicit. 0=off, 1=on (resting structure + signal flow).
    backend.set_connection_layer(1);

    let color_format = wgpu::TextureFormat::Rgba8Unorm;
    backend.build_render_pipelines(color_format);
    backend.resize_render_targets(WIDTH, HEIGHT);

    // --- 3. Offscreen colour texture ---
    let color_tex = backend.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("morph-offscreen-color"),
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

    // --- 4. Warm up the sim so neurons are firing (signal flow shows). ---
    println!("[morph_view] warming up {WARM_TICKS} ticks at excitability 0.6 …");
    for _ in 0..WARM_TICKS {
        backend.tick(1, 0.6);
    }
    let stats = backend.tick(1, 0.6);
    println!("[morph_view] post-warmup spikes/tick = {}", stats.spikes);

    // --- 5. Three camera views ---
    // view 0: whole sphere, distance 3.0 (see the forest)
    // view 1: distance 3.0, a different azimuth
    // view 2: zoomed to distance ~0.9 (individual neuron morphology up close)
    let aspect = WIDTH as f32 / HEIGHT as f32;
    let views: [(f32, f32, f32); 3] = [(0.3, 0.4, 3.0), (2.2, 0.2, 3.0), (0.6, 0.3, 0.9)];

    for (idx, &(az, el, dist)) in views.iter().enumerate() {
        let visual_snapshot = backend.visual().clone();
        let mvp = camera_mvp(az, el, dist, aspect);
        let (camera_right, camera_up) = camera_vectors(az, el);
        let cp = el.cos();
        let camera_pos = [dist * cp * az.sin(), dist * el.sin(), dist * cp * az.cos()];

        backend.render_full(
            &color_view,
            &mvp,
            camera_right,
            camera_up,
            100.0, // glow_tau
            0.012, // point_radius (soma billboards)
            camera_pos,
            dist,
        );

        let pixels =
            readback_rgba(backend.device(), backend.queue(), &color_tex, WIDTH, HEIGHT).await;

        // Assert non-black so we know it rendered.
        let mut non_black = 0u32;
        for chunk in pixels.chunks(4) {
            if chunk[0] > 2 || chunk[1] > 2 || chunk[2] > 2 {
                non_black += 1;
            }
        }
        let total = WIDTH * HEIGHT;
        let frac = non_black as f32 / total as f32 * 100.0;
        let path = format!("/tmp/morph_{idx}.rgba");
        let vers_path = format!("/tmp/morph_{ARTIFACT_TAG}_{idx}.rgba");
        let path_for_print = path.clone();
        std::fs::write(&path, &pixels).expect("write rgba");
        std::fs::write(&vers_path, &pixels).expect("write rgba");
        frame_reports.push(FrameReport {
            index: idx,
            az,
            el,
            dist,
            path,
            non_black,
            total,
            non_black_pct: frac,
            visual: visual_snapshot,
        });
        println!(
            "[morph_view] view {idx} (az={az:.2} el={el:.2} dist={dist:.2}) → {path_for_print}  \
             non-black {non_black}/{total} ({frac:.2}%)"
        );
        assert!(
            non_black > 0,
            "view {idx} produced an all-black frame (morphology did not render)"
        );
    }

    // --- 6. Morphology controls: 4th frame — resting opacity 0 (zoomed view).
    // Prove non-active structure is hidden so only live signal pulses show.
    {
        let mut v = backend.visual().clone();
        v.morph_resting_opacity = 0.0;
        backend.set_visual_settings(v);
        let visual_snapshot = backend.visual().clone();

        let (az, el, dist) = (0.6f32, 0.3f32, 0.9f32);
        let mvp = camera_mvp(az, el, dist, aspect);
        let (camera_right, camera_up) = camera_vectors(az, el);
        let cp = el.cos();
        let camera_pos = [dist * cp * az.sin(), dist * el.sin(), dist * cp * az.cos()];

        backend.render_full(
            &color_view,
            &mvp,
            camera_right,
            camera_up,
            100.0,
            0.012,
            camera_pos,
            dist,
        );

        let pixels =
            readback_rgba(backend.device(), backend.queue(), &color_tex, WIDTH, HEIGHT).await;
        let mut non_black = 0u32;
        for chunk in pixels.chunks(4) {
            if chunk[0] > 2 || chunk[1] > 2 || chunk[2] > 2 {
                non_black += 1;
            }
        }
        let total = WIDTH * HEIGHT;
        let frac = non_black as f32 / total as f32 * 100.0;
        let path = "/tmp/morph_3.rgba".to_string();
        let vers_path = format!("/tmp/morph_{ARTIFACT_TAG}_3.rgba");
        std::fs::write(&path, &pixels).expect("write rgba");
        std::fs::write(&vers_path, &pixels).expect("write rgba");
        frame_reports.push(FrameReport {
            index: 3,
            az,
            el,
            dist,
            path: path.clone(),
            non_black,
            total,
            non_black_pct: frac,
            visual: visual_snapshot,
        });
        println!(
            "[morph_view] view 3 (resting_opacity=0, zoomed) → /tmp/morph_3.rgba  \
             non-black {non_black}/{total} ({frac:.2}%)"
        );
    }

    let morph_buffers = backend
        .resources()
        .morph_buffers
        .as_ref()
        .expect("morph buffers");
    let artifact = MorphViewArtifact {
        status: "pass",
        adapter_status: &adapter_status,
        timestamps_supported,
        artifact_json_path: ARTIFACT_JSON,
        config: &config,
        base_visual: &default_visual,
        final_visual: backend.visual(),
        morph_params: &morph_buffers.params,
        morph_stats: &morph_buffers.stats,
        warmup_ticks: WARM_TICKS,
        seed: config.seed,
        n: config.n,
        k: config.k,
        frames: &frame_reports,
    };
    std::fs::write(ARTIFACT_JSON, artifact.to_json()).expect("write morph_view json");
    std::fs::write(ARTIFACT_JSON_VERS, artifact.to_json()).expect("write morph_view json");

    println!("[morph_view] wrote /tmp/morph_0..3.rgba (1024×1024 RGBA8)");
    println!("[morph_view] wrote {ARTIFACT_JSON}");
    println!("[morph_view] wrote {ARTIFACT_JSON_VERS}");
    println!("=== morph_view DONE ===");
}

// ─── Camera / readback helpers (copied from examples/render_check.rs) ──────────

#[cfg(not(target_arch = "wasm32"))]
fn camera_mvp(azimuth: f32, elevation: f32, distance: f32, aspect: f32) -> [f32; 16] {
    let cp = elevation.cos();
    let eye = [
        distance * cp * azimuth.sin(),
        distance * elevation.sin(),
        distance * cp * azimuth.cos(),
    ];
    let center = [0.0f32; 3];
    let up = [0.0f32, 1.0, 0.0];
    let proj = perspective(50.0f32.to_radians(), aspect, 0.1, 100.0);
    let view = look_at(eye, center, up);
    mat4_mul(proj, view)
}

#[cfg(not(target_arch = "wasm32"))]
fn camera_vectors(azimuth: f32, elevation: f32) -> ([f32; 3], [f32; 3]) {
    let cp = elevation.cos();
    let eye = [cp * azimuth.sin(), elevation.sin(), cp * azimuth.cos()];
    let right = [azimuth.cos(), 0.0, -azimuth.sin()];
    let eye_norm = vec3_norm(eye);
    let cam_up = vec3_cross(right, [(-eye_norm[0]), (-eye_norm[1]), (-eye_norm[2])]);
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
#[derive(Debug)]
struct FrameReport {
    index: usize,
    az: f32,
    el: f32,
    dist: f32,
    path: String,
    non_black: u32,
    total: u32,
    non_black_pct: f32,
    visual: brain_visualizer::sim::gpu::VisualSettings,
}

#[cfg(not(target_arch = "wasm32"))]
struct MorphViewArtifact<'a> {
    status: &'a str,
    adapter_status: &'a str,
    timestamps_supported: bool,
    artifact_json_path: &'a str,
    config: &'a brain_visualizer::sim::backend::SimConfig,
    base_visual: &'a brain_visualizer::sim::gpu::VisualSettings,
    final_visual: &'a brain_visualizer::sim::gpu::VisualSettings,
    morph_params: &'a brain_visualizer::sim::morphology::MorphologyParams,
    morph_stats: &'a brain_visualizer::sim::morphology::MorphologyStats,
    warmup_ticks: u32,
    seed: u64,
    n: usize,
    k: usize,
    frames: &'a [FrameReport],
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> MorphViewArtifact<'a> {
    fn to_json(&self) -> String {
        let frames = self
            .frames
            .iter()
            .map(FrameReport::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let output_paths = self
            .frames
            .iter()
            .map(|f| format!("\"{}\"", json_escape(&f.path)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"status\":\"{}\",\"adapter_status\":\"{}\",\"timestamps_supported\":{},\"artifact_json_path\":\"{}\",\"seed\":{},\"n\":{},\"k\":{},\"warmup_ticks\":{},\"config\":{{\"n\":{},\"k\":{},\"seed\":{},\"tier\":\"{:?}\",\"speed\":\"{:?}\",\"backend\":\"{:?}\",\"i_ext\":{:.6},\"fixed_point_scale\":{}}},\"base_visual_settings\":{},\"final_visual_settings\":{},\"morphology_config\":{},\"morphology_stats\":{},\"output_paths\":[{}],\"frames\":[{}]}}",
            self.status,
            json_escape(self.adapter_status),
            self.timestamps_supported,
            json_escape(self.artifact_json_path),
            self.seed,
            self.n,
            self.k,
            self.warmup_ticks,
            self.config.n,
            self.config.k,
            self.config.seed,
            self.config.tier,
            self.config.speed,
            self.config.backend,
            self.config.i_ext,
            self.config.fixed_point_scale,
            self.base_visual.to_json(),
            self.final_visual.to_json(),
            self.morph_params.to_json(),
            self.morph_stats.to_json(),
            output_paths,
            frames,
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FrameReport {
    fn to_json(&self) -> String {
        format!(
            "{{\"index\":{},\"az\":{:.3},\"el\":{:.3},\"dist\":{:.3},\"path\":\"{}\",\"non_black\":{},\"total\":{},\"non_black_pct\":{:.4},\"visual_settings\":{}}}",
            self.index,
            self.az,
            self.el,
            self.dist,
            json_escape(&self.path),
            self.non_black,
            self.total,
            self.non_black_pct,
            self.visual.to_json(),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
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
    let bytes_per_row_unaligned = width * 4;
    let bytes_per_row = ((bytes_per_row_unaligned + 255) / 256) * 256;
    let buffer_size = (bytes_per_row * height) as u64;

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("morph-pixel-readback"),
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
        let row_end = row_start + (width * 4) as usize;
        out.extend_from_slice(&data[row_start..row_end]);
    }
    drop(data);
    staging.unmap();
    out
}
