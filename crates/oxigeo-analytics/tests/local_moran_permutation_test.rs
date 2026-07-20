//! Integration tests for Local Moran's I with conditional permutation inference.
//!
//! Tests cover: cluster significance, outlier detection, p-value bounds,
//! determinism under fixed seed, consistency with the analytical `calculate`
//! method, and the LisaClass re-export from the hotspot module.

use approx::assert_abs_diff_eq;
use oxigeo_analytics::hotspot::{LisaClass, LocalMoransI, SpatialWeights};
use scirs2_core::ndarray::{Array1, Array2};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build row-standardized 4-connectivity (rook) weights for a `rows x cols` grid.
///
/// Cell (r, c) maps to index `r * cols + c`.
#[allow(clippy::expect_used)]
fn grid_weights_4(rows: usize, cols: usize) -> SpatialWeights {
    let n = rows * cols;
    let mut mat = Array2::zeros((n, n));

    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            if r > 0 {
                mat[[i, (r - 1) * cols + c]] = 1.0;
            }
            if r + 1 < rows {
                mat[[i, (r + 1) * cols + c]] = 1.0;
            }
            if c > 0 {
                mat[[i, r * cols + (c - 1)]] = 1.0;
            }
            if c + 1 < cols {
                mat[[i, r * cols + (c + 1)]] = 1.0;
            }
        }
    }

    let mut sw = SpatialWeights::from_adjacency(mat)
        .expect("grid_weights_4: adjacency matrix should be square");
    sw.row_standardize();
    sw
}

/// Build a simple n-node chain (line graph) adjacency, row-standardized.
#[allow(clippy::expect_used)]
fn chain_weights(n: usize) -> SpatialWeights {
    let mut mat = Array2::zeros((n, n));
    for i in 0..n - 1 {
        mat[[i, i + 1]] = 1.0;
        mat[[i + 1, i]] = 1.0;
    }
    let mut sw = SpatialWeights::from_adjacency(mat)
        .expect("chain_weights: adjacency matrix should be square");
    sw.row_standardize();
    sw
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A 5x5 spatial field with an extremely strong HH cluster in the top-left
/// quadrant should produce HH classifications with permutation p-values well
/// below 0.05 at the cluster interior.
///
/// Layout (5x5 grid, index = row * 5 + col):
///  [100 100 100  1   1 ]   row 0: idx  0  1  2  3  4
///  [100 100 100  1   1 ]   row 1: idx  5  6  7  8  9
///  [100 100   1  1   1 ]   row 2: idx 10 11 12 13 14
///  [  1   1   1  1   1 ]   row 3: idx 15 ...
///  [  1   1   1  1   1 ]   row 4: idx 20 ...
///
/// Cell 6 = (1,1): 4 neighbours all at 100. Very strong HH signal.
#[test]
fn test_permutation_hh_cluster_significant() {
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![
        100.0, 100.0, 100.0, 1.0, 1.0, // row 0
        100.0, 100.0, 100.0, 1.0, 1.0, // row 1
        100.0, 100.0, 1.0, 1.0, 1.0, // row 2
        1.0, 1.0, 1.0, 1.0, 1.0, // row 3
        1.0, 1.0, 1.0, 1.0, 1.0, // row 4
    ]);
    let weights = grid_weights_4(5, 5);
    let li = LocalMoransI::new(0.05);

    let result = li
        .calculate_with_permutations(&values.view(), &weights, 999, 42)
        .expect("permutation calculation should succeed for 5x5 HH cluster");

    // Cell 6 = (row 1, col 1): all 4 neighbours are 100 → strong HH
    assert_eq!(
        result.classifications[6],
        LisaClass::HH,
        "cell (1,1) idx=6 surrounded by high values should be HH, p={}",
        result.p_values[6]
    );
    // Also check cell 1 = (row 0, col 1): 3 high neighbours
    assert_eq!(
        result.classifications[1],
        LisaClass::HH,
        "cell (0,1) idx=1 surrounded by high values should be HH, p={}",
        result.p_values[1]
    );
}

/// LL cluster significance achieved through extreme low outliers on a
/// predominantly-high background.
///
/// Statistical insight: for conditional permutation, LL cells achieve small
/// p-values when z_i (the cell's standardised value) is LARGE in magnitude
/// (far below mean). This requires the field to be mostly HIGH with a few
/// extreme LOW outliers, so that:
///
/// 1. z_low is large negative (mean is pulled up by the many HIGH values), and
/// 2. z_high is small positive (many near-mean HIGH cells), so permutations
///    that draw HIGH neighbours give small-magnitude I_perm (not extreme).
///
/// Design: 10x10 grid, 90 cells at 10 (HIGH), 10 cells at 0 (LOW) in
/// bottom-right 2×5 block (rows 8-9, cols 5-9 → indices 85-89, 95-99).
///
/// Mean = (90*10 + 10*0)/100 = 9.0; std = 3.0.
/// z_high = (10-9)/3 ≈ 0.333; z_low = (0-9)/3 = -3.0.
/// I_obs for interior LL = z_low^2 = 9.  Permutation only exceeds this if
/// all 4 drawn neighbours are LOW: P ≈ C(9,4)/C(99,4) ≈ 0.003% << 5%.
#[test]
fn test_permutation_ll_cluster_significant() {
    // 10x10 grid: rows 0-7 + cols 0-4 of rows 8-9 = 90 HIGH (10.0)
    // rows 8-9, cols 5-9 = 10 LOW (0.0) forming a 2×5 block in bottom-right.
    let mut vals = vec![10.0f64; 100];
    for r in 8..10_usize {
        for c in 5..10_usize {
            vals[r * 10 + c] = 0.0;
        }
    }
    let values = Array1::from_vec(vals);
    let weights = grid_weights_4(10, 10);
    let li = LocalMoransI::new(0.05);

    let result = li
        .calculate_with_permutations(&values.view(), &weights, 999, 42)
        .expect("permutation calculation should succeed for extreme-outlier LL test");

    // Interior LL cell: (8,6) = index 86.  Its 4 neighbours:
    // (7,6)=76=HIGH, (9,6)=96=LOW, (8,5)=85=LOW, (8,7)=87=LOW.
    // So 1 HIGH and 3 LOW neighbours → I_obs = z_low * (0.25*z_high + 0.75*z_low)
    //                                        ≈ (-3) * (0.083 - 2.25) = (-3)*(-2.167) ≈ 6.5
    // For permutations: the threshold I_obs ≈ 6.5 is very hard to exceed by
    // chance (would need ≥3 LOW drawn from pool of 9 LOW / 99 total).

    // Cell (9,7) = index 97: all 4 neighbours are LOW (96,98,87=10... wait).
    // Actually (8,7) = 87 = LOW, (9,6) = 96 = LOW, (9,8) = 98 = LOW. But
    // (8,7) is row8 col7 → LOW (in the 2×5 block: rows 8-9, cols 5-9). Yes.
    // So cell (9,7) = 97: neighbours are (8,7)=87=LOW, (9,6)=96=LOW, (9,8)=98=LOW.
    // Only 3 neighbours (edge cell). All are LOW.
    // Use cell (8,6) = 86 which is interior with 1 HIGH + 3 LOW neighbours.
    // OR use (8,7) = 87: neighbours are (7,7)=77=HIGH, (9,7)=97=LOW, (8,6)=86=LOW, (8,8)=88=LOW.
    // Cell 87 has 1 HIGH, 3 LOW → same situation.

    // The strongest LL signal: we need ALL neighbours to be LOW.
    // Cell (9,6) = 96: neighbours are (8,6)=86=LOW, (9,5)=95=LOW, (9,7)=97=LOW. (3 neighbours, all LOW)
    // Cell (9,7) = 97: neighbours are (8,7)=87=LOW, (9,6)=96=LOW, (9,8)=98=LOW. (3 neighbours, all LOW)
    // Both are edge cells with 3 all-LOW neighbours.

    // P(all 3 drawn LOW from pool of 99 with 9 LOW) = C(9,3)/C(99,3) = 84/152096 ≈ 0.055%
    // → definitely significant at alpha=0.05.

    assert_eq!(
        result.classifications[97],
        LisaClass::LL,
        "cell (9,7)=index 97 with all-LOW neighbours should be LL, p={}",
        result.p_values[97]
    );
}

/// A single very high-value cell in a chain of low-value cells should be
/// classified as HL (or at minimum not HH, since its neighbours are low).
#[test]
fn test_permutation_hl_outlier_classified() {
    // Chain of 11 nodes: 1 1 1 1 1 1000 1 1 1 1 1 — index 5 is the extreme outlier
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![
        1.0, 1.0, 1.0, 1.0, 1.0, 1000.0, 1.0, 1.0, 1.0, 1.0, 1.0,
    ]);
    let weights = chain_weights(11);
    let li = LocalMoransI::new(0.05);

    let result = li
        .calculate_with_permutations(&values.view(), &weights, 999, 123)
        .expect("permutation calculation should succeed for HL outlier scenario");

    let cls = result.classifications[5];
    // High value surrounded by low neighbours → HL or NotSignificant (never HH).
    assert!(
        cls == LisaClass::HL || cls == LisaClass::NotSignificant,
        "isolated high outlier should be HL or NotSignificant, got {:?}",
        cls
    );
    // The high-value outlier must NEVER be classified HH.
    assert_ne!(result.classifications[5], LisaClass::HH);
}

/// A near-uniform field should produce mostly NotSignificant classifications.
#[test]
fn test_permutation_random_field_mostly_not_significant() {
    // Near-constant values with very small perturbations — no real clustering.
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![
        5.0, 5.1, 4.9, 5.0, 5.2, 4.8, 5.0, 5.1, 4.9, 5.0, 5.2, 4.8,
    ]);
    let weights = chain_weights(12);
    let li = LocalMoransI::new(0.05);

    let result = li
        .calculate_with_permutations(&values.view(), &weights, 999, 7)
        .expect("permutation calculation should succeed for near-uniform field");

    let n_significant = result
        .classifications
        .iter()
        .filter(|&&c| c != LisaClass::NotSignificant)
        .count();

    // For a near-uniform field, very few cells should reach significance.
    assert!(
        n_significant <= 2,
        "near-uniform field should have at most 2 significant cells, got {}",
        n_significant
    );
}

/// All pseudo p-values must lie in the closed interval [1/(M+1), 1].
#[test]
fn test_permutation_pseudo_pvalue_in_unit_interval() {
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    let weights = grid_weights_4(3, 3);
    let li = LocalMoransI::new(0.05);
    let m = 99_usize;

    let result = li
        .calculate_with_permutations(&values.view(), &weights, m, 17)
        .expect("permutation calculation should succeed for p-value range test");

    let min_possible = 1.0 / (m as f64 + 1.0);
    for (i, &pv) in result.p_values.iter().enumerate() {
        assert!(
            pv >= min_possible - 1e-15 && pv <= 1.0 + 1e-15,
            "p_values[{}] = {} is out of [{}, 1]",
            i,
            pv,
            min_possible
        );
    }
}

/// The minimum pseudo p-value satisfies 1/(M+1) <= min_p <= alpha, confirming
/// that at least the most significant cells in a strongly polarised field reach
/// statistical significance and that the formula's lower bound is respected.
///
/// The formula guarantees min_p >= 1/(M+1); additionally, for a strongly
/// polarised field, some cells should achieve p < 0.05.
#[test]
fn test_permutation_pseudo_pvalue_min_respects_lower_bound() {
    // Strong 5x5 HH cluster: 8 cells at 1000, 17 cells at 1.
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![
        1000.0, 1000.0, 1000.0, 1000.0, 1000.0, // row 0: all high
        1000.0, 1000.0, 1000.0, 1.0, 1.0, // row 1
        1000.0, 1.0, 1.0, 1.0, 1.0, // row 2
        1.0, 1.0, 1.0, 1.0, 1.0, // row 3
        1.0, 1.0, 1.0, 1.0, 1.0, // row 4
    ]);
    let weights = grid_weights_4(5, 5);
    let li = LocalMoransI::new(0.05);
    let m = 999_usize;

    let result = li
        .calculate_with_permutations(&values.view(), &weights, m, 31)
        .expect("permutation calculation should succeed for lower-bound test");

    let min_pv = result
        .p_values
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let lower_bound = 1.0 / (m as f64 + 1.0);

    // The formula guarantees no p-value can be smaller than 1/(M+1).
    assert!(
        min_pv >= lower_bound - 1e-15,
        "minimum p-value {} must be >= 1/(M+1) = {}",
        min_pv,
        lower_bound
    );

    // With a strong cluster and 999 permutations, at least some HH cells must
    // reach significance at alpha=0.05.
    assert!(
        min_pv < 0.05,
        "minimum p-value {} should be < 0.05 for a strongly polarised field",
        min_pv
    );
}

/// Given the same inputs and the same seed, two calls must produce identical results.
#[test]
fn test_permutation_deterministic_with_fixed_seed() {
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![1.0, 3.0, 2.0, 8.0, 7.0, 9.0, 2.0, 1.0, 3.0]);
    let weights = grid_weights_4(3, 3);
    let li = LocalMoransI::new(0.05);

    let r1 = li
        .calculate_with_permutations(&values.view(), &weights, 99, 42)
        .expect("first determinism call should succeed");
    let r2 = li
        .calculate_with_permutations(&values.view(), &weights, 99, 42)
        .expect("second determinism call should succeed");

    for i in 0..values.len() {
        assert_abs_diff_eq!(r1.p_values[i], r2.p_values[i], epsilon = 1e-15);
        assert_abs_diff_eq!(r1.z_scores[i], r2.z_scores[i], epsilon = 1e-15);
        assert_abs_diff_eq!(r1.local_i[i], r2.local_i[i], epsilon = 1e-15);
        assert_eq!(r1.classifications[i], r2.classifications[i]);
    }
}

/// More permutations should not flip the significance conclusion for a strong
/// cluster: M=99 and M=999 must agree on whether the core HH cell is significant.
#[test]
fn test_permutation_more_permutations_stabilizes_pvalue() {
    // Strong 5x5 HH cluster
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![
        100.0, 100.0, 100.0, 1.0, 1.0, 100.0, 100.0, 100.0, 1.0, 1.0, 100.0, 100.0, 1.0, 1.0, 1.0,
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
    ]);
    let weights = grid_weights_4(5, 5);
    let li = LocalMoransI::new(0.05);

    let r_small = li
        .calculate_with_permutations(&values.view(), &weights, 99, 42)
        .expect("99-permutation run should succeed");
    let r_large = li
        .calculate_with_permutations(&values.view(), &weights, 999, 42)
        .expect("999-permutation run should succeed");

    // Cell 6 = (row 1, col 1) is the strong HH interior cell.
    let sig_small = r_small.p_values[6] < 0.05;
    let sig_large = r_large.p_values[6] < 0.05;
    assert_eq!(
        sig_small, sig_large,
        "core HH cell significance should agree between M=99 and M=999: p99={}, p999={}",
        r_small.p_values[6], r_large.p_values[6]
    );
}

/// The `local_i` field from `calculate_with_permutations` must match the
/// `local_i` from the analytical `calculate` method to floating-point precision,
/// because both compute the same observed statistic formula.
#[test]
fn test_permutation_local_i_matches_analytical_calculate() {
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![1.0, 1.0, 1.0, 10.0, 10.0, 10.0, 1.0, 1.0, 1.0]);
    let weights = grid_weights_4(3, 3);
    let li = LocalMoransI::new(0.05);

    let r_analytical = li
        .calculate(&values.view(), &weights)
        .expect("analytical calculate should succeed");
    let r_perm = li
        .calculate_with_permutations(&values.view(), &weights, 99, 42)
        .expect("permutation calculate should succeed");

    for i in 0..values.len() {
        assert_abs_diff_eq!(r_perm.local_i[i], r_analytical.local_i[i], epsilon = 1e-12);
    }
}

/// This test verifies at compile time and runtime that `LisaClass` is
/// re-exported from `oxigeo_analytics::hotspot` (not just `moran`).
#[test]
fn test_lisa_class_reexported_from_hotspot_module() {
    // If LisaClass is not re-exported this file will not compile.
    let hh: LisaClass = LisaClass::HH;
    let ll: LisaClass = LisaClass::LL;
    let hl: LisaClass = LisaClass::HL;
    let lh: LisaClass = LisaClass::LH;
    let ns: LisaClass = LisaClass::NotSignificant;

    assert_ne!(hh, ll);
    assert_ne!(hl, lh);
    assert_ne!(ns, hh);
}

/// Passing a `values` slice whose length differs from the weights dimension must
/// return a `DimensionMismatch` error and never panic.
#[test]
fn test_permutation_dimension_mismatch_errors() {
    // weights is 3x3 = 9 nodes; values has only 5 elements
    #[allow(clippy::expect_used)]
    let values = Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    let weights = grid_weights_4(3, 3); // n=9
    let li = LocalMoransI::new(0.05);

    let result = li.calculate_with_permutations(&values.view(), &weights, 99, 42);
    assert!(
        result.is_err(),
        "dimension mismatch should return Err, not Ok"
    );
}

/// n < 3 must return an InsufficientData error.
#[test]
fn test_permutation_insufficient_data_error() {
    let values = Array1::from_vec(vec![1.0, 2.0]);
    let weights = chain_weights(2);
    let li = LocalMoransI::new(0.05);

    let result = li.calculate_with_permutations(&values.view(), &weights, 99, 42);
    assert!(
        result.is_err(),
        "n=2 should return Err(InsufficientData), not Ok"
    );
}

/// n_permutations == 0 must return an InsufficientData error.
#[test]
fn test_permutation_zero_permutations_error() {
    let values = Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    let weights = chain_weights(5);
    let li = LocalMoransI::new(0.05);

    let result = li.calculate_with_permutations(&values.view(), &weights, 0, 42);
    assert!(
        result.is_err(),
        "n_permutations=0 should return Err, not Ok"
    );
}
