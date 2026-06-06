//! Morphology — procedural per-neuron geometry (V2 Beauty-First).
//!
//! Each neuron is drawn not as an abstract billboard glow alone but as a real
//! cell: a soma with a bushy **dendrite** tree (local, receiving) and a
//! branching **axon** arbor (projecting, terminating near the neuron's real
//! synaptic targets). The whole thing is a list of line SEGMENTS, generated on
//! the CPU at `initialize()` time and uploaded once to a GPU storage buffer.
//! The morphology renderer (render_morphology.wgsl) draws each segment as a
//! camera-facing tapered tube. When a neuron fires, its actual synaptic
//! connections (axon segments) light up instantly and fade with the same
//! `exp(-tick_diff/glow_tau)` curve as the far-glow neuron dot — keyed off the
//! segment's source (`neuron_id`) for downstream lighting and its `target_id`
//! for upstream lighting.
//!
//! ALL randomness is drawn from the locked BV22 hash (`mix_key`/`hash32`) so the
//! morphology is bit-reproducible for a given seed (BV16 determinism).
//!
//! `MorphSegment` field order + size (48 bytes) MUST match the WGSL struct in
//! render_morphology.wgsl verbatim (#1 corruption source — see the doc on the
//! struct). Host-testable; no GPU dependency.

use crate::connectivity::hash::{hash32, mix_key};
use crate::connectivity::{self};
use crate::connectivity::spatial::SpatialGrid;

// ─── Salts (decorrelate the different morphology hash uses) ───────────────────
// Distinct from connectivity::salt values (those go up to 4); pick a high,
// disjoint range so a morphology draw never collides with a target/weight draw.
mod salt {
    pub const DENDRITE_DIR: u32 = 0x00A0_0001; // primary dendrite direction
    pub const DENDRITE_CURL: u32 = 0x00A0_0002; // per-segment curl jitter
    pub const DENDRITE_COUNT: u32 = 0x00A0_0003; // how many primary dendrites
    pub const AXON_BOW: u32 = 0x00A0_0004; // axon perpendicular arc seed
}

/// Per-neuron morphology tuning parameters (world units; tuned to the ~0.15
/// inter-neuron spacing at N=1200 so neighbouring trees nearly touch but do not
/// fuse into a hairball).
pub mod params {
    /// Soma-end dendrite/axon radius (world units).
    pub const R0: f32 = 0.006;
    /// Dendrites: minimum primary count (D = MIN + hash % SPAN).
    pub const DENDRITE_MIN: u32 = 3;
    pub const DENDRITE_SPAN: u32 = 3; // → 3..=5 primary dendrites
    /// Dendrite total reach (soma → tip), randomized per dendrite in this band.
    pub const DENDRITE_REACH_LO: f32 = 0.04;
    pub const DENDRITE_REACH_HI: f32 = 0.07;
    /// Axon stops short of the target so boutons cluster near the target's
    /// dendrites rather than inside its soma.
    pub const AXON_STOP_FRACTION: f32 = 0.85;
    /// Axon trunk radius at the soma (fraction of R0).
    pub const AXON_R0_FRACTION: f32 = 0.7;
}

/// One morphology line segment. 48 bytes, std430, 16-aligned.
///
/// Field order + size MUST match the `MorphSegment` struct in
/// render_morphology.wgsl verbatim:
/// ```text
///   a: vec3<f32>,  radius_a: f32   // 16
///   b: vec3<f32>,  radius_b: f32   // 16
///   neuron_id: u32, path_len: f32, kind: u32, target_id: u32  // 16
/// ```
/// `kind`: 0 = dendrite, 1 = axon. `neuron_id` = the segment's SOURCE neuron
/// (drives downstream "next" lighting). `target_id` = the axon segment's
/// destination neuron (drives upstream "past" lighting); for dendrites it is set
/// to `neuron_id` (self) and is unused. `path_len` = cumulative path length FROM
/// THE SOMA to endpoint `a` (retained for the renderer; no longer drives timing).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MorphSegment {
    pub a: [f32; 3],
    pub radius_a: f32,
    pub b: [f32; 3],
    pub radius_b: f32,
    pub neuron_id: u32,
    pub path_len: f32,
    pub kind: u32,
    pub target_id: u32,
}

/// Generated morphology: the flat segment list plus the total count. (Per-neuron
/// ranges are implicit in `neuron_id`; the renderer keys off that.)
pub struct Morphology {
    pub segments: Vec<MorphSegment>,
    /// Upper bound used for the allocation cap; segments past this were dropped
    /// (logged — "no silent caps").
    pub dropped: usize,
}

/// Worst-case dendrite segments per neuron: up to 5 primaries × (1 stem + 2
/// children) = 15.
pub const DENDRITE_MAX: usize = 15;

/// Axon segments emitted per branch (the curved poly-line resolution). MUST stay
/// in sync with `SEGS` inside `generate()`.
pub const AXON_SEGS_PER_BRANCH: usize = 6;

/// Worst-case segments per neuron for a given fan-out `k`, used to size the GPU
/// buffer cap. Now that we draw ALL K axon branches (not a 5-branch subset), the
/// per-neuron cap scales with K: dendrites (≤ DENDRITE_MAX) + k branches ×
/// AXON_SEGS_PER_BRANCH, plus a little slack. No silent caps — the generator
/// logs if it is ever hit.
#[inline]
pub fn max_segs_per_neuron(k: usize) -> usize {
    DENDRITE_MAX + k * AXON_SEGS_PER_BRANCH + 4
}

/// Decode a hash value into a float in [0,1).
#[inline]
fn unit(h: u32) -> f32 {
    (h as f32) / (u32::MAX as f32 + 1.0)
}

/// Roughly-uniform direction on the sphere from two hash draws.
#[inline]
fn dir_from_hashes(h0: u32, h1: u32) -> [f32; 3] {
    use std::f32::consts::TAU;
    let cos_theta = unit(h0) * 2.0 - 1.0;
    let phi = unit(h1) * TAU;
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]
}

#[inline]
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
#[inline]
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = len(a).max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}
#[inline]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A unit vector perpendicular to `dir`, seeded so the axon arc is reproducible.
fn perp(dir: [f32; 3], seed: u32) -> [f32; 3] {
    let r = dir_from_hashes(hash32(seed ^ 0x1111_1111), hash32(seed ^ 0x2222_2222));
    let d = norm(dir);
    // Remove component along dir → perpendicular.
    let dot = r[0] * d[0] + r[1] * d[1] + r[2] * d[2];
    let mut p = sub(r, scale(d, dot));
    if len(p) < 1e-5 {
        p = cross(d, [0.0, 1.0, 0.0]);
        if len(p) < 1e-5 {
            p = cross(d, [1.0, 0.0, 0.0]);
        }
    }
    norm(p)
}

/// Generate the full morphology for all `n` neurons. Deterministic in
/// `(seed_lo, positions, grid, k)`. Caps the total at `n * max_segs_per_neuron(k)`
/// (never hit in practice; logged if it is — no silent truncation).
/// `curve_lift` scales the axon arc height (bow) — wired live from the
/// `connection_curve_lift` setting so the morphology can be regenerated with a
/// different curl. 0 → straight axons, larger → more pronounced arcs.
pub fn generate(
    positions: &[[f32; 3]],
    grid: &SpatialGrid,
    k: usize,
    seed_lo: u32,
    curve_lift: f32,
) -> Morphology {
    let n = positions.len();
    let cap = n * max_segs_per_neuron(k);
    let mut segments: Vec<MorphSegment> = Vec::with_capacity(cap.min(n * (DENDRITE_MAX + k * 4)));
    let mut dropped = 0usize;

    // Precompute each neuron's grid cell once (O(N)) so the axon-arbor loop below
    // can use the hot-path `target_with_cell` entry. The uncached
    // `connectivity::target` re-derives the cell with an O(N) `cell_of_index`
    // scan per call, which made morphology generation O(N²·K) and dominated
    // network-rebuild time at high N. The CPU/GPU paths already cache this map.
    let cell_of_neuron = grid.cell_of_neuron_map();

    // Local helper: push a segment unless the cap is hit (count drops instead).
    let push = |segments: &mut Vec<MorphSegment>, seg: MorphSegment, dropped: &mut usize| {
        if segments.len() < cap {
            segments.push(seg);
        } else {
            *dropped += 1;
        }
    };

    for i in 0..n {
        let soma = positions[i];
        let id = i as u32;

        // ── Dendrites (kind 0): bushy local tree, decorative. ────────────────
        let dcount = params::DENDRITE_MIN
            + (mix_key(seed_lo, id, 0, salt::DENDRITE_COUNT) % params::DENDRITE_SPAN);
        for d in 0..dcount {
            // Primary direction (roughly uniform).
            let dir = dir_from_hashes(
                mix_key(seed_lo, id, d, salt::DENDRITE_DIR),
                mix_key(seed_lo, id, d.wrapping_add(64), salt::DENDRITE_DIR),
            );
            let reach = params::DENDRITE_REACH_LO
                + unit(mix_key(seed_lo, id, d, salt::DENDRITE_CURL))
                    * (params::DENDRITE_REACH_HI - params::DENDRITE_REACH_LO);

            // Segment 1: soma → mid (half the reach), then bifurcate into 2.
            let half = reach * 0.5;
            // A small per-segment curl so dendrites aren't dead straight.
            let curl1 = perp(dir, mix_key(seed_lo, id, d, salt::DENDRITE_CURL));
            let mid = add(
                add(soma, scale(dir, half)),
                scale(curl1, half * 0.25),
            );
            // Soma path_len starts at 0; accumulate along the branch.
            let r_soma = params::R0;
            let r_mid = params::R0 * 0.6;
            push(
                &mut segments,
                MorphSegment {
                    a: soma,
                    radius_a: r_soma,
                    b: mid,
                    radius_b: r_mid,
                    neuron_id: id,
                    path_len: 0.0,
                    kind: 0,
                    target_id: id, // dendrite: self (unused)
                },
                &mut dropped,
            );
            let len_mid = len(sub(mid, soma));

            // Two child branches from the tip of segment 1 (the bifurcation).
            for c in 0..2u32 {
                let salt_c = salt::DENDRITE_CURL ^ (c.wrapping_mul(0x9e37_79b1));
                let spread = perp(dir, mix_key(seed_lo, id, d.wrapping_add(c * 17), salt_c));
                let sign = if c == 0 { 1.0 } else { -1.0 };
                let child_dir = norm(add(scale(dir, 1.0), scale(spread, 0.7 * sign)));
                let tip = add(mid, scale(child_dir, half));
                let r_tip = params::R0 * 0.3; // ~0.3·r0 at the tips
                push(
                    &mut segments,
                    MorphSegment {
                        a: mid,
                        radius_a: r_mid,
                        b: tip,
                        radius_b: r_tip,
                        neuron_id: id,
                        path_len: len_mid,
                        kind: 0,
                        target_id: id, // dendrite: self (unused)
                    },
                    &mut dropped,
                );
            }
        }

        // ── Axon arbor (kind 1): projecting branches toward real targets. ────
        // Draw ALL K outgoing connections (one axon arbor per synaptic target),
        // not a 5-branch subset, so the lit connections match real synapses.
        let branches = k;
        for j in 0..branches as u32 {
            let src_type = 0u8; // E/I only changes target dz bias; for arbor shape
                                // a fixed type keeps the look stable. Targets still
                                // come from the real spatial rule.
            let src_cell = grid.unpack(cell_of_neuron[i]);
            let tgt_id = connectivity::target_with_cell(id, j, grid, k, seed_lo, src_type, src_cell);
            let t = positions[tgt_id as usize];
            if tgt_id == id {
                continue; // degenerate self-target: skip (no segment)
            }
            let full = sub(t, soma);
            let dist = len(full);
            if dist < 1e-6 {
                continue;
            }
            let dir = norm(full);
            // Stop short of the target so boutons sit near its dendrites.
            let end = add(soma, scale(full, params::AXON_STOP_FRACTION));
            // Seeded perpendicular bow so the axon arcs (connection_curve_lift-style).
            let bow_dir = perp(dir, mix_key(seed_lo, id, j, salt::AXON_BOW));
            // Morphology controls: arc height scales with curve_lift (live
            // setting). The bow is amplified (×2.5) and the arc resolved with more
            // segments so the curve is clearly visible at the default lift 0.15
            // and straightens fully at lift 0.
            const BOW_GAIN: f32 = 2.5;
            let bow = dist * curve_lift.max(0.0) * BOW_GAIN;

            // Curved poly-line of SEGS segments through bowed control points.
            // Parametric points along the straight soma→end line, lifted by a
            // sin bow so the midpoint bulges out and the ends meet soma/target.
            // SEGS must match morphology::AXON_SEGS_PER_BRANCH for the cap.
            let r_soma = params::R0 * params::AXON_R0_FRACTION;
            const SEGS: usize = AXON_SEGS_PER_BRANCH;
            let mut prev = soma;
            let mut prev_path = 0.0f32;
            let mut prev_r = r_soma;
            for s in 1..=SEGS {
                let tt = s as f32 / SEGS as f32;
                let base = add(soma, scale(sub(end, soma), tt));
                let lift = (std::f32::consts::PI * tt).sin() * bow;
                let pnt = add(base, scale(bow_dir, lift));
                // Taper toward the terminal bouton (tiny at the tip).
                let r_here = if s == SEGS {
                    params::R0 * 0.2 // terminal bouton: small
                } else {
                    r_soma * (1.0 - 0.6 * tt)
                };
                push(
                    &mut segments,
                    MorphSegment {
                        a: prev,
                        radius_a: prev_r,
                        b: pnt,
                        radius_b: r_here,
                        neuron_id: id,
                        path_len: prev_path,
                        kind: 1,
                        target_id: tgt_id, // axon: destination neuron (drives "past" lighting)
                    },
                    &mut dropped,
                );
                prev_path += len(sub(pnt, prev));
                prev = pnt;
                prev_r = r_here;
            }
        }
    }

    if dropped > 0 {
        eprintln!(
            "[morphology] segment cap {cap} hit: {dropped} segments dropped (raise max_segs_per_neuron)"
        );
    }

    Morphology { segments, dropped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_grid() -> (Vec<[f32; 3]>, SpatialGrid) {
        // A little cube of neurons so connectivity::target has real cells.
        let mut pos = Vec::new();
        let side = 6;
        for z in 0..side {
            for y in 0..side {
                for x in 0..side {
                    pos.push([
                        x as f32 * 0.15,
                        y as f32 * 0.15,
                        z as f32 * 0.15,
                    ]);
                }
            }
        }
        let g = SpatialGrid::build(&pos, side as u32);
        (pos, g)
    }

    #[test]
    fn segment_layout_is_48_bytes() {
        assert_eq!(std::mem::size_of::<MorphSegment>(), 48);
        assert_eq!(std::mem::size_of::<MorphSegment>() % 16, 0);
    }

    #[test]
    fn generates_segments_for_every_neuron() {
        let (pos, g) = small_grid();
        let m = generate(&pos, &g, 16, 1234, 0.15);
        // Every neuron contributes at least one dendrite + (usually) axon segment.
        assert!(!m.segments.is_empty());
        assert_eq!(m.dropped, 0, "should not hit the cap at this size");
        // All segment neuron_ids and target_ids are in range.
        for s in &m.segments {
            assert!((s.neuron_id as usize) < pos.len());
            assert!((s.target_id as usize) < pos.len(), "target_id out of range");
            assert!(s.kind == 0 || s.kind == 1);
            assert!(s.radius_a > 0.0 && s.radius_b > 0.0);
            if s.kind == 0 {
                // Dendrites carry self as target (unused).
                assert_eq!(s.target_id, s.neuron_id, "dendrite target_id must be self");
            }
        }
        // Both kinds present.
        assert!(m.segments.iter().any(|s| s.kind == 0), "no dendrites");
        assert!(m.segments.iter().any(|s| s.kind == 1), "no axons");
        // Axon segments carry a real (non-self) target neuron.
        assert!(
            m.segments
                .iter()
                .any(|s| s.kind == 1 && s.target_id != s.neuron_id),
            "axon segments should point at distinct target neurons"
        );
    }

    #[test]
    fn draws_all_k_axon_branches() {
        // With all-K coverage, the morphology emits one axon arbor per real
        // synaptic target (j in 0..k) instead of the old min(5, k) subset. Verify
        // the per-neuron distinct axon targets match exactly the distinct,
        // non-self targets connectivity::target resolves for j in 0..k — i.e. we
        // draw ALL K, not a 5-branch cap.
        let (pos, g) = small_grid();
        let k = 8usize;
        let seed = 4242u32;
        let m = generate(&pos, &g, k, seed, 0.15);
        let probe = (pos.len() / 2) as u32;

        // Expected distinct, non-self targets from the connectivity rule.
        let mut expected: Vec<u32> = (0..k as u32)
            .map(|j| connectivity::target(probe, j, &g, k, seed, 0u8))
            .filter(|&t| t != probe)
            .collect();
        expected.sort_unstable();
        expected.dedup();

        let mut got: Vec<u32> = m
            .segments
            .iter()
            .filter(|s| s.kind == 1 && s.neuron_id == probe)
            .map(|s| s.target_id)
            .collect();
        got.sort_unstable();
        got.dedup();

        assert_eq!(got, expected, "axon targets must cover all K connectivity targets");
        // And it must exceed the old 5-branch cap somewhere in the network.
        let max_axons_for_a_neuron = (0..pos.len() as u32)
            .map(|nid| {
                m.segments
                    .iter()
                    .filter(|s| s.kind == 1 && s.neuron_id == nid)
                    .map(|s| s.target_id)
                    .collect::<std::collections::HashSet<_>>()
                    .len()
            })
            .max()
            .unwrap_or(0);
        assert!(
            max_axons_for_a_neuron > 5,
            "all-K coverage should exceed the old 5-branch cap (got max {max_axons_for_a_neuron})"
        );
        assert_eq!(m.dropped, 0, "all-K cap should not drop at this size");
    }

    #[test]
    fn deterministic_for_same_seed() {
        let (pos, g) = small_grid();
        let a = generate(&pos, &g, 16, 99, 0.15);
        let b = generate(&pos, &g, 16, 99, 0.15);
        assert_eq!(a.segments.len(), b.segments.len());
        for (x, y) in a.segments.iter().zip(b.segments.iter()) {
            assert_eq!(x.a, y.a);
            assert_eq!(x.b, y.b);
            assert_eq!(x.path_len, y.path_len);
        }
    }

    #[test]
    fn seed_changes_morphology() {
        let (pos, g) = small_grid();
        let a = generate(&pos, &g, 16, 1, 0.15);
        let b = generate(&pos, &g, 16, 2, 0.15);
        let differ = a
            .segments
            .iter()
            .zip(b.segments.iter())
            .filter(|(x, y)| x.a != y.a || x.b != y.b)
            .count();
        assert!(differ > 0, "seed had no effect on morphology");
    }

    #[test]
    fn soma_segments_start_at_path_zero() {
        let (pos, g) = small_grid();
        let m = generate(&pos, &g, 16, 7, 0.15);
        // The first segment of each branch (touching the soma) has path_len 0.
        let zero_count = m.segments.iter().filter(|s| s.path_len == 0.0).count();
        assert!(zero_count >= pos.len(), "expected ≥1 root segment per neuron");
    }
}
