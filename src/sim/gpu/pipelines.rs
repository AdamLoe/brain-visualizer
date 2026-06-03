//! Compute pipeline creation + shader module loading (architecture §5).
//!
//! Phase 1 embeds the WGSL stub sources via `include_str!` and exposes them so
//! later phases create modules/pipelines without scattering shader strings
//! across the codebase. The BV22 `hash.wgsl` is embedded here too so the
//! determinism test (and phase 2 scatter) can prepend it to shaders that need
//! the hash.

/// BV22 hash WGSL — prepended to any shader that derives synapse targets.
/// MUST match `connectivity::hash` (see that module's tests + the native
/// determinism test).
pub const HASH_WGSL: &str = include_str!("shaders/hash.wgsl");

/// Integrate+threshold pass (phase-2 stub source).
pub const INTEGRATE_WGSL: &str = include_str!("shaders/integrate.wgsl");

/// Scatter pass (phase-2 stub source).
pub const SCATTER_WGSL: &str = include_str!("shaders/scatter.wgsl");

/// Holds compiled compute pipelines. Phase 1 stub: none created yet.
pub struct GpuPipelines {
    // TODO(phase 2): integrate / scatter / stimulate compute pipelines.
}

impl Default for GpuPipelines {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuPipelines {
    pub fn new() -> Self {
        Self {}
    }

    /// Build all sim pipelines from the embedded WGSL. Phase 1: no-op stub.
    pub fn build(&mut self, _device: &wgpu::Device) {
        // TODO(phase 2): create_shader_module(HASH_WGSL + SCATTER_WGSL) etc.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_wgsl_embedded_and_locked() {
        // Sanity that the embedded source carries the locked BV22 constants.
        assert!(HASH_WGSL.contains("0x7feb352du"));
        assert!(HASH_WGSL.contains("0x846ca68bu"));
        assert!(HASH_WGSL.contains("fn mix_key"));
        assert!(HASH_WGSL.contains("0x9e3779b1u"));
    }

    #[test]
    fn pass_stubs_present() {
        assert!(INTEGRATE_WGSL.contains("@compute"));
        assert!(SCATTER_WGSL.contains("@compute"));
    }
}
