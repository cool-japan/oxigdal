//! Geometric augmentations (flip, rotate, crop, etc.).

use crate::augmentation::Augmentation;
use crate::{Error, Result};
use ndarray::Array3;
use serde::{Deserialize, Serialize};

/// Horizontal flip augmentation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HorizontalFlip;

impl Augmentation for HorizontalFlip {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let (c, h, w) = image.dim();
        let mut result = Array3::zeros((c, h, w));

        for ch in 0..c {
            for row in 0..h {
                for col in 0..w {
                    result[[ch, row, w - 1 - col]] = image[[ch, row, col]];
                }
            }
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "HorizontalFlip"
    }
}

/// Vertical flip augmentation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VerticalFlip;

impl Augmentation for VerticalFlip {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let (c, h, w) = image.dim();
        let mut result = Array3::zeros((c, h, w));

        for ch in 0..c {
            for row in 0..h {
                for col in 0..w {
                    result[[ch, h - 1 - row, col]] = image[[ch, row, col]];
                }
            }
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "VerticalFlip"
    }
}

/// Rotation by 90 degrees.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rotate90 {
    /// Number of 90-degree rotations (1, 2, or 3)
    pub k: usize,
}

impl Rotate90 {
    /// Creates a new Rotate90 augmentation.
    pub fn new(k: usize) -> Result<Self> {
        if !(1..=3).contains(&k) {
            return Err(Error::invalid_parameter("k", k, "must be 1, 2, or 3"));
        }
        Ok(Self { k })
    }
}

impl Augmentation for Rotate90 {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let mut result = image.clone();

        for _ in 0..self.k {
            let (c, h, w) = result.dim();
            let mut rotated = Array3::zeros((c, w, h));

            for ch in 0..c {
                for row in 0..h {
                    for col in 0..w {
                        rotated[[ch, col, h - 1 - row]] = result[[ch, row, col]];
                    }
                }
            }

            result = rotated;
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "Rotate90"
    }

    fn is_random(&self) -> bool {
        false
    }
}

/// Center crop augmentation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CenterCrop {
    /// Target height
    pub height: usize,
    /// Target width
    pub width: usize,
}

impl CenterCrop {
    /// Creates a new center crop augmentation.
    pub fn new(height: usize, width: usize) -> Result<Self> {
        if height == 0 || width == 0 {
            return Err(Error::invalid_parameter(
                "height/width",
                format!("{}/{}", height, width),
                "must be positive",
            ));
        }
        Ok(Self { height, width })
    }

    /// Creates a square center crop.
    pub fn square(size: usize) -> Result<Self> {
        Self::new(size, size)
    }
}

impl Augmentation for CenterCrop {
    fn apply(&self, image: &Array3<f32>) -> Result<Array3<f32>> {
        let (_c, h, w) = image.dim();

        if self.height > h || self.width > w {
            return Err(Error::Augmentation(format!(
                "Crop size ({}, {}) larger than image size ({}, {})",
                self.height, self.width, h, w
            )));
        }

        let start_h = (h - self.height) / 2;
        let start_w = (w - self.width) / 2;

        let cropped = image.slice(ndarray::s![
            ..,
            start_h..start_h + self.height,
            start_w..start_w + self.width
        ]);

        Ok(cropped.to_owned())
    }

    fn name(&self) -> &str {
        "CenterCrop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::arr3;

    #[test]
    fn test_horizontal_flip() {
        let image = arr3(&[[[1.0, 2.0], [3.0, 4.0]]]);
        let flip = HorizontalFlip;
        let result = flip.apply(&image).expect("Failed to apply flip");

        assert_eq!(result[[0, 0, 0]], 2.0);
        assert_eq!(result[[0, 0, 1]], 1.0);
        assert_eq!(result[[0, 1, 0]], 4.0);
        assert_eq!(result[[0, 1, 1]], 3.0);
    }

    #[test]
    fn test_vertical_flip() {
        let image = arr3(&[[[1.0, 2.0], [3.0, 4.0]]]);
        let flip = VerticalFlip;
        let result = flip.apply(&image).expect("Failed to apply flip");

        assert_eq!(result[[0, 0, 0]], 3.0);
        assert_eq!(result[[0, 0, 1]], 4.0);
        assert_eq!(result[[0, 1, 0]], 1.0);
        assert_eq!(result[[0, 1, 1]], 2.0);
    }

    #[test]
    fn test_rotate90() {
        let image = arr3(&[[[1.0, 2.0], [3.0, 4.0]]]);
        let rotate = Rotate90::new(1).expect("Failed to create Rotate90");
        let result = rotate.apply(&image).expect("Failed to apply rotation");

        assert_eq!(result.dim(), (1, 2, 2));
    }

    #[test]
    fn test_center_crop() {
        let image = Array3::from_elem((3, 10, 10), 1.0);
        let crop = CenterCrop::square(5).expect("Failed to create CenterCrop");
        let result = crop.apply(&image).expect("Failed to apply crop");

        assert_eq!(result.dim(), (3, 5, 5));
    }

    #[test]
    fn test_center_crop_error() {
        let image = Array3::from_elem((3, 5, 5), 1.0);
        let crop = CenterCrop::square(10).expect("Failed to create CenterCrop");
        let result = crop.apply(&image);

        assert!(result.is_err());
    }
}
