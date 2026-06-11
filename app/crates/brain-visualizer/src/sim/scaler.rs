//! Adaptive scaler — STUB in phase 1 (BV1, BV3, architecture §9).
//!
//! Phase 1: reads `SimConfig`, logs a *proposed* N/K based on the frame budget,
//! but performs **no resize**. The scaler operates strictly *inside* the
//! selected tier (never jumps Low/Balanced/Max) and uses hysteresis + cooldown
//! when it becomes real. The proposal math here is host-testable.

use crate::sim::backend::{SimConfig, Tier, PRODUCT_MAX_N};

/// Per-tier (N, K) ranges. The scaler may compress/expand within these, but all
/// ranges remain below the product cap.
#[derive(Debug, Clone, Copy)]
pub struct TierRange {
    pub n_min: usize,
    pub n_max: usize,
    pub k_min: usize,
    pub k_max: usize,
}

impl TierRange {
    pub fn for_tier(tier: Tier) -> Self {
        match tier {
            Tier::Low => TierRange {
                n_min: 1_000,
                n_max: 10_000,
                k_min: 16,
                k_max: 32,
            },
            Tier::Balanced => TierRange {
                n_min: 5_000,
                n_max: 15_000,
                k_min: 32,
                k_max: 64,
            },
            Tier::Max => TierRange {
                n_min: 10_000,
                n_max: PRODUCT_MAX_N,
                k_min: 64,
                k_max: 128,
            },
        }
    }
}

/// A proposed scaling change. In phase 1 this is logged, never applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleProposal {
    pub proposed_n: usize,
    pub proposed_k: usize,
    pub grow: bool,
}

/// Target frame budget (ms) — ~60 fps with headroom (architecture §9).
pub const TARGET_FRAME_MS: f32 = 12.0;

/// Stateless proposal: given the current config and the measured frame time,
/// propose grow/shrink within the tier range. Pure & testable. Phase 1 callers
/// log the result and do not resize.
pub fn propose(config: &SimConfig, frame_ms_avg: f32) -> ScaleProposal {
    let range = TierRange::for_tier(config.tier);

    // Shrink quickly when over budget; grow only with sustained headroom
    // (the "sustained" part is a cooldown the real scaler will add).
    let over_budget = frame_ms_avg > TARGET_FRAME_MS;
    let grow = frame_ms_avg < TARGET_FRAME_MS * 0.8;

    let (proposed_n, proposed_k) = if over_budget {
        // Shrink N by ~15%, clamp to tier min.
        let n = ((config.n as f32 * 0.85) as usize).max(range.n_min);
        let k = config.k.max(range.k_min);
        (n, k)
    } else if grow {
        // Grow N by ~10%, clamp to tier max.
        let n = ((config.n as f32 * 1.10) as usize).min(range.n_max);
        let k = config.k.min(range.k_max);
        (n, k)
    } else {
        (config.n, config.k)
    };

    ScaleProposal {
        proposed_n,
        proposed_k,
        grow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(n: usize, tier: Tier) -> SimConfig {
        SimConfig {
            n,
            tier,
            ..SimConfig::default()
        }
    }

    #[test]
    fn over_budget_shrinks() {
        let c = cfg(10_000, Tier::Balanced);
        let p = propose(&c, 20.0);
        assert!(p.proposed_n < c.n);
        assert!(!p.grow);
    }

    #[test]
    fn under_budget_grows_within_tier() {
        let c = cfg(10_000, Tier::Balanced);
        let p = propose(&c, 8.0);
        assert!(p.proposed_n > c.n);
        assert!(p.proposed_n <= TierRange::for_tier(Tier::Balanced).n_max);
        assert!(p.grow);
    }

    #[test]
    fn shrink_clamps_to_tier_min() {
        let c = cfg(5_000, Tier::Balanced);
        let p = propose(&c, 100.0);
        assert!(p.proposed_n >= TierRange::for_tier(Tier::Balanced).n_min);
    }

    #[test]
    fn near_budget_holds_steady() {
        let c = cfg(10_000, Tier::Balanced);
        let p = propose(&c, TARGET_FRAME_MS); // exactly at budget
        assert_eq!(p.proposed_n, c.n);
    }

    #[test]
    fn tier_ranges_respect_product_cap() {
        for tier in [Tier::Low, Tier::Balanced, Tier::Max] {
            let range = TierRange::for_tier(tier);
            assert!(range.n_min <= range.n_max);
            assert!(range.n_max <= PRODUCT_MAX_N);
        }
    }
}
