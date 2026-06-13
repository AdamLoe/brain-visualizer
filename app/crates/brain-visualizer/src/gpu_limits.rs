//! Adapter limits → derived project caps (architecture §10, phase-1 doc).
//!
//! Later phases consume these caps instead of hard-coding desktop assumptions.
//! The **derivation math is host-testable**: `GpuCaps::derive` takes a plain
//! `LimitsInput` struct so it can be exercised without a real adapter. On wasm
//! the caller fills `LimitsInput` from `adapter.limits()` and logs the result;
//! that logging is the only wasm-specific part.

/// Raw adapter limits we care about (mirrors the relevant `wgpu::Limits` /
/// feature flags). Plain data so it is trivially constructible in tests.
#[derive(Debug, Clone, Copy)]
pub struct LimitsInput {
    pub max_storage_buffer_binding_size: u64,
    pub max_buffer_size: u64,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_compute_invocations_per_workgroup: u32,
    pub max_compute_workgroup_size_x: u32,
    pub timestamp_query: bool,
}

impl LimitsInput {
    /// The conservative WebGPU defaults / llvmpipe values from §9.1 — a sane
    /// fallback and a stable test fixture.
    pub fn webgpu_defaults() -> Self {
        Self {
            max_storage_buffer_binding_size: 128 * 1024 * 1024, // 128 MiB
            max_buffer_size: 256 * 1024 * 1024,
            max_compute_workgroups_per_dimension: 65_535,
            max_compute_invocations_per_workgroup: 256,
            max_compute_workgroup_size_x: 256,
            timestamp_query: false,
        }
    }
}

/// Per-neuron SoA footprint in bytes (architecture §2: 24 B logical, but the
/// largest single *field* is 4 B; max-neurons-by-binding is governed by the
/// largest single storage binding, i.e. one 4-byte field).
pub const FIELD_ELEMENT_BYTES: u64 = 4;

/// Derived project caps. All downstream tier/scan/instance logic reads these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuCaps {
    /// Chosen 1-D compute workgroup size (≤ device max, power of two).
    pub workgroup_size: u32,
    /// Max neurons addressable by a single 4-byte storage binding (before the
    /// chunked layout in `buffers.rs` kicks in).
    pub max_neurons_per_binding: u64,
    /// Max threads a single dispatch can launch in one dimension.
    pub max_dispatch_threads_x: u64,
    /// Max items a flat scan/bin pass can cover before chunking is required.
    pub max_scan_items: u64,
    /// Retired render-cap slot kept out of the public settings contract.
    pub max_near_instances: u64,
    /// Whether timestamp queries are available for per-pass GPU timing (§8).
    pub timestamp_available: bool,
}

impl GpuCaps {
    /// Derive caps from adapter limits. Pure; no device required.
    pub fn derive(limits: &LimitsInput) -> Self {
        // Pick the largest power-of-two workgroup ≤ both X size and invocation
        // caps, clamped to a sane 256 default upper bound for occupancy.
        let wg_ceiling = limits
            .max_compute_workgroup_size_x
            .min(limits.max_compute_invocations_per_workgroup)
            .min(256);
        let workgroup_size = prev_pow2(wg_ceiling).max(1);

        let max_neurons_per_binding = limits.max_storage_buffer_binding_size / FIELD_ELEMENT_BYTES;

        // One flat dispatch can launch (workgroups_per_dim × workgroup_size)
        // threads in X. Beyond that, callers must use 2-D dispatch (as the
        // bench does to dodge the 65535 cap).
        let max_dispatch_threads_x =
            limits.max_compute_workgroups_per_dimension as u64 * workgroup_size as u64;

        // A single 1-D scan/bin pass is bounded by what one dispatch can cover.
        let max_scan_items = max_dispatch_threads_x;

        // Near-LOD instance lists are bounded by both buffer size (4 B/instance
        // minimum) and a single dispatch's thread count.
        let max_near_instances =
            (limits.max_buffer_size / FIELD_ELEMENT_BYTES).min(max_dispatch_threads_x);

        Self {
            workgroup_size,
            max_neurons_per_binding,
            max_dispatch_threads_x,
            max_scan_items,
            max_near_instances,
            timestamp_available: limits.timestamp_query,
        }
    }
}

/// Largest power of two ≤ `x` (for `x >= 1`).
#[inline]
fn prev_pow2(x: u32) -> u32 {
    if x == 0 {
        0
    } else {
        1u32 << (31 - x.leading_zeros())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_derive_sane_caps() {
        let caps = GpuCaps::derive(&LimitsInput::webgpu_defaults());
        assert_eq!(caps.workgroup_size, 256);
        // 128 MiB / 4 B = 32M neurons per binding.
        assert_eq!(caps.max_neurons_per_binding, 32 * 1024 * 1024);
        assert_eq!(caps.max_dispatch_threads_x, 65_535 * 256);
        assert!(!caps.timestamp_available);
    }

    #[test]
    fn llvmpipe_limits() {
        // §9.1 llvmpipe figures: 1024 workgroup size, timestamp present.
        let limits = LimitsInput {
            max_storage_buffer_binding_size: 134_217_728,
            max_buffer_size: 2_147_483_647,
            max_compute_workgroups_per_dimension: 65_535,
            max_compute_invocations_per_workgroup: 1024,
            max_compute_workgroup_size_x: 1024,
            timestamp_query: true,
        };
        let caps = GpuCaps::derive(&limits);
        // Clamped to our 256 occupancy ceiling.
        assert_eq!(caps.workgroup_size, 256);
        assert!(caps.timestamp_available);
        assert_eq!(caps.max_neurons_per_binding, 32 * 1024 * 1024);
    }

    #[test]
    fn prev_pow2_cases() {
        assert_eq!(prev_pow2(1), 1);
        assert_eq!(prev_pow2(255), 128);
        assert_eq!(prev_pow2(256), 256);
        assert_eq!(prev_pow2(257), 256);
    }
}
