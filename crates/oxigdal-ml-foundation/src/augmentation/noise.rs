//! Noise augmentations (Gaussian, salt-and-pepper, etc.).

use crate::augmentation::Augmentation;
use crate::{Error, Result};
use ndarray::Array3;
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
}

impl Augmentation for GaussianNoise {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        // Note: In a real implementation, we'd use a proper RNG
        // For now, this is a placeholder that adds fixed noise
        let noise_pattern = image.mapv(|_| self.mean);
        Ok(image + &noise_pattern)
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
        // Placeholder: In real implementation, randomly drop channels
        Ok(image.clone())
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
    use ndarray::Array3;

    #[test]
    fn test_gaussian_noise() {
        let image = Array3::from_elem((3, 2, 2), 0.5);
        let noise = GaussianNoise::new(0.0, 0.1).expect("Failed to create Gaussian noise");
        let result = noise.apply(&image).expect("Failed to apply noise");

        assert_eq!(result.shape(), image.shape());
    }

    #[test]
    fn test_channel_dropout() {
        let image = Array3::from_elem((3, 2, 2), 1.0);
        let dropout = ChannelDropout::new(0.5).expect("Failed to create channel dropout");
        let result = dropout.apply(&image).expect("Failed to apply dropout");

        assert_eq!(result.shape(), image.shape());
    }
}
