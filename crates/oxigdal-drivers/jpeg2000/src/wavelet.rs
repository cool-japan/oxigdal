//! Wavelet transform implementation
//!
//! This module implements the discrete wavelet transform (DWT) used in JPEG2000.
//! Both 5/3 reversible (lossless) and 9/7 irreversible (lossy) transforms are supported.

use crate::error::{Jpeg2000Error, Result};
use num_traits::NumCast;

/// Wavelet transform direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformDirection {
    /// Forward transform (analysis)
    Forward,
    /// Inverse transform (synthesis)
    Inverse,
}

/// 5/3 reversible wavelet transform (lossless)
///
/// This is a lifting scheme implementation of the LeGall 5/3 wavelet.
/// Supports both forward (analysis) and inverse (synthesis) transforms,
/// enabling full lossless compression round-trip capability.
pub struct Reversible53;

impl Reversible53 {
    /// Perform 1D forward transform (analysis) on row/column
    ///
    /// Splits the signal into low-frequency (even) and high-frequency (odd)
    /// subbands using the lifting scheme. The transform is perfectly reversible
    /// with integer arithmetic.
    pub fn forward_1d(data: &mut [i32]) {
        let len = data.len();
        if len < 2 {
            return;
        }

        let half = len.div_ceil(2);

        // Forward update step (update even samples)
        for i in 0..half {
            let even_idx = 2 * i;
            if even_idx == 0 {
                if len > 1 {
                    data[even_idx] -= data[1] >> 1;
                }
            } else if even_idx + 1 < len {
                data[even_idx] -= (data[even_idx - 1] + data[even_idx + 1] + 2) >> 2;
            } else {
                data[even_idx] -= data[even_idx - 1] >> 1;
            }
        }

        // Forward predict step (predict odd samples from even)
        for i in 0..(len / 2) {
            let odd_idx = 2 * i + 1;
            let left = data[2 * i];
            let right = if 2 * i + 2 < len {
                data[2 * i + 2]
            } else {
                data[2 * i]
            };
            data[odd_idx] += (left + right) >> 1;
        }
    }

    /// Perform 2D forward transform (analysis)
    ///
    /// Applies the forward 5/3 wavelet transform in both dimensions,
    /// producing four subbands: LL (approximation), LH (vertical detail),
    /// HL (horizontal detail), and HH (diagonal detail).
    pub fn forward_2d(data: &mut [i32], width: usize, height: usize) -> Result<()> {
        if data.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Data size mismatch: expected {}, got {}",
                width * height,
                data.len()
            )));
        }

        // Process rows first (horizontal transform)
        for y in 0..height {
            let start = y * width;
            let end = start + width;
            Self::forward_1d(&mut data[start..end]);
        }

        // Process columns (vertical transform)
        let mut column = vec![0i32; height];
        for x in 0..width {
            for y in 0..height {
                column[y] = data[y * width + x];
            }
            Self::forward_1d(&mut column);
            for y in 0..height {
                data[y * width + x] = column[y];
            }
        }

        Ok(())
    }

    /// Perform 1D inverse transform on row/column
    pub fn inverse_1d(data: &mut [i32]) {
        let len = data.len();
        if len < 2 {
            return;
        }

        let half = len.div_ceil(2);

        // Inverse predict step
        for i in 0..(len / 2) {
            let odd_idx = 2 * i + 1;
            let left = data[2 * i];
            let right = if 2 * i + 2 < len {
                data[2 * i + 2]
            } else {
                data[2 * i]
            };
            data[odd_idx] -= (left + right) >> 1;
        }

        // Inverse update step
        for i in 0..half {
            let even_idx = 2 * i;
            if even_idx == 0 {
                if len > 1 {
                    data[even_idx] += data[1] >> 1;
                }
            } else if even_idx + 1 < len {
                data[even_idx] += (data[even_idx - 1] + data[even_idx + 1] + 2) >> 2;
            } else {
                data[even_idx] += data[even_idx - 1] >> 1;
            }
        }
    }

    /// Perform 2D inverse transform
    pub fn inverse_2d(data: &mut [i32], width: usize, height: usize) -> Result<()> {
        if data.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Data size mismatch: expected {}, got {}",
                width * height,
                data.len()
            )));
        }

        // Process columns
        let mut column = vec![0i32; height];
        for x in 0..width {
            for y in 0..height {
                column[y] = data[y * width + x];
            }
            Self::inverse_1d(&mut column);
            for y in 0..height {
                data[y * width + x] = column[y];
            }
        }

        // Process rows
        for y in 0..height {
            let start = y * width;
            let end = start + width;
            Self::inverse_1d(&mut data[start..end]);
        }

        Ok(())
    }
}

/// 9/7 irreversible wavelet transform (lossy)
///
/// This implements the CDF 9/7 wavelet using lifting scheme.
/// Supports both forward (analysis) and inverse (synthesis) transforms.
pub struct Irreversible97;

impl Irreversible97 {
    // Lifting coefficients for 9/7 wavelet
    const ALPHA: f32 = -1.586_134_3;
    const BETA: f32 = -0.052980118;
    const GAMMA: f32 = 0.882_911_1;
    const DELTA: f32 = 0.443_506_87;
    const K: f32 = 1.230_174_1;

    /// Perform 1D forward transform (analysis) on row/column
    ///
    /// Applies the CDF 9/7 wavelet forward transform using the lifting scheme.
    /// This produces lossy compression with good visual quality.
    pub fn forward_1d(data: &mut [f32]) {
        let len = data.len();
        if len < 2 {
            return;
        }

        let half = len.div_ceil(2);

        // Step 0: Alpha (predict odd from even)
        for i in 0..(len / 2) {
            let odd_idx = 2 * i + 1;
            let left = data[2 * i];
            let right = if 2 * i + 2 < len {
                data[2 * i + 2]
            } else {
                data[2 * i]
            };
            data[odd_idx] += Self::ALPHA * (left + right);
        }

        // Step 1: Beta (update even from odd)
        for i in 0..half {
            let even_idx = 2 * i;
            if even_idx == 0 {
                if len > 1 {
                    data[even_idx] += Self::BETA * data[1];
                }
            } else if even_idx + 1 < len {
                data[even_idx] += Self::BETA * (data[even_idx - 1] + data[even_idx + 1]);
            } else {
                data[even_idx] += Self::BETA * data[even_idx - 1];
            }
        }

        // Step 2: Gamma (predict odd from even)
        for i in 0..(len / 2) {
            let odd_idx = 2 * i + 1;
            let left = data[2 * i];
            let right = if 2 * i + 2 < len {
                data[2 * i + 2]
            } else {
                data[2 * i]
            };
            data[odd_idx] += Self::GAMMA * (left + right);
        }

        // Step 3: Delta (update even from odd)
        for i in 0..half {
            let even_idx = 2 * i;
            if even_idx == 0 {
                if len > 1 {
                    data[even_idx] += Self::DELTA * data[1];
                }
            } else if even_idx + 1 < len {
                data[even_idx] += Self::DELTA * (data[even_idx - 1] + data[even_idx + 1]);
            } else {
                data[even_idx] += Self::DELTA * data[even_idx - 1];
            }
        }

        // Step 4: Scale
        for i in 0..half {
            data[2 * i] *= Self::K;
        }
        for i in 0..(len / 2) {
            data[2 * i + 1] /= Self::K;
        }
    }

    /// Perform 2D forward transform (analysis)
    ///
    /// Applies the CDF 9/7 wavelet in both dimensions, producing
    /// LL, LH, HL, HH subbands for lossy compression.
    pub fn forward_2d(data: &mut [f32], width: usize, height: usize) -> Result<()> {
        if data.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Data size mismatch: expected {}, got {}",
                width * height,
                data.len()
            )));
        }

        // Process rows first (horizontal transform)
        for y in 0..height {
            let start = y * width;
            let end = start + width;
            Self::forward_1d(&mut data[start..end]);
        }

        // Process columns (vertical transform)
        let mut column = vec![0.0f32; height];
        for x in 0..width {
            for y in 0..height {
                column[y] = data[y * width + x];
            }
            Self::forward_1d(&mut column);
            for y in 0..height {
                data[y * width + x] = column[y];
            }
        }

        Ok(())
    }

    /// Perform 1D inverse transform on row/column
    pub fn inverse_1d(data: &mut [f32]) {
        let len = data.len();
        if len < 2 {
            return;
        }

        let half = len.div_ceil(2);

        // Inverse lifting steps (in reverse order)

        // Step 4: Scale
        for i in 0..half {
            data[2 * i] /= Self::K;
        }
        for i in 0..(len / 2) {
            data[2 * i + 1] *= Self::K;
        }

        // Step 3: Delta
        for i in 0..half {
            let even_idx = 2 * i;
            if even_idx == 0 {
                if len > 1 {
                    data[even_idx] -= Self::DELTA * data[1];
                }
            } else if even_idx + 1 < len {
                data[even_idx] -= Self::DELTA * (data[even_idx - 1] + data[even_idx + 1]);
            } else {
                data[even_idx] -= Self::DELTA * data[even_idx - 1];
            }
        }

        // Step 2: Gamma
        for i in 0..(len / 2) {
            let odd_idx = 2 * i + 1;
            let left = data[2 * i];
            let right = if 2 * i + 2 < len {
                data[2 * i + 2]
            } else {
                data[2 * i]
            };
            data[odd_idx] -= Self::GAMMA * (left + right);
        }

        // Step 1: Beta
        for i in 0..half {
            let even_idx = 2 * i;
            if even_idx == 0 {
                if len > 1 {
                    data[even_idx] -= Self::BETA * data[1];
                }
            } else if even_idx + 1 < len {
                data[even_idx] -= Self::BETA * (data[even_idx - 1] + data[even_idx + 1]);
            } else {
                data[even_idx] -= Self::BETA * data[even_idx - 1];
            }
        }

        // Step 0: Alpha
        for i in 0..(len / 2) {
            let odd_idx = 2 * i + 1;
            let left = data[2 * i];
            let right = if 2 * i + 2 < len {
                data[2 * i + 2]
            } else {
                data[2 * i]
            };
            data[odd_idx] -= Self::ALPHA * (left + right);
        }
    }

    /// Perform 2D inverse transform
    pub fn inverse_2d(data: &mut [f32], width: usize, height: usize) -> Result<()> {
        if data.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Data size mismatch: expected {}, got {}",
                width * height,
                data.len()
            )));
        }

        // Process columns
        let mut column = vec![0.0f32; height];
        for x in 0..width {
            for y in 0..height {
                column[y] = data[y * width + x];
            }
            Self::inverse_1d(&mut column);
            for y in 0..height {
                data[y * width + x] = column[y];
            }
        }

        // Process rows
        for y in 0..height {
            let start = y * width;
            let end = start + width;
            Self::inverse_1d(&mut data[start..end]);
        }

        Ok(())
    }
}

/// Multi-level wavelet transform handler
///
/// Supports both forward (analysis/encoding) and inverse (synthesis/decoding)
/// multi-level wavelet transforms for lossless (5/3) and lossy (9/7) modes.
pub struct WaveletTransformer {
    /// Number of decomposition levels
    pub num_levels: usize,
}

impl WaveletTransformer {
    /// Create new wavelet transformer
    pub fn new(num_levels: usize) -> Self {
        Self { num_levels }
    }

    /// Perform multi-level forward transform (reversible 5/3) for lossless encoding
    ///
    /// Decomposes the image into multiple resolution levels. Each level
    /// produces LL (approximation), LH (vertical), HL (horizontal), and
    /// HH (diagonal) subbands. The LL subband is further decomposed at
    /// the next level.
    pub fn forward_reversible(&self, data: &mut [i32], width: usize, height: usize) -> Result<()> {
        let mut current_width = width;
        let mut current_height = height;

        // Apply forward transform at each decomposition level
        for level in 0..self.num_levels {
            if current_width < 2 || current_height < 2 {
                break;
            }

            // Extract this level's data
            let mut level_data = vec![0i32; current_width * current_height];
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        level_data[y * current_width + x] = data[y * width + x];
                    }
                }
            }

            Reversible53::forward_2d(&mut level_data, current_width, current_height)?;

            // Copy back
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        data[y * width + x] = level_data[y * current_width + x];
                    }
                }
            }

            // Next level works on the LL subband (top-left quadrant)
            current_width = current_width.div_ceil(2);
            current_height = current_height.div_ceil(2);

            tracing::debug!("Applied forward reversible transform at level {}", level);
        }

        Ok(())
    }

    /// Perform multi-level forward transform (irreversible 9/7) for lossy encoding
    pub fn forward_irreversible(
        &self,
        data: &mut [f32],
        width: usize,
        height: usize,
    ) -> Result<()> {
        let mut current_width = width;
        let mut current_height = height;

        // Apply forward transform at each decomposition level
        for level in 0..self.num_levels {
            if current_width < 2 || current_height < 2 {
                break;
            }

            // Extract this level's data
            let mut level_data = vec![0.0f32; current_width * current_height];
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        level_data[y * current_width + x] = data[y * width + x];
                    }
                }
            }

            Irreversible97::forward_2d(&mut level_data, current_width, current_height)?;

            // Copy back
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        data[y * width + x] = level_data[y * current_width + x];
                    }
                }
            }

            // Next level works on the LL subband (top-left quadrant)
            current_width = current_width.div_ceil(2);
            current_height = current_height.div_ceil(2);

            tracing::debug!("Applied forward irreversible transform at level {}", level);
        }

        Ok(())
    }

    /// Perform multi-level inverse transform (reversible 5/3)
    ///
    /// The inverse processes levels from the deepest (smallest subband)
    /// to the shallowest (full image), which is the exact reverse of the
    /// forward transform order.
    pub fn inverse_reversible(&self, data: &mut [i32], width: usize, height: usize) -> Result<()> {
        // Compute dimensions at each decomposition level
        // level_dims[0] = full image, level_dims[1] = after first decomposition, etc.
        let mut level_dims = Vec::with_capacity(self.num_levels + 1);
        level_dims.push((width, height));
        for _ in 0..self.num_levels {
            let &(w, h) = level_dims
                .last()
                .ok_or_else(|| Jpeg2000Error::WaveletError("no dimensions".to_string()))?;
            if w < 2 || h < 2 {
                break;
            }
            level_dims.push((w.div_ceil(2), h.div_ceil(2)));
        }

        let actual_levels = level_dims.len() - 1;

        // Apply inverse transform from deepest level (smallest) to shallowest (largest)
        // The forward went: level_dims[0] -> level_dims[1] -> ... -> level_dims[actual_levels]
        // The inverse goes: level_dims[actual_levels-1] -> ... -> level_dims[0]
        for level in (0..actual_levels).rev() {
            let (current_width, current_height) = level_dims[level];

            // Extract this level's data
            let mut level_data = vec![0i32; current_width * current_height];
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        level_data[y * current_width + x] = data[y * width + x];
                    }
                }
            }

            Reversible53::inverse_2d(&mut level_data, current_width, current_height)?;

            // Copy back
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        data[y * width + x] = level_data[y * current_width + x];
                    }
                }
            }

            tracing::debug!("Applied inverse reversible transform at level {}", level);
        }

        Ok(())
    }

    /// Perform multi-level inverse transform (irreversible 9/7)
    ///
    /// The inverse processes levels from the deepest (smallest subband)
    /// to the shallowest (full image), which is the exact reverse of the
    /// forward transform order.
    pub fn inverse_irreversible(
        &self,
        data: &mut [f32],
        width: usize,
        height: usize,
    ) -> Result<()> {
        // Compute dimensions at each decomposition level
        let mut level_dims = Vec::with_capacity(self.num_levels + 1);
        level_dims.push((width, height));
        for _ in 0..self.num_levels {
            let &(w, h) = level_dims
                .last()
                .ok_or_else(|| Jpeg2000Error::WaveletError("no dimensions".to_string()))?;
            if w < 2 || h < 2 {
                break;
            }
            level_dims.push((w.div_ceil(2), h.div_ceil(2)));
        }

        let actual_levels = level_dims.len() - 1;

        // Apply inverse transform from deepest to shallowest
        for level in (0..actual_levels).rev() {
            let (current_width, current_height) = level_dims[level];

            // Extract this level's data
            let mut level_data = vec![0.0f32; current_width * current_height];
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        level_data[y * current_width + x] = data[y * width + x];
                    }
                }
            }

            Irreversible97::inverse_2d(&mut level_data, current_width, current_height)?;

            // Copy back
            for y in 0..current_height {
                for x in 0..current_width {
                    if y < height && x < width {
                        data[y * width + x] = level_data[y * current_width + x];
                    }
                }
            }

            tracing::debug!("Applied inverse irreversible transform at level {}", level);
        }

        Ok(())
    }
}

/// Convert floating point coefficients to integer samples
pub fn float_to_int<T: NumCast>(data: &[f32], precision: u8) -> Result<Vec<T>> {
    let max_val = (1 << precision) - 1;

    data.iter()
        .map(|&val| {
            let clamped = val.max(0.0).min(max_val as f32);
            T::from(clamped).ok_or_else(|| {
                Jpeg2000Error::WaveletError("Failed to convert sample value".to_string())
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reversible_1d_round_trip() {
        let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let original = data.clone();

        // Forward then inverse should be identity for reversible transform
        Reversible53::forward_1d(&mut data);
        Reversible53::inverse_1d(&mut data);

        assert_eq!(data, original, "5/3 reversible 1D round-trip failed");
    }

    #[test]
    fn test_reversible_1d_odd_length() {
        let mut data = vec![5, 15, 25, 35, 45];
        let original = data.clone();

        Reversible53::forward_1d(&mut data);
        Reversible53::inverse_1d(&mut data);

        assert_eq!(
            data, original,
            "5/3 reversible 1D odd-length round-trip failed"
        );
    }

    #[test]
    fn test_reversible_2d_round_trip() {
        let mut data = vec![
            10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
        ];
        let original = data.clone();

        Reversible53::forward_2d(&mut data, 4, 4).expect("forward failed");
        Reversible53::inverse_2d(&mut data, 4, 4).expect("inverse failed");

        assert_eq!(data, original, "5/3 reversible 2D round-trip failed");
    }

    #[test]
    fn test_reversible_2d_non_square() {
        let mut data = vec![
            10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190,
            200, 210, 220, 230, 240,
        ];
        let original = data.clone();

        Reversible53::forward_2d(&mut data, 6, 4).expect("forward failed");
        Reversible53::inverse_2d(&mut data, 6, 4).expect("inverse failed");

        assert_eq!(
            data, original,
            "5/3 reversible 2D non-square round-trip failed"
        );
    }

    #[test]
    fn test_irreversible_1d_forward_inverse() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let original = data.clone();

        Irreversible97::forward_1d(&mut data);
        Irreversible97::inverse_1d(&mut data);

        // Lossy transform: check within tolerance
        for (a, b) in data.iter().zip(original.iter()) {
            assert!(
                (a - b).abs() < 0.01,
                "9/7 irreversible 1D round-trip error: {} vs {}",
                a,
                b
            );
        }
    }

    #[test]
    fn test_irreversible_2d_forward_inverse() {
        let mut data = vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ];
        let original = data.clone();

        Irreversible97::forward_2d(&mut data, 4, 4).expect("forward failed");
        Irreversible97::inverse_2d(&mut data, 4, 4).expect("inverse failed");

        for (a, b) in data.iter().zip(original.iter()) {
            assert!(
                (a - b).abs() < 0.1,
                "9/7 irreversible 2D round-trip error: {} vs {}",
                a,
                b
            );
        }
    }

    #[test]
    fn test_wavelet_transformer_creation() {
        let transformer = WaveletTransformer::new(5);
        assert_eq!(transformer.num_levels, 5);
    }

    #[test]
    fn test_multilevel_reversible_round_trip() {
        let width = 8;
        let height = 8;
        let mut data: Vec<i32> = (0..(width * height) as i32).collect();
        let original = data.clone();

        let transformer = WaveletTransformer::new(2);
        transformer
            .forward_reversible(&mut data, width, height)
            .expect("forward failed");
        transformer
            .inverse_reversible(&mut data, width, height)
            .expect("inverse failed");

        assert_eq!(data, original, "Multi-level reversible round-trip failed");
    }

    #[test]
    fn test_multilevel_irreversible_round_trip() {
        let width = 8;
        let height = 8;
        let mut data: Vec<f32> = (0..(width * height))
            .map(|i| i as f32 * std::f32::consts::PI)
            .collect();
        let original = data.clone();

        let transformer = WaveletTransformer::new(2);
        transformer
            .forward_irreversible(&mut data, width, height)
            .expect("forward failed");
        transformer
            .inverse_irreversible(&mut data, width, height)
            .expect("inverse failed");

        for (a, b) in data.iter().zip(original.iter()) {
            assert!(
                (a - b).abs() < 1.0,
                "Multi-level irreversible round-trip error: {} vs {}",
                a,
                b
            );
        }
    }

    #[test]
    fn test_forward_produces_subbands() {
        // After forward transform, LL subband should be lower frequency
        let mut data = vec![
            100, 100, 100, 100, 100, 100, 100, 100, 200, 200, 200, 200, 200, 200, 200, 200,
        ];

        Reversible53::forward_2d(&mut data, 4, 4).expect("forward failed");

        // The transform should have produced coefficients, not all same values
        let all_same = data.windows(2).all(|w| w[0] == w[1]);
        assert!(
            !all_same,
            "Forward transform should produce varied coefficients"
        );
    }

    #[test]
    fn test_forward_2d_size_mismatch() {
        let mut data = vec![1i32; 10];
        let result = Reversible53::forward_2d(&mut data, 4, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_float_to_int_conversion() {
        let data = vec![0.0, 127.5, 255.0, 300.0];
        let result: Vec<u8> = float_to_int(&data, 8).expect("conversion failed");
        assert_eq!(result[0], 0);
        assert_eq!(result[1], 127);
        assert_eq!(result[2], 255);
        assert_eq!(result[3], 255); // Clamped
    }

    #[test]
    fn test_single_element_transform() {
        let mut data = vec![42i32];
        let original = data.clone();
        Reversible53::forward_1d(&mut data);
        Reversible53::inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn test_two_element_transform() {
        let mut data = vec![10i32, 20];
        let original = data.clone();
        Reversible53::forward_1d(&mut data);
        Reversible53::inverse_1d(&mut data);
        assert_eq!(data, original);
    }
}
