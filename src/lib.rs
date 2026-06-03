//! Brain Visualizer — Rust crate (compiles to WASM for the browser; pure logic
//! also compiles & unit-tests on the host).
//!
//! Module layout follows architecture §10.1: sim, connectivity, manifold,
//! buffers, profiler, and gpu_limits are separate; nothing is a god-object.
//!
//! Dual-target: all pure logic builds on the host (`cargo build` / `cargo
//! test`). The wasm-only glue (`#[wasm_bindgen]` entry points, DOM/Worker code)
//! is gated behind `#[cfg(target_arch = "wasm32")]`.

pub mod buffers;
pub mod connectivity;
pub mod gpu_limits;
pub mod manifold;
pub mod profiler;
pub mod sim;

use crate::manifold::{Manifold, ManifoldParams, RegionKind};
use crate::sim::backend::SimConfig;

/// Build the manifold for a config. Pure, host-callable; the wasm entry point
/// and `cargo test` both go through this so there is one code path.
pub fn build_manifold(config: &SimConfig) -> Manifold {
    let params = ManifoldParams::new(config.n, config.seed_lo());
    Manifold::generate(&params)
}

/// Region split summary (Input, Association, Output) — used for startup logging
/// and as a host-testable sanity surface.
pub fn region_split(manifold: &Manifold) -> (usize, usize, usize) {
    let mut input = 0;
    let mut assoc = 0;
    let mut output = 0;
    for r in &manifold.neuron_regions {
        match r {
            RegionKind::Input => input += 1,
            RegionKind::Association => assoc += 1,
            RegionKind::Output => output += 1,
        }
    }
    (input, assoc, output)
}

// ---------------------------------------------------------------------------
// wasm entry points (browser only)
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
mod wasm_entry {
    use super::*;
    use wasm_bindgen::prelude::*;

    /// Install the panic hook so Rust panics surface in the browser console.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        web_sys::console::log_1(&"[brain-visualizer] wasm module loaded".into());
    }

    /// Generate the manifold and log neuron count + region split. Returns the
    /// neuron count so `main.ts` can confirm geometry exists (phase-1 "done
    /// when": neuron positions exist even though nothing draws them).
    #[wasm_bindgen]
    pub fn init_manifold(n: usize, seed: u32) -> usize {
        let config = SimConfig {
            n,
            seed: seed as u64,
            ..SimConfig::default()
        };
        let manifold = build_manifold(&config);
        let (input, assoc, output) = region_split(&manifold);
        web_sys::console::log_1(
            &format!(
                "[manifold] verts={} faces={} neurons={} regions(I/A/O)={}/{}/{}",
                manifold.vertices.len(),
                manifold.faces.len(),
                manifold.neuron_positions.len(),
                input,
                assoc,
                output
            )
            .into(),
        );
        manifold.neuron_positions.len()
    }

    /// Report cross-origin isolation status (SharedArrayBuffer availability) to
    /// the console — part of the phase-1 startup log (COOP/COEP check).
    #[wasm_bindgen]
    pub fn log_cross_origin_isolation(isolated: bool) {
        web_sys::console::log_1(
            &format!("[coi] crossOriginIsolated={isolated} (SharedArrayBuffer {})",
                if isolated { "available" } else { "UNAVAILABLE" })
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_manifold_via_config() {
        let c = SimConfig {
            n: 4000,
            ..SimConfig::default()
        };
        let m = build_manifold(&c);
        assert_eq!(m.neuron_positions.len(), 4000);
        let (i, a, o) = region_split(&m);
        assert_eq!(i + a + o, 4000);
    }
}
