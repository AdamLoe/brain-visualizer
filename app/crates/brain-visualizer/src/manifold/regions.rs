//! Cortical region assignment.
//!
//! Default assignment preserves the production hash-random 30/40/30 split.
//! An opt-in prototype can assign the same split with soft anterior-posterior
//! coherence for visual review.

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

/// Host-side region assignment strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionAssignmentMode {
    /// Production default: deterministic hash shuffle, spatially random.
    HashRandom,
    /// Internal prototype: posterior-biased input and anterior-biased output.
    AnteriorPosteriorPrototype,
}

/// Assign a region to each neuron randomly (uniform over the volume).
/// 30% Input, 40% Association, 30% Output — same proportions as before but
/// scattered throughout the sphere rather than spatially blocked.
pub fn assign_regions(positions: &[[f32; 3]], _axis: [f32; 3]) -> Vec<RegionKind> {
    assign_regions_with_mode(positions, _axis, RegionAssignmentMode::HashRandom)
}

/// Assign a region using the requested host-side strategy.
pub fn assign_regions_with_mode(
    positions: &[[f32; 3]],
    axis: [f32; 3],
    mode: RegionAssignmentMode,
) -> Vec<RegionKind> {
    match mode {
        RegionAssignmentMode::HashRandom => assign_hash_random_regions(positions.len()),
        RegionAssignmentMode::AnteriorPosteriorPrototype => {
            assign_anterior_posterior_prototype_regions(positions, axis)
        }
    }
}

fn assign_hash_random_regions(n: usize) -> Vec<RegionKind> {
    if n == 0 {
        return Vec::new();
    }

    let (input_end, assoc_end) = split_bounds(n);

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

fn assign_anterior_posterior_prototype_regions(
    positions: &[[f32; 3]],
    axis: [f32; 3],
) -> Vec<RegionKind> {
    let n = positions.len();
    if n == 0 {
        return Vec::new();
    }

    let axis = normalize_axis(axis);
    let projections: Vec<f32> = positions
        .iter()
        .map(|&pos| finite(dot(pos, axis)))
        .collect();
    let (min_projection, max_projection) = projections.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(min, max), &projection| (min.min(projection), max.max(projection)),
    );
    let jitter_span = (max_projection - min_projection).max(1e-4) * 0.18;

    let mut scored: Vec<(usize, f32, u32)> = positions
        .iter()
        .enumerate()
        .map(|(i, &pos)| {
            let key = position_key(i, pos, 0x9e37_79b9);
            let jitter = (hash_to_unit(key) * 2.0 - 1.0) * jitter_span;
            (i, projections[i] + jitter, key)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    let (input_end, assoc_end) = split_bounds(n);
    let mut regions = vec![RegionKind::Association; n];
    for &(idx, _, _) in &scored[..input_end] {
        regions[idx] = RegionKind::Input;
    }
    for &(idx, _, _) in &scored[assoc_end..] {
        regions[idx] = RegionKind::Output;
    }
    regions
}

fn split_bounds(n: usize) -> (usize, usize) {
    (
        (n as f32 * 0.30).round() as usize,
        (n as f32 * 0.70).round() as usize,
    )
}

#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn normalize_axis(axis: [f32; 3]) -> [f32; 3] {
    let len = dot(axis, axis).sqrt();
    if len <= 1e-6 {
        [0.0, 0.0, 1.0]
    } else {
        [axis[0] / len, axis[1] / len, axis[2] / len]
    }
}

#[inline]
fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn position_key(index: usize, pos: [f32; 3], salt: u32) -> u32 {
    let mut key = scramble((index as u32).wrapping_mul(0x85eb_ca6b) ^ salt);
    key = scramble(key ^ pos[0].to_bits());
    key = scramble(key ^ pos[1].to_bits().rotate_left(11));
    scramble(key ^ pos[2].to_bits().rotate_left(22))
}

#[inline]
fn hash_to_unit(h: u32) -> f32 {
    (h as f32) / (u32::MAX as f32 + 1.0)
}

#[inline]
fn scramble(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(regions: &[RegionKind], kind: RegionKind) -> usize {
        regions.iter().filter(|&&region| region == kind).count()
    }

    fn mean_projection(
        positions: &[[f32; 3]],
        regions: &[RegionKind],
        kind: RegionKind,
        axis: [f32; 3],
    ) -> f32 {
        let mut sum = 0.0;
        let mut n = 0usize;
        for (&position, &region) in positions.iter().zip(regions) {
            if region == kind {
                sum += dot(position, axis);
                n += 1;
            }
        }
        sum / n as f32
    }

    #[test]
    fn default_split_is_roughly_30_40_30() {
        // 1000 points spread along the axis.
        let positions: Vec<[f32; 3]> = (0..1000).map(|i| [0.0, 0.0, i as f32 / 1000.0]).collect();
        let regions = assign_regions(&positions, [0.0, 0.0, 1.0]);

        let input = count(&regions, RegionKind::Input);
        let assoc = count(&regions, RegionKind::Association);
        let output = count(&regions, RegionKind::Output);

        assert_eq!(input + assoc + output, 1000);
        assert!((280..=320).contains(&input), "input {input}");
        assert!((280..=320).contains(&output), "output {output}");
        assert!((360..=440).contains(&assoc), "assoc {assoc}");
    }

    #[test]
    fn default_assignment_is_deterministic() {
        let positions: Vec<[f32; 3]> = (0..100).map(|i| [i as f32, 0.0, 0.0]).collect();
        let a = assign_regions(&positions, [0.0, 0.0, 1.0]);
        let b = assign_regions(&positions, [0.0, 0.0, 1.0]);
        assert_eq!(a, b);
    }

    #[test]
    fn default_assignment_ignores_positions_and_axis() {
        let a_positions: Vec<[f32; 3]> = (0..256).map(|i| [0.0, 0.0, i as f32]).collect();
        let b_positions: Vec<[f32; 3]> = (0..256).map(|i| [i as f32, -3.0, 2.0]).collect();
        let a = assign_regions(&a_positions, [0.0, 0.0, 1.0]);
        let b = assign_regions(&b_positions, [1.0, 0.0, 0.0]);
        assert_eq!(a, b);
    }

    #[test]
    fn empty_input() {
        assert!(assign_regions(&[], [1.0, 0.0, 0.0]).is_empty());
    }

    #[test]
    fn prototype_empty_input() {
        assert!(assign_regions_with_mode(
            &[],
            [0.0, 0.0, 1.0],
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        )
        .is_empty());
    }

    #[test]
    fn prototype_preserves_exact_30_40_30_split() {
        let positions: Vec<[f32; 3]> = (0..1000)
            .map(|i| {
                let z = i as f32 / 999.0 * 2.0 - 1.0;
                [0.05 * (i as f32).sin(), 0.0, z]
            })
            .collect();
        let regions = assign_regions_with_mode(
            &positions,
            [0.0, 0.0, 1.0],
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );
        assert_eq!(count(&regions, RegionKind::Input), 300);
        assert_eq!(count(&regions, RegionKind::Association), 400);
        assert_eq!(count(&regions, RegionKind::Output), 300);
    }

    #[test]
    fn prototype_assignment_is_deterministic() {
        let positions: Vec<[f32; 3]> = (0..512)
            .map(|i| {
                let t = i as f32 / 511.0;
                [t.sin(), t.cos() * 0.25, t * 2.0 - 1.0]
            })
            .collect();
        let a = assign_regions_with_mode(
            &positions,
            [0.0, 0.0, 1.0],
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );
        let b = assign_regions_with_mode(
            &positions,
            [0.0, 0.0, 1.0],
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn prototype_biases_input_posterior_and_output_anterior() {
        let positions: Vec<[f32; 3]> = (0..2000)
            .map(|i| {
                let z = i as f32 / 1999.0 * 2.0 - 1.0;
                let t = i as f32 * 0.017;
                [t.sin() * 0.08, t.cos() * 0.04, z]
            })
            .collect();
        let axis = [0.0, 0.0, 1.0];
        let regions = assign_regions_with_mode(
            &positions,
            axis,
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );

        let input_mean = mean_projection(&positions, &regions, RegionKind::Input, axis);
        let assoc_mean = mean_projection(&positions, &regions, RegionKind::Association, axis);
        let output_mean = mean_projection(&positions, &regions, RegionKind::Output, axis);
        assert!(
            input_mean > assoc_mean,
            "input={input_mean} assoc={assoc_mean}"
        );
        assert!(
            assoc_mean > output_mean,
            "assoc={assoc_mean} output={output_mean}"
        );
        assert!(input_mean > 0.35, "input={input_mean}");
        assert!(output_mean < -0.35, "output={output_mean}");
    }

    #[test]
    fn prototype_softly_mixes_boundary_neurons() {
        let positions: Vec<[f32; 3]> = (0..2000)
            .map(|i| [0.0, 0.0, i as f32 / 1999.0 * 2.0 - 1.0])
            .collect();
        let regions = assign_regions_with_mode(
            &positions,
            [0.0, 0.0, 1.0],
            RegionAssignmentMode::AnteriorPosteriorPrototype,
        );

        let input_min = positions
            .iter()
            .zip(&regions)
            .filter_map(|(&position, &region)| (region == RegionKind::Input).then_some(position[2]))
            .fold(f32::INFINITY, f32::min);
        let assoc_max = positions
            .iter()
            .zip(&regions)
            .filter_map(|(&position, &region)| {
                (region == RegionKind::Association).then_some(position[2])
            })
            .fold(f32::NEG_INFINITY, f32::max);
        let output_max = positions
            .iter()
            .zip(&regions)
            .filter_map(|(&position, &region)| {
                (region == RegionKind::Output).then_some(position[2])
            })
            .fold(f32::NEG_INFINITY, f32::max);
        let assoc_min = positions
            .iter()
            .zip(&regions)
            .filter_map(|(&position, &region)| {
                (region == RegionKind::Association).then_some(position[2])
            })
            .fold(f32::INFINITY, f32::min);

        assert!(
            input_min < assoc_max,
            "input_min={input_min} assoc_max={assoc_max}"
        );
        assert!(
            output_max > assoc_min,
            "output_max={output_max} assoc_min={assoc_min}"
        );
    }
}
