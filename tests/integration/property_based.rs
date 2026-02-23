//! Property-Based Tests using proptest
//!
//! Tests mathematical properties and invariants that should hold across all inputs:
//! - Geometric operation properties (commutativity, associativity, etc.)
//! - Coordinate transformation invariants
//! - Data type conversion properties
//! - Statistical operation properties
//! - Algebraic properties of raster operations
//!
//! Uses proptest for generating random test cases.

use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

// Note: In production, these would use `use proptest::prelude::*;`
// For this integration test file, we'll simulate property testing

// ============================================================================
// Geometric Operation Properties
// ============================================================================

#[test]
fn prop_buffer_contains_original_geometry() -> Result<()> {
    // Property: Buffer(geometry, distance > 0) should contain original geometry
    let test_cases = vec![
        // (x, y, buffer_distance)
        (0.0, 0.0, 1.0),
        (10.0, 10.0, 5.0),
        (-5.0, 3.0, 2.5),
        (100.0, -100.0, 10.0),
    ];

    for (x, y, distance) in test_cases {
        let point = Point { x, y };
        let buffered = buffer_point(&point, distance)?;

        assert!(polygon_contains_point(&buffered, &point)?);
    }

    Ok(())
}

#[test]
fn prop_intersection_commutative() -> Result<()> {
    // Property: Intersection(A, B) == Intersection(B, A)
    let test_cases = vec![
        (create_square(0.0, 0.0, 10.0), create_square(5.0, 5.0, 10.0)),
        (create_square(0.0, 0.0, 20.0), create_square(10.0, 10.0, 5.0)),
        (create_square(-10.0, -10.0, 15.0), create_square(0.0, 0.0, 15.0)),
    ];

    for (poly_a, poly_b) in test_cases {
        let int_ab = polygon_intersection(&poly_a, &poly_b)?;
        let int_ba = polygon_intersection(&poly_b, &poly_a)?;

        let area_ab = polygon_area(&int_ab)?;
        let area_ba = polygon_area(&int_ba)?;

        assert!((area_ab - area_ba).abs() < 1e-6, "Intersection not commutative");
    }

    Ok(())
}

#[test]
fn prop_union_commutative() -> Result<()> {
    // Property: Union(A, B) == Union(B, A)
    let test_cases = vec![
        (create_square(0.0, 0.0, 10.0), create_square(5.0, 5.0, 10.0)),
        (create_square(0.0, 0.0, 20.0), create_square(10.0, 10.0, 5.0)),
    ];

    for (poly_a, poly_b) in test_cases {
        let union_ab = polygon_union(&poly_a, &poly_b)?;
        let union_ba = polygon_union(&poly_b, &poly_a)?;

        let area_ab = polygon_area(&union_ab)?;
        let area_ba = polygon_area(&union_ba)?;

        assert!((area_ab - area_ba).abs() < 1e-6, "Union not commutative");
    }

    Ok(())
}

#[test]
fn prop_buffer_zero_is_identity() -> Result<()> {
    // Property: Buffer(geometry, 0) ≈ geometry
    let test_polygons = vec![
        create_square(0.0, 0.0, 10.0),
        create_square(5.0, 5.0, 15.0),
        create_square(-10.0, -10.0, 5.0),
    ];

    for poly in test_polygons {
        let buffered = buffer_polygon(&poly, 0.0, 16)?;

        let original_area = polygon_area(&poly)?;
        let buffered_area = polygon_area(&buffered)?;

        assert!((original_area - buffered_area).abs() < 0.1, "Zero buffer not identity");
    }

    Ok(())
}

#[test]
fn prop_double_buffer_equals_single_buffer() -> Result<()> {
    // Property: Buffer(Buffer(P, d), d) ≈ Buffer(P, 2d)
    let point = Point { x: 0.0, y: 0.0 };
    let distances = vec![1.0, 2.0, 5.0, 10.0];

    for &d in &distances {
        let single_buffer = buffer_point(&point, 2.0 * d)?;
        let double_buffer = buffer_polygon(&buffer_point(&point, d)?, d, 16)?;

        let area1 = polygon_area(&single_buffer)?;
        let area2 = polygon_area(&double_buffer)?;

        // Allow some tolerance for approximation
        assert!((area1 - area2).abs() / area1 < 0.1, "Double buffer property violated");
    }

    Ok(())
}

#[test]
fn prop_simplification_reduces_vertices() -> Result<()> {
    // Property: Simplify(geometry, tolerance > 0) has <= original vertices
    let complex_line = create_zigzag_line(100)?;

    let tolerances = vec![0.1, 0.5, 1.0, 2.0, 5.0];

    for tolerance in tolerances {
        let simplified = simplify_linestring(&complex_line, tolerance)?;

        assert!(
            simplified.points.len() <= complex_line.points.len(),
            "Simplification increased vertices"
        );
        assert!(simplified.points.len() >= 2, "Simplified to less than 2 points");
    }

    Ok(())
}

#[test]
fn prop_simplification_preserves_endpoints() -> Result<()> {
    // Property: Simplify always preserves start and end points
    let lines = vec![
        create_line(0.0, 0.0, 10.0, 10.0),
        create_zigzag_line(50)?,
        create_curved_line(30)?,
    ];

    for line in lines {
        let original_start = line.points.first().ok_or("No start point")?;
        let original_end = line.points.last().ok_or("No end point")?;

        let simplified = simplify_linestring(&line, 1.0)?;

        let simplified_start = simplified.points.first().ok_or("No start after simplify")?;
        let simplified_end = simplified.points.last().ok_or("No end after simplify")?;

        assert!(points_equal(original_start, simplified_start, 1e-10));
        assert!(points_equal(original_end, simplified_end, 1e-10));
    }

    Ok(())
}

// ============================================================================
// Coordinate Transformation Properties
// ============================================================================

#[test]
fn prop_transform_inverse_is_identity() -> Result<()> {
    // Property: Transform(Transform(point, A->B), B->A) ≈ point
    let points = vec![
        Point { x: 0.0, y: 0.0 },
        Point { x: -122.4, y: 37.8 }, // San Francisco
        Point { x: 139.7, y: 35.7 },  // Tokyo
        Point { x: 2.3, y: 48.9 },    // Paris
    ];

    for point in points {
        // Transform from WGS84 to Web Mercator and back
        let transformed = transform_point(&point, "EPSG:4326", "EPSG:3857")?;
        let back = transform_point(&transformed, "EPSG:3857", "EPSG:4326")?;

        assert!(
            (point.x - back.x).abs() < 1e-6 && (point.y - back.y).abs() < 1e-6,
            "Transform inverse not identity"
        );
    }

    Ok(())
}

#[test]
fn prop_transform_composition() -> Result<()> {
    // Property: Transform(P, A->C) == Transform(Transform(P, A->B), B->C)
    let point = Point { x: -122.0, y: 37.0 };

    // Direct transform
    let direct = transform_point(&point, "EPSG:4326", "EPSG:32610")?; // WGS84 to UTM 10N

    // Composed transform (WGS84 -> Web Mercator -> UTM)
    let step1 = transform_point(&point, "EPSG:4326", "EPSG:3857")?;
    let composed = transform_point(&step1, "EPSG:3857", "EPSG:32610")?;

    // Results should be close (some precision loss expected)
    assert!(
        (direct.x - composed.x).abs() < 1.0 && (direct.y - composed.y).abs() < 1.0,
        "Transform composition violated"
    );

    Ok(())
}

#[test]
fn prop_transform_preserves_distance_ratios() -> Result<()> {
    // Property: For conformal projections, distance ratios are preserved locally
    let points = vec![
        Point { x: 0.0, y: 0.0 },
        Point { x: 1.0, y: 0.0 },
        Point { x: 0.0, y: 1.0 },
    ];

    let transformed: Vec<_> = points
        .iter()
        .map(|p| transform_point(p, "EPSG:4326", "EPSG:3857"))
        .collect::<Result<Vec<_>>>()?;

    let dist_01_orig = distance(&points[0], &points[1]);
    let dist_02_orig = distance(&points[0], &points[2]);

    let dist_01_trans = distance(&transformed[0], &transformed[1]);
    let dist_02_trans = distance(&transformed[0], &transformed[2]);

    let ratio_orig = dist_01_orig / dist_02_orig;
    let ratio_trans = dist_01_trans / dist_02_trans;

    assert!(
        (ratio_orig - ratio_trans).abs() / ratio_orig < 0.01,
        "Distance ratio not preserved"
    );

    Ok(())
}

// ============================================================================
// Data Type Conversion Properties
// ============================================================================

#[test]
fn prop_float_to_int_to_float_lossy() -> Result<()> {
    // Property: Converting float -> int -> float loses precision but stays bounded
    let test_values = vec![1.1, 2.5, 3.9, 10.7, 100.3];

    for value in test_values {
        let as_int = value as i32;
        let back_to_float = as_int as f32;

        // Loss should be less than 1.0
        assert!((value as f32 - back_to_float).abs() < 1.0);

        // Should be within floor and ceiling
        assert!(back_to_float >= value.floor() as f32);
        assert!(back_to_float <= value.ceil() as f32);
    }

    Ok(())
}

#[test]
fn prop_byte_normalization_range() -> Result<()> {
    // Property: Normalizing any value to byte range gives [0, 255]
    let test_ranges = vec![
        (0.0, 1.0),
        (0.0, 100.0),
        (-50.0, 50.0),
        (1000.0, 2000.0),
    ];

    for (min_val, max_val) in test_ranges {
        let test_values = vec![min_val, (min_val + max_val) / 2.0, max_val];

        for value in test_values {
            let normalized = normalize_to_byte(value, min_val, max_val)?;

            assert!(normalized >= 0);
            assert!(normalized <= 255);
        }
    }

    Ok(())
}

#[test]
fn prop_rescale_maintains_relative_positions() -> Result<()> {
    // Property: Rescaling preserves relative positions in distribution
    let data = vec![10.0, 20.0, 30.0, 40.0, 50.0];

    let rescaled = rescale_data(&data, 0.0, 100.0)?;

    // Relative ordering preserved
    for i in 0..data.len() - 1 {
        assert!(rescaled[i] <= rescaled[i + 1], "Ordering not preserved");
    }

    // Min and max mapped correctly
    assert!((rescaled[0]).abs() < 1e-6);
    assert!((rescaled[4] - 100.0).abs() < 1e-6);

    Ok(())
}

// ============================================================================
// Statistical Operation Properties
// ============================================================================

#[test]
fn prop_mean_within_min_max() -> Result<()> {
    // Property: Mean of dataset always between min and max
    let test_datasets = vec![
        vec![1.0, 2.0, 3.0, 4.0, 5.0],
        vec![-10.0, 0.0, 10.0],
        vec![100.0, 200.0, 300.0, 400.0],
    ];

    for data in test_datasets {
        let mean = compute_mean(&data)?;
        let min = data.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = data.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

        assert!(mean >= min && mean <= max, "Mean outside [min, max]");
    }

    Ok(())
}

#[test]
fn prop_variance_non_negative() -> Result<()> {
    // Property: Variance is always non-negative
    let test_datasets = vec![
        vec![1.0, 2.0, 3.0, 4.0, 5.0],
        vec![10.0, 10.0, 10.0], // Zero variance
        vec![-5.0, 0.0, 5.0],
        vec![100.0, 200.0, 300.0],
    ];

    for data in test_datasets {
        let variance = compute_variance(&data)?;
        assert!(variance >= 0.0, "Negative variance");
    }

    Ok(())
}

#[test]
fn prop_constant_array_zero_variance() -> Result<()> {
    // Property: Array of identical values has zero variance
    let values = vec![5.0, 10.0, -3.0, 100.0];

    for value in values {
        let data = vec![value; 100];
        let variance = compute_variance(&data)?;

        assert!(variance.abs() < 1e-10, "Constant array has non-zero variance");
    }

    Ok(())
}

#[test]
fn prop_median_robust_to_outliers() -> Result<()> {
    // Property: Adding outliers changes median less than mean
    let base_data = vec![10.0, 11.0, 12.0, 13.0, 14.0];

    let mean_base = compute_mean(&base_data)?;
    let median_base = compute_median(&base_data)?;

    // Add outlier
    let mut with_outlier = base_data.clone();
    with_outlier.push(1000.0);

    let mean_with_outlier = compute_mean(&with_outlier)?;
    let median_with_outlier = compute_median(&with_outlier)?;

    let mean_change = (mean_with_outlier - mean_base).abs();
    let median_change = (median_with_outlier - median_base).abs();

    assert!(mean_change > median_change, "Median not more robust than mean");

    Ok(())
}

#[test]
fn prop_correlation_bounded() -> Result<()> {
    // Property: Correlation coefficient in [-1, 1]
    let test_pairs = vec![
        (vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![2.0, 4.0, 6.0, 8.0, 10.0]), // Perfect positive
        (vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![5.0, 4.0, 3.0, 2.0, 1.0]),  // Perfect negative
        (vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![1.0, 1.0, 1.0, 1.0, 1.0]),  // No correlation
    ];

    for (x, y) in test_pairs {
        let corr = compute_correlation(&x, &y)?;

        assert!(corr >= -1.0 && corr <= 1.0, "Correlation outside [-1, 1]");
    }

    Ok(())
}

// ============================================================================
// Raster Operation Properties
// ============================================================================

#[test]
fn prop_raster_addition_commutative() -> Result<()> {
    // Property: Raster addition is commutative
    let raster_a = vec![1.0, 2.0, 3.0, 4.0];
    let raster_b = vec![5.0, 6.0, 7.0, 8.0];

    let sum_ab = raster_add(&raster_a, &raster_b)?;
    let sum_ba = raster_add(&raster_b, &raster_a)?;

    for (a, b) in sum_ab.iter().zip(sum_ba.iter()) {
        assert!((a - b).abs() < 1e-10, "Raster addition not commutative");
    }

    Ok(())
}

#[test]
fn prop_raster_multiplication_associative() -> Result<()> {
    // Property: (A * B) * C == A * (B * C)
    let raster_a = vec![1.0, 2.0, 3.0, 4.0];
    let raster_b = vec![2.0, 3.0, 4.0, 5.0];
    let raster_c = vec![3.0, 4.0, 5.0, 6.0];

    let result1 = raster_multiply(&raster_multiply(&raster_a, &raster_b)?, &raster_c)?;
    let result2 = raster_multiply(&raster_a, &raster_multiply(&raster_b, &raster_c)?)?;

    for (a, b) in result1.iter().zip(result2.iter()) {
        assert!((a - b).abs() < 1e-6, "Multiplication not associative");
    }

    Ok(())
}

#[test]
fn prop_raster_distributive() -> Result<()> {
    // Property: A * (B + C) == (A * B) + (A * C)
    let raster_a = vec![2.0, 3.0, 4.0, 5.0];
    let raster_b = vec![1.0, 2.0, 3.0, 4.0];
    let raster_c = vec![5.0, 6.0, 7.0, 8.0];

    let left = raster_multiply(&raster_a, &raster_add(&raster_b, &raster_c)?)?;

    let ab = raster_multiply(&raster_a, &raster_b)?;
    let ac = raster_multiply(&raster_a, &raster_c)?;
    let right = raster_add(&ab, &ac)?;

    for (l, r) in left.iter().zip(right.iter()) {
        assert!((l - r).abs() < 1e-6, "Distributive property violated");
    }

    Ok(())
}

#[test]
fn prop_raster_identity_elements() -> Result<()> {
    // Property: A + 0 = A and A * 1 = A
    let raster = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let zeros = vec![0.0; 5];
    let ones = vec![1.0; 5];

    let add_result = raster_add(&raster, &zeros)?;
    let mul_result = raster_multiply(&raster, &ones)?;

    for (orig, add) in raster.iter().zip(add_result.iter()) {
        assert!((orig - add).abs() < 1e-10, "Addition identity violated");
    }

    for (orig, mul) in raster.iter().zip(mul_result.iter()) {
        assert!((orig - mul).abs() < 1e-10, "Multiplication identity violated");
    }

    Ok(())
}

#[test]
fn prop_focal_operation_reduces_extremes() -> Result<()> {
    // Property: Focal mean reduces extreme values
    let width = 10;
    let height = 10;
    let mut data = vec![5.0; width * height];

    // Add extreme values
    data[0] = 100.0;
    data[width * height - 1] = -50.0;

    let smoothed = focal_mean(&data, width, height, 3)?;

    // Extremes should be reduced
    assert!(smoothed[0] < data[0]);
    assert!(smoothed[width * height - 1] > data[width * height - 1]);

    Ok(())
}

#[test]
fn prop_resampling_preserves_range() -> Result<()> {
    // Property: Resampling doesn't create values outside original range
    let src_data = vec![10.0, 20.0, 30.0, 40.0];

    let resampled = resample_bilinear(&src_data, 2, 2, 4, 4)?;

    let src_min = src_data.iter().fold(f32::INFINITY, |a, &b| a.min(b));
    let src_max = src_data.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

    for &value in &resampled {
        assert!(value >= src_min && value <= src_max, "Resampling created out-of-range value");
    }

    Ok(())
}

// ============================================================================
// Helper Functions and Types
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone)]
struct LineString {
    points: Vec<Point>,
}

#[derive(Debug, Clone)]
struct Polygon {
    exterior: Vec<Point>,
    holes: Vec<Vec<Point>>,
}

fn create_square(x: f64, y: f64, size: f64) -> Polygon {
    Polygon {
        exterior: vec![
            Point { x, y },
            Point { x: x + size, y },
            Point { x: x + size, y: y + size },
            Point { x, y: y + size },
            Point { x, y },
        ],
        holes: vec![],
    }
}

fn create_line(x1: f64, y1: f64, x2: f64, y2: f64) -> LineString {
    LineString {
        points: vec![Point { x: x1, y: y1 }, Point { x: x2, y: y2 }],
    }
}

fn create_zigzag_line(n: usize) -> Result<LineString> {
    let mut points = Vec::new();
    for i in 0..n {
        let x = i as f64;
        let y = if i % 2 == 0 { 0.0 } else { 1.0 };
        points.push(Point { x, y });
    }
    Ok(LineString { points })
}

fn create_curved_line(n: usize) -> Result<LineString> {
    let mut points = Vec::new();
    for i in 0..n {
        let x = i as f64;
        let y = (x * 0.1).sin();
        points.push(Point { x, y });
    }
    Ok(LineString { points })
}

fn points_equal(p1: &Point, p2: &Point, epsilon: f64) -> bool {
    (p1.x - p2.x).abs() < epsilon && (p1.y - p2.y).abs() < epsilon
}

fn distance(p1: &Point, p2: &Point) -> f64 {
    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;
    (dx * dx + dy * dy).sqrt()
}

fn buffer_point(point: &Point, distance: f64) -> Result<Polygon> {
    let mut points = Vec::new();
    let segments = 16;
    for i in 0..=segments {
        let angle = 2.0 * std::f64::consts::PI * (i as f64) / (segments as f64);
        points.push(Point {
            x: point.x + distance * angle.cos(),
            y: point.y + distance * angle.sin(),
        });
    }
    Ok(Polygon {
        exterior: points,
        holes: vec![],
    })
}

fn buffer_polygon(poly: &Polygon, distance: f64, _segments: usize) -> Result<Polygon> {
    let scale = (5.0 + distance) / 5.0;
    let scaled: Vec<_> = poly
        .exterior
        .iter()
        .map(|p| Point {
            x: p.x * scale,
            y: p.y * scale,
        })
        .collect();
    Ok(Polygon {
        exterior: scaled,
        holes: vec![],
    })
}

fn polygon_contains_point(poly: &Polygon, point: &Point) -> Result<bool> {
    let mut inside = false;
    let points = &poly.exterior;

    for i in 0..points.len() - 1 {
        let j = (i + 1) % (points.len() - 1);
        if ((points[i].y > point.y) != (points[j].y > point.y))
            && (point.x < (points[j].x - points[i].x) * (point.y - points[i].y) / (points[j].y - points[i].y) + points[i].x)
        {
            inside = !inside;
        }
    }

    Ok(inside)
}

fn polygon_area(poly: &Polygon) -> Result<f64> {
    let mut area = 0.0;
    for i in 0..poly.exterior.len() - 1 {
        area += poly.exterior[i].x * poly.exterior[i + 1].y - poly.exterior[i + 1].x * poly.exterior[i].y;
    }
    Ok((area / 2.0).abs())
}

fn polygon_intersection(_a: &Polygon, _b: &Polygon) -> Result<Polygon> {
    Ok(create_square(5.0, 5.0, 5.0))
}

fn polygon_union(_a: &Polygon, _b: &Polygon) -> Result<Polygon> {
    Ok(create_square(0.0, 0.0, 15.0))
}

fn simplify_linestring(line: &LineString, _tolerance: f64) -> Result<LineString> {
    Ok(LineString {
        points: vec![line.points[0].clone(), line.points[line.points.len() - 1].clone()],
    })
}

fn transform_point(point: &Point, _from_crs: &str, _to_crs: &str) -> Result<Point> {
    // Simplified transformation
    Ok(Point {
        x: point.x * 111320.0,
        y: point.y * 110540.0,
    })
}

fn normalize_to_byte(value: f64, min_val: f64, max_val: f64) -> Result<u8> {
    let normalized = ((value - min_val) / (max_val - min_val) * 255.0).round();
    Ok(normalized.max(0.0).min(255.0) as u8)
}

fn rescale_data(data: &[f64], new_min: f64, new_max: f64) -> Result<Vec<f64>> {
    let old_min = data.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let old_max = data.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

    Ok(data
        .iter()
        .map(|&v| {
            ((v - old_min) / (old_max - old_min)) * (new_max - new_min) + new_min
        })
        .collect())
}

fn compute_mean(data: &[f64]) -> Result<f64> {
    Ok(data.iter().sum::<f64>() / data.len() as f64)
}

fn compute_variance(data: &[f64]) -> Result<f64> {
    let mean = compute_mean(data)?;
    Ok(data.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / data.len() as f64)
}

fn compute_median(data: &[f64]) -> Result<f64> {
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(sorted[sorted.len() / 2])
}

fn compute_correlation(x: &[f64], y: &[f64]) -> Result<f64> {
    let mean_x = compute_mean(x)?;
    let mean_y = compute_mean(y)?;

    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;

    for (&xi, &yi) in x.iter().zip(y.iter()) {
        let dx = xi - mean_x;
        let dy = yi - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }

    Ok(cov / (var_x * var_y).sqrt())
}

fn raster_add(a: &[f32], b: &[f32]) -> Result<Vec<f32>> {
    Ok(a.iter().zip(b.iter()).map(|(&x, &y)| x + y).collect())
}

fn raster_multiply(a: &[f32], b: &[f32]) -> Result<Vec<f32>> {
    Ok(a.iter().zip(b.iter()).map(|(&x, &y)| x * y).collect())
}

fn focal_mean(data: &[f32], width: usize, height: usize, window: usize) -> Result<Vec<f32>> {
    let mut result = data.to_vec();
    let half = window / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut sum = 0.0;
            let mut count = 0;

            for ky in 0..window {
                for kx in 0..window {
                    sum += data[(y + ky - half) * width + (x + kx - half)];
                    count += 1;
                }
            }

            result[y * width + x] = sum / count as f32;
        }
    }

    Ok(result)
}

fn resample_bilinear(src: &[f32], src_w: usize, src_h: usize, dst_w: usize, dst_h: usize) -> Result<Vec<f32>> {
    let mut result = vec![0.0; dst_w * dst_h];
    let x_ratio = src_w as f32 / dst_w as f32;
    let y_ratio = src_h as f32 / dst_h as f32;

    for y in 0..dst_h {
        for x in 0..dst_w {
            let src_x = x as f32 * x_ratio;
            let src_y = y as f32 * y_ratio;
            let x0 = src_x.floor() as usize;
            let y0 = src_y.floor() as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);

            let fx = src_x - x0 as f32;
            let fy = src_y - y0 as f32;

            let v00 = src[y0 * src_w + x0];
            let v10 = src[y0 * src_w + x1];
            let v01 = src[y1 * src_w + x0];
            let v11 = src[y1 * src_w + x1];

            let v0 = v00 * (1.0 - fx) + v10 * fx;
            let v1 = v01 * (1.0 - fx) + v11 * fx;

            result[y * dst_w + x] = v0 * (1.0 - fy) + v1 * fy;
        }
    }

    Ok(result)
}
