// Renderer (Phase 3). The wasm backend owns the live WebGPU canvas surface.
// This wrapper intentionally stays passive during GPU boot so it does not
// acquire a second WebGPU/WebGL/2D context on the same canvas.
//
// Architecture §5 / phase-3 spec: render resources (pipelines, bind groups,
// depth targets, uniform buffers) are created ONCE or on resize — never per
// frame. The wasm backend owns canvas context configuration and all GPU objects.

import type { Camera } from "./camera";

/** Minimal interface the renderer needs from the wasm backend. */
export interface RenderBackend {
  /** Advance simulation ticks. */
  tick?: (ticks: number, excitability: number) => void;
}

export class Renderer {
  constructor(_canvas: HTMLCanvasElement) {}

  async init(): Promise<void> {
    console.log("[renderer] passive startup renderer ready");
  }

  /** Compatibility no-op; live glow settings are pushed to the wasm backend. */
  setGlowTau(_tau: number): void {}
  /** Compatibility no-op; live point radius settings are pushed to the wasm backend. */
  setPointRadius(_r: number): void {}

  /**
   * Render one frame.
   *
   * Compatibility no-op for callers that still ask the renderer to paint while
   * the wasm backend is pending.
   */
  render(
    _camera: Camera,
    _tick: number,
    _wasmBackend?: unknown,
  ): void {
    // The startup overlay and CSS canvas background provide the visible
    // pre-backend state. Creating any canvas context here can prevent the wasm
    // backend from claiming the WebGPU surface later.
  }
}
