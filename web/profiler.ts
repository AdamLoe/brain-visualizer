// JS-side profiler mirror (BV8). The canonical profiler lives in Rust
// (src/profiler.rs) and will own counters once the backend produces real
// stats; phase 1 mirrors its JSON shape here so the rAF loop can dump
// per-second counters to the console with no wasm round-trip while ticks are
// stubbed. Allocation-light: fixed-size frame-time ring, one string/sec.

import type { BackendKind, Tier, TickStats } from "./types";

const RING_CAP = 120;

export class Profiler {
  private frameTimes = new Float32Array(RING_CAP);
  private ringLen = 0;
  private ringHead = 0;

  private windowTicks = 0;
  private windowSpikes = 0;
  private windowSyn = 0;
  private framesThisWindow = 0;
  private lastDumpMs = 0;
  private started = false;

  constructor(
    private backend: BackendKind,
    private tier: Tier,
    private n: number,
    private k: number,
  ) {}

  setConfig(backend: BackendKind, tier: Tier, n: number, k: number): void {
    this.backend = backend;
    this.tier = tier;
    this.n = n;
    this.k = k;
  }

  recordFrame(nowMs: number, frameMs: number, stats: TickStats): void {
    if (!this.started) {
      this.started = true;
      this.lastDumpMs = nowMs;
    }
    this.frameTimes[this.ringHead] = frameMs;
    this.ringHead = (this.ringHead + 1) % RING_CAP;
    if (this.ringLen < RING_CAP) this.ringLen++;

    this.windowTicks += stats.tickCount;
    this.windowSpikes += stats.spikes;
    this.windowSyn += stats.synapticEvents;
    this.framesThisWindow++;
  }

  /**
   * Dump one profiler snapshot per second to the console.
   * Returns true when a dump was emitted (so the rAF loop can trigger the
   * adaptive scaler exactly once per second).
   */
  maybeDump(nowMs: number): boolean {
    if (!this.started) return false;
    const elapsedMs = nowMs - this.lastDumpMs;
    if (elapsedMs < 1000) return false;
    const elapsedS = elapsedMs / 1000;

    const snapshot = {
      fps: +(this.framesThisWindow / elapsedS).toFixed(1),
      frame_ms_avg: +this.avg().toFixed(3),
      frame_ms_p95: +this.percentile(95).toFixed(3),
      ticks_per_sec: +(this.windowTicks / elapsedS).toFixed(1),
      spikes_per_sec: +(this.windowSpikes / elapsedS).toFixed(1),
      synaptic_events_per_sec: +(this.windowSyn / elapsedS).toFixed(1),
      backend: this.backend,
      tier: this.tier,
      n: this.n,
      k: this.k,
    };
    console.log(JSON.stringify(snapshot));

    this.windowTicks = 0;
    this.windowSpikes = 0;
    this.windowSyn = 0;
    this.framesThisWindow = 0;
    this.lastDumpMs = nowMs;
    return true;
  }

  /** Return the p95 frame time from the rolling window (ms). Used by scaler. */
  getFrameP95(): number {
    return this.percentile(95);
  }

  private avg(): number {
    if (this.ringLen === 0) return 0;
    let sum = 0;
    for (let i = 0; i < this.ringLen; i++) sum += this.frameTimes[i];
    return sum / this.ringLen;
  }

  private percentile(p: number): number {
    if (this.ringLen === 0) return 0;
    const v = Array.from(this.frameTimes.slice(0, this.ringLen)).sort(
      (a, b) => a - b,
    );
    const rank = Math.round((p / 100) * (this.ringLen - 1));
    return v[Math.min(rank, this.ringLen - 1)];
  }
}
