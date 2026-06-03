//! GPU resource ownership boundary (architecture §5 "frame graph and resource
//! lifecycle"). Phase 2 allocates the real storage buffers, builds the bind
//! group layouts + bind groups, and uploads the initial silent-start state.
//!
//! The rAF loop must never recreate buffers/bind groups/targets. Only the rare
//! structural-change methods here allocate.

use crate::buffers::ChunkedBuffer;
use crate::connectivity::spatial::SpatialGrid;
use crate::sim::backend::{initial_last_spike, SimConfig};
use crate::manifold::RegionKind;
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

/// Bind-group layouts shared by pipelines (phase 2 real handles + phase 3 render).
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

        Self {
            integrate_bgl,
            integrate_uniform_bgl,
            write_dispatch_bgl,
            scatter_bgl,
            connect_uniform_bgl,
            render_far_bgl,
            render_manifold_bgl,
            stimulate_bgl,
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

        self.bind_groups = Some(GpuBindGroups {
            integrate,
            integrate_uniform,
            write_dispatch,
            scatter,
            connect_uniform,
            render_far,
            render_manifold,
            stimulate,
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
    }

    #[test]
    fn render_uniform_size_aligned() {
        assert_eq!(std::mem::size_of::<RenderUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<ManifoldUniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<StimUniform>() % 16, 0);
    }
}
