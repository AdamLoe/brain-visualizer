// web/settings.ts — V2 Phase 0
//
// Canonical settings store for Brain Visualizer V2.
// Shared contract with Rust: the Float32Array layout produced by toFloat32Array
// MUST match the VisualSettings struct in src/sim/gpu/mod.rs index-for-index.
//
// Persistence: versioned localStorage key "bv2_settings_v1".
// Schema: { version:4, public:{…}, dev:{…} }
// (version bumped 1→2: morphology controls changed defaults/semantics — width is
//  now a 1.0 multiplier, index 15 repurposed to morphRestingOpacity,
//  connectionLayer default 1, bloom default 0.5; old v1 data is ignored.)
// (version bumped 2→3: connections redesign — indices 8/9 repurposed from
//  connectionLifetime/connectionPulseSpeed (the retired traveling-pulse model)
//  to the two whole-connection lighting toggles connectionLightNext (downstream,
//  default 1) and connectionLightPast (upstream, default 0). Old v2 data is
//  discarded — no migration; defaults are applied.)
// (version bumped 3→4: morphology readability tuning — defaults for connection
//  width, bloom, and resting opacity were narrowed to reduce lattice clutter;
//  old v3 data is ignored and defaults are applied.)
// (version bumped 4→5: connectionLightPast removed — index 9 tombstoned as
//  reserved_zero; upstream lighting on shared arbors was misleading and is
//  deferred until whole-path semantics are redesigned. Old v4 data is discarded.)
// On version mismatch → ignore saved data, use defaults (no migration for now).
// Never persist runtime counters (there are none in this struct).

// ─── canonical flat-array length ─────────────────────────────────────────────
export const SETTINGS_LENGTH = 24;

// ─── VisualizerSettings (runtime flat type) ───────────────────────────────────
// One field per contract index.  Mode fields typed as number (integer) for now;
// Phase B will introduce typed enums for the subset that has a dev-panel selector.

export interface VisualizerSettings {
  // ── index 0–11: visual continuous knobs ──
  glowTau:                  number;   // 0  glow decay in ticks
  pointRadius:              number;   // 1  billboard radius (world units)
  neuronVisualRadius:       number;   // 2  neuron mesh radius (world units)
  activeNeuronRadiusBoost:  number;   // 3  radius multiplier when firing
  inactiveNeuronOpacity:    number;   // 4  opacity of non-firing neurons
  voltageGlowStrength:      number;   // 5  debug voltage glow (0=off)
  connectionVisualWidth:    number;   // 6  Morphology: branch-width multiplier (1.0 = raw radii)
  connectionCurveLift:      number;   // 7  Morphology: axon branch curl (rebuilds geometry)
  connectionLightNext:      number;   // 8  Morphology: light a firing neuron's downstream (outgoing) connections (0/1)
  // index 9: reserved_zero (connectionLightPast removed)
  bloomStrength:            number;   // 10 bloom post-process intensity (0=off)
  surfaceOpacity:           number;   // 11 manifold surface opacity
  // ── index 12–14: sim knobs ───────────────
  iExt:                     number;   // 12 ambient drive current
  synapticScale:            number;   // 13 recurrent coupling scale
  heterogeneity:            number;   // 14 per-neuron param spread 0→1
  // ── index 15: Morphology — resting opacity ───────────────
  morphRestingOpacity:      number;   // 15 opacity of non-active structure (0..1)
  // ── index 16–23: mode enums ──────────────
  signalSource:             number;   // 16 signal source mode
  connectionLayer:          number;   // 17 connection layer mode
  colorBy:                  number;   // 18 color-by mode
  neuronVisibility:         number;   // 19 neuron visibility mode
  surface:                  number;   // 20 surface display mode
  weightNormalization:      number;   // 21 0=none, 1=sqrt_k, 2=k
  inputMode:                number;   // 22 0=constant, 1=poisson, …
  adaptiveScalerEnabled:    number;   // 23 RESERVED/INERT — auto-scaling removed in 0.1.1; index kept to preserve the Rust↔TS contract

}

// ─── DEFAULT_SETTINGS ─────────────────────────────────────────────────────────
// Values reproduce pre-V2 behavior exactly.  N/K defaults live in types.ts.
export const DEFAULT_SETTINGS: VisualizerSettings = {
  glowTau:                  60.0,
  pointRadius:              0.004,
  neuronVisualRadius:       0.004,
  activeNeuronRadiusBoost:  2.0,
  inactiveNeuronOpacity:    1.0,
  voltageGlowStrength:      0.0,
  connectionVisualWidth:    0.80,  // Morphology: width multiplier (1.0 = raw radii)
  connectionCurveLift:      0.15,
  connectionLightNext:      1,     // Morphology: downstream lighting on by default
  bloomStrength:            0.40,  // Morphology: bloom on by default so glow blooms
  surfaceOpacity:           1.0,
  iExt:                     0.055,
  synapticScale:            0.03,
  heterogeneity:            0.0,
  morphRestingOpacity:      0.20,  // Morphology: resting structure opacity (0=only pulses)
  signalSource:             0,
  // Morphology: default 1 = on (resting structure + signal flow). 0 = off.
  connectionLayer:          1,
  colorBy:                  0,
  neuronVisibility:         0,
  surface:                  0,
  weightNormalization:      1,  // sqrt_k default
  inputMode:                0,  // constant
  adaptiveScalerEnabled:    0,  // RESERVED/INERT — auto-scaling removed in 0.1.1; index 23 kept for the contract
};

// ─── SavedVisualizerSettings — persisted schema ───────────────────────────────
// Split into "public" (user-facing beauty knobs) and "dev" (tuning).
// The flat VisualizerSettings at runtime is the merge of both.

/** User-facing settings persisted in localStorage (beauty knobs). */
interface SavedPublic {
  glowTau:               number;
  bloomStrength:         number;
  surfaceOpacity:        number;
  connectionLayer:       number;   // off / active_only / active+fade
  colorBy:               number;
  neuronVisibility:      number;
  surface:               number;
}

/** Dev-only tuning settings persisted in localStorage. */
interface SavedDev {
  pointRadius:              number;
  neuronVisualRadius:       number;
  activeNeuronRadiusBoost:  number;
  inactiveNeuronOpacity:    number;
  voltageGlowStrength:      number;
  connectionVisualWidth:    number;
  connectionCurveLift:      number;
  connectionLightNext:      number;
  iExt:                     number;
  synapticScale:            number;
  heterogeneity:            number;
  morphRestingOpacity:      number;
  signalSource:             number;
  weightNormalization:      number;
  inputMode:                number;
  adaptiveScalerEnabled:    number;
}

/** Versioned localStorage schema.  version !== 5 → ignored (no migration). */
interface SavedVisualizerSettings {
  version: 5;
  public: SavedPublic;
  dev: SavedDev;
}

// ─── localStorage key ────────────────────────────────────────────────────────
export const SETTINGS_LS_KEY = "bv2_settings_v1";
const LS_KEY = SETTINGS_LS_KEY;

// ─── persistence helpers ─────────────────────────────────────────────────────

function settingsToSaved(s: VisualizerSettings): SavedVisualizerSettings {
  return {
    version: 5,
    public: {
      glowTau:           s.glowTau,
      bloomStrength:     s.bloomStrength,
      surfaceOpacity:    s.surfaceOpacity,
      connectionLayer:   s.connectionLayer,
      colorBy:           s.colorBy,
      neuronVisibility:  s.neuronVisibility,
      surface:           s.surface,
    },
    dev: {
      pointRadius:              s.pointRadius,
      neuronVisualRadius:       s.neuronVisualRadius,
      activeNeuronRadiusBoost:  s.activeNeuronRadiusBoost,
      inactiveNeuronOpacity:    s.inactiveNeuronOpacity,
      voltageGlowStrength:      s.voltageGlowStrength,
      connectionVisualWidth:    s.connectionVisualWidth,
      connectionCurveLift:      s.connectionCurveLift,
      connectionLightNext:      s.connectionLightNext,
      iExt:                     s.iExt,
      synapticScale:            s.synapticScale,
      heterogeneity:            s.heterogeneity,
      morphRestingOpacity:      s.morphRestingOpacity,
      signalSource:             s.signalSource,
      weightNormalization:      s.weightNormalization,
      inputMode:                s.inputMode,
      adaptiveScalerEnabled:    s.adaptiveScalerEnabled,
    },
  };
}

function mergeOver(base: VisualizerSettings, saved: SavedVisualizerSettings): VisualizerSettings {
  // Merge saved fields over defaults field-by-field.  Never trust missing fields
  // (each key access is guarded: if the key is undefined it falls back to base).
  const p = saved.public;
  const d = saved.dev;
  return {
    ...base,
    // public
    glowTau:              p.glowTau              ?? base.glowTau,
    bloomStrength:        p.bloomStrength         ?? base.bloomStrength,
    surfaceOpacity:       p.surfaceOpacity        ?? base.surfaceOpacity,
    connectionLayer:      p.connectionLayer       ?? base.connectionLayer,
    colorBy:              p.colorBy               ?? base.colorBy,
    neuronVisibility:     p.neuronVisibility      ?? base.neuronVisibility,
    surface:              p.surface               ?? base.surface,
    // dev
    pointRadius:              d.pointRadius              ?? base.pointRadius,
    neuronVisualRadius:       d.neuronVisualRadius       ?? base.neuronVisualRadius,
    activeNeuronRadiusBoost:  d.activeNeuronRadiusBoost  ?? base.activeNeuronRadiusBoost,
    inactiveNeuronOpacity:    d.inactiveNeuronOpacity    ?? base.inactiveNeuronOpacity,
    voltageGlowStrength:      d.voltageGlowStrength      ?? base.voltageGlowStrength,
    connectionVisualWidth:    d.connectionVisualWidth    ?? base.connectionVisualWidth,
    connectionCurveLift:      d.connectionCurveLift      ?? base.connectionCurveLift,
    connectionLightNext:      d.connectionLightNext      ?? base.connectionLightNext,
    iExt:                     d.iExt                     ?? base.iExt,
    synapticScale:            d.synapticScale            ?? base.synapticScale,
    heterogeneity:            d.heterogeneity            ?? base.heterogeneity,
    morphRestingOpacity:      d.morphRestingOpacity      ?? base.morphRestingOpacity,
    signalSource:             d.signalSource             ?? base.signalSource,
    weightNormalization:      d.weightNormalization      ?? base.weightNormalization,
    inputMode:                d.inputMode                ?? base.inputMode,
    adaptiveScalerEnabled:    d.adaptiveScalerEnabled    ?? base.adaptiveScalerEnabled,
  };
}

/** Load from localStorage.  Returns defaults on version mismatch or parse error. */
export function loadSettings(): VisualizerSettings {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw) as { version?: number };
    if (parsed.version !== 5) return { ...DEFAULT_SETTINGS };
    return mergeOver({ ...DEFAULT_SETTINGS }, parsed as SavedVisualizerSettings);
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

/** Persist current settings to localStorage. */
export function saveSettings(s: VisualizerSettings): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(settingsToSaved(s)));
  } catch {
    // localStorage unavailable (private browsing, quota, etc.) — silent.
  }
}

// ─── Module-level settings store ─────────────────────────────────────────────
// Single owner: module-level `current`; all mutations go through setSetting().
// Subscribers are notified synchronously after each change.

let current: VisualizerSettings = loadSettings();
const subscribers: Set<(s: VisualizerSettings) => void> = new Set();

/** Read the current settings (Readonly to discourage direct mutation). */
export function getSettings(): Readonly<VisualizerSettings> {
  return current;
}

/** Update one setting, persist, and notify all subscribers. */
export function setSetting<K extends keyof VisualizerSettings>(
  key: K,
  value: VisualizerSettings[K],
): void {
  current = { ...current, [key]: value };
  saveSettings(current);
  notify();
}

/** Replace the full settings payload, persist once, and notify subscribers once. */
export function replaceSettings(next: VisualizerSettings): void {
  current = { ...next };
  saveSettings(current);
  notify();
}

/** Subscribe to settings changes.  Returns an unsubscribe function. */
export function subscribe(fn: (s: VisualizerSettings) => void): () => void {
  subscribers.add(fn);
  return () => { subscribers.delete(fn); };
}

/** Reset all settings to defaults, clear localStorage, and notify. */
export function resetSettings(): void {
  try { localStorage.removeItem(LS_KEY); } catch { /* ignore */ }
  current = { ...DEFAULT_SETTINGS };
  notify();
}

function notify(): void {
  for (const fn of subscribers) fn(current);
}

// ─── Flat-array serialisation ────────────────────────────────────────────────
// Produces the exact 24-element Float32Array the Rust VisualSettings::from_slice
// expects.  Indices MUST stay in sync with the contract table.

export function toFloat32Array(s: VisualizerSettings): Float32Array {
  const a = new Float32Array(SETTINGS_LENGTH);
  a[0]  = s.glowTau;
  a[1]  = s.pointRadius;
  a[2]  = s.neuronVisualRadius;
  a[3]  = s.activeNeuronRadiusBoost;
  a[4]  = s.inactiveNeuronOpacity;
  a[5]  = s.voltageGlowStrength;
  a[6]  = s.connectionVisualWidth;
  a[7]  = s.connectionCurveLift;
  a[8]  = s.connectionLightNext;
  a[9]  = 0; // index 9: reserved_zero (connectionLightPast removed)
  a[10] = s.bloomStrength;
  a[11] = s.surfaceOpacity;
  a[12] = s.iExt;
  a[13] = s.synapticScale;
  a[14] = s.heterogeneity;
  a[15] = s.morphRestingOpacity;    // Morphology: resting opacity (0..1)
  a[16] = s.signalSource;
  a[17] = s.connectionLayer;
  a[18] = s.colorBy;
  a[19] = s.neuronVisibility;
  a[20] = s.surface;
  a[21] = s.weightNormalization;
  a[22] = s.inputMode;
  a[23] = s.adaptiveScalerEnabled;
  return a;
}

// ─── Metrics layout ───────────────────────────────────────────────────────────
// Mirrors the Rust WasmGpuBackend::metrics() Vec<f32> layout.
//   indices 0..16  : 17 scalar metrics (below)
//   indices 17..32 : 16-bin voltage histogram (fraction of neurons per bin)
// METRICS_LENGTH = 33.  V2 Phase 0 returns zeros (except index 16); Phase A
// populates everything via a GPU reduction pass + async readback.

export const VOLTAGE_HISTOGRAM_BINS = 16;
export const METRICS_SCALAR_COUNT = 17;
export const METRICS_LENGTH = METRICS_SCALAR_COUNT + VOLTAGE_HISTOGRAM_BINS; // 33

export const METRICS_LAYOUT: readonly string[] = [
  "spikesThisTick",          // 0
  "spikesPerSec",            // 1
  "meanFiringRateHz",        // 2
  "synapticEventsPerSec",    // 3
  "meanMembraneVoltage",     // 4
  "inputSpikes",             // 5
  "assocSpikes",             // 6
  "outputSpikes",            // 7
  "eSpikes",                 // 8
  "iSpikes",                 // 9
  "pctFired100ms",           // 10
  "pctFired500ms",           // 11
  "pctFired2s",              // 12
  "branchingRatio",          // 13
  "timeSinceLastLargeCascade", // 14
  "refractoryBlockedAttempts", // 15
  "currentAccumulatorHighWater", // 16  = max_abs_current_hw
] as const;

export interface Metrics {
  spikesThisTick: number;
  spikesPerSec: number;
  meanFiringRateHz: number;
  synapticEventsPerSec: number;
  meanMembraneVoltage: number;
  inputSpikes: number;
  assocSpikes: number;
  outputSpikes: number;
  eSpikes: number;
  iSpikes: number;
  pctFired100ms: number;
  pctFired500ms: number;
  pctFired2s: number;
  branchingRatio: number;
  timeSinceLastLargeCascade: number;
  refractoryBlockedAttempts: number;
  currentAccumulatorHighWater: number;
  /** 16-bin voltage histogram, fraction of neurons per bin (sums ~1). */
  voltageHistogram: number[];
}

export function parseMetrics(arr: Float32Array): Metrics {
  const scalar = (i: number) => arr[i] ?? 0;
  const histo: number[] = [];
  for (let b = 0; b < VOLTAGE_HISTOGRAM_BINS; b++) {
    histo.push(arr[METRICS_SCALAR_COUNT + b] ?? 0);
  }
  return {
    spikesThisTick:              scalar(0),
    spikesPerSec:                scalar(1),
    meanFiringRateHz:            scalar(2),
    synapticEventsPerSec:        scalar(3),
    meanMembraneVoltage:         scalar(4),
    inputSpikes:                 scalar(5),
    assocSpikes:                 scalar(6),
    outputSpikes:                scalar(7),
    eSpikes:                     scalar(8),
    iSpikes:                     scalar(9),
    pctFired100ms:               scalar(10),
    pctFired500ms:               scalar(11),
    pctFired2s:                  scalar(12),
    branchingRatio:              scalar(13),
    timeSinceLastLargeCascade:   scalar(14),
    refractoryBlockedAttempts:   scalar(15),
    currentAccumulatorHighWater: scalar(16),
    voltageHistogram:            histo,
  };
}
