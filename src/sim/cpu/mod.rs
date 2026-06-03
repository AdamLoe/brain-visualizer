//! CPU backend (event-driven, rayon active list) — STUB in phase 1 (BV4, BV24).
//!
//! Real simulation lands in phase 6 on a dedicated coordinator worker + rayon
//! pool writing SharedArrayBuffer (BV24). Phase 1: `tick()` returns zeroed
//! stats and allocates nothing; SoA render slices are empty.

use crate::sim::backend::{RenderState, SimBackend, SimConfig, TickStats};

/// Event-driven CPU simulation backend.
pub struct CpuBackend {
    config: SimConfig,
    // Phase-6 state (positions / v_render / last_spike in SharedArrayBuffer)
    // is intentionally absent in phase 1. Empty render slices below.
    v_render: Vec<f32>,
    last_spike: Vec<u32>,
    positions: Vec<[f32; 3]>,
}

impl CpuBackend {
    pub fn new(config: SimConfig) -> Self {
        Self {
            config,
            v_render: Vec::new(),
            last_spike: Vec::new(),
            positions: Vec::new(),
        }
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }
}

impl SimBackend for CpuBackend {
    fn tick(&mut self, _ticks: u32, _excitability: f32) -> TickStats {
        TickStats::default()
    }

    fn stimulate(&mut self, _pos: [f32; 3], _radius: f32, _current: f32) {}

    fn render_state(&self) -> RenderState<'_> {
        RenderState::Cpu {
            v_render: &self.v_render,
            last_spike: &self.last_spike,
            positions: &self.positions,
        }
    }

    fn resize(&mut self, config: &SimConfig) {
        self.config = config.clone();
        // TODO(phase 6): allocate SoA SharedArrayBuffer-backed state for N.
    }

    fn destroy(&mut self) {
        // TODO(phase 6): terminate coordinator worker + rayon pool.
        self.v_render = Vec::new();
        self.last_spike = Vec::new();
        self.positions = Vec::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_returns_zeros() {
        let mut b = CpuBackend::new(SimConfig::default());
        assert_eq!(b.tick(2, 0.3), TickStats::default());
    }

    #[test]
    fn render_state_is_empty_cpu_slices() {
        let b = CpuBackend::new(SimConfig::default());
        match b.render_state() {
            RenderState::Cpu { v_render, .. } => assert!(v_render.is_empty()),
            _ => panic!("expected Cpu render state"),
        }
    }
}
