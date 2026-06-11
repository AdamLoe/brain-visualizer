#![cfg(not(target_arch = "wasm32"))]

use brain_visualizer::connectivity;
use brain_visualizer::manifold::{Manifold, ManifoldParams};
use brain_visualizer::sim::morphology::{
    build_source_types, generate, MorphSegment, MorphologyParams,
};

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn point_bits(p: [f32; 3]) -> [u32; 3] {
    [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
}

fn dendrites_for<'a>(
    segments: &'a [MorphSegment],
    target_id: u32,
) -> impl Iterator<Item = &'a MorphSegment> {
    segments
        .iter()
        .filter(move |segment| segment.kind == 0 && segment.neuron_id == target_id)
}

#[test]
fn default_incoming_dendrites_branch_near_soma_without_drops() {
    let seed = 22u32;
    let k = 16usize;
    let manifold = Manifold::generate(&ManifoldParams::new(1200, seed));
    let positions = &manifold.neuron_positions;
    let params = MorphologyParams::locked_default();
    let source_types = build_source_types(seed, &manifold.neuron_regions);
    let morphology = generate(
        positions,
        &manifold.spatial_grid,
        k,
        seed,
        &params,
        &source_types,
        connectivity::ReachParams::LOCAL_ONLY,
    );

    assert_eq!(morphology.dropped, 0);
    assert_eq!(morphology.stats.dropped_count, 0);
    assert_eq!(morphology.stats.incoming_dropped_count, 0);
    assert!(morphology.stats.incoming_visible_groups_max > params.dendrite_primary_root_count);
    assert!(morphology.stats.segments_per_neuron_max <= params.segment_cap(k));

    let target_id = morphology
        .incoming_socket_group_ranges
        .iter()
        .enumerate()
        .filter(|(_, range)| range.len >= params.dendrite_primary_root_count)
        .max_by_key(|(_, range)| range.len)
        .map(|(target_id, _)| target_id as u32)
        .expect("default network should have dense incoming dendrite targets");
    let target_pos = positions[target_id as usize];
    let range = morphology.incoming_socket_group_ranges[target_id as usize];
    let groups = &morphology.incoming_socket_groups[range.start..range.start + range.len];

    let collars: Vec<&MorphSegment> = dendrites_for(&morphology.segments, target_id)
        .filter(|segment| segment.target_id == target_id && segment.path_len == 0.0)
        .collect();
    assert!(
        !collars.is_empty(),
        "target should emit soma-surface collars"
    );
    assert!(collars.len() <= params.dendrite_primary_root_count);
    for collar in &collars {
        let start_dist = dist(collar.a, target_pos);
        assert!(
            (params.base_radius * 1.02..=params.base_radius * 1.08).contains(&start_dist),
            "collar starts at {start_dist}, not on the soma surface"
        );
        assert!(collar.radius_a >= params.base_radius * params.dendrite_mid_radius_fraction * 0.99);
    }

    let close_fork_endpoint = dendrites_for(&morphology.segments, target_id)
        .filter(|segment| segment.target_id == target_id)
        .any(|segment| {
            let endpoint_dist = dist(segment.b, target_pos);
            endpoint_dist >= params.base_radius * 1.14
                && endpoint_dist <= params.base_radius * 2.35
                && segment.radius_b <= params.base_radius * params.dendrite_mid_radius_fraction
        });
    assert!(close_fork_endpoint, "expected a tapered fork near the soma");

    for group in groups {
        let has_source_owned_terminal =
            dendrites_for(&morphology.segments, target_id).any(|segment| {
                segment.target_id == group.source_id
                    && segment.path_len == 0.0
                    && point_bits(segment.a) == point_bits(group.socket_pos)
            });
        assert!(
            has_source_owned_terminal,
            "missing individually legible terminal leaf for group {group:?}"
        );
    }
}
