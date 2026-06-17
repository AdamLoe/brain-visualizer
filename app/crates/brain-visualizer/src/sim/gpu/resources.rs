//! GPU resource ownership boundary (architecture §5 "frame graph and resource
//! lifecycle"). Phase 2 allocates the real storage buffers, builds the bind
//! group layouts + bind groups, and uploads the initial silent-start state.
//! The rAF loop must never recreate buffers/bind groups/targets. Only the rare
//! structural-change methods here allocate.

use crate::buffers::{ChunkLayout, ChunkedBuffer, MAX_CHUNK_BYTES};
use crate::connectivity::spatial::SpatialGrid;
use crate::manifold::RegionKind;
use crate::sim::backend::{initial_last_spike, SimConfig};
use wgpu::util::DeviceExt;

/// The per-neuron SoA storage buffers (chunked). Phase 2 allocates the device
/// buffers; for N ≤ 16M each field is a single chunk.
pub struct NeuronBuffers {
    pub pos_x: ChunkedBuffer,
    pub pos_y: ChunkedBuffer,
    pub pos_z: ChunkedBuffer,
    pub v: ChunkedBuffer,
    /// Accumulated input current (fixed-point i32). Double-buffered with
    /// `i_current_next`; the integrate pass reads the "front" buffer and the
    /// scatter pass writes the "back" buffer. The two are flipped each tick.
    pub i_current: ChunkedBuffer,
    pub i_current_next: ChunkedBuffer,
    /// Packed valid/type/tick (BV21).
    pub last_spike: ChunkedBuffer,
}

impl NeuronBuffers {
    /// Build the chunked *layouts* for `n` neurons (each field is 4 bytes/elem).
    pub fn new(n: usize) -> Self {
        Self {
            pos_x: ChunkedBuffer::new(n, 4),
            pos_y: ChunkedBuffer::new(n, 4),
            pos_z: ChunkedBuffer::new(n, 4),
            v: ChunkedBuffer::new(n, 4),
            i_current: ChunkedBuffer::new(n, 4),
            i_current_next: ChunkedBuffer::new(n, 4),
            last_spike: ChunkedBuffer::new(n, 4),
        }
    }
}

/// Spatial grid (CSR) buffers shared by the scatter pass — uploaded once per
/// resize (geometry is static).
pub struct GridBuffers {
    pub cell_of_neuron: wgpu::Buffer,
    pub cell_start: wgpu::Buffer,
    pub cell_neurons: wgpu::Buffer,
    pub grid_dim: u32,
}

/// Per-tick sim scratch buffers (spike list, counters, indirect dispatch args).
pub struct SimBuffers {
    pub spike_list: wgpu::Buffer,
    pub spike_count: wgpu::Buffer,
    pub dispatch_args: wgpu::Buffer,
    pub max_abs_current: wgpu::Buffer,
    /// Staging buffer for async stats readback (spike_count + max_abs_current).
    pub stats_staging: wgpu::Buffer,
    pub integrate_uniform: wgpu::Buffer,
    pub connect_uniform: wgpu::Buffer,
    // ─── V2 Phase A: metrics reduction ────────────────────────────────────────
    /// Metrics reduction output (METRICS_SLOT_COUNT × u32). STORAGE | COPY_SRC |
    /// COPY_DST (zeroed via write_buffer before each reduce pass).
    pub metrics_buf: wgpu::Buffer,
    /// Staging buffer for non-blocking metrics readback (MAP_READ | COPY_DST).
    pub metrics_staging: wgpu::Buffer,
    /// Metrics reduction uniform (current_tick, n, voltage range, …).
    pub metrics_uniform: wgpu::Buffer,
}

// ─── Morphology: procedural neuron geometry render pipeline ───────────────────

pub use crate::sim::morphology::{MorphSegment, MorphSphereInstance};

/// Morphology: per-frame render uniform — layout MUST match `MorphUniforms` in
/// render_morphology.wgsl verbatim. 192 B total (mat4=64 + 8×16).
///
/// Byte map (offsets from struct start):
///   0:   mvp: [f32;16]                  (64 B)
///  64:   camera_right:[f32;3] + tick:u32 (16 B)
///  80:   camera_up:[f32;3] + width_scale:f32 (16 B)
///  96:   camera_pos:[f32;3] + light_next:u32 (16 B)
/// 112:   light_past:u32 + glow_tau:f32 + base_brightness:f32 + connection_layer:u32 (16 B)
/// 128:   color_by:u32 + _pad_a:u32 + _pad_b:u32 + _pad_c:u32 (16 B)
/// 144:   light_dir:[f32;3] + ambient:f32 (16 B)
/// 160:   diffuse_intensity:f32 + rim_intensity:f32 + rim_power:f32 + _pad3:u32 (16 B)
/// 176:   resting_brightness:f32 + active_boost:f32 + active_opacity:f32 + inactive_opacity_floor:f32 (16 B)
/// Total = 192 B
///
/// v0.3.1: `resting_brightness` and `active_boost` are owned by the morphology
/// config (`set_morphology_config`), NOT by the VisualSettings Float32Array.
/// `resting_brightness` is the morph-config-owned resting structure brightness;
/// `active_boost` replaces the former hardcoded WGSL `const BOOST = 1.8`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MorphUniforms {
    pub mvp: [f32; 16],
    pub camera_right: [f32; 3],
    pub tick: u32,
    pub camera_up: [f32; 3],
    pub width_scale: f32,
    pub camera_pos: [f32; 3],
    // Morphology: whole-connection lighting toggles (0/1) — light a firing
    // neuron's downstream (next) and/or upstream (past) axon connections.
    pub light_next: u32,
    pub light_past: u32,
    // Morphology: glow decay constant (ticks) — the lit connection fades with the
    // SAME exp(-tick_diff/glow_tau) curve as the far-glow neuron dot.
    pub glow_tau: f32,
    pub base_brightness: f32,
    pub connection_layer: u32,
    pub color_by: u32,
    pub _pad_a: u32, // pad block to 16-B boundary before light_dir
    pub _pad_b: u32,
    pub _pad_c: u32,
    // ── Lighting preset (Stage 0 / v0.3.0) ────────────────────────────────────
    // Defaults locked here; dev-panel exposure in v0.3.1.
    pub light_dir: [f32; 3], // world-space directional light direction (normalised)
    pub ambient: f32,        // ambient term (fills the vec3's 16-B slot)
    pub diffuse_intensity: f32,
    pub rim_intensity: f32,
    pub rim_power: f32,
    pub _pad3: u32, // pad block to 16-B boundary before the brightness split
    // ── Active/resting brightness split (v0.3.1, morph-config owned) ───────────
    // resting_brightness: resting structure brightness (config-owned; supersedes
    // the Float32Array morph_resting_opacity as the morph-config source).
    // active_boost: multiplier on the lit (spiking) contribution — replaces the
    // former hardcoded WGSL `const BOOST = 1.8`.
    pub resting_brightness: f32,
    pub active_boost: f32,
    // ── True-opacity active layer (active-opacity-render-pass) ─────────────────
    // Repurposed from the former trailing reserved pads (_pad4/_pad5 → f32). Read
    // only by the NEW depth-tested fs_main_active / fs_sphere_active entry points;
    // the additive resting passes share this buffer and ignore them. Size is
    // unchanged (two u32→f32 in place), so the 192 B asserts stay green.
    pub active_opacity: f32,         // active coverage/emphasis (was _pad4)
    pub inactive_opacity_floor: f32, // inactive coverage floor (was _pad5; pads to 192 B)
}

/// Morphology: active/recent compaction uniform — layout MUST match
/// `CompactUniforms` in compact_morph_segments.wgsl. 32 B, 16-aligned.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CompactUniforms {
    pub tick: u32,
    pub segment_count: u32,
    pub glow_tau: f32,
    pub connection_layer: u32,
    pub light_next: u32,
    pub light_past: u32,
    pub tube_verts: u32,
    pub _pad: u32,
}

/// Derive the per-binding segment layout for morphology chunks.
pub fn morph_segment_chunk_layout(
    total_segments: usize,
    max_storage_buffer_binding_size: usize,
) -> ChunkLayout {
    let element_bytes = std::mem::size_of::<MorphSegment>();
    let chunk_budget = MAX_CHUNK_BYTES.min(max_storage_buffer_binding_size);
    ChunkLayout::with_budget(total_segments, element_bytes, chunk_budget)
}

/// Morphology: one segment chunk and its chunk-local compaction/draw resources.
/// Allocated once per network rebuild; the frame loop only writes uniforms and
/// dispatches/draws against these existing bindings.
pub struct MorphSegmentChunk {
    /// Chunk-local MorphSegments (read-only storage in the morphology VS).
    pub segment_buffer: wgpu::Buffer,
    /// Number of segments in this chunk.
    pub segment_count: u32,
    /// Bytes bound for this chunk's segment storage buffer.
    pub segment_bytes: u64,
    /// Compacted instance→chunk-local segment index list (u32). Hard cap =
    /// segment_count, so the compact pass cannot overflow.
    pub active_segment_indices: wgpu::Buffer,
    /// Atomic selected-segment counter (u32), zeroed each frame by reset.
    pub active_segment_count: wgpu::Buffer,
    /// DrawIndirectArgs (non-indexed): vertex_count, instance_count,
    /// first_vertex, first_instance = 4 × u32.
    pub active_draw_args: wgpu::Buffer,
    /// Compaction compute uniform (uploaded each frame the layer is on).
    pub compact_uniform: wgpu::Buffer,
    /// Profiler: last selected count for this chunk.
    pub active_selected: wgpu::Buffer,
    /// Staging buffer for diagnostic selected-count readback.
    pub selected_staging: wgpu::Buffer,
}

/// Morphology: GPU buffers. Allocated ONCE per network (re)build in
/// `init_morph_resources`; never per-frame. Segment storage is chunked so every
/// storage binding stays below adapter limits while soma spheres remain flat.
pub struct MorphBuffers {
    /// Chunked MorphSegment storage and chunk-local active/recent draw state.
    pub segment_chunks: Vec<MorphSegmentChunk>,
    /// Layout used to split the generated flat segment list.
    pub segment_layout: ChunkLayout,
    /// Number of segments actually generated across all chunks.
    pub segment_count: u32,
    /// Per-frame morphology render uniform (shared by tube and soma-sphere pass).
    pub morph_uniform: wgpu::Buffer,
    /// Soma sphere instances — one per neuron (Wave 2 / Stream 2).
    pub sphere_buffer: wgpu::Buffer,
    /// Number of soma sphere instances (= neuron count).
    pub sphere_count: u32,
    /// Generation parameters used to build this buffer (for artifact capture).
    pub params: crate::sim::morphology::MorphologyParams,
    /// Build-time stats for this buffer (for artifact capture).
    pub stats: crate::sim::morphology::MorphologyStats,
}

/// V2 Phase A: number of u32 slots in the metrics reduction buffer. Slots
/// 0..=11 are scalar accumulators; 16..=31 are the 16-bin voltage histogram.
/// (Slots 12..=15 reserved.) Layout documented in metrics.wgsl.
pub const METRICS_SLOT_COUNT: usize = 32;

/// V2 Phase A: metrics reduction uniform — layout must match `MetricsUniforms`
/// in metrics.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MetricsUniforms {
    pub current_tick: u32,
    pub n: u32,
    pub volt_lo: f32,
    pub volt_hi: f32,
    pub volt_scale: f32,
    pub histo_bins: u32,
    pub _pad: [u32; 2], // pad to 32 B (16-B alignment for UBO)
}

/// Color / depth / HDR render targets. Phase 3: real depth texture + dimensions.
/// V2 Phase E: optional HDR scene + half-res ping-pong textures for bloom. These
/// are only allocated/used when bloom_strength > 0; the default path renders
/// directly to the surface and never touches them.
pub struct RenderTargets {
    pub width: u32,
    pub height: u32,
    /// Depth texture for the manifold mesh pass (depth-test before glow).
    pub depth_texture: Option<wgpu::Texture>,
    pub depth_view: Option<wgpu::TextureView>,
    // ─── V2 Phase E: bloom offscreen targets (rgba16float) ───────────────────
    /// Full-res HDR scene target (the bloom path renders the scene here).
    pub hdr_texture: Option<wgpu::Texture>,
    pub hdr_view: Option<wgpu::TextureView>,
    /// Half-res ping-pong pair for the separable Gaussian blur.
    pub bloom_a_texture: Option<wgpu::Texture>,
    pub bloom_a_view: Option<wgpu::TextureView>,
    pub bloom_b_texture: Option<wgpu::Texture>,
    pub bloom_b_view: Option<wgpu::TextureView>,
    /// Half-res dims (for the blur passes' inverse-texel-size uniform).
    pub bloom_width: u32,
    pub bloom_height: u32,
}

/// Render-pass GPU resources (Phase 3).
/// Created once per resize; never per frame.
pub struct RenderResources {
    /// Static manifold mesh vertex buffer (vec3 positions).
    pub manifold_vb: wgpu::Buffer,
    /// Static manifold mesh index buffer (u32 triangle indices).
    pub manifold_ib: wgpu::Buffer,
    /// Index count for the manifold draw call.
    pub manifold_index_count: u32,
    /// Uniform buffer: render uniforms (mvp, camera_right, camera_up, tick, …).
    pub render_uniform: wgpu::Buffer,
    /// Uniform buffer: manifold pass MVP (mat4x4 only).
    pub manifold_uniform: wgpu::Buffer,
    /// Stimulation uniform buffer (pos, radius, current_fp, active).
    pub stim_uniform: wgpu::Buffer,
    /// Grid uniform buffer for stimulate pass (grid_dim, n).
    pub stim_grid_uniform: wgpu::Buffer,
    // ─── V2 Phase E: bloom resources (created once; used only when bloom on) ──
    /// Linear-clamp sampler for the bloom fullscreen passes.
    pub bloom_sampler: wgpu::Sampler,
    /// Bloom uniform buffers — one per pass (values differ: blur direction, etc.).
    pub bloom_bright_uniform: wgpu::Buffer,
    pub bloom_blur_h_uniform: wgpu::Buffer,
    pub bloom_blur_v_uniform: wgpu::Buffer,
    pub bloom_composite_uniform: wgpu::Buffer,
}

/// Stimulation state written each frame from the JS/native caller.
/// Field names match `StimUniforms` in stimulate.wgsl (active → is_active).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StimUniform {
    pub pos: [f32; 3],
    pub radius: f32,
    pub current_fp: i32,
    pub is_active: u32,
    pub _pad: [u32; 2],
}

/// Render far-LOD uniform — layout must match `Uniforms` in render_far.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RenderUniforms {
    pub mvp: [f32; 16],
    pub camera_right: [f32; 3],
    pub _pad0: f32,
    pub camera_up: [f32; 3],
    pub _pad1: f32,
    pub tick: u32,
    pub glow_tau: f32,
    pub point_radius: f32,
    pub n: u32,
    pub camera_pos: [f32; 3],
    /// V2 Phase B: debug subthreshold-voltage glow strength (0.0 = off, the
    /// default — reproduces pre-V2 look). Higher = resting neurons glow by |v|.
    pub voltage_glow_strength: f32,
    // ─── V2 Phase E: orthogonal color/visibility/radius controls ─────────────
    // New 16-byte block (offset 128). Field order MUST match `Uniforms` in
    // render_far.wgsl verbatim (#1 corruption source — do not reorder).
    /// color_by mode: 0=region,1=E/I,2=spike-age,3=voltage,4=activity,5=identity,6=brain (default 6).
    pub color_by: u32,
    /// neuron_visibility: 0=all,1=active-emphasis,2=active-only (default 0).
    pub neuron_visibility: u32,
    /// base neuron radius in world units (replaces point_radius; default 0.004).
    pub neuron_visual_radius: f32,
    /// radius multiplier when fully active (default 2.0 → +100% on full glow).
    pub active_neuron_radius_boost: f32,
    // New 16-byte block (offset 144).
    /// opacity multiplier for inactive (low-glow) neurons (default 1.0).
    pub inactive_neuron_opacity: f32,
    pub _pad2: f32,
    pub _pad3: f32,
    pub _pad4: f32,
}

/// Manifold-pass uniform — MVP matrix + V2 Phase E surface controls.
/// Layout MUST match `Uniforms` in render_manifold.wgsl (mvp, then a 16-byte
/// block: opacity, mode, color_by, pad).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ManifoldUniforms {
    pub mvp: [f32; 16],
    /// V2 Phase E: surface opacity [0,1] (settings index 11; default 1.0).
    pub surface_opacity: f32,
    /// V2 Phase E: surface mode (1=dim, 2=normal). Never 0 here (0 ⇒ pass skipped).
    pub surface_mode: u32,
    /// color_by mode, used by the optional surface pass for Brain mode tinting.
    pub color_by: u32,
    pub _pad1: u32,
}

/// V2 Phase E: bloom post-process uniform — layout MUST match `BloomUniforms`
/// in bloom.wgsl. 32 bytes, 16-aligned.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BloomUniforms {
    pub inv_texel: [f32; 2],
    pub direction: [f32; 2],
    pub threshold: f32,
    pub bloom_strength: f32,
    pub exposure: f32,
    pub _pad: f32,
}

/// Bind-group layouts shared by pipelines (phase 2 real handles + phase 3 render
/// + morphology render passes).
pub struct GpuLayouts {
    pub integrate_bgl: wgpu::BindGroupLayout,
    pub integrate_uniform_bgl: wgpu::BindGroupLayout,
    pub write_dispatch_bgl: wgpu::BindGroupLayout,
    pub scatter_bgl: wgpu::BindGroupLayout,
    pub connect_uniform_bgl: wgpu::BindGroupLayout,
    /// Phase 3: render far-LOD bind-group layout
    /// group(0): uniform + 5 storage (pos_x/y/z, last_spike, v).
    pub render_far_bgl: wgpu::BindGroupLayout,
    /// Phase 3: manifold mesh bind-group layout (uniform only).
    pub render_manifold_bgl: wgpu::BindGroupLayout,
    /// Phase 3: stimulate compute bind-group layout.
    pub stimulate_bgl: wgpu::BindGroupLayout,
    // ─── V2 Phase A ───────────────────────────────────────────────────────────
    /// Metrics reduction group 0: last_spike(read) + v(read) + metrics_buf(rw).
    pub metrics_bgl: wgpu::BindGroupLayout,
    /// Metrics reduction group 1: metrics uniform.
    pub metrics_uniform_bgl: wgpu::BindGroupLayout,
    // ─── Morphology ───────────────────────────────────────────────────────────
    /// morphology render group 0: one segment chunk (read) + last_spike (read) +
    /// morph uniform + chunk-local active_segment_indices (read, binding 6).
    pub render_morphology_bgl: wgpu::BindGroupLayout,
    /// morphology active/recent compaction compute group 0: segments(r) +
    /// last_spike(r) + compact uniform + active_indices(rw) + active_count(rw) +
    /// draw_args(rw) + selected_count(rw).
    pub compact_morph_bgl: wgpu::BindGroupLayout,
    /// soma sphere render group 0: sphere_instances (read) + last_spike (read) +
    /// morph uniform (shared with tube pass for same lighting/mvp).
    pub render_soma_spheres_bgl: wgpu::BindGroupLayout,
    // ─── V2 Phase E: bloom post-process layouts ───────────────────────────────
    /// bright/blur group 0: sampler(0) + input tex(1) + bloom uniform(2).
    pub bloom_pass_bgl: wgpu::BindGroupLayout,
    /// composite group 0: sampler(0) + scene tex(1) + bloom uniform(2) + bloom tex(3).
    pub bloom_composite_bgl: wgpu::BindGroupLayout,
}

impl GpuLayouts {
    pub fn new(device: &wgpu::Device) -> Self {
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        // integrate group 0: v, last_spike, I, spike_list, spike_count (all rw).
        let integrate_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("integrate-bgl"),
            entries: &[
                storage(0, false),
                storage(1, false),
                storage(2, false),
                storage(3, false),
                storage(4, false),
            ],
        });
        let integrate_uniform_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("integrate-uniform-bgl"),
                entries: &[uniform(0)],
            });

        // write_dispatch group 0: spike_count (read), dispatch_args (rw).
        let write_dispatch_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("write-dispatch-bgl"),
                entries: &[storage(0, true), storage(1, false)],
            });

        // scatter group 0: spike_list(r), spike_count(r), I_next(rw),
        // last_spike(r), cell_of_neuron(r), cell_start(r), cell_neurons(r),
        // max_abs_current(rw).
        let scatter_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scatter-bgl"),
            entries: &[
                storage(0, true),
                storage(1, true),
                storage(2, false),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, true),
                storage(7, false),
            ],
        });
        let connect_uniform_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("connect-uniform-bgl"),
                entries: &[uniform(0)],
            });

        // Phase 3: render far-LOD bind-group layout.
        // group(0) binding 0 = uniform (RenderUniforms),
        //          bindings 1-5 = storage read-only (pos_x, pos_y, pos_z, last_spike, v).
        let render_vs_storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let render_vs_uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let render_far_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render-far-bgl"),
            entries: &[
                render_vs_uniform(0),
                render_vs_storage(1),
                render_vs_storage(2),
                render_vs_storage(3),
                render_vs_storage(4),
                render_vs_storage(5),
            ],
        });

        // Manifold mesh layout: just the uniform buffer (MVP).
        let render_manifold_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("render-manifold-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // V2 Phase E: fragment now reads surface_opacity/mode too.
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Stimulate compute layout: 2 uniforms + 5 read-only storages + 1 read-write.
        let stim_uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let stim_storage_ro = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let stim_storage_rw = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let stimulate_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stimulate-bgl"),
            entries: &[
                stim_uniform_entry(0), // stim uniforms
                stim_uniform_entry(1), // grid uniforms
                stim_storage_ro(2),    // pos_x
                stim_storage_ro(3),    // pos_y
                stim_storage_ro(4),    // pos_z
                stim_storage_ro(5),    // cell_of_neuron (unused by shader but included for layout)
                stim_storage_ro(6),    // cell_start
                stim_storage_ro(7),    // cell_neurons
                stim_storage_rw(8),    // i_current (atomic write)
            ],
        });

        // ─── V2 Phase A: metrics reduction layouts ───────────────────────────
        // group 0: last_spike(read) + v(read) + metrics_buf(rw atomic).
        let metrics_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("metrics-bgl"),
            entries: &[
                storage(0, true),  // last_spike
                storage(1, true),  // v
                storage(2, false), // metrics_buf (atomic rw)
            ],
        });
        // group 1: metrics uniform.
        let metrics_uniform_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("metrics-uniform-bgl"),
                entries: &[uniform(0)],
            });

        // ─── Morphology: render layout ────────────────────────────────────────
        // group 0: one segment chunk (read storage, VS) + last_spike (read
        // storage, VS) + morph uniform (VS).
        let render_morphology_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("render-morphology-bgl"),
                entries: &[
                    render_vs_storage(0), // segment_buffer
                    render_vs_storage(1), // last_spike
                    render_vs_uniform(2), // MorphUniforms
                    render_vs_storage(6), // active_segment_indices (compacted instance map)
                ],
            });
        // Active/recent compaction compute layout (binding slots match
        // compact_morph_segments.wgsl group 0).
        let compact_morph_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compact-morph-bgl"),
            entries: &[
                storage(0, true),  // segments (read)
                storage(1, true),  // last_spike (read)
                uniform(2),        // CompactUniforms
                storage(3, false), // active_segment_indices (rw)
                storage(4, false), // active_segment_count (atomic rw)
                storage(5, false), // active_draw_args (rw)
                storage(6, false), // selected_count (atomic rw, profiler)
            ],
        });
        // Soma sphere render layout (Wave 2). Uses binding slots 3/4/5 to avoid
        // a WGSL name clash with the tube bindings (0/1/2) in the same shader
        // module. vs_sphere/fs_sphere only touch 3/4/5; vs_main/fs_main only
        // touch 0/1/2. WebGPU validates only reachable bindings per entry point.
        let render_soma_spheres_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("render-soma-spheres-bgl"),
                entries: &[
                    render_vs_storage(3), // sphere_instances
                    render_vs_storage(4), // last_spike (same buffer, slot 4)
                    render_vs_uniform(5), // MorphUniforms (same buffer, slot 5)
                ],
            });

        // ─── V2 Phase E: bloom layouts ────────────────────────────────────────
        let frag_sampler = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let frag_tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let frag_uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bloom_pass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom-pass-bgl"),
            entries: &[frag_sampler(0), frag_tex(1), frag_uniform(2)],
        });
        let bloom_composite_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bloom-composite-bgl"),
                entries: &[frag_sampler(0), frag_tex(1), frag_uniform(2), frag_tex(3)],
            });

        Self {
            integrate_bgl,
            integrate_uniform_bgl,
            write_dispatch_bgl,
            scatter_bgl,
            connect_uniform_bgl,
            render_far_bgl,
            render_manifold_bgl,
            stimulate_bgl,
            // V2 Phase A
            metrics_bgl,
            metrics_uniform_bgl,
            // Morphology
            render_morphology_bgl,
            compact_morph_bgl,
            render_soma_spheres_bgl,
            // V2 Phase E
            bloom_pass_bgl,
            bloom_composite_bgl,
        }
    }
}

/// Bind groups for the per-tick passes. Two scatter/integrate bind-group
/// variants alternate so I / I_next double-buffer with a pointer flip (no
/// realloc): on even ticks integrate reads `i_current` and scatter writes
/// `i_current_next`; on odd ticks they swap.
pub struct GpuBindGroups {
    pub integrate: [wgpu::BindGroup; 2],
    pub integrate_uniform: wgpu::BindGroup,
    pub write_dispatch: wgpu::BindGroup,
    pub scatter: [wgpu::BindGroup; 2],
    pub connect_uniform: wgpu::BindGroup,
    /// Phase 3: render far-LOD bind group (pos_x/y/z, last_spike, v read-only).
    /// None until `init_render_resources` has been called.
    pub render_far: Option<wgpu::BindGroup>,
    /// Phase 3: manifold mesh bind group (MVP uniform only).
    pub render_manifold: Option<wgpu::BindGroup>,
    /// Phase 3: stimulate compute bind groups — two variants for I/I_next parity.
    pub stimulate: Option<[wgpu::BindGroup; 2]>,
    // ─── V2 Phase A ───────────────────────────────────────────────────────────
    /// Metrics reduction group 0 (last_spike + v + metrics_buf).
    pub metrics: wgpu::BindGroup,
    /// Metrics reduction group 1 (metrics uniform).
    pub metrics_uniform: wgpu::BindGroup,
    // ─── Morphology ───────────────────────────────────────────────────────────
    /// Per-segment-chunk morphology tube render and compaction bind groups.
    /// Empty until both morph buffers + neuron buffers are allocated.
    pub morph_segment_chunks: Vec<MorphSegmentBindGroups>,
    /// soma sphere render group 0 (sphere_instances + last_spike + morph uniform).
    /// None until both sphere buffers + neuron buffers are allocated.
    pub render_soma_spheres: Option<wgpu::BindGroup>,
}

pub struct MorphSegmentBindGroups {
    pub render_morphology: wgpu::BindGroup,
    pub compact_morph: wgpu::BindGroup,
}

/// Owns all GPU buffers/targets and tracks when bind groups must be rebuilt.
pub struct GpuResources {
    pub neuron_buffers: Option<NeuronBuffers>,
    pub grid_buffers: Option<GridBuffers>,
    pub sim_buffers: Option<SimBuffers>,
    pub bind_groups: Option<GpuBindGroups>,
    pub render_targets: Option<RenderTargets>,
    /// Phase 3: render-pass resources (manifold mesh + uniform buffers).
    pub render_resources: Option<RenderResources>,
    /// Morphology: per-network procedural neuron geometry buffers.
    pub morph_buffers: Option<MorphBuffers>,
    /// Set whenever a buffer/texture is recreated; cleared by
    /// `refresh_bind_groups`. The frame loop checks this before encoding.
    pub bind_groups_dirty: bool,
}

impl Default for GpuResources {
    fn default() -> Self {
        Self {
            neuron_buffers: None,
            grid_buffers: None,
            sim_buffers: None,
            bind_groups: None,
            render_targets: None,
            render_resources: None,
            morph_buffers: None,
            bind_groups_dirty: false,
        }
    }
}

impl GpuResources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recreate neuron + sim + grid buffers for a new network size, upload the
    /// silent-start state, then mark bind groups dirty. Rare-path (resize / tier
    /// change / restart); allocation is allowed here only.
    pub fn resize_neurons(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &SimConfig,
        positions: &[[f32; 3]],
        regions: &[RegionKind],
        grid: &SpatialGrid,
    ) {
        let n = config.n;
        let mut nb = NeuronBuffers::new(n);

        // --- per-neuron initial state ---
        let mut pos_x = vec![0f32; n];
        let mut pos_y = vec![0f32; n];
        let mut pos_z = vec![0f32; n];
        let mut last_spike = vec![0u32; n];
        let seed_lo = config.seed_lo();
        for i in 0..n {
            let p = positions[i];
            pos_x[i] = p[0];
            pos_y[i] = p[1];
            pos_z[i] = p[2];
            last_spike[i] = initial_last_spike(i as u32, seed_lo, regions[i]);
        }
        let v_zero = vec![0f32; n];
        let i_zero = vec![0i32; n];

        let st_init = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        alloc_field(
            device,
            &mut nb.pos_x,
            bytemuck::cast_slice(&pos_x),
            st_init,
            "pos_x",
        );
        alloc_field(
            device,
            &mut nb.pos_y,
            bytemuck::cast_slice(&pos_y),
            st_init,
            "pos_y",
        );
        alloc_field(
            device,
            &mut nb.pos_z,
            bytemuck::cast_slice(&pos_z),
            st_init,
            "pos_z",
        );
        alloc_field(
            device,
            &mut nb.v,
            bytemuck::cast_slice(&v_zero),
            st_init,
            "v",
        );
        alloc_field(
            device,
            &mut nb.i_current,
            bytemuck::cast_slice(&i_zero),
            st_init,
            "i_current",
        );
        alloc_field(
            device,
            &mut nb.i_current_next,
            bytemuck::cast_slice(&i_zero),
            st_init,
            "i_current_next",
        );
        alloc_field(
            device,
            &mut nb.last_spike,
            bytemuck::cast_slice(&last_spike),
            st_init,
            "last_spike",
        );
        self.neuron_buffers = Some(nb);

        // --- spatial grid (CSR) buffers ---
        let cell_of_neuron = grid.cell_of_neuron_map();
        self.grid_buffers = Some(GridBuffers {
            cell_of_neuron: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cell_of_neuron"),
                contents: bytemuck::cast_slice(&cell_of_neuron),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            cell_start: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cell_start"),
                contents: bytemuck::cast_slice(&grid.cell_start),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            cell_neurons: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cell_neurons"),
                contents: bytemuck::cast_slice(&grid.cell_neurons),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            grid_dim: grid.dim,
        });

        // --- sim scratch buffers ---
        // spike_list holds up to N ids (worst case: every neuron fires).
        let spike_list = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spike_list"),
            size: (n.max(1) * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let spike_count = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spike_count"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dispatch_args = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dispatch_args"),
            size: 12, // 3 x u32 (DispatchIndirectArgs)
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT,
            mapped_at_creation: false,
        });
        let max_abs_current = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("max_abs_current"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // stats staging: [spike_count, max_abs_current] = 2 x u32.
        let stats_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stats_staging"),
            size: 8,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let integrate_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("integrate_uniform"),
            size: std::mem::size_of::<IntegrateUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let connect_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("connect_uniform"),
            size: std::mem::size_of::<ConnectUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // ConnectUniforms is static for the run; write it once here.
        queue.write_buffer(
            &connect_uniform,
            0,
            bytemuck::bytes_of(&ConnectUniforms {
                n: config.n as u32,
                k: config.k as u32,
                fixed_point_scale: config.fixed_point_scale as f32,
                seed_lo,
                grid_dim: grid.dim,
                // Default-off heavy-tailed reach (== LOCAL_ONLY); the GPU
                // dev-panel knob re-writes this buffer via set_visual_settings.
                long_range_frac: crate::connectivity::ReachParams::LOCAL_ONLY.long_range_frac,
                max_reach: crate::connectivity::ReachParams::LOCAL_ONLY.max_reach,
                _pad: [0; 1],
            }),
        );

        // ─── V2 Phase A: metrics reduction buffers ────────────────────────────
        let metrics_bytes = (METRICS_SLOT_COUNT * 4) as u64;
        let metrics_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metrics_buf"),
            size: metrics_bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let metrics_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metrics_staging"),
            size: metrics_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let metrics_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metrics_uniform"),
            size: std::mem::size_of::<MetricsUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.sim_buffers = Some(SimBuffers {
            spike_list,
            spike_count,
            dispatch_args,
            max_abs_current,
            stats_staging,
            integrate_uniform,
            connect_uniform,
            metrics_buf,
            metrics_staging,
            metrics_uniform,
        });

        self.bind_groups = None;
        self.bind_groups_dirty = true;
    }

    /// Initialise the static render resources (manifold mesh + uniform buffers).
    /// Called ONCE after `resize_neurons`; call again on tier resize.
    /// Manifold geometry is static; uniforms are updated per-frame via writeBuffer.
    pub fn init_render_resources(
        &mut self,
        device: &wgpu::Device,
        manifold_vertices: &[[f32; 3]],
        manifold_faces: &[[u32; 3]],
        n: u32,
        grid_dim: u32,
    ) {
        use wgpu::util::DeviceExt;

        // Flat-pack vertices to [f32; 3] for vertex attribute binding.
        let vb_data: Vec<f32> = manifold_vertices
            .iter()
            .flat_map(|v| v.iter().copied())
            .collect();
        let manifold_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("manifold_vb"),
            contents: bytemuck::cast_slice(&vb_data),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let ib_data: Vec<u32> = manifold_faces
            .iter()
            .flat_map(|f| f.iter().copied())
            .collect();
        let manifold_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("manifold_ib"),
            contents: bytemuck::cast_slice(&ib_data),
            usage: wgpu::BufferUsages::INDEX,
        });
        let manifold_index_count = ib_data.len() as u32;

        let render_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render_uniform"),
            size: std::mem::size_of::<RenderUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let manifold_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("manifold_uniform"),
            size: std::mem::size_of::<ManifoldUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stim_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stim_uniform"),
            size: std::mem::size_of::<StimUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Grid uniform: static (grid_dim, n). Written once.
        let stim_grid_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stim_grid_uniform"),
            contents: bytemuck::cast_slice(&[grid_dim, n, 0u32, 0u32]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // ─── V2 Phase E: bloom sampler + per-pass uniform buffers ─────────────
        let bloom_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bloom_uniform_buf = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<BloomUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        self.render_resources = Some(RenderResources {
            manifold_vb,
            manifold_ib,
            manifold_index_count,
            render_uniform,
            manifold_uniform,
            stim_uniform,
            stim_grid_uniform,
            bloom_sampler,
            bloom_bright_uniform: bloom_uniform_buf("bloom-bright-uniform"),
            bloom_blur_h_uniform: bloom_uniform_buf("bloom-blur-h-uniform"),
            bloom_blur_v_uniform: bloom_uniform_buf("bloom-blur-v-uniform"),
            bloom_composite_uniform: bloom_uniform_buf("bloom-composite-uniform"),
        });
        self.bind_groups_dirty = true;
    }

    /// Morphology: generate the procedural neuron geometry on the CPU and upload
    /// it to a single storage buffer (allocated ONCE per network rebuild). The
    /// buffer is sized to the actual generated segment list; the generator caps
    /// at `n * max_segs_per_neuron(k)` and logs how many
    /// segments it had to drop if the cap is ever hit ("no silent caps").
    ///
    /// Wave 2: also builds the soma-sphere instance buffer (one entry per neuron)
    /// using `crate::sim::morphology::emit_soma_spheres`.
    pub fn init_morph_resources(
        &mut self,
        device: &wgpu::Device,
        positions: &[[f32; 3]],
        grid: &SpatialGrid,
        neuron_regions: &[crate::manifold::RegionKind],
        config: &SimConfig,
        params: &crate::sim::morphology::MorphologyParams,
        reach: crate::connectivity::ReachParams,
    ) {
        let source_types =
            crate::sim::morphology::build_source_types(config.seed_lo(), neuron_regions);
        let morph = crate::sim::morphology::generate(
            positions,
            grid,
            config.k,
            config.seed_lo(),
            params,
            &source_types,
            reach,
        );
        let dropped = morph.dropped;
        let stats = morph.stats;
        let process_roots = morph.process_roots;
        let segments = morph.segments;
        // Wave 2: soma sphere instances (one per neuron). Radius = params.base_radius
        // (the soma-end R0 that anchors all dendrite/axon branches).
        let spheres = crate::sim::morphology::emit_soma_spheres(
            positions,
            &source_types,
            params,
            &process_roots,
        );
        self.init_morph_resources_from_prepared(
            device,
            segments,
            spheres,
            *params,
            stats,
            dropped,
            positions.len(),
        );
    }

    /// Upload already-prepared morphology payloads. The payload remains a flat,
    /// GPU-agnostic CPU contract; this method owns all WebGPU chunking,
    /// allocation, and compaction-resource policy.
    pub fn init_morph_resources_from_prepared(
        &mut self,
        device: &wgpu::Device,
        segments: Vec<MorphSegment>,
        spheres: Vec<MorphSphereInstance>,
        params: crate::sim::morphology::MorphologyParams,
        stats: crate::sim::morphology::MorphologyStats,
        dropped: usize,
        neuron_count: usize,
    ) {
        let segment_count =
            u32::try_from(segments.len()).expect("morphology segment count exceeds u32");
        eprintln!(
            "[morphology] prepared {} segments for {} neurons ({} dropped); incoming raw={} groups={} in_degree mean/p99/max={:.2}/{}/{} visible_groups mean/p99/max={:.2}/{}/{} incoming capped/dropped={}/{}",
            segment_count,
            neuron_count,
            dropped,
            stats.incoming_raw_count,
            stats.incoming_socket_group_count,
            stats.incoming_in_degree_mean,
            stats.incoming_in_degree_p99,
            stats.incoming_in_degree_max,
            stats.incoming_visible_groups_mean,
            stats.incoming_visible_groups_p99,
            stats.incoming_visible_groups_max,
            stats.incoming_capped_count,
            stats.incoming_dropped_count,
        );

        let segment_layout = morph_segment_chunk_layout(
            segments.len(),
            device.limits().max_storage_buffer_binding_size as usize,
        );
        let mut segment_chunks = Vec::with_capacity(segment_layout.chunk_count());
        for chunk_idx in 0..segment_layout.chunk_count() {
            let start = chunk_idx * segment_layout.chunk_size;
            let end = (start + segment_layout.chunk_size).min(segments.len());
            let chunk_segments = &segments[start..end];
            let local_count =
                u32::try_from(chunk_segments.len()).expect("morphology chunk exceeds u32");
            let segment_bytes = segment_layout.chunk_bytes(chunk_idx) as u64;
            let segment_label = format!("morph_segment_buffer_{chunk_idx}");
            let segment_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&segment_label),
                contents: bytemuck::cast_slice(chunk_segments),
                usage: wgpu::BufferUsages::STORAGE,
            });

            // Active/recent compaction buffers are chunk-local. The index list is
            // sized to the chunk's segment count, so the worst case ("every
            // segment active") still cannot overflow.
            let cap = local_count.max(1);
            let indices_label = format!("morph_active_segment_indices_{chunk_idx}");
            let active_segment_indices = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&indices_label),
                size: (cap as u64) * 4,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let count_label = format!("morph_active_segment_count_{chunk_idx}");
            let active_segment_count = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&count_label),
                size: 4,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let args_label = format!("morph_active_draw_args_{chunk_idx}");
            let active_draw_args = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&args_label),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDIRECT
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let uniform_label = format!("morph_compact_uniform_{chunk_idx}");
            let compact_uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&uniform_label),
                size: std::mem::size_of::<CompactUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let selected_label = format!("morph_active_selected_{chunk_idx}");
            let active_selected = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&selected_label),
                size: 4,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let staging_label = format!("morph_selected_staging_{chunk_idx}");
            let selected_staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&staging_label),
                size: 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            segment_chunks.push(MorphSegmentChunk {
                segment_buffer,
                segment_count: local_count,
                segment_bytes,
                active_segment_indices,
                active_segment_count,
                active_draw_args,
                compact_uniform,
                active_selected,
                selected_staging,
            });
        }
        eprintln!(
            "[morphology] segment chunks: {} chunks, chunk_size={} segments, max_chunk_bytes={} B",
            segment_chunks.len(),
            segment_layout.chunk_size,
            segment_chunks
                .iter()
                .map(|chunk| chunk.segment_bytes)
                .max()
                .unwrap_or(0),
        );

        let sphere_count = spheres.len() as u32;
        // Non-empty guard: wgpu rejects zero-sized buffers.
        let sphere_data: Vec<MorphSphereInstance> = if spheres.is_empty() {
            vec![MorphSphereInstance {
                center: [0.0; 3],
                radius: 0.0,
                neuron_id: 0,
                kind: 2,
                _pad0: 0,
                _pad1: 0,
                root_dir: [0.0; 3],
                root_pull: 0.0,
            }]
        } else {
            spheres
        };
        let sphere_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("morph_sphere_buffer"),
            contents: bytemuck::cast_slice(&sphere_data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        eprintln!(
            "[morphology] soma spheres: {} instances ({} B)",
            sphere_count,
            sphere_count as usize * std::mem::size_of::<MorphSphereInstance>(),
        );

        let morph_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("morph_uniform"),
            size: std::mem::size_of::<MorphUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.morph_buffers = Some(MorphBuffers {
            segment_chunks,
            segment_layout,
            segment_count,
            morph_uniform,
            sphere_buffer,
            sphere_count,
            params,
            stats,
        });
        self.bind_groups_dirty = true;
    }

    /// Recreate render targets (depth texture) only when dimensions/format change.
    pub fn resize_render_targets(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        scene_format: wgpu::TextureFormat,
    ) {
        let changed = self
            .render_targets
            .as_ref()
            .map(|t| t.width != width || t.height != height)
            .unwrap_or(true);
        if changed {
            let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth_texture.create_view(&Default::default());

            // ─── V2 Phase E: bloom offscreen targets ─────────────────────────
            // Full-res HDR scene + half-res ping-pong (rgba16float). Allocated
            // unconditionally on resize (rare path); only USED when bloom is on.
            let hdr_usage =
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let make_tex = |w: u32, h: u32, label: &str, fmt: wgpu::TextureFormat| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: fmt,
                    usage: hdr_usage,
                    view_formats: &[],
                })
            };
            let bw = (width / 2).max(1);
            let bh = (height / 2).max(1);
            // The HDR scene target uses the SURFACE format so the (format-specific)
            // scene pipelines stay compatible when rendered offscreen. The blur
            // ping-pong stays rgba16float for soft-halo headroom.
            let hdr_texture = make_tex(width, height, "bloom-hdr", scene_format);
            let bloom_a_texture = make_tex(bw, bh, "bloom-a", wgpu::TextureFormat::Rgba16Float);
            let bloom_b_texture = make_tex(bw, bh, "bloom-b", wgpu::TextureFormat::Rgba16Float);
            let hdr_view = hdr_texture.create_view(&Default::default());
            let bloom_a_view = bloom_a_texture.create_view(&Default::default());
            let bloom_b_view = bloom_b_texture.create_view(&Default::default());

            self.render_targets = Some(RenderTargets {
                width,
                height,
                depth_texture: Some(depth_texture),
                depth_view: Some(depth_view),
                hdr_texture: Some(hdr_texture),
                hdr_view: Some(hdr_view),
                bloom_a_texture: Some(bloom_a_texture),
                bloom_a_view: Some(bloom_a_view),
                bloom_b_texture: Some(bloom_b_texture),
                bloom_b_view: Some(bloom_b_view),
                bloom_width: bw,
                bloom_height: bh,
            });
            self.bind_groups_dirty = true;
        }
    }

    /// Rebuild bind groups after any buffer recreation, then clear the dirty
    /// flag. Builds both double-buffer variants (front/back I buffers swapped).
    pub fn refresh_bind_groups(&mut self, device: &wgpu::Device, layouts: &GpuLayouts) {
        let (Some(nb), Some(grid), Some(sim)) =
            (&self.neuron_buffers, &self.grid_buffers, &self.sim_buffers)
        else {
            self.bind_groups_dirty = false;
            return;
        };

        // Single-chunk path (N ≤ 16M): chunk 0 holds the whole field. The
        // multi-chunk path compiles via ChunkedBuffer but is not exercised here.
        let v = chunk0(&nb.v);
        let last_spike = chunk0(&nb.last_spike);
        let i_front = chunk0(&nb.i_current);
        let i_back = chunk0(&nb.i_current_next);

        // integrate group 0 has two variants: I = front then back.
        let make_integrate = |i_buf: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("integrate-bg"),
                layout: &layouts.integrate_bgl,
                entries: &[
                    entry(0, v),
                    entry(1, last_spike),
                    entry(2, i_buf),
                    entry(3, &sim.spike_list),
                    entry(4, &sim.spike_count),
                ],
            })
        };
        let integrate = [make_integrate(i_front), make_integrate(i_back)];

        let integrate_uniform = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("integrate-uniform-bg"),
            layout: &layouts.integrate_uniform_bgl,
            entries: &[entry(0, &sim.integrate_uniform)],
        });

        let write_dispatch = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("write-dispatch-bg"),
            layout: &layouts.write_dispatch_bgl,
            entries: &[entry(0, &sim.spike_count), entry(1, &sim.dispatch_args)],
        });

        // scatter writes the OPPOSITE I buffer from the one integrate read this
        // tick. Variant 0: integrate reads front -> scatter writes back.
        let make_scatter = |i_next: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scatter-bg"),
                layout: &layouts.scatter_bgl,
                entries: &[
                    entry(0, &sim.spike_list),
                    entry(1, &sim.spike_count),
                    entry(2, i_next),
                    entry(3, last_spike),
                    entry(4, &grid.cell_of_neuron),
                    entry(5, &grid.cell_start),
                    entry(6, &grid.cell_neurons),
                    entry(7, &sim.max_abs_current),
                ],
            })
        };
        let scatter = [make_scatter(i_back), make_scatter(i_front)];

        let connect_uniform = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("connect-uniform-bg"),
            layout: &layouts.connect_uniform_bgl,
            entries: &[entry(0, &sim.connect_uniform)],
        });

        // Phase 3: render far-LOD bind group.
        // Requires render_resources (uniform buf) + neuron buffers (read-only).
        let (render_far, render_manifold, stimulate) = if let Some(rr) = &self.render_resources {
            let pos_x = chunk0(&nb.pos_x);
            let pos_y = chunk0(&nb.pos_y);
            let pos_z = chunk0(&nb.pos_z);
            let render_far_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("render-far-bg"),
                layout: &layouts.render_far_bgl,
                entries: &[
                    entry(0, &rr.render_uniform),
                    entry(1, pos_x),
                    entry(2, pos_y),
                    entry(3, pos_z),
                    entry(4, last_spike),
                    entry(5, v),
                ],
            });
            let render_manifold_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("render-manifold-bg"),
                layout: &layouts.render_manifold_bgl,
                entries: &[entry(0, &rr.manifold_uniform)],
            });
            // Stimulate bind groups: two variants for I parity.
            // parity 0: stim writes i_front (same buffer integrate reads at p=0).
            // parity 1: stim writes i_back.
            let make_stim = |i_buf: &wgpu::Buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("stimulate-bg"),
                    layout: &layouts.stimulate_bgl,
                    entries: &[
                        entry(0, &rr.stim_uniform),
                        entry(1, &rr.stim_grid_uniform),
                        entry(2, pos_x),
                        entry(3, pos_y),
                        entry(4, pos_z),
                        entry(5, &grid.cell_of_neuron),
                        entry(6, &grid.cell_start),
                        entry(7, &grid.cell_neurons),
                        entry(8, i_buf),
                    ],
                })
            };
            (
                Some(render_far_bg),
                Some(render_manifold_bg),
                Some([make_stim(i_front), make_stim(i_back)]),
            )
        } else {
            (None, None, None)
        };

        // ─── V2 Phase A: metrics reduction bind groups ───────────────────────
        let metrics = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("metrics-bg"),
            layout: &layouts.metrics_bgl,
            entries: &[
                entry(0, last_spike),
                entry(1, v),
                entry(2, &sim.metrics_buf),
            ],
        });
        let metrics_uniform = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("metrics-uniform-bg"),
            layout: &layouts.metrics_uniform_bgl,
            entries: &[entry(0, &sim.metrics_uniform)],
        });

        // ─── Morphology: per-chunk tube render + compaction bind groups ──────
        let morph_segment_chunks = if let Some(mb) = &self.morph_buffers {
            mb.segment_chunks
                .iter()
                .enumerate()
                .map(|(chunk_idx, chunk)| {
                    let render_label = format!("render-morphology-bg-{chunk_idx}");
                    let compact_label = format!("compact-morph-bg-{chunk_idx}");
                    let render_morphology = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&render_label),
                        layout: &layouts.render_morphology_bgl,
                        entries: &[
                            entry(0, &chunk.segment_buffer),
                            entry(1, last_spike),
                            entry(2, &mb.morph_uniform),
                            entry(6, &chunk.active_segment_indices),
                        ],
                    });
                    let compact_morph = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&compact_label),
                        layout: &layouts.compact_morph_bgl,
                        entries: &[
                            entry(0, &chunk.segment_buffer),
                            entry(1, last_spike),
                            entry(2, &chunk.compact_uniform),
                            entry(3, &chunk.active_segment_indices),
                            entry(4, &chunk.active_segment_count),
                            entry(5, &chunk.active_draw_args),
                            entry(6, &chunk.active_selected),
                        ],
                    });
                    MorphSegmentBindGroups {
                        render_morphology,
                        compact_morph,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        // ─── Morphology: soma sphere render bind group (Wave 2) ──────────────
        // Uses binding slots 3/4/5 (matching render_soma_spheres_bgl).
        // sphere_buffer and morph_uniform are from MorphBuffers; last_spike is
        // the same NeuronBuffers buffer reused at slot 4.
        let render_soma_spheres = if let Some(mb) = &self.morph_buffers {
            Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("render-soma-spheres-bg"),
                layout: &layouts.render_soma_spheres_bgl,
                entries: &[
                    entry(3, &mb.sphere_buffer),
                    entry(4, last_spike),
                    entry(5, &mb.morph_uniform),
                ],
            }))
        } else {
            None
        };

        self.bind_groups = Some(GpuBindGroups {
            integrate,
            integrate_uniform,
            write_dispatch,
            scatter,
            connect_uniform,
            render_far,
            render_manifold,
            stimulate,
            // V2 Phase A
            metrics,
            metrics_uniform,
            // Morphology
            morph_segment_chunks,
            render_soma_spheres,
        });
        self.bind_groups_dirty = false;
    }

    /// Release all owned GPU resources (backend switch / device loss / teardown).
    pub fn destroy(&mut self) {
        self.neuron_buffers = None;
        self.grid_buffers = None;
        self.sim_buffers = None;
        self.bind_groups = None;
        self.render_targets = None;
        self.render_resources = None;
        self.morph_buffers = None;
        self.bind_groups_dirty = false;
    }
}

/// Allocate the device buffer(s) for a chunked field and upload `data`.
/// Single chunk for N ≤ 16M; the loop generalises to multi-chunk.
fn alloc_field(
    device: &wgpu::Device,
    field: &mut ChunkedBuffer,
    data: &[u8],
    usage: wgpu::BufferUsages,
    label: &str,
) {
    let layout = field.layout;
    let chunks = layout.chunk_count().max(1);
    field.chunks.clear();
    for c in 0..chunks {
        let bytes = if layout.total == 0 {
            layout.element_bytes // never zero-sized
        } else {
            layout.chunk_bytes(c).max(layout.element_bytes)
        };
        let start = c * layout.chunk_size * layout.element_bytes;
        let end = (start + bytes).min(data.len());
        let slice = if start < data.len() {
            &data[start..end]
        } else {
            &[]
        };
        let buf = if slice.len() as usize == bytes {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: slice,
                usage,
            })
        } else {
            // Partial/empty: allocate sized buffer, then write what we have.
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes as u64,
                usage,
                mapped_at_creation: false,
            });
            b
        };
        field.chunks.push(buf);
    }
}

fn chunk0(field: &ChunkedBuffer) -> &wgpu::Buffer {
    &field.chunks[0]
}

fn entry(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

/// Integrate uniforms — layout must match `Uniforms` in integrate.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct IntegrateUniforms {
    pub tick: u32,
    pub n: u32,
    pub leak_decay: f32,
    pub threshold: f32,
    pub reset_potential: f32,
    pub refractory_ticks: u32,
    pub i_ext: f32,
    pub excitability: f32,
    pub fixed_point_scale: f32,
    pub synaptic_scale: f32,
    // ─── V2 Phase C: field order MUST match `Uniforms` in integrate.wgsl ──────
    pub seed_lo: u32,            // BV22 connectivity seed (per-neuron hash draws)
    pub heterogeneity: f32,      // [0,1] per-neuron spread; 0 => homogeneous
    pub weight_norm_factor: f32, // K-invariant recurrent scale; 1.0 at K=16
    pub input_mode: u32,         // 0=constant 1=poisson 2=pulsed 3=cursor 4=scripted 5=off
    pub _pad: [u32; 2],          // pad to 64 B (16-B alignment for UBO)
}

/// Connect uniforms — layout must match `ConnectUniforms` in scatter.wgsl /
/// write_scatter_dispatch.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ConnectUniforms {
    pub n: u32,
    pub k: u32,
    pub fixed_point_scale: f32,
    pub seed_lo: u32,
    pub grid_dim: u32,
    /// Heavy-tailed reach: numerator over `connectivity::REACH_FRAC_DEN` (0 = local only).
    pub long_range_frac: u32,
    /// Heavy-tailed reach: long-range cell radius (>= 1).
    pub max_reach: u32,
    pub _pad: [u32; 1], // pad to 32 B
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neuron_buffer_layouts_match_n() {
        let nb = NeuronBuffers::new(1_000_000);
        assert_eq!(nb.v.total(), 1_000_000);
        assert_eq!(nb.pos_x.total(), 1_000_000);
        assert_eq!(nb.v.layout.chunk_count(), 1);
    }

    #[test]
    fn uniform_sizes_aligned() {
        assert_eq!(std::mem::size_of::<IntegrateUniforms>(), 64);
        assert_eq!(std::mem::size_of::<ConnectUniforms>(), 32);
        assert_eq!(std::mem::size_of::<MetricsUniforms>(), 32);
        assert_eq!(METRICS_SLOT_COUNT, 32);
    }

    #[test]
    fn destroy_releases_everything() {
        let mut r = GpuResources::new();
        r.neuron_buffers = Some(NeuronBuffers::new(100));
        r.render_targets = Some(RenderTargets {
            width: 800,
            height: 600,
            depth_texture: None,
            depth_view: None,
            hdr_texture: None,
            hdr_view: None,
            bloom_a_texture: None,
            bloom_a_view: None,
            bloom_b_texture: None,
            bloom_b_view: None,
            bloom_width: 400,
            bloom_height: 300,
        });
        r.destroy();
        assert!(r.neuron_buffers.is_none());
        assert!(r.render_targets.is_none());
    }

    #[test]
    fn render_uniform_size_aligned() {
        assert_eq!(std::mem::size_of::<RenderUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<ManifoldUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<StimUniform>() % 16, 0);
    }

    #[test]
    fn morph_layouts_locked() {
        // Morphology: MorphSegment must be 48 B (Rust ⇄ WGSL parity); the render
        // uniform must be 16-aligned AND exactly 192 B (mat4=64 + 8×16 blocks;
        // includes Stage 0 lighting fields plus the v0.3.1 active/resting
        // brightness split: resting_brightness / active_boost).
        assert_eq!(std::mem::size_of::<MorphSegment>(), 48);
        assert_eq!(std::mem::size_of::<MorphSegment>() % 16, 0);
        assert_eq!(std::mem::size_of::<MorphUniforms>(), 192);
        assert_eq!(std::mem::size_of::<MorphUniforms>() % 16, 0);
    }

    #[test]
    fn morph_segment_chunk_layout_splits_above_128_mib_payload() {
        let elem = std::mem::size_of::<MorphSegment>();
        let total = (128 * 1024 * 1024 / elem) + 1_000;
        let layout = morph_segment_chunk_layout(total, 128 * 1024 * 1024);

        assert_eq!(elem, 48);
        assert!(layout.chunk_count() > 1);
        for chunk in 0..layout.chunk_count() {
            assert!(layout.chunk_bytes(chunk) <= MAX_CHUNK_BYTES);
            assert!(layout.chunk_bytes(chunk) <= 128 * 1024 * 1024);
        }
    }

    #[test]
    fn morph_segment_chunk_layout_handles_empty_input() {
        let layout = morph_segment_chunk_layout(0, 128 * 1024 * 1024);

        assert_eq!(layout.chunk_count(), 0);
        assert_eq!(layout.chunk_bytes(0), 0);
    }

    #[test]
    fn morph_segment_chunk_layout_last_chunk_is_partial() {
        let elem = std::mem::size_of::<MorphSegment>();
        let budget = elem * 3;
        let layout = morph_segment_chunk_layout(10, budget);

        assert_eq!(layout.chunk_size, 3);
        assert_eq!(layout.chunk_count(), 4);
        assert_eq!(layout.chunk_bytes(0), elem * 3);
        assert_eq!(layout.chunk_bytes(3), elem);
    }

    #[test]
    fn morph_segment_chunk_layout_respects_adapter_limit_below_default_budget() {
        let elem = std::mem::size_of::<MorphSegment>();
        let adapter_limit = 8 * 1024 * 1024;
        let total = adapter_limit / elem * 3 + 7;
        let layout = morph_segment_chunk_layout(total, adapter_limit);

        assert!(layout.chunk_count() > 1);
        for chunk in 0..layout.chunk_count() {
            assert!(layout.chunk_bytes(chunk) <= adapter_limit);
        }
    }

    #[test]
    fn product_scale_morph_segment_cap_uses_multiple_chunks() {
        let product_max_n = 20_000usize;
        let default_k = 16usize;
        let total = crate::sim::morphology::MorphologyParams::locked_default()
            .segment_cap(default_k)
            * product_max_n;
        let adapter_limit = 128 * 1024 * 1024;
        let layout = morph_segment_chunk_layout(total, adapter_limit);

        assert!(layout.chunk_count() > 1);
        for chunk in 0..layout.chunk_count() {
            assert!(layout.chunk_bytes(chunk) <= MAX_CHUNK_BYTES);
            assert!(layout.chunk_bytes(chunk) <= adapter_limit);
        }
    }

}
