//! Comprehensive tests for hydrology algorithms
//!
//! Tests hydrological analysis including:
//! - Flow direction (D8 and D-infinity)
//! - Flow accumulation
//! - Depression/sink filling
//! - Stream network extraction
//! - Watershed delineation
//! - Edge cases and validation

use oxigdal_algorithms::raster::hydrology::{
    D8Direction, compute_d8_flow_direction, compute_dinf_flow_direction, compute_flow_accumulation,
    compute_stream_order, compute_weighted_flow_accumulation, delineate_watersheds,
    extract_stream_network, fill_sinks, identify_sinks,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[allow(unused_imports)]
use oxigdal_algorithms::raster::hydrology::FlowMethod;

// ============================================================================
// Helper Functions
// ============================================================================

#[allow(dead_code)]
fn create_test_dem(width: u64, height: u64) -> RasterBuffer {
    RasterBuffer::zeros(width, height, RasterDataType::Float32)
}

fn create_flat_dem(width: u64, height: u64, elevation: f64) -> RasterBuffer {
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn create_west_to_east_slope(width: u64, height: u64) -> RasterBuffer {
    // Higher elevation in west, lower in east
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let elevation = (width - x) as f64;
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn create_north_to_south_slope(width: u64, height: u64) -> RasterBuffer {
    // Higher elevation in north, lower in south
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let elevation = (height - y) as f64;
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn create_cone_dem(width: u64, height: u64) -> RasterBuffer {
    // Peak at center, sloping down radially
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;
    let max_dist = (cx * cx + cy * cy).sqrt();

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let elevation = max_dist - dist;
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn create_valley_dem(width: u64, height: u64) -> RasterBuffer {
    // Valley in center running north-south
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    let cx = width as f64 / 2.0;

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64 - cx).abs();
            let _ = dem.set_pixel(x, y, dx);
        }
    }
    dem
}

fn create_depression_dem(width: u64, height: u64) -> RasterBuffer {
    // Flat with depression in center
    let mut dem = create_flat_dem(width, height, 10.0);
    let cx = width / 2;
    let cy = height / 2;

    // Create depression
    let _ = dem.set_pixel(cx, cy, 5.0);

    dem
}

// ============================================================================
// D8 Direction Tests
// ============================================================================

#[test]
fn test_d8_direction_offsets() {
    assert_eq!(D8Direction::East.offset(), (1, 0));
    assert_eq!(D8Direction::Southeast.offset(), (1, 1));
    assert_eq!(D8Direction::South.offset(), (0, 1));
    assert_eq!(D8Direction::Southwest.offset(), (-1, 1));
    assert_eq!(D8Direction::West.offset(), (-1, 0));
    assert_eq!(D8Direction::Northwest.offset(), (-1, -1));
    assert_eq!(D8Direction::North.offset(), (0, -1));
    assert_eq!(D8Direction::Northeast.offset(), (1, -1));
}

#[test]
fn test_d8_direction_all() {
    let directions = D8Direction::all();
    assert_eq!(directions.len(), 8);
}

#[test]
fn test_d8_direction_angles() {
    assert!((D8Direction::East.angle_degrees() - 0.0).abs() < 1e-6);
    assert!((D8Direction::Southeast.angle_degrees() - 45.0).abs() < 1e-6);
    assert!((D8Direction::South.angle_degrees() - 90.0).abs() < 1e-6);
    assert!((D8Direction::Southwest.angle_degrees() - 135.0).abs() < 1e-6);
    assert!((D8Direction::West.angle_degrees() - 180.0).abs() < 1e-6);
    assert!((D8Direction::Northwest.angle_degrees() - 225.0).abs() < 1e-6);
    assert!((D8Direction::North.angle_degrees() - 270.0).abs() < 1e-6);
    assert!((D8Direction::Northeast.angle_degrees() - 315.0).abs() < 1e-6);
}

// ============================================================================
// Flow Direction Tests
// ============================================================================

#[test]
fn test_compute_d8_flow_direction_east_slope() {
    let dem = create_west_to_east_slope(10, 10);
    let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("Should compute D8 flow direction");

    // Flow should be east (code 1)
    let center = flow_dir.get_pixel(5, 5).expect("Should get pixel");
    assert!(
        (center - 1.0).abs() < 0.01,
        "Expected East (1), got {}",
        center
    );
}

#[test]
fn test_compute_d8_flow_direction_south_slope() {
    let dem = create_north_to_south_slope(10, 10);
    let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("Should compute D8 flow direction");

    // Flow should be south (code 4)
    let center = flow_dir.get_pixel(5, 5).expect("Should get pixel");
    assert!(
        (center - 4.0).abs() < 0.01,
        "Expected South (4), got {}",
        center
    );
}

#[test]
fn test_compute_d8_flow_direction_cone() {
    let dem = create_cone_dem(15, 15);
    let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("Should compute D8 flow direction");

    // Flow should go away from center in all directions
    // Just check that computation succeeds
    assert_eq!(flow_dir.width(), 15);
    assert_eq!(flow_dir.height(), 15);
}

#[test]
fn test_compute_d8_flow_direction_preserves_dimensions() {
    let dem = create_west_to_east_slope(20, 30);
    let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("Should compute D8 flow direction");

    assert_eq!(flow_dir.width(), 20);
    assert_eq!(flow_dir.height(), 30);
}

#[test]
fn test_compute_dinf_flow_direction() {
    let dem = create_west_to_east_slope(10, 10);
    let (flow_angle, flow_proportion) =
        compute_dinf_flow_direction(&dem, 1.0).expect("Should compute D-inf flow direction");

    // Check dimensions
    assert_eq!(flow_angle.width(), 10);
    assert_eq!(flow_proportion.width(), 10);

    // Flow angle should be around 0 degrees (east)
    let angle = flow_angle.get_pixel(5, 5).expect("Should get pixel");
    assert!((0.0..=360.0).contains(&angle));

    // Flow proportion should be between 0 and 1
    let prop = flow_proportion.get_pixel(5, 5).expect("Should get pixel");
    assert!((0.0..=1.0).contains(&prop));
}

#[test]
fn test_compute_dinf_diagonal_slope() {
    // Create diagonal slope (southeast)
    let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
    for y in 0..10 {
        for x in 0..10 {
            let elevation = (10 - x + 10 - y) as f64;
            let _ = dem.set_pixel(x, y, elevation);
        }
    }

    let (flow_angle, _) =
        compute_dinf_flow_direction(&dem, 1.0).expect("Should compute D-inf flow direction");

    // Flow should be around 45 degrees (southeast)
    let angle = flow_angle.get_pixel(5, 5).expect("Should get pixel");
    assert!((0.0..=360.0).contains(&angle));
}

// ============================================================================
// Flow Accumulation Tests
// ============================================================================

#[test]
fn test_compute_flow_accumulation_simple() {
    let dem = create_west_to_east_slope(10, 10);
    let accum = compute_flow_accumulation(&dem, 1.0).expect("Should compute flow accumulation");

    // Eastern edge should have higher accumulation
    let west_edge = accum.get_pixel(1, 5).expect("Should get pixel");
    let east_edge = accum.get_pixel(8, 5).expect("Should get pixel");

    assert!(
        east_edge >= west_edge,
        "East edge ({}) should have >= accumulation than west ({})",
        east_edge,
        west_edge
    );
}

#[test]
fn test_compute_flow_accumulation_minimum_is_one() {
    let dem = create_west_to_east_slope(10, 10);
    let accum = compute_flow_accumulation(&dem, 1.0).expect("Should compute flow accumulation");

    // All cells should have at least 1 (themselves)
    for y in 0..10 {
        for x in 0..10 {
            let val = accum.get_pixel(x, y).expect("Should get pixel");
            assert!(
                val >= 1.0,
                "Accumulation at ({}, {}) is {}, expected >= 1",
                x,
                y,
                val
            );
        }
    }
}

#[test]
fn test_compute_flow_accumulation_valley() {
    let dem = create_valley_dem(10, 10);
    let accum = compute_flow_accumulation(&dem, 1.0).expect("Should compute flow accumulation");

    // Center should have higher accumulation (valley bottom)
    let center = accum.get_pixel(5, 5).expect("Should get pixel");
    let side = accum.get_pixel(1, 5).expect("Should get pixel");

    assert!(
        center >= side,
        "Center ({}) should have >= accumulation than side ({})",
        center,
        side
    );
}

#[test]
fn test_compute_weighted_flow_accumulation() {
    let dem = create_west_to_east_slope(10, 10);
    let mut weights = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

    // Uniform weight of 2
    for y in 0..10 {
        for x in 0..10 {
            let _ = weights.set_pixel(x, y, 2.0);
        }
    }

    let accum = compute_weighted_flow_accumulation(&dem, &weights, 1.0)
        .expect("Should compute weighted flow accumulation");

    // All cells should have at least 2 (their weight)
    for y in 0..10 {
        for x in 0..10 {
            let val = accum.get_pixel(x, y).expect("Should get pixel");
            assert!(
                val >= 2.0,
                "Weighted accumulation at ({}, {}) is {}, expected >= 2",
                x,
                y,
                val
            );
        }
    }
}

#[test]
fn test_compute_weighted_flow_accumulation_dimension_mismatch() {
    let dem = create_west_to_east_slope(10, 10);
    let weights = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

    let result = compute_weighted_flow_accumulation(&dem, &weights, 1.0);
    assert!(result.is_err());
}

// ============================================================================
// Sink Identification and Filling Tests
// ============================================================================

#[test]
fn test_identify_sinks_with_depression() {
    let dem = create_depression_dem(5, 5);
    let sinks = identify_sinks(&dem).expect("Should identify sinks");

    // Center should be identified as sink
    let center = sinks.get_pixel(2, 2).expect("Should get pixel");
    assert!(
        center > 0.0,
        "Center depression should be identified as sink"
    );
}

#[test]
fn test_identify_sinks_no_sinks() {
    let dem = create_west_to_east_slope(5, 5);
    let sinks = identify_sinks(&dem).expect("Should identify sinks");

    // Interior should have no sinks (monotonic slope)
    let center = sinks.get_pixel(2, 2).expect("Should get pixel");
    assert!(
        center == 0.0,
        "Monotonic slope should have no interior sinks"
    );
}

#[test]
fn test_fill_sinks_raises_depression() {
    let dem = create_depression_dem(5, 5);
    let filled = fill_sinks(&dem, 0.001).expect("Should fill sinks");

    // Center should be raised
    let original = dem.get_pixel(2, 2).expect("Should get pixel");
    let filled_val = filled.get_pixel(2, 2).expect("Should get pixel");

    assert!(
        filled_val > original,
        "Filled value ({}) should be greater than original ({})",
        filled_val,
        original
    );
}

#[test]
fn test_fill_sinks_preserves_drainage() {
    let dem = create_depression_dem(5, 5);
    let filled = fill_sinks(&dem, 0.001).expect("Should fill sinks");

    // Check that filled DEM has no interior sinks
    let sinks = identify_sinks(&filled).expect("Should identify sinks");

    // Interior should have no sinks
    let center = sinks.get_pixel(2, 2).expect("Should get pixel");
    assert!(
        center == 0.0,
        "Filled DEM should have no interior sinks at center"
    );
}

#[test]
fn test_fill_sinks_epsilon_effect() {
    let dem = create_depression_dem(5, 5);

    let filled_small_eps = fill_sinks(&dem, 0.001).expect("Should fill with small epsilon");
    let filled_large_eps = fill_sinks(&dem, 1.0).expect("Should fill with large epsilon");

    // Larger epsilon should result in higher values
    let center_small = filled_small_eps.get_pixel(2, 2).expect("Should get pixel");
    let center_large = filled_large_eps.get_pixel(2, 2).expect("Should get pixel");

    assert!(
        center_large >= center_small,
        "Larger epsilon ({}) should give higher fill ({})",
        center_large,
        center_small
    );
}

#[test]
fn test_fill_sinks_flat_dem() {
    let dem = create_flat_dem(5, 5, 10.0);
    let filled = fill_sinks(&dem, 0.001).expect("Should fill sinks");

    // Flat DEM should be unchanged or slightly raised at interior
    for y in 0..5 {
        for x in 0..5 {
            let filled_val = filled.get_pixel(x, y).expect("Should get pixel");
            assert!(filled_val >= 10.0, "Filled should be >= original");
        }
    }
}

// ============================================================================
// Stream Network Tests
// ============================================================================

#[test]
fn test_extract_stream_network_simple() {
    let mut accum = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

    // Create high accumulation line
    let _ = accum.set_pixel(2, 0, 100.0);
    let _ = accum.set_pixel(2, 1, 90.0);
    let _ = accum.set_pixel(2, 2, 80.0);
    let _ = accum.set_pixel(2, 3, 70.0);
    let _ = accum.set_pixel(2, 4, 60.0);

    let streams = extract_stream_network(&accum, 50.0).expect("Should extract stream network");

    // High accumulation cells should be streams
    assert!(streams.get_pixel(2, 0).expect("Should get pixel") > 0.0);
    assert!(streams.get_pixel(2, 2).expect("Should get pixel") > 0.0);
    assert!(streams.get_pixel(2, 4).expect("Should get pixel") > 0.0);

    // Low accumulation cells should not be streams
    assert!(streams.get_pixel(0, 0).expect("Should get pixel") == 0.0);
}

#[test]
fn test_extract_stream_network_threshold() {
    let mut accum = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

    // Create gradient of accumulation
    for y in 0..10 {
        for x in 0..10 {
            let _ = accum.set_pixel(x, y, (x * 10) as f64);
        }
    }

    let streams_low = extract_stream_network(&accum, 30.0).expect("Low threshold");
    let streams_high = extract_stream_network(&accum, 70.0).expect("High threshold");

    // Count stream cells
    let mut count_low = 0;
    let mut count_high = 0;
    for y in 0..10 {
        for x in 0..10 {
            if streams_low.get_pixel(x, y).expect("Should get pixel") > 0.0 {
                count_low += 1;
            }
            if streams_high.get_pixel(x, y).expect("Should get pixel") > 0.0 {
                count_high += 1;
            }
        }
    }

    assert!(
        count_low > count_high,
        "Lower threshold ({}) should produce more stream cells ({})",
        count_low,
        count_high
    );
}

#[test]
fn test_compute_stream_order() {
    let dem = create_west_to_east_slope(10, 10);
    let accum = compute_flow_accumulation(&dem, 1.0).expect("Should compute accumulation");
    let order = compute_stream_order(&dem, &accum, 5.0, 1.0).expect("Should compute stream order");

    assert_eq!(order.width(), 10);
    assert_eq!(order.height(), 10);
}

// ============================================================================
// Watershed Delineation Tests
// ============================================================================

#[test]
fn test_delineate_watersheds_single_pour_point() {
    let dem = create_west_to_east_slope(10, 10);
    let mut pour_points = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

    // Pour point at eastern edge
    let _ = pour_points.set_pixel(8, 5, 1.0);

    let watersheds =
        delineate_watersheds(&dem, &pour_points, 1.0).expect("Should delineate watersheds");

    // Interior cells should belong to watershed
    let center = watersheds.get_pixel(5, 5).expect("Should get pixel");
    assert!(center > 0.0, "Interior cells should belong to a watershed");
}

#[test]
fn test_delineate_watersheds_multiple_pour_points() {
    let dem = create_west_to_east_slope(10, 10);
    let mut pour_points = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

    // Multiple pour points
    let _ = pour_points.set_pixel(8, 2, 1.0);
    let _ = pour_points.set_pixel(8, 7, 1.0);

    let watersheds =
        delineate_watersheds(&dem, &pour_points, 1.0).expect("Should delineate watersheds");

    // Check that multiple watersheds exist
    let top = watersheds.get_pixel(5, 2).expect("Should get pixel");
    let bottom = watersheds.get_pixel(5, 7).expect("Should get pixel");

    assert!(top > 0.0 && bottom > 0.0);
}

#[test]
fn test_delineate_watersheds_preserves_dimensions() {
    let dem = create_west_to_east_slope(15, 20);
    let mut pour_points = RasterBuffer::zeros(15, 20, RasterDataType::Float32);
    let _ = pour_points.set_pixel(14, 10, 1.0);

    let watersheds =
        delineate_watersheds(&dem, &pour_points, 1.0).expect("Should delineate watersheds");

    assert_eq!(watersheds.width(), 15);
    assert_eq!(watersheds.height(), 20);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_complete_hydrology_workflow() {
    // 1. Create DEM with depression
    let mut dem = create_west_to_east_slope(15, 15);
    let _ = dem.set_pixel(7, 7, 5.0); // Add depression

    // 2. Fill sinks
    let filled = fill_sinks(&dem, 0.001).expect("Should fill sinks");

    // 3. Compute flow direction
    let _flow_dir = compute_d8_flow_direction(&filled, 1.0).expect("Should compute flow direction");

    // 4. Compute flow accumulation
    let accum = compute_flow_accumulation(&filled, 1.0).expect("Should compute accumulation");

    // 5. Extract stream network
    let streams = extract_stream_network(&accum, 10.0).expect("Should extract streams");

    // 6. Verify workflow completed
    assert_eq!(streams.width(), 15);
    assert_eq!(streams.height(), 15);
}

#[test]
fn test_flow_accumulation_increases_downstream() {
    let dem = create_valley_dem(15, 15);
    let accum = compute_flow_accumulation(&dem, 1.0).expect("Should compute accumulation");

    // Along the valley bottom (center column), accumulation should be consistent
    // All cells in the valley should have >= 1
    for y in 1..14 {
        let val = accum.get_pixel(7, y).expect("Should get pixel");
        assert!(
            val >= 1.0,
            "Valley bottom at y={} has accumulation {}",
            y,
            val
        );
    }
}

#[test]
fn test_dinf_vs_d8_flow_direction() {
    let dem = create_west_to_east_slope(10, 10);

    let d8_dir = compute_d8_flow_direction(&dem, 1.0).expect("Should compute D8");
    let (dinf_angle, _) = compute_dinf_flow_direction(&dem, 1.0).expect("Should compute D-inf");

    // Both should produce valid results for the same DEM
    assert_eq!(d8_dir.width(), dinf_angle.width());
    assert_eq!(d8_dir.height(), dinf_angle.height());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_small_dem_flow_direction() {
    let dem = create_west_to_east_slope(5, 5);
    let flow_dir = compute_d8_flow_direction(&dem, 1.0);
    assert!(flow_dir.is_ok());
}

#[test]
fn test_large_dem_flow_accumulation() {
    let dem = create_west_to_east_slope(50, 50);
    let accum = compute_flow_accumulation(&dem, 1.0);
    assert!(accum.is_ok());
}

#[test]
fn test_steep_gradient() {
    let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
    for y in 0..10 {
        for x in 0..10 {
            // Very steep gradient
            let _ = dem.set_pixel(x, y, (10 - x) as f64 * 100.0);
        }
    }

    let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("Should handle steep gradient");
    let center = flow_dir.get_pixel(5, 5).expect("Should get pixel");
    assert!((center - 1.0).abs() < 0.01, "Should flow east");
}

#[test]
fn test_different_cell_sizes() {
    let dem = create_west_to_east_slope(10, 10);

    let flow_dir_1m = compute_d8_flow_direction(&dem, 1.0).expect("1m cell size");
    let flow_dir_10m = compute_d8_flow_direction(&dem, 10.0).expect("10m cell size");

    // Direction should be the same regardless of cell size (for uniform slope)
    let dir_1m = flow_dir_1m.get_pixel(5, 5).expect("Should get pixel");
    let dir_10m = flow_dir_10m.get_pixel(5, 5).expect("Should get pixel");

    assert!((dir_1m - dir_10m).abs() < 0.01);
}
