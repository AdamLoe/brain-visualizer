//! Network-payload timing harness (boot-speed observability).
//!
//! The "Prepare network payload" boot stage runs entirely in a Web Worker doing
//! PURE CPU work — it never touches a WebGPU device. That means the exact stall
//! phase can be timed on a box with NO GPU adapter (this one, under llvmpipe),
//! reproducing what the browser worker does. This harness builds the production
//! payload at the real default scale (N=6000, K=16 — mirrors
//! `web/src/core/types.ts → DEFAULT_CONFIG`) via the same
//! `PreparedNetworkBuild::prepare` path `prepare_network_payload` uses, and
//! prints per-phase wall-clock ms so we can SEE where the seconds go:
//!   folding manifold, source types, morphology TOTAL + MorphTimer breakdown
//!   (setup / incoming / dendrite / axon), soma spheres.
//!
//! Run: `cd app && cargo run -p brain-visualizer --example time_network_payload --release`
//!
//! Contract (boot UX): no single phase should represent > ~2s of silent work at
//! 6k/16, else the overlay percent would freeze; the harness numbers prove it.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use brain_visualizer::manifold::RegionAssignmentMode;
    use brain_visualizer::sim::backend::{BackendKind, SimConfig};
    use brain_visualizer::sim::gpu::{
        morph_params_from_config_and_visual, reach_from_visual_settings, VisualSettings,
    };
    use brain_visualizer::sim::morphology::{self, MorphologyConfig};
    use std::time::Instant;

    // Mirror DEFAULT_CONFIG (web/src/core/types.ts): N=6000, K=16.
    const N: usize = 6_000;
    const K: usize = 16;
    const SEED: u32 = 0;

    // Same production derivation as `prepare_network_payload` in lib.rs.
    let visual = VisualSettings::default();
    let morph_config = MorphologyConfig::default();
    let params = morph_params_from_config_and_visual(&morph_config, &visual);
    let reach = reach_from_visual_settings(&visual);
    let region_assignment = RegionAssignmentMode::HashRandom;
    let config = SimConfig {
        n: N,
        k: K,
        seed: SEED as u64,
        i_ext: visual.i_ext,
        backend: BackendKind::Gpu,
        ..SimConfig::default()
    };

    println!("== time_network_payload: N={N} K={K} seed={SEED} ==");

    // Phase 1: fold the manifold (band 0.0..0.15 in the boot overlay).
    let t = Instant::now();
    let manifold =
        brain_visualizer::build_manifold_with_region_assignment(&config, region_assignment);
    let manifold_ms = t.elapsed().as_secs_f64() * 1e3;

    // Phase 2: source types (band 0.15..0.25).
    let t = Instant::now();
    let source_types =
        morphology::build_source_types(config.seed_lo(), &manifold.neuron_regions);
    let source_ms = t.elapsed().as_secs_f64() * 1e3;

    // Phase 3: morphology generate — the heavy phase (band 0.25..0.85). The
    // MorphTimer sub-phase ms (setup/incoming/dendrite/axon/finalize) are read
    // from the returned stats.timings so we surface WHERE the time goes.
    let t = Instant::now();
    let morph = morphology::generate(
        &manifold.neuron_positions,
        &manifold.spatial_grid,
        config.k,
        config.seed_lo(),
        &params,
        &source_types,
        reach,
    );
    let morph_ms = t.elapsed().as_secs_f64() * 1e3;

    // Phase 4: soma spheres (band 0.85..1.0).
    let t = Instant::now();
    let spheres = morphology::emit_soma_spheres(
        &manifold.neuron_positions,
        &source_types,
        &params,
        &morph.process_roots,
    );
    let spheres_ms = t.elapsed().as_secs_f64() * 1e3;

    let tim = morph.stats.timings;
    let total = manifold_ms + source_ms + morph_ms + spheres_ms;

    println!("phase                         ms");
    println!("-------------------------------------");
    println!("folding manifold        {manifold_ms:>10.1}");
    println!("source types            {source_ms:>10.1}");
    println!("morphology TOTAL        {morph_ms:>10.1}");
    println!("  - setup               {:>10.1}", tim.setup_ms);
    println!("  - incoming (view)     {:>10.1}", tim.incoming_ms);
    println!("  - dendrite            {:>10.1}", tim.dendrite_ms);
    println!("  - axon                {:>10.1}", tim.axon_ms);
    println!("  - finalize            {:>10.1}", tim.finalize_ms);
    println!("soma spheres            {spheres_ms:>10.1}");
    println!("-------------------------------------");
    println!("TOTAL                   {total:>10.1}");
    println!();
    println!(
        "segments={} spheres={} dropped={} duplicate_targets={}",
        morph.stats.segment_count,
        spheres.len(),
        morph.dropped,
        morph.stats.duplicate_targets
    );

    // Re-run through the production `prepare_with_progress` path to verify the
    // continuous sub-progress cadence: count emits, confirm fractions are
    // monotone, and report the largest wall-clock gap between consecutive emits
    // (the no-silent-gap contract: must stay well under ~1.5s even on slow CPUs).
    use brain_visualizer::sim::gpu::PreparedNetworkBuild;
    use std::cell::RefCell;
    let samples: RefCell<Vec<(f32, f64)>> = RefCell::new(Vec::new());
    let start = Instant::now();
    let cb = |_label: &str, frac: f32| {
        samples
            .borrow_mut()
            .push((frac, start.elapsed().as_secs_f64() * 1e3));
    };
    let _ = PreparedNetworkBuild::prepare_with_progress(
        config,
        params,
        reach,
        region_assignment,
        Some(&cb),
    );
    let s = samples.into_inner();
    let mut max_gap = 0.0f64;
    let mut prev_t = 0.0f64;
    let mut monotone = true;
    let mut prev_f = -1.0f32;
    for &(f, t) in &s {
        max_gap = max_gap.max(t - prev_t);
        prev_t = t;
        if f + 1e-4 < prev_f {
            monotone = false;
        }
        prev_f = prev_f.max(f);
    }
    println!();
    println!(
        "prepare_with_progress: {} emits, monotone={monotone}, max_gap_between_emits={max_gap:.1}ms",
        s.len()
    );
}
