//! Geospatial-specific augmentations for multispectral imagery.

use crate::augmentation::Augmentation;
use crate::{Error, Result};
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};

/// Band selection (select specific spectral bands).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandSelection {
    /// Indices of bands to keep
    pub band_indices: Vec<usize>,
}

impl BandSelection {
    /// Creates a new band selection augmentation.
    pub fn new(band_indices: Vec<usize>) -> Result<Self> {
        if band_indices.is_empty() {
            return Err(Error::invalid_parameter(
                "band_indices",
                "empty",
                "must have at least one band",
            ));
        }
        Ok(Self { band_indices })
    }

    /// Creates RGB band selection (bands 0, 1, 2).
    pub fn rgb() -> Self {
        Self {
            band_indices: vec![0, 1, 2],
        }
    }
}

impl Augmentation for BandSelection {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let (c, h, w) = image.dim();

        for &idx in &self.band_indices {
            if idx >= c {
                return Err(Error::Augmentation(format!(
                    "Band index {} out of bounds (image has {} bands)",
                    idx, c
                )));
            }
        }

        let mut result = Array3::zeros((self.band_indices.len(), h, w));

        for (new_idx, &old_idx) in self.band_indices.iter().enumerate() {
            for row in 0..h {
                for col in 0..w {
                    result[[new_idx, row, col]] = image[[old_idx, row, col]];
                }
            }
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "BandSelection"
    }
}

/// Spectral normalization for multispectral imagery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectralNormalization {
    /// Per-band min values
    pub min_values: Vec<f32>,
    /// Per-band max values
    pub max_values: Vec<f32>,
}

impl SpectralNormalization {
    /// Creates a new spectral normalization.
    pub fn new(min_values: Vec<f32>, max_values: Vec<f32>) -> Result<Self> {
        if min_values.len() != max_values.len() {
            return Err(Error::invalid_parameter(
                "min_values/max_values",
                format!("{}/{}", min_values.len(), max_values.len()),
                "must have the same length",
            ));
        }

        for (i, (&min_val, &max_val)) in min_values.iter().zip(max_values.iter()).enumerate() {
            if min_val >= max_val {
                return Err(Error::invalid_parameter(
                    "min_values/max_values",
                    format!("{}/{}", min_val, max_val),
                    format!("min[{}] must be less than max[{}]", i, i),
                ));
            }
        }

        Ok(Self {
            min_values,
            max_values,
        })
    }
}

impl Augmentation for SpectralNormalization {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let (c, h, w) = image.dim();

        if c != self.min_values.len() {
            return Err(Error::invalid_dimensions(
                format!("{} channels", self.min_values.len()),
                format!("{} channels", c),
            ));
        }

        let mut result = Array3::zeros((c, h, w));

        for ch in 0..c {
            let min_val = self.min_values[ch];
            let max_val = self.max_values[ch];
            let range = max_val - min_val;

            for row in 0..h {
                for col in 0..w {
                    result[[ch, row, col]] = (image[[ch, row, col]] - min_val) / range;
                }
            }
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "SpectralNormalization"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array3;

    #[test]
    fn test_band_selection() {
        let image = Array3::from_shape_fn((10, 4, 4), |(c, _, _)| c as f32);
        let band_sel = BandSelection::rgb();
        let result = band_sel
            .apply(&image)
            .expect("Failed to apply band selection");

        assert_eq!(result.dim(), (3, 4, 4));
        assert_eq!(result[[0, 0, 0]], 0.0);
        assert_eq!(result[[1, 0, 0]], 1.0);
        assert_eq!(result[[2, 0, 0]], 2.0);
    }

    #[test]
    fn test_spectral_normalization() {
        let image = Array3::from_elem((3, 2, 2), 50.0);
        let norm = SpectralNormalization::new(vec![0.0, 0.0, 0.0], vec![100.0, 100.0, 100.0])
            .expect("Failed to create spectral normalization");

        let result = norm
            .apply(&image)
            .expect("Failed to apply spectral normalization");

        assert!((result[[0, 0, 0]] - 0.5).abs() < 1e-6);
    }
}
