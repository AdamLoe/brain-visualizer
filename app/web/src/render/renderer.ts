// Renderer (Phase 3). Wires the WebGPU canvas context to the wasm backend's
// render path. Keeps the manifold dark mesh + far-LOD glow pipeline running
// each frame. Falls back to a clear-only black frame if WebGPU is unavailable
// or the backend has not been initialised yet.
//
// Architecture §5 / phase-3 spec: render resources (pipelines, bind groups,
// depth targets, uniform buffers) are created ONCE or on resize — never per
// frame. This class owns the canvas context configuration and the per-frame
// call sequence; the wasm backend owns all GPU objects.

import type { Camera } from "./camera";

/** Minimal interface the renderer needs from the wasm backend. */
export interface RenderBackend {
  /** Advance simulation ticks. */
  tick?: (ticks: number, excitability: number) => void;
}

export class Renderer {
  private gpuCtx: GPUCanvasContext | null = null;
  private gpuDevice: GPUDevice | null = null;
  private gl: WebGL2RenderingContext | null = null;
  private ctx2d: CanvasRenderingContext2D | null = null;
  private canvasFormat: GPUTextureFormat | null = null;

  // Render parameters (set once after init, updated on changes).
  private glowTau = 100.0;    // ~100 ticks glow decay (100ms biological)
  private pointRadius = 0.012; // world units — tuned for N=50k

  constructor(private canvas: HTMLCanvasElement) {}

  async init(): Promise<void> {
    const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
    if (gpu) {
      try {
        const adapter = await gpu.requestAdapter({ powerPreference: "high-performance" });
        if (adapter) {
          this.gpuDevice = await adapter.requestDevice();
          this.gpuCtx = this.canvas.getContext("webgpu") as GPUCanvasContext | null;
          if (this.gpuCtx) {
            this.canvasFormat = gpu.getPreferredCanvasFormat();
            this.gpuCtx.configure({
              device: this.gpuDevice,
              format: this.canvasFormat,
              alphaMode: "opaque",
            });
            console.log("[renderer] WebGPU context ready (phase-3 glow pipeline)");
            return;
          }
        }
      } catch (e) {
        console.warn("[renderer] WebGPU init failed, falling back:", e);
      }
    }
    this.gl = this.canvas.getContext("webgl2");
    if (this.gl) {
      console.log("[renderer] WebGL2 fallback (clear-only — phase-3 requires WebGPU)");
      return;
    }
    this.ctx2d = this.canvas.getContext("2d");
    console.log("[renderer] 2D canvas fallback (clear-only)");
  }

  /** Set glow decay constant (ticks). Default 100. */
  setGlowTau(tau: number): void { this.glowTau = tau; }
  /** Set billboard radius (world units). Default 0.012. */
  setPointRadius(r: number): void { this.pointRadius = r; }

  /**
   * Render one frame.
   *
   * @param camera - Camera for MVP matrix, camera_right, camera_up.
   * @param tick   - Current sim tick counter (passed into glow recency shader).
   * @param wasmBackend - Optional wasm-exported backend object that exposes a
   *   `render(view, mvpPtr, camera_right_x, ..., tick, ...)` method.
   *   In the browser this will be the real wasm GpuBackend.
   *   When null, clears to black (stub mode for environments without the wasm backend).
   */
  render(
    camera: Camera,
    _tick: number,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    _wasmBackend?: any,
  ): void {
    if (this.gpuCtx && this.gpuDevice) {
      // Phase 3: The wasm backend owns the WebGPU device and pipelines. In the
      // browser, the wasm backend's render() is called directly from main.ts
      // after tick(). This thin wrapper provides the canvas texture view so
      // the caller can route it through the wasm bridge.
      //
      // If a wasmBackend with a render method is provided, delegate to it.
      // Otherwise clear to black (stub / fallback).
      if (_wasmBackend && typeof _wasmBackend.render === "function") {
        try {
          const view = this.gpuCtx.getCurrentTexture().createView();
          const mvp = camera.mvpMatrix();
          const right = camera.cameraRight();
          const up = camera.cameraUp();
          _wasmBackend.render(
            view,
            mvp,
            right[0], right[1], right[2],
            up[0], up[1], up[2],
            this.glowTau,
            this.pointRadius,
          );
        } catch (e) {
          console.warn("[renderer] wasm render call failed:", e);
          this._clearBlack();
        }
        return;
      }
      // Stub clear (no wasm backend yet / wasm backend uses its own submit).
      this._clearBlack();
      return;
    }
    if (this.gl) {
      this.gl.clearColor(0, 0, 0, 1);
      this.gl.clear(this.gl.COLOR_BUFFER_BIT);
      return;
    }
    if (this.ctx2d) {
      this.ctx2d.fillStyle = "black";
      this.ctx2d.fillRect(0, 0, this.canvas.width, this.canvas.height);
    }
  }

  private _clearBlack(): void {
    if (!this.gpuCtx || !this.gpuDevice) return;
    const enc = this.gpuDevice.createCommandEncoder();
    const view = this.gpuCtx.getCurrentTexture().createView();
    const pass = enc.beginRenderPass({
      colorAttachments: [{
        view,
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear",
        storeOp: "store",
      }],
    });
    pass.end();
    this.gpuDevice.queue.submit([enc.finish()]);
  }
}
