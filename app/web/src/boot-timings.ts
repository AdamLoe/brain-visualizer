/**
 * Boot timings + stall watchdog (boot speed/observability).
 *
 * Owns three things, all additive to the existing overlay:
 *  1. `window.__bvBootTimings` — a structured `{ stage, ms }[]` covering Phase-A
 *     main.ts steps, Phase-B boot-sequencer stages, and the worker payload
 *     sub-phases. The user pastes this from real hardware to confirm GPU-stage
 *     timings (acquire / pipeline-compile / first-frame) that can't be measured
 *     on this no-adapter box.
 *  2. A clean end-of-boot console summary (console.table) emitted at "Ready".
 *  3. A DEV-only stall watchdog: if the overlay's step label AND its percent are
 *     both unchanged for > 2s during boot, warn. This encodes the product rule
 *     "a boot step taking > 2s is a signal something is wrong" as a live
 *     diagnostic. Cheap (a single interval) and cleared at "Ready".
 *
 * The stall-detection decision is split into a pure function
 * (`evaluateStall`) so it can be unit-tested without timers or a DOM.
 */

export interface BootTiming {
  stage: string;
  ms: number;
}

interface BootTimingWindow {
  __bvBootTimings?: BootTiming[];
}

/** Append a `{ stage, ms }` row to `window.__bvBootTimings` (creates the array
 * on first use). Uses `globalThis` so it works on the main thread (where it
 * lands on `window`) and in tests; the array is the same object the browser
 * console reads as `window.__bvBootTimings`. */
export function recordBootTiming(stage: string, ms: number): void {
  const g = globalThis as unknown as BootTimingWindow;
  (g.__bvBootTimings ??= []).push({ stage, ms: Math.round(ms * 10) / 10 });
}

/** Read the current boot-timings array (empty when none recorded yet). */
export function getBootTimings(): BootTiming[] {
  return (globalThis as unknown as BootTimingWindow).__bvBootTimings ?? [];
}

/** Emit one clean console.table of the recorded boot timings. Called once at
 * "Ready". Falls back to a plain log if console.table is unavailable. */
export function logBootSummary(): void {
  const rows = getBootTimings();
  if (rows.length === 0) return;
  const total = rows.reduce((sum, r) => sum + r.ms, 0);
  // eslint-disable-next-line no-console
  if (typeof console.table === "function") {
    console.table(rows);
  } else {
    for (const r of rows) console.log(`[boot] ${r.stage}: ${r.ms}ms`);
  }
  console.log(`[boot] total boot ${total.toFixed(1)}ms across ${rows.length} steps`);
}

/** A snapshot of the live overlay step the watchdog samples. */
export interface BootStepSnapshot {
  /** The current step's label (without any appended "NN%"). */
  label: string;
  /** The current overall progress percent (0..100), rounded as displayed. */
  percent: number;
}

export interface StallState {
  lastLabel: string;
  lastPercent: number;
  /** Timestamp (ms) when the current (label, percent) pair was first seen. */
  unchangedSinceMs: number;
  /** Whether a warning has already fired for the current stuck pair (so the
   * watchdog warns once per stall, not every poll). */
  warned: boolean;
}

export interface StallDecision {
  next: StallState;
  /** Non-null when the watchdog should warn THIS tick. */
  warning: { label: string; percent: number } | null;
}

/**
 * Pure stall decision. Given the previous state, the current snapshot, the
 * current time, and the threshold, decide whether to warn and compute the next
 * state. A step is "stalled" when BOTH its label and its percent have been
 * unchanged for longer than `thresholdMs`. Warns at most once per stuck pair.
 */
export function evaluateStall(
  prev: StallState | null,
  snapshot: BootStepSnapshot,
  nowMs: number,
  thresholdMs: number,
): StallDecision {
  const changed =
    prev === null ||
    prev.lastLabel !== snapshot.label ||
    prev.lastPercent !== snapshot.percent;

  if (changed) {
    return {
      next: {
        lastLabel: snapshot.label,
        lastPercent: snapshot.percent,
        unchangedSinceMs: nowMs,
        warned: false,
      },
      warning: null,
    };
  }

  const elapsed = nowMs - prev.unchangedSinceMs;
  if (elapsed > thresholdMs && !prev.warned) {
    return {
      next: { ...prev, warned: true },
      warning: { label: snapshot.label, percent: snapshot.percent },
    };
  }
  return { next: prev, warning: null };
}

/** Default stall threshold: a boot step (label + %) unchanged longer than this
 * is the "something is wrong" signal. */
export const BOOT_STALL_THRESHOLD_MS = 2000;

export interface BootWatchdog {
  stop(): void;
}

/**
 * Start a dev-only stall watchdog. Polls `sample()` on an interval; when the
 * step (label + percent) is unchanged for > `thresholdMs` it `console.warn`s
 * once. Returns a handle whose `stop()` clears the interval (call it at
 * "Ready"). No-op outside a browser (no `setInterval`/`window`).
 */
export function startBootWatchdog(
  sample: () => BootStepSnapshot,
  options: {
    thresholdMs?: number;
    intervalMs?: number;
    now?: () => number;
    warn?: (message: string) => void;
  } = {},
): BootWatchdog {
  const thresholdMs = options.thresholdMs ?? BOOT_STALL_THRESHOLD_MS;
  const intervalMs = options.intervalMs ?? 500;
  const now = options.now ?? (() => performance.now());
  const warn =
    options.warn ?? ((m: string) => console.warn(m));

  if (typeof setInterval === "undefined") {
    return { stop() {} };
  }

  let state: StallState | null = null;
  const handle = setInterval(() => {
    const snapshot = sample();
    const decision = evaluateStall(state, snapshot, now(), thresholdMs);
    state = decision.next;
    if (decision.warning) {
      warn(
        `[boot] step '${decision.warning.label}' stalled >${(thresholdMs / 1000).toFixed(0)}s ` +
          `at ${decision.warning.percent}%`,
      );
    }
  }, intervalMs);

  return {
    stop() {
      clearInterval(handle);
    },
  };
}
