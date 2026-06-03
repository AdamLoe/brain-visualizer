// CPU backend WebGL2 renderer (Phase 6, architecture §6).
//
// The CPU sim writes its SoA (v_render[], last_spike[], positions[]) into the
// wasm linear memory (a SharedArrayBuffer when threads are on). Each frame the
// main thread uploads v_render + last_spike (and the static positions once) as
// ARRAY_BUFFERs and draws N instanced billboards with the SAME glow/region
// logic as the GPU far-LOD shader (render_far.wgsl), ported to GLSL ES 3.0.
//
// Full upload each frame is fine for N <= 200k (~1.6 MB/frame for v_render +
// last_spike). Delta upload is a future optimization for larger N.
//
// This module typechecks + bundles headless; the runtime path needs a browser
// WebGL2 context (manual TODO — no browser in the build env).

import type { Camera } from "./camera";

const VERT_SRC = `#version 300 es
precision highp float;

uniform mat4 u_mvp;
uniform vec3 u_camera_right;
uniform vec3 u_camera_up;
uniform uint u_tick;
uniform float u_glow_tau;
uniform float u_point_radius;

// Per-instance attributes (one per neuron).
layout(location = 0) in vec3 a_pos;        // neuron world position
layout(location = 1) in uint a_last_spike; // packed: bit31 valid, [30:24] type, [23:0] tick
layout(location = 2) in float a_v;         // decayed render voltage

out float v_glow;
out vec3 v_color;
out vec2 v_uv;

const uint HAS_SPIKED_MASK = 0x80000000u;
const uint TICK_MASK = 0x00FFFFFFu;

bool has_spiked(uint packed) { return (packed & HAS_SPIKED_MASK) != 0u; }
uint tick_diff(uint now, uint then_tick) { return (now - then_tick) & TICK_MASK; }

vec3 region_color(uint neuron_type) {
  uint region = (neuron_type >> 2u) & 0x3u;
  if (region == 0u) return vec3(0.2, 0.6, 1.0);  // input: cool blue
  if (region == 1u) return vec3(0.4, 0.9, 0.4);  // association: green
  if (region == 2u) return vec3(1.0, 0.5, 0.2);  // output: warm orange
  return vec3(0.8, 0.8, 0.8);
}

void main() {
  uint packed = a_last_spike;
  uint neuron_type = (packed >> 24u) & 0x7Fu;
  uint last_tick = packed & TICK_MASK;

  uint ticks_since = tick_diff(u_tick, last_tick);
  float glow = has_spiked(packed) ? exp(-float(ticks_since) / u_glow_tau) : 0.0;
  float v_glow_sub = clamp(a_v * 0.15, 0.0, 0.15);

  // Two-triangle quad (6 vertices per instance) from gl_VertexID.
  vec2 corners[6] = vec2[6](
    vec2(-1.0, -1.0), vec2( 1.0, -1.0), vec2(-1.0,  1.0),
    vec2(-1.0,  1.0), vec2( 1.0, -1.0), vec2( 1.0,  1.0)
  );
  vec2 corner = corners[gl_VertexID];
  float radius = u_point_radius * (1.0 + glow * 2.0);
  vec3 world_pos = a_pos
    + u_camera_right * corner.x * radius
    + u_camera_up    * corner.y * radius;

  gl_Position = u_mvp * vec4(world_pos, 1.0);
  v_glow = glow + v_glow_sub;
  v_color = region_color(neuron_type);
  v_uv = corner;
}`;

const FRAG_SRC = `#version 300 es
precision highp float;

in float v_glow;
in vec3 v_color;
in vec2 v_uv;
out vec4 frag_color;

void main() {
  float d = length(v_uv);
  if (d > 1.0) discard;
  float falloff = exp(-d * d * 3.0);
  float alpha = (v_glow * 0.9 + 0.05) * falloff;
  frag_color = vec4(v_color * v_glow * falloff, alpha);
}`;

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const sh = gl.createShader(type)!;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(sh);
    gl.deleteShader(sh);
    throw new Error(`[cpu-renderer] shader compile failed: ${log}`);
  }
  return sh;
}

/** WebGL2 renderer for the CPU backend's SoA, mirroring the GPU far-LOD glow. */
export class CpuRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private posBuf: WebGLBuffer;
  private lastSpikeBuf: WebGLBuffer;
  private vBuf: WebGLBuffer;
  private vao: WebGLVertexArrayObject;
  private n = 0;

  private uMvp: WebGLUniformLocation;
  private uRight: WebGLUniformLocation;
  private uUp: WebGLUniformLocation;
  private uTick: WebGLUniformLocation;
  private uGlowTau: WebGLUniformLocation;
  private uPointRadius: WebGLUniformLocation;

  private glowTau = 100.0;
  private pointRadius = 0.012;

  constructor(gl: WebGL2RenderingContext) {
    this.gl = gl;
    const prog = gl.createProgram()!;
    gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, VERT_SRC));
    gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, FRAG_SRC));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      throw new Error(`[cpu-renderer] link failed: ${gl.getProgramInfoLog(prog)}`);
    }
    this.program = prog;

    this.uMvp = gl.getUniformLocation(prog, "u_mvp")!;
    this.uRight = gl.getUniformLocation(prog, "u_camera_right")!;
    this.uUp = gl.getUniformLocation(prog, "u_camera_up")!;
    this.uTick = gl.getUniformLocation(prog, "u_tick")!;
    this.uGlowTau = gl.getUniformLocation(prog, "u_glow_tau")!;
    this.uPointRadius = gl.getUniformLocation(prog, "u_point_radius")!;

    this.posBuf = gl.createBuffer()!;
    this.lastSpikeBuf = gl.createBuffer()!;
    this.vBuf = gl.createBuffer()!;
    this.vao = gl.createVertexArray()!;
  }

  setGlowTau(tau: number): void { this.glowTau = tau; }
  setPointRadius(r: number): void { this.pointRadius = r; }

  /**
   * Upload the static neuron positions once (after each resize / restart).
   * `positions` is a Float32Array of 3*N xyz triples (a view over wasm memory).
   */
  setPositions(positions: Float32Array, neuronCount: number): void {
    const gl = this.gl;
    this.n = neuronCount;
    gl.bindVertexArray(this.vao);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(0, 1); // one position per instance

    // Pre-size the dynamic buffers.
    gl.bindBuffer(gl.ARRAY_BUFFER, this.lastSpikeBuf);
    gl.bufferData(gl.ARRAY_BUFFER, neuronCount * 4, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribIPointer(1, 1, gl.UNSIGNED_INT, 0, 0);
    gl.vertexAttribDivisor(1, 1);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.vBuf);
    gl.bufferData(gl.ARRAY_BUFFER, neuronCount * 4, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(2, 1);

    gl.bindVertexArray(null);
  }

  /**
   * Draw one frame. `vRender` (Float32Array, len N) and `lastSpike`
   * (Uint32Array, len N) are views over the wasm SoA (full upload each frame).
   */
  render(
    camera: Camera,
    tick: number,
    vRender: Float32Array,
    lastSpike: Uint32Array,
  ): void {
    const gl = this.gl;
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (this.n === 0) return;

    // Additive blend (matches the GPU far-LOD pass: src=One, dst=One).
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE);
    gl.disable(gl.DEPTH_TEST);

    gl.useProgram(this.program);
    gl.bindVertexArray(this.vao);

    // Full upload of the changed per-frame attributes.
    gl.bindBuffer(gl.ARRAY_BUFFER, this.lastSpikeBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, lastSpike);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, vRender);

    const mvp = camera.mvpMatrix();
    const right = camera.cameraRight();
    const up = camera.cameraUp();
    gl.uniformMatrix4fv(this.uMvp, false, mvp);
    gl.uniform3f(this.uRight, right[0], right[1], right[2]);
    gl.uniform3f(this.uUp, up[0], up[1], up[2]);
    gl.uniform1ui(this.uTick, tick >>> 0);
    gl.uniform1f(this.uGlowTau, this.glowTau);
    gl.uniform1f(this.uPointRadius, this.pointRadius);

    // 6 vertices per instance, N instances (instanced billboards).
    gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, this.n);

    gl.bindVertexArray(null);
  }

  destroy(): void {
    const gl = this.gl;
    gl.deleteBuffer(this.posBuf);
    gl.deleteBuffer(this.lastSpikeBuf);
    gl.deleteBuffer(this.vBuf);
    gl.deleteVertexArray(this.vao);
    gl.deleteProgram(this.program);
  }
}
