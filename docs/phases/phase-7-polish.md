# Phase 7 — Polish (Sound, HUD, SOC Tuning, Mobile)

_The experience is complete. This phase is about quality and feel, not new
architectural work. No new layers; fill in the last stubs._

## Done when
- Sound toggle works; muted by default; audible when enabled.
- Small corner HUD shows live fps and neuron count.
- On a mid-range mobile device the sim runs at 30+ fps with Low tier.
- Excitability near `focused` (≈ 0.55) produces visually interesting
  traveling waves and occasional large cascade events, not boring random
  blinking and not constant seizure.
- 10M neurons is labeled as "best-case discrete GPU max" in docs and UI;
  the adaptive scaler does not promise it on all hardware.

---

## Sonification (BV11)

Spike rate per region is already tracked in the profiler (phase 1+). Wire
it to Web Audio.

### Architecture
- **Muted by default.** A sound toggle button (🔇/🔊) appears in the top bar.
- On unmute: resume AudioContext (requires user gesture — the button click
  qualifies).
- **Voice bank:** 3 oscillators (one per region: input, association, output).
  - Input region → low pitch base (~110 Hz).
  - Association → mid (~220 Hz).
  - Output → high (~440 Hz).
- **Modulation:** each oscillator's gain is driven by the region's normalized
  spike rate (spikes this second / max_expected_spikes).
- **Texture:** add a small amount of noise (AudioNode `createScriptProcessor`
  or `AudioWorklet`) modulated by total spike rate. Near criticality (focused
  state) produces a crackling, rain-like texture.
- **Filter:** low-pass filter on the output bus, cutoff ~800 Hz, to keep it
  from sounding harsh.

### Implementation
```typescript
class SonificationEngine {
  private ctx: AudioContext | null = null;
  private oscillators: OscillatorNode[] = [];
  private gains: GainNode[] = [];
  private masterGain: GainNode;

  enable() {
    this.ctx = new AudioContext();
    const freqs = [110, 220, 440];
    for (let r = 0; r < 3; r++) {
      const osc = this.ctx.createOscillator();
      osc.type = 'sine';
      osc.frequency.value = freqs[r];
      const gain = this.ctx.createGain();
      gain.gain.value = 0;
      osc.connect(gain).connect(this.ctx.destination);
      osc.start();
      this.oscillators.push(osc);
      this.gains.push(gain);
    }
  }

  // Called once per second with region spike rates [input, assoc, output]
  update(rates: [number, number, number], maxRate: number) {
    if (!this.ctx) return;
    const now = this.ctx.currentTime;
    rates.forEach((rate, r) => {
      const normalized = Math.min(rate / (maxRate || 1), 1.0);
      this.gains[r].gain.linearRampToValueAtTime(normalized * 0.3, now + 0.1);
    });
  }

  disable() {
    this.ctx?.close();
    this.ctx = null;
  }
}
```

Keep sound updates off the hot path: update once per profiler dump (1/sec).

---

## Corner HUD (BV8 amendment)

Small, unobtrusive. Bottom-right corner, semi-transparent.

```
fps: 58   |  N: 200k  |  GPU
syn/s: 1.2B
```

Implementation: a `<div>` with `position: fixed; bottom: 8px; right: 8px;
font-family: monospace; font-size: 11px; color: rgba(255,255,255,0.5);
pointer-events: none;`. Updated once per profiler dump. Nothing fancy.

Fields: fps (rolling avg), N (current neuron count), backend (GPU/CPU),
synaptic events/sec (abbreviated). No p95/p99 in the HUD — that stays in
the console JSON dump.

HUD update cadence: once per profiler dump, not every rAF frame. The HUD should
only format already-aggregated counters; it must not trigger GPU readbacks,
allocate large strings repeatedly, or inspect per-neuron state.

Optional debug HUD fields (off by default): render resolution scale, near-LOD
instance count, GPU timing total, and current adaptive-scaler reason. These are
developer aids, not visitor-facing chrome.

---

## SOC tuning

Goal: near `focused` excitability, the network exhibits traveling waves and
occasional large cascade events (neuronal avalanche-ish behavior). This is a
parameter tuning task, not an architectural one.

### Tuning knobs
1. `i_ext` (ambient drive to input regions): too low → silence; too high →
   constant seizure. Sweep from 0.01 to 0.05 at `focused` excitability.
2. `leak_decay`: 0.95 is the starting point (~50ms time constant). Values
   closer to 1.0 (e.g. 0.98) give longer integration windows and more
   synchronization tendency.
3. E/I ratio: 80/20 is the starting point. Shift inhibition up to 25% if
   seizure state is too easily reached.
4. Weight magnitudes: scale the `weight()` function outputs up or down
   uniformly.

### Acceptance criteria for MVP
- `deep_sleep`: nearly silent (< 0.5 Hz mean firing rate).
- `relaxed`: slow waves visible, 1–3 Hz.
- `focused`: traveling waves, occasional cascade bursts, 5–15 Hz.
- `hyperstimulated`: fast, dense activity, 20–40 Hz.
- `seizure`: synchronized burst firing, > 50 Hz, visible as strobing.

Avalanche-size histogram (power-law check) is a quality metric, not a
blocker. If the visual looks right at `focused`, that's MVP.

### Tuning procedure
1. Fix seed, fix N=200k, GPU backend, 1× speed.
2. Sweep `i_ext` ∈ {0.01, 0.02, 0.03, 0.04, 0.05} at excitability=0.55.
3. For each: record mean firing rate from console dump after 10s.
4. Pick the `i_ext` where mean rate is 5–15 Hz.
5. Verify other excitability presets produce expected behavior.
6. Lock the chosen `i_ext` in `SimConfig` defaults.

---

## Mobile scaling

Mobile devices use:
- **Low tier only** (N ≈ 50k, K ≈ 16).
- **GPU backend only** (no rayon workers — too much overhead for small N).
- **Reduced render resolution:** canvas at 0.75× devicePixelRatio.
- **No near LOD** (skip phase 4 frustum cull pass; far LOD only).
- **No enhanced HDR/bloom path** unless benchmarked on the actual device class.
- **No debug overlays or timestamp-query HUD by default.**
- **No sound** (mobile audio context is flaky; disable the toggle on mobile).
- **Cursor stimulation disabled** (hover doesn't exist on touch; rely on I_ext).

Mobile detection:
```typescript
const isMobile = /Mobi|Android/i.test(navigator.userAgent)
  || window.innerWidth < 768;
```

Apply before initializing the backend.

---

## 10M neuron disclaimer

Update the tier UI tooltip and `architecture.md §9`:
- Max tier label: "Max (up to 1M — up to 10M on high-end discrete GPU)".
- Adaptive scaler cap for Max tier: 1M as the practical default; 10M only
  if device reports a large `maxStorageBufferBindingSize` AND the benchmark
  burst sustains the frame budget.
- Add a note to docs: "10M is a best-case discrete GPU target, not a promise."

## Final performance audit

Before shipping, do one pass specifically looking for accidental hot-path costs:

- no CPU readbacks in normal rAF frames;
- no per-frame creation of buffers, bind groups, pipelines, textures, arrays of
  per-neuron/per-synapse data, or string-keyed spatial maps;
- timestamp query resolve/mapping is async and skipped when staging buffers are
  busy;
- debug overlays are off by default and do not run hidden passes;
- render targets recreate only on size/format changes;
- backend restart/tier resize tears down and rebuilds cleanly with the same seed;
- profiler counters derive inner-loop totals cheaply (`spikes × K`) where
  possible.

Add the audit result to the final phase notes with the measured default
balanced-tier numbers and the machine/device used.

---

## Deferred to future work (do not implement in phase 7)
- Click-to-inspect neuron (BV10 deferred).
- Side-by-side CPU/GPU race mode (BV12 deferred).
- GLIF neuron model upgrade (BV5).
- STDP / synaptic plasticity.
- Avalanche-size histogram in UI.
- Auto-tier selection heuristic that chooses Low/Balanced/Max for the visitor
  (BV3 deferred; adaptive resizing within the selected tier already exists).
- Richer near-LOD neuron geometry (dendrite hints).
