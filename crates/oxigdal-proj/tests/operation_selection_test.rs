//! Integration tests for `oxigdal_proj::operation_selection`.
//!
//! Covers 13 scenarios listed in Slice 18 / W1.

use oxigdal_proj::area_of_use::AreaOfUse;
use oxigdal_proj::operation_selection::{
    CandidateOperation, area_coverage_fraction, operation_score, rank_operations,
    select_best_operation,
};
use oxigdal_proj::transform::BoundingBox;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn global_aou() -> AreaOfUse {
    AreaOfUse::new(-180.0, -90.0, 180.0, 90.0, "World")
}

fn europe_aou() -> AreaOfUse {
    AreaOfUse::new(-10.0, 35.0, 40.0, 72.0, "Europe")
}

fn north_america_aou() -> AreaOfUse {
    AreaOfUse::new(-170.0, 15.0, -50.0, 85.0, "North America")
}

fn make_op(name: &str, accuracy_m: f64, aou: Option<AreaOfUse>) -> CandidateOperation {
    CandidateOperation {
        name: name.to_string(),
        accuracy_m,
        area_of_use: aou,
        source_id: None,
    }
}

// ── 1. Score: better accuracy ranks higher ────────────────────────────────────

#[test]
fn test_operation_score_higher_for_better_accuracy() {
    let s_good = operation_score(0.5, 1.0);
    let s_bad = operation_score(5.0, 1.0);
    assert!(
        s_good > s_bad,
        "0.5 m should score higher than 5.0 m: {s_good} vs {s_bad}"
    );
}

// ── 2. Score: better coverage ranks higher ────────────────────────────────────

#[test]
fn test_operation_score_higher_for_better_coverage() {
    let s_high = operation_score(1.0, 0.9);
    let s_low = operation_score(1.0, 0.5);
    assert!(
        s_high > s_low,
        "0.9 coverage should score higher than 0.5: {s_high} vs {s_low}"
    );
}

// ── 3. Score: identity operation is infinite ──────────────────────────────────

#[test]
fn test_operation_score_identity_is_infinite() {
    let s = operation_score(0.0, 1.0);
    assert_eq!(s, f64::INFINITY, "accuracy=0.0 must yield INFINITY");
}

// ── 4. Single candidate always selected ──────────────────────────────────────

#[test]
fn test_select_best_single_candidate() {
    let candidates = vec![make_op("only", 1.0, Some(global_aou()))];
    assert_eq!(select_best_operation(&candidates, None), Some(0));
}

// ── 5. Empty slice returns None ───────────────────────────────────────────────

#[test]
fn test_select_best_empty_returns_none() {
    let candidates: Vec<CandidateOperation> = vec![];
    assert_eq!(select_best_operation(&candidates, None), None);
}

// ── 6. Higher accuracy wins when coverage is equal ───────────────────────────

#[test]
fn test_select_best_chooses_higher_accuracy_when_coverage_equal() {
    let candidates = vec![
        make_op("precise", 0.5, Some(global_aou())), // index 0
        make_op("coarse", 5.0, Some(global_aou())),  // index 1
    ];
    let best = select_best_operation(&candidates, None);
    assert_eq!(best, Some(0), "0.5 m (idx 0) should beat 5.0 m (idx 1)");
}

// ── 7. Better coverage wins when accuracy is equal ───────────────────────────

#[test]
fn test_select_best_chooses_better_coverage_when_accuracy_equal() {
    // Query: central Europe.
    let query = BoundingBox::new(0.0, 45.0, 10.0, 55.0).expect("valid bbox");

    let candidates = vec![
        make_op("eu_op", 1.0, Some(europe_aou())), // index 0 — full coverage
        make_op("na_op", 1.0, Some(north_america_aou())), // index 1 — disjoint
    ];

    let best = select_best_operation(&candidates, Some(&query));
    assert_eq!(best, Some(0), "EU operation should win for European query");
}

// ── 8. Low-coverage candidates are filtered out ───────────────────────────────

#[test]
fn test_select_best_filters_low_coverage_candidates() {
    let query = BoundingBox::new(5.0, 48.0, 15.0, 55.0).expect("valid bbox");

    let asia_aou = AreaOfUse::new(70.0, 10.0, 150.0, 60.0, "East Asia");

    let candidates = vec![
        make_op("asia_op", 0.1, Some(asia_aou)), // index 0 — disjoint → filtered
        make_op("eu_op", 1.0, Some(europe_aou())), // index 1 — covers query
    ];

    let best = select_best_operation(&candidates, Some(&query));
    assert_eq!(
        best,
        Some(1),
        "Asia op filtered; EU op (idx 1) should be selected"
    );
}

// ── 9. No query_bbox → accuracy-only ranking ─────────────────────────────────

#[test]
fn test_select_best_no_query_bbox_uses_accuracy_only() {
    let japan_aou = AreaOfUse::new(120.0, 20.0, 150.0, 50.0, "Japan");

    let candidates = vec![
        make_op("japan_precise", 0.3, Some(japan_aou)), // index 0: best accuracy
        make_op("global_rough", 10.0, None),            // index 1
    ];

    let best = select_best_operation(&candidates, None);
    assert_eq!(
        best,
        Some(0),
        "without bbox, best accuracy (0.3 m) wins regardless of AOU"
    );
}

// ── 10. rank_operations returns sorted descending ────────────────────────────

#[test]
fn test_rank_operations_returns_sorted_descending() {
    let candidates = vec![
        make_op("coarse", 50.0, None), // index 0
        make_op("fine", 0.5, None),    // index 1
        make_op("medium", 10.0, None), // index 2
    ];

    let rankings = rank_operations(&candidates, None);
    assert_eq!(rankings.len(), 3, "all 3 candidates should appear");

    for w in rankings.windows(2) {
        assert!(
            w[0].score >= w[1].score,
            "rankings must be descending: {} >= {}",
            w[0].score,
            w[1].score
        );
    }
}

// ── 11. Full overlap → coverage = 1.0 ────────────────────────────────────────

#[test]
fn test_area_coverage_fraction_full_overlap_returns_one() {
    let aou = AreaOfUse::new(-20.0, 30.0, 50.0, 80.0, "Large region");
    let query = BoundingBox::new(0.0, 45.0, 10.0, 55.0).expect("valid bbox");
    let frac = area_coverage_fraction(&aou, &query);
    assert!(
        (frac - 1.0).abs() < 1e-9,
        "query fully inside AOU should yield 1.0, got {frac}"
    );
}

// ── 12. No overlap → coverage = 0.0 ──────────────────────────────────────────

#[test]
fn test_area_coverage_fraction_no_overlap_returns_zero() {
    let aou = europe_aou(); // [-10,35,40,72]
    let query = BoundingBox::new(100.0, 20.0, 140.0, 50.0).expect("valid bbox"); // East Asia
    let frac = area_coverage_fraction(&aou, &query);
    assert!(
        frac < 1e-9,
        "disjoint rectangles should yield 0.0, got {frac}"
    );
}

// ── 13. Partial overlap → coverage ≈ 0.5 ─────────────────────────────────────

#[test]
fn test_area_coverage_fraction_partial_overlap() {
    // AOU: [10, 45] → [40, 72]
    // Query: [0, 45] → [20, 55]
    // Intersection: [10, 45] → [20, 55] = 10 × 10 = 100
    // Query area: 20 × 10 = 200
    // Fraction: 0.5
    let aou = AreaOfUse::new(10.0, 45.0, 40.0, 72.0, "Partial AOU");
    let query = BoundingBox::new(0.0, 45.0, 20.0, 55.0).expect("valid bbox");
    let frac = area_coverage_fraction(&aou, &query);
    assert!(
        (frac - 0.5).abs() < 1e-9,
        "half-overlapping bboxes should yield ~0.5, got {frac}"
    );
}
