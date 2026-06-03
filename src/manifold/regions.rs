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

/// Assign a region to each position by its projection onto `axis`.
///
/// The split is by rank (percentile), not by absolute dot value, so it is
/// robust to the folded surface's irregular extent. `axis` need not be unit
/// length — only the ordering of dot products matters.
pub fn assign_regions(positions: &[[f32; 3]], axis: [f32; 3]) -> Vec<RegionKind> {
    let n = positions.len();
    if n == 0 {
        return Vec::new();
    }

    // Dot product of each position with the axis.
    let dots: Vec<f32> = positions
        .iter()
        .map(|p| p[0] * axis[0] + p[1] * axis[1] + p[2] * axis[2])
        .collect();

    // Rank by dot via an index sort (ascending). Lowest dot = anterior (Output).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| dots[a].partial_cmp(&dots[b]).unwrap_or(std::cmp::Ordering::Equal));

    let bottom = (n as f32 * 0.30).round() as usize; // Output count
    let top = (n as f32 * 0.30).round() as usize; // Input count

    let mut regions = vec![RegionKind::Association; n];
    for (rank, &idx) in order.iter().enumerate() {
        if rank < bottom {
            regions[idx] = RegionKind::Output; // lowest dot = anterior
        } else if rank >= n - top {
            regions[idx] = RegionKind::Input; // highest dot = posterior
        }
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_is_roughly_30_40_30() {
        // 1000 points spread along the axis.
        let positions: Vec<[f32; 3]> = (0..1000)
            .map(|i| [0.0, 0.0, i as f32 / 1000.0])
            .collect();
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
    fn input_is_posterior_output_anterior() {
        let positions: Vec<[f32; 3]> = (0..100)
            .map(|i| [0.0, 0.0, i as f32])
            .collect();
        let regions = assign_regions(&positions, [0.0, 0.0, 1.0]);
        // Highest z (index 99) should be Input; lowest (index 0) Output.
        assert_eq!(regions[99], RegionKind::Input);
        assert_eq!(regions[0], RegionKind::Output);
    }

    #[test]
    fn empty_input() {
        assert!(assign_regions(&[], [1.0, 0.0, 0.0]).is_empty());
    }
}
