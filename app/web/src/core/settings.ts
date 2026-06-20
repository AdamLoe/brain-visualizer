// Canonical settings store for Brain Visualizer V2.
// Shared contract with Rust: the Float32Array layout produced by toFloat32Array
// MUST match the VisualSettings struct in src/sim/gpu/mod.rs index-for-index.
//
// Persistence: versioned localStorage key "bv2_settings_v2".
// Schema: { version:5, public:{…}, dev:{…} }
// Removed settings keep their existing indices and are either zero-written
// tombstones or default-written quarantine slots; do not renumber the array.
// On version mismatch → ignore saved data, use defaults (no migration for now).
// Never persist runtime counters (there are none in this struct).

// ─── canonical flat-array length ─────────────────────────────────────────────
export const SETTINGS_LENGTH = 27;

// ─── VisualizerSettings (runtime flat type) ───────────────────────────────────
// One field per contract index. Mode fields are carried as numbers because the
// Rust boundary is a positional Float32Array.

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
  bloomStrength:            number;   // 10 RESERVED/INERT — bloom strength removed; index kept for the Rust↔TS contract
  surfaceOpacity:           number;   // 11 manifold surface opacity
  // ── index 12–14: sim knobs ───────────────
  iExt:                     number;   // 12 ambient drive current
  synapticScale:            number;   // 13 recurrent coupling scale
  heterogeneity:            number;   // 14 per-neuron param spread 0→1
  // ── index 15: Morphology — resting opacity ───────────────
  morphRestingOpacity:      number;   // 15 opacity of non-active structure (0..1)
  // ── index 16–23: mode enums ──────────────
  signalSource:             number;   // 16 RESERVED/INERT — signal source removed; index kept for the Rust↔TS contract
  connectionLayer:          number;   // 17 connection layer mode: 0=Off, 1=Active/recent only (default), 2=Visible until impulse arrival
  colorBy:                  number;   // 18 color-by mode
  neuronVisibility:         number;   // 19 neuron visibility mode
  surface:                  number;   // 20 surface display mode
  weightNormalization:      number;   // 21 0=none, 1=sqrt_k, 2=k
  inputMode:                number;   // 22 0=constant, 1=poisson, …
  adaptiveScalerEnabled:    number;   // 23 RESERVED/INERT — auto-scaling removed in 0.1.1; index kept to preserve the Rust↔TS contract
  // ── index 24–25: heavy-tailed synapse reach ──
  longRangeReachFrac:       number;   // 24 fraction of synapses routed long-range (0..1; 0 = local only)
  maxReachCells:            number;   // 25 long-range max-reach radius in cells (integer)
  // ── index 26: until-arrival visibility ──
  arrivalHoldTicks:          number;   // 26 extra ticks that until-arrival visibility remains after aggregate arrival
}

// ─── DEFAULT_SETTINGS ─────────────────────────────────────────────────────────
// Accepted product defaults. N/K defaults live in types.ts.
export const DEFAULT_SETTINGS: VisualizerSettings = {
  glowTau:                  10.0,
  pointRadius:              0.004,
  neuronVisualRadius:       0.004,
  activeNeuronRadiusBoost:  2.0,
  inactiveNeuronOpacity:    1.0,
  voltageGlowStrength:      0.0,
  connectionVisualWidth:    0.80,  // Morphology: width multiplier (1.0 = raw radii)
  connectionCurveLift:      0.15,
  connectionLightNext:      1,     // Morphology: downstream lighting on by default
  bloomStrength:            0.0,   // RESERVED/INERT — index 10 is zero-written
  surfaceOpacity:           1.0,
  iExt:                     0.014,
  synapticScale:            0.03,
  heterogeneity:            0.50,
  morphRestingOpacity:      0.0,   // Morphology: resting structure hidden by default (0=only pulses)
  signalSource:             0,
  // Morphology connection layer: 0=Off, 1=Active/recent only, 2=Visible until impulse arrival.
  connectionLayer:          1,
  colorBy:                  6,
  neuronVisibility:         0,
  surface:                  0,
  weightNormalization:      1,  // sqrt_k default
  inputMode:                0,  // constant
  adaptiveScalerEnabled:    0,  // RESERVED/INERT — auto-scaling removed in 0.1.1; index 23 kept for the contract
  longRangeReachFrac:       0.14, // heavy-tailed reach: 14% long-range synapses
  maxReachCells:            14,   // long-range max-reach radius in cells
  arrivalHoldTicks:         30.0, // until-arrival mode: hold subdued full-branch visibility after aggregate arrival
};

// ─── SavedVisualizerSettings — persisted schema ───────────────────────────────
// Split into "public" (user-facing beauty knobs) and "dev" (tuning).
// The flat VisualizerSettings at runtime is the merge of both.

/** User-facing settings persisted in localStorage (beauty knobs). */
interface SavedPublic {
  glowTau:               number;
  connectionLayer:       number;   // off / active_recent / visible_until_arrival
  colorBy:               number;
  neuronVisibility:      number;
}

/** Dev-only tuning settings persisted in localStorage. */
interface SavedDev {
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
  weightNormalization:      number;
  inputMode:                number;
  longRangeReachFrac:       number;
  maxReachCells:            number;
  arrivalHoldTicks:         number;
}

/** Versioned localStorage schema.  version !== 5 → ignored (no migration). */
interface SavedVisualizerSettings {
  version: 5;
  public: SavedPublic;
  dev: SavedDev;
}

// ─── localStorage key ────────────────────────────────────────────────────────
export const SETTINGS_LS_KEY = "bv2_settings_v2";
const LS_KEY = SETTINGS_LS_KEY;

// ─── persistence helpers ─────────────────────────────────────────────────────

function normalizedNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function normalizedRange(
  value: unknown,
  min: number,
  max: number,
  fallback: number,
  integer = false,
): number {
  const finite = normalizedNumber(value, fallback);
  const rounded = integer ? Math.round(finite) : finite;
  return Math.max(min, Math.min(max, rounded));
}

function normalizeEnum(value: unknown, allowed: readonly number[], fallback: number): number {
  return typeof value === "number" && allowed.includes(value) ? value : fallback;
}

function normalizeVisualSettings(settings: VisualizerSettings): VisualizerSettings {
  const base = DEFAULT_SETTINGS;
  return {
    ...base,
    glowTau:                  normalizedRange(settings.glowTau, 1, 200, base.glowTau, true),
    neuronVisualRadius:       normalizedRange(settings.neuronVisualRadius, 0.001, 0.02, base.neuronVisualRadius),
    activeNeuronRadiusBoost:  normalizedRange(settings.activeNeuronRadiusBoost, 1, 5, base.activeNeuronRadiusBoost),
    inactiveNeuronOpacity:    normalizedRange(settings.inactiveNeuronOpacity, 0, 1, base.inactiveNeuronOpacity),
    voltageGlowStrength:      normalizedRange(settings.voltageGlowStrength, 0, 2, base.voltageGlowStrength),
    connectionVisualWidth:    normalizedRange(settings.connectionVisualWidth, 0.1, 4, base.connectionVisualWidth),
    connectionCurveLift:      normalizedRange(settings.connectionCurveLift, 0, 0.5, base.connectionCurveLift),
    connectionLightNext:      normalizeEnum(settings.connectionLightNext, [0, 1], base.connectionLightNext),
    iExt:                     normalizedRange(settings.iExt, 0, 0.3, base.iExt),
    synapticScale:            normalizedRange(settings.synapticScale, 0, 0.2, base.synapticScale),
    heterogeneity:            normalizedRange(settings.heterogeneity, 0, 1, base.heterogeneity),
    morphRestingOpacity:      normalizedRange(settings.morphRestingOpacity, 0, 1, base.morphRestingOpacity),
    connectionLayer:          normalizeConnectionLayer(settings.connectionLayer),
    colorBy:                  normalizeEnum(settings.colorBy, [0, 1, 2, 3, 4, 5, 6], base.colorBy),
    neuronVisibility:         normalizeEnum(settings.neuronVisibility, [0, 1, 2], base.neuronVisibility),
    weightNormalization:      normalizeEnum(settings.weightNormalization, [0, 1, 2], base.weightNormalization),
    inputMode:                normalizeEnum(settings.inputMode, [0, 1, 2, 3, 4, 5], base.inputMode),
    longRangeReachFrac:       normalizedRange(settings.longRangeReachFrac, 0, 1, base.longRangeReachFrac),
    maxReachCells:            normalizedRange(settings.maxReachCells, 2, 16, base.maxReachCells, true),
    arrivalHoldTicks:         normalizedRange(settings.arrivalHoldTicks, 0, 180, base.arrivalHoldTicks, true),
  };
}

function settingsToSaved(s: VisualizerSettings): SavedVisualizerSettings {
  const normalized = normalizeVisualSettings(s);
  return {
    version: 5,
    public: {
      glowTau:           normalized.glowTau,
      connectionLayer:   normalized.connectionLayer,
      colorBy:           normalized.colorBy,
      neuronVisibility:  normalized.neuronVisibility,
    },
    dev: {
      neuronVisualRadius:       normalized.neuronVisualRadius,
      activeNeuronRadiusBoost:  normalized.activeNeuronRadiusBoost,
      inactiveNeuronOpacity:    normalized.inactiveNeuronOpacity,
      voltageGlowStrength:      normalized.voltageGlowStrength,
      connectionVisualWidth:    normalized.connectionVisualWidth,
      connectionCurveLift:      normalized.connectionCurveLift,
      connectionLightNext:      normalized.connectionLightNext,
      iExt:                     normalized.iExt,
      synapticScale:            normalized.synapticScale,
      heterogeneity:            normalized.heterogeneity,
      morphRestingOpacity:      normalized.morphRestingOpacity,
      weightNormalization:      normalized.weightNormalization,
      inputMode:                normalized.inputMode,
      longRangeReachFrac:       normalized.longRangeReachFrac,
      maxReachCells:            normalized.maxReachCells,
      arrivalHoldTicks:         normalized.arrivalHoldTicks,
    },
  };
}

function mergeOver(base: VisualizerSettings, saved: SavedVisualizerSettings): VisualizerSettings {
  // Merge saved fields over defaults field-by-field.  Never trust missing fields
  // (each key access is guarded: if the key is undefined it falls back to base).
  const p: Partial<SavedPublic> = saved.public ?? {};
  const d: Partial<SavedDev> = saved.dev ?? {};
  return normalizeVisualSettings({
    ...base,
    // public
    glowTau:              p.glowTau               ?? base.glowTau,
    connectionLayer:      normalizeConnectionLayer(p.connectionLayer ?? base.connectionLayer),
    colorBy:              p.colorBy               ?? base.colorBy,
    neuronVisibility:     p.neuronVisibility      ?? base.neuronVisibility,
    // dev
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
    weightNormalization:      d.weightNormalization      ?? base.weightNormalization,
    inputMode:                d.inputMode                ?? base.inputMode,
    longRangeReachFrac:       d.longRangeReachFrac       ?? base.longRangeReachFrac,
    maxReachCells:            d.maxReachCells            ?? base.maxReachCells,
    arrivalHoldTicks:         d.arrivalHoldTicks         ?? base.arrivalHoldTicks,
  });
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

/** Update one setting, optionally persist, and notify all subscribers. */
export function setSetting<K extends keyof VisualizerSettings>(
  key: K,
  value: VisualizerSettings[K],
  opts: { persist?: boolean } = {},
): void {
  current = normalizeVisualSettings({ ...current, [key]: value });
  if (opts.persist !== false) saveSettings(current);
  notify();
}

/** Replace the full settings payload, persist once, and notify subscribers once. */
export function replaceSettings(next: VisualizerSettings): void {
  current = normalizeVisualSettings({ ...next });
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

function normalizeConnectionLayer(value: number): number {
  if (value === 0) return 0;
  if (value === 2) return 2;
  return 1;
}

// ─── Flat-array serialisation ────────────────────────────────────────────────
// Produces the exact SETTINGS_LENGTH-element Float32Array the Rust
// VisualSettings::from_slice expects.  Indices MUST stay in sync with the
// contract table.

export function toFloat32Array(s: VisualizerSettings): Float32Array {
  const a = new Float32Array(SETTINGS_LENGTH);
  a[0]  = s.glowTau;
  a[1]  = DEFAULT_SETTINGS.pointRadius; // index 1: stale pointRadius UI retired; default-written
  a[2]  = s.neuronVisualRadius;
  a[3]  = s.activeNeuronRadiusBoost;
  a[4]  = s.inactiveNeuronOpacity;
  a[5]  = s.voltageGlowStrength;
  a[6]  = s.connectionVisualWidth;
  a[7]  = s.connectionCurveLift;
  a[8]  = s.connectionLightNext;
  a[9]  = 0; // index 9: reserved_zero (connectionLightPast removed)
  a[10] = 0; // index 10: reserved_zero (bloomStrength removed)
  a[11] = DEFAULT_SETTINGS.surfaceOpacity; // index 11: hidden surface path retired; default-written
  a[12] = s.iExt;
  a[13] = s.synapticScale;
  a[14] = s.heterogeneity;
  a[15] = s.morphRestingOpacity;    // Morphology: resting opacity (0..1)
  a[16] = 0; // index 16: reserved_zero (signalSource removed)
  a[17] = normalizeConnectionLayer(s.connectionLayer);
  a[18] = s.colorBy;
  a[19] = s.neuronVisibility;
  a[20] = DEFAULT_SETTINGS.surface; // index 20: hidden surface path retired; default-written
  a[21] = s.weightNormalization;
  a[22] = s.inputMode;
  a[23] = 0; // index 23: reserved_zero (adaptiveScalerEnabled removed)
  a[24] = s.longRangeReachFrac;   // heavy-tailed reach: long-range fraction (0..1)
  a[25] = s.maxReachCells;        // heavy-tailed reach: max-reach radius (cells)
  a[26] = s.arrivalHoldTicks;     // until-arrival mode: post-arrival full-branch hold (ticks)
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
