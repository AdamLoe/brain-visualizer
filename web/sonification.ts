// Sonification engine (BV11 — Phase 7).
//
// Muted by default. A sound-toggle button click calls enable() to create the
// AudioContext (fulfills the user-gesture requirement). The profiler dumps
// per-region spike fractions once per second; call update() at that cadence —
// NEVER in the hot rAF path.
//
// Voice bank: 3 sine oscillators (input / association / output cortical
// regions), gain driven by normalized per-region spike rate.  A small noise
// texture (ScriptProcessorNode) is modulated by total firing rate.
// A low-pass filter (~800 Hz) on the output bus prevents harshness.
//
// Per the spec: disabled on mobile (audio context is flaky; the enable()
// method is never exposed when isMobile() is true).
//
// Architecture §13 / BV11.

// Region indices — must match cortical region codes in backend.rs.
export const REGION_INPUT  = 0;  // posterior sensory   → 110 Hz
export const REGION_ASSOC  = 1;  // prefrontal assoc    → 220 Hz
export const REGION_OUTPUT = 2;  // anterior motor      → 440 Hz

// Tuning constants.
const BASE_FREQS: [number, number, number] = [110, 220, 440];
const MAX_GAIN     = 0.3;   // each oscillator's peak amplitude
const NOISE_MAX    = 0.04;  // noise layer peak amplitude
const FILTER_CUTOFF = 800;  // Hz — low-pass to avoid harshness
const RAMP_TIME    = 0.1;   // seconds — gain ramp per update call

export class SonificationEngine {
  private ctx: AudioContext | null = null;
  private oscillators: OscillatorNode[] = [];
  private gains: GainNode[] = [];
  private noiseGain: GainNode | null = null;
  // eslint-disable-next-line deprecation/no-deprecated
  private noiseProc: ScriptProcessorNode | null = null;
  private masterGain: GainNode | null = null;
  private filter: BiquadFilterNode | null = null;
  private _enabled = false;

  get enabled(): boolean {
    return this._enabled;
  }

  /**
   * Create AudioContext and start oscillators. Call only on a user-gesture
   * (e.g. button click) — browsers require this.
   */
  enable(): void {
    if (this._enabled || this.ctx) return;
    try {
      this.ctx = new AudioContext();
    } catch {
      console.warn("[sonification] AudioContext creation failed");
      return;
    }

    // Low-pass filter on the master bus.
    this.filter = this.ctx.createBiquadFilter();
    this.filter.type = "lowpass";
    this.filter.frequency.value = FILTER_CUTOFF;

    // Master gain (controls overall volume; keep headroom).
    this.masterGain = this.ctx.createGain();
    this.masterGain.gain.value = 1.0;

    this.filter.connect(this.masterGain);
    this.masterGain.connect(this.ctx.destination);

    // 3 sine oscillators — one per cortical region.
    for (let r = 0; r < 3; r++) {
      const osc = this.ctx.createOscillator();
      osc.type = "sine";
      osc.frequency.value = BASE_FREQS[r];

      const gain = this.ctx.createGain();
      gain.gain.value = 0;

      osc.connect(gain);
      gain.connect(this.filter);
      osc.start();

      this.oscillators.push(osc);
      this.gains.push(gain);
    }

    // Noise texture (white noise, modulated by total firing rate).
    this._startNoise();

    this._enabled = true;
    console.log("[sonification] enabled (Web Audio)");
  }

  /** Close AudioContext and tear down all nodes. */
  disable(): void {
    if (!this.ctx) return;
    this.ctx.close().catch(() => undefined);
    this.ctx = null;
    this.oscillators = [];
    this.gains = [];
    this.noiseGain = null;
    this.noiseProc = null;
    this.masterGain = null;
    this.filter = null;
    this._enabled = false;
    console.log("[sonification] disabled");
  }

  /**
   * Update oscillator gains from per-region rates. Called once per profiler
   * dump (~1 Hz) — NOT in the hot rAF path.
   *
   * @param regionFractions - [input, assoc, output] fractions of neurons that
   *   fired in this second (spikes / (n_region * ticks_per_sec)). Range [0,1].
   * @param totalFraction   - Overall firing fraction across all regions. Used
   *   to modulate noise texture intensity.
   */
  update(
    regionFractions: [number, number, number],
    totalFraction: number,
  ): void {
    if (!this.ctx || !this._enabled) return;
    const now = this.ctx.currentTime;

    for (let r = 0; r < 3; r++) {
      const normalized = Math.min(Math.max(regionFractions[r], 0), 1);
      this.gains[r].gain.linearRampToValueAtTime(
        normalized * MAX_GAIN,
        now + RAMP_TIME,
      );
    }

    // Noise level tracks total firing fraction — critical regime produces
    // a rain-like crackling texture (SOC / BV9).
    if (this.noiseGain) {
      const noiseLevel = Math.min(Math.max(totalFraction, 0), 1) * NOISE_MAX;
      this.noiseGain.gain.linearRampToValueAtTime(noiseLevel, now + RAMP_TIME);
    }
  }

  // ── Private ────────────────────────────────────────────────────────────────

  private _startNoise(): void {
    if (!this.ctx || !this.filter) return;

    // ScriptProcessorNode is deprecated but still universally supported.
    // An AudioWorklet is the modern alternative; deferred as an enhancement.
    // eslint-disable-next-line deprecation/no-deprecated
    const bufSize = 4096;
    try {
      // eslint-disable-next-line deprecation/no-deprecated
      this.noiseProc = this.ctx.createScriptProcessor(bufSize, 0, 1);
    } catch {
      // Some browsers may reject ScriptProcessorNode — harmless, noise is optional.
      console.warn("[sonification] ScriptProcessorNode unavailable; noise disabled");
      return;
    }
    this.noiseProc.onaudioprocess = (ev) => {
      const out = ev.outputBuffer.getChannelData(0);
      for (let i = 0; i < out.length; i++) {
        out[i] = Math.random() * 2 - 1; // white noise
      }
    };

    this.noiseGain = this.ctx.createGain();
    this.noiseGain.gain.value = 0; // starts silent

    this.noiseProc.connect(this.noiseGain);
    this.noiseGain.connect(this.filter);
  }
}

/**
 * Derive per-region spike fractions from a single-second profiler snapshot.
 *
 * Since the JS profiler tracks total spikes but not per-region split, we
 * approximate using the anatomical region fractions (~30% input / 40% assoc /
 * 30% output) that the manifold generates. This is a visual/audio effect —
 * sub-Hz accuracy is irrelevant.  For the browser the per-region numbers
 * are close enough for the auditory experience.
 *
 * @param spikesPerSec   - Total spikes/sec from the profiler dump.
 * @param n              - Current neuron count.
 * @param ticksPerSec    - Simulated ticks/sec.
 */
export function deriveRegionFractions(
  spikesPerSec: number,
  n: number,
  ticksPerSec: number,
): [number, number, number] {
  if (n <= 0 || ticksPerSec <= 0) return [0, 0, 0];
  // Approximate region sizes: 30% / 40% / 30%.
  const REGION_FRAC: [number, number, number] = [0.30, 0.40, 0.30];
  const overallRate = spikesPerSec / (n * ticksPerSec); // firings/tick
  return REGION_FRAC.map((f) => Math.min(overallRate / f, 1)) as [number, number, number];
}
