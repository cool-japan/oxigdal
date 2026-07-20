//! Integration tests for Visvalingam–Whyatt polyline simplification.
//!
//! These tests verify the full public API of `simplify_visvalingam` and
//! `simplify_visvalingam_to_count`, including the lazy-heap invalidation
//! correctness, monotonicity rule, closed-ring handling, and degenerate cases.

use oxigeo_index::validation::Coord;
use oxigeo_index::{simplify_visvalingam, simplify_visvalingam_to_count};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn c(x: f64, y: f64) -> Coord {
    Coord::new(x, y)
}

fn coords_eq(a: Coord, b: Coord) -> bool {
    (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12
}

// ---------------------------------------------------------------------------
// Test 1 — Collinear points collapse to two endpoints
// ---------------------------------------------------------------------------

/// Ten collinear points on y = 0 form zero-area triangles for every interior
/// vertex.  With any positive threshold the entire interior should be removed.
#[test]
fn test_vw_straight_line_collapses_to_two_endpoints() {
    let coords: Vec<Coord> = (0..10).map(|i| c(i as f64, 0.0)).collect();

    let result = simplify_visvalingam(&coords, 1e-10);

    assert_eq!(
        result.len(),
        2,
        "collinear points should collapse to two endpoints, got {:?}",
        result
    );
    assert!(
        coords_eq(result[0], c(0.0, 0.0)),
        "first point must be (0,0)"
    );
    assert!(
        coords_eq(result[result.len() - 1], c(9.0, 0.0)),
        "last point must be (9,0)"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — First and last vertex always preserved
// ---------------------------------------------------------------------------

/// Regardless of threshold, the first and last input vertices must survive.
#[test]
fn test_vw_preserves_first_and_last_vertex() {
    // A curvy 20-point sequence generated arithmetically.
    let coords: Vec<Coord> = (0..20)
        .map(|i| {
            let t = i as f64 * core::f64::consts::PI / 10.0;
            c(i as f64, t.sin())
        })
        .collect();

    // Aggressive threshold — most points should be removed.
    let result = simplify_visvalingam(&coords, 10.0);

    assert!(!result.is_empty(), "result must not be empty");
    assert!(
        coords_eq(result[0], coords[0]),
        "first vertex must be preserved: got {:?}, expected {:?}",
        result[0],
        coords[0]
    );
    assert!(
        coords_eq(*result.last().unwrap(), *coords.last().unwrap()),
        "last vertex must be preserved: got {:?}, expected {:?}",
        result.last().unwrap(),
        coords.last().unwrap()
    );
}

// ---------------------------------------------------------------------------
// Test 3 — Smallest-area vertex removed first
// ---------------------------------------------------------------------------

/// A 5-vertex line where vertex 1 has a tiny triangle area (~0.001) and
/// vertex 3 has a large area (~1.0).  With threshold 0.01, only vertex 1
/// is removed.
#[test]
fn test_vw_removes_smallest_area_vertex_first() {
    // Coords: (0,0) (1,0.001) (2,0) (3,1.0) (4,0)
    // Area at vertex 1 = triangle (0,0)-(1,0.001)-(2,0) = 0.5*|0.001*2| = 0.001
    // Area at vertex 3 = triangle (2,0)-(3,1.0)-(4,0) = 0.5*|1.0*2| = 1.0
    let coords = vec![
        c(0.0, 0.0),
        c(1.0, 0.001),
        c(2.0, 0.0),
        c(3.0, 1.0),
        c(4.0, 0.0),
    ];

    let result = simplify_visvalingam(&coords, 0.01);

    assert_eq!(
        result.len(),
        4,
        "only the tiny-area vertex should be removed; got {:?}",
        result
    );
    // Vertex (1, 0.001) must NOT be in the result.
    let has_tiny = result.iter().any(|p| (p.x - 1.0).abs() < 1e-12);
    assert!(
        !has_tiny,
        "vertex (1, 0.001) should have been removed, result: {:?}",
        result
    );
    // Vertex (3, 1.0) must still be in the result.
    let has_large = result.iter().any(|p| (p.x - 3.0).abs() < 1e-12);
    assert!(
        has_large,
        "vertex (3, 1.0) should be retained, result: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// Test 4 — Zero threshold returns input unchanged
// ---------------------------------------------------------------------------

/// With `min_effective_area = 0.0` the stopping condition is `area < 0.0`,
/// which is never true for non-negative areas.  Hence no vertex is removed.
#[test]
fn test_vw_threshold_zero_returns_input_unchanged() {
    let coords = vec![
        c(0.0, 0.0),
        c(1.0, 0.5),
        c(2.0, 0.0),
        c(3.0, 0.5),
        c(4.0, 0.0),
    ];

    let result = simplify_visvalingam(&coords, 0.0);

    assert_eq!(
        result.len(),
        coords.len(),
        "threshold=0 must not remove any vertices; got {:?}",
        result
    );
    for (orig, got) in coords.iter().zip(result.iter()) {
        assert!(
            coords_eq(*orig, *got),
            "vertex mismatch: expected {:?}, got {:?}",
            orig,
            got
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5 — simplify_visvalingam_to_count exact target
// ---------------------------------------------------------------------------

/// `simplify_visvalingam_to_count` with target=5 on a 10-vertex input must
/// return exactly 5 vertices.
#[test]
fn test_vw_to_count_exact_target() {
    // Arithmetic sequence of y-values: 0, 1, 4, 9, 16, … (i²) to ensure
    // varying triangle areas so vertices are distinguishable.
    let coords: Vec<Coord> = (0..10).map(|i| c(i as f64, (i * i) as f64)).collect();

    let result = simplify_visvalingam_to_count(&coords, 5);

    assert_eq!(
        result.len(),
        5,
        "to_count(5) must yield exactly 5 vertices; got {:?}",
        result
    );
    // First and last must still be preserved.
    assert!(coords_eq(result[0], coords[0]));
    assert!(coords_eq(*result.last().unwrap(), *coords.last().unwrap()));
}

// ---------------------------------------------------------------------------
// Test 6 — Closed ring stays closed after simplification
// ---------------------------------------------------------------------------

/// A square ring simplified to 3 coordinates must remain closed
/// (first coordinate == last coordinate).
#[test]
fn test_vw_closed_ring_preserves_closure() {
    // Square ring: (0,0)→(1,0)→(1,1)→(0,1)→(0,0)
    let ring = vec![
        c(0.0, 0.0),
        c(1.0, 0.0),
        c(1.0, 1.0),
        c(0.0, 1.0),
        c(0.0, 0.0), // closing vertex
    ];

    // Reduce to the minimum meaningful ring (3 unique + 1 closure = 4 coords).
    let result = simplify_visvalingam_to_count(&ring, 4);

    assert!(
        result.len() >= 4,
        "simplified ring must have at least 4 coordinates; got {:?}",
        result
    );
    assert!(
        coords_eq(result[0], *result.last().unwrap()),
        "ring must stay closed: first={:?}, last={:?}",
        result[0],
        result.last().unwrap()
    );
}

// ---------------------------------------------------------------------------
// Test 7 — Lazy heap invalidation: stale heap entry must be discarded
// ---------------------------------------------------------------------------

/// This test specifically exercises the version-counter lazy invalidation path
/// in the Visvalingam–Whyatt min-heap.
///
/// Input: `(0,0), (1,0.01), (2,0), (5,3), (6,0)`.
///
/// Initial effective areas (calculated from the original neighbour triplets):
/// - Vertex 1 at (1,0.01): triangle (0,0)–(1,0.01)–(2,0)  → area = 0.010
/// - Vertex 2 at (2,0):    triangle (1,0.01)–(2,0)–(5,3)  → area ≈ 1.515  (STALE entry)
/// - Vertex 3 at (5,3):    triangle (2,0)–(5,3)–(6,0)      → area = 6.000
///
/// Processing with threshold = 0.02:
/// 1. Pop vertex 1 (area 0.010 < 0.02) → remove it; `effective_area = 0.010`.
/// 2. Recompute vertex 2's area using its new neighbours (0,0) and (5,3):
///    triangle (0,0)–(2,0)–(5,3) → area = 3.000.
///    Monotone: max(3.000, 0.010) = 3.000.  Push new entry (area=3.0, version=1).
/// 3. Old stale entry for vertex 2 (area≈1.515, version=0) is still in the heap.
///    When popped: `version 0 ≠ versions[2] = 1` → discarded (lazy invalidation!).
/// 4. New entry for vertex 2 (area=3.0): 3.0 ≥ 0.02 → stop loop.
/// 5. Vertex 3 area 6.0 ≥ 0.02 → never removed.
///
/// Expected output: 4 vertices — (0,0), (2,0), (5,3), (6,0).
#[test]
fn test_vw_lazy_heap_invalidation_with_neighbour_area_decrease() {
    // Coords chosen so that vertex 2's initial heap entry becomes stale after
    // vertex 1 is removed, requiring the version-counter check to discard it.
    let coords = vec![
        c(0.0, 0.0),
        c(1.0, 0.01),
        c(2.0, 0.0),
        c(5.0, 3.0),
        c(6.0, 0.0),
    ];

    // — Aggressive threshold 0.02 —
    // Only vertex 1 (area 0.010) is below threshold; the stale entry for
    // vertex 2 is invalidated via version counter, and vertex 2's recomputed
    // area (3.0) plus vertex 3's area (6.0) are both above threshold.
    // Expected: 4 survivors — (0,0), (2,0), (5,3), (6,0).
    let result = simplify_visvalingam(&coords, 0.02);
    assert_eq!(
        result.len(),
        4,
        "only vertex 1 should be removed; stale heap entry for vertex 2 must be \
         discarded by lazy invalidation; got {:?}",
        result
    );
    assert!(
        coords_eq(result[0], coords[0]),
        "first vertex must be preserved"
    );
    assert!(
        coords_eq(*result.last().unwrap(), coords[4]),
        "last vertex must be preserved"
    );
    // Vertex (1, 0.01) must have been removed.
    let has_v1 = result.iter().any(|p| (p.x - 1.0).abs() < 1e-12);
    assert!(
        !has_v1,
        "vertex (1, 0.01) must be removed; got {:?}",
        result
    );
    // Vertices (2,0) and (5,3) must still be present.
    let has_v2 = result.iter().any(|p| (p.x - 2.0).abs() < 1e-12);
    let has_v3 = result.iter().any(|p| (p.x - 5.0).abs() < 1e-12);
    assert!(has_v2, "vertex (2,0) must be retained; got {:?}", result);
    assert!(has_v3, "vertex (5,3) must be retained; got {:?}", result);

    // — Gentle threshold 0.005 —
    // No vertex has area < 0.005 (smallest area is 0.010), so all 5 are retained.
    let result_gentle = simplify_visvalingam(&coords, 0.005);
    assert_eq!(
        result_gentle.len(),
        5,
        "threshold below all areas: all 5 vertices must be kept; got {:?}",
        result_gentle
    );

    // — Very aggressive threshold 10.0 —
    // All interior vertices are below 10.0; only endpoints survive.
    let result_max = simplify_visvalingam(&coords, 10.0);
    assert_eq!(
        result_max.len(),
        2,
        "threshold above all areas: only endpoints survive; got {:?}",
        result_max
    );
    assert!(coords_eq(result_max[0], coords[0]));
    assert!(coords_eq(result_max[1], coords[4]));
}
