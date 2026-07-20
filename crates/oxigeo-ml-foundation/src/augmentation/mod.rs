//! Data augmentation pipelines for geospatial imagery.
//!
//! Provides geometric, color, and geospatial-specific augmentations.

pub mod color;
pub mod geometric;
pub mod geospatial;
pub mod noise;

use crate::{Error, Result};
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};

/// Trait for data augmentation transforms.
pub trait Augmentation: Send + Sync {
    /// Applies the augmentation to an image.
    ///
    /// # Arguments
    /// * `image` - Input image (C x H x W)
    ///
    /// Returns the augmented image with the same shape.
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>>;

    /// Returns the name of the augmentation.
    fn name(&self) -> &str;

    /// Returns whether the augmentation is random (non-deterministic).
    fn is_random(&self) -> bool {
        false
    }
}

/// Augmentation pipeline that applies multiple augmentations sequentially.
#[derive(Default)]
pub struct AugmentationPipeline {
    /// List of augmentations to apply
    augmentations: Vec<Box<dyn Augmentation>>,
}

impl std::fmt::Debug for AugmentationPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AugmentationPipeline")
            .field(
                "augmentations",
                &self
                    .augmentations
                    .iter()
                    .map(|a| a.name())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl AugmentationPipeline {
    /// Creates a new empty augmentation pipeline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an augmentation to the pipeline.
    pub fn add(&mut self, augmentation: Box<dyn Augmentation>) -> &mut Self {
        self.augmentations.push(augmentation);
        self
    }

    /// Applies all augmentations in the pipeline sequentially.
    ///
    /// # Arguments
    /// * `image` - Input image (C x H x W)
    pub fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let mut result = image.clone();

        for aug in &self.augmentations {
            result = aug.apply(&result)?;
        }

        Ok(result)
    }

    /// Returns the number of augmentations in the pipeline.
    pub fn len(&self) -> usize {
        self.augmentations.len()
    }

    /// Returns whether the pipeline is empty.
    pub fn is_empty(&self) -> bool {
        self.augmentations.is_empty()
    }

    /// Clears all augmentations from the pipeline.
    pub fn clear(&mut self) {
        self.augmentations.clear();
    }
}

/// Configuration for random augmentation selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomAugmentConfig {
    /// Probability of applying each augmentation (0.0 - 1.0)
    pub probability: f64,
    /// Maximum number of augmentations to apply
    pub max_augmentations: Option<usize>,
}

impl Default for RandomAugmentConfig {
    fn default() -> Self {
        Self {
            probability: 0.5,
            max_augmentations: None,
        }
    }
}

impl RandomAugmentConfig {
    /// Creates a new random augmentation configuration.
    pub fn new(probability: f64) -> Result<Self> {
        if !(0.0..=1.0).contains(&probability) {
            return Err(Error::invalid_parameter(
                "probability",
                probability,
                "must be in [0, 1]",
            ));
        }

        Ok(Self {
            probability,
            max_augmentations: None,
        })
    }

    /// Sets the maximum number of augmentations to apply.
    pub fn with_max_augmentations(mut self, max: usize) -> Self {
        self.max_augmentations = Some(max);
        self
    }
}

/// Identity augmentation (no-op).
#[derive(Debug, Clone, Copy)]
pub struct Identity;

impl Augmentation for Identity {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        Ok(image.clone())
    }

    fn name(&self) -> &str {
        "Identity"
    }
}

/// Normalizes image to [0, 1] range.
#[derive(Debug, Clone, Copy)]
pub struct Normalize {
    /// Minimum value in input
    pub min_val: f32,
    /// Maximum value in input
    pub max_val: f32,
}

impl Normalize {
    /// Creates a new normalize augmentation.
    pub fn new(min_val: f32, max_val: f32) -> Result<Self> {
        if min_val >= max_val {
            return Err(Error::invalid_parameter(
                "min_val/max_val",
                format!("{}/{}", min_val, max_val),
                "min_val must be less than max_val",
            ));
        }

        Ok(Self { min_val, max_val })
    }

    /// Creates a normalize augmentation for [0, 255] images.
    pub fn from_uint8() -> Self {
        Self {
            min_val: 0.0,
            max_val: 255.0,
        }
    }
}

impl Augmentation for Normalize {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let range = self.max_val - self.min_val;
        Ok(image.mapv(|x| (x - self.min_val) / range))
    }

    fn name(&self) -> &str {
        "Normalize"
    }
}

/// Standardizes image using mean and std.
#[derive(Debug, Clone)]
pub struct Standardize {
    /// Mean for each channel
    pub mean: Vec<f32>,
    /// Standard deviation for each channel
    pub std: Vec<f32>,
}

impl Standardize {
    /// Creates a new standardize augmentation.
    pub fn new(mean: Vec<f32>, std: Vec<f32>) -> Result<Self> {
        if mean.len() != std.len() {
            return Err(Error::invalid_parameter(
                "mean/std",
                format!("{}/{}", mean.len(), std.len()),
                "mean and std must have the same length",
            ));
        }

        for (i, &s) in std.iter().enumerate() {
            if s <= 0.0 {
                return Err(Error::invalid_parameter(
                    "std",
                    s,
                    format!("std[{}] must be positive", i),
                ));
            }
        }

        Ok(Self { mean, std })
    }

    /// Creates ImageNet standardization (RGB images).
    pub fn imagenet() -> Self {
        Self {
            mean: vec![0.485, 0.456, 0.406],
            std: vec![0.229, 0.224, 0.225],
        }
    }
}

impl Augmentation for Standardize {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let num_channels = image.shape()[0];

        if num_channels != self.mean.len() {
            return Err(Error::invalid_dimensions(
                format!("{} channels", self.mean.len()),
                format!("{} channels", num_channels),
            ));
        }

        let mut result = image.clone();

        for c in 0..num_channels {
            let mean = self.mean[c];
            let std = self.std[c];

            for h in 0..image.shape()[1] {
                for w in 0..image.shape()[2] {
                    result[[c, h, w]] = (image[[c, h, w]] - mean) / std;
                }
            }
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "Standardize"
    }
}

/// Clips values to a specified range.
#[derive(Debug, Clone, Copy)]
pub struct Clip {
    /// Minimum value
    pub min_val: f32,
    /// Maximum value
    pub max_val: f32,
}

impl Clip {
    /// Creates a new clip augmentation.
    pub fn new(min_val: f32, max_val: f32) -> Result<Self> {
        if min_val >= max_val {
            return Err(Error::invalid_parameter(
                "min_val/max_val",
                format!("{}/{}", min_val, max_val),
                "min_val must be less than max_val",
            ));
        }

        Ok(Self { min_val, max_val })
    }
}

impl Augmentation for Clip {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        Ok(image.mapv(|x| x.clamp(self.min_val, self.max_val)))
    }

    fn name(&self) -> &str {
        "Clip"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array3;

    fn create_test_image() -> Array3<f32> {
        Array3::from_shape_fn((3, 4, 4), |(c, h, w)| {
            (c as f32 + h as f32 + w as f32) * 10.0
        })
    }

    #[test]
    fn test_identity() {
        let image = create_test_image();
        let identity = Identity;
        let result = identity.apply(&image).expect("Failed to apply identity");
        assert_eq!(result, image);
    }

    #[test]
    fn test_normalize() {
        let image = Array3::from_elem((3, 2, 2), 127.5);
        let normalize = Normalize::from_uint8();
        let result = normalize.apply(&image).expect("Failed to apply normalize");

        assert!((result[[0, 0, 0]] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_standardize() {
        let image = Array3::from_elem((3, 2, 2), 0.5);
        let standardize = Standardize::imagenet();
        let result = standardize
            .apply(&image)
            .expect("Failed to apply standardize");

        assert_eq!(result.shape(), image.shape());
    }

    #[test]
    fn test_clip() {
        let mut image = create_test_image();
        image[[0, 0, 0]] = -100.0;
        image[[0, 1, 1]] = 1000.0;

        let clip = Clip::new(0.0, 100.0).expect("Failed to create clip");
        let result = clip.apply(&image).expect("Failed to apply clip");

        assert_eq!(result[[0, 0, 0]], 0.0);
        assert_eq!(result[[0, 1, 1]], 100.0);
    }

    #[test]
    fn test_augmentation_pipeline() {
        let image = create_test_image();
        let mut pipeline = AugmentationPipeline::new();

        pipeline.add(Box::new(Normalize::from_uint8()));
        pipeline.add(Box::new(
            Clip::new(-1.0, 1.0).expect("Failed to create clip"),
        ));

        assert_eq!(pipeline.len(), 2);
        assert!(!pipeline.is_empty());

        let result = pipeline.apply(&image).expect("Failed to apply pipeline");
        assert_eq!(result.shape(), image.shape());

        pipeline.clear();
        assert_eq!(pipeline.len(), 0);
        assert!(pipeline.is_empty());
    }

    #[test]
    fn test_normalize_errors() {
        let result = Normalize::new(100.0, 50.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_standardize_errors() {
        let result = Standardize::new(vec![0.5, 0.5], vec![0.1]);
        assert!(result.is_err());

        let result = Standardize::new(vec![0.5], vec![0.0]);
        assert!(result.is_err());
    }
}
