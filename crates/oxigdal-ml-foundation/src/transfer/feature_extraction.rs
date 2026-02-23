//! Feature extraction utilities for transfer learning.

use crate::{Error, Result};
use ndarray::Array2;

/// Feature extractor for pre-trained models.
pub struct FeatureExtractor {
    /// Number of output features
    num_features: usize,
}

impl FeatureExtractor {
    /// Creates a new feature extractor.
    pub fn new(num_features: usize) -> Result<Self> {
        if num_features == 0 {
            return Err(Error::invalid_parameter(
                "num_features",
                num_features,
                "must be positive",
            ));
        }

        Ok(Self { num_features })
    }

    /// Extracts features from input data.
    ///
    /// # Note
    /// This requires the `pytorch` feature to be enabled.
    pub fn extract(&self, _batch_size: usize) -> Result<Array2<f32>> {
        #[cfg(not(feature = "ml"))]
        {
            Err(Error::feature_not_available(
                "Feature extraction",
                "pytorch",
            ))
        }

        #[cfg(feature = "ml")]
        {
            // PyTorch feature extraction implementation would go here
            Ok(Array2::zeros((0, self.num_features)))
        }
    }

    /// Gets the number of output features.
    pub fn num_features(&self) -> usize {
        self.num_features
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_extractor_creation() {
        let extractor = FeatureExtractor::new(512).expect("Failed to create feature extractor");
        assert_eq!(extractor.num_features(), 512);

        let invalid = FeatureExtractor::new(0);
        assert!(invalid.is_err());
    }
}
