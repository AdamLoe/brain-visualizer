/**
 * Boot-load overlay helpers (boot-load overhaul, channel B).
 *
 * Pure formatting/mapping logic for the startup overlay, split out of `main.ts`
 * so the boot-panel contract can be unit-tested without importing `main.ts`
 * (which runs `boot()` at module load and requires a DOM + WebGPU adapter).
 */

/**
 * Format a sub-stage label with its WITHIN-STAGE percent appended, e.g.
 * `formatSubStageLabel("Prepare network payload", 0.42)` →
 * `"Prepare network payload 42%"`. The fraction is clamped to [0,1] so a stray
 * out-of-range value can never render a nonsensical percent.
 */
export function formatSubStageLabel(label: string, fraction: number): string {
  const clamped = clampFraction(fraction);
  return `${label} ${Math.round(clamped * 100)}%`;
}

/**
 * Map an in-stage fraction (0..1) onto the current stage's [start, end] progress
 * band. Used so the overall bar advances continuously as a stage's sub-stage
 * callback ticks, instead of freezing at the band start and snapping at the end.
 */
export function mapSubStageProgress(
  fraction: number,
  bandStart: number,
  bandEnd: number,
): number {
  return bandStart + clampFraction(fraction) * (bandEnd - bandStart);
}

function clampFraction(fraction: number): number {
  return Math.max(0, Math.min(1, fraction));
}
