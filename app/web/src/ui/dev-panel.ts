import "./dev-panel.css";
// web/dev-panel.ts — V2 Phase A / Phase B / Phase C / UX overhaul
//
// DevPanel: a hidden-by-default right-docked drawer for development diagnostics.
//
// Open/close triggers:
//   1. URL ?dev=1 opens it at boot.
//   2. Backtick key (`) toggles it.
//   3. A small bottom-right "·" affordance (24px, low opacity) toggles it.
//
// Layout: a fixed right-side drawer (PANEL_WIDTH_PX = 360px), dark/monospace,
// pointer-events only on the panel itself (canvas interaction unaffected when
// closed). main.ts listens via onVisibilityChange to shrink the canvas.
//
// Tabs: Monitor · Dynamics · Network · Rendering · Debug View · Storage
//   - Monitor has live metrics content (Phase A) + System section (UX overhaul).
//   - Rendering has live visual-knob controls (Phase B).
//   - Storage has reset + localStorage readout (Phase B).
//   - Dynamics has live E/I, branching-ratio, cascade readouts (Phase C).
//   - Network has Simulation controls (UX overhaul — brain state, speed, scale).
//   - Debug View shows current visual mode readout.
//
// Monitor tab — Silent/Tuned/Overactive classifier thresholds.
// NOTE: pctFired* metrics are FRACTIONS in [0,1] (count/N), not percentages.
//   SILENT:     pctFired500ms < 0.005 (<0.5% firing — flat/dead)
//   OVERACTIVE: pctFired100ms > 0.30  (>30% in 100ms — runaway/seizure-like)
//             OR branchingRatio > 1.5 (avalanche propagation > 1:1)
//   TUNED:      everything else       (healthy critical dynamics)
//
// Architecture note: update(m, sys?) is called from the once-per-second block
// in rafLoop (main.ts) ONLY when isOpen() is true — avoids unnecessary work.

import type { Metrics } from "../core/settings";
import { getSettings, resetSettings, setSetting } from "../core/settings";
import { subscribe } from "../core/settings";
import {
  impactColor,
  impactLabel,
  type SettingImpact,
} from "../core/setting-metadata";
// UX round 2: BRAIN_STATES / TIER_PRESETS no longer used in the panel
// (presets removed). Controls.ts exports are kept for external consumers.

// ── UX overhaul: SysInfo interface (exported — main.ts uses it) ──────────────

/** System-level info passed to update() from the rAF loop. */
export interface SysInfo {
  /** Current neuron count. */
  n: number;
  /** Fan-out (synapses per neuron). */
  k: number;
  /** Current rendered frames per second. */
  fps: number;
  /** Simulation ticks executed per second. */
  ticksPerSec: number;
  /** Theoretical maximum ticks/s for this hardware/backend. */
  maxTicksPerSec: number;
}

// ── Tab definitions ──────────────────────────────────────────────────────────

type TabId = "monitor" | "dynamics" | "network" | "rendering" | "debugview" | "storage";

const TABS: { id: TabId; label: string }[] = [
  { id: "monitor",    label: "Monitor"   },
  { id: "dynamics",   label: "Dynamics"  },
  { id: "network",    label: "Network"   },
  { id: "rendering",  label: "Rendering" },
  { id: "debugview",  label: "Debug View"},
  { id: "storage",    label: "Storage"   },
];

// ── Classifier ────────────────────────────────────────────────────────────────

type NetworkState = "SILENT" | "TUNED" | "OVERACTIVE";

// Thresholds (documented in module-level comment above).
const SILENT_THRESHOLD_PCT500MS   = 0.005; // <0.5% fired in 500ms → SILENT
const OVERACTIVE_THRESHOLD_PCT100 = 0.30;  // >30% fired in 100ms → OVERACTIVE
const OVERACTIVE_BRANCHING_RATIO  = 1.5;   // branchingRatio above this → OVERACTIVE

// V2 Phase C: Dynamics tab — branching-ratio critical band thresholds.
const BRANCH_SUBCRITICAL  = 0.9;  // σ < 0.9 → subcritical (fading)
const BRANCH_SUPERCRITICAL = 1.1; // σ > 1.1 → supercritical (runaway)

function classify(m: Metrics): NetworkState {
  if (m.pctFired500ms < SILENT_THRESHOLD_PCT500MS) return "SILENT";
  if (m.pctFired100ms > OVERACTIVE_THRESHOLD_PCT100 || m.branchingRatio > OVERACTIVE_BRANCHING_RATIO) {
    return "OVERACTIVE";
  }
  return "TUNED";
}

// ── Voltage histogram sparkline ───────────────────────────────────────────────
// Renders the 16-bin voltageHistogram as a unicode block sparkline.
// Bins are scaled relative to the maximum bin value.

const SPARK_BLOCKS = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

function voltageSparkline(histogram: number[]): string {
  const max = Math.max(...histogram, 1e-9); // avoid /0
  return histogram.map((v) => {
    const idx = Math.min(
      SPARK_BLOCKS.length - 1,
      Math.floor((v / max) * SPARK_BLOCKS.length),
    );
    return SPARK_BLOCKS[Math.max(0, idx)];
  }).join("");
}

// ── V2 Phase B: Slider spec ───────────────────────────────────────────────────
// Describes each numeric setting rendered as a range input.

interface SliderSpec {
  key: keyof import("../core/settings").VisualizerSettings;
  label: string;
  min: number;
  max: number;
  step: number;
  decimals: number; // digits after decimal in the readout
  tooltip?: string; // one-sentence description shown via the instant tooltip
  // Morphology controls: when true, apply on 'change' (release/Enter) instead of
  // 'input' — used by curve-lift, which regenerates geometry on every apply.
  changeOnly?: boolean;
}

// ── V2 Phase B: Select spec ───────────────────────────────────────────────────
// Describes each integer-enum setting rendered as a <select>.

interface SelectSpec {
  key: keyof import("../core/settings").VisualizerSettings;
  label: string;
  options: { value: number; label: string }[];
  tooltip?: string; // one-sentence description shown via the instant tooltip
}

// ── Apply-handler callbacks (provided by main.ts) ─────────────────────────────
// V2 Phase B — wired in main.ts after boot().

export interface ApplyHandlers {
  onBrainReset: () => void;
}

// ── UX overhaul: SimHandlers (provided by main.ts via setSimHandlers) ────────

/** Sim-control callbacks set via setSimHandlers(). UX round 2. */
export interface SimHandlers {
  /** Called when excitability slider changes (0–1). */
  onExcitability: (v: number) => void;
  /** Called when speed slider changes (ticks/sec, 1–60). */
  onSpeed: (tps: number) => void;
  /** Called when N/K/seed changes or Regenerate is pressed (triggers network rebuild). */
  onNetwork: (params: { n: number; k: number; seed: number }) => void;
}

// ── Formatting helpers ────────────────────────────────────────────────────────

function fmtRate(r: number): string {
  if (r >= 1e9) return `${(r / 1e9).toFixed(2)}B`;
  if (r >= 1e6) return `${(r / 1e6).toFixed(2)}M`;
  if (r >= 1e3) return `${(r / 1e3).toFixed(1)}k`;
  return r.toFixed(0);
}

/** Format an integer with locale-style comma grouping. */
function fmtInt(n: number): string {
  return Math.round(n).toLocaleString();
}

// ── DevPanel class ────────────────────────────────────────────────────────────

export class DevPanel {
  // UX overhaul: fixed panel width — main.ts reads this to shrink the canvas.
  static readonly PANEL_WIDTH_PX = 360;

  private container: HTMLDivElement;
  private activeTab: TabId = "monitor";
  private tabContents: Map<TabId, HTMLDivElement> = new Map();
  private tabButtons: Map<TabId, HTMLButtonElement> = new Map();
  private monitorFields: {
    verdict:        HTMLSpanElement;
    spikesThisTick: HTMLSpanElement;
    spikesPerSec:   HTMLSpanElement;
    meanFiringRate: HTMLSpanElement;
    synEventsPerSec:HTMLSpanElement;
    pct100:         HTMLSpanElement;
    pct500:         HTMLSpanElement;
    pct2s:          HTMLSpanElement;
    inputSpikes:    HTMLSpanElement;
    assocSpikes:    HTMLSpanElement;
    outputSpikes:   HTMLSpanElement;
    eSpikes:        HTMLSpanElement;
    iSpikes:        HTMLSpanElement;
    meanVoltage:    HTMLSpanElement;
    branchingRatio: HTMLSpanElement;
    accumHW:        HTMLSpanElement;
    sparkline:      HTMLSpanElement;
    // UX overhaul: system section
    sysNeurons:       HTMLSpanElement | null;
    sysConnections:   HTMLSpanElement | null;
    sysFps:           HTMLSpanElement | null;
    sysTicksPerSec:   HTMLSpanElement | null;
    sysMaxTicks:      HTMLSpanElement | null;
    sysInputEnergy:   HTMLSpanElement | null;
    sysOutputEnergy:  HTMLSpanElement | null;
  } | null = null;

  // V2 Phase C: dynamics-tab live fields.
  private dynamicsFields: {
    eiRatio:            HTMLSpanElement;
    eiBarE:             HTMLDivElement;
    eiBarI:             HTMLDivElement;
    branchValue:        HTMLSpanElement;
    branchBand:         HTMLSpanElement;
    inputSpikes:        HTMLSpanElement;
    assocSpikes:        HTMLSpanElement;
    outputSpikes:       HTMLSpanElement;
    cascadeSize:        HTMLSpanElement;
    pct100:             HTMLSpanElement;
    pct500:             HTMLSpanElement;
    pct2s:              HTMLSpanElement;
    interpret:          HTMLSpanElement;
  } | null = null;

  // V2 Phase B: rendering-tab slider elements (for syncing on external changes).
  private sliderElements: Map<string, { input: HTMLInputElement; readout: HTMLSpanElement }> = new Map();

  // V2 Phase E: debug view — live visual-mode readout spans.
  private debugViewFields: {
    colorBy:          HTMLSpanElement;
    neuronVisibility: HTMLSpanElement;
    connectionLayer:  HTMLSpanElement;
    surface:          HTMLSpanElement;
    signalSource:     HTMLSpanElement;
  } | null = null;

  // V2 Phase B: apply handlers (kept for API compat; brain-reset pending UI removed UX round 2).
  private applyHandlers: ApplyHandlers | null = null;

  // UX overhaul: sim handlers for Network tab controls.
  private simHandlers: SimHandlers | null = null;

  // UX overhaul: visibility callback(s) for main.ts canvas shrinking.
  private visibilityCallbacks: Array<(open: boolean) => void> = [];

  // UX round 2: initial values set by main.ts via setInitialValues().
  private _initN = 10_000;
  private _initK = 16;
  private _initSeed = 0;
  private _initExcitability = 0.71;
  private _initTps = 30;

  // V2 Phase B: unsubscribe function returned by subscribe().
  // Called in destroy() to clean up when the panel is removed.
  private readonly _unsubSettings: () => void;

  // V2 Phase B: localStorage key (from settings.ts — kept in sync manually).
  private static readonly LS_KEY = "bv2_settings_v1";

  // v0.1.2: instant-tooltip floating element (appended to <body>, not the panel,
  // so the panel's scroll overflow can't clip it). Shown with ZERO delay on
  // hover of any element carrying a `data-tip` attribute; positioned to the left
  // of the right-docked panel and clamped to the viewport. A single delegated
  // pair of listeners on `document` drives it (keyed off `data-tip`).
  private _tipEl: HTMLDivElement | null = null;

  constructor() {
    // UX round 2: affordance circle removed; gear button is the only opener.
    // Build the main panel container.
    this.container = this._buildPanel();
    document.body.appendChild(this.container);

    // v0.1.2: build the instant-tooltip element + wire delegated listeners.
    this._buildTooltip();

    // Open via ?dev=1.
    if (new URLSearchParams(window.location.search).get("dev") === "1") {
      this._setOpen(true);
    }

    // Toggle on backtick key (harmless, invisible to normal users).
    window.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "`" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        this.toggle();
      }
    });

    // V2 Phase B: subscribe to settings changes to sync slider positions.
    // V2 Phase E: also refresh debug view readouts on external changes.
    // Stored so destroy() can unsubscribe cleanly.
    this._unsubSettings = subscribe((s) => {
      this._syncSliders(s);
      this._updateDebugViewFields(s); // V2 Phase E
    });
  }

  // ── UX overhaul: public open/close/toggle API ──────────────────────────────

  /** Open the dev panel and notify visibility callbacks. */
  open(): void {
    this._setOpen(true);
  }

  /** Close the dev panel and notify visibility callbacks. */
  close(): void {
    this._setOpen(false);
  }

  /** Toggle the dev panel open/closed and notify visibility callbacks. */
  toggle(): void {
    this._setOpen(!this.isOpen());
  }

  /** Register a callback fired on every open/close (from any trigger). */
  onVisibilityChange(cb: (open: boolean) => void): void {
    this.visibilityCallbacks.push(cb);
  }

  /**
   * Wire the sim-control handlers used by the Network tab.
   * Must be called before the Network tab is opened for the first time.
   */
  setSimHandlers(h: SimHandlers): void {
    this.simHandlers = h;
  }

  /**
   * UX round 2: Seed initial control values from the live app config.
   * Call after setSimHandlers so the Network tab shows current state.
   */
  setInitialValues(opts: {
    n: number; k: number; seed: number;
    excitability: number; tps: number;
  }): void {
    this._initN           = opts.n;
    this._initK           = opts.k;
    this._initSeed        = opts.seed;
    this._initExcitability = opts.excitability;
    this._initTps         = opts.tps;

    // The Network tab is built eagerly in the constructor, before main.ts calls
    // this method — so its sliders captured the field defaults, not the persisted
    // config. Rebuild that one tab's content now that the real values are set, so
    // the N / K / excitability / speed controls show what actually booted.
    // (Without this, persisted values are applied to the sim but the panel still
    // displays defaults, making persistence look broken.)
    const networkContent = this.tabContents.get("network");
    if (networkContent) {
      networkContent.replaceChildren();
      this._buildNetworkTab(networkContent);
    }
  }

  /** Called from main.ts once-per-second when the panel is open. */
  update(m: Metrics, sys?: SysInfo): void {
    if (!this.monitorFields) return;
    const f = this.monitorFields;

    const state = classify(m);
    f.verdict.textContent = state;
    f.verdict.className = `dp-verdict dp-verdict--${state.toLowerCase()}`;

    f.spikesThisTick.textContent  = m.spikesThisTick.toFixed(0);
    f.spikesPerSec.textContent    = fmtRate(m.spikesPerSec);
    f.meanFiringRate.textContent  = m.meanFiringRateHz.toFixed(2) + " Hz";
    f.synEventsPerSec.textContent = fmtRate(m.synapticEventsPerSec);

    // pctFired* are fractions in [0,1] → display as percentages.
    f.pct100.textContent   = (m.pctFired100ms * 100).toFixed(2) + "%";
    f.pct500.textContent   = (m.pctFired500ms * 100).toFixed(2) + "%";
    f.pct2s.textContent    = (m.pctFired2s    * 100).toFixed(2) + "%";

    f.inputSpikes.textContent  = m.inputSpikes.toFixed(0);
    f.assocSpikes.textContent  = m.assocSpikes.toFixed(0);
    f.outputSpikes.textContent = m.outputSpikes.toFixed(0);

    f.eSpikes.textContent = m.eSpikes.toFixed(0);
    f.iSpikes.textContent = m.iSpikes.toFixed(0);

    f.meanVoltage.textContent   = m.meanMembraneVoltage.toFixed(4);
    f.branchingRatio.textContent = m.branchingRatio.toFixed(3);
    f.accumHW.textContent       = m.currentAccumulatorHighWater.toFixed(4);

    f.sparkline.textContent = voltageSparkline(m.voltageHistogram);

    // UX overhaul: update System section if sys provided.
    if (sys !== undefined) {
      if (f.sysNeurons)     f.sysNeurons.textContent     = fmtInt(sys.n);
      if (f.sysConnections) f.sysConnections.textContent = fmtInt(sys.n * sys.k);
      if (f.sysFps)         f.sysFps.textContent         = sys.fps.toFixed(1);
      if (f.sysTicksPerSec) f.sysTicksPerSec.textContent = fmtRate(sys.ticksPerSec);
      if (f.sysMaxTicks)    f.sysMaxTicks.textContent    = fmtRate(sys.maxTicksPerSec);
      // Input energy: input-region spikes per tick (proxy for input drive activity).
      if (f.sysInputEnergy)  f.sysInputEnergy.textContent  = m.inputSpikes.toFixed(1) + " sp/tick";
      // Output energy: output-region spikes per tick (proxy for readout activity).
      if (f.sysOutputEnergy) f.sysOutputEnergy.textContent = m.outputSpikes.toFixed(1) + " sp/tick";
    }

    // V2 Phase C: also refresh Dynamics tab fields (cheap text writes).
    this._updateDynamicsFields(m);
  }

  /** Returns true when the panel is currently visible. */
  isOpen(): boolean {
    return this.container.classList.contains("dp--open");
  }

  // V2 Phase B: clean up subscriptions (call if the panel is ever removed).
  destroy(): void {
    this._unsubSettings();
  }

  // V2 Phase B: wire the brain-reset callbacks (kept for API compat; UX round 2
  // removes the pending-UI but the handler slot is kept in case callers still wire it).
  setApplyHandlers(handlers: ApplyHandlers): void {
    this.applyHandlers = handlers;
  }

  // V2 Phase B: called by main.ts after a brain reset completes (now a no-op; UX round 2).
  clearPendingBrainReset(): void {
    // No-op: brain-reset pending UI removed in UX round 2.
    void this.applyHandlers; // suppress unused warning
  }

  // ── Private: open/close internals ─────────────────────────────────────────

  private _setOpen(open: boolean): void {
    const was = this.isOpen();
    if (open) {
      this.container.classList.add("dp--open");
    } else {
      this.container.classList.remove("dp--open");
    }
    // Fire visibility callbacks only on actual state change.
    if (open !== was) {
      for (const cb of this.visibilityCallbacks) cb(open);
    }
  }

  // ── v0.1.2: instant tooltip system ─────────────────────────────────────────
  // A single floating element on <body> shown immediately on hover of any
  // element carrying a `data-tip` attribute. Two delegated listeners
  // (mouseover/mouseout) on `document` find the nearest [data-tip] ancestor and
  // show/hide the tip — no per-element listeners, no native `title` (which has a
  // ~1 s delay). Positioned to the LEFT of the right-docked panel (above the
  // hovered row when possible) and clamped into the viewport. The element lives
  // outside the panel's overflow-scroll container so it is never clipped.

  private _buildTooltip(): void {
    const tip = document.createElement("div");
    tip.className = "dp-tooltip";
    tip.style.display = "none";
    document.body.appendChild(tip);
    this._tipEl = tip;

    // Delegated show: walk up from the event target to the nearest [data-tip].
    document.addEventListener("mouseover", (e) => {
      const el = (e.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
      if (!el) return;
      const text = el.getAttribute("data-tip");
      if (!text) return;
      this._showTip(el, text);
    });

    // Delegated hide: hide whenever the pointer leaves a tipped element.
    document.addEventListener("mouseout", (e) => {
      const el = (e.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
      if (!el) return;
      this._hideTip();
    });
  }

  private _showTip(anchor: HTMLElement, text: string): void {
    const tip = this._tipEl;
    if (!tip) return;
    tip.textContent = text;
    tip.style.display = "block";

    // Measure after content is set.
    const a = anchor.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    const margin = 8;

    // Prefer placing the tip just LEFT of the panel, vertically aligned to the
    // hovered row; fall back to above the row if there isn't room on the left.
    let left = a.left - t.width - margin;
    let top = a.top;

    if (left < margin) {
      // Not enough room on the left → place above the row instead.
      left = a.left;
      top = a.top - t.height - margin;
      if (top < margin) top = a.bottom + margin; // …or below if no room above.
    }

    // Clamp into the viewport.
    left = Math.max(margin, Math.min(left, window.innerWidth - t.width - margin));
    top = Math.max(margin, Math.min(top, window.innerHeight - t.height - margin));

    tip.style.left = `${Math.round(left)}px`;
    tip.style.top = `${Math.round(top)}px`;
  }

  private _hideTip(): void {
    if (this._tipEl) this._tipEl.style.display = "none";
  }

  /**
   * Register instant-tooltip text on an element. Routes through the custom
   * floating-tooltip system (zero hover delay, not clipped by panel scroll).
   * Uses a `data-tip` attribute read by the delegated document listeners.
   */
  private _attachTip(el: HTMLElement, text: string): void {
    el.setAttribute("data-tip", text);
  }

  private _buildPanel(): HTMLDivElement {
    const panel = document.createElement("div");
    panel.id = "dev-panel";

    // ── Header ──────────────────────────────────────────────────────────────
    const header = document.createElement("div");
    header.className = "dp-header";

    const titleSpan = document.createElement("span");
    titleSpan.className = "dp-title";
    titleSpan.textContent = "DEV";
    header.appendChild(titleSpan);

    const closeBtn = document.createElement("button");
    closeBtn.className = "dp-close";
    closeBtn.textContent = "×";
    closeBtn.title = "Close (` or ·)";
    closeBtn.addEventListener("click", () => this.close());
    header.appendChild(closeBtn);

    panel.appendChild(header);

    // ── Tab bar ─────────────────────────────────────────────────────────────
    const tabBar = document.createElement("div");
    tabBar.className = "dp-tabbar";

    for (const tab of TABS) {
      const btn = document.createElement("button");
      btn.className = "dp-tab";
      btn.textContent = tab.label;
      btn.dataset.tabId = tab.id;
      if (tab.id === this.activeTab) btn.classList.add("dp-tab--active");
      btn.addEventListener("click", () => this._switchTab(tab.id));
      tabBar.appendChild(btn);
      this.tabButtons.set(tab.id, btn);
    }

    panel.appendChild(tabBar);

    // ── Tab contents ─────────────────────────────────────────────────────────
    const body = document.createElement("div");
    body.className = "dp-body";

    for (const tab of TABS) {
      const content = document.createElement("div");
      content.className = "dp-content";
      if (tab.id !== this.activeTab) content.style.display = "none";

      if (tab.id === "monitor") {
        this._buildMonitorTab(content);
      } else if (tab.id === "dynamics") {
        // V2 Phase C: live dynamics readouts.
        this._buildDynamicsTab(content);
      } else if (tab.id === "network") {
        // UX overhaul: real Network/Simulation controls.
        this._buildNetworkTab(content);
      } else if (tab.id === "rendering") {
        // V2 Phase B: real rendering tab with live knobs.
        this._buildRenderingTab(content);
      } else if (tab.id === "storage") {
        // V2 Phase B: storage tab with reset + readout.
        this._buildStorageTab(content);
      } else if (tab.id === "debugview") {
        // V2 Phase E: debug view — current visual mode readout.
        this._buildDebugViewTab(content);
      }

      body.appendChild(content);
      this.tabContents.set(tab.id, content);
    }

    panel.appendChild(body);
    return panel;
  }

  private _switchTab(id: TabId): void {
    // Hide previous.
    const prevContent = this.tabContents.get(this.activeTab);
    const prevBtn = this.tabButtons.get(this.activeTab);
    if (prevContent) prevContent.style.display = "none";
    if (prevBtn) prevBtn.classList.remove("dp-tab--active");

    // Show new.
    this.activeTab = id;
    const nextContent = this.tabContents.get(id);
    const nextBtn = this.tabButtons.get(id);
    if (nextContent) nextContent.style.display = "";
    if (nextBtn) nextBtn.classList.add("dp-tab--active");

    // V2 Phase B: refresh storage readout when switching to storage tab.
    if (id === "storage") this._refreshStorageReadout();
  }

  // ── Monitor tab DOM ────────────────────────────────────────────────────────

  private _buildMonitorTab(root: HTMLDivElement): void {
    // UX overhaul: System section at the top (populated by update(m, sys)).
    root.appendChild(this._sep("System"));
    const sysNeurons     = this._row(root, "Neurons",
      "Total neuron count in the current network.");
    const sysConnections = this._row(root, "Connections",
      "Total synaptic connections (N × K — fan-out per neuron times neuron count).");
    this._caption(root, "(N×K — synapses per neuron × neuron count)");
    const sysFps         = this._row(root, "FPS",
      "Rendered frames per second (measured in the rAF loop).");
    const sysTicksPerSec = this._row(root, "Ticks/s",
      "Simulation ticks executed per second.");
    const sysMaxTicks    = this._row(root, "Max ticks/s",
      "Theoretical maximum simulation ticks per second for this hardware/backend.");
    const sysInputEnergy = this._row(root, "Input energy",
      "Input-region spikes per tick — proxy for how much external drive is entering the network.");
    const sysOutputEnergy = this._row(root, "Output energy",
      "Output-region spikes per tick — proxy for how much activity is reaching the readout region.");

    // Verdict headline.
    root.appendChild(this._sep("Network State"));
    const verdictRow = document.createElement("div");
    verdictRow.className = "dp-verdict-row";
    this._attachTip(verdictRow,
      "Overall health verdict: SILENT (<0.5% fired in 500 ms), OVERACTIVE (>30% in 100 ms or branching ratio >1.5), otherwise TUNED (healthy critical dynamics).");
    const verdictLabel = document.createElement("span");
    verdictLabel.className = "dp-label";
    verdictLabel.textContent = "State";
    const verdictSpan = document.createElement("span");
    verdictSpan.className = "dp-verdict dp-verdict--tuned";
    verdictSpan.textContent = "TUNED";
    verdictRow.appendChild(verdictLabel);
    verdictRow.appendChild(verdictSpan);
    root.appendChild(verdictRow);

    root.appendChild(this._sep("Spike Activity"));
    const spikesThisTick  = this._row(root, "Spikes/tick",
      "Number of neurons that fired on the most recent simulation tick.");
    const spikesPerSec    = this._row(root, "Spikes/sec",
      "Total spikes emitted per second across the whole network.");
    const meanFiringRate  = this._row(root, "Mean rate",
      "Average per-neuron firing rate in Hz (spikes per neuron per second).");
    const synEventsPerSec = this._row(root, "Syn. events/sec",
      "Synaptic transmission events per second (≈ spikes/sec × fan-out K).");

    root.appendChild(this._sep("Recent Firing %"));
    const pct100 = this._row(root, "Last 100 ms",
      "Fraction of all neurons that fired at least once in the last 100 ms, shown as a percentage.");
    const pct500 = this._row(root, "Last 500 ms",
      "Fraction of all neurons that fired at least once in the last 500 ms, shown as a percentage.");
    const pct2s  = this._row(root, "Last 2 s",
      "Fraction of all neurons that fired at least once in the last 2 seconds, shown as a percentage.");

    root.appendChild(this._sep("Per-Region"));
    const inputSpikes  = this._row(root, "Input spikes",
      "Spikes per tick from the Input cortical region (where external drive enters).");
    const assocSpikes  = this._row(root, "Assoc spikes",
      "Spikes per tick from the Association region (the recurrent middle layer).");
    const outputSpikes = this._row(root, "Output spikes",
      "Spikes per tick from the Output region (the network's readout).");

    root.appendChild(this._sep("E/I Split"));
    const eSpikes = this._row(root, "E spikes",
      "Spikes per tick from excitatory neurons.");
    const iSpikes = this._row(root, "I spikes",
      "Spikes per tick from inhibitory neurons.");

    root.appendChild(this._sep("Dynamics"));
    const meanVoltage    = this._row(root, "Mean V_m",
      "Mean membrane potential across all neurons (normalized units).");
    const branchingRatio = this._row(root, "Branching ratio",
      "Avalanche branching ratio σ: mean number of descendant spikes each spike triggers (≈1 = critical, <1 fading, >1 runaway).");
    const accumHW        = this._row(root, "Accum. HW",
      "High-water mark of the fixed-point synaptic-current accumulator — how close the integer current accumulation came to saturating.");

    root.appendChild(this._sep("Voltage Histogram"));
    const sparkRow = document.createElement("div");
    sparkRow.className = "dp-spark-row";
    this._attachTip(sparkRow,
      "Distribution of membrane voltages across all neurons (16 bins, low to high) as a sparkline.");
    const sparkline = document.createElement("span");
    sparkline.className = "dp-sparkline";
    sparkline.textContent = "▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁"; // placeholder
    sparkRow.appendChild(sparkline);
    root.appendChild(sparkRow);

    // Classifier legend below sparkline.
    const legend = document.createElement("div");
    legend.className = "dp-legend";
    legend.innerHTML =
      `<span class="dp-verdict dp-verdict--silent">SILENT</span> &lt;0.5% / ` +
      `<span class="dp-verdict dp-verdict--tuned">TUNED</span> nominal / ` +
      `<span class="dp-verdict dp-verdict--overactive">OVERACTIVE</span> &gt;30% | BR&gt;1.5`;
    root.appendChild(legend);

    this.monitorFields = {
      verdict: verdictSpan,
      spikesThisTick,
      spikesPerSec,
      meanFiringRate,
      synEventsPerSec,
      pct100,
      pct500,
      pct2s,
      inputSpikes,
      assocSpikes,
      outputSpikes,
      eSpikes,
      iSpikes,
      meanVoltage,
      branchingRatio,
      accumHW,
      sparkline,
      // UX overhaul: system fields.
      sysNeurons,
      sysConnections,
      sysFps,
      sysTicksPerSec,
      sysMaxTicks,
      sysInputEnergy,
      sysOutputEnergy,
    };
  }

  // ── V2 Phase C: Dynamics tab DOM ──────────────────────────────────────────

  private _buildDynamicsTab(root: HTMLDivElement): void {

    // ── E/I Balance ──────────────────────────────────────────────────────────
    root.appendChild(this._sep("E / I Balance"));

    // Ratio row.
    const eiRatio = this._row(root, "E/I ratio",
      "Ratio of excitatory to inhibitory spikes this tick (~1 = balanced; high = E-dominated, low = I-dominated).");

    // Split bar row.
    const barRow = document.createElement("div");
    barRow.className = "dp-ei-bar-row";

    const barTrack = document.createElement("div");
    barTrack.className = "dp-ei-bar-track";

    const eiBarE = document.createElement("div");
    eiBarE.className = "dp-ei-bar-e";
    eiBarE.style.width = "50%";
    eiBarE.title = "Excitatory";

    const eiBarI = document.createElement("div");
    eiBarI.className = "dp-ei-bar-i";
    eiBarI.style.width = "50%";
    eiBarI.title = "Inhibitory";

    barTrack.appendChild(eiBarE);
    barTrack.appendChild(eiBarI);
    barRow.appendChild(barTrack);

    const barLegend = document.createElement("div");
    barLegend.className = "dp-ei-bar-legend";
    barLegend.innerHTML =
      `<span class="dp-ei-legend-e">E</span>` +
      `<span class="dp-ei-legend-i">I</span>`;
    barRow.appendChild(barLegend);

    root.appendChild(barRow);

    // ── Branching Ratio ───────────────────────────────────────────────────────
    root.appendChild(this._sep("Branching Ratio σ"));

    const branchValue = this._row(root, "σ value",
      "Branching ratio σ: mean descendant spikes per spike — the avalanche propagation factor (≈1 = critical).");

    const bandRow = document.createElement("div");
    bandRow.className = "dp-row";
    this._attachTip(bandRow,
      "Critical-band label for σ: subcritical (σ<0.9, fading), critical (0.9–1.1, healthy), or supercritical (σ>1.1, runaway).");
    const bandLbl = document.createElement("span");
    bandLbl.className = "dp-label";
    bandLbl.textContent = "Band";
    const branchBand = document.createElement("span");
    branchBand.className = "dp-branch-band dp-branch--sub";
    branchBand.textContent = "subcritical (fading)";
    bandRow.appendChild(bandLbl);
    bandRow.appendChild(branchBand);
    root.appendChild(bandRow);

    const bandLegend = document.createElement("div");
    bandLegend.className = "dp-legend";
    bandLegend.innerHTML =
      `<span class="dp-branch-band dp-branch--sub">σ&lt;0.9 subcritical</span> · ` +
      `<span class="dp-branch-band dp-branch--crit">0.9–1.1 critical</span> · ` +
      `<span class="dp-branch-band dp-branch--super">σ&gt;1.1 runaway</span>`;
    root.appendChild(bandLegend);

    // ── Per-Region Rates ──────────────────────────────────────────────────────
    root.appendChild(this._sep("Per-Region Spikes / tick"));
    const inputSpikes  = this._row(root, "Input",
      "Spikes per tick from the Input cortical region (where external drive enters).");
    const assocSpikes  = this._row(root, "Assoc",
      "Spikes per tick from the Association region (the recurrent middle layer).");
    const outputSpikes = this._row(root, "Output",
      "Spikes per tick from the Output region (the network's readout).");

    // ── Cascade / Avalanche Size ──────────────────────────────────────────────
    root.appendChild(this._sep("Cascade / Avalanche (approx)"));

    const cascadeSize = this._row(root, "Cascade size now",
      "Spikes on the current tick used as an approximation of the active avalanche size.");

    root.appendChild(this._sep("Spread over time (% neurons fired)"));
    const pct100 = this._row(root, "Last 100 ms",
      "Fraction of all neurons that fired at least once in the last 100 ms, shown as a percentage.");
    const pct500 = this._row(root, "Last 500 ms",
      "Fraction of all neurons that fired at least once in the last 500 ms, shown as a percentage.");
    const pct2s  = this._row(root, "Last 2 s",
      "Fraction of all neurons that fired at least once in the last 2 seconds, shown as a percentage.");

    const cavNote = document.createElement("div");
    cavNote.className = "dp-caption";
    cavNote.textContent = "(approx — full avalanche histogram TODO)";
    root.appendChild(cavNote);

    // ── Interpretive Summary ──────────────────────────────────────────────────
    root.appendChild(this._sep("Summary"));
    const interpretRow = document.createElement("div");
    interpretRow.className = "dp-interpret-row";
    this._attachTip(interpretRow,
      "One-line interpretation combining the network-state verdict, the branching-ratio regime, and the E/I balance.");
    const interpret = document.createElement("span");
    interpret.className = "dp-interpret";
    interpret.textContent = "—";
    interpretRow.appendChild(interpret);
    root.appendChild(interpretRow);

    this.dynamicsFields = {
      eiRatio,
      eiBarE,
      eiBarI,
      branchValue,
      branchBand,
      inputSpikes,
      assocSpikes,
      outputSpikes,
      cascadeSize,
      pct100,
      pct500,
      pct2s,
      interpret,
    };
  }

  // ── UX round 2: Network / Simulation tab DOM ──────────────────────────────
  // Sections: Network Scale (rebuild) · Drive (live) · Structure (live)

  private _buildNetworkTab(root: HTMLDivElement): void {
    const s = getSettings();

    // ── Network Scale ─────────────────────────────────────────────────────────
    root.appendChild(this._sep("Network Scale"));
    this._caption(root, "Changing N / K / seed rebuilds the network.");

    // current seed state (mutable within this closure)
    let currentSeed = this._initSeed;

    // N — slider + number input, fires onNetwork on change (not every input tick)
    const [nSlider, nInput] = this._sliderWithInput(root, {
      label: "N (neurons)",
      tooltip: "Number of neurons in the network — increasing N increases realism but costs GPU memory and compute.",
      impact: "renderer-rebuild",
      min: 1000, max: 200_000, step: 1000,
      initialValue: this._initN,
    }, (n) => {
      this.simHandlers?.onNetwork({ n, k: parseInt(kInput.value, 10), seed: currentSeed });
    }, /* liveOnInput */ false);

    // K — slider + number input, fires onNetwork on change
    const [kSlider, kInput] = this._sliderWithInput(root, {
      label: "K (fan-out)",
      tooltip: "Number of outgoing synapses per neuron — higher K means denser connectivity and stronger cascade potential.",
      impact: "renderer-rebuild",
      min: 4, max: 64, step: 1,
      initialValue: this._initK,
    }, (k) => {
      this.simHandlers?.onNetwork({ n: parseInt(nInput.value, 10), k, seed: currentSeed });
    }, /* liveOnInput */ false);

    // Seed row: number input + Regenerate button
    const seedRow = document.createElement("div");
    seedRow.className = "dp-ctrl-row";
    const seedDot = this._impactDot("renderer-rebuild");
    seedRow.appendChild(seedDot);
    this._attachTip(seedRow, "Random seed for network connectivity — change it (or hit Regenerate) to get a different wiring; rebuilds the network.");
    const seedLbl = document.createElement("span");
    seedLbl.className = "dp-label dp-ctrl-label";
    seedLbl.textContent = "Seed";
    seedRow.appendChild(seedLbl);
    const seedInput = document.createElement("input");
    seedInput.type = "number";
    seedInput.className = "dp-num-input";
    seedInput.min = "0";
    seedInput.max = "4294967295";
    seedInput.step = "1";
    seedInput.value = String(currentSeed >>> 0);
    const applySeed = () => {
      currentSeed = (parseInt(seedInput.value, 10) || 0) >>> 0;
      seedInput.value = String(currentSeed);
      this.simHandlers?.onNetwork({
        n: parseInt(nInput.value, 10),
        k: parseInt(kInput.value, 10),
        seed: currentSeed,
      });
    };
    seedInput.addEventListener("change", applySeed);
    seedInput.addEventListener("keydown", (e) => { if (e.key === "Enter") applySeed(); });
    seedRow.appendChild(seedInput);
    root.appendChild(seedRow);

    const regenRow = document.createElement("div");
    regenRow.style.cssText = "display:flex;gap:4px;padding:3px 10px 6px;";
    const regenBtn = document.createElement("button");
    regenBtn.className = "dp-regen-btn";
    regenBtn.textContent = "Regenerate network";
    this._attachTip(regenBtn, "Pick a new random seed and rebuild the network.");
    regenBtn.addEventListener("click", () => {
      // Simple seed mutation: add a large prime, wrap in u32. UX round 2.
      currentSeed = (currentSeed + 0x9e3779b9) >>> 0;
      seedInput.value = String(currentSeed);
      this.simHandlers?.onNetwork({
        n: parseInt(nInput.value, 10),
        k: parseInt(kInput.value, 10),
        seed: currentSeed,
      });
    });
    regenRow.appendChild(regenBtn);
    root.appendChild(regenRow);

    // Suppress unused-var warnings (nSlider/kSlider refs needed for hoisting in closures).
    void nSlider; void kSlider;

    // ── Drive (live) ──────────────────────────────────────────────────────────
    root.appendChild(this._sep("Drive"));
    this._caption(root, "All drive controls take effect immediately.");

    // Excitability — slider + number input (live)
    this._sliderWithInput(root, {
      label: "Excitability",
      tooltip: "Global gain on synaptic input — low = sleepy, high = seizure-like.",
      impact: "live",
      min: 0, max: 1, step: 0.01,
      decimals: 2,
      initialValue: this._initExcitability,
    }, (v) => {
      this.simHandlers?.onExcitability(v);
    }, /* liveOnInput */ true);

    // Speed (ticks/sec) — slider + number input (live). UX round 2.
    this._sliderWithInput(root, {
      label: "Speed (ticks/sec)",
      tooltip: "Target simulation ticks per second (time-based, independent of frame rate). Default 30.",
      impact: "live",
      min: 1, max: 60, step: 1,
      decimals: 0,
      initialValue: this._initTps,
    }, (v) => {
      this.simHandlers?.onSpeed(Math.max(1, Math.min(60, Math.round(v))));
    }, /* liveOnInput */ true);

    // iExt (sim drive) — live
    this._sliderRow(root, {
      key: "iExt",
      label: "I_ext (drive)",
      tooltip: "Ambient input drive injected into input-region neurons each tick — higher values push the network toward more firing.",
      min: 0, max: 0.3, step: 0.001,
      decimals: 3,
    }, s.iExt, "live");

    // synapticScale — live
    this._sliderRow(root, {
      key: "synapticScale",
      label: "Synaptic scale",
      tooltip: "Global scale factor on recurrent synaptic weights — increase to amplify network activity, decrease to dampen it.",
      min: 0, max: 0.2, step: 0.001,
      decimals: 3,
    }, s.synapticScale, "live");

    // inputMode select — now live (UX round 2)
    this._selectRow(root, {
      key: "inputMode",
      label: "Input mode",
      tooltip: "How external drive is injected into the input region: constant current, Poisson spikes, periodic pulses, cursor-driven, scripted, or off.",
      options: [
        { value: 0, label: "Constant" },
        { value: 1, label: "Poisson" },
        { value: 2, label: "Pulsed" },
        { value: 3, label: "Cursor only" },
        { value: 4, label: "Scripted" },
        { value: 5, label: "Off" },
      ],
    }, s.inputMode, "live");

    // ── Structure (live) ──────────────────────────────────────────────────────
    root.appendChild(this._sep("Structure"));
    this._caption(root, "Structural params now read live from the integrate uniform.");

    // heterogeneity — now live (UX round 2)
    this._sliderRow(root, {
      key: "heterogeneity",
      label: "Heterogeneity",
      tooltip: "Per-neuron variation in threshold, leak rate, and refractory period (0 = identical neurons, 1 = maximum diversity).",
      min: 0, max: 1, step: 0.01,
      decimals: 2,
    }, s.heterogeneity, "live");

    // weightNormalization — now live (UX round 2)
    this._selectRow(root, {
      key: "weightNormalization",
      label: "Weight norm",
      tooltip: "Scales recurrent synaptic input by 1, 1/√K, or 1/K to keep network dynamics stable as fan-out K changes.",
      options: [
        { value: 0, label: "None" },
        { value: 1, label: "sqrt(K)" },
        { value: 2, label: "K" },
      ],
    }, s.weightNormalization, "live");
  }

  // ── UX round 2: Slider + number input helper ──────────────────────────────
  // Returns [sliderEl, numberInputEl] so callers can cross-reference values.
  // liveOnInput=true → fires callback on every 'input' event (smooth live update).
  // liveOnInput=false → fires only on 'change' (release / Enter / blur), for rebuild.

  private _sliderWithInput(
    parent: HTMLElement,
    spec: {
      label: string;
      tooltip?: string;
      impact: import("../core/setting-metadata").SettingImpact;
      min: number; max: number; step: number;
      decimals?: number;
      initialValue: number;
    },
    onApply: (value: number) => void,
    liveOnInput: boolean,
  ): [HTMLInputElement, HTMLInputElement] {
    const decimals = spec.decimals ?? (spec.step < 1 ? 2 : 0);

    // Label row (dot + label, no separate readout — number input serves as readout)
    const row = document.createElement("div");
    row.className = "dp-ctrl-row";
    if (spec.tooltip) this._attachTip(row, spec.tooltip); // v0.1.2: instant tooltip
    const dot = this._impactDot(spec.impact);
    row.appendChild(dot);
    const lbl = document.createElement("span");
    lbl.className = "dp-label dp-ctrl-label";
    lbl.textContent = spec.label;
    row.appendChild(lbl);
    parent.appendChild(row);

    // Slider + number input side-by-side
    const wrap = document.createElement("div");
    wrap.className = "dp-slider-input-wrap";
    if (spec.tooltip) this._attachTip(wrap, spec.tooltip); // v0.1.2: instant tooltip

    const slider = document.createElement("input");
    slider.type = "range";
    slider.className = "dp-slider";
    slider.min = String(spec.min);
    slider.max = String(spec.max);
    slider.step = String(spec.step);
    slider.value = String(spec.initialValue);

    const numInput = document.createElement("input");
    numInput.type = "number";
    numInput.className = "dp-num-input";
    numInput.min = String(spec.min);
    numInput.max = String(spec.max);
    numInput.step = String(spec.step);
    numInput.value = spec.initialValue.toFixed(decimals);

    // Slider → number input sync
    const onSliderChange = () => {
      const v = parseFloat(slider.value);
      numInput.value = v.toFixed(decimals);
      onApply(v);
    };
    slider.addEventListener(liveOnInput ? "input" : "change", onSliderChange);

    // Number input → slider sync (apply on change/Enter/blur)
    const onNumApply = () => {
      let v = parseFloat(numInput.value);
      if (isNaN(v)) v = spec.initialValue;
      v = Math.max(spec.min, Math.min(spec.max, v));
      slider.value = String(v);
      numInput.value = v.toFixed(decimals);
      onApply(v);
    };
    numInput.addEventListener("change", onNumApply);
    numInput.addEventListener("keydown", (e) => { if (e.key === "Enter") onNumApply(); });

    wrap.appendChild(slider);
    wrap.appendChild(numInput);
    parent.appendChild(wrap);

    return [slider, numInput];
  }

  // ── V2 Phase B: Rendering tab DOM ──────────────────────────────────────────

  private _buildRenderingTab(root: HTMLDivElement): void {
    const s = getSettings();

    // ── V2 Phase E: Appearance — orthogonal mode selects ─────────────────────
    root.appendChild(this._sep("Appearance"));

    this._selectRow(root, {
      key: "colorBy",
      label: "Color by",
      tooltip: "Choose what neuron property determines the displayed color (region, E/I type, spike recency, voltage, or activity level).",
      options: [
        { value: 0, label: "Region" },
        { value: 1, label: "E/I" },
        { value: 2, label: "Spike age" },
        { value: 3, label: "Voltage (debug)" },
        { value: 4, label: "Activity" },
      ],
    }, s.colorBy, "live");

    this._selectRow(root, {
      key: "neuronVisibility",
      label: "Neurons",
      tooltip: "Controls which neurons are rendered — showing only active ones reduces overdraw and improves legibility at high N.",
      options: [
        { value: 0, label: "All" },
        { value: 1, label: "Active emphasis" },
        { value: 2, label: "Active only" },
      ],
    }, s.neuronVisibility, "live");
    // V2 Phase F: at high N (balanced/max tier) "Active only" or "Active emphasis"
    // significantly reduces overdraw and makes the activity pattern more legible.
    this._caption(root, "(tip: Active only / Active emphasis helps legibility at high N)");

    // Surface select removed from the panel — the morphology replaces the
    // brain-mesh context. The `surface` setting field remains (default off).

    this._selectRow(root, {
      key: "signalSource",
      label: "Signal source",
      tooltip: "Which per-neuron quantity drives visual glow/color: spike events, membrane voltage, or a smoothed activity estimate.",
      options: [
        { value: 0, label: "Spike" },
        { value: 1, label: "Voltage" },
        { value: 2, label: "Activity" },
      ],
    }, s.signalSource, "live");

    // ── Neuron glow (all live, real effect now) ──────────────────────────────
    root.appendChild(this._sep("Neuron Glow"));

    this._sliderRow(root, {
      key: "glowTau",
      label: "Glow decay (τ)",
      tooltip: "How long a neuron keeps glowing after it fires (decay time in ticks) — higher values give a longer, softer afterglow.",
      min: 1, max: 200, step: 1,
      decimals: 0,
    }, s.glowTau, "live");

    this._sliderRow(root, {
      key: "pointRadius",
      label: "Point radius",
      tooltip: "Billboard radius of each neuron point in world units — larger values make individual neurons more visible.",
      min: 0.001, max: 0.02, step: 0.0005,
      decimals: 4,
    }, s.pointRadius, "live");

    this._sliderRow(root, {
      key: "voltageGlowStrength",
      label: "Voltage glow",
      tooltip: "Strength of the membrane-voltage debug glow overlay (0 = off) — helps visualise sub-threshold dynamics.",
      min: 0, max: 2, step: 0.05,
      decimals: 2,
    }, s.voltageGlowStrength, "live");

    // ── Neuron body (no-op until Phase E) ────────────────────────────────────
    root.appendChild(this._sep("Neuron Body"));
    this._caption(root, "(applies in Phase E)");

    this._sliderRow(root, {
      key: "neuronVisualRadius",
      label: "Visual radius",
      tooltip: "Base mesh radius of neuron spheres in world units (distinct from point-sprite radius).",
      min: 0.001, max: 0.02, step: 0.0005,
      decimals: 4,
    }, s.neuronVisualRadius, "live");

    this._sliderRow(root, {
      key: "activeNeuronRadiusBoost",
      label: "Active boost ×",
      tooltip: "Radius multiplier applied to actively-firing neurons — higher values make spikes visually pop.",
      min: 1.0, max: 5.0, step: 0.1,
      decimals: 1,
    }, s.activeNeuronRadiusBoost, "live");

    this._sliderRow(root, {
      key: "inactiveNeuronOpacity",
      label: "Inactive opacity",
      tooltip: "Opacity of neurons that are not currently firing (0 = invisible, 1 = fully opaque) — lower values de-emphasise inactive neurons.",
      min: 0, max: 1, step: 0.02,
      decimals: 2,
    }, s.inactiveNeuronOpacity, "live");

    // ── Morphology (procedural neuron connectivity) ─────────────────────────
    root.appendChild(this._sep("Morphology"));

    // connection_layer: Off / On. On = resting structure (at resting opacity) +
    // live signal flow racing outward from each firing neuron's soma.
    this._selectRow(root, {
      key: "connectionLayer",
      label: "Connections",
      tooltip: "Show the procedural neuron morphology (dendrite trees + axon arbors) with spike-keyed connection lighting.",
      options: [
        { value: 0, label: "Off" },
        { value: 1, label: "On" },
      ],
    }, s.connectionLayer, "live");

    // v0.1.2: whole-connection spike lighting toggles (replace the retired
    // traveling-pulse "Signal speed" / "Recent trail" sliders).
    this._selectRow(root, {
      key: "connectionLightNext",
      label: "Light next (downstream)",
      tooltip: "When a neuron fires, its outgoing connections (toward the neurons it drives) light up and fade out in sync with the neuron's own glow.",
      options: [
        { value: 0, label: "Off" },
        { value: 1, label: "On" },
      ],
    }, s.connectionLightNext, "live");

    this._selectRow(root, {
      key: "connectionLightPast",
      label: "Light past (upstream)",
      tooltip: "When a neuron fires, the incoming connections from the neurons that feed it light up and fade out in sync with the firing neuron's glow.",
      options: [
        { value: 0, label: "Off" },
        { value: 1, label: "On" },
      ],
    }, s.connectionLightPast, "live");

    this._sliderRow(root, {
      key: "morphRestingOpacity",
      label: "Resting opacity",
      tooltip: "Opacity of non-active structure. Set to 0 to show only live signals.",
      min: 0, max: 1, step: 0.02,
      decimals: 2,
    }, s.morphRestingOpacity, "live");

    this._sliderRow(root, {
      key: "connectionVisualWidth",
      label: "Width",
      tooltip: "Branch thickness multiplier.",
      min: 0.1, max: 4.0, step: 0.05,
      decimals: 2,
    }, s.connectionVisualWidth, "live");

    // Curve: regenerates the morphology, so apply on release (changeOnly).
    this._sliderRow(root, {
      key: "connectionCurveLift",
      label: "Curve",
      tooltip: "Bend of the axon connections from straight (0) to strongly arced (rebuilds the morphology).",
      min: 0, max: 0.5, step: 0.01,
      decimals: 2,
      changeOnly: true,
    }, s.connectionCurveLift, "renderer-rebuild");

    // ── Post (bloom on by default; morphology glow blooms out of the box) ──────
    root.appendChild(this._sep("Post"));

    this._sliderRow(root, {
      key: "bloomStrength",
      label: "Bloom strength",
      tooltip: "Intensity of the post-process bloom glow around bright structure (0 = off).",
      min: 0, max: 2, step: 0.05,
      decimals: 2,
    }, s.bloomStrength, "live");
    // Surface controls (surfaceOpacity slider + surface select) removed: the
    // morphology replaces the brain-mesh context. The settings fields remain
    // (default off) but are no longer exposed in the panel.

    // 0.1.1: runtime auto-scaling removed — the "Adaptive scaler" toggle is gone.
    // The settings field at index 23 (adaptiveScalerEnabled) is kept RESERVED/INERT
    // to preserve the Rust↔TS VisualSettings contract; it is no longer exposed.
    // UX round 2: Sim Drive + Network Params moved to Network tab (Drive + Structure sections).
  }

  // ── V2 Phase E: Debug View tab DOM ────────────────────────────────────────

  private _buildDebugViewTab(root: HTMLDivElement): void {
    const s = getSettings();

    root.appendChild(this._sep("Current Visual Mode"));
    this._caption(root, "(reflects live settings; updates on change)");

    const colorBy          = this._row(root, "Color by");
    const neuronVisibility = this._row(root, "Neurons");
    const connectionLayer  = this._row(root, "Connection layer");
    const surface          = this._row(root, "Surface");
    const signalSource     = this._row(root, "Signal source");

    this.debugViewFields = {
      colorBy,
      neuronVisibility,
      connectionLayer,
      surface,
      signalSource,
    };

    // Populate immediately with current values.
    this._updateDebugViewFields(s);
  }

  // V2 Phase E: update Debug View readouts from a settings snapshot.
  private _updateDebugViewFields(s: import("../core/settings").VisualizerSettings): void {
    if (!this.debugViewFields) return;
    const d = this.debugViewFields;

    const COLOR_BY_LABELS   = ["Region", "E/I", "Spike age", "Voltage (debug)", "Activity"];
    const NEURON_VIS_LABELS = ["All", "Active emphasis", "Active only"];
    const CONN_LAYER_LABELS = ["Off", "On"];
    const SURFACE_LABELS    = ["Off", "Dim", "Normal"];
    const SIGNAL_SRC_LABELS = ["Spike", "Voltage", "Activity"];

    d.colorBy.textContent          = COLOR_BY_LABELS[s.colorBy]          ?? String(s.colorBy);
    d.neuronVisibility.textContent = NEURON_VIS_LABELS[s.neuronVisibility] ?? String(s.neuronVisibility);
    d.connectionLayer.textContent  = CONN_LAYER_LABELS[s.connectionLayer]  ?? String(s.connectionLayer);
    d.surface.textContent          = SURFACE_LABELS[s.surface]             ?? String(s.surface);
    d.signalSource.textContent     = SIGNAL_SRC_LABELS[s.signalSource]     ?? String(s.signalSource);
  }

  // ── V2 Phase B: Storage tab DOM ────────────────────────────────────────────

  private _buildStorageTab(root: HTMLDivElement): void {
    root.appendChild(this._sep("Settings"));

    const resetBtn = document.createElement("button");
    resetBtn.className = "dp-action-btn";
    resetBtn.textContent = "Reset settings to defaults";
    resetBtn.addEventListener("click", () => {
      resetSettings();
      this._refreshStorageReadout();
    });

    const btnRow = document.createElement("div");
    btnRow.className = "dp-btn-row";
    btnRow.appendChild(resetBtn);
    root.appendChild(btnRow);

    root.appendChild(this._sep("localStorage"));

    const readoutEl = document.createElement("div");
    readoutEl.className = "dp-storage-readout";
    readoutEl.id = "dp-storage-readout";
    root.appendChild(readoutEl);

    this._refreshStorageReadout();
  }

  // ── V2 Phase B: Slider row builder ────────────────────────────────────────

  private _sliderRow(
    parent: HTMLElement,
    spec: SliderSpec,
    initialValue: number,
    impact: SettingImpact,
  ): void {
    const row = document.createElement("div");
    row.className = "dp-ctrl-row";
    if (spec.tooltip) this._attachTip(row, spec.tooltip); // v0.1.2: instant tooltip

    // Impact dot.
    const dot = this._impactDot(impact);
    row.appendChild(dot);

    // Label.
    const lbl = document.createElement("span");
    lbl.className = "dp-label dp-ctrl-label";
    lbl.textContent = spec.label;
    row.appendChild(lbl);

    // Numeric readout.
    const readout = document.createElement("span");
    readout.className = "dp-value dp-ctrl-readout";
    readout.textContent = initialValue.toFixed(spec.decimals);
    row.appendChild(readout);

    // Slider (full row beneath).
    const sliderWrap = document.createElement("div");
    sliderWrap.className = "dp-slider-wrap";
    if (spec.tooltip) this._attachTip(sliderWrap, spec.tooltip); // v0.1.2: instant tooltip

    const input = document.createElement("input");
    input.type = "range";
    input.className = "dp-slider";
    input.min = String(spec.min);
    input.max = String(spec.max);
    input.step = String(spec.step);
    input.value = String(initialValue);

    // Morphology controls: curve-lift uses 'change' (release/Enter) so geometry
    // regenerates once per release, not on every drag tick. All other sliders
    // stay live on 'input'. The readout still tracks the slider live in both.
    input.addEventListener("input", () => {
      readout.textContent = parseFloat(input.value).toFixed(spec.decimals);
      if (!spec.changeOnly) {
        // setSetting triggers the subscribe callback in main.ts → pendingSettingsPush.
        setSetting(spec.key, parseFloat(input.value) as never);
      }
    });
    if (spec.changeOnly) {
      input.addEventListener("change", () => {
        const v = parseFloat(input.value);
        readout.textContent = v.toFixed(spec.decimals);
        setSetting(spec.key, v as never);
      });
    }

    sliderWrap.appendChild(input);
    parent.appendChild(row);
    parent.appendChild(sliderWrap);

    // Register for external sync (e.g. resetSettings).
    this.sliderElements.set(spec.key, { input, readout });
  }

  // ── V2 Phase B: Select row builder ───────────────────────────────────────

  private _selectRow(
    parent: HTMLElement,
    spec: SelectSpec,
    initialValue: number,
    impact: SettingImpact,
  ): void {
    const row = document.createElement("div");
    row.className = "dp-ctrl-row";
    if (spec.tooltip) this._attachTip(row, spec.tooltip); // v0.1.2: instant tooltip

    // Impact dot.
    const dot = this._impactDot(impact);
    row.appendChild(dot);

    // Label.
    const lbl = document.createElement("span");
    lbl.className = "dp-label dp-ctrl-label";
    lbl.textContent = spec.label;
    row.appendChild(lbl);

    // <select>.
    const sel = document.createElement("select");
    sel.className = "dp-select";
    for (const opt of spec.options) {
      const el = document.createElement("option");
      el.value = String(opt.value);
      el.textContent = opt.label;
      if (opt.value === initialValue) el.selected = true;
      sel.appendChild(el);
    }

    sel.addEventListener("change", () => {
      const v = parseInt(sel.value, 10);
      setSetting(spec.key, v as never);
      // UX round 2: brain-reset pending UI removed; all these settings are now live.
    });

    row.appendChild(sel);
    parent.appendChild(row);

    // Register the select for external sync.
    // We repurpose the sliderElements map — store a synthetic object.
    // For selects we only need to sync .value; readout is the select itself.
    const fakeInput = { value: String(initialValue) } as HTMLInputElement;
    // Store select reference under a special key so _syncSliders can update it.
    this.sliderElements.set(`${spec.key}__select`, {
      input: sel as unknown as HTMLInputElement,
      readout: document.createElement("span"), // unused for selects
    });
    void fakeInput; // suppress unused-variable warning
  }

  // ── V2 Phase B: impact dot helper ────────────────────────────────────────

  private _impactDot(impact: SettingImpact): HTMLSpanElement {
    const dot = document.createElement("span");
    dot.className = "dp-impact-dot";
    dot.title = impactLabel(impact);
    dot.style.background = impactColor(impact);
    return dot;
  }

  // ── V2 Phase B: small caption line ────────────────────────────────────────

  private _caption(parent: HTMLElement, text: string): void {
    const cap = document.createElement("div");
    cap.className = "dp-caption";
    cap.textContent = text;
    parent.appendChild(cap);
  }

  // ── V2 Phase B: sync sliders to external settings changes ────────────────
  // Called when settings change from any source (including resetSettings).

  private _syncSliders(s: import("../core/settings").VisualizerSettings): void {
    for (const [key, el] of this.sliderElements) {
      // Select elements are stored with "__select" suffix.
      if (key.endsWith("__select")) {
        const realKey = key.replace("__select", "") as keyof import("../core/settings").VisualizerSettings;
        const val = s[realKey];
        if (val !== undefined) {
          el.input.value = String(val);
        }
        continue;
      }
      // Slider elements.
      const realKey = key as keyof import("../core/settings").VisualizerSettings;
      const val = s[realKey];
      if (val !== undefined) {
        el.input.value = String(val);
        // Find the spec's decimals by looking at the current readout format.
        // Cheapest approach: re-derive decimals from step stored in input.step.
        const step = parseFloat(el.input.step);
        const decimals = step < 0.001 ? 4 : step < 0.01 ? 3 : step < 0.1 ? 2 : step < 1 ? 1 : 0;
        el.readout.textContent = (val as number).toFixed(decimals);
      }
    }
  }

  // ── V2 Phase B: storage readout ───────────────────────────────────────────

  private _refreshStorageReadout(): void {
    const el = document.getElementById("dp-storage-readout");
    if (!el) return;
    try {
      const raw = localStorage.getItem(DevPanel.LS_KEY);
      if (!raw) {
        el.textContent = `Key: ${DevPanel.LS_KEY}\nSize: (not set)`;
      } else {
        const byteLen = new TextEncoder().encode(raw).length;
        el.textContent =
          `Key: ${DevPanel.LS_KEY}\nSize: ${byteLen} bytes`;
      }
    } catch {
      el.textContent = "(localStorage unavailable)";
    }
  }

  // ── V2 Phase C: populate Dynamics tab live readouts. ─────────────────────

  private _updateDynamicsFields(m: Metrics): void {
    if (!this.dynamicsFields) return;
    const d = this.dynamicsFields;

    // ── E/I balance ────────────────────────────────────────────────────────
    const total = m.eSpikes + m.iSpikes;
    const eiRatio = m.eSpikes / (m.iSpikes || 1);
    d.eiRatio.textContent = eiRatio.toFixed(2);

    // Update the inline split bar (percentage widths).
    const ePct = total > 0 ? (m.eSpikes / total) * 100 : 50;
    const iPct = 100 - ePct;
    d.eiBarE.style.width = `${ePct.toFixed(1)}%`;
    d.eiBarI.style.width = `${iPct.toFixed(1)}%`;

    // ── Branching ratio ─────────────────────────────────────────────────────
    const br = m.branchingRatio;
    d.branchValue.textContent = br.toFixed(3);

    // Critical-band labelling: σ<0.9 subcritical, 0.9–1.1 critical, >1.1 supercritical.
    let bandLabel: string;
    let bandClass: string;
    if (br < BRANCH_SUBCRITICAL) {
      bandLabel = "subcritical (fading)";
      bandClass = "dp-branch--sub";
    } else if (br <= BRANCH_SUPERCRITICAL) {
      bandLabel = "≈ critical";
      bandClass = "dp-branch--crit";
    } else {
      bandLabel = "supercritical (runaway)";
      bandClass = "dp-branch--super";
    }
    d.branchBand.textContent = bandLabel;
    d.branchBand.className = `dp-branch-band ${bandClass}`;

    // ── Per-region rates ────────────────────────────────────────────────────
    d.inputSpikes.textContent  = m.inputSpikes.toFixed(0);
    d.assocSpikes.textContent  = m.assocSpikes.toFixed(0);
    d.outputSpikes.textContent = m.outputSpikes.toFixed(0);

    // ── Cascade / avalanche size ────────────────────────────────────────────
    d.cascadeSize.textContent = m.spikesThisTick.toFixed(0);
    d.pct100.textContent      = (m.pctFired100ms * 100).toFixed(2) + "%";
    d.pct500.textContent      = (m.pctFired500ms * 100).toFixed(2) + "%";
    d.pct2s.textContent       = (m.pctFired2s    * 100).toFixed(2) + "%";

    // ── Interpretive summary ────────────────────────────────────────────────
    d.interpret.textContent = _dynInterpret(m, br, eiRatio);
  }

  // ── Monitor tab DOM ────────────────────────────────────────────────────────

  /**
   * Create a label/value row, appending to parent; returns the value span.
   * v0.1.2: an optional `tip` attaches an instant tooltip to the whole row.
   */
  private _row(parent: HTMLElement, label: string, tip?: string): HTMLSpanElement {
    const row = document.createElement("div");
    row.className = "dp-row";
    if (tip) this._attachTip(row, tip);
    const lbl = document.createElement("span");
    lbl.className = "dp-label";
    lbl.textContent = label;
    const val = document.createElement("span");
    val.className = "dp-value";
    val.textContent = "—";
    row.appendChild(lbl);
    row.appendChild(val);
    parent.appendChild(row);
    return val;
  }

  /** Section separator. */
  private _sep(title: string): HTMLDivElement {
    const sep = document.createElement("div");
    sep.className = "dp-sep";
    sep.textContent = title;
    return sep;
  }
}

// ── V2 Phase C: one-liner interpretive summary for the Dynamics tab. ─────────
function _dynInterpret(m: Metrics, branchingRatio: number, eiRatio: number): string {
  // Network-state verdict first.
  const state = m.pctFired500ms < SILENT_THRESHOLD_PCT500MS
    ? "SILENT"
    : (m.pctFired100ms > OVERACTIVE_THRESHOLD_PCT100 || m.branchingRatio > OVERACTIVE_BRANCHING_RATIO)
      ? "OVERACTIVE"
      : "TUNED";

  let branchDesc: string;
  if (branchingRatio < BRANCH_SUBCRITICAL)        branchDesc = "fading cascade";
  else if (branchingRatio <= BRANCH_SUPERCRITICAL) branchDesc = "near-critical propagation";
  else                                              branchDesc = "runaway cascade";

  const eiDesc = eiRatio > 5  ? "E-dominated"
               : eiRatio < 0.5 ? "I-dominated"
               : "balanced E/I";

  return `${state} · ${branchDesc} · ${eiDesc}`;
}
