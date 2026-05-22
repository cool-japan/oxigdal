//! Integration tests for the power diagram (weighted Voronoi) implementation.

use oxigdal_algorithms::{Coordinate, PowerDiagramOptions, WeightedPoint, power_diagram};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Shoelace formula — returns the (unsigned) area of a simple polygon.
fn polygon_area(polygon: &[Coordinate]) -> f64 {
    let n = polygon.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += polygon[i].x * polygon[j].y;
        sum -= polygon[j].x * polygon[i].y;
    }
    sum.abs() / 2.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_power_diagram_empty_input_returns_empty_diagram() {
    let result = power_diagram(&[], &PowerDiagramOptions::default()).expect("empty diagram ok");
    assert!(result.cells.is_empty());
}

#[test]
fn test_power_diagram_single_site_returns_full_bbox() {
    let points = vec![WeightedPoint::new(0.0, 0.0, 0.0)];
    let options = PowerDiagramOptions {
        bounding_box: Some((-1.0, -1.0, 1.0, 1.0)),
    };
    let diagram = power_diagram(&points, &options).expect("single site ok");
    assert_eq!(diagram.cells.len(), 1);
    assert!(!diagram.cells[0].is_empty);
    // The cell polygon must cover the four corners of the bounding box.
    assert!(
        diagram.cells[0].polygon.len() >= 4,
        "Expected ≥4 vertices, got {}",
        diagram.cells[0].polygon.len()
    );
}

#[test]
fn test_power_diagram_two_equal_weights_bisector_is_midpoint() {
    // Two sites at (−1, 0) and (1, 0) with equal weights → bisector at x = 0.
    let points = vec![
        WeightedPoint::new(-1.0, 0.0, 0.0),
        WeightedPoint::new(1.0, 0.0, 0.0),
    ];
    let options = PowerDiagramOptions {
        bounding_box: Some((-2.0, -2.0, 2.0, 2.0)),
    };
    let diagram = power_diagram(&points, &options).expect("two equal weights ok");
    assert_eq!(diagram.cells.len(), 2);

    let left_cell = &diagram.cells[0];
    let right_cell = &diagram.cells[1];
    assert!(!left_cell.is_empty, "Left cell must be non-empty");
    assert!(!right_cell.is_empty, "Right cell must be non-empty");

    // Left cell must be entirely at x ≤ 0 (bisector at x = 0).
    let max_x_left = left_cell
        .polygon
        .iter()
        .map(|c| c.x)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_x_left - 0.0).abs() < 1e-9,
        "Left cell max_x should be 0.0, got {max_x_left}"
    );

    // Right cell must be entirely at x ≥ 0.
    let min_x_right = right_cell
        .polygon
        .iter()
        .map(|c| c.x)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (min_x_right - 0.0).abs() < 1e-9,
        "Right cell min_x should be 0.0, got {min_x_right}"
    );
}

#[test]
fn test_power_diagram_unequal_weights_shifts_boundary() {
    // Site 0 at (0, 0) with weight 4.0 (heavy).
    // Site 1 at (2, 0) with weight 0.0 (light).
    // The heavier site should capture more area.
    let points = vec![
        WeightedPoint::new(0.0, 0.0, 4.0), // heavy
        WeightedPoint::new(2.0, 0.0, 0.0), // light
    ];
    let options = PowerDiagramOptions {
        bounding_box: Some((-3.0, -3.0, 5.0, 3.0)),
    };
    let diagram = power_diagram(&points, &options).expect("unequal weights ok");
    assert_eq!(diagram.cells.len(), 2);

    let area_0 = polygon_area(&diagram.cells[0].polygon);
    let area_1 = polygon_area(&diagram.cells[1].polygon);
    assert!(
        area_0 > area_1,
        "Heavy site cell ({area_0}) should be larger than light site cell ({area_1})"
    );
}

#[test]
fn test_power_diagram_heavy_site_dominates_to_empty_cell() {
    // A very heavy central site should drive nearby light sites to empty cells.
    let points = vec![
        WeightedPoint::new(0.0, 0.0, 1000.0), // very heavy
        WeightedPoint::new(0.1, 0.0, 0.0),    // very light — may vanish
        WeightedPoint::new(0.0, 0.1, 0.0),    // very light — may vanish
    ];
    let options = PowerDiagramOptions::default();
    let diagram = power_diagram(&points, &options).expect("should not error");
    assert_eq!(diagram.cells.len(), 3);
    // The heavy site's cell must be non-empty.
    assert!(
        !diagram.cells[0].is_empty,
        "Dominant site must have a non-empty cell"
    );
    // Light sites near the dominant site should be empty.
    assert!(
        diagram.cells[1].is_empty,
        "Light site adjacent to heavy should be empty"
    );
    assert!(
        diagram.cells[2].is_empty,
        "Light site adjacent to heavy should be empty"
    );
}

#[test]
fn test_power_diagram_bounding_box_clips_cells() {
    let points = vec![
        WeightedPoint::new(0.0, 0.0, 0.0),
        WeightedPoint::new(1.0, 0.0, 0.0),
    ];
    let bbox = (-0.5_f64, -0.5_f64, 1.5_f64, 0.5_f64);
    let options = PowerDiagramOptions {
        bounding_box: Some(bbox),
    };
    let diagram = power_diagram(&points, &options).expect("bbox clipping ok");

    // Every vertex of every cell must lie within the bounding box.
    for cell in &diagram.cells {
        for coord in &cell.polygon {
            assert!(
                coord.x >= bbox.0 - 1e-9 && coord.x <= bbox.2 + 1e-9,
                "x={} outside [{}, {}]",
                coord.x,
                bbox.0,
                bbox.2
            );
            assert!(
                coord.y >= bbox.1 - 1e-9 && coord.y <= bbox.3 + 1e-9,
                "y={} outside [{}, {}]",
                coord.y,
                bbox.1,
                bbox.3
            );
        }
    }
}

#[test]
fn test_weighted_bisector_perpendicular_for_equal_weights() {
    use oxigdal_algorithms::vector::voronoi::weighted_bisector;

    // Equal weights → radical axis is the perpendicular bisector.
    // Sites at (−1, 0) and (1, 0): midpoint is (0, 0), bisector is x = 0.
    let (a, b, c) = weighted_bisector(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    // The midpoint (0, 0) must be exactly on the bisector (a·0 + b·0 = c).
    assert!(
        (a * 0.0 + b * 0.0 - c).abs() < 1e-9,
        "Midpoint should lie on the bisector: a={a}, b={b}, c={c}"
    );

    // weighted_bisector returns (a,b,c) where a·x + b·y ≤ c is site i's region.
    // Site i at (−1, 0) must satisfy a·(−1)+b·0 ≤ c (inside site i's half-plane).
    assert!(
        -a + b * 0.0 <= c + 1e-9,
        "Site i should be inside its own half-plane (a·xi+b·yi ≤ c)"
    );

    // Site j at (1, 0) must be on the other side: a·1+b·0 ≥ c.
    assert!(
        a * 1.0 + b * 0.0 >= c - 1e-9,
        "Site j should be outside site i's half-plane (a·xj+b·yj ≥ c)"
    );
}

#[test]
fn test_power_diagram_cells_partition_bbox() {
    // Three equal-weight sites: together their cells should tile the entire
    // bounding box without gaps (total area = bbox area, all cells non-empty).
    //
    // Note: the cells are clipped by a *square* bbox, so for sites placed at
    // 120° offsets the individual cell areas are NOT equal (the square has only
    // 4-fold symmetry, not 3-fold).  We therefore check only the coverage
    // invariant: sum(cell_area) ≈ bbox_area.
    use std::f64::consts::PI;
    let r = 2.0_f64;
    let sites: Vec<WeightedPoint> = (0..3)
        .map(|k| {
            let angle = 2.0 * PI * (k as f64) / 3.0;
            WeightedPoint::new(r * angle.cos(), r * angle.sin(), 0.0)
        })
        .collect();

    let bbox = (-5.0_f64, -5.0_f64, 5.0_f64, 5.0_f64);
    let options = PowerDiagramOptions {
        bounding_box: Some(bbox),
    };
    let diagram = power_diagram(&sites, &options).expect("partition test ok");
    assert_eq!(diagram.cells.len(), 3);

    // All cells must be non-empty.
    for (i, cell) in diagram.cells.iter().enumerate() {
        assert!(
            !cell.is_empty,
            "Cell {i} should be non-empty for equal-weight sites"
        );
    }

    // Total area of all cells should equal the bbox area.
    let total_area: f64 = diagram.cells.iter().map(|c| polygon_area(&c.polygon)).sum();
    let bbox_area = (bbox.2 - bbox.0) * (bbox.3 - bbox.1);
    let rel_err = (total_area - bbox_area).abs() / bbox_area;
    assert!(
        rel_err < 1e-9,
        "Total cell area {total_area} should equal bbox area {bbox_area} (rel_err={rel_err})"
    );
}
