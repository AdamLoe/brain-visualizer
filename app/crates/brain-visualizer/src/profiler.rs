//! First-class performance profiler (BV8, architecture §8).
//!
//! Emits one structured JSON line per second. Host-testable: the clock is
//! **injected** (caller passes a `now_ms`) rather than calling
//! `performance.now()` internally, so the dump cadence and the JSON payload can
//! be unit-tested without a browser. Hot-path hygiene: `record_frame` allocates
//! nothing; the per-second dump builds one small string.

use crate::sim::backend::{BackendKind, TickStats, Tier};

/// Fixed-capacity ring buffer of `f32` frame times. No per-frame allocation.
#[derive(Debug)]
pub struct RingBuffer<const CAP: usize> {
    data: [f32; CAP],
    len: usize,
    head: usize,
}

impl<const CAP: usize> Default for RingBuffer<CAP> {
    fn default() -> Self {
        Self {
            data: [0.0; CAP],
            len: 0,
            head: 0,
        }
    }
}

impl<const CAP: usize> RingBuffer<CAP> {
    pub fn push(&mut self, v: f32) {
        self.data[self.head] = v;
        self.head = (self.head + 1) % CAP;
        if self.len < CAP {
            self.len += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn avg(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let sum: f32 = self.data[..self.len].iter().sum();
        sum / self.len as f32
    }

    /// p-th percentile (0..=100) of the current contents. Copies into a small
    /// stack-ish Vec only when called (once/second), never in the hot path.
    pub fn percentile(&self, p: f32) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let mut v: Vec<f32> = self.data[..self.len].to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let rank = ((p / 100.0) * (self.len as f32 - 1.0)).round() as usize;
        v[rank.min(self.len - 1)]
    }
}

/// One second's worth of aggregated counters, ready to serialize.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileSnapshot {
    pub fps: f32,
    pub frame_ms_avg: f32,
    pub frame_ms_p95: f32,
    pub ticks_per_sec: f32,
    pub spikes_per_sec: f32,
    pub synaptic_events_per_sec: f32,
    pub backend: BackendKind,
    pub tier: Tier,
    pub n: usize,
    pub k: usize,
}

impl ProfileSnapshot {
    /// Serialize to the one-line JSON the console dump emits.
    pub fn to_json(&self) -> String {
        let backend = match self.backend {
            BackendKind::Gpu => "gpu",
            BackendKind::Cpu => "cpu",
        };
        let tier = match self.tier {
            Tier::Low => "low",
            Tier::Balanced => "balanced",
            Tier::Max => "max",
        };
        format!(
            "{{\"fps\":{:.1},\"frame_ms_avg\":{:.3},\"frame_ms_p95\":{:.3},\
\"ticks_per_sec\":{:.1},\"spikes_per_sec\":{:.1},\
\"synaptic_events_per_sec\":{:.1},\"backend\":\"{}\",\"tier\":\"{}\",\
\"n\":{},\"k\":{}}}",
            self.fps,
            self.frame_ms_avg,
            self.frame_ms_p95,
            self.ticks_per_sec,
            self.spikes_per_sec,
            self.synaptic_events_per_sec,
            backend,
            tier,
            self.n,
            self.k,
        )
    }
}

/// Rolling profiler. 120-frame window for frame-time stats; per-second dump.
pub struct Profiler {
    frame_times: RingBuffer<120>,
    /// Counters accumulated since the last dump.
    window: TickStats,
    frames_this_window: u32,
    last_dump_ms: f64,
    window_start_ms: f64,
    started: bool,
    // Static config echoed into the dump.
    backend: BackendKind,
    tier: Tier,
    n: usize,
    k: usize,
}

impl Profiler {
    pub fn new(backend: BackendKind, tier: Tier, n: usize, k: usize) -> Self {
        Self {
            frame_times: RingBuffer::default(),
            window: TickStats::default(),
            frames_this_window: 0,
            last_dump_ms: 0.0,
            window_start_ms: 0.0,
            started: false,
            backend,
            tier,
            n,
            k,
        }
    }

    /// Echo config changes (tier/backend/N/K resize).
    pub fn set_config(&mut self, backend: BackendKind, tier: Tier, n: usize, k: usize) {
        self.backend = backend;
        self.tier = tier;
        self.n = n;
        self.k = k;
    }

    /// Record one rendered frame. `frame_ms` is the wall time since the last
    /// frame; `stats` are this frame's tick counters. Allocation-free.
    pub fn record_frame(&mut self, now_ms: f64, frame_ms: f32, stats: TickStats) {
        if !self.started {
            self.started = true;
            self.last_dump_ms = now_ms;
            self.window_start_ms = now_ms;
        }
        self.frame_times.push(frame_ms);
        self.window.accumulate(&stats);
        self.frames_this_window += 1;
    }

    /// Produce a snapshot iff ≥1000 ms elapsed since the last dump, resetting
    /// the window. Returns `None` otherwise. The caller logs the JSON
    /// (`console.log` on wasm). Keeping I/O out of here keeps it host-testable.
    pub fn maybe_dump(&mut self, now_ms: f64) -> Option<ProfileSnapshot> {
        if !self.started {
            return None;
        }
        let elapsed_ms = now_ms - self.last_dump_ms;
        if elapsed_ms < 1000.0 {
            return None;
        }
        let elapsed_s = (elapsed_ms / 1000.0) as f32;

        let snapshot = ProfileSnapshot {
            fps: self.frames_this_window as f32 / elapsed_s,
            frame_ms_avg: self.frame_times.avg(),
            frame_ms_p95: self.frame_times.percentile(95.0),
            ticks_per_sec: self.window.tick_count as f32 / elapsed_s,
            spikes_per_sec: self.window.spikes as f32 / elapsed_s,
            synaptic_events_per_sec: self.window.synaptic_events as f32 / elapsed_s,
            backend: self.backend,
            tier: self.tier,
            n: self.n,
            k: self.k,
        };

        // Reset the window (frame_times ring persists for smoother avg/p95).
        self.window = TickStats::default();
        self.frames_this_window = 0;
        self.last_dump_ms = now_ms;
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_avg_and_percentile() {
        let mut r: RingBuffer<4> = RingBuffer::default();
        for v in [1.0, 2.0, 3.0, 4.0] {
            r.push(v);
        }
        assert_eq!(r.avg(), 2.5);
        assert_eq!(r.percentile(0.0), 1.0);
        assert_eq!(r.percentile(100.0), 4.0);
        // Ring overwrites oldest.
        r.push(5.0);
        assert_eq!(r.len(), 4);
        assert_eq!(r.percentile(100.0), 5.0);
    }

    #[test]
    fn no_dump_before_one_second() {
        let mut p = Profiler::new(BackendKind::Gpu, Tier::Balanced, 100, 32);
        p.record_frame(0.0, 16.0, TickStats::default());
        assert!(p.maybe_dump(500.0).is_none());
    }

    #[test]
    fn dumps_after_one_second_with_rates() {
        let mut p = Profiler::new(BackendKind::Gpu, Tier::Balanced, 1000, 64);
        // 60 frames over exactly 1 s, each with 1 tick / 10 spikes / 320 syn.
        for f in 0..60 {
            let stats = TickStats {
                tick_count: 1,
                spikes: 10,
                synaptic_events: 320,
                tick_ms: 0.1,
            };
            p.record_frame(f as f64 * (1000.0 / 60.0), 16.0, stats);
        }
        let snap = p.maybe_dump(1000.0).expect("should dump at 1 s");
        assert!((snap.fps - 60.0).abs() < 0.5, "fps {}", snap.fps);
        assert!((snap.ticks_per_sec - 60.0).abs() < 0.5);
        assert!((snap.spikes_per_sec - 600.0).abs() < 5.0);
        assert!((snap.synaptic_events_per_sec - 19200.0).abs() < 50.0);
        assert!((snap.frame_ms_avg - 16.0).abs() < 0.1);
    }

    #[test]
    fn json_has_all_fields() {
        let snap = ProfileSnapshot {
            fps: 60.0,
            frame_ms_avg: 16.0,
            frame_ms_p95: 18.0,
            ticks_per_sec: 60.0,
            spikes_per_sec: 600.0,
            synaptic_events_per_sec: 19200.0,
            backend: BackendKind::Cpu,
            tier: Tier::Max,
            n: 1000,
            k: 64,
        };
        let json = snap.to_json();
        for field in [
            "fps",
            "frame_ms_avg",
            "frame_ms_p95",
            "ticks_per_sec",
            "spikes_per_sec",
            "synaptic_events_per_sec",
            "backend",
            "tier",
            "\"n\":1000",
            "\"k\":64",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }
        assert!(json.contains("\"backend\":\"cpu\""));
        assert!(json.contains("\"tier\":\"max\""));
    }
}
