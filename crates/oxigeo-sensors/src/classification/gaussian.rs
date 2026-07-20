//! Gaussian Cholesky utilities for Maximum Likelihood Classification
//!
//! Provides hand-rolled Cholesky decomposition and SPD matrix inversion with
//! log-determinant computation. No external linear-algebra dependency is used;
//! matrices are small (typically 3-10 × 3-10 spectral bands) so the O(n³)
//! implementation is perfectly adequate.

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::Array2;

/// Invert a symmetric positive-definite (SPD) matrix via Cholesky decomposition,
/// also returning the natural log of its determinant.
///
/// Tikhonov regularisation `lambda * I` is added to the input before
/// factorisation, ensuring numerical stability when the raw sample covariance
/// is rank-deficient (e.g. a single training sample per class).
///
/// # Returns
/// `(sigma_inv, log_det)` where
/// - `sigma_inv` is the `n×n` inverse of `matrix + lambda*I`
/// - `log_det` is `ln |matrix + lambda*I|`
///
/// # Errors
/// Returns [`SensorError::SingularCovariance`] with the column index at which
/// the Cholesky diagonal became non-positive.
pub(crate) fn invert_spd_with_logdet(
    matrix: &Array2<f64>,
    lambda: f64,
) -> Result<(Array2<f64>, f64)> {
    let n = matrix.nrows();
    assert_eq!(n, matrix.ncols(), "covariance matrix must be square");

    // A = matrix + lambda * I  (regularised copy)
    let mut a = matrix.to_owned();
    for i in 0..n {
        a[[i, i]] += lambda;
    }

    // ---- Cholesky decomposition: A = L * L^T  (lower-triangular L) ----
    let mut l = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        for j in 0..=i {
            let mut sum = a[[i, j]];
            for k in 0..j {
                sum -= l[[i, k]] * l[[j, k]];
            }
            if i == j {
                if sum <= 0.0 {
                    return Err(SensorError::singular_covariance(i));
                }
                l[[i, j]] = sum.sqrt();
            } else {
                l[[i, j]] = sum / l[[j, j]];
            }
        }
    }

    // ln|A| = 2 * Σ ln(L_ii)
    let mut log_det = 0.0_f64;
    for i in 0..n {
        log_det += l[[i, i]].ln();
    }
    log_det *= 2.0;

    // ---- A^{-1} via two triangular solves per column ----
    //
    // For each standard basis vector e_col we solve:
    //   L * y  = e_col   (forward substitution)
    //   L^T * x = y      (back substitution)
    // The resulting x is the col-th column of A^{-1}.
    let mut inv = Array2::<f64>::zeros((n, n));
    let mut y = vec![0.0_f64; n];
    let mut x = vec![0.0_f64; n];

    for col in 0..n {
        // --- forward substitution: L * y = e_col ---
        for (i, yi) in y.iter_mut().enumerate().take(n) {
            *yi = if i == col { 1.0 } else { 0.0 };
        }
        for i in 0..n {
            let mut s = y[i];
            for k in 0..i {
                s -= l[[i, k]] * y[k];
            }
            y[i] = s / l[[i, i]];
        }

        // --- back substitution: L^T * x = y ---
        for i in (0..n).rev() {
            let mut s = y[i];
            for k in (i + 1)..n {
                s -= l[[k, i]] * x[k];
            }
            x[i] = s / l[[i, i]];
        }

        for i in 0..n {
            inv[[i, col]] = x[i];
        }
    }

    Ok((inv, log_det))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_identity_2x2() {
        // I^{-1} = I, log|I| = 0
        let id = Array2::<f64>::eye(2);
        let (inv, log_det) =
            invert_spd_with_logdet(&id, 0.0).expect("identity matrix is always SPD");
        assert_abs_diff_eq!(log_det, 0.0, epsilon = 1e-12);
        assert_abs_diff_eq!(inv[[0, 0]], 1.0, epsilon = 1e-12);
        assert_abs_diff_eq!(inv[[1, 1]], 1.0, epsilon = 1e-12);
        assert_abs_diff_eq!(inv[[0, 1]], 0.0, epsilon = 1e-12);
    }

    #[test]
    fn test_diagonal_3x3() {
        // diag(2, 3, 4)^{-1} = diag(0.5, 1/3, 0.25), log_det = ln(24)
        let m = array![[2.0_f64, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
        let (inv, log_det) =
            invert_spd_with_logdet(&m, 0.0).expect("diagonal positive matrix is SPD");
        assert_abs_diff_eq!(log_det, (24.0_f64).ln(), epsilon = 1e-12);
        assert_abs_diff_eq!(inv[[0, 0]], 0.5, epsilon = 1e-12);
        assert_abs_diff_eq!(inv[[1, 1]], 1.0 / 3.0, epsilon = 1e-12);
        assert_abs_diff_eq!(inv[[2, 2]], 0.25, epsilon = 1e-12);
    }

    #[test]
    fn test_singular_without_regularisation_succeeds_with_lambda() {
        // All-zero matrix is singular, but lambda=1e-6 makes it 1e-6 * I.
        let zero = Array2::<f64>::zeros((2, 2));
        let result = invert_spd_with_logdet(&zero, 1e-6);
        assert!(
            result.is_ok(),
            "Tikhonov regularisation should save singular matrix"
        );
    }

    #[test]
    fn test_inverse_times_original_is_identity() {
        // A * A^{-1} ≈ I for a 3×3 SPD matrix.
        let m = array![[4.0_f64, 2.0, 1.0], [2.0, 5.0, 2.0], [1.0, 2.0, 6.0]];
        let (inv, _) = invert_spd_with_logdet(&m, 0.0).expect("known SPD matrix must invert");
        let product = m.dot(&inv);
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert_abs_diff_eq!(product[[i, j]], expected, epsilon = 1e-10);
            }
        }
    }
}
