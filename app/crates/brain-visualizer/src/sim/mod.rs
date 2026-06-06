//! Simulation layer: the backend trait + two implementations + the adaptive
//! scaler stub (architecture §1, §9).

pub mod backend;
pub mod cpu;
pub mod gpu;
pub mod morphology;
pub mod scaler;

pub use backend::{
    BackendKind, RenderState, SimBackend, SimConfig, SpeedPreset, TickStats, Tier,
};
