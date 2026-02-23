//! Color augmentations (brightness, contrast, saturation, etc.).

use crate::augmentation::Augmentation;
use crate::{Error, Result};
use ndarray::Array3;
use serde::{Deserialize, Serialize};

/// Brightness adjustment.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Brightness {
    /// Brightness factor (0.0 = black, 1.0 = original, >1.0 = brighter)
    pub factor: f32,
}

impl Brightness {
    /// Creates a new brightness augmentation.
    pub fn new(factor: f32) -> Result<Self> {
        if factor < 0.0 {
            return Err(Error::invalid_parameter(
                "factor",
                factor,
                "must be non-negative",
            ));
        }
        Ok(Self { factor })
    }
}

impl Augmentation for Brightness {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        Ok(image.mapv(|x| x * self.factor))
    }

    fn name(&self) -> &str {
        "Brightness"
    }
}

/// Contrast adjustment.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Contrast {
    /// Contrast factor (0.0 = gray, 1.0 = original, >1.0 = more contrast)
    pub factor: f32,
}

impl Contrast {
    /// Creates a new contrast augmentation.
    pub fn new(factor: f32) -> Result<Self> {
        if factor < 0.0 {
            return Err(Error::invalid_parameter(
                "factor",
                factor,
                "must be non-negative",
            ));
        }
        Ok(Self { factor })
    }
}

impl Augmentation for Contrast {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let mean = image
            .mean()
            .ok_or_else(|| Error::Augmentation("Failed to compute mean".to_string()))?;

        Ok(image.mapv(|x| mean + (x - mean) * self.factor))
    }

    fn name(&self) -> &str {
        "Contrast"
    }
}

/// Gamma correction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Gamma {
    /// Gamma value (< 1.0 = brighter, > 1.0 = darker)
    pub gamma: f32,
}

impl Gamma {
    /// Creates a new gamma correction augmentation.
    pub fn new(gamma: f32) -> Result<Self> {
        if gamma <= 0.0 {
            return Err(Error::invalid_parameter("gamma", gamma, "must be positive"));
        }
        Ok(Self { gamma })
    }
}

impl Augmentation for Gamma {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        Ok(image.mapv(|x| x.powf(self.gamma)))
    }

    fn name(&self) -> &str {
        "Gamma"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    #[test]
    fn test_brightness() {
        let image = Array3::from_elem((3, 2, 2), 0.5);
        let brightness = Brightness::new(2.0).expect("Failed to create brightness");
        let result = brightness
            .apply(&image)
            .expect("Failed to apply brightness");

        assert!((result[[0, 0, 0]] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_contrast() {
        let image = Array3::from_elem((3, 2, 2), 0.5);
        let contrast = Contrast::new(2.0).expect("Failed to create contrast");
        let result = contrast.apply(&image).expect("Failed to apply contrast");

        assert_eq!(result.shape(), image.shape());
    }

    #[test]
    fn test_gamma() {
        let image = Array3::from_elem((3, 2, 2), 0.5);
        let gamma = Gamma::new(2.0).expect("Failed to create gamma");
        let result = gamma.apply(&image).expect("Failed to apply gamma");

        assert!((result[[0, 0, 0]] - 0.25).abs() < 1e-6);
    }
}
