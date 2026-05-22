//! Tests for the Voronoi diagram builder in `oxigdal-index`.

use oxigdal_index::{VoronoiPoint, build_voronoi, cell_areas, circumcenter, find_cell_containing};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn pts(coords: &[(f64, f64)]) -> Vec<VoronoiPoint> {
    coords.iter().map(|&(x, y)| VoronoiPoint { x, y }).collect()
}

// ---------------------------------------------------------------------------
// 1. Circumcenter tests
// ---------------------------------------------------------------------------

#[test]
fn test_circumcenter_equilateral_triangle_at_centroid() {
    // Equilateral triangle: (0,0), (2,0), (1, √3)
    // Circumcenter of an equilateral triangle is its centroid = (1, √3/3).
    let a = (0.0_f64, 0.0_f64);
    let b = (2.0, 0.0);
    let c = (1.0, 3.0_f64.sqrt());
    let cc = circumcenter(a, b, c).expect("should have a circumcenter");
    let expected_y = 1.0 / 3.0_f64.sqrt();
    assert!(
        (cc.0 - 1.0).abs() < 1e-9,
        "x mismatch: got {}, expected 1.0",
        cc.0
    );
    assert!(
        (cc.1 - expected_y).abs() < 1e-9,
        "y mismatch: got {}, expected {}",
        cc.1,
        expected_y
    );
}

#[test]
fn test_circumcenter_collinear_returns_none() {
    let a = (0.0_f64, 0.0_f64);
    let b = (1.0, 0.0);
    let c = (2.0, 0.0);
    assert!(
        circumcenter(a, b, c).is_none(),
        "collinear points must return None"
    );
}

// ---------------------------------------------------------------------------
// 2. Edge-case build_voronoi tests
// ---------------------------------------------------------------------------

#[test]
fn test_build_voronoi_empty_input_returns_empty() {
    let bbox = (-1.0, -1.0, 1.0, 1.0);
    let diagram = build_voronoi(&[], bbox).expect("should succeed");
    assert!(
        diagram.cells.is_empty(),
        "expected empty cells for empty input"
    );
}

#[test]
fn test_build_voronoi_single_point_yields_one_cell_covering_bbox() {
    let bbox = (0.0_f64, 0.0_f64, 4.0, 3.0);
    let bbox_area = 4.0 * 3.0;
    let diagram = build_voronoi(&pts(&[(2.0, 1.5)]), bbox).expect("should succeed");
    assert_eq!(diagram.cells.len(), 1);
    let areas = cell_areas(&diagram);
    assert!(
        (areas[0] - bbox_area).abs() < 1e-9,
        "single-cell area should equal bbox area; got {}",
        areas[0]
    );
}

#[test]
fn test_build_voronoi_two_points_yields_perpendicular_bisector() {
    // Two points (0,0) and (2,0), bbox (-1,-1,3,1) → 2 cells each ≈ half bbox area.
    let bbox = (-1.0_f64, -1.0, 3.0, 1.0);
    let bbox_area = 4.0 * 2.0; // width=4, height=2
    let diagram = build_voronoi(&pts(&[(0.0, 0.0), (2.0, 0.0)]), bbox).expect("two points");
    assert_eq!(diagram.cells.len(), 2);
    let areas = cell_areas(&diagram);
    let half = bbox_area / 2.0;
    assert!(
        (areas[0] - half).abs() < 1e-9,
        "cell 0 area should be half bbox; got {}",
        areas[0]
    );
    assert!(
        (areas[1] - half).abs() < 1e-9,
        "cell 1 area should be half bbox; got {}",
        areas[1]
    );
}

// ---------------------------------------------------------------------------
// 3. General case tests
// ---------------------------------------------------------------------------

#[test]
fn test_build_voronoi_four_corners_yields_four_non_empty_cells() {
    let bbox = (-0.5_f64, -0.5, 1.5, 1.5);
    let seeds = pts(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]);
    let diagram = build_voronoi(&seeds, bbox).expect("four corners");
    assert_eq!(diagram.cells.len(), 4);
    for (i, cell) in diagram.cells.iter().enumerate() {
        assert!(
            cell.vertices.len() >= 3,
            "cell {} should have at least 3 vertices; got {}",
            i,
            cell.vertices.len()
        );
        let area = cell_areas(&diagram)[i];
        assert!(
            area > 0.0,
            "cell {} should have positive area; got {}",
            i,
            area
        );
    }
}

#[test]
fn test_build_voronoi_grid_3x3_yields_9_cells() {
    let bbox = (-0.5_f64, -0.5, 2.5, 2.5);
    let mut seeds = Vec::new();
    for row in 0..3usize {
        for col in 0..3usize {
            seeds.push(VoronoiPoint {
                x: col as f64,
                y: row as f64,
            });
        }
    }
    let diagram = build_voronoi(&seeds, bbox).expect("3×3 grid");
    assert_eq!(diagram.cells.len(), 9, "expected 9 cells for 3×3 grid");
    for (i, cell) in diagram.cells.iter().enumerate() {
        assert!(
            cell.vertices.len() >= 3,
            "cell {} should have at least 3 vertices",
            i
        );
    }
}

#[test]
fn test_voronoi_cell_neighbors_symmetric() {
    // For a sufficiently interior point the neighbor relation should be symmetric.
    let bbox = (-2.0_f64, -2.0, 4.0, 4.0);
    let seeds = pts(&[
        (0.0, 0.0),
        (2.0, 0.0),
        (1.0, 2.0),
        (0.0, 2.0),
        (2.0, 2.0),
        (1.0, 0.0),
    ]);
    let diagram = build_voronoi(&seeds, bbox).expect("symmetric neighbor test");

    for (i, cell_i) in diagram.cells.iter().enumerate() {
        for &j in &cell_i.neighbors {
            assert!(j < diagram.cells.len(), "neighbor index {} out of range", j);
            let cell_j = &diagram.cells[j];
            assert!(
                cell_j.neighbors.contains(&i),
                "neighbor relation not symmetric: cell {} lists {} but not vice versa",
                i,
                j
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Query tests
// ---------------------------------------------------------------------------

#[test]
fn test_find_cell_containing_returns_correct_cell() {
    // 4 seeds on the axes; query near each seed.
    let bbox = (-3.0_f64, -3.0, 3.0, 3.0);
    let seeds = pts(&[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]);
    let diagram = build_voronoi(&seeds, bbox).expect("4-point diagram");

    // For each seed, a query point very close to the seed should land in its cell.
    for (i, seed) in seeds.iter().enumerate() {
        let query = (seed.x, seed.y);
        let found = find_cell_containing(&diagram, query);
        assert!(
            found == Some(i),
            "query at seed {} should land in cell {}; got {:?}",
            i,
            i,
            found
        );
    }
}

#[test]
fn test_find_cell_containing_outside_bbox_returns_none() {
    let bbox = (0.0_f64, 0.0, 1.0, 1.0);
    let seeds = pts(&[(0.5, 0.5)]);
    let diagram = build_voronoi(&seeds, bbox).expect("single seed");

    assert_eq!(
        find_cell_containing(&diagram, (-1.0, 0.5)),
        None,
        "query left of bbox"
    );
    assert_eq!(
        find_cell_containing(&diagram, (0.5, 2.0)),
        None,
        "query above bbox"
    );
    assert_eq!(
        find_cell_containing(&diagram, (2.0, 2.0)),
        None,
        "query outside corner"
    );
}

// ---------------------------------------------------------------------------
// 5. Area tests
// ---------------------------------------------------------------------------

#[test]
fn test_cell_areas_sum_equals_bbox_area_within_tolerance() {
    let bbox = (0.0_f64, 0.0, 10.0, 8.0);
    let bbox_area = 10.0 * 8.0;
    // Random-ish spread of points.
    let seeds = pts(&[
        (1.0, 1.0),
        (5.0, 1.0),
        (9.0, 1.0),
        (2.0, 4.0),
        (5.0, 4.0),
        (8.0, 4.0),
        (1.0, 7.0),
        (5.0, 7.0),
        (9.0, 7.0),
    ]);
    let diagram = build_voronoi(&seeds, bbox).expect("area sum test");
    let total: f64 = cell_areas(&diagram).iter().sum();
    let rel_error = (total - bbox_area).abs() / bbox_area;
    assert!(
        rel_error < 0.001,
        "total area {:.4} should be within 0.1% of bbox area {:.4}; rel_error={:.6}",
        total,
        bbox_area,
        rel_error
    );
}

// ---------------------------------------------------------------------------
// 6. Vertex containment test
// ---------------------------------------------------------------------------

#[test]
fn test_voronoi_cells_clipped_to_bbox() {
    let bbox = (-1.0_f64, -1.0, 5.0, 5.0);
    let (min_x, min_y, max_x, max_y) = bbox;
    let seeds = pts(&[
        (0.0, 0.0),
        (2.0, 0.0),
        (4.0, 0.0),
        (0.0, 2.0),
        (2.0, 2.0),
        (4.0, 2.0),
        (0.0, 4.0),
        (2.0, 4.0),
        (4.0, 4.0),
    ]);
    let diagram = build_voronoi(&seeds, bbox).expect("clip test");
    for (i, cell) in diagram.cells.iter().enumerate() {
        for &(vx, vy) in &cell.vertices {
            assert!(
                vx >= min_x - 1e-9 && vx <= max_x + 1e-9,
                "cell {} vertex x={} outside bbox [{}, {}]",
                i,
                vx,
                min_x,
                max_x
            );
            assert!(
                vy >= min_y - 1e-9 && vy <= max_y + 1e-9,
                "cell {} vertex y={} outside bbox [{}, {}]",
                i,
                vy,
                min_y,
                max_y
            );
        }
    }
}
