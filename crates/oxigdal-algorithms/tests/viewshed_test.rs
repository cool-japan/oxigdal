//! Comprehensive tests for viewshed analysis algorithms
//!
//! Tests visibility computation including:
//! - Single observer viewshed
//! - Cumulative viewshed from multiple observers
//! - Line of sight calculations
//! - Observer and target height effects
//! - Maximum distance limiting
//! - Terrain features (obstacles, valleys)
//! - Edge cases

use oxigdal_algorithms::raster::{compute_cumulative_viewshed, compute_viewshed};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_flat_dem(width: u64, height: u64, elevation: f64) -> RasterBuffer {
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn create_dem_with_obstacle(
    width: u64,
    height: u64,
    base_elevation: f64,
    obstacle_x: u64,
    obstacle_y: u64,
    obstacle_height: f64,
) -> RasterBuffer {
    let mut dem = create_flat_dem(width, height, base_elevation);
    let _ = dem.set_pixel(obstacle_x, obstacle_y, base_elevation + obstacle_height);
    dem
}

fn create_dem_with_wall(
    width: u64,
    height: u64,
    base_elevation: f64,
    wall_y: u64,
    wall_height: f64,
) -> RasterBuffer {
    let mut dem = create_flat_dem(width, height, base_elevation);
    for x in 0..width {
        let _ = dem.set_pixel(x, wall_y, base_elevation + wall_height);
    }
    dem
}

fn create_sloped_dem(width: u64, height: u64, slope: f64) -> RasterBuffer {
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let elevation = y as f64 * slope;
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn create_valley_dem(width: u64, height: u64, depth: f64) -> RasterBuffer {
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    let cy = height as f64 / 2.0;

    for y in 0..height {
        for x in 0..width {
            let dy = (y as f64 - cy).abs();
            let elevation = dy / cy * depth;
            let _ = dem.set_pixel(x, y, elevation);
        }
    }
    dem
}

fn count_visible_cells(viewshed: &RasterBuffer) -> u64 {
    let mut count = 0;
    for y in 0..viewshed.height() {
        for x in 0..viewshed.width() {
            if viewshed.get_pixel(x, y).unwrap_or(0.0) > 0.0 {
                count += 1;
            }
        }
    }
    count
}

// ============================================================================
// Flat Terrain Tests
// ============================================================================

#[test]
fn test_viewshed_flat_terrain_all_visible() {
    let dem = create_flat_dem(10, 10, 0.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // All cells should be visible on flat terrain from elevated observer
    for y in 0..10 {
        for x in 0..10 {
            let val = viewshed.get_pixel(x, y).expect("Should get pixel");
            assert!(
                val > 0.0,
                "Cell ({}, {}) should be visible on flat terrain",
                x,
                y
            );
        }
    }
}

#[test]
fn test_viewshed_flat_terrain_observer_at_ground() {
    let dem = create_flat_dem(10, 10, 0.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 0.1, 0.0, None, 1.0).expect("Should compute viewshed");

    // Observer location should always be visible
    let observer_visible = viewshed.get_pixel(5, 5).expect("Should get pixel");
    assert!(
        observer_visible > 0.0,
        "Observer location should be visible"
    );
}

#[test]
fn test_viewshed_preserves_dimensions() {
    let dem = create_flat_dem(20, 15, 0.0);

    let viewshed =
        compute_viewshed(&dem, 10, 7, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    assert_eq!(viewshed.width(), 20);
    assert_eq!(viewshed.height(), 15);
}

// ============================================================================
// Obstacle Tests
// ============================================================================

#[test]
fn test_viewshed_single_obstacle_blocks_view() {
    // Create flat DEM with single obstacle
    let dem = create_dem_with_obstacle(10, 10, 0.0, 5, 6, 20.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 1.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Observer should be visible
    assert!(viewshed.get_pixel(5, 5).expect("Should get pixel") > 0.0);

    // Cell behind obstacle should be blocked
    let behind_obstacle = viewshed.get_pixel(5, 8).expect("Should get pixel");
    assert!(
        behind_obstacle == 0.0,
        "Cell behind obstacle should be blocked, got {}",
        behind_obstacle
    );
}

#[test]
fn test_viewshed_wall_blocks_view() {
    // Create DEM with wall across the middle
    let dem = create_dem_with_wall(10, 10, 0.0, 6, 20.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 1.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Cells beyond wall should be blocked
    let beyond_wall = viewshed.get_pixel(5, 8).expect("Should get pixel");
    assert!(
        beyond_wall == 0.0,
        "Cell beyond wall should be blocked, got {}",
        beyond_wall
    );

    // Cells before wall should be visible
    let before_wall = viewshed.get_pixel(5, 3).expect("Should get pixel");
    assert!(before_wall > 0.0, "Cell before wall should be visible");
}

#[test]
fn test_viewshed_high_observer_sees_over_obstacle() {
    let dem = create_dem_with_obstacle(10, 10, 0.0, 5, 6, 10.0);

    // High observer (above obstacle)
    let viewshed_high =
        compute_viewshed(&dem, 5, 5, 20.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Low observer
    let viewshed_low =
        compute_viewshed(&dem, 5, 5, 1.0, 0.0, None, 1.0).expect("Should compute viewshed");

    let high_visibility = count_visible_cells(&viewshed_high);
    let low_visibility = count_visible_cells(&viewshed_low);

    assert!(
        high_visibility >= low_visibility,
        "Higher observer should see at least as many cells (high: {}, low: {})",
        high_visibility,
        low_visibility
    );
}

// ============================================================================
// Observer Height Tests
// ============================================================================

#[test]
fn test_viewshed_observer_height_increases_visibility() {
    let dem = create_sloped_dem(15, 15, 2.0);

    let viewshed_low =
        compute_viewshed(&dem, 7, 7, 1.0, 0.0, None, 1.0).expect("Low observer viewshed");
    let viewshed_high =
        compute_viewshed(&dem, 7, 7, 50.0, 0.0, None, 1.0).expect("High observer viewshed");

    let low_visibility = count_visible_cells(&viewshed_low);
    let high_visibility = count_visible_cells(&viewshed_high);

    assert!(
        high_visibility >= low_visibility,
        "Higher observer height should see more (low: {}, high: {})",
        low_visibility,
        high_visibility
    );
}

#[test]
fn test_viewshed_zero_observer_height() {
    let dem = create_flat_dem(10, 10, 0.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 0.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Observer location should be visible
    assert!(viewshed.get_pixel(5, 5).expect("Should get pixel") > 0.0);
}

// ============================================================================
// Target Height Tests
// ============================================================================

#[test]
fn test_viewshed_target_height_increases_visibility() {
    let dem = create_dem_with_wall(15, 15, 0.0, 7, 10.0);

    // No target height
    let viewshed_low_target =
        compute_viewshed(&dem, 5, 5, 5.0, 0.0, None, 1.0).expect("Low target viewshed");

    // High target height (like a tall building)
    let viewshed_high_target =
        compute_viewshed(&dem, 5, 5, 5.0, 15.0, None, 1.0).expect("High target viewshed");

    let low_target_visibility = count_visible_cells(&viewshed_low_target);
    let high_target_visibility = count_visible_cells(&viewshed_high_target);

    assert!(
        high_target_visibility >= low_target_visibility,
        "Higher targets should be more visible (low: {}, high: {})",
        low_target_visibility,
        high_target_visibility
    );
}

// ============================================================================
// Maximum Distance Tests
// ============================================================================

#[test]
fn test_viewshed_max_distance_limits_visibility() {
    let dem = create_flat_dem(20, 20, 0.0);

    let viewshed_limited = compute_viewshed(&dem, 10, 10, 10.0, 0.0, Some(5.0), 1.0)
        .expect("Limited distance viewshed");
    let viewshed_unlimited =
        compute_viewshed(&dem, 10, 10, 10.0, 0.0, None, 1.0).expect("Unlimited distance viewshed");

    let limited_visibility = count_visible_cells(&viewshed_limited);
    let unlimited_visibility = count_visible_cells(&viewshed_unlimited);

    assert!(
        unlimited_visibility >= limited_visibility,
        "Unlimited should see more (limited: {}, unlimited: {})",
        limited_visibility,
        unlimited_visibility
    );
}

#[test]
fn test_viewshed_cells_beyond_max_distance_invisible() {
    let dem = create_flat_dem(20, 20, 0.0);

    let viewshed =
        compute_viewshed(&dem, 10, 10, 10.0, 0.0, Some(3.0), 1.0).expect("Should compute viewshed");

    // Cell at distance > 3 should be invisible
    // Distance from (10,10) to (0,0) is sqrt(200) ~ 14.14
    let far_cell = viewshed.get_pixel(0, 0).expect("Should get pixel");
    assert!(
        far_cell == 0.0,
        "Cell beyond max distance should be invisible, got {}",
        far_cell
    );

    // Cell within distance should be visible
    let near_cell = viewshed.get_pixel(10, 11).expect("Should get pixel");
    assert!(
        near_cell > 0.0,
        "Cell within max distance should be visible"
    );
}

#[test]
fn test_viewshed_zero_max_distance() {
    let dem = create_flat_dem(10, 10, 0.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, Some(0.0), 1.0).expect("Should compute viewshed");

    // Only observer should be visible
    let observer_visible = viewshed.get_pixel(5, 5).expect("Should get pixel");
    assert!(observer_visible > 0.0, "Observer should be visible");

    // All other cells should be invisible (due to 0 distance)
    let other_visible = count_visible_cells(&viewshed);
    // Account for the observer cell
    assert!(
        other_visible <= 1,
        "Only observer should be visible with 0 max distance"
    );
}

// ============================================================================
// Cell Size Tests
// ============================================================================

#[test]
fn test_viewshed_different_cell_sizes() {
    let dem = create_flat_dem(10, 10, 0.0);

    // Same DEM with different cell sizes
    let viewshed_1m =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, Some(50.0), 1.0).expect("1m cell size viewshed");
    let viewshed_10m =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, Some(50.0), 10.0).expect("10m cell size viewshed");

    // Both should be valid
    assert_eq!(viewshed_1m.width(), 10);
    assert_eq!(viewshed_10m.width(), 10);
}

#[test]
fn test_viewshed_cell_size_affects_distance() {
    let dem = create_flat_dem(10, 10, 0.0);

    // Max distance in map units, cell size affects how this maps to pixels
    let viewshed_small =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, Some(3.0), 1.0).expect("Small cell size viewshed");
    let viewshed_large = compute_viewshed(&dem, 5, 5, 10.0, 0.0, Some(30.0), 10.0)
        .expect("Large cell size viewshed");

    // Both should produce valid results
    assert!(count_visible_cells(&viewshed_small) > 0);
    assert!(count_visible_cells(&viewshed_large) > 0);
}

// ============================================================================
// Cumulative Viewshed Tests
// ============================================================================

#[test]
fn test_cumulative_viewshed_single_observer() {
    let dem = create_flat_dem(10, 10, 0.0);
    let observers = vec![(5, 5, 10.0)];

    let cumulative = compute_cumulative_viewshed(&dem, &observers, 0.0, None, 1.0)
        .expect("Should compute cumulative viewshed");

    // All cells should have value 1 (seen by 1 observer)
    for y in 0..10 {
        for x in 0..10 {
            let val = cumulative.get_pixel(x, y).expect("Should get pixel");
            assert!(
                (val - 1.0).abs() < 0.01,
                "Cell ({}, {}) should be seen by 1 observer, got {}",
                x,
                y,
                val
            );
        }
    }
}

#[test]
fn test_cumulative_viewshed_multiple_observers() {
    let dem = create_flat_dem(10, 10, 0.0);
    let observers = vec![(2, 2, 10.0), (7, 7, 10.0)];

    let cumulative = compute_cumulative_viewshed(&dem, &observers, 0.0, None, 1.0)
        .expect("Should compute cumulative viewshed");

    // Center should be visible from both observers
    let center = cumulative.get_pixel(5, 5).expect("Should get pixel");
    assert!(
        center >= 2.0,
        "Center should be visible from both observers, got {}",
        center
    );
}

#[test]
fn test_cumulative_viewshed_with_obstacles() {
    // Create DEM with wall in the middle
    let dem = create_dem_with_wall(10, 10, 0.0, 5, 20.0);
    let observers = vec![(2, 2, 5.0), (7, 7, 5.0)];

    let cumulative = compute_cumulative_viewshed(&dem, &observers, 0.0, None, 1.0)
        .expect("Should compute cumulative viewshed");

    // Cells should have varying visibility
    let near_first = cumulative.get_pixel(2, 3).expect("Should get pixel");
    let near_second = cumulative.get_pixel(7, 8).expect("Should get pixel");

    // Each observer should see cells near them
    assert!(near_first > 0.0);
    assert!(near_second > 0.0);
}

#[test]
fn test_cumulative_viewshed_varying_heights() {
    let dem = create_flat_dem(10, 10, 0.0);
    let observers = vec![(2, 2, 5.0), (5, 5, 10.0), (7, 7, 15.0)];

    let cumulative = compute_cumulative_viewshed(&dem, &observers, 0.0, None, 1.0)
        .expect("Should compute cumulative viewshed");

    // All cells should be visible from all 3 observers on flat terrain
    for y in 0..10 {
        for x in 0..10 {
            let val = cumulative.get_pixel(x, y).expect("Should get pixel");
            assert!(
                val >= 3.0,
                "Cell ({}, {}) should be seen by all 3 observers on flat terrain, got {}",
                x,
                y,
                val
            );
        }
    }
}

#[test]
fn test_cumulative_viewshed_empty_observers() {
    let dem = create_flat_dem(10, 10, 0.0);
    let observers: Vec<(u64, u64, f64)> = vec![];

    let cumulative = compute_cumulative_viewshed(&dem, &observers, 0.0, None, 1.0)
        .expect("Should compute cumulative viewshed");

    // All cells should have value 0
    for y in 0..10 {
        for x in 0..10 {
            let val = cumulative.get_pixel(x, y).expect("Should get pixel");
            assert!(
                val == 0.0,
                "All cells should be invisible with no observers"
            );
        }
    }
}

// ============================================================================
// Terrain Feature Tests
// ============================================================================

#[test]
fn test_viewshed_valley() {
    let dem = create_valley_dem(15, 15, 20.0);

    // Observer at one end of valley
    let viewshed =
        compute_viewshed(&dem, 7, 1, 2.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Should see along valley bottom
    let valley_cell = viewshed.get_pixel(7, 7).expect("Should get pixel");
    assert!(valley_cell > 0.0, "Should see along valley bottom");
}

#[test]
fn test_viewshed_ridge() {
    // Create ridge running horizontally
    let mut dem = create_flat_dem(15, 15, 10.0);
    for x in 0..15 {
        let _ = dem.set_pixel(x, 7, 30.0); // Ridge at y=7
    }

    let viewshed =
        compute_viewshed(&dem, 7, 3, 5.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Cells behind ridge should be blocked
    let behind_ridge = viewshed.get_pixel(7, 12).expect("Should get pixel");
    assert!(
        behind_ridge == 0.0,
        "Cell behind ridge should be blocked, got {}",
        behind_ridge
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_viewshed_observer_at_corner() {
    let dem = create_flat_dem(10, 10, 0.0);

    let viewshed =
        compute_viewshed(&dem, 0, 0, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Observer at corner should still see all cells
    assert!(viewshed.get_pixel(0, 0).expect("Should get pixel") > 0.0);
    assert!(viewshed.get_pixel(9, 9).expect("Should get pixel") > 0.0);
}

#[test]
fn test_viewshed_observer_at_edge() {
    let dem = create_flat_dem(10, 10, 0.0);

    let viewshed =
        compute_viewshed(&dem, 0, 5, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Observer at edge should work correctly
    assert!(viewshed.get_pixel(0, 5).expect("Should get pixel") > 0.0);
}

#[test]
fn test_viewshed_small_dem() {
    let dem = create_flat_dem(3, 3, 0.0);

    let viewshed =
        compute_viewshed(&dem, 1, 1, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    assert_eq!(viewshed.width(), 3);
    assert_eq!(viewshed.height(), 3);
}

#[test]
fn test_viewshed_large_dem() {
    let dem = create_flat_dem(50, 50, 0.0);

    let viewshed =
        compute_viewshed(&dem, 25, 25, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    assert_eq!(viewshed.width(), 50);
    assert_eq!(viewshed.height(), 50);
}

#[test]
fn test_viewshed_negative_elevations() {
    let dem = create_flat_dem(10, 10, -100.0);

    let viewshed =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Should still work with negative elevations
    assert!(viewshed.get_pixel(5, 5).expect("Should get pixel") > 0.0);
}

#[test]
fn test_viewshed_high_elevations() {
    let dem = create_flat_dem(10, 10, 8000.0); // Everest height

    let viewshed =
        compute_viewshed(&dem, 5, 5, 10.0, 0.0, None, 1.0).expect("Should compute viewshed");

    // Should still work with high elevations
    assert!(viewshed.get_pixel(5, 5).expect("Should get pixel") > 0.0);
}
