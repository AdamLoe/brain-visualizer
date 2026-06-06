// CPU sim coordinator Web Worker (Phase 6, BV24).
//
// Ownership boundary (BV24): this worker owns the WASM instance, the rayon pool,
// the CPU sim state, tick scheduling, and the active lists. The main thread owns
// input, controls, WebGL2 rendering, and the profiler/HUD. Backend/speed/
// excitability changes arrive as messages; sim work never runs on the main
// thread (except tiny startup self-test code).
//
// The WASM linear memory is a SharedArrayBuffer (when cross-origin isolated +
// threaded build), so the main thread can build Float32Array/Uint32Array views
// over v_render[]/last_spike[]/positions[] by pointer/len without copying. The
// worker posts the pointers + the shared memory once after init; thereafter it
// only posts "ticked" notifications (the data is already visible via the SAB).
//
// Graceful fallback (done-when): if cross-origin isolation / threads are
// unavailable, the worker still runs the CPU backend SINGLE-THREADED (the wasm
// build without the threaded pool is correct, just slower) and reports it.
//
// This typechecks + bundles headless. The runtime path needs a browser (manual
// TODO — no browser in the build env). The pkg import is dynamic so the bundle
// builds even before `wasm-pack build` has produced pkg/.

/* eslint-disable @typescript-eslint/no-explicit-any */

interface InitMsg {
  type: "init";
  n: number;
  k: number;
  seed: number;
  iExt: number;
  synapticScale: number;
  requestedThreads: number;
}
interface TickMsg { type: "tick"; ticks: number; excitability: number }
interface StimMsg { type: "stim"; x: number; y: number; z: number; radius: number; current: number }
interface DestroyMsg { type: "destroy" }
type InMsg = InitMsg | TickMsg | StimMsg | DestroyMsg;

let backend: any = null;
let wasm: any = null;
let neuronCount = 0;

async function loadWasm(): Promise<any> {
  // Dynamic import keeps the bundle building before pkg/ exists.
  const mod = await import("../../../crates/brain-visualizer/pkg/brain_visualizer.js");
  await mod.default();
  return mod;
}

async function handleInit(msg: InitMsg): Promise<void> {
  wasm = await loadWasm();

  // Try to spin up the threaded rayon pool. Present only in the threaded build
  // (cpu-threads feature + nightly + build-std). Absent → single-threaded.
  let threaded = false;
  if (typeof wasm.init_cpu_thread_pool === "function" && (globalThis as any).crossOriginIsolated) {
    try {
      await wasm.init_cpu_thread_pool(msg.requestedThreads);
      threaded = true;
    } catch (e) {
      console.warn("[cpu-worker] thread pool init failed, single-threaded:", e);
    }
  }

  backend = new wasm.WasmCpuBackend(
    msg.n, msg.k, msg.seed >>> 0, msg.iExt, msg.synapticScale,
  );
  neuronCount = backend.neuron_count();

  // Share the wasm memory + the SoA pointers so the main thread can build views.
  const memory: WebAssembly.Memory = wasm.memory ?? (wasm as any).__wbindgen_export_0;
  (self as any).postMessage({
    type: "ready",
    threaded,
    neuronCount,
    memory,
    vRenderPtr: backend.v_render_ptr(),
    lastSpikePtr: backend.last_spike_ptr(),
    positionsPtr: backend.positions_ptr(),
  });
}

self.onmessage = async (ev: MessageEvent<InMsg>) => {
  const msg = ev.data;
  switch (msg.type) {
    case "init":
      await handleInit(msg);
      break;
    case "tick": {
      if (!backend) return;
      const spikes = backend.tick(msg.ticks, msg.excitability);
      // Pointers can change if a Vec reallocates; resend them each tick is
      // cheap. The data itself is already in the shared memory.
      (self as any).postMessage({
        type: "ticked",
        spikes,
        tick: backend.tick_count(),
        vRenderPtr: backend.v_render_ptr(),
        lastSpikePtr: backend.last_spike_ptr(),
      });
      break;
    }
    case "stim":
      if (backend) backend.stimulate(msg.x, msg.y, msg.z, msg.radius, msg.current);
      break;
    case "destroy":
      if (backend) {
        backend.destroy();
        backend = null;
      }
      break;
  }
};
