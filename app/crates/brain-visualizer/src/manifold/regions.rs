//! Cortical region assignment (BV17).
//!
//! Splits neurons into Input / Association / Output classes by their position
//! along a fixed anterior–posterior axis:
//! - top 30% of the dot product → Input (posterior, sensory analog);
//! - bottom 30% → Output (anterior, motor analog);
//! - the middle 40% → Association.
//!
//! Pure & host-testable. Only assignment lives here.

/// Region class. Stored per-neuron and packed into the `type` byte upstream
/// (BV21); this enum is the host-side representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// Posterior — receives ambient `I_ext` drive.
    Input,
    /// Central — pure relay.
    Association,
    /// Anterior — no special treatment.
    Output,
}

/// Assign a region to each neuron randomly (uniform over the volume).
/// 30% Input, 40% Association, 30% Output — same proportions as before but
/// scattered throughout the sphere rather than spatially blocked.
pub fn assign_regions(positions: &[[f32; 3]], _axis: [f32; 3]) -> Vec<RegionKind> {
    let n = positions.len();
    if n == 0 {
        return Vec::new();
    }

    let input_end = (n as f32 * 0.30).round() as usize;
    let assoc_end = (n as f32 * 0.70).round() as usize;

    // Shuffle indices with a deterministic hash so assignment is stable across
    // rebuilds with the same N but spatially random.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| {
        let i = i as u32;
        i.wrapping_mul(2654435761)
            .wrapping_add(i.wrapping_mul(1234567891) >> 4)
    });

    let mut regions = vec![RegionKind::Association; n];
    for &idx in &order[..input_end] {
        regions[idx] = RegionKind::Input;
    }
    for &idx in &order[assoc_end..] {
        regions[idx] = RegionKind::Output;
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_is_roughly_30_40_30() {
        // 1000 points spread along the axis.
        let positions: Vec<[f32; 3]> = (0..1000).map(|i| [0.0, 0.0, i as f32 / 1000.0]).collect();
        let regions = assign_regions(&positions, [0.0, 0.0, 1.0]);

        let count = |k: RegionKind| regions.iter().filter(|&&r| r == k).count();
        let input = count(RegionKind::Input);
        let assoc = count(RegionKind::Association);
        let output = count(RegionKind::Output);

        assert_eq!(input + assoc + output, 1000);
        assert!((280..=320).contains(&input), "input {input}");
        assert!((280..=320).contains(&output), "output {output}");
        assert!((360..=440).contains(&assoc), "assoc {assoc}");
    }

    #[test]
    fn assignment_is_deterministic() {
        let positions: Vec<[f32; 3]> = (0..100).map(|i| [i as f32, 0.0, 0.0]).collect();
        let a = assign_regions(&positions, [0.0, 0.0, 1.0]);
        let b = assign_regions(&positions, [0.0, 0.0, 1.0]);
        assert_eq!(a, b);
    }

    #[test]
    fn empty_input() {
        assert!(assign_regions(&[], [1.0, 0.0, 0.0]).is_empty());
    }
}
