//! Ramer-Douglas-Peucker polyline/polygon simplification.
//!
//! Implements the iterative (stack-based) variant to avoid stack overflow on
//! long coordinate sequences.  The algorithm is coordinate-system agnostic;
//! callers supply an `epsilon` in the same units as the coordinates (degrees
//! for WGS-84, metres for projected CRS, etc.).

// ─── Public API ───────────────────────────────────────────────────────────────

/// Simplify a 2-D polyline using the Ramer-Douglas-Peucker algorithm.
///
/// `points` is a slice of `[longitude, latitude]` (or `[x, y]`) pairs.
/// `epsilon` is the distance threshold in the same units as the coordinates.
///
/// * Returns the original slice unchanged when `epsilon` is zero or when the
///   input has fewer than three points.
/// * Guarantees that the first and last point are always preserved.
/// * For **closed rings** (first point == last point) the ring-closure
///   invariant is maintained after simplification; if simplification would
///   produce fewer than 4 positions the original ring is returned intact.
/// * For **open line strings** (not closed) the degenerate guard is triggered
///   when simplification would produce fewer than 2 points.
#[must_use]
pub fn simplify_dp(points: &[[f64; 2]], epsilon: f64) -> Vec<[f64; 2]> {
    let n = points.len();

    // Trivially short sequences — nothing to simplify.
    if n <= 2 {
        return points.to_vec();
    }

    // Zero (or negative) epsilon — caller wants no simplification.
    if epsilon <= 0.0 {
        return points.to_vec();
    }

    // Detect closed ring: first and last point are the same.
    let is_closed = points_equal_2d(points[0], points[n - 1]);

    // ── Iterative Ramer-Douglas-Peucker ──────────────────────────────────────
    //
    // `keep[i]` encodes whether points[i] must be included in the output.
    // Endpoints are always kept.
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;

    // Stack stores (start_idx, end_idx) segments to examine.
    let mut stack: Vec<(usize, usize)> = Vec::with_capacity(32);
    stack.push((0, n - 1));

    while let Some((start, end)) = stack.pop() {
        // Segment is already a single edge — nothing to split.
        if end.saturating_sub(start) <= 1 {
            continue;
        }

        let mut max_dist = 0.0_f64;
        let mut max_idx = start;

        for i in (start + 1)..end {
            let dist = perpendicular_distance(points[i], points[start], points[end]);
            if dist > max_dist {
                max_dist = dist;
                max_idx = i;
            }
        }

        if max_dist > epsilon {
            keep[max_idx] = true;
            stack.push((start, max_idx));
            stack.push((max_idx, end));
        }
    }

    // Collect surviving points in order.
    let simplified: Vec<[f64; 2]> = points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| if keep[i] { Some(*p) } else { None })
        .collect();

    // ── Degenerate-guard ─────────────────────────────────────────────────────
    //
    // If the result is too small to be geometrically valid, fall back to the
    // original input so callers do not receive an invalid geometry.
    let min_valid = if is_closed { 4 } else { 2 };
    if simplified.len() < min_valid {
        return points.to_vec();
    }

    // ── Ring-closure invariant ───────────────────────────────────────────────
    //
    // If the original was a closed ring, make sure the simplified result is
    // also closed.  The last kept point may differ from the first because the
    // closing duplicate could have been removed by the algorithm's endpoint
    // rule (it is always kept, but its coordinate must equal points[0]).
    // We re-attach it explicitly just in case floating-point comparison of the
    // keep-mask is insufficient.
    if is_closed {
        let mut result = simplified;
        let last_idx = result.len() - 1;
        if !points_equal_2d(result[0], result[last_idx]) {
            result[last_idx] = result[0];
        }
        return result;
    }

    simplified
}

// ─── Geometry helpers ─────────────────────────────────────────────────────────

/// Perpendicular distance from point `p` to the line segment `[a, b]`.
///
/// When `a == b` (degenerate segment) the distance degenerates to the
/// Euclidean distance between `p` and `a`.
#[inline]
fn perpendicular_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];

    let len_sq = dx * dx + dy * dy;

    if len_sq == 0.0 {
        // Degenerate segment: a == b.
        return euclidean(p, a);
    }

    // Scalar projection of AP onto AB, clamped to [0, 1] so the nearest
    // point is constrained to the segment.
    let t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len_sq;
    let t_clamped = t.clamp(0.0, 1.0);

    let nearest = [a[0] + t_clamped * dx, a[1] + t_clamped * dy];
    euclidean(p, nearest)
}

/// Euclidean distance between two 2-D points.
#[inline]
fn euclidean(p: [f64; 2], q: [f64; 2]) -> f64 {
    let dx = p[0] - q[0];
    let dy = p[1] - q[1];
    (dx * dx + dy * dy).sqrt()
}

/// Test coordinate equality with a very tight tolerance to account for
/// floating-point round-trip artefacts while still distinguishing genuinely
/// different coordinates.
#[inline]
fn points_equal_2d(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < f64::EPSILON && (a[1] - b[1]).abs() < f64::EPSILON
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perpendicular_distance_on_segment() {
        // Point exactly on the segment midpoint should have distance 0.
        let a = [0.0, 0.0];
        let b = [4.0, 0.0];
        let p = [2.0, 0.0];
        assert!(perpendicular_distance(p, a, b) < 1e-12);
    }

    #[test]
    fn test_perpendicular_distance_off_segment() {
        // Point 3 units above midpoint of horizontal segment of length 4.
        let a = [0.0, 0.0];
        let b = [4.0, 0.0];
        let p = [2.0, 3.0];
        let d = perpendicular_distance(p, a, b);
        assert!((d - 3.0).abs() < 1e-12, "expected 3.0, got {d}");
    }

    #[test]
    fn test_perpendicular_distance_degenerate_segment() {
        // When a == b the function should return the Euclidean distance to a.
        let a = [1.0, 1.0];
        let p = [4.0, 5.0];
        let d = perpendicular_distance(p, a, a);
        let expected = euclidean(p, a);
        assert!((d - expected).abs() < 1e-12);
    }

    #[test]
    fn test_two_point_line_unchanged() {
        let pts = vec![[0.0_f64, 0.0], [10.0, 0.0]];
        let result = simplify_dp(&pts, 1.0);
        assert_eq!(result, pts);
    }

    #[test]
    fn test_single_point_unchanged() {
        let pts = vec![[5.0_f64, 3.0]];
        let result = simplify_dp(&pts, 1.0);
        assert_eq!(result, pts);
    }
}
