//! Hobby snap rounding for robust geometric operations.
//!
//! Snap rounding is a technique to achieve robustness in computational geometry
//! by snapping all coordinates (and intersection points) to a regular grid.
//! This eliminates floating-point inconsistencies and ensures topological
//! validity of noded segment arrangements.
//!
//! # Algorithm
//!
//! The iterative Hobby snap rounding algorithm proceeds as:
//! 1. Snap all input coordinates to the precision grid.
//! 2. Collect all pairwise segment-segment intersections (cross-line).
//! 3. Snap each intersection to the grid.
//! 4. Split every segment that an intersection point lies on.
//! 5. Repeat steps 2-4 until convergence or `max_iterations` is reached.
//! 6. Remove zero-length segments.
//!
//! # References
//!
//! - J. Hobby, "Practical segment intersection with finite precision output",
//!   *Computational Geometry*, 13(4):199-214, 1999.

use crate::error::{AlgorithmError, Result};
use oxigeo_core::vector::Coordinate;

/// Options controlling snap rounding behaviour.
///
/// All output coordinates produced by [`snap_round`] are exact multiples of
/// `precision` on both axes.
#[derive(Debug, Clone)]
pub struct SnapRoundingOptions {
    /// Grid cell size (epsilon).
    ///
    /// All output coordinates are multiples of this value.
    /// Must be strictly positive. Typical values:
    /// - `1e-6` for geographic (degree) coordinates
    /// - `0.01` for centimetre-precision projected coordinates
    /// - `1.0` for integer grid coordinates
    pub precision: f64,

    /// Maximum propagation iterations (default `8`).
    ///
    /// Each pass may introduce new intersection points that themselves need
    /// snapping and may lie on other segments. The algorithm terminates when
    /// no new points are found or this limit is reached.
    pub max_iterations: usize,
}

impl Default for SnapRoundingOptions {
    fn default() -> Self {
        Self {
            precision: 1e-6,
            max_iterations: 8,
        }
    }
}

/// A single output segment after snap rounding.
///
/// Both endpoints are guaranteed to lie on the precision grid.
#[derive(Debug, Clone, PartialEq)]
pub struct SnappedSegment {
    /// Start vertex (on grid).
    pub start: Coordinate,
    /// End vertex (on grid).
    pub end: Coordinate,
    /// Index of the source polyline this segment was derived from.
    pub source_line: usize,
}

/// Result returned by [`snap_round`].
#[derive(Debug, Clone)]
pub struct SnapRoundingResult {
    /// All output segments — noded and snapped to the precision grid.
    pub segments: Vec<SnappedSegment>,
    /// Total number of unique intersection points inserted during propagation.
    pub intersections_added: usize,
    /// Number of propagation iterations actually performed.
    pub iterations: usize,
}

// ---------------------------------------------------------------------------
// Internal working segment type
// ---------------------------------------------------------------------------

/// An internal mutable segment used during the snap-rounding computation.
#[derive(Debug, Clone)]
struct WorkSegment {
    start: Coordinate,
    end: Coordinate,
    source_line: usize,
}

// ---------------------------------------------------------------------------
// Low-level grid helpers
// ---------------------------------------------------------------------------

/// Snap (x, y) to the nearest multiple of `prec` on each axis.
#[inline]
fn snap_to_grid(x: f64, y: f64, prec: f64) -> (f64, f64) {
    ((x / prec).round() * prec, (y / prec).round() * prec)
}

/// Return true when two coordinates coincide within half a grid cell.
#[inline]
fn coords_equal_grid(a: &Coordinate, b: &Coordinate, prec: f64) -> bool {
    let half = prec * 0.5;
    (a.x - b.x).abs() < half && (a.y - b.y).abs() < half
}

/// Test whether `p` lies on the open or closed segment `[a, b]` within the
/// tolerance implied by `prec`.
///
/// The check uses:
/// 1. The cross product (signed area) must be near zero — collinearity test.
/// 2. The dot product must place `p` between `a` and `b` along the segment
///    direction — endpoint inclusivity test.
#[inline]
fn point_on_segment(p: &Coordinate, a: &Coordinate, b: &Coordinate, prec: f64) -> bool {
    // Cross product of (b-a) × (p-a).  For p on the line this is 0.
    let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    // Tolerance for cross product: we allow up to `prec` absolute deviation.
    if cross.abs() > prec {
        return false;
    }
    // Dot product of (p-a) · (b-a) must be in [0, |b-a|²].
    let dot = (p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y);
    let len2 = (b.x - a.x).powi(2) + (b.y - a.y).powi(2);
    dot >= -prec && dot <= len2 + prec
}

/// Split a [`WorkSegment`] at point `p`, returning the two halves.
#[inline]
fn split_segment_at(seg: &WorkSegment, p: &Coordinate) -> (WorkSegment, WorkSegment) {
    (
        WorkSegment {
            start: seg.start,
            end: *p,
            source_line: seg.source_line,
        },
        WorkSegment {
            start: *p,
            end: seg.end,
            source_line: seg.source_line,
        },
    )
}

// ---------------------------------------------------------------------------
// Public API — coordinate-level helpers
// ---------------------------------------------------------------------------

/// Snap a single coordinate to the precision grid.
///
/// Each axis is independently rounded to the nearest multiple of `precision`.
///
/// # Panics
///
/// Does not panic. Returns `NaN` coordinates only if `precision` is zero or
/// `NaN` (callers are expected to validate `precision > 0`).
///
/// # Examples
///
/// ```
/// use oxigeo_algorithms::vector::{snap_coordinate, Coordinate};
///
/// let c = Coordinate::new_2d(1.23456789, -0.99999999);
/// let snapped = snap_coordinate(&c, 1e-6);
/// assert!((snapped.x - 1.234568).abs() < 1e-12);
/// ```
#[must_use]
pub fn snap_coordinate(c: &Coordinate, precision: f64) -> Coordinate {
    let (sx, sy) = snap_to_grid(c.x, c.y, precision);
    Coordinate::new_2d(sx, sy)
}

/// Snap all coordinates in a slice to the precision grid, removing consecutive
/// duplicates that arise after snapping.
///
/// Duplicate detection uses an exact equality check on the snapped (already
/// grid-aligned) values.
///
/// # Examples
///
/// ```
/// use oxigeo_algorithms::vector::{snap_linestring, Coordinate};
///
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(0.0000001, 0.0000001),  // collapses to same as first
///     Coordinate::new_2d(1.0, 0.0),
/// ];
/// let snapped = snap_linestring(&coords, 1e-6);
/// assert_eq!(snapped.len(), 2);
/// ```
#[must_use]
pub fn snap_linestring(coords: &[Coordinate], precision: f64) -> Vec<Coordinate> {
    let mut result: Vec<Coordinate> = Vec::with_capacity(coords.len());
    for c in coords {
        let s = snap_coordinate(c, precision);
        // Remove consecutive duplicates (exact equality after grid snapping).
        if result.last().is_none_or(|prev| {
            (prev.x - s.x).abs() > f64::EPSILON || (prev.y - s.y).abs() > f64::EPSILON
        }) {
            result.push(s);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Core snap-rounding algorithm
// ---------------------------------------------------------------------------

/// Perform iterative Hobby snap rounding on a collection of polylines.
///
/// Each element of `lines` is a polyline given as a `Vec<Coordinate>`.
/// The algorithm nodes the arrangement — every pair of segments from
/// *different* source lines is checked for intersection — and guarantees
/// that all vertices in the output lie on the precision grid.
///
/// # Errors
///
/// Returns [`AlgorithmError::InvalidParameter`] if `options.precision` is not
/// strictly positive.
///
/// # Complexity
///
/// The brute-force intersection search is O(n²) per iteration where *n* is
/// the total segment count.  This is appropriate for typical geospatial inputs
/// with up to tens of thousands of segments.
pub fn snap_round(
    lines: &[Vec<Coordinate>],
    options: &SnapRoundingOptions,
) -> Result<SnapRoundingResult> {
    if options.precision <= 0.0 || options.precision.is_nan() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "precision",
            message: format!(
                "precision must be strictly positive, got {}",
                options.precision
            ),
        });
    }
    let prec = options.precision;

    // ------------------------------------------------------------------
    // Step 1: Initial snapping — snap every vertex to the grid.
    // ------------------------------------------------------------------
    let mut segments: Vec<WorkSegment> = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let snapped = snap_linestring(line, prec);
        // Collect pairwise segments from consecutive snapped vertices.
        for pair in snapped.windows(2) {
            let start = pair[0];
            let end = pair[1];
            // Skip zero-length segments that emerged from initial snapping.
            if coords_equal_grid(&start, &end, prec) {
                continue;
            }
            segments.push(WorkSegment {
                start,
                end,
                source_line: line_idx,
            });
        }
    }

    // ------------------------------------------------------------------
    // Steps 2-5: Iterative intersection propagation.
    // ------------------------------------------------------------------
    let mut total_intersections_added: usize = 0;
    let mut iterations_done: usize = 0;

    for _iter in 0..options.max_iterations {
        let new_pts = collect_cross_intersections(&segments, prec);
        if new_pts.is_empty() {
            break;
        }
        total_intersections_added += new_pts.len();
        segments = insert_intersection_points(segments, &new_pts, prec);
        iterations_done += 1;
    }

    // ------------------------------------------------------------------
    // Step 6: Remove zero-length segments from the final list.
    // ------------------------------------------------------------------
    let output: Vec<SnappedSegment> = segments
        .into_iter()
        .filter(|seg| !coords_equal_grid(&seg.start, &seg.end, prec))
        .map(|seg| SnappedSegment {
            start: seg.start,
            end: seg.end,
            source_line: seg.source_line,
        })
        .collect();

    Ok(SnapRoundingResult {
        segments: output,
        intersections_added: total_intersections_added,
        iterations: iterations_done,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers for the iterative algorithm
// ---------------------------------------------------------------------------

/// Collect all unique pairwise intersection points between segments that belong
/// to *different* source lines.
///
/// Each intersection is snapped to the grid before deduplication.
fn collect_cross_intersections(segments: &[WorkSegment], prec: f64) -> Vec<Coordinate> {
    use crate::vector::intersection::{SegmentIntersection, intersect_segment_segment};

    let mut found: Vec<Coordinate> = Vec::new();

    let n = segments.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let a = &segments[i];
            let b = &segments[j];
            // Only consider pairs from different source lines.
            if a.source_line == b.source_line {
                continue;
            }
            match intersect_segment_segment(&a.start, &a.end, &b.start, &b.end) {
                SegmentIntersection::Point(pt) => {
                    let (sx, sy) = snap_to_grid(pt.x, pt.y, prec);
                    let snapped = Coordinate::new_2d(sx, sy);
                    // Deduplicate: skip if already recorded.
                    if !found.iter().any(|p| coords_equal_grid(p, &snapped, prec)) {
                        found.push(snapped);
                    }
                }
                SegmentIntersection::Overlap(c1, c2) => {
                    // For overlaps, record both endpoint-snaps.
                    for &raw in &[c1, c2] {
                        let (sx, sy) = snap_to_grid(raw.x, raw.y, prec);
                        let snapped = Coordinate::new_2d(sx, sy);
                        if !found.iter().any(|p| coords_equal_grid(p, &snapped, prec)) {
                            found.push(snapped);
                        }
                    }
                }
                SegmentIntersection::None => {}
            }
        }
    }
    found
}

/// For each point in `new_pts`, split every segment in `segments` that the
/// point lies on.  Returns the updated segment list.
fn insert_intersection_points(
    segments: Vec<WorkSegment>,
    new_pts: &[Coordinate],
    prec: f64,
) -> Vec<WorkSegment> {
    // Process one intersection point at a time, rebuilding the segment list.
    // This ensures that a split segment can itself be split by a later point.
    let mut current = segments;

    for pt in new_pts {
        let mut next: Vec<WorkSegment> = Vec::with_capacity(current.len() + 4);
        for seg in &current {
            // If `pt` is already an endpoint, no split needed.
            let at_start = coords_equal_grid(&seg.start, pt, prec);
            let at_end = coords_equal_grid(&seg.end, pt, prec);
            if at_start || at_end {
                next.push(seg.clone());
                continue;
            }
            // Check whether pt lies strictly inside the segment.
            if point_on_segment(pt, &seg.start, &seg.end, prec) {
                let (left, right) = split_segment_at(seg, pt);
                // Discard degenerate halves.
                if !coords_equal_grid(&left.start, &left.end, prec) {
                    next.push(left);
                }
                if !coords_equal_grid(&right.start, &right.end, prec) {
                    next.push(right);
                }
            } else {
                next.push(seg.clone());
            }
        }
        current = next;
    }

    current
}

// ---------------------------------------------------------------------------
// Tests (unit)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a Coordinate from (x, y)
    fn c(x: f64, y: f64) -> Coordinate {
        Coordinate::new_2d(x, y)
    }

    #[test]
    fn test_snap_coordinate_basic() {
        let coord = c(1.2345678, -9.8765432);
        let snapped = snap_coordinate(&coord, 1e-6);
        // Should be rounded to 6 decimal places
        assert!((snapped.x - 1.234568_f64).abs() < 1e-12);
        assert!((snapped.y - -9.876543_f64).abs() < 1e-12);
    }

    #[test]
    fn test_snap_linestring_no_duplicates() {
        // Two coords that collapse to the same grid point.
        let coords = vec![c(0.0, 0.0), c(1e-9, 1e-9), c(1.0, 0.0)];
        let result = snap_linestring(&coords, 1e-6);
        // The first two snap to (0, 0); only one survives.
        assert_eq!(result.len(), 2);
        assert!((result[0].x).abs() < 1e-12);
        assert!((result[1].x - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_snap_round_invalid_precision() {
        let lines: Vec<Vec<Coordinate>> = vec![vec![c(0.0, 0.0), c(1.0, 1.0)]];
        let opts = SnapRoundingOptions {
            precision: -1.0,
            max_iterations: 4,
        };
        assert!(snap_round(&lines, &opts).is_err());

        let opts_zero = SnapRoundingOptions {
            precision: 0.0,
            max_iterations: 4,
        };
        assert!(snap_round(&lines, &opts_zero).is_err());
    }

    #[test]
    fn test_point_on_segment_midpoint() {
        let a = c(0.0, 0.0);
        let b = c(10.0, 0.0);
        let p = c(5.0, 0.0);
        assert!(point_on_segment(&p, &a, &b, 1e-6));
    }

    #[test]
    fn test_point_on_segment_off_line() {
        let a = c(0.0, 0.0);
        let b = c(10.0, 0.0);
        let p = c(5.0, 1.0); // above the segment
        assert!(!point_on_segment(&p, &a, &b, 1e-6));
    }
}
