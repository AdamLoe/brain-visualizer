//! Simulation layer: the backend trait + GPU implementation + the adaptive
//! scaler stub (architecture §1, §9).

pub mod backend;
pub mod gpu;
pub mod morphology;
pub mod scaler;

pub use backend::{BackendKind, RenderState, SimBackend, SimConfig, SpeedPreset, TickStats, Tier};
