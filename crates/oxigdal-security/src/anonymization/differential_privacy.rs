//! Differential privacy.

use rand::Rng;

/// Laplace mechanism for differential privacy.
pub struct LaplaceMechanism {
    epsilon: f64,
    sensitivity: f64,
}

impl LaplaceMechanism {
    /// Create new Laplace mechanism.
    pub fn new(epsilon: f64, sensitivity: f64) -> Self {
        Self {
            epsilon,
            sensitivity,
        }
    }

    /// Add noise to a value.
    pub fn add_noise(&self, value: f64) -> f64 {
        let scale = self.sensitivity / self.epsilon;
        let noise = self.sample_laplace(scale);
        value + noise
    }

    fn sample_laplace(&self, scale: f64) -> f64 {
        let mut rng = rand::thread_rng();
        let u: f64 = rng.gen_range(-0.5..0.5);
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
    pub fn new(epsilon: f64, delta: f64, sensitivity: f64) -> Self {
        Self {
            epsilon,
            delta,
            sensitivity,
        }
    }

    /// Add noise to a value.
    pub fn add_noise(&self, value: f64) -> f64 {
        let sigma = self.sensitivity * (2.0 * (1.25 / self.delta).ln()).sqrt() / self.epsilon;
        let mut rng = rand::thread_rng();
        let noise: f64 = rng.sample(rand_distr::Normal::new(0.0, sigma).expect("Invalid sigma"));
        value + noise
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_laplace_mechanism() {
        let mechanism = LaplaceMechanism::new(1.0, 1.0);
        let original = 100.0;
        let noisy = mechanism.add_noise(original);

        // Noise should be different
        assert!((original - noisy).abs() < 50.0); // Reasonable bound
    }
}
