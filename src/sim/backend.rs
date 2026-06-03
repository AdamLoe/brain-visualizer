//! The `SimBackend` interface and shared simulation types (BV4).
//!
//! Two implementations (GPU/WebGPU compute, CPU/rayon event-driven) sit behind
//! this one trait and are switched at runtime via full teardown+restart (BV16).
//! Phase 1 defines the types and stub backends; ticks return zeroed stats.

/// Locked fixed-point current scale factor S = 2^12 (BV19).
pub const FIXED_POINT_SCALE: i32 = 4096;

/// One interchangeable simulation backend.
pub trait SimBackend {
    /// Advance the simulation by one or more ticks. `ticks` is determined by
    /// the speed preset; `excitability` is in `[0.0, 1.0]` (BV9).
    fn tick(&mut self, ticks: u32, excitability: f32) -> TickStats;

    /// Inject current into neurons within `radius` of `pos` (world space).
    fn stimulate(&mut self, pos: [f32; 3], radius: f32, current: f32);

    /// Read-only view of current neuron state for rendering.
    fn render_state(&self) -> RenderState<'_>;

    /// Resize the network. Triggers reallocation; call only on tier change.
    fn resize(&mut self, config: &SimConfig);

    /// Release owned GPU resources / terminate workers. Required for backend
    /// switch, tier restart, page teardown, and device-loss recovery (BV16).
    fn destroy(&mut self);
}

/// Full simulation configuration. All fields present from phase 1; later
/// phases consume them.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Neuron count.
    pub n: usize,
    /// Synaptic out-degree (BV18, per-tier knob).
    pub k: usize,
    /// Network seed; same seed → same network across backends (BV16).
    pub seed: u64,
    pub tier: Tier,
    pub speed: SpeedPreset,
    pub backend: BackendKind,
    /// Ambient drive for input-region neurons (BV17).
    pub i_ext: f32,
    /// Locked at 4096 (2^12); do not change without an overflow check (BV19).
    pub fixed_point_scale: i32,
}

impl SimConfig {
    /// Lower 32 bits of the seed — the `seed_lo` fed to the BV22 hash.
    #[inline]
    pub fn seed_lo(&self) -> u32 {
        self.seed as u32
    }
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            n: 50_000,
            k: 32,
            seed: 0x5eed_5eed,
            tier: Tier::Balanced,
            speed: SpeedPreset::Normal,
            backend: BackendKind::Gpu,
            i_ext: 0.06,
            fixed_point_scale: FIXED_POINT_SCALE,
        }
    }
}

/// Per-frame simulation statistics (accumulated across this frame's ticks).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TickStats {
    pub tick_count: u32,
    pub spikes: u64,
    pub synaptic_events: u64,
    /// Wall time for all ticks this frame (ms).
    pub tick_ms: f32,
}

impl TickStats {
    /// Accumulate another stats record into this one (sum counts, sum time).
    pub fn accumulate(&mut self, other: &TickStats) {
        self.tick_count += other.tick_count;
        self.spikes += other.spikes;
        self.synaptic_events += other.synaptic_events;
        self.tick_ms += other.tick_ms;
    }
}

/// Speed multiplier presets (BV14). The ticks-per-frame mapping lives in the TS
/// rAF loop (`ticksThisFrame`); the renderer always runs at native rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedPreset {
    Quarter,
    Half,
    Normal,
    Double,
}

/// Active simulation backend (BV4, BV12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Gpu,
    Cpu,
}

/// Difficulty tier (BV3). The adaptive scaler operates inside a tier (BV1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Low,
    Balanced,
    Max,
}

/// Data the renderer reads each frame.
/// - GPU backend: raw buffer handles, state stays GPU-resident (zero readback).
/// - CPU backend: slices of changed neurons (uploaded to WebGL2 each frame).
pub enum RenderState<'a> {
    Gpu {
        v_buf: &'a wgpu::Buffer,
        /// bit31 valid, bits[30:24] type, bits[23:0] tick (BV21).
        last_spike_buf: &'a wgpu::Buffer,
        pos_x_buf: &'a wgpu::Buffer,
        pos_y_buf: &'a wgpu::Buffer,
        pos_z_buf: &'a wgpu::Buffer,
        neuron_count: usize,
    },
    Cpu {
        v_render: &'a [f32],
        last_spike: &'a [u32],
        positions: &'a [[f32; 3]],
    },
    /// Phase-1 stub state: nothing allocated, nothing to render.
    Empty,
}

// --- Packed `last_spike` helpers (BV21, architecture §2) ------------------
pub const HAS_SPIKED_MASK: u32 = 0x8000_0000;
pub const TYPE_MASK: u32 = 0x7F00_0000;
pub const TICK_MASK: u32 = 0x00FF_FFFF;

/// 7-bit neuron type (E/I flag + cortical region) from a packed `last_spike`.
#[inline]
pub fn neuron_type(packed: u32) -> u8 {
    ((packed >> 24) & 0x7F) as u8
}

/// Whether the neuron has ever spiked (silent-start safe).
#[inline]
pub fn has_spiked(packed: u32) -> bool {
    packed & HAS_SPIKED_MASK != 0
}

/// Modular 24-bit tick difference (`(now - then) & TICK_MASK`).
#[inline]
pub fn tick_diff(now: u32, then: u32) -> u32 {
    (now.wrapping_sub(then)) & TICK_MASK
}

/// Region code packed into the type byte above the E/I bit. The integrate
/// shader treats `(type >> 2) == 0` as an input-region neuron, so Input must be
/// 0. Region occupies bits [3:2] of the 7-bit type field.
#[inline]
pub fn region_code(region: crate::manifold::RegionKind) -> u8 {
    match region {
        crate::manifold::RegionKind::Input => 0,
        crate::manifold::RegionKind::Association => 1,
        crate::manifold::RegionKind::Output => 2,
    }
}

/// Build the 7-bit neuron type byte (BV21): `(region_code << 2) | ei_flag`.
/// E/I assignment is `hash32(neuron_id ^ seed_lo) % 5 == 0` → inhibitory
/// (bit 0 = 1), i.e. ~20% inhibitory (phase-2 spec / BV5 E/I ratio).
#[inline]
pub fn neuron_type_byte(neuron_id: u32, seed_lo: u32, region: crate::manifold::RegionKind) -> u8 {
    let inhibitory = crate::connectivity::hash::hash32(neuron_id ^ seed_lo) % 5 == 0;
    let ei = if inhibitory { 1u8 } else { 0u8 };
    (region_code(region) << 2) | ei
}

/// Pack an initial silent-start `last_spike` word: `HAS_SPIKED = 0`, type bits
/// set, tick = 0 (BV21).
#[inline]
pub fn initial_last_spike(neuron_id: u32, seed_lo: u32, region: crate::manifold::RegionKind) -> u32 {
    ((neuron_type_byte(neuron_id, seed_lo, region) as u32) << 24) & TYPE_MASK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickstats_accumulate() {
        let mut a = TickStats {
            tick_count: 1,
            spikes: 10,
            synaptic_events: 100,
            tick_ms: 1.5,
        };
        let b = TickStats {
            tick_count: 2,
            spikes: 5,
            synaptic_events: 50,
            tick_ms: 0.5,
        };
        a.accumulate(&b);
        assert_eq!(a.tick_count, 3);
        assert_eq!(a.spikes, 15);
        assert_eq!(a.synaptic_events, 150);
        assert_eq!(a.tick_ms, 2.0);
    }

    #[test]
    fn packed_helpers() {
        let packed = HAS_SPIKED_MASK | (0x42 << 24) | 0x00AB_CDEF;
        assert!(has_spiked(packed));
        assert_eq!(neuron_type(packed), 0x42);
        assert!(!has_spiked(0x0042_0000));
    }

    #[test]
    fn tick_diff_wraps_mod_24bit() {
        assert_eq!(tick_diff(5, 3), 2);
        // Wrap across the 24-bit boundary.
        assert_eq!(tick_diff(1, TICK_MASK), 2);
    }

    #[test]
    fn seed_lo_truncates() {
        let c = SimConfig {
            seed: 0xABCD_1234_5678_9ABC,
            ..Default::default()
        };
        assert_eq!(c.seed_lo(), 0x5678_9ABC);
    }

    #[test]
    fn input_region_decodes_to_zero_upper_bits() {
        use crate::manifold::RegionKind;
        // The integrate shader treats (type >> 2) == 0 as input region.
        let t_in = neuron_type_byte(0, 0, RegionKind::Input);
        let t_assoc = neuron_type_byte(0, 0, RegionKind::Association);
        let t_out = neuron_type_byte(0, 0, RegionKind::Output);
        assert_eq!(t_in >> 2, 0, "input region must have upper type bits 0");
        assert_ne!(t_assoc >> 2, 0);
        assert_ne!(t_out >> 2, 0);
    }

    #[test]
    fn initial_last_spike_is_silent_with_type() {
        use crate::manifold::RegionKind;
        let w = initial_last_spike(7, 0x5eed, RegionKind::Output);
        // Silent start: HAS_SPIKED clear, tick bits 0, type bits set.
        assert!(!has_spiked(w));
        assert_eq!(w & TICK_MASK, 0);
        assert_eq!(neuron_type(w), neuron_type_byte(7, 0x5eed, RegionKind::Output));
    }

    #[test]
    fn ei_ratio_about_20_percent_inhibitory() {
        use crate::manifold::RegionKind;
        let n = 20_000u32;
        let inhib = (0..n)
            .filter(|&i| neuron_type_byte(i, 0xdead, RegionKind::Association) & 1 == 1)
            .count();
        let frac = inhib as f32 / n as f32;
        // hash32(id ^ seed) % 5 == 0 -> ~20% inhibitory (BV5).
        assert!((frac - 0.20).abs() < 0.02, "E/I ratio off: {frac:.3}");
    }
}
