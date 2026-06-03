# Phase 3 — GPU Rendering (Far LOD + Camera)

_The brain lights up. Neurons glow when they fire; the folded surface is
visible; the camera orbits and zooms. This is the first phase where the
visitor sees something._

## Done when
- Folded brain surface is visible from the default camera angle.
- Neurons glow when they spike; glow decays over ~100–200ms.
- Region colors are distinct (input / association / output).
- Click-drag orbits; scroll zooms. Touch equivalents work.
- Cursor hover injects current visibly (neurons near cursor light up more).
- Excitability slider changes visible activity level.
- The natural startup ramp (silent → active) is visible over ~2–5 seconds.
- Console profiler reports GPU timing per pass (if timestamp-query available).
- Render path has a cheap default mode and optional expensive modes are gated
  behind settings, not always-on pipeline work.

## Render pipeline overview

```
[GPU sim buffers: pos_x[], pos_y[], pos_z[], v[], last_spike[]]
         |
         v
[Vertex shader: read SoA positions, last_spike[], v[]]
         |  compute glow brightness + region color
         v
[Fragment shader: additive blended billboard glow]
         |
         v
[Canvas — dark background, glowing neurons]
```

No CPU readback. The render pass reads directly from the sim storage buffers.

Render resource rules:
- far LOD is the default and must work without HDR/bloom;
- render targets are recreated only on canvas size/format change;
- camera/render uniforms are updated only when values change materially;
- no per-frame creation of pipelines, bind groups, textures, or vertex buffers.

## Far LOD: billboard glow pass

### `src/sim/gpu/shaders/render_far.wgsl`
```wgsl
struct Uniforms {
    mvp: mat4x4<f32>,
    camera_right: vec3<f32>,
    camera_up: vec3<f32>,
    tick: u32,
    glow_tau: f32,      // decay constant in ticks (~100ms = 100 ticks)
    point_radius: f32,  // base radius in world units
    n: u32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> pos_x: array<f32>;
@group(0) @binding(2) var<storage, read> pos_y: array<f32>;
@group(0) @binding(3) var<storage, read> pos_z: array<f32>;
@group(0) @binding(4) var<storage, read> last_spike: array<u32>;
@group(0) @binding(5) var<storage, read> v: array<f32>;

const HAS_SPIKED_MASK: u32 = 0x80000000u;
const TICK_MASK: u32 = 0x00FFFFFFu;

fn has_spiked(packed: u32) -> bool {
    return (packed & HAS_SPIKED_MASK) != 0u;
}

fn tick_diff(now: u32, then_tick: u32) -> u32 {
    return (now - then_tick) & TICK_MASK;
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) glow: f32,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

fn region_color(neuron_type: u32) -> vec3<f32> {
    let region = (neuron_type >> 2u) & 0x3u;  // bits [3:2] of type byte
    switch region {
        case 0u: { return vec3(0.2, 0.6, 1.0); }   // input: cool blue
        case 1u: { return vec3(0.4, 0.9, 0.4); }   // association: green
        case 2u: { return vec3(1.0, 0.5, 0.2); }   // output: warm orange
        default: { return vec3(0.8, 0.8, 0.8); }
    }
}

@vertex
fn vs_main(
    @builtin(vertex_index) quad_vertex: u32,
    @builtin(instance_index) neuron_id: u32,
) -> VertOut {
    let packed      = last_spike[neuron_id];
    let neuron_type = (packed >> 24u) & 0x7Fu;
    let last_tick   = packed & TICK_MASK;

    let ticks_since = tick_diff(u.tick, last_tick);
    let glow = select(0.0, exp(-f32(ticks_since) / u.glow_tau), has_spiked(packed));

    // Sub-threshold voltage contributes a faint background glow
    let v_glow = clamp(v[neuron_id] * 0.15, 0.0, 0.15);

    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2( 1.0, -1.0), vec2(-1.0,  1.0),
        vec2(-1.0,  1.0), vec2( 1.0, -1.0), vec2( 1.0,  1.0),
    );
    let corner = corners[quad_vertex];
    let radius = u.point_radius * (1.0 + glow * 2.0);
    let center = vec3<f32>(pos_x[neuron_id], pos_y[neuron_id], pos_z[neuron_id]);
    let world_pos = center
        + u.camera_right * corner.x * radius
        + u.camera_up * corner.y * radius;

    var out: VertOut;
    out.pos   = u.mvp * vec4(world_pos, 1.0);
    out.glow  = glow + v_glow;
    out.color = region_color(neuron_type);
    out.uv    = corner;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    if d > 1.0 { discard; }
    let falloff = exp(-d * d * 3.0);
    let alpha = (in.glow * 0.9 + 0.05) * falloff;
    return vec4(in.color * in.glow * falloff, alpha);
}
```

**Blend state:** additive blending (`src = One, dst = One`). Multiple overlapping
glowing neurons sum their brightness — correct for the volumetric cluster effect.

**Draw call:** `draw(6, N, 0, 0)` — six vertices per neuron, one billboard
instance per neuron. Use triangle-list topology. Do not use WebGPU point
sprites or `@builtin(point_size)`; programmable point size is not available
portably in WGSL/WebGPU.

## Render-cost modes

Ship the far LOD in two cost modes:

1. **Default:** direct additive billboard pass to the canvas. This is the target
   for low/mobile and for the first visible milestone.
2. **Enhanced:** optional HDR glow/bloom path. Render bright neurons to an HDR
   texture, run a small bloom chain, then fullscreen-compose to the canvas.

Enhanced mode must be easy to disable from config and the adaptive scaler. Avoid
MSAA and mipmaps on HDR render targets unless a benchmark shows they are worth
it. Bloom textures should be recreated only when canvas dimensions change, and
the bloom chain should use a fixed small mip count so cost is predictable.

Render resolution is a scaler knob. Allow internal render size to be below
device pixel ratio on mobile or when frame p95 exceeds budget.

## Camera

### `web/camera.ts`
```typescript
export class Camera {
  private azimuth = 0.3;      // radians
  private elevation = 0.4;
  private distance = 3.0;     // world units from origin
  private target = [0, 0, 0];

  mvpMatrix(): Float32Array { ... }  // perspective * view * model

  onMouseMove(dx: number, dy: number, buttons: number) {
    if (buttons & 1) {           // left drag = orbit
      this.azimuth   += dx * 0.005;
      this.elevation += dy * 0.005;
      this.elevation  = clamp(this.elevation, -1.4, 1.4);
    }
    // hover (no button) = stimulate — handled in main.ts
  }

  onWheel(dy: number) {
    this.distance *= 1 + dy * 0.001;
    this.distance = clamp(this.distance, 0.5, 10.0);
  }

  // Touch: one finger = orbit (same as left drag), pinch = zoom
  onTouchMove(touches: TouchList) { ... }
}
```

MVP uniform update: rebuild every frame from azimuth/elevation/distance,
upload via `device.queue.writeBuffer(mvp_uniform_buf, ...)`.

## Cursor stimulation hookup

In `web/main.ts`, on `mousemove` (no button held):
1. Unproject mouse position to a ray in world space using the inverse MVP.
2. Find intersection with the bounding sphere of the manifold.
3. Call `backend.stimulate(hit_point, STIM_RADIUS, STIM_CURRENT)`.

In GPU backend (`src/sim/gpu/mod.rs`):
- `stimulate()` writes to a small uniform buffer: `{pos: [f32;3], radius: f32, current: f32, active: u32}`.
- A separate compute dispatch at the start of each tick finds neurons within
  `radius` using the spatial hash and adds `current` (fixed-point) to their `I`.
- If `active = 0` (no hover this frame), skip the dispatch.

`STIM_RADIUS = 0.15` (world units), `STIM_CURRENT = 0.3` (biological mV,
converted to fixed-point before upload).

## Render timing via timestamp queries
```rust
// In GpuBackend::render():
if self.timestamp_queries_available {
    encoder.write_timestamp(&query_set, 0);  // before render pass
    // ... render pass ...
    encoder.write_timestamp(&query_set, 1);  // after render pass
    // Resolve + read back asynchronously; report to profiler
}
```
Gate entirely on feature availability check at init. Never block the render loop
waiting for timing results. Resolve timestamps through the same async staging
pool used by the sim passes; if the pool is full, drop that sample.

## Manifold surface visualization
The folded surface itself (not just neurons) should be faintly visible as a
dark mesh or subtle depth cue so the brain shape reads clearly.

- Render the manifold faces as a dark (`vec3(0.05, 0.05, 0.08)`) triangle mesh
  with depth testing enabled, drawn before the billboard glow pass.
- The face geometry is static; upload once as a vertex buffer from the manifold
  data generated in phase 1.
- No lighting model needed — flat dark fill is enough. The neuron glow provides
  all the illumination.

## Debug overlays

Debug views such as spatial cells, stimulation radius, frustum bounds, or render
resolution should be separate optional render passes. They may read production
buffers but production rendering must not depend on them. Default off; update
their uniforms/buffers only while visible.

## What is still stubbed
- Near LOD (zoomed-in spheres/cylinders) — phase 4.
- Speed control UI — phase 5.
- Brain states button group — phase 5.
- Backend toggle UI — phase 5.
- HUD overlay — phase 7.
