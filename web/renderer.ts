// Renderer stub (BV7). Phase 1: clears the canvas to black each frame. Real
// WebGPU/WebGL2 LOD rendering lands in phase 3. Tries WebGPU first, falls back
// to WebGL2 clear, then 2D clear — so the canvas reliably shows *something*
// (black) regardless of device support.

export class Renderer {
  private gpuCtx: GPUCanvasContext | null = null;
  private gpuDevice: GPUDevice | null = null;
  private gl: WebGL2RenderingContext | null = null;
  private ctx2d: CanvasRenderingContext2D | null = null;

  constructor(private canvas: HTMLCanvasElement) {}

  async init(): Promise<void> {
    // WebGPU path.
    const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
    if (gpu) {
      try {
        const adapter = await gpu.requestAdapter();
        if (adapter) {
          this.gpuDevice = await adapter.requestDevice();
          this.gpuCtx = this.canvas.getContext(
            "webgpu",
          ) as GPUCanvasContext | null;
          if (this.gpuCtx) {
            this.gpuCtx.configure({
              device: this.gpuDevice,
              format: gpu.getPreferredCanvasFormat(),
              alphaMode: "opaque",
            });
            console.log("[renderer] WebGPU context ready (clear-only stub)");
            return;
          }
        }
      } catch (e) {
        console.warn("[renderer] WebGPU init failed, falling back:", e);
      }
    }
    // WebGL2 fallback.
    this.gl = this.canvas.getContext("webgl2");
    if (this.gl) {
      console.log("[renderer] WebGL2 context ready (clear-only stub)");
      return;
    }
    // 2D fallback.
    this.ctx2d = this.canvas.getContext("2d");
    console.log("[renderer] 2D context fallback (clear-only stub)");
  }

  // render(): phase 1 clears to black. Accepts the (unused) backend render
  // state so the signature matches the rAF loop's eventual real call.
  render(): void {
    if (this.gpuCtx && this.gpuDevice) {
      const encoder = this.gpuDevice.createCommandEncoder();
      const view = this.gpuCtx.getCurrentTexture().createView();
      const pass = encoder.beginRenderPass({
        colorAttachments: [
          {
            view,
            clearValue: { r: 0, g: 0, b: 0, a: 1 },
            loadOp: "clear",
            storeOp: "store",
          },
        ],
      });
      pass.end();
      this.gpuDevice.queue.submit([encoder.finish()]);
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
}
