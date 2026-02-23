//! Resampling kernel functions
//!
//! This module provides kernel weight calculation functions used by various
//! resampling algorithms.

#![allow(dead_code)] // Many kernels reserved for future resampling algorithms

use core::f64::consts::PI;

/// Computes Lanczos kernel weight
///
/// The Lanczos kernel is defined as:
/// L(x) = sinc(x) * sinc(x/a) for |x| < a
/// L(x) = 0 otherwise
///
/// where sinc(x) = sin(πx) / (πx) for x ≠ 0, and sinc(0) = 1
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `a` - Lobe count (typically 2 or 3)
#[must_use]
#[inline]
pub fn lanczos(x: f64, a: usize) -> f64 {
    let a_f64 = a as f64;
    let abs_x = x.abs();

    if abs_x < f64::EPSILON {
        1.0
    } else if abs_x < a_f64 {
        let pi_x = PI * abs_x;
        let sinc_x = pi_x.sin() / pi_x;
        let sinc_xa = (pi_x / a_f64).sin() / (pi_x / a_f64);
        sinc_x * sinc_xa
    } else {
        0.0
    }
}

/// Computes cubic convolution kernel weight (Catmull-Rom cubic spline)
///
/// The cubic kernel is defined as:
/// C(x) = (a+2)|x|³ - (a+3)|x|² + 1                    for |x| ≤ 1
/// C(x) = a|x|³ - 5a|x|² + 8a|x| - 4a                  for 1 < |x| < 2
/// C(x) = 0                                            for |x| ≥ 2
///
/// where a = -0.5 (Catmull-Rom), a = -0.75 (sharper), a = -1.0 (softer)
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `a` - Sharpness parameter (typically -0.5 for Catmull-Rom)
#[must_use]
#[inline]
pub fn cubic(x: f64, a: f64) -> f64 {
    let abs_x = x.abs();

    if abs_x <= 1.0 {
        ((a + 2.0) * abs_x - (a + 3.0)) * abs_x * abs_x + 1.0
    } else if abs_x < 2.0 {
        ((a * abs_x - 5.0 * a) * abs_x + 8.0 * a) * abs_x - 4.0 * a
    } else {
        0.0
    }
}

/// Computes bilinear (triangle) kernel weight
///
/// The bilinear kernel is simply:
/// B(x) = 1 - |x| for |x| < 1
/// B(x) = 0 otherwise
///
/// # Arguments
///
/// * `x` - Distance from sample point
#[must_use]
#[inline]
pub fn bilinear(x: f64) -> f64 {
    let abs_x = x.abs();
    if abs_x < 1.0 { 1.0 - abs_x } else { 0.0 }
}

/// Computes Mitchell-Netravali kernel weight
///
/// A parametric cubic filter with parameters B and C:
/// - B=1, C=0: Cubic B-Spline (maximum smoothness)
/// - B=0, C=0.5: Catmull-Rom (good balance)
/// - B=1/3, C=1/3: Mitchell-Netravali (recommended default)
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `b` - B parameter
/// * `c` - C parameter
#[must_use]
#[inline]
pub fn mitchell_netravali(x: f64, b: f64, c: f64) -> f64 {
    let abs_x = x.abs();

    if abs_x < 1.0 {
        let x2 = abs_x * abs_x;
        let x3 = x2 * abs_x;
        ((12.0 - 9.0 * b - 6.0 * c) * x3 + (-18.0 + 12.0 * b + 6.0 * c) * x2 + (6.0 - 2.0 * b))
            / 6.0
    } else if abs_x < 2.0 {
        let x2 = abs_x * abs_x;
        let x3 = x2 * abs_x;
        ((-b - 6.0 * c) * x3
            + (6.0 * b + 30.0 * c) * x2
            + (-12.0 * b - 48.0 * c) * abs_x
            + (8.0 * b + 24.0 * c))
            / 6.0
    } else {
        0.0
    }
}

/// Computes Gaussian kernel weight
///
/// G(x) = exp(-(x/sigma)²/2) / (sigma * sqrt(2π))
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `sigma` - Standard deviation (width of the Gaussian)
#[must_use]
#[inline]
pub fn gaussian(x: f64, sigma: f64) -> f64 {
    let x_norm = x / sigma;
    let coeff = 1.0 / (sigma * (2.0 * PI).sqrt());
    coeff * (-0.5 * x_norm * x_norm).exp()
}

/// Computes sinc kernel weight (for windowed sinc filters)
///
/// sinc(x) = sin(πx) / (πx) for x ≠ 0
/// sinc(0) = 1
///
/// # Arguments
///
/// * `x` - Distance from sample point
#[must_use]
#[inline]
pub fn sinc(x: f64) -> f64 {
    if x.abs() < f64::EPSILON {
        1.0
    } else {
        let pi_x = PI * x;
        pi_x.sin() / pi_x
    }
}

/// Computes blackman window weight
///
/// Blackman window is used to taper filters at the edges:
/// W(x) = 0.42 + 0.5*cos(πx/a) + 0.08*cos(2πx/a) for |x| < a
/// W(x) = 0 otherwise
///
/// Maximum is at x=0 (center), tapering to 0 at |x|=a (edges).
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `a` - Window width
#[must_use]
#[inline]
pub fn blackman_window(x: f64, a: f64) -> f64 {
    let abs_x = x.abs();
    if abs_x < a {
        let ratio = abs_x / a;
        let pi_ratio = PI * ratio;
        0.42 + 0.5 * pi_ratio.cos() + 0.08 * (2.0 * pi_ratio).cos()
    } else {
        0.0
    }
}

/// Computes Hamming window weight
///
/// Hamming window for tapering:
/// W(x) = 0.54 + 0.46*cos(πx/a) for |x| < a
/// W(x) = 0 otherwise
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `a` - Window width
#[must_use]
#[inline]
pub fn hamming_window(x: f64, a: f64) -> f64 {
    let abs_x = x.abs();
    if abs_x < a {
        0.54 + 0.46 * (PI * abs_x / a).cos()
    } else {
        0.0
    }
}

/// Computes Hann window weight
///
/// Hann (Hanning) window for tapering:
/// W(x) = 0.5 * (1 + cos(πx/a)) for |x| < a
/// W(x) = 0 otherwise
///
/// # Arguments
///
/// * `x` - Distance from sample point
/// * `a` - Window width
#[must_use]
#[inline]
pub fn hann_window(x: f64, a: f64) -> f64 {
    let abs_x = x.abs();
    if abs_x < a {
        0.5 * (1.0 + (PI * abs_x / a).cos())
    } else {
        0.0
    }
}

/// Normalizes a set of kernel weights so they sum to 1.0
///
/// This ensures that resampling doesn't change the overall brightness/values
/// of the image.
///
/// # Arguments
///
/// * `weights` - Mutable slice of weights to normalize
pub fn normalize_weights(weights: &mut [f64]) {
    let sum: f64 = weights.iter().sum();
    if sum.abs() > f64::EPSILON {
        let inv_sum = 1.0 / sum;
        for weight in weights {
            *weight *= inv_sum;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_lanczos() {
        // At 0, should be 1.0
        assert_abs_diff_eq!(lanczos(0.0, 3), 1.0, epsilon = 1e-10);

        // At integer positions (except 0), should be 0
        assert_abs_diff_eq!(lanczos(1.0, 3), 0.0, epsilon = 1e-10);
        assert_abs_diff_eq!(lanczos(2.0, 3), 0.0, epsilon = 1e-10);

        // Outside support, should be 0
        assert_eq!(lanczos(3.5, 3), 0.0);
        assert_eq!(lanczos(-3.5, 3), 0.0);

        // Symmetric
        assert_abs_diff_eq!(lanczos(0.5, 3), lanczos(-0.5, 3), epsilon = 1e-10);
    }

    #[test]
    fn test_cubic() {
        // At 0, should be 1.0
        assert_abs_diff_eq!(cubic(0.0, -0.5), 1.0, epsilon = 1e-10);

        // Symmetric
        assert_abs_diff_eq!(cubic(0.5, -0.5), cubic(-0.5, -0.5), epsilon = 1e-10);

        // Outside support, should be 0
        assert_eq!(cubic(2.5, -0.5), 0.0);
        assert_eq!(cubic(-2.5, -0.5), 0.0);
    }

    #[test]
    fn test_bilinear() {
        // At 0, should be 1.0
        assert_eq!(bilinear(0.0), 1.0);

        // Linear falloff
        assert_eq!(bilinear(0.5), 0.5);
        assert_eq!(bilinear(-0.5), 0.5);

        // Outside support, should be 0
        assert_eq!(bilinear(1.5), 0.0);
        assert_eq!(bilinear(-1.5), 0.0);
    }

    #[test]
    fn test_mitchell_netravali() {
        let b = 1.0 / 3.0;
        let c = 1.0 / 3.0;

        // At 0, with b=c=1/3, the formula gives (16 - 12)/18 = 4/18 + 6/9 = 8/9
        assert_abs_diff_eq!(mitchell_netravali(0.0, b, c), 8.0 / 9.0, epsilon = 1e-10);

        // Symmetric
        assert_abs_diff_eq!(
            mitchell_netravali(0.5, b, c),
            mitchell_netravali(-0.5, b, c),
            epsilon = 1e-10
        );

        // Outside support, should be 0
        assert_eq!(mitchell_netravali(2.5, b, c), 0.0);
    }

    #[test]
    fn test_sinc() {
        // At 0, should be 1.0
        assert_abs_diff_eq!(sinc(0.0), 1.0, epsilon = 1e-10);

        // At integer positions (except 0), should be 0
        assert_abs_diff_eq!(sinc(1.0), 0.0, epsilon = 1e-10);
        assert_abs_diff_eq!(sinc(-1.0), 0.0, epsilon = 1e-10);
        assert_abs_diff_eq!(sinc(2.0), 0.0, epsilon = 1e-10);

        // Symmetric
        assert_abs_diff_eq!(sinc(0.5), sinc(-0.5), epsilon = 1e-10);
    }

    #[test]
    fn test_gaussian() {
        let sigma = 1.0;

        // At 0, should be maximum
        let center = gaussian(0.0, sigma);
        assert!(center > gaussian(1.0, sigma));
        assert!(center > gaussian(2.0, sigma));

        // Symmetric
        assert_abs_diff_eq!(gaussian(1.0, sigma), gaussian(-1.0, sigma), epsilon = 1e-10);
    }

    #[test]
    fn test_normalize_weights() {
        let mut weights = [0.1, 0.2, 0.3, 0.4];
        normalize_weights(&mut weights);

        let sum: f64 = weights.iter().sum();
        assert_abs_diff_eq!(sum, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_windows() {
        // Test blackman window - should be maximum at center (x=0), taper to 0 at edges
        let center = blackman_window(0.0, 2.0);
        let halfway = blackman_window(1.0, 2.0);
        let outside = blackman_window(3.0, 2.0);

        assert!(
            center > halfway,
            "Blackman window should be maximum at center"
        );
        assert!(
            halfway > 0.0,
            "Blackman window should be positive inside window"
        );
        assert_eq!(
            outside, 0.0,
            "Blackman window should be zero outside window"
        );

        // Verify symmetry
        assert!((blackman_window(1.0, 2.0) - blackman_window(-1.0, 2.0)).abs() < 1e-10);

        // Test hamming window - maximum at center, taper to edges
        assert!(
            hamming_window(0.0, 2.0) > hamming_window(1.0, 2.0),
            "Hamming window should be maximum at center"
        );
        assert_eq!(
            hamming_window(3.0, 2.0),
            0.0,
            "Hamming window should be zero outside window"
        );

        // Verify symmetry
        assert!((hamming_window(0.5, 2.0) - hamming_window(-0.5, 2.0)).abs() < 1e-10);

        // Test hann window - maximum at center, taper to edges
        assert!(
            hann_window(0.0, 2.0) > hann_window(1.0, 2.0),
            "Hann window should be maximum at center"
        );
        assert_eq!(
            hann_window(3.0, 2.0),
            0.0,
            "Hann window should be zero outside window"
        );

        // Verify symmetry
        assert!((hann_window(0.75, 2.0) - hann_window(-0.75, 2.0)).abs() < 1e-10);
    }
}
