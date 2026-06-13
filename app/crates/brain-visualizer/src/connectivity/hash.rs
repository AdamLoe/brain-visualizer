//! BV22 — WGSL-friendly 32-bit integer hash.
//!
//! These two functions are the determinism backbone of the whole project:
//! the GPU (WGSL) and Rust host paths must produce **bit-identical**
//! synapse targets and weights from the same neuron id. The constants here are
//! locked by BV22 and must match `src/sim/gpu/shaders/hash.wgsl` verbatim.
//!
//! All multiplies wrap modulo 2^32 (WGSL `u32` multiply wraps; Rust uses
//! `wrapping_mul`). There is no `u64` anywhere — WGSL has no native `u64`.

/// BV22 32-bit avalanche hash (lowbias32 variant). Pure, stateless.
#[inline]
pub fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Mix a `(seed_lo, neuron_id, synapse_j, salt)` tuple into a single `u32`
/// key and hash it. Distinct odd multiplier constants decorrelate the four
/// input axes before the avalanche. Matches `mix_key` in `hash.wgsl`.
#[inline]
pub fn mix_key(seed_lo: u32, neuron_id: u32, synapse_j: u32, salt: u32) -> u32 {
    hash32(
        seed_lo
            ^ neuron_id.wrapping_mul(0x9e37_79b1)
            ^ synapse_j.wrapping_mul(0x85eb_ca6b)
            ^ salt.wrapping_mul(0xc2b2_ae35),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vectors for `hash32`. Values are the verified output of the
    /// BV22 constants for these inputs; the native WGSL determinism test
    /// (`tests/wgsl_hash_determinism.rs`) recomputes them on-GPU and asserts
    /// equality. If either side drifts, both this table and the WGSL must be
    /// re-derived together — never edit one in isolation.
    #[test]
    fn hash32_golden() {
        const CASES: &[(u32, u32)] = &[
            (0x0000_0000, 0x0000_0000),
            (0x0000_0001, 0x6889_90c0),
            (0x0000_0002, 0xd113_2181),
            (0xffff_ffff, 0x6768_824a),
            (0xdead_beef, 0xe628_c683),
            (0x1234_5678, 0xf5e7_1c96),
        ];
        for &(input, expected) in CASES {
            assert_eq!(
                hash32(input),
                expected,
                "hash32(0x{input:08x}) drifted; expected 0x{expected:08x}"
            );
        }
    }

    #[test]
    fn mix_key_golden() {
        // (seed_lo, neuron_id, synapse_j, salt) -> expected
        const CASES: &[(u32, u32, u32, u32, u32)] = &[
            (0, 0, 0, 0, 0x0000_0000),
            (1, 0, 0, 0, 0x6889_90c0),
            (0, 1, 0, 0, 0x6d52_3710),
            (0, 0, 1, 0, 0x1efe_e872),
            (0, 0, 0, 1, 0x61d7_7ce2),
            (42, 1000, 7, 3, 0x3c7b_bd27),
            (0xdead_beef, 123, 45, 6, 0xf69b_8aca),
        ];
        for &(seed, id, j, salt, expected) in CASES {
            assert_eq!(
                mix_key(seed, id, j, salt),
                expected,
                "mix_key({seed},{id},{j},{salt}) drifted; expected 0x{expected:08x}"
            );
        }
    }

    #[test]
    fn hash32_deterministic() {
        for x in [0u32, 1, 7, 99, 0xabcd_1234] {
            assert_eq!(hash32(x), hash32(x));
        }
    }
}
