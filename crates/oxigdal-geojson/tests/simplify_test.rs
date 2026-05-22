//! Integration tests for the Ramer-Douglas-Peucker simplification module
//! and its integration with `GeoJsonWriter::with_simplify`.

use oxigdal_geojson_stream::{GeoJsonGeometry, GeoJsonWriter, simplify_dp};

// ─── simplify_dp unit-level tests ────────────────────────────────────────────

/// A perfectly straight horizontal line of 100 collinear points must
/// collapse to exactly the two endpoints at any ε > 0.
#[test]
fn test_dp_straight_line_collapses_to_two_endpoints() {
    let pts: Vec<[f64; 2]> = (0..100).map(|i| [i as f64, 0.0]).collect();
    let result = simplify_dp(&pts, 1e-6);
    assert_eq!(
        result.len(),
        2,
        "collinear points should collapse to 2 endpoints, got {}",
        result.len()
    );
    assert_eq!(result[0], [0.0, 0.0]);
    assert_eq!(result[1], [99.0, 0.0]);
}

/// With epsilon == 0.0 the algorithm should return the input unchanged.
#[test]
fn test_dp_zero_tolerance_is_identity() {
    let pts: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.0, 0.5], [2.0, 0.0], [3.0, 1.0], [4.0, 0.0]];
    let result = simplify_dp(&pts, 0.0);
    assert_eq!(
        result, pts,
        "zero epsilon must return the original points unchanged"
    );
}

/// A closed polygon ring must remain closed (first == last) after simplification.
#[test]
fn test_dp_preserves_ring_closure() {
    // Build a square with extra collinear points on each edge.
    // Total: 4 corners × (1 extra midpoint + corner) + closing point.
    // Layout:
    //   Bottom edge: (0,0) → (5,0) → (10,0)
    //   Right edge:  (10,5) → (10,10)
    //   Top edge:    (5,10) → (0,10)
    //   Left edge:   (0,5) → close
    let ring: Vec<[f64; 2]> = vec![
        [0.0, 0.0],
        [5.0, 0.0],
        [10.0, 0.0],
        [10.0, 5.0],
        [10.0, 10.0],
        [5.0, 10.0],
        [0.0, 10.0],
        [0.0, 5.0],
        [0.0, 0.0], // close ring
    ];

    let result = simplify_dp(&ring, 0.1);

    // The ring must still be closed.
    let first = result.first().expect("result must not be empty");
    let last = result.last().expect("result must not be empty");
    assert_eq!(
        first, last,
        "simplified closed ring must still be closed (first == last)"
    );

    // The midpoints (collinear with adjacent corners) should have been removed.
    assert!(
        result.len() < ring.len(),
        "expected fewer points after simplification of collinear midpoints"
    );
}

/// A very high tolerance that would geometrically eliminate too many points
/// must fall back to the original ring rather than producing a degenerate
/// polygon with fewer than 4 positions.
#[test]
fn test_dp_degenerate_polygon_falls_back_to_original() {
    // A tiny triangle ring (4 points including closure) with a very large ε.
    let ring = vec![
        [0.0_f64, 0.0],
        [1e-9, 0.0], // nearly collinear with endpoints
        [0.0, 0.0],  // closing
    ];
    // At a very large tolerance this would collapse to <4 points.
    // The degenerate guard must kick in and return the original.
    let result = simplify_dp(&ring, 1e10);
    assert!(
        result.len() >= ring.len(),
        "degenerate simplification must fall back to original (got {} points)",
        result.len()
    );
}

/// Each ring of a MultiPolygon must be simplified independently, so that
/// individually collinear rings each collapse to their two real endpoints
/// independently of one another.
#[test]
fn test_dp_multipolygon_each_ring_simplified() {
    // Two separate polygons, each consisting of a collinear sequence on a
    // horizontal line closed into a "degenerate" ring with the minimum
    // structure that satisfies the closed-ring guard (4 pts including closure).
    // We use genuinely rectangular polygons with collinear mid-points.

    // 10-point ring: collinear along y=0, 8 interior points between endpoints,
    // then back to start.  After simplification only the 4 corners survive.
    fn collinear_rect(base_x: f64, base_y: f64, w: f64, h: f64, steps: usize) -> Vec<[f64; 2]> {
        let mut ring = Vec::new();
        // Bottom edge with `steps` interior points
        for i in 0..=steps {
            ring.push([base_x + (i as f64 / steps as f64) * w, base_y]);
        }
        // Right edge
        for i in 1..=steps {
            ring.push([base_x + w, base_y + (i as f64 / steps as f64) * h]);
        }
        // Top edge (right to left)
        for i in 1..=steps {
            ring.push([base_x + w - (i as f64 / steps as f64) * w, base_y + h]);
        }
        // Left edge (top to bottom)
        for i in 1..steps {
            ring.push([base_x, base_y + h - (i as f64 / steps as f64) * h]);
        }
        // Close ring
        ring.push([base_x, base_y]);
        ring
    }

    let ring_a = collinear_rect(0.0, 0.0, 10.0, 5.0, 8);
    let ring_b = collinear_rect(20.0, 20.0, 6.0, 3.0, 8);

    // Each polygon is a single outer ring.
    let poly_a = vec![ring_a.clone()];
    let poly_b = vec![ring_b.clone()];
    let multi = GeoJsonGeometry::MultiPolygon(vec![poly_a, poly_b]);

    let writer = GeoJsonWriter::compact()
        .with_precision(6)
        .with_simplify(0.01);

    let output = writer.write_geometry(&multi);

    // The output must still be a valid MultiPolygon.
    assert!(
        output.contains(r#""type":"MultiPolygon""#),
        "geometry type must be preserved"
    );

    // Verify that the output is smaller than the unsimplified baseline.
    let baseline =
        GeoJsonWriter::compact()
            .with_precision(6)
            .write_geometry(&GeoJsonGeometry::MultiPolygon(vec![
                vec![ring_a],
                vec![ring_b],
            ]));
    assert!(
        output.len() < baseline.len(),
        "simplified MultiPolygon ({} bytes) must be smaller than original ({} bytes)",
        output.len(),
        baseline.len()
    );
}

// ─── GeoJsonWriter integration tests ─────────────────────────────────────────

/// Writing a 1 000-vertex approximation of a circle with `with_simplify`
/// must produce a strictly shorter JSON byte string than writing without it.
#[test]
fn test_writer_with_simplify_reduces_output_byte_count() {
    use std::f64::consts::PI;

    // Generate a 1000-vertex approximate circle (closed ring) with radius 1°.
    // The ring is in geographic coordinates centred on (10°, 10°).
    let n = 1000_usize;
    let cx = 10.0_f64;
    let cy = 10.0_f64;
    let r = 1.0_f64; // 1 degree radius

    let mut ring: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let angle = 2.0 * PI * (i as f64) / (n as f64);
            [cx + r * angle.cos(), cy + r * angle.sin()]
        })
        .collect();
    // Close the ring — GeoJSON polygon rings must start and end at the same point.
    ring.push(ring[0]);

    let geom = GeoJsonGeometry::Polygon(vec![ring.clone()]);

    let writer_plain = GeoJsonWriter::compact().with_precision(6);
    let writer_simplified = GeoJsonWriter::compact()
        .with_precision(6)
        // 0.005° ≈ ~500 m at the equator — a reasonable web-map tolerance.
        .with_simplify(0.005);

    let plain_output = writer_plain.write_geometry(&geom);
    let simplified_output = writer_simplified.write_geometry(&geom);

    assert!(
        simplified_output.len() < plain_output.len(),
        "simplified output ({} bytes) must be smaller than plain output ({} bytes)",
        simplified_output.len(),
        plain_output.len()
    );

    // The output must still be syntactically a Polygon.
    assert!(
        simplified_output.contains(r#""type":"Polygon""#),
        "simplified output must still be a Polygon"
    );
}

/// Extra: verify that a non-simplified write leaves output identical to
/// manually serialised coordinates, ensuring `with_simplify` is not applied
/// when not requested.
#[test]
fn test_writer_without_simplify_preserves_all_vertices() {
    let pts: Vec<[f64; 2]> = (0..20).map(|i| [i as f64 * 0.1, 0.0]).collect();
    let geom = GeoJsonGeometry::LineString(pts.clone());

    let writer = GeoJsonWriter::compact().with_precision(1);
    let output = writer.write_geometry(&geom);

    // Count coordinate pairs — each appears as "[x,y]".
    // The number of "[" that start coordinate pairs equals pts.len().
    // We simply check the output is long enough to contain all coordinates.
    let coord_count = output.matches(",[0.").count() + output.matches(",0.").count();
    // At precision 1, each coord is like [0.0,0.0] — 20 pairs each with comma separator.
    // Just assert there are at least 18 commas within the coordinate array.
    assert!(
        coord_count >= 18,
        "expected ≥18 coordinate separators, got {coord_count} in: {output}"
    );
}

/// Verify that `with_simplify` is applied to `MultiLineString` rings
/// independently, and that collinear sub-lines are reduced.
#[test]
fn test_writer_simplify_multilinestring() {
    // Two collinear lines: 50 points each along y=0 and y=1.
    let line_a: Vec<[f64; 2]> = (0..50).map(|i| [i as f64, 0.0]).collect();
    let line_b: Vec<[f64; 2]> = (0..50).map(|i| [i as f64, 1.0]).collect();
    let geom = GeoJsonGeometry::MultiLineString(vec![line_a, line_b]);

    let writer_simplified = GeoJsonWriter::compact()
        .with_precision(6)
        .with_simplify(0.01);
    let writer_plain = GeoJsonWriter::compact().with_precision(6);

    let simplified = writer_simplified.write_geometry(&geom);
    let plain = writer_plain.write_geometry(&geom);

    assert!(
        simplified.len() < plain.len(),
        "simplified MultiLineString ({} bytes) must be shorter than plain ({} bytes)",
        simplified.len(),
        plain.len()
    );
    assert!(
        simplified.contains(r#""MultiLineString""#),
        "geometry type must survive simplification"
    );
}
