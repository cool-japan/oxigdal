//! Differential privacy.

use crate::error::{Result, SecurityError};
use scirs2_core::random::prelude::{Normal, thread_rng};

/// Laplace mechanism for differential privacy.
pub struct LaplaceMechanism {
    epsilon: f64,
    sensitivity: f64,
}

impl LaplaceMechanism {
    /// Create new Laplace mechanism.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::InvalidInput`] if `epsilon` is not finite
    /// and strictly positive, or if `sensitivity` is not finite and
    /// non-negative.
    pub fn new(epsilon: f64, sensitivity: f64) -> Result<Self> {
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(SecurityError::invalid_input(format!(
                "epsilon must be finite and > 0.0, got {epsilon}"
            )));
        }
        if !sensitivity.is_finite() || sensitivity < 0.0 {
            return Err(SecurityError::invalid_input(format!(
                "sensitivity must be finite and >= 0.0, got {sensitivity}"
            )));
        }
        Ok(Self {
            epsilon,
            sensitivity,
        })
    }

    /// Add noise to a value.
    pub fn add_noise(&self, value: f64) -> f64 {
        let scale = self.sensitivity / self.epsilon;
        let noise = self.sample_laplace(scale);
        value + noise
    }

    fn sample_laplace(&self, scale: f64) -> f64 {
        let mut rng = thread_rng();
        let u: f64 = rng.random_range(-0.5..0.5);
        -scale * u.signum() * (1.0 - 2.0 * u.abs()).ln()
    }
}

/// Gaussian mechanism for differential privacy.
pub struct GaussianMechanism {
    epsilon: f64,
    delta: f64,
    sensitivity: f64,
}

impl GaussianMechanism {
    /// Create new Gaussian mechanism.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::InvalidInput`] if `epsilon` is not finite
    /// and strictly positive, if `delta` is not in the open interval
    /// `(0.0, 1.0)` (required for the analytic Gaussian mechanism formula
    /// `sigma = sensitivity * sqrt(2 * ln(1.25 / delta)) / epsilon` to be
    /// well-defined), or if `sensitivity` is not finite and non-negative.
    pub fn new(epsilon: f64, delta: f64, sensitivity: f64) -> Result<Self> {
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(SecurityError::invalid_input(format!(
                "epsilon must be finite and > 0.0, got {epsilon}"
            )));
        }
        if !delta.is_finite() || delta <= 0.0 || delta >= 1.0 {
            return Err(SecurityError::invalid_input(format!(
                "delta must be finite and in the open interval (0.0, 1.0), got {delta}"
            )));
        }
        if !sensitivity.is_finite() || sensitivity < 0.0 {
            return Err(SecurityError::invalid_input(format!(
                "sensitivity must be finite and >= 0.0, got {sensitivity}"
            )));
        }
        Ok(Self {
            epsilon,
            delta,
            sensitivity,
        })
    }

    /// Add noise to a value.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityError::Internal`] if the computed noise standard
    /// deviation is not a valid, finite, positive value (this should not
    /// happen given the validation performed in [`GaussianMechanism::new`],
    /// but is checked defensively rather than panicking).
    pub fn add_noise(&self, value: f64) -> Result<f64> {
        let sigma = self.sensitivity * (2.0 * (1.25 / self.delta).ln()).sqrt() / self.epsilon;
        if !sigma.is_finite() || sigma < 0.0 {
            return Err(SecurityError::internal(format!(
                "computed Gaussian noise sigma is invalid: {sigma}"
            )));
        }
        let mut rng = thread_rng();
        let normal = Normal::new(0.0, sigma).map_err(|e| {
            SecurityError::internal(format!("failed to build Normal distribution: {e}"))
        })?;
        let noise: f64 = rng.sample(normal);
        Ok(value + noise)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_laplace_mechanism() {
        let mechanism = LaplaceMechanism::new(1.0, 1.0).expect("valid parameters");
        let original = 100.0;
        let noisy = mechanism.add_noise(original);

        // Noise should be different
        assert!((original - noisy).abs() < 50.0); // Reasonable bound
    }

    #[test]
    fn test_laplace_mechanism_rejects_non_positive_epsilon() {
        assert!(LaplaceMechanism::new(0.0, 1.0).is_err());
        assert!(LaplaceMechanism::new(-1.0, 1.0).is_err());
        assert!(LaplaceMechanism::new(f64::NAN, 1.0).is_err());
    }

    #[test]
    fn test_laplace_mechanism_rejects_invalid_sensitivity() {
        assert!(LaplaceMechanism::new(1.0, -1.0).is_err());
        assert!(LaplaceMechanism::new(1.0, f64::INFINITY).is_err());
    }

    #[test]
    fn test_gaussian_mechanism_valid_parameters() {
        let mechanism = GaussianMechanism::new(1.0, 0.5, 1.0).expect("valid parameters");
        let original = 100.0;
        let noisy = mechanism
            .add_noise(original)
            .expect("add_noise should succeed");
        assert!(noisy.is_finite());
    }

    #[test]
    fn test_gaussian_mechanism_rejects_delta_greater_than_1_25() {
        // Regression test: delta >= 1.25 previously made ln(1.25/delta) <= 0,
        // causing sigma to become NaN and the old `.expect("Invalid sigma")`
        // to panic inside add_noise.
        let result = GaussianMechanism::new(1.0, 1.5, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_gaussian_mechanism_rejects_delta_at_or_above_1() {
        assert!(GaussianMechanism::new(1.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn test_gaussian_mechanism_rejects_non_positive_delta() {
        // Regression test: delta <= 0.0 previously made 1.25/delta produce
        // inf/NaN, which propagated into a NaN sigma and a panic.
        assert!(GaussianMechanism::new(1.0, 0.0, 1.0).is_err());
        assert!(GaussianMechanism::new(1.0, -0.1, 1.0).is_err());
    }

    #[test]
    fn test_gaussian_mechanism_rejects_non_positive_epsilon() {
        assert!(GaussianMechanism::new(0.0, 0.5, 1.0).is_err());
        assert!(GaussianMechanism::new(-1.0, 0.5, 1.0).is_err());
    }

    #[test]
    fn test_gaussian_mechanism_rejects_invalid_sensitivity() {
        assert!(GaussianMechanism::new(1.0, 0.5, -1.0).is_err());
        assert!(GaussianMechanism::new(1.0, 0.5, f64::NAN).is_err());
    }
}
