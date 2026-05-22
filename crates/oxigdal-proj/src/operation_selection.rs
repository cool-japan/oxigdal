//! Coordinate operation selection by accuracy and area-of-use coverage.
//!
//! This module implements ranked selection of coordinate operations (datum shifts,
//! projections, pipelines) based on declared accuracy in metres and the fraction
//! of a query bounding box covered by each operation's area of use.
//!
//! The primary entry points are [`select_best_operation`] and [`rank_operations`].
//! Both accept a slice of [`CandidateOperation`] and an optional query bounding box.
//!
//! # Scoring
//!
//! Each candidate is assigned a score via [`operation_score`]:
//!
//! ```text
//! score = ln(1 + coverage³ / accuracy_m.max(1e-9))
//! ```
//!
//! Special case: `accuracy_m == 0.0` (identity / perfect) yields `f64::INFINITY`,
//! which always sorts first.
//!
//! # Filtering
//!
//! When a `query_bbox` is provided, candidates whose area-of-use covers less than
//! 1 % of the query are discarded (unless the candidate declares no area of use,
//! in which case it is treated as global and retains coverage 1.0).
//!
//! When no `query_bbox` is provided, ranking is by accuracy alone (smallest positive
//! value first; identity / zero first).

use crate::area_of_use::AreaOfUse;
use crate::transform::BoundingBox;
use core::cmp::Ordering;

/// A candidate coordinate operation that can be ranked for selection.
#[derive(Debug, Clone)]
pub struct CandidateOperation {
    /// Human-readable name identifying the operation (e.g. EPSG pipeline ID).
    pub name: String,
    /// Declared accuracy of the operation in metres.
    ///
    /// `0.0` indicates a perfect / identity transformation (lossless).
    /// Positive values indicate approximate accuracy; larger = less accurate.
    pub accuracy_m: f64,
    /// Geographic area over which this operation is valid.
    ///
    /// `None` means the operation is considered globally applicable.
    pub area_of_use: Option<AreaOfUse>,
    /// Optional source identifier (e.g. EPSG operation code, NTv2 grid filename).
    pub source_id: Option<String>,
}

/// Ranking record produced for a single [`CandidateOperation`].
#[derive(Debug, Clone)]
pub struct OperationRanking {
    /// Index into the original `candidates` slice.
    pub candidate_idx: usize,
    /// Effective accuracy used for scoring (same as `CandidateOperation::accuracy_m`).
    pub effective_accuracy_m: f64,
    /// Fraction of the query bounding box area covered by this operation's
    /// area of use.  `1.0` if no area of use is declared (global), or if
    /// no query bbox was provided.
    pub coverage_fraction: f64,
    /// Composite score computed by [`operation_score`].  Higher is better.
    /// `f64::INFINITY` for identity operations (accuracy 0.0).
    pub score: f64,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns the index of the best candidate operation, or `None` if the slice is
/// empty or no candidate passes the coverage threshold.
///
/// Selection strategy:
/// - When `query_bbox` is `Some`: filter out candidates whose coverage fraction
///   is below 0.01, then pick the one with the highest [`operation_score`].
/// - When `query_bbox` is `None`: pick the candidate with the smallest
///   `accuracy_m` (ties broken by index order).  Identity (0.0) is always best.
///
/// # Examples
///
/// ```rust
/// use oxigdal_proj::operation_selection::{CandidateOperation, select_best_operation};
///
/// let candidates = vec![
///     CandidateOperation { name: "op_a".into(), accuracy_m: 1.0, area_of_use: None, source_id: None },
///     CandidateOperation { name: "op_b".into(), accuracy_m: 5.0, area_of_use: None, source_id: None },
/// ];
/// let best = select_best_operation(&candidates, None);
/// assert_eq!(best, Some(0)); // 1.0 m < 5.0 m
/// ```
pub fn select_best_operation(
    candidates: &[CandidateOperation],
    query_bbox: Option<&BoundingBox>,
) -> Option<usize> {
    let ranked = rank_operations(candidates, query_bbox);
    ranked.into_iter().next().map(|r| r.candidate_idx)
}

/// Returns all candidates as [`OperationRanking`] records, sorted by score descending.
///
/// When `query_bbox` is `Some`, candidates with `coverage_fraction < 0.01` are
/// excluded from the result entirely.
///
/// When `query_bbox` is `None`, every candidate is ranked purely by accuracy.
pub fn rank_operations(
    candidates: &[CandidateOperation],
    query_bbox: Option<&BoundingBox>,
) -> Vec<OperationRanking> {
    match query_bbox {
        Some(bbox) => rank_with_bbox(candidates, bbox),
        None => rank_accuracy_only(candidates),
    }
}

/// Computes the fraction of `query` covered by `candidate_aou`.
///
/// Returns `1.0` if:
/// - `query` has zero area (point query) and the centre lies inside `candidate_aou`.
///
/// Returns `0.0` if the rectangles are disjoint.
///
/// Otherwise returns `intersection_area / query_area`, clamped to `[0.0, 1.0]`.
///
/// Coordinates are treated as rectilinear geographic degrees; no spherical
/// correction is applied (consistent with the rest of the area-of-use API).
pub fn area_coverage_fraction(candidate_aou: &AreaOfUse, query: &BoundingBox) -> f64 {
    let west = query.min_x;
    let south = query.min_y;
    let east = query.max_x;
    let north = query.max_y;

    let query_area = (east - west) * (north - south);

    if query_area <= 0.0 {
        // Point (or degenerate line) query — check if centre is inside AOU.
        let cx = (west + east) / 2.0;
        let cy = (south + north) / 2.0;
        if point_in_aou(candidate_aou, cx, cy) {
            1.0
        } else {
            0.0
        }
    } else {
        let inter_w = west.max(candidate_aou.west);
        let inter_s = south.max(candidate_aou.south);
        let inter_e = east.min(candidate_aou.east);
        let inter_n = north.min(candidate_aou.north);

        let inter_area = (inter_e - inter_w).max(0.0) * (inter_n - inter_s).max(0.0);
        (inter_area / query_area).min(1.0)
    }
}

/// Computes the composite score for an operation.
///
/// ```text
/// score = ln(1 + coverage³ / accuracy_m.max(1e-9))
/// ```
///
/// When `accuracy_m == 0.0` (identity / perfect), returns `f64::INFINITY`.
///
/// `coverage` is expected to be in `[0.0, 1.0]`.
pub fn operation_score(accuracy_m: f64, coverage: f64) -> f64 {
    if accuracy_m == 0.0 {
        return f64::INFINITY;
    }
    let cov_cubed = coverage * coverage * coverage;
    let denom = accuracy_m.max(1e-9_f64);
    // ln_1p(x) = ln(1 + x)
    (cov_cubed / denom).ln_1p()
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Rank when a query bbox is available.
fn rank_with_bbox(candidates: &[CandidateOperation], bbox: &BoundingBox) -> Vec<OperationRanking> {
    let mut rankings: Vec<OperationRanking> = candidates
        .iter()
        .enumerate()
        .filter_map(|(idx, candidate)| {
            let coverage = match &candidate.area_of_use {
                None => 1.0_f64, // global — always full coverage
                Some(aou) => area_coverage_fraction(aou, bbox),
            };

            // Discard if coverage is below the 1 % threshold.
            if coverage < 0.01 {
                return None;
            }

            let score = operation_score(candidate.accuracy_m, coverage);
            Some(OperationRanking {
                candidate_idx: idx,
                effective_accuracy_m: candidate.accuracy_m,
                coverage_fraction: coverage,
                score,
            })
        })
        .collect();

    rankings.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

    rankings
}

/// Rank by accuracy only (no spatial filtering).
fn rank_accuracy_only(candidates: &[CandidateOperation]) -> Vec<OperationRanking> {
    let mut rankings: Vec<OperationRanking> = candidates
        .iter()
        .enumerate()
        .map(|(idx, candidate)| {
            // When no bbox is provided, coverage is defined as 1.0 for scoring
            // purposes, but the score itself is not meaningful — we rank by
            // accuracy_m directly.  We still compute the score so the struct is
            // populated consistently.
            let score = operation_score(candidate.accuracy_m, 1.0);
            OperationRanking {
                candidate_idx: idx,
                effective_accuracy_m: candidate.accuracy_m,
                coverage_fraction: 1.0,
                score,
            }
        })
        .collect();

    // Sort: identity (INFINITY) first, then smallest accuracy_m ascending.
    rankings.sort_by(|a, b| {
        // Primary: score descending (handles INFINITY for identity).
        let score_ord = b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal);
        if score_ord != Ordering::Equal {
            return score_ord;
        }
        // Secondary: accuracy ascending.
        a.effective_accuracy_m
            .partial_cmp(&b.effective_accuracy_m)
            .unwrap_or(Ordering::Equal)
    });

    rankings
}

/// Returns `true` if the geographic point `(lon, lat)` is inside `aou`.
///
/// This delegates to the existing `AreaOfUse::contains` implementation which
/// handles antimeridian crossing.
fn point_in_aou(aou: &AreaOfUse, lon: f64, lat: f64) -> bool {
    aou.contains(lon, lat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::BoundingBox;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn global_aou() -> AreaOfUse {
        AreaOfUse::new(-180.0, -90.0, 180.0, 90.0, "World")
    }

    fn europe_aou() -> AreaOfUse {
        AreaOfUse::new(-10.0, 35.0, 40.0, 72.0, "Europe")
    }

    fn north_america_aou() -> AreaOfUse {
        AreaOfUse::new(-170.0, 15.0, -50.0, 85.0, "North America")
    }

    fn make_candidate(name: &str, accuracy_m: f64, aou: Option<AreaOfUse>) -> CandidateOperation {
        CandidateOperation {
            name: name.to_string(),
            accuracy_m,
            area_of_use: aou,
            source_id: None,
        }
    }

    // ── Score tests ───────────────────────────────────────────────────────────

    #[test]
    fn test_operation_score_higher_for_better_accuracy() {
        let score_good = operation_score(0.5, 1.0);
        let score_bad = operation_score(5.0, 1.0);
        assert!(
            score_good > score_bad,
            "0.5 m accuracy should score higher than 5.0 m: {score_good} vs {score_bad}"
        );
    }

    #[test]
    fn test_operation_score_higher_for_better_coverage() {
        let score_high = operation_score(1.0, 0.9);
        let score_low = operation_score(1.0, 0.5);
        assert!(
            score_high > score_low,
            "0.9 coverage should score higher than 0.5: {score_high} vs {score_low}"
        );
    }

    #[test]
    fn test_operation_score_identity_is_infinite() {
        let score = operation_score(0.0, 1.0);
        assert_eq!(
            score,
            f64::INFINITY,
            "identity accuracy (0.0) must yield INFINITY"
        );
    }

    // ── select_best_operation tests ───────────────────────────────────────────

    #[test]
    fn test_select_best_single_candidate() {
        let candidates = vec![make_candidate("only", 1.0, Some(global_aou()))];
        let result = select_best_operation(&candidates, None);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_select_best_empty_returns_none() {
        let candidates: Vec<CandidateOperation> = vec![];
        let result = select_best_operation(&candidates, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_select_best_chooses_higher_accuracy_when_coverage_equal() {
        // Both candidates have the same global AOU; accuracy differs.
        let candidates = vec![
            make_candidate("precise", 0.5, Some(global_aou())), // index 0
            make_candidate("coarse", 5.0, Some(global_aou())),  // index 1
        ];
        // No bbox → rank by accuracy only.
        let result = select_best_operation(&candidates, None);
        assert_eq!(
            result,
            Some(0),
            "0.5 m candidate (idx 0) should win over 5.0 m (idx 1)"
        );
    }

    #[test]
    fn test_select_best_chooses_better_coverage_when_accuracy_equal() {
        // Same accuracy, different coverage fraction relative to a European query.
        let query = BoundingBox::new(0.0, 45.0, 10.0, 55.0).expect("valid bbox");

        let candidates = vec![
            // Europe AOU — covers the query fully.
            make_candidate("eu_full", 1.0, Some(europe_aou())), // index 0
            // North America AOU — disjoint from European query.
            make_candidate("na", 1.0, Some(north_america_aou())), // index 1
        ];

        let result = select_best_operation(&candidates, Some(&query));
        // North America is disjoint → coverage 0 → filtered out.
        assert_eq!(
            result,
            Some(0),
            "European operation should win for European query"
        );
    }

    #[test]
    fn test_select_best_filters_low_coverage_candidates() {
        // Query bbox is in Europe; one candidate covers Europe, the other is
        // restricted entirely to East Asia (disjoint).
        let query = BoundingBox::new(5.0, 48.0, 15.0, 55.0).expect("valid bbox");

        let asia_aou = AreaOfUse::new(70.0, 10.0, 150.0, 60.0, "East Asia");

        let candidates = vec![
            make_candidate("asia_op", 0.1, Some(asia_aou)), // index 0 — disjoint → filtered
            make_candidate("eu_op", 1.0, Some(europe_aou())), // index 1 — covers query
        ];

        let result = select_best_operation(&candidates, Some(&query));
        assert_eq!(
            result,
            Some(1),
            "Asia operation should be filtered; EU operation (idx 1) should win"
        );
    }

    #[test]
    fn test_select_best_no_query_bbox_uses_accuracy_only() {
        // One candidate has a restricted AOU, the other is global — but without
        // a query bbox the AOU is irrelevant; only accuracy matters.
        let japan_aou = AreaOfUse::new(120.0, 20.0, 150.0, 50.0, "Japan");

        let candidates = vec![
            make_candidate("japan_precise", 0.3, Some(japan_aou)), // index 0: best accuracy
            make_candidate("global_rough", 10.0, None),            // index 1
        ];

        let result = select_best_operation(&candidates, None);
        assert_eq!(
            result,
            Some(0),
            "without bbox, accuracy determines winner: japan_precise (0.3 m) should win"
        );
    }

    // ── rank_operations tests ─────────────────────────────────────────────────

    #[test]
    fn test_rank_operations_returns_sorted_descending() {
        let candidates = vec![
            make_candidate("coarse_global", 50.0, None), // index 0
            make_candidate("fine_global", 0.5, None),    // index 1
            make_candidate("medium_global", 10.0, None), // index 2
        ];

        let rankings = rank_operations(&candidates, None);
        assert_eq!(rankings.len(), 3, "all 3 candidates should be ranked");

        for window in rankings.windows(2) {
            let a = &window[0];
            let b = &window[1];
            assert!(
                a.score >= b.score,
                "rankings must be sorted descending by score: {} vs {}",
                a.score,
                b.score
            );
        }
    }

    // ── area_coverage_fraction tests ──────────────────────────────────────────

    #[test]
    fn test_area_coverage_fraction_full_overlap_returns_one() {
        // Query entirely inside the AOU.
        let aou = AreaOfUse::new(-20.0, 30.0, 50.0, 80.0, "Large region");
        let query = BoundingBox::new(0.0, 45.0, 10.0, 55.0).expect("valid bbox");
        let frac = area_coverage_fraction(&aou, &query);
        assert!(
            (frac - 1.0).abs() < 1e-9,
            "query fully inside AOU should yield 1.0, got {frac}"
        );
    }

    #[test]
    fn test_area_coverage_fraction_no_overlap_returns_zero() {
        // AOU is Europe; query is in East Asia — no overlap.
        let aou = europe_aou();
        let query = BoundingBox::new(100.0, 20.0, 140.0, 50.0).expect("valid bbox");
        let frac = area_coverage_fraction(&aou, &query);
        assert!(
            frac < 1e-9,
            "disjoint rectangles should yield 0.0, got {frac}"
        );
    }

    #[test]
    fn test_area_coverage_fraction_partial_overlap() {
        // Query spans [0, 45] → [20, 55]; AOU covers [10, 45] → [40, 72].
        // Intersection: [10, 45] → [20, 55].
        // Intersection area: 10 * 10 = 100.
        // Query area: 20 * 10 = 200.
        // Expected fraction: 0.5.
        let aou = AreaOfUse::new(10.0, 45.0, 40.0, 72.0, "Partial AOU");
        let query = BoundingBox::new(0.0, 45.0, 20.0, 55.0).expect("valid bbox");
        let frac = area_coverage_fraction(&aou, &query);
        assert!(
            (frac - 0.5).abs() < 1e-9,
            "half-overlapping bboxes should yield ~0.5, got {frac}"
        );
    }
}
