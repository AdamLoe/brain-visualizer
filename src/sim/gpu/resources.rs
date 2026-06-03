//! GPU resource ownership boundary (architecture §5 "frame graph and resource
//! lifecycle"). Phase 2 allocates the real storage buffers, builds the bind
//! group layouts + bind groups, and uploads the initial silent-start state.
//! Phase 4 adds near-LOD (frustum cull + indirect draw + sphere/cylinder render)
//! resources.
//!
//! The rAF loop must never recreate buffers/bind groups/targets. Only the rare
//! structural-change methods here allocate.

use crate::buffers::ChunkedBuffer;
use crate::connectivity::spatial::SpatialGrid;
use crate::sim::backend::{initial_last_spike, SimConfig};
use crate::manifold::RegionKind;
use wgpu::util::DeviceExt;

// ─── Phase 4 defaults ────────────────────────────────────────────────────────

/// Default max neuron instances in the near-LOD append buffer.
pub const DEFAULT_MAX_NEAR_INSTANCES: u32 = 32_768;
/// Default max synapse instances in the near-LOD append buffer.
pub const DEFAULT_MAX_SYNAPSE_INSTANCES: u32 = 262_144;
/// Default K_NEAR: synapses materialized per visible neuron in near-LOD.
pub const DEFAULT_K_NEAR: u32 = 8;

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
}

/// Color / depth / HDR render targets. Phase 3: real depth texture + dimensions.
pub struct RenderTargets {
    pub width: u32,
    pub height: u32,
    /// Depth texture for the manifold mesh pass (depth-test before glow).
    pub depth_texture: Option<wgpu::Texture>,
    pub depth_view: Option<wgpu::TextureView>,
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
}

/// Manifold-pass uniform — only the MVP matrix.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ManifoldUniforms {
    pub mvp: [f32; 16],
}

/// Phase 4 near-LOD render uniform.
/// Layout must match `NearUniforms` in render_sphere.wgsl / render_cylinder.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NearRenderUniforms {
    pub mvp: [f32; 16],
    pub camera_pos: [f32; 3],
    pub sphere_radius: f32,
    pub lod_alpha: f32,
    pub _pad: [f32; 3],
}

/// Phase 4 frustum cull uniform.
/// Layout must match `FrustumUniforms` in frustum_cull.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrustumCullUniforms {
    /// 6 planes × vec4 = 24 f32 = 96 bytes.
    pub planes: [[f32; 4]; 6],
    pub camera_pos: [f32; 3],
    pub max_synapse_dist: f32,
    pub current_tick: u32,
    pub n: u32,
    pub _pad: [u32; 2],
}

/// Phase 4 near-LOD connect uniforms (shared between cull_neurons/cull_synapses).
/// Layout must match `NearConnectUniforms` in frustum_cull.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NearConnectUniforms {
    pub n: u32,
    pub k_near: u32,
    pub seed_lo: u32,
    pub grid_dim: u32,
    pub max_near_instances: u32,
    pub max_synapse_instances: u32,
    pub _pad: [u32; 2],
}

/// Phase 4 indirect write uniforms.
/// Layout must match `IndirectUniforms` in draw_indirect.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct IndirectWriteUniforms {
    pub sphere_index_count: u32,
    pub cylinder_index_count: u32,
    pub max_near_instances: u32,
    pub max_synapse_instances: u32,
}

/// Phase 4: near-LOD GPU buffers (allocated ONCE at startup; cleared per frame).
pub struct NearLodBuffers {
    /// Append buffer for NeuronInstance structs (32 B each).
    pub neuron_instances: wgpu::Buffer,
    /// Append buffer for SynapseInstance structs (32 B each).
    pub synapse_instances: wgpu::Buffer,
    /// Atomic counter for neuron append.
    pub neuron_count: wgpu::Buffer,
    /// Atomic counter for synapse append.
    pub synapse_count: wgpu::Buffer,
    /// Overflow profiler counters (unclamped excess).
    pub neuron_overflow: wgpu::Buffer,
    pub synapse_overflow: wgpu::Buffer,
    /// DrawIndexedIndirectArgs buffers (5 × u32 = 20 B each).
    pub neuron_draw_args: wgpu::Buffer,
    pub synapse_draw_args: wgpu::Buffer,
    /// Profiler: total (unclamped) visible counts after write_indirect.
    pub neuron_visible: wgpu::Buffer,
    pub synapse_visible: wgpu::Buffer,
    /// Frustum cull uniform (uploaded each frame).
    pub frustum_uniform: wgpu::Buffer,
    /// Near-LOD connect uniform (static after init).
    pub near_connect_uniform: wgpu::Buffer,
    /// Indirect-write uniform (static after init).
    pub indirect_write_uniform: wgpu::Buffer,
    /// Near-LOD render uniform (uploaded each frame).
    pub near_render_uniform: wgpu::Buffer,
    /// Static sphere vertex buffer (icosphere level-1, 12 verts × 24 B).
    pub sphere_vb: wgpu::Buffer,
    /// Static sphere index buffer (20 tris × 3 = 60 u16 indices).
    pub sphere_ib: wgpu::Buffer,
    pub sphere_index_count: u32,
    /// Static cylinder vertex buffer (12 verts of 6-sided prism × 12 B).
    pub cylinder_vb: wgpu::Buffer,
    /// Static cylinder index buffer (12 tris × 3 = 36 u16 indices).
    pub cylinder_ib: wgpu::Buffer,
    pub cylinder_index_count: u32,
    /// Staging buffer for async readback of near-LOD profiler counters (8 × u32).
    pub profiler_staging: wgpu::Buffer,
    /// Caps stored for bind-group / pipeline rebuild.
    pub max_near_instances: u32,
    pub max_synapse_instances: u32,
}

/// Phase 4 near-LOD profiler stats (read back once per frame from GPU).
#[derive(Debug, Default, Clone, Copy)]
pub struct NearLodStats {
    pub visible_neuron_candidates: u32,
    pub emitted_neuron_instances: u32,
    pub neuron_overflow: u32,
    pub visible_synapse_candidates: u32,
    pub emitted_synapse_instances: u32,
    pub synapse_overflow: u32,
}

/// Bind-group layouts shared by pipelines (phase 2 real handles + phase 3 render
/// + phase 4 near-LOD).
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
    // ─── Phase 4 ────────────────────────────────────────────────────────────
    /// Phase 4: frustum cull group 0: uniform + 5 neuron storages (pos_x/y/z,
    /// last_spike, v) + 4 instance/count rw + 2 overflow atomics.
    pub cull_bgl_group0: wgpu::BindGroupLayout,
    /// Phase 4: frustum cull group 1: CSR grid + near-connect uniform.
    pub cull_bgl_group1: wgpu::BindGroupLayout,
    /// Phase 4: draw_indirect write group 0.
    pub draw_indirect_bgl: wgpu::BindGroupLayout,
    /// Phase 4: sphere render group 0 (uniform + neuron_instances).
    pub render_sphere_bgl: wgpu::BindGroupLayout,
    /// Phase 4: cylinder render group 0 (uniform + synapse_instances).
    pub render_cylinder_bgl: wgpu::BindGroupLayout,
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
                    visibility: wgpu::ShaderStages::VERTEX,
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

        // ─── Phase 4: near-LOD layouts (SEPARATE from far-LOD, per phase-3 note) ─

        // Cull group 0: binding 0 = frustum uniform; 1-5 = neuron SoA (read);
        // 6,7 = instance append bufs (rw); 8,9 = atomic counters (rw);
        // 10,11 = overflow atomic counters (rw).
        let cull_cs_uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cull_cs_ro = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cull_cs_rw = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cull_bgl_group0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cull-bgl-g0"),
            entries: &[
                cull_cs_uniform(0),  // frustum uniforms
                cull_cs_ro(1),       // pos_x
                cull_cs_ro(2),       // pos_y
                cull_cs_ro(3),       // pos_z
                cull_cs_ro(4),       // last_spike
                cull_cs_ro(5),       // v
                cull_cs_rw(6),       // neuron_instances (append)
                cull_cs_rw(7),       // synapse_instances (append)
                cull_cs_rw(8),       // neuron_count (atomic)
                cull_cs_rw(9),       // synapse_count (atomic)
                cull_cs_rw(10),      // neuron_overflow (atomic)
                cull_cs_rw(11),      // synapse_overflow (atomic)
            ],
        });

        // Cull group 1: CSR grid storages + near-connect uniform.
        let cull_bgl_group1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cull-bgl-g1"),
            entries: &[
                cull_cs_ro(0),      // cell_of_neuron
                cull_cs_ro(1),      // cell_start
                cull_cs_ro(2),      // cell_neurons
                cull_cs_uniform(3), // near_connect_uniform
            ],
        });

        // draw_indirect group 0: 2 atomic counters (rw) + 2 draw-arg bufs (rw) +
        // 2 profiler visible counters (rw) + 1 indirect-write uniform.
        let draw_indirect_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("draw-indirect-bgl"),
            entries: &[
                cull_cs_rw(0),      // neuron_count (atomic)
                cull_cs_rw(1),      // synapse_count (atomic)
                cull_cs_rw(2),      // neuron_draw_args
                cull_cs_rw(3),      // synapse_draw_args
                cull_cs_rw(4),      // neuron_visible_count (profiler, atomic)
                cull_cs_rw(5),      // synapse_visible_count (profiler, atomic)
                cull_cs_uniform(6), // indirect-write params uniform
            ],
        });

        // Sphere render group 0: near-render uniform + neuron_instances (read-only
        // storage, vertex + fragment visible).
        let near_vs_uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let near_vs_ro = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let render_sphere_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render-sphere-bgl"),
            entries: &[
                near_vs_uniform(0), // NearUniforms
                near_vs_ro(1),      // neuron_instances
            ],
        });
        let render_cylinder_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render-cylinder-bgl"),
            entries: &[
                near_vs_uniform(0), // NearUniforms
                near_vs_ro(1),      // synapse_instances
            ],
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
            // Phase 4
            cull_bgl_group0,
            cull_bgl_group1,
            draw_indirect_bgl,
            render_sphere_bgl,
            render_cylinder_bgl,
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
    // ─── Phase 4 ────────────────────────────────────────────────────────────
    /// Phase 4: frustum-cull group 0 (frustum uniform + neuron SoA + instance bufs).
    /// None until `init_near_lod_resources` has been called.
    pub cull_group0: Option<wgpu::BindGroup>,
    /// Phase 4: frustum-cull group 1 (CSR grid + near-connect uniform).
    pub cull_group1: Option<wgpu::BindGroup>,
    /// Phase 4: draw_indirect write bind group.
    pub draw_indirect: Option<wgpu::BindGroup>,
    /// Phase 4: sphere render bind group.
    pub render_sphere: Option<wgpu::BindGroup>,
    /// Phase 4: cylinder render bind group.
    pub render_cylinder: Option<wgpu::BindGroup>,
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
    /// Phase 4: near-LOD GPU buffers.
    pub near_lod_buffers: Option<NearLodBuffers>,
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
            near_lod_buffers: None,
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

        let st_init = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        alloc_field(device, &mut nb.pos_x, bytemuck::cast_slice(&pos_x), st_init, "pos_x");
        alloc_field(device, &mut nb.pos_y, bytemuck::cast_slice(&pos_y), st_init, "pos_y");
        alloc_field(device, &mut nb.pos_z, bytemuck::cast_slice(&pos_z), st_init, "pos_z");
        alloc_field(device, &mut nb.v, bytemuck::cast_slice(&v_zero), st_init, "v");
        alloc_field(device, &mut nb.i_current, bytemuck::cast_slice(&i_zero), st_init, "i_current");
        alloc_field(device, &mut nb.i_current_next, bytemuck::cast_slice(&i_zero), st_init, "i_current_next");
        alloc_field(device, &mut nb.last_spike, bytemuck::cast_slice(&last_spike), st_init, "last_spike");
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
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
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
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
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
                _pad: [0; 3],
            }),
        );

        self.sim_buffers = Some(SimBuffers {
            spike_list,
            spike_count,
            dispatch_args,
            max_abs_current,
            stats_staging,
            integrate_uniform,
            connect_uniform,
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
        let vb_data: Vec<f32> = manifold_vertices.iter().flat_map(|v| v.iter().copied()).collect();
        let manifold_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("manifold_vb"),
            contents: bytemuck::cast_slice(&vb_data),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let ib_data: Vec<u32> = manifold_faces.iter().flat_map(|f| f.iter().copied()).collect();
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

        self.render_resources = Some(RenderResources {
            manifold_vb,
            manifold_ib,
            manifold_index_count,
            render_uniform,
            manifold_uniform,
            stim_uniform,
            stim_grid_uniform,
        });
        self.bind_groups_dirty = true;
    }

    /// Initialise Phase 4 near-LOD GPU buffers (instance append, indirect draw,
    /// sphere/cylinder geometry). Allocates ONCE; cleared per frame, never grown.
    /// Derives caps from adapter limits; disables near-LOD when buffers won't fit.
    pub fn init_near_lod_resources(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &SimConfig,
        grid: &SpatialGrid,
    ) {
        // Derive caps: check adapter limits via buffer-size limits.
        // NeuronInstance = 32 B, SynapseInstance = 32 B.
        let limits = device.limits();
        let max_binding = limits.max_storage_buffer_binding_size as u64;

        let max_near = (DEFAULT_MAX_NEAR_INSTANCES as u64).min(max_binding / 32) as u32;
        let max_syn  = (DEFAULT_MAX_SYNAPSE_INSTANCES as u64).min(max_binding / 32) as u32;

        if max_near == 0 || max_syn == 0 {
            // Adapter cannot support near-LOD buffers; leave near_lod_buffers None.
            eprintln!("[near_lod] adapter cannot support near-LOD buffers, disabling");
            self.near_lod_buffers = None;
            return;
        }

        let append_usage =
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        let atomic_usage =
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let indirect_usage =
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST;
        let uniform_usage =
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;

        let neuron_instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_neuron_instances"),
            size: (max_near as u64) * 32, // NeuronInstance = 32 B
            usage: append_usage,
            mapped_at_creation: false,
        });
        let synapse_instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_synapse_instances"),
            size: (max_syn as u64) * 32, // SynapseInstance = 32 B
            usage: append_usage,
            mapped_at_creation: false,
        });
        let neuron_count = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_neuron_count"),
            size: 4,
            usage: atomic_usage,
            mapped_at_creation: false,
        });
        let synapse_count = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_synapse_count"),
            size: 4,
            usage: atomic_usage,
            mapped_at_creation: false,
        });
        let neuron_overflow = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_neuron_overflow"),
            size: 4,
            usage: atomic_usage,
            mapped_at_creation: false,
        });
        let synapse_overflow = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_synapse_overflow"),
            size: 4,
            usage: atomic_usage,
            mapped_at_creation: false,
        });
        // DrawIndexedIndirectArgs: 5 × u32 = 20 B.
        let neuron_draw_args = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_neuron_draw_args"),
            size: 20,
            usage: indirect_usage,
            mapped_at_creation: false,
        });
        let synapse_draw_args = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_synapse_draw_args"),
            size: 20,
            usage: indirect_usage,
            mapped_at_creation: false,
        });
        // Profiler visible counts (written by draw_indirect shader).
        let neuron_visible = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_neuron_visible"),
            size: 4,
            usage: atomic_usage,
            mapped_at_creation: false,
        });
        let synapse_visible = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_synapse_visible"),
            size: 4,
            usage: atomic_usage,
            mapped_at_creation: false,
        });

        // Frustum uniform (64 B: 6*16 + 16 + 16 = 128 B → 112 B in struct).
        let frustum_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frustum_uniform"),
            size: std::mem::size_of::<FrustumCullUniforms>() as u64,
            usage: uniform_usage,
            mapped_at_creation: false,
        });

        // Near connect uniform (static after init).
        let near_connect_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("near_connect_uniform"),
            contents: bytemuck::bytes_of(&NearConnectUniforms {
                n: config.n as u32,
                k_near: DEFAULT_K_NEAR,
                seed_lo: config.seed_lo(),
                grid_dim: grid.dim,
                max_near_instances: max_near,
                max_synapse_instances: max_syn,
                _pad: [0; 2],
            }),
            usage: uniform_usage,
        });

        // Build sphere (icosphere level-1): 12 verts × (pos + normal) = 12 × 24 B.
        let (sphere_verts, sphere_indices) = build_icosphere();
        let sphere_index_count = sphere_indices.len() as u32;
        // Sphere VB: position + normal = 6 × f32 = 24 B/vert.
        let sphere_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sphere_vb"),
            contents: bytemuck::cast_slice(&sphere_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let sphere_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sphere_ib"),
            contents: bytemuck::cast_slice(&sphere_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Build 6-sided prism cylinder: 12 verts × 12 B (pos only).
        let (cyl_verts, cyl_indices) = build_cylinder_prism();
        let cylinder_index_count = cyl_indices.len() as u32;
        let cylinder_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cylinder_vb"),
            contents: bytemuck::cast_slice(&cyl_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cylinder_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cylinder_ib"),
            contents: bytemuck::cast_slice(&cyl_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Indirect write uniform (static after init).
        let indirect_write_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("indirect_write_uniform"),
            contents: bytemuck::bytes_of(&IndirectWriteUniforms {
                sphere_index_count,
                cylinder_index_count,
                max_near_instances: max_near,
                max_synapse_instances: max_syn,
            }),
            usage: uniform_usage,
        });

        // Near-render uniform (uploaded each frame).
        let near_render_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_render_uniform"),
            size: std::mem::size_of::<NearRenderUniforms>() as u64,
            usage: uniform_usage,
            mapped_at_creation: false,
        });

        // Profiler staging: 6 × u32 (neuron_count, neuron_overflow, synapse_count,
        // synapse_overflow, neuron_visible, synapse_visible).
        let profiler_staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("near_lod_profiler_staging"),
            size: 24, // 6 × u32
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Zero all counters via queue.write_buffer (not COPY_DST for instances,
        // but the count/overflow bufs have COPY_DST so we zero them here).
        let zero4 = [0u32];
        queue.write_buffer(&neuron_count,    0, bytemuck::cast_slice(&zero4));
        queue.write_buffer(&synapse_count,   0, bytemuck::cast_slice(&zero4));
        queue.write_buffer(&neuron_overflow, 0, bytemuck::cast_slice(&zero4));
        queue.write_buffer(&synapse_overflow,0, bytemuck::cast_slice(&zero4));
        queue.write_buffer(&neuron_visible,  0, bytemuck::cast_slice(&zero4));
        queue.write_buffer(&synapse_visible, 0, bytemuck::cast_slice(&zero4));

        self.near_lod_buffers = Some(NearLodBuffers {
            neuron_instances,
            synapse_instances,
            neuron_count,
            synapse_count,
            neuron_overflow,
            synapse_overflow,
            neuron_draw_args,
            synapse_draw_args,
            neuron_visible,
            synapse_visible,
            frustum_uniform,
            near_connect_uniform,
            indirect_write_uniform,
            near_render_uniform,
            sphere_vb,
            sphere_ib,
            sphere_index_count,
            cylinder_vb,
            cylinder_ib,
            cylinder_index_count,
            profiler_staging,
            max_near_instances: max_near,
            max_synapse_instances: max_syn,
        });
        self.bind_groups_dirty = true;
    }

    /// Recreate render targets (depth texture) only when dimensions/format change.
    pub fn resize_render_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let changed = self
            .render_targets
            .as_ref()
            .map(|t| t.width != width || t.height != height)
            .unwrap_or(true);
        if changed {
            let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth_texture.create_view(&Default::default());
            self.render_targets = Some(RenderTargets {
                width,
                height,
                depth_texture: Some(depth_texture),
                depth_view: Some(depth_view),
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
        let (render_far, render_manifold, stimulate) =
            if let Some(rr) = &self.render_resources {
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

        // ─── Phase 4: near-LOD bind groups ──────────────────────────────────────
        let (cull_group0, cull_group1, draw_indirect_bg, render_sphere_bg, render_cylinder_bg) =
            if let (Some(nlb), Some(gb)) = (&self.near_lod_buffers, &self.grid_buffers) {
                let pos_x = chunk0(&nb.pos_x);
                let pos_y = chunk0(&nb.pos_y);
                let pos_z = chunk0(&nb.pos_z);

                // Cull group 0.
                let cg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("cull-bg-g0"),
                    layout: &layouts.cull_bgl_group0,
                    entries: &[
                        entry(0, &nlb.frustum_uniform),
                        entry(1, pos_x),
                        entry(2, pos_y),
                        entry(3, pos_z),
                        entry(4, last_spike),
                        entry(5, v),
                        entry(6, &nlb.neuron_instances),
                        entry(7, &nlb.synapse_instances),
                        entry(8, &nlb.neuron_count),
                        entry(9, &nlb.synapse_count),
                        entry(10, &nlb.neuron_overflow),
                        entry(11, &nlb.synapse_overflow),
                    ],
                });
                // Cull group 1.
                let cg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("cull-bg-g1"),
                    layout: &layouts.cull_bgl_group1,
                    entries: &[
                        entry(0, &gb.cell_of_neuron),
                        entry(1, &gb.cell_start),
                        entry(2, &gb.cell_neurons),
                        entry(3, &nlb.near_connect_uniform),
                    ],
                });
                // Draw indirect group.
                let dig = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("draw-indirect-bg"),
                    layout: &layouts.draw_indirect_bgl,
                    entries: &[
                        entry(0, &nlb.neuron_count),
                        entry(1, &nlb.synapse_count),
                        entry(2, &nlb.neuron_draw_args),
                        entry(3, &nlb.synapse_draw_args),
                        entry(4, &nlb.neuron_visible),
                        entry(5, &nlb.synapse_visible),
                        entry(6, &nlb.indirect_write_uniform),
                    ],
                });
                // Sphere render group.
                let srg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("render-sphere-bg"),
                    layout: &layouts.render_sphere_bgl,
                    entries: &[
                        entry(0, &nlb.near_render_uniform),
                        entry(1, &nlb.neuron_instances),
                    ],
                });
                // Cylinder render group.
                let crg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("render-cylinder-bg"),
                    layout: &layouts.render_cylinder_bgl,
                    entries: &[
                        entry(0, &nlb.near_render_uniform),
                        entry(1, &nlb.synapse_instances),
                    ],
                });
                (Some(cg0), Some(cg1), Some(dig), Some(srg), Some(crg))
            } else {
                (None, None, None, None, None)
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
            // Phase 4
            cull_group0,
            cull_group1,
            draw_indirect: draw_indirect_bg,
            render_sphere: render_sphere_bg,
            render_cylinder: render_cylinder_bg,
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
        self.near_lod_buffers = None;
        self.bind_groups_dirty = false;
    }
}

// ─── Phase 4: geometry generators ────────────────────────────────────────────

/// Build an icosphere (subdivision level 1) → 12 vertices, 20 triangles.
/// Returns (vertices_f32, indices_u16) where each vertex is [px, py, pz, nx, ny, nz]
/// (6 × f32 = 24 B) and indices are 3 × u16 per triangle (60 u16 = 120 B).
/// The sphere is a unit sphere (radius 1); the VS scales per instance.
pub fn build_icosphere() -> (Vec<f32>, Vec<u16>) {
    // Golden ratio φ = (1 + √5) / 2 ≈ 1.618...
    let phi = (1.0f32 + 5.0f32.sqrt()) / 2.0;

    // 12 vertices of a regular icosahedron (normalized to unit sphere).
    let raw: [[f32; 3]; 12] = [
        [-1.0,  phi,  0.0],
        [ 1.0,  phi,  0.0],
        [-1.0, -phi,  0.0],
        [ 1.0, -phi,  0.0],
        [ 0.0, -1.0,  phi],
        [ 0.0,  1.0,  phi],
        [ 0.0, -1.0, -phi],
        [ 0.0,  1.0, -phi],
        [ phi,  0.0, -1.0],
        [ phi,  0.0,  1.0],
        [-phi,  0.0, -1.0],
        [-phi,  0.0,  1.0],
    ];
    let mut verts: Vec<f32> = Vec::with_capacity(12 * 6);
    for p in &raw {
        let l = (p[0]*p[0] + p[1]*p[1] + p[2]*p[2]).sqrt().max(1e-9);
        let n = [p[0]/l, p[1]/l, p[2]/l];
        verts.extend_from_slice(&[n[0], n[1], n[2], n[0], n[1], n[2]]);
    }

    // 20 faces of the icosahedron (from Wikipedia / standard winding CCW).
    let indices: Vec<u16> = vec![
        0, 11, 5,    0, 5, 1,     0, 1, 7,     0, 7, 10,    0, 10, 11,
        1, 5, 9,     5, 11, 4,    11, 10, 2,    10, 7, 6,     7, 1, 8,
        3, 9, 4,     3, 4, 2,     3, 2, 6,      3, 6, 8,     3, 8, 9,
        4, 9, 5,     2, 4, 11,    6, 2, 10,     8, 6, 7,     9, 8, 1,
    ];
    assert_eq!(indices.len(), 60);
    (verts, indices)
}

/// Build a 6-sided prism cylinder.
/// - 12 vertices: 6 bottom ring + 6 top ring.
/// - Bottom ring at Y=0, top ring at Y=1, radius=1 in XZ.
/// - 12 triangles: 6 side quads (2 tris each) + 0 end caps (not needed for synapse lines).
/// Vertex layout: [x, y, z] × 12 = 36 f32 = 144 B.
/// Index layout: 36 u16 (12 tris × 3).
pub fn build_cylinder_prism() -> (Vec<f32>, Vec<u16>) {
    use std::f32::consts::PI;
    let sides = 6u32;
    let mut verts: Vec<f32> = Vec::with_capacity(12 * 3);
    // Bottom ring (y=0), then top ring (y=1).
    for ring in 0..2 {
        let y = ring as f32;
        for s in 0..sides {
            let angle = (s as f32) * 2.0 * PI / (sides as f32);
            verts.push(angle.cos()); // x
            verts.push(y);            // y
            verts.push(angle.sin()); // z
        }
    }
    // Indices: side quads only (no end caps).
    // Bottom ring: indices 0..5, top ring: indices 6..11.
    let mut indices: Vec<u16> = Vec::with_capacity(36);
    for s in 0..sides {
        let b0 = s as u16;
        let b1 = ((s + 1) % sides) as u16;
        let t0 = (s + sides) as u16;
        let t1 = ((s + 1) % sides + sides) as u16;
        // Two triangles per side quad (CCW).
        indices.extend_from_slice(&[b0, b1, t0]);
        indices.extend_from_slice(&[b1, t1, t0]);
    }
    assert_eq!(indices.len(), 36);
    (verts, indices)
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
        let slice = if start < data.len() { &data[start..end] } else { &[] };
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
    pub _pad: [u32; 2], // pad to 48 B (16-B alignment for UBO)
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
    pub _pad: [u32; 3], // pad to 32 B
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
        assert_eq!(std::mem::size_of::<IntegrateUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<ConnectUniforms>() % 16, 0);
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
        });
        r.destroy();
        assert!(r.neuron_buffers.is_none());
        assert!(r.render_targets.is_none());
        assert!(r.near_lod_buffers.is_none());
    }

    #[test]
    fn render_uniform_size_aligned() {
        assert_eq!(std::mem::size_of::<RenderUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<ManifoldUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<StimUniform>() % 16, 0);
    }

    #[test]
    fn near_lod_uniform_sizes_aligned() {
        assert_eq!(std::mem::size_of::<NearRenderUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<FrustumCullUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<NearConnectUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<IndirectWriteUniforms>() % 16, 0);
    }

    #[test]
    fn icosphere_has_correct_geometry() {
        let (verts, indices) = build_icosphere();
        // 12 verts × 6 f32 = 72 f32
        assert_eq!(verts.len(), 72);
        // 20 tris × 3 = 60 u16
        assert_eq!(indices.len(), 60);
        // All verts should be on unit sphere (radius ≈ 1.0).
        for v in verts.chunks(6) {
            let r = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
            assert!((r - 1.0).abs() < 1e-5, "vertex not on unit sphere: r={r}");
        }
    }

    #[test]
    fn cylinder_prism_has_correct_geometry() {
        let (verts, indices) = build_cylinder_prism();
        // 12 verts × 3 f32 = 36 f32
        assert_eq!(verts.len(), 36);
        // 12 tris × 3 = 36 u16
        assert_eq!(indices.len(), 36);
    }
}
