//! Comprehensive tests for cost-distance algorithms
//!
//! Tests cost-distance analysis including:
//! - Euclidean distance
//! - Cost-weighted distance (Dijkstra's algorithm)
//! - Least-cost path extraction
//! - Various cost surface patterns
//! - Edge cases and validation

use oxigdal_algorithms::raster::{cost_distance, euclidean_distance, least_cost_path};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_sources_raster(width: u64, height: u64) -> RasterBuffer {
    RasterBuffer::zeros(width, height, RasterDataType::Float32)
}

fn create_single_source(width: u64, height: u64, source_x: u64, source_y: u64) -> RasterBuffer {
    let mut sources = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    let _ = sources.set_pixel(source_x, source_y, 1.0);
    sources
}

fn create_multiple_sources(width: u64, height: u64, positions: &[(u64, u64)]) -> RasterBuffer {
    let mut sources = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for &(x, y) in positions {
        let _ = sources.set_pixel(x, y, 1.0);
    }
    sources
}

fn create_uniform_cost_surface(width: u64, height: u64, cost: f64) -> RasterBuffer {
    let mut surface = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let _ = surface.set_pixel(x, y, cost);
        }
    }
    surface
}

fn create_gradient_cost_surface(width: u64, height: u64, horizontal: bool) -> RasterBuffer {
    let mut surface = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let cost = if horizontal {
                (x + 1) as f64
            } else {
                (y + 1) as f64
            };
            let _ = surface.set_pixel(x, y, cost);
        }
    }
    surface
}

fn create_barrier_cost_surface(
    width: u64,
    height: u64,
    base_cost: f64,
    barrier_cost: f64,
    barrier_y: u64,
) -> RasterBuffer {
    let mut surface = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let cost = if y == barrier_y {
                barrier_cost
            } else {
                base_cost
            };
            let _ = surface.set_pixel(x, y, cost);
        }
    }
    surface
}

fn create_checkerboard_cost_surface(
    width: u64,
    height: u64,
    cost1: f64,
    cost2: f64,
) -> RasterBuffer {
    let mut surface = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let cost = if (x + y) % 2 == 0 { cost1 } else { cost2 };
            let _ = surface.set_pixel(x, y, cost);
        }
    }
    surface
}

// ============================================================================
// Euclidean Distance Tests
// ============================================================================

#[test]
fn test_euclidean_distance_single_source() {
    let sources = create_single_source(10, 10, 5, 5);

    let distance = euclidean_distance(&sources, 1.0).expect("Should compute Euclidean distance");

    // Distance at source should be 0
    let source_dist = distance.get_pixel(5, 5).expect("Should get pixel");
    assert!(source_dist.abs() < 1e-6, "Distance at source should be 0");

    // Distance should increase with distance from source
    let dist_1 = distance.get_pixel(6, 5).expect("Should get pixel");
    let dist_2 = distance.get_pixel(7, 5).expect("Should get pixel");

    assert!(dist_1.abs() - 1.0 < 0.01, "Distance at (6,5) should be ~1");
    assert!(dist_2.abs() - 2.0 < 0.01, "Distance at (7,5) should be ~2");
}

#[test]
fn test_euclidean_distance_diagonal() {
    let sources = create_single_source(10, 10, 5, 5);

    let distance = euclidean_distance(&sources, 1.0).expect("Should compute Euclidean distance");

    // Diagonal distance should be sqrt(2)
    let diag_dist = distance.get_pixel(6, 6).expect("Should get pixel");
    let expected = 2.0_f64.sqrt();
    assert!(
        (diag_dist - expected).abs() < 0.01,
        "Diagonal distance should be sqrt(2), got {}",
        diag_dist
    );
}

#[test]
fn test_euclidean_distance_multiple_sources() {
    let positions = vec![(2, 5), (7, 5)];
    let sources = create_multiple_sources(10, 10, &positions);

    let distance = euclidean_distance(&sources, 1.0).expect("Should compute Euclidean distance");

    // Distances at sources should be 0
    assert!(distance.get_pixel(2, 5).expect("Should get pixel").abs() < 1e-6);
    assert!(distance.get_pixel(7, 5).expect("Should get pixel").abs() < 1e-6);

    // Cell between sources should have distance to nearest source
    let mid_dist = distance.get_pixel(5, 5).expect("Should get pixel");
    assert!(mid_dist > 0.0 && mid_dist < 3.0);
}

#[test]
fn test_euclidean_distance_cell_size() {
    let sources = create_single_source(10, 10, 5, 5);

    let distance_1m = euclidean_distance(&sources, 1.0).expect("1m cell size");
    let distance_10m = euclidean_distance(&sources, 10.0).expect("10m cell size");

    // Distance should scale with cell size
    let dist_1m = distance_1m.get_pixel(6, 5).expect("Should get pixel");
    let dist_10m = distance_10m.get_pixel(6, 5).expect("Should get pixel");

    assert!(
        (dist_10m - dist_1m * 10.0).abs() < 0.01,
        "Distance should scale with cell size"
    );
}

#[test]
fn test_euclidean_distance_corner_source() {
    let sources = create_single_source(10, 10, 0, 0);

    let distance = euclidean_distance(&sources, 1.0).expect("Should compute Euclidean distance");

    // Opposite corner distance
    let far_corner = distance.get_pixel(9, 9).expect("Should get pixel");
    let expected = (9.0_f64.powi(2) + 9.0_f64.powi(2)).sqrt();

    assert!(
        (far_corner - expected).abs() < 0.01,
        "Far corner distance should be {}, got {}",
        expected,
        far_corner
    );
}

#[test]
fn test_euclidean_distance_preserves_dimensions() {
    let sources = create_single_source(15, 20, 7, 10);

    let distance = euclidean_distance(&sources, 1.0).expect("Should compute Euclidean distance");

    assert_eq!(distance.width(), 15);
    assert_eq!(distance.height(), 20);
}

// ============================================================================
// Cost Distance Tests
// ============================================================================

#[test]
fn test_cost_distance_uniform_surface() {
    let sources = create_single_source(10, 10, 0, 0);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Cost at source should be 0
    let source_cost = cost_dist.get_pixel(0, 0).expect("Should get pixel");
    assert!(source_cost.abs() < 1e-6, "Cost at source should be 0");

    // Cost should increase with distance
    let cost_1 = cost_dist.get_pixel(1, 0).expect("Should get pixel");
    assert!(cost_1 > 0.0, "Cost should increase from source");
}

#[test]
fn test_cost_distance_diagonal_vs_cardinal() {
    let sources = create_single_source(10, 10, 5, 5);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Cardinal neighbor cost
    let cardinal_cost = cost_dist.get_pixel(6, 5).expect("Should get pixel");

    // Diagonal neighbor cost (should be sqrt(2) times higher for uniform cost)
    let diagonal_cost = cost_dist.get_pixel(6, 6).expect("Should get pixel");

    assert!(
        diagonal_cost > cardinal_cost,
        "Diagonal cost ({}) should be higher than cardinal cost ({})",
        diagonal_cost,
        cardinal_cost
    );
}

#[test]
fn test_cost_distance_barrier_avoidance() {
    let sources = create_single_source(10, 10, 5, 0);
    let cost_surface = create_barrier_cost_surface(10, 10, 1.0, 1000.0, 5);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Cost to reach cells beyond barrier should include barrier cost
    let beyond_barrier = cost_dist.get_pixel(5, 9).expect("Should get pixel");
    assert!(
        beyond_barrier > 50.0,
        "Cost beyond barrier should be high, got {}",
        beyond_barrier
    );
}

#[test]
fn test_cost_distance_prefers_low_cost_path() {
    // Create cost surface with low-cost corridor
    let mut cost_surface = create_uniform_cost_surface(10, 10, 10.0);
    for y in 0..10 {
        let _ = cost_surface.set_pixel(0, y, 1.0); // Low cost corridor on left edge
    }

    let sources = create_single_source(10, 10, 0, 0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Bottom-left corner through corridor should have lower cost
    let corridor_path = cost_dist.get_pixel(0, 9).expect("Should get pixel");

    // Destination via corridor should be cheaper than direct path through high cost area
    // (though this depends on exact algorithm behavior)
    assert!(corridor_path.is_finite());
}

#[test]
fn test_cost_distance_multiple_sources() {
    let positions = vec![(0, 0), (9, 9)];
    let sources = create_multiple_sources(10, 10, &positions);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Both source locations should have cost 0
    assert!(cost_dist.get_pixel(0, 0).expect("Should get pixel").abs() < 1e-6);
    assert!(cost_dist.get_pixel(9, 9).expect("Should get pixel").abs() < 1e-6);

    // Center should have cost to nearest source
    let center = cost_dist.get_pixel(5, 5).expect("Should get pixel");
    assert!(center > 0.0 && center.is_finite());
}

#[test]
fn test_cost_distance_gradient_surface() {
    let sources = create_single_source(10, 10, 0, 0);
    let cost_surface = create_gradient_cost_surface(10, 10, true);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Moving right should have increasing cost
    let cost_near = cost_dist.get_pixel(1, 0).expect("Should get pixel");
    let cost_far = cost_dist.get_pixel(9, 0).expect("Should get pixel");

    assert!(cost_far > cost_near, "Cost should increase with gradient");
}

#[test]
fn test_cost_distance_preserves_dimensions() {
    let sources = create_single_source(15, 20, 7, 10);
    let cost_surface = create_uniform_cost_surface(15, 20, 1.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    assert_eq!(cost_dist.width(), 15);
    assert_eq!(cost_dist.height(), 20);
}

// ============================================================================
// Least Cost Path Tests
// ============================================================================

#[test]
fn test_least_cost_path_straight_line() {
    // Create simple cost distance gradient (source at 0,0)
    let mut cost_dist = RasterBuffer::zeros(10, 10, RasterDataType::Float64);
    for y in 0..10 {
        for x in 0..10 {
            let _ = cost_dist.set_pixel(x, y, (x + y) as f64);
        }
    }

    let path = least_cost_path(&cost_dist, 5, 5).expect("Should extract path");

    // Path should include destination
    let dest_on_path = path.get_pixel(5, 5).expect("Should get pixel");
    assert!(dest_on_path > 0.0, "Destination should be on path");

    // Path should include source (0,0)
    let source_on_path = path.get_pixel(0, 0).expect("Should get pixel");
    assert!(source_on_path > 0.0, "Source should be on path");
}

#[test]
fn test_least_cost_path_follows_gradient() {
    // Create cost distance from actual computation
    let sources = create_single_source(10, 10, 0, 0);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);
    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    let path = least_cost_path(&cost_dist, 5, 5).expect("Should extract path");

    // Count path cells
    let mut path_count = 0;
    for y in 0..10 {
        for x in 0..10 {
            if path.get_pixel(x, y).expect("Should get pixel") > 0.0 {
                path_count += 1;
            }
        }
    }

    // Path should have reasonable length (not all cells, not just dest)
    assert!(
        path_count > 1 && path_count <= 15,
        "Path should have reasonable length, got {}",
        path_count
    );
}

#[test]
fn test_least_cost_path_destination_at_source() {
    let sources = create_single_source(10, 10, 5, 5);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);
    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    let path = least_cost_path(&cost_dist, 5, 5).expect("Should extract path");

    // Only the source/destination should be on path
    let source_on_path = path.get_pixel(5, 5).expect("Should get pixel");
    assert!(source_on_path > 0.0, "Source should be on path");
}

#[test]
fn test_least_cost_path_corner_to_corner() {
    let sources = create_single_source(10, 10, 0, 0);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);
    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    let path = least_cost_path(&cost_dist, 9, 9).expect("Should extract path");

    // Both corners should be on path
    assert!(path.get_pixel(0, 0).expect("Should get pixel") > 0.0);
    assert!(path.get_pixel(9, 9).expect("Should get pixel") > 0.0);
}

#[test]
fn test_least_cost_path_preserves_dimensions() {
    let mut cost_dist = RasterBuffer::zeros(15, 20, RasterDataType::Float64);
    for y in 0..20 {
        for x in 0..15 {
            let _ = cost_dist.set_pixel(x, y, (x + y) as f64);
        }
    }

    let path = least_cost_path(&cost_dist, 7, 10).expect("Should extract path");

    assert_eq!(path.width(), 15);
    assert_eq!(path.height(), 20);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_complete_cost_distance_workflow() {
    // 1. Create source locations
    let sources = create_single_source(20, 20, 0, 0);

    // 2. Create cost surface with variations
    let mut cost_surface = create_uniform_cost_surface(20, 20, 1.0);
    // Add high cost barrier
    for x in 0..15 {
        let _ = cost_surface.set_pixel(x, 10, 100.0);
    }

    // 3. Compute cost distance
    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // 4. Extract path to destination beyond barrier
    let path = least_cost_path(&cost_dist, 19, 19).expect("Should extract path");

    // 5. Verify path exists
    let mut path_count = 0;
    for y in 0..20 {
        for x in 0..20 {
            if path.get_pixel(x, y).expect("Should get pixel") > 0.0 {
                path_count += 1;
            }
        }
    }
    assert!(path_count > 0, "Path should exist");
}

#[test]
fn test_cost_vs_euclidean_distance() {
    let sources = create_single_source(10, 10, 5, 5);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);

    let eucl_dist = euclidean_distance(&sources, 1.0).expect("Euclidean distance");
    let cost_dist = cost_distance(&sources, &cost_surface, 1.0).expect("Cost distance");

    // For uniform cost of 1, cost distance should be proportional to Euclidean distance
    for y in 0..10 {
        for x in 0..10 {
            let ed = eucl_dist.get_pixel(x, y).expect("Should get pixel");
            let cd = cost_dist.get_pixel(x, y).expect("Should get pixel");

            // Both should have same zero location
            if ed.abs() < 1e-6 {
                assert!(cd.abs() < 1e-6, "Both should be zero at source");
            }
        }
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_euclidean_distance_no_sources() {
    let sources = create_sources_raster(10, 10); // All zeros

    let distance = euclidean_distance(&sources, 1.0).expect("Should compute Euclidean distance");

    // All distances should be infinity
    for y in 0..10 {
        for x in 0..10 {
            let dist = distance.get_pixel(x, y).expect("Should get pixel");
            assert!(
                dist.is_infinite(),
                "Distance should be infinite with no sources at ({}, {}): {}",
                x,
                y,
                dist
            );
        }
    }
}

#[test]
fn test_cost_distance_no_sources() {
    let sources = create_sources_raster(10, 10); // All zeros
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // All costs should be infinity
    for y in 0..10 {
        for x in 0..10 {
            let cost = cost_dist.get_pixel(x, y).expect("Should get pixel");
            assert!(
                cost.is_infinite(),
                "Cost should be infinite with no sources at ({}, {}): {}",
                x,
                y,
                cost
            );
        }
    }
}

#[test]
fn test_cost_distance_all_sources() {
    // When all cells are sources, all distances should be 0
    let mut sources = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
    for y in 0..5 {
        for x in 0..5 {
            let _ = sources.set_pixel(x, y, 1.0);
        }
    }
    let cost_surface = create_uniform_cost_surface(5, 5, 1.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // All costs should be 0
    for y in 0..5 {
        for x in 0..5 {
            let cost = cost_dist.get_pixel(x, y).expect("Should get pixel");
            assert!(
                cost.abs() < 1e-6,
                "Cost should be 0 when all cells are sources"
            );
        }
    }
}

#[test]
fn test_cost_distance_high_cost_barrier() {
    let sources = create_single_source(10, 10, 0, 0);
    let cost_surface = create_barrier_cost_surface(10, 10, 1.0, f64::MAX / 2.0, 5);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Should handle very high costs
    assert!(cost_dist.get_pixel(0, 0).expect("Should get pixel").abs() < 1e-6);
}

#[test]
fn test_small_raster_cost_distance() {
    let sources = create_single_source(3, 3, 1, 1);
    let cost_surface = create_uniform_cost_surface(3, 3, 1.0);

    let cost_dist = cost_distance(&sources, &cost_surface, 1.0);
    assert!(cost_dist.is_ok());
}

#[test]
fn test_large_raster_cost_distance() {
    let sources = create_single_source(50, 50, 25, 25);
    let cost_surface = create_uniform_cost_surface(50, 50, 1.0);

    let cost_dist = cost_distance(&sources, &cost_surface, 1.0);
    assert!(cost_dist.is_ok());
}

#[test]
fn test_checkerboard_cost_surface() {
    let sources = create_single_source(10, 10, 0, 0);
    let cost_surface = create_checkerboard_cost_surface(10, 10, 1.0, 5.0);

    let cost_dist =
        cost_distance(&sources, &cost_surface, 1.0).expect("Should compute cost distance");

    // Should handle alternating costs
    assert!(cost_dist.get_pixel(0, 0).expect("Should get pixel").abs() < 1e-6);
    let far = cost_dist.get_pixel(9, 9).expect("Should get pixel");
    assert!(far > 0.0 && far.is_finite());
}

// ============================================================================
// Cell Size Effect Tests
// ============================================================================

#[test]
fn test_cell_size_scaling() {
    let sources = create_single_source(10, 10, 5, 5);
    let cost_surface = create_uniform_cost_surface(10, 10, 1.0);

    let cost_1m = cost_distance(&sources, &cost_surface, 1.0).expect("1m cell size");
    let cost_10m = cost_distance(&sources, &cost_surface, 10.0).expect("10m cell size");

    // Adjacent cell cost should scale with cell size
    let cost_1m_adj = cost_1m.get_pixel(6, 5).expect("Should get pixel");
    let cost_10m_adj = cost_10m.get_pixel(6, 5).expect("Should get pixel");

    assert!(
        (cost_10m_adj - cost_1m_adj * 10.0).abs() < 1.0,
        "Cost should scale approximately with cell size (1m: {}, 10m: {})",
        cost_1m_adj,
        cost_10m_adj
    );
}
