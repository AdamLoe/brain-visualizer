use brain_visualizer::connectivity::spatial::SpatialGrid;
use brain_visualizer::sim::gpu::resources::StimGridUniform;

const STIMULATE_WGSL: &str = include_str!("../src/sim/gpu/shaders/stimulate.wgsl");

#[test]
fn stim_grid_uniform_uploads_actual_spatial_grid_bounds() {
    let positions = vec![[2.0, -3.0, 0.5], [3.0, -2.0, 1.5], [4.0, -1.0, 2.5]];
    let grid = SpatialGrid::build(&positions, 8);

    let uniform = StimGridUniform::from_grid(&grid, positions.len() as u32);

    assert_eq!(std::mem::size_of::<StimGridUniform>(), 32);
    assert_eq!(std::mem::size_of::<StimGridUniform>() % 16, 0);
    assert_eq!(uniform.grid_min, grid.min);
    assert_eq!(uniform.cell_size, grid.cell_size);
    assert_eq!(uniform.grid_dim, grid.dim);
    assert_eq!(uniform.n, positions.len() as u32);
    assert_ne!(uniform.grid_min, [-1.5; 3]);
}

#[test]
fn stimulate_shader_uses_uploaded_grid_bounds_instead_of_constants() {
    assert!(STIMULATE_WGSL.contains("grid_min: vec3<f32>"));
    assert!(STIMULATE_WGSL.contains("cell_size: f32"));
    assert!(STIMULATE_WGSL.contains("p - grid_u.grid_min[axis]"));
    assert!(STIMULATE_WGSL.contains("/ grid_u.cell_size"));
    assert!(!STIMULATE_WGSL.contains("const GRID_MIN"));
    assert!(!STIMULATE_WGSL.contains("const GRID_MAX"));
}
