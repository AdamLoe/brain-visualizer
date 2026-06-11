//! Procedural / implicit connectivity (BV6).
//!
//! There is no stored edge list. `target(i, j, ...)` and `weight(i, j, ...)`
//! are **pure integer functions** of the neuron ids and type, so the CPU
//! (Rust) and GPU (WGSL) backends derive bit-identical synapse lists from the
//! same seed. All arithmetic is on integer cell coordinates and the BV22 hash
//! (`hash.rs`); there is **no float distance comparison** anywhere on this
//! path.
//!
//! Deviation from the phase-1 doc signature: the doc shows
//! `target(i, j, grid, k)`. We add an explicit `seed` parameter (and a
//! `source_type` to `weight` per the doc) because real determinism is keyed on
//! `SimConfig.seed`. The seed is the `seed_lo` argument of `mix_key`. This is a
//! deliberate, documented extension, not a structural change.

pub mod hash;
pub mod spatial;

pub use hash::{hash32, mix_key};
pub use spatial::SpatialGrid;

/// Fixed-point current scale factor S = 2^12 (BV19). Weights are returned
/// already scaled by this.
pub const FIXED_POINT_SCALE: i32 = 4096;

/// Salt constants keep the different hash *uses* decorrelated. Distinct odd
/// values so target-cell, in-cell pick, weight, and bias draws never collide.
mod salt {
    pub const CELL_OFFSET: u32 = 0x0000_0001;
    pub const IN_CELL_PICK: u32 = 0x0000_0002;
    pub const WEIGHT: u32 = 0x0000_0003;
    pub const ANTERIOR_BIAS: u32 = 0x0000_0004;
    /// Local-vs-long-range coin flip (heavy-tailed reach).
    pub const REACH_COIN: u32 = 0x0000_0005;
    /// Long-range cell offset draw (heavy-tailed reach).
    pub const REACH_OFFSET: u32 = 0x0000_0006;
}

/// Denominator for the integer long-range fraction (`long_range_frac` is the
/// numerator over this). A fixed constant so the local-vs-long-range coin flip
/// is a pure integer compare — no float on the determinism path. Mirrored as
/// `REACH_FRAC_DEN` in scatter.wgsl.
pub const REACH_FRAC_DEN: u32 = 256;

/// Heavy-tailed reach knobs threaded into [`target`]/[`target_with_cell`].
/// All integer so the Rust↔WGSL `target` path stays bit-identical and float-free.
///
/// - `long_range_frac`: numerator over [`REACH_FRAC_DEN`] (so `0` = always local,
///   `REACH_FRAC_DEN` = always long-range). A per-synapse hash coin `< frac`
///   selects the long-range branch.
/// - `max_reach`: integer cell radius for the long-range offset, `>= 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReachParams {
    pub long_range_frac: u32,
    pub max_reach: u32,
}

impl ReachParams {
    /// Default-off: no long-range synapses. Output is bit-identical to the
    /// pre-heavy-tail network (the long-range branch never executes).
    pub const LOCAL_ONLY: ReachParams = ReachParams {
        long_range_frac: 0,
        max_reach: 1,
    };
}

/// Half-width (in cells) of the local connectivity neighbourhood. A synapse
/// targets a cell within +/- `LOCAL_D` of the source cell on each axis →
/// `(2D+1)^3` candidate cells. Keeps cortex locally connected (architecture
/// §3). Integer only.
pub const LOCAL_D: i32 = 1;

/// Number of distinct integer offsets per axis in the local neighbourhood.
const AXIS_SPAN: u32 = (2 * LOCAL_D + 1) as u32;

/// Fraction (out of 16) of *excitatory* synapses that get a mild forward
/// (anterior, +Z by convention) bias so input-region activity propagates
/// posterior→anterior (BV17, architecture §3). Inhibition is left unbiased.
const ANTERIOR_BIAS_NUM: u32 = 5; // ~31% of excitatory synapses
const ANTERIOR_BIAS_DEN: u32 = 16;

/// E/I flag lives in bit 0 of the 7-bit neuron type byte (BV21). 0 = excitatory.
#[inline]
pub fn is_excitatory(source_type: u8) -> bool {
    source_type & 0x01 == 0
}

/// Decode a hashed value into a signed cell offset in `[-LOCAL_D, LOCAL_D]`.
#[inline]
fn offset_component(h: u32) -> i32 {
    (h % AXIS_SPAN) as i32 - LOCAL_D
}

/// Decode a hashed value into a signed cell offset in `[-max_reach, +max_reach]`
/// for the long-range branch. Mirrors `long_offset_component` in scatter.wgsl.
#[inline]
fn long_offset_component(bits: u32, max_reach: u32) -> i32 {
    (bits % (2 * max_reach + 1)) as i32 - max_reach as i32
}

/// Returns the target neuron index for synapse `j` of neuron `i`.
///
/// Pure function of `(i, j, grid, k, seed, source_type)` — no float math.
/// Algorithm (architecture §3):
/// 1. Look up the integer cell of neuron `i`.
/// 2. BV22-hash `(seed, i, j)` → a candidate `(dx, dy, dz)` offset within
///    `LOCAL_D` cells. For excitatory neurons a small deterministic fraction of
///    synapses bias `dz` forward (anterior) so activity flows posterior→anterior.
/// 3. Clamp the candidate cell to the grid; if empty, walk outward to the
///    nearest occupied cell (deterministic spiral over the neighbourhood).
/// 4. Hash again to pick a neuron within the chosen cell.
///
/// `k` participates only as part of the per-synapse key space; the caller
/// iterates `j in 0..k`.
pub fn target(
    i: u32,
    j: u32,
    grid: &SpatialGrid,
    k: usize,
    seed: u32,
    source_type: u8,
    reach: ReachParams,
) -> u32 {
    debug_assert!((j as usize) < k.max(1));
    let src_cell = grid.unpack(grid.cell_of_index(i));
    target_with_cell(i, j, grid, k, seed, source_type, src_cell, reach)
}

/// Identical to [`target`] but takes the source neuron's already-known integer
/// cell coordinate, avoiding the O(N) `cell_of_index` scan. This is the hot-path
/// entry the CPU backend uses (it precomputes `SpatialGrid::cell_of_neuron_map`
/// once); the GPU scatter does the same by reading its `cell_of_neuron` buffer.
/// `target` delegates here so both produce bit-identical results.
#[inline]
pub fn target_with_cell(
    i: u32,
    j: u32,
    grid: &SpatialGrid,
    k: usize,
    seed: u32,
    source_type: u8,
    src_cell: [u32; 3],
    reach: ReachParams,
) -> u32 {
    debug_assert!((j as usize) < k.max(1));

    let h = mix_key(seed, i, j, salt::CELL_OFFSET);

    // Three independent offset components from one 32-bit hash (10 bits each).
    let mut dx = offset_component(h & 0x3ff);
    let mut dy = offset_component((h >> 10) & 0x3ff);
    let mut dz = offset_component((h >> 20) & 0x3ff);

    // Mild anterior (+Z) feed-forward bias for a fraction of excitatory
    // synapses (BV17). Inhibition stays local & unbiased (dz untouched).
    if is_excitatory(source_type) {
        let bias_draw = mix_key(seed, i, j, salt::ANTERIOR_BIAS) % ANTERIOR_BIAS_DEN;
        if bias_draw < ANTERIOR_BIAS_NUM {
            dz = LOCAL_D; // push this synapse forward
        }
    }

    // Heavy-tailed reach: a deterministic per-synapse coin flips a tunable
    // fraction of synapses long-range. When long, OVERWRITE the local
    // (already biased) offset with a wider integer draw bounded by max_reach.
    // The coin compare `coin < long_range_frac` is integer-only; at
    // `long_range_frac == 0` it is always false, so this branch never runs and
    // output is bit-identical to the local-only network (REACH_FRAC_DEN units).
    let coin = mix_key(seed, i, j, salt::REACH_COIN) % REACH_FRAC_DEN;
    if coin < reach.long_range_frac {
        let h2 = mix_key(seed, i, j, salt::REACH_OFFSET);
        dx = long_offset_component(h2 & 0x3ff, reach.max_reach);
        dy = long_offset_component((h2 >> 10) & 0x3ff, reach.max_reach);
        dz = long_offset_component((h2 >> 20) & 0x3ff, reach.max_reach);
    }

    let target_cell = clamp_cell(src_cell, [dx, dy, dz], grid.dim);
    let cell_id = nearest_occupied(grid, target_cell);

    // Pick a neuron within that cell.
    let occupants = grid.neurons_in_cell(cell_id);
    if occupants.is_empty() {
        // Fully empty grid (degenerate) — self-connect as a safe fallback.
        return i;
    }
    let pick = mix_key(seed, i, j, salt::IN_CELL_PICK) % occupants.len() as u32;
    occupants[pick as usize]
}

/// Returns the fixed-point synaptic weight (already scaled by S = 4096, BV19).
///
/// - Excitatory (type bit 0 = 0): positive, ~1000..4096 (≈0.25..1.0 mV × S).
/// - Inhibitory (type bit 0 = 1): negative, ~ -2000..-1000.
///
/// Deterministic from `(i, j, source_type)` via the BV22 hash.
pub fn weight(i: u32, j: u32, source_type: u8) -> i32 {
    // Seed-independent on purpose: weight is a property of the synapse identity,
    // not the network instance. (seed_lo = 0.)
    let h = mix_key(0, i, j, salt::WEIGHT);
    if is_excitatory(source_type) {
        // Range [1000, 4095] inclusive-ish.
        let span = (FIXED_POINT_SCALE - 1000) as u32; // 3096
        1000 + (h % span) as i32
    } else {
        // Range [-2000, -1000].
        let span = 1000u32; // -2000..-1000
        -2000 + (h % span) as i32
    }
}

/// Clamp `base + delta` cell coordinate to `[0, dim-1]` on each axis.
#[inline]
fn clamp_cell(base: [u32; 3], delta: [i32; 3], dim: u32) -> [u32; 3] {
    let mut out = [0u32; 3];
    for a in 0..3 {
        let v = base[a] as i32 + delta[a];
        out[a] = v.clamp(0, dim as i32 - 1) as u32;
    }
    out
}

/// If `cell` is empty, deterministically walk the surrounding neighbourhood
/// (increasing Chebyshev radius) until an occupied cell is found. Integer-only.
/// Returns the packed cell id.
fn nearest_occupied(grid: &SpatialGrid, cell: [u32; 3]) -> u32 {
    let id = grid.pack(cell);
    if !grid.neurons_in_cell(id).is_empty() {
        return id;
    }
    let dim = grid.dim as i32;
    for r in 1..dim {
        for dz in -r..=r {
            for dy in -r..=r {
                for dx in -r..=r {
                    // Only the shell at Chebyshev radius r.
                    if dx.abs() != r && dy.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let c = clamp_cell(cell, [dx, dy, dz], grid.dim);
                    let cid = grid.pack(c);
                    if !grid.neurons_in_cell(cid).is_empty() {
                        return cid;
                    }
                }
            }
        }
    }
    id
}

impl SpatialGrid {
    /// Packed cell id of neuron `index`, found via its membership in the CSR
    /// layout. O(occupancy) worst case but only used off the hot path / in
    /// tests; the GPU path stores per-neuron cell ids in a buffer (phase 2).
    ///
    /// For phase-1 host logic we reconstruct from `cell_neurons`. To keep this
    /// O(1)-amortized for `target()`, we cache nothing here but rely on the
    /// caller passing the source cell when available. The straightforward host
    /// implementation scans — acceptable for tests and startup logging.
    pub fn cell_of_index(&self, index: u32) -> u32 {
        // Binary search over CSR offsets: find the cell whose range contains
        // the position of `index` in cell_neurons. But cell_neurons is keyed
        // by slot, not neuron id, so we must locate the slot first. For phase
        // 1 we store an inverse map lazily; absent that, linear find.
        for cell in 0..self.cell_count() {
            if self.neurons_in_cell(cell).contains(&index) {
                return cell;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_from_cube(side: usize) -> (Vec<[f32; 3]>, SpatialGrid) {
        let mut pos = Vec::new();
        for z in 0..side {
            for y in 0..side {
                for x in 0..side {
                    pos.push([x as f32, y as f32, z as f32]);
                }
            }
        }
        let g = SpatialGrid::build(&pos, side as u32);
        (pos, g)
    }

    #[test]
    fn target_is_deterministic() {
        let (pos, g) = grid_from_cube(4);
        let n = pos.len() as u32;
        for i in 0..n {
            for j in 0..8u32 {
                let a = target(i, j, &g, 8, 1234, 0, ReachParams::LOCAL_ONLY);
                let b = target(i, j, &g, 8, 1234, 0, ReachParams::LOCAL_ONLY);
                assert_eq!(a, b, "target({i},{j}) not deterministic");
            }
        }
    }

    #[test]
    fn target_in_range() {
        let (pos, g) = grid_from_cube(4);
        let n = pos.len() as u32;
        for i in 0..n {
            for j in 0..16u32 {
                let t = target(i, j, &g, 16, 7, (i % 2) as u8, ReachParams::LOCAL_ONLY);
                assert!(t < n, "target {t} out of range (n={n})");
            }
        }
    }

    #[test]
    fn seed_changes_targets() {
        let (pos, g) = grid_from_cube(5);
        let mut differ = 0;
        for i in 0..pos.len() as u32 {
            for j in 0..8u32 {
                if target(i, j, &g, 8, 1, 0, ReachParams::LOCAL_ONLY)
                    != target(i, j, &g, 8, 2, 0, ReachParams::LOCAL_ONLY)
                {
                    differ += 1;
                }
            }
        }
        assert!(differ > 0, "seed had no effect on targets");
    }

    #[test]
    fn weight_signs_by_type() {
        for i in 0..100u32 {
            for j in 0..16u32 {
                let we = weight(i, j, 0); // excitatory
                let wi = weight(i, j, 1); // inhibitory
                assert!(we > 0, "excitatory weight {we} not positive");
                assert!((1000..=FIXED_POINT_SCALE).contains(&we));
                assert!(wi < 0, "inhibitory weight {wi} not negative");
                assert!((-2000..=-1000).contains(&wi));
            }
        }
    }

    #[test]
    fn weight_deterministic() {
        assert_eq!(weight(5, 3, 0), weight(5, 3, 0));
        assert_eq!(weight(99, 1, 1), weight(99, 1, 1));
    }

    #[test]
    fn anterior_bias_present_for_excitatory() {
        // Over many excitatory synapses, a non-trivial fraction should land at
        // the forward (+Z) edge of the neighbourhood.
        let (pos, g) = grid_from_cube(6);
        let mut forward = 0usize;
        let mut total = 0usize;
        // Pick a source near the posterior face so +Z is in-range.
        for i in 0..pos.len() as u32 {
            let src = g.unpack(g.cell_of_index(i));
            if src[2] >= g.dim - 1 {
                continue; // already at front, bias clamps — skip for clarity
            }
            for j in 0..32u32 {
                let t = target(i, j, &g, 32, 99, 0, ReachParams::LOCAL_ONLY);
                let tc = g.unpack(g.cell_of_index(t));
                if tc[2] > src[2] {
                    forward += 1;
                }
                total += 1;
            }
        }
        let frac = forward as f32 / total as f32;
        assert!(frac > 0.10, "anterior bias too weak: {frac:.3}");
    }
}
