// Corner HUD (BV8 amendment — Phase 7).
//
// Small bottom-right fixed div. Updated ONCE per profiler dump (~1/sec) — NOT
// every rAF frame. Never triggers GPU readbacks, never allocates large strings
// in the hot path, and reads only already-aggregated counters.
//
// Layout:
//   fps: 58  |  N: 50k  |  GPU
//   syn/s est: 1.2B
//
// Optional debug fields (off by default, behind SHOW_DEBUG_HUD flag):
//   res: 0.75×  |  near: 321i  |  gpu_ms: 4.2  |  scale: shrink_n
//
// The HUD div is created lazily on first update() and never recreated.
// All text is plain strings — no innerHTML, no DOM tree rebuilding.
//
// Architecture §8 / BV8 amendment.

export interface HudStats {
  fps: number;
  n: number;
  backend: "gpu" | "cpu";
  synapticEventsPerSec: number;

  // Optional debug fields (only shown when debugEnabled = true):
  renderResScale?: number;
  nearLodInstances?: number;
  gpuTimingTotalMs?: number;
  scalerReason?: string;
}

export class CornerHud {
  private el: HTMLDivElement | null = null;
  private debugEnabled: boolean;

  constructor(debugEnabled = false) {
    this.debugEnabled = debugEnabled;
  }

  /**
   * Update the HUD with the latest profiler snapshot.
   * Call at most once per second (on profiler dump), never per-frame.
   */
  update(stats: HudStats): void {
    const el = this._getOrCreate();

    const nLabel = formatCount(stats.n);
    const synLabel = formatRate(stats.synapticEventsPerSec);
    const backendLabel = stats.backend.toUpperCase();

    let text =
      `fps: ${stats.fps.toFixed(0).padStart(3)}  |  N: ${nLabel}  |  ${backendLabel}\n` +
      `syn/s est: ${synLabel}`;

    if (this.debugEnabled) {
      const parts: string[] = [];
      if (stats.renderResScale !== undefined) {
        parts.push(`res: ${stats.renderResScale.toFixed(2)}×`);
      }
      if (stats.nearLodInstances !== undefined) {
        parts.push(`near: ${stats.nearLodInstances}i`);
      }
      if (stats.gpuTimingTotalMs !== undefined) {
        parts.push(`gpu_ms: ${stats.gpuTimingTotalMs.toFixed(1)}`);
      }
      if (stats.scalerReason) {
        parts.push(`scale: ${stats.scalerReason}`);
      }
      if (parts.length > 0) {
        text += "\n" + parts.join("  |  ");
      }
    }

    el.textContent = text;
  }

  /** Toggle debug fields on/off at runtime (no DOM recreation). */
  setDebugEnabled(v: boolean): void {
    this.debugEnabled = v;
  }

  /** Hide the HUD (e.g. on mobile where debug overlays are disabled). */
  hide(): void {
    if (this.el) this.el.style.display = "none";
  }

  /** Show the HUD after a hide() call. */
  show(): void {
    if (this.el) this.el.style.display = "";
  }

  // ── Private ────────────────────────────────────────────────────────────────

  private _getOrCreate(): HTMLDivElement {
    if (this.el) return this.el;

    const div = document.createElement("div");
    div.id = "corner-hud";
    // Styling per spec: fixed, bottom-right, monospace, semi-transparent.
    div.style.cssText = [
      "position:fixed",
      "bottom:8px",
      "right:8px",
      "font-family:monospace",
      "font-size:11px",
      "color:rgba(255,255,255,0.5)",
      "background:rgba(0,0,0,0.35)",
      "padding:4px 6px",
      "border-radius:3px",
      "pointer-events:none",
      "white-space:pre",
      "line-height:1.4",
      "z-index:100",
      // No debug overlays or timestamp-query HUD by default (Phase 7 spec).
    ].join(";");
    document.body.appendChild(div);
    this.el = div;
    return div;
  }
}

// ── Formatting helpers ────────────────────────────────────────────────────────

/** Format a neuron count: 1200 → "1.2k", 1500000 → "1.5M". */
function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000)     return `${(n / 1_000).toFixed(0)}k`;
  return `${n}`;
}

/** Format a synaptic-events/sec rate: 1_200_000_000 → "1.2B", 12_000_000 → "12M". */
function formatRate(r: number): string {
  if (r >= 1e9) return `${(r / 1e9).toFixed(1)}B`;
  if (r >= 1e6) return `${(r / 1e6).toFixed(1)}M`;
  if (r >= 1e3) return `${(r / 1e3).toFixed(0)}k`;
  return `${r.toFixed(0)}`;
}
