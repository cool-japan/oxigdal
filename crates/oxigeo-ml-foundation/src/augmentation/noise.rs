//! Noise augmentations (Gaussian, salt-and-pepper, etc.).

use crate::augmentation::Augmentation;
use crate::{Error, Result};
use scirs2_core::ndarray::Array3;
use scirs2_core::random::rand_distributions::Distribution;
use scirs2_core::random::{ChaCha8Rng, Normal, Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// Gaussian noise augmentation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GaussianNoise {
    /// Mean of the Gaussian distribution
    pub mean: f32,
    /// Standard deviation of the Gaussian distribution
    pub std: f32,
}

impl GaussianNoise {
    /// Creates a new Gaussian noise augmentation.
    pub fn new(mean: f32, std: f32) -> Result<Self> {
        if std < 0.0 {
            return Err(Error::invalid_parameter("std", std, "must be non-negative"));
        }
        Ok(Self { mean, std })
    }

    /// Adds independent Gaussian noise to every pixel using the supplied RNG.
    ///
    /// Each element receives an independent sample from `N(mean, std²)`. Passing
    /// a seeded RNG (e.g. `ChaCha8Rng::seed_from_u64(..)`) makes the result
    /// deterministic — used by tests to check the sampled statistics — while
    /// [`Augmentation::apply`] seeds from OS entropy for genuine randomness.
    pub fn apply_with_rng<R: Rng>(&self, image: &Array3<f32>, rng: &mut R) -> Result<Array3<f32>> {
        let normal = Normal::new(self.mean, self.std).map_err(|e| {
            Error::Augmentation(format!(
                "invalid Gaussian parameters (mean={}, std={}): {e}",
                self.mean, self.std
            ))
        })?;

        let mut result = image.clone();
        for value in result.iter_mut() {
            *value += normal.sample(rng);
        }
        Ok(result)
    }
}

impl Augmentation for GaussianNoise {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        // Seed a ChaCha8 RNG from OS entropy (Pure Rust, no_std-friendly) so that
        // each invocation draws fresh, independent Gaussian noise per pixel.
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)
            .map_err(|e| Error::Augmentation(format!("getrandom failed in GaussianNoise: {e}")))?;
        let mut rng = ChaCha8Rng::from_seed(seed);
        self.apply_with_rng(image, &mut rng)
    }

    fn name(&self) -> &str {
        "GaussianNoise"
    }

    fn is_random(&self) -> bool {
        true
    }
}

/// Channel dropout (randomly drop entire channels).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ChannelDropout {
    /// Probability of dropping each channel
    pub p: f32,
}

impl ChannelDropout {
    /// Creates a new channel dropout augmentation.
    pub fn new(p: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&p) {
            return Err(Error::invalid_parameter("p", p, "must be in [0, 1]"));
        }
        Ok(Self { p })
    }
}

impl Augmentation for ChannelDropout {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let num_channels = image.shape()[0];
        let mut result = image.clone();

        for c in 0..num_channels {
            // Draw a random f32 in [0, 1) using getrandom entropy.
            let mut buf = [0u8; 4];
            getrandom::fill(&mut buf).map_err(|e| {
                crate::Error::Numerical(format!("getrandom failed in ChannelDropout: {}", e))
            })?;
            // Map the 32-bit integer to [0, 1) with full mantissa resolution.
            let bits = u32::from_ne_bytes(buf);
            // IEEE 754 single: set exponent to 127 (value 1.0–2.0) then subtract 1.0
            let r = f32::from_bits((bits >> 9) | 0x3f80_0000) - 1.0;

            if r < self.p {
                // Zero out the entire channel slice (all H×W pixels)
                let height = image.shape()[1];
                let width = image.shape()[2];
                for h in 0..height {
                    for w in 0..width {
                        result[[c, h, w]] = 0.0;
                    }
                }
            }
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "ChannelDropout"
    }

    fn is_random(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array3;

    #[test]
    fn test_gaussian_noise() {
        let image = Array3::from_elem((3, 2, 2), 0.5);
        let noise = GaussianNoise::new(0.0, 0.1).expect("Failed to create Gaussian noise");
        let result = noise.apply(&image).expect("Failed to apply noise");

        assert_eq!(result.shape(), image.shape());
    }

    #[test]
    fn test_gaussian_noise_statistics() {
        use scirs2_core::random::{ChaCha8Rng, SeedableRng};

        // Zero image, so every output value equals a sampled noise draw.
        let image = Array3::<f32>::from_elem((1, 200, 200), 0.0);
        let noise = GaussianNoise::new(0.5, 2.0).expect("create gaussian noise");

        let mut rng = ChaCha8Rng::seed_from_u64(12345);
        let result = noise
            .apply_with_rng(&image, &mut rng)
            .expect("apply gaussian noise");

        let n = result.len() as f32;
        let mean = result.iter().sum::<f32>() / n;
        let variance = result.iter().map(|&v| (v - mean) * (v - mean)).sum::<f32>() / n;
        let std = variance.sqrt();

        // 40,000 samples: standard error of the mean is std/200 = 0.01.
        assert!((mean - 0.5).abs() < 0.1, "sample mean {mean} not near 0.5");
        assert!((std - 2.0).abs() < 0.1, "sample std {std} not near 2.0");
    }

    #[test]
    fn test_gaussian_noise_deterministic_with_seed() {
        use scirs2_core::random::{ChaCha8Rng, SeedableRng};

        let image = Array3::<f32>::from_elem((2, 4, 4), 1.0);
        let noise = GaussianNoise::new(0.0, 0.5).expect("create gaussian noise");

        let mut rng_a = ChaCha8Rng::seed_from_u64(7);
        let mut rng_b = ChaCha8Rng::seed_from_u64(7);
        let a = noise.apply_with_rng(&image, &mut rng_a).expect("apply a");
        let b = noise.apply_with_rng(&image, &mut rng_b).expect("apply b");

        assert_eq!(a, b, "same seed must yield identical noise");
    }

    #[test]
    fn test_gaussian_noise_zero_std_is_constant_shift() {
        use scirs2_core::random::{ChaCha8Rng, SeedableRng};

        let image = Array3::<f32>::from_elem((1, 3, 3), 2.0);
        let noise = GaussianNoise::new(0.25, 0.0).expect("create gaussian noise");
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let result = noise.apply_with_rng(&image, &mut rng).expect("apply");

        // std == 0 collapses the distribution to its mean: a pure +0.25 shift.
        assert!(result.iter().all(|&v| (v - 2.25).abs() < 1e-5));
    }

    #[test]
    fn test_channel_dropout() {
        let image = Array3::from_elem((3, 2, 2), 1.0);
        let dropout = ChannelDropout::new(0.5).expect("Failed to create channel dropout");
        let result = dropout.apply(&image).expect("Failed to apply dropout");

        assert_eq!(result.shape(), image.shape());
    }

    #[test]
    fn test_channel_dropout_p1_zeroes_all_channels() {
        // p=1.0 must zero every channel unconditionally.
        let image = Array3::from_elem((4, 5, 5), 2.5_f32);
        let dropout = ChannelDropout::new(1.0).expect("Failed to create channel dropout");
        let result = dropout.apply(&image).expect("Failed to apply dropout");

        assert_eq!(result.shape(), image.shape());
        assert!(
            result.iter().all(|&v| v == 0.0),
            "All pixels must be zero when p=1.0"
        );
    }

    #[test]
    fn test_channel_dropout_p0_preserves_image() {
        // p=0.0 must never drop any channel.
        let image = Array3::from_shape_fn((3, 4, 4), |(c, h, w)| {
            (c as f32 + 1.0) * 10.0 + h as f32 + w as f32
        });
        let dropout = ChannelDropout::new(0.0).expect("Failed to create channel dropout");
        let result = dropout.apply(&image).expect("Failed to apply dropout");

        assert_eq!(result, image, "Image must be unchanged when p=0.0");
    }
}
