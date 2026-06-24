//! Until-arrival mode-2 fade-factor shape, mirroring the WGSL `arrival_fade_factor`
//! in `render_morphology.wgsl` exactly. The render fade is GPU-only, so this is a
//! deterministic CPU mirror of the load-bearing math (same pattern as the other
//! `wgsl_*` mirror tests): it pins the ramp window and the hold==0 guard so a
//! refactor that breaks the [28 .. 28+hold] shape fails host-side without a GPU.
//!
//! The two constants below MUST match `render_morphology.wgsl`:
//!   - ARRIVAL_MODE_MAX_TRAVEL_TICKS = 28.0 (also mirrored from compaction)
//!   - the fade expression: 1 - clamp((age - 28) / max(hold, 1), 0, 1)

const ARRIVAL_MODE_MAX_TRAVEL_TICKS: f32 = 28.0;

/// Exact mirror of the WGSL `arrival_fade_factor(arrival_age, hold_ticks)`.
fn arrival_fade_factor(arrival_age: f32, hold_ticks: f32) -> f32 {
    let hold = hold_ticks.max(0.0);
    let denom = hold.max(1.0);
    1.0 - ((arrival_age - ARRIVAL_MODE_MAX_TRAVEL_TICKS) / denom).clamp(0.0, 1.0)
}

#[test]
fn full_brightness_at_or_before_travel_window() {
    // age <= 28 → 1.0 (subdued rest value unchanged from the hard-cut behaviour).
    assert_eq!(arrival_fade_factor(0.0, 30.0), 1.0);
    assert_eq!(arrival_fade_factor(20.0, 30.0), 1.0);
    assert_eq!(arrival_fade_factor(28.0, 30.0), 1.0);
}

#[test]
fn ramps_monotonically_to_zero_across_hold_window() {
    let hold = 30.0;
    // Across [28 .. 58] the factor strictly decreases from 1.0 toward 0.0.
    let mut prev = arrival_fade_factor(28.0, hold);
    assert_eq!(prev, 1.0);
    for age in [34.0, 40.0, 46.0, 52.0, 58.0] {
        let f = arrival_fade_factor(age, hold);
        assert!(f < prev, "fade must decrease: age {age} gave {f} >= {prev}");
        assert!((0.0..=1.0).contains(&f));
        prev = f;
    }
    // Halfway through the window (age 43 = 28 + 15) the factor is ~0.5.
    assert!((arrival_fade_factor(43.0, hold) - 0.5).abs() < 1e-5);
}

#[test]
fn fully_faded_at_and_after_compaction_drop_point() {
    let hold = 30.0;
    // age >= 28 + hold → 0.0 (matches the compaction drop at 28 + arrival_hold_ticks).
    assert_eq!(arrival_fade_factor(58.0, hold), 0.0);
    assert_eq!(arrival_fade_factor(100.0, hold), 0.0);
}

#[test]
fn hold_zero_does_not_divide_by_zero_and_drops_immediately() {
    // hold == 0: denom clamps to 1, so age 28 → 1.0 and age 29 → 0.0. Compaction
    // already drops the segment at age 28, so there is no regression vs. hard-cut.
    assert_eq!(arrival_fade_factor(28.0, 0.0), 1.0);
    assert_eq!(arrival_fade_factor(29.0, 0.0), 0.0);
    assert!(arrival_fade_factor(28.5, 0.0).is_finite());
}
