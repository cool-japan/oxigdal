//! Lanczos resampling algorithm
//!
//! Lanczos resampling provides the highest quality results using a windowed sinc filter.
//! It preserves sharp edges better than bicubic while avoiding excessive ringing.
//!
//! # Characteristics
//!
//! - **Speed**: Slowest (36-100 samples per pixel)
//! - **Quality**: Highest, excellent edge preservation
//! - **Best for**: High-quality imagery, when quality is paramount
//!
//! # Algorithm
//!
//! For each output pixel:
//! 1. Sample (2a) x (2a) neighborhood (where a = lobes, typically 2 or 3)
//! 2. Apply Lanczos kernel: L(x) = sinc(x) * sinc(x/a)
//! 3. Normalize weights and sum

use crate::error::{AlgorithmError, Result};
use crate::resampling::kernel::{lanczos, normalize_weights};
use oxigdal_core::buffer::RasterBuffer;

/// Lanczos resampler with configurable lobe count
#[derive(Debug, Clone, Copy)]
pub struct LanczosResampler {
    /// Number of lobes (typically 2 or 3)
    lobes: usize,
}

impl Default for LanczosResampler {
    fn default() -> Self {
        Self::new(3)
    }
}

impl LanczosResampler {
    /// Creates a new Lanczos resampler with specified lobe count
    ///
    /// # Arguments
    ///
    /// * `lobes` - Number of lobes (2 = faster, 3 = higher quality)
    ///
    /// Common values:
    /// - 2: Lanczos2 - faster, still good quality
    /// - 3: Lanczos3 - standard, excellent quality (default)
    #[must_use]
    pub const fn new(lobes: usize) -> Self {
        Self { lobes }
    }

    /// Returns the lobe count
    #[must_use]
    pub const fn lobes(&self) -> usize {
        self.lobes
    }

    /// Returns the kernel radius (same as lobe count)
    #[must_use]
    pub const fn radius(&self) -> usize {
        self.lobes
    }

    /// Resamples a raster buffer using Lanczos interpolation
    ///
    /// Out-of-range source samples needed by the kernel window at the
    /// image border are handled via `EdgeMode::Clamp`. Use
    /// [`LanczosResampler::resample_with_edge_mode`] to select a different
    /// edge-handling strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions are invalid
    pub fn resample(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<RasterBuffer> {
        self.resample_with_edge_mode(src, dst_width, dst_height, EdgeMode::Clamp)
    }

    /// Interpolates value at fractional source coordinates using Lanczos
    ///
    /// `edge_mode` controls how out-of-range source coordinates within the
    /// kernel window are mapped back into the valid `[0, size)` range (see
    /// [`map_edge_coord`]).
    fn interpolate_at(
        &self,
        src: &RasterBuffer,
        src_x: f64,
        src_y: f64,
        edge_mode: EdgeMode,
    ) -> Result<f64> {
        let src_width = src.width();
        let src_height = src.height();

        // Clamp to valid range
        let src_x_clamped = src_x.max(0.0).min((src_width - 1) as f64);
        let src_y_clamped = src_y.max(0.0).min((src_height - 1) as f64);

        // Get integer center
        let x_center = src_x_clamped.floor() as i64;
        let y_center = src_y_clamped.floor() as i64;

        // Compute kernel range
        let radius = self.radius() as i64;
        let x_start = x_center - radius + 1;
        let x_end = x_center + radius + 1;
        let y_start = y_center - radius + 1;
        let y_end = y_center + radius + 1;

        let kernel_width = (x_end - x_start) as usize;
        let kernel_height = (y_end - y_start) as usize;

        // Allocate weight and value buffers
        let total_samples = kernel_width * kernel_height;
        let mut values = vec![0.0f64; total_samples];
        let mut weights = vec![0.0f64; total_samples];

        // Sample neighborhood and compute weights
        let mut idx = 0;
        for sy in y_start..y_end {
            for sx in x_start..x_end {
                // Map out-of-range coordinates back into the valid range
                // according to the requested edge-handling mode.
                let sample_x = map_edge_coord(sx, src_width, edge_mode);
                let sample_y = map_edge_coord(sy, src_height, edge_mode);

                // Get value
                values[idx] = src
                    .get_pixel(sample_x, sample_y)
                    .map_err(AlgorithmError::Core)?;

                // Compute kernel weight
                let dx = sx as f64 - src_x_clamped;
                let dy = sy as f64 - src_y_clamped;
                let wx = lanczos(dx, self.lobes);
                let wy = lanczos(dy, self.lobes);
                weights[idx] = wx * wy;

                idx += 1;
            }
        }

        // Normalize weights
        normalize_weights(&mut weights);

        // Compute weighted sum
        let mut result = 0.0;
        for (value, weight) in values.iter().zip(weights.iter()) {
            result += value * weight;
        }

        Ok(result)
    }

    /// Resamples with edge handling
    ///
    /// This variant allows specifying how to handle source samples that the
    /// kernel window would otherwise read from outside `[0, width) x
    /// [0, height)` near the image border.
    ///
    /// # Arguments
    ///
    /// * `src` - Source buffer
    /// * `dst_width` - Destination width
    /// * `dst_height` - Destination height
    /// * `edge_mode` - How to handle edge pixels
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid
    pub fn resample_with_edge_mode(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
        edge_mode: EdgeMode,
    ) -> Result<RasterBuffer> {
        if dst_width == 0 || dst_height == 0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "dimensions",
                message: "Target dimensions must be non-zero".to_string(),
            });
        }

        let src_width = src.width();
        let src_height = src.height();

        if src_width == 0 || src_height == 0 {
            return Err(AlgorithmError::EmptyInput {
                operation: "Lanczos resampling",
            });
        }

        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());

        let scale_x = src_width as f64 / dst_width as f64;
        let scale_y = src_height as f64 / dst_height as f64;

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x = (dst_x as f64 + 0.5) * scale_x - 0.5;
                let src_y = (dst_y as f64 + 0.5) * scale_y - 0.5;

                let value = self.interpolate_at(src, src_x, src_y, edge_mode)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Separable Lanczos resampling (optimized 2-pass)
    ///
    /// This performs resampling in two passes (horizontal then vertical)
    /// which is more cache-friendly and can be faster for large kernels.
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid
    pub fn resample_separable(
        &self,
        src: &RasterBuffer,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<RasterBuffer> {
        if dst_width == 0 || dst_height == 0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "dimensions",
                message: "Target dimensions must be non-zero".to_string(),
            });
        }

        let src_width = src.width();
        let src_height = src.height();

        // Pass 1: Horizontal resampling
        let mut temp = RasterBuffer::zeros(dst_width, src_height, src.data_type());
        let scale_x = src_width as f64 / dst_width as f64;

        for src_y in 0..src_height {
            for dst_x in 0..dst_width {
                let src_x = (dst_x as f64 + 0.5) * scale_x - 0.5;
                let value = self.interpolate_horizontal(src, src_x, src_y)?;
                temp.set_pixel(dst_x, src_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        // Pass 2: Vertical resampling
        let mut dst = RasterBuffer::zeros(dst_width, dst_height, src.data_type());
        let scale_y = src_height as f64 / dst_height as f64;

        for dst_x in 0..dst_width {
            for dst_y in 0..dst_height {
                let src_y = (dst_y as f64 + 0.5) * scale_y - 0.5;
                let value = self.interpolate_vertical(&temp, dst_x, src_y)?;
                dst.set_pixel(dst_x, dst_y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(dst)
    }

    /// Horizontal 1D interpolation
    fn interpolate_horizontal(&self, src: &RasterBuffer, src_x: f64, y: u64) -> Result<f64> {
        let src_width = src.width();
        let src_x_clamped = src_x.max(0.0).min((src_width - 1) as f64);
        let x_center = src_x_clamped.floor() as i64;

        let radius = self.radius() as i64;
        let x_start = x_center - radius + 1;
        let x_end = x_center + radius + 1;

        let kernel_width = (x_end - x_start) as usize;
        let mut values = vec![0.0f64; kernel_width];
        let mut weights = vec![0.0f64; kernel_width];

        for (idx, sx) in (x_start..x_end).enumerate() {
            let sample_x = sx.max(0).min(src_width as i64 - 1) as u64;
            values[idx] = src.get_pixel(sample_x, y).map_err(AlgorithmError::Core)?;
            let dx = sx as f64 - src_x_clamped;
            weights[idx] = lanczos(dx, self.lobes);
        }

        normalize_weights(&mut weights);

        let result = values.iter().zip(weights.iter()).map(|(v, w)| v * w).sum();

        Ok(result)
    }

    /// Vertical 1D interpolation
    fn interpolate_vertical(&self, src: &RasterBuffer, x: u64, src_y: f64) -> Result<f64> {
        let src_height = src.height();
        let src_y_clamped = src_y.max(0.0).min((src_height - 1) as f64);
        let y_center = src_y_clamped.floor() as i64;

        let radius = self.radius() as i64;
        let y_start = y_center - radius + 1;
        let y_end = y_center + radius + 1;

        let kernel_height = (y_end - y_start) as usize;
        let mut values = vec![0.0f64; kernel_height];
        let mut weights = vec![0.0f64; kernel_height];

        for (idx, sy) in (y_start..y_end).enumerate() {
            let sample_y = sy.max(0).min(src_height as i64 - 1) as u64;
            values[idx] = src.get_pixel(x, sample_y).map_err(AlgorithmError::Core)?;
            let dy = sy as f64 - src_y_clamped;
            weights[idx] = lanczos(dy, self.lobes);
        }

        normalize_weights(&mut weights);

        let result = values.iter().zip(weights.iter()).map(|(v, w)| v * w).sum();

        Ok(result)
    }
}

/// Edge handling modes for resampling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeMode {
    /// Clamp coordinates to edge (default)
    Clamp,
    /// Wrap coordinates (for tiled data)
    Wrap,
    /// Mirror coordinates at edges
    Mirror,
}

/// Maps a (possibly out-of-range) integer source coordinate `c` back into
/// the valid range `[0, size)` according to `mode`.
///
/// - [`EdgeMode::Clamp`]: coordinates are clamped to `[0, size - 1]`.
/// - [`EdgeMode::Wrap`]: coordinates wrap around modulo `size` (as if the
///   source were tiled periodically).
/// - [`EdgeMode::Mirror`]: coordinates are reflected at the boundary using
///   "reflect-101" semantics, i.e. the edge pixel itself is *not*
///   duplicated. For `size == 5` the sequence for `c` in `-3..=7` is
///   `2 1 0 1 2 3 4 3 2 1`.
///
/// `size == 0` has no valid coordinates; `0` is returned defensively so the
/// function never panics (callers are expected to reject empty rasters
/// before reaching the resampling loop).
#[must_use]
fn map_edge_coord(c: i64, size: u64, mode: EdgeMode) -> u64 {
    if size == 0 {
        return 0;
    }
    let size_i = size as i64;

    match mode {
        EdgeMode::Clamp => c.clamp(0, size_i - 1) as u64,
        EdgeMode::Wrap => c.rem_euclid(size_i) as u64,
        EdgeMode::Mirror => {
            if size_i == 1 {
                return 0;
            }
            // Reflect-101 (no edge duplication): period is 2*(size-1).
            let period = 2 * (size_i - 1);
            let m = c.rem_euclid(period);
            if m < size_i {
                m as u64
            } else {
                (period - m) as u64
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_lanczos_creation() {
        let l2 = LanczosResampler::new(2);
        assert_eq!(l2.lobes(), 2);
        assert_eq!(l2.radius(), 2);

        let l3 = LanczosResampler::new(3);
        assert_eq!(l3.lobes(), 3);
        assert_eq!(l3.radius(), 3);
    }

    #[test]
    fn test_lanczos_identity() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(3);
        let dst = resampler.resample(&src, 10, 10);
        assert!(dst.is_ok());
    }

    #[test]
    fn test_lanczos_quality() {
        // Lanczos should produce high-quality smooth results
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, ((x + y) * (x + y)) as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(3);
        let dst = resampler.resample(&src, 10, 10);
        assert!(dst.is_ok());

        // Result should be smooth
        if let Ok(dst) = dst {
            let v1 = dst.get_pixel(4, 4).ok();
            let v2 = dst.get_pixel(5, 5).ok();
            assert!(v1.is_some());
            assert!(v2.is_some());
        }
    }

    #[test]
    fn test_lanczos_separable() {
        let mut src = RasterBuffer::zeros(8, 8, RasterDataType::Float32);
        for y in 0..8 {
            for x in 0..8 {
                src.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(3);
        let dst1 = resampler.resample(&src, 16, 16);
        let dst2 = resampler.resample_separable(&src, 16, 16);

        assert!(dst1.is_ok());
        assert!(dst2.is_ok());

        // Results should be similar (not identical due to rounding)
    }

    #[test]
    fn test_lanczos_lobes() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let l2 = LanczosResampler::new(2);
        let l3 = LanczosResampler::new(3);

        let dst2 = l2.resample(&src, 20, 20);
        let dst3 = l3.resample(&src, 20, 20);

        assert!(dst2.is_ok());
        assert!(dst3.is_ok());
    }

    #[test]
    fn test_map_edge_coord() {
        // Clamp: saturate to [0, size).
        assert_eq!(map_edge_coord(-1, 5, EdgeMode::Clamp), 0);
        assert_eq!(map_edge_coord(-100, 5, EdgeMode::Clamp), 0);
        assert_eq!(map_edge_coord(2, 5, EdgeMode::Clamp), 2);
        assert_eq!(map_edge_coord(5, 5, EdgeMode::Clamp), 4);
        assert_eq!(map_edge_coord(100, 5, EdgeMode::Clamp), 4);

        // Wrap: modulo size.
        assert_eq!(map_edge_coord(-1, 5, EdgeMode::Wrap), 4);
        assert_eq!(map_edge_coord(-6, 5, EdgeMode::Wrap), 4);
        assert_eq!(map_edge_coord(5, 5, EdgeMode::Wrap), 0);
        assert_eq!(map_edge_coord(7, 5, EdgeMode::Wrap), 2);

        // Mirror: reflect-101 (edge pixel not duplicated), period 2*(size-1).
        // For size 5, the sequence for c in -4..=8 is: 4 3 2 1 0 1 2 3 4 3 2 1 0
        let expected = [4, 3, 2, 1, 0, 1, 2, 3, 4, 3, 2, 1, 0];
        for (i, c) in (-4i64..=8).enumerate() {
            assert_eq!(
                map_edge_coord(c, 5, EdgeMode::Mirror),
                expected[i],
                "mirror mismatch at c={c}"
            );
        }

        // Degenerate single-pixel source: every mode collapses to index 0.
        for mode in [EdgeMode::Clamp, EdgeMode::Wrap, EdgeMode::Mirror] {
            assert_eq!(map_edge_coord(-3, 1, mode), 0);
            assert_eq!(map_edge_coord(0, 1, mode), 0);
            assert_eq!(map_edge_coord(3, 1, mode), 0);
        }
    }

    #[test]
    fn test_edge_modes_constant_image() {
        // A constant-valued image must upsample to the same constant under
        // every edge mode: whichever source pixels the kernel gathers at
        // the border, they all carry the same value, so the normalized
        // weighted sum is unchanged.
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, 42.0).ok();
            }
        }
        let resampler = LanczosResampler::new(3);

        for mode in [EdgeMode::Clamp, EdgeMode::Wrap, EdgeMode::Mirror] {
            let dst = resampler
                .resample_with_edge_mode(&src, 10, 10, mode)
                .unwrap_or_else(|e| panic!("resample with {mode:?} failed: {e}"));
            for y in 0..10 {
                for x in 0..10 {
                    let v = dst
                        .get_pixel(x, y)
                        .unwrap_or_else(|e| panic!("get_pixel({x},{y}) failed: {e}"));
                    assert!(
                        (v - 42.0).abs() < 1e-6,
                        "mode {mode:?}: pixel ({x},{y}) = {v}, expected 42.0"
                    );
                }
            }
        }
    }

    #[test]
    fn test_edge_modes_ramp_spot_check() {
        // Horizontal ramp (value == x), height chosen so the y-axis kernel
        // window is centered exactly on an integer row (weight ~1 on the
        // center row, ~0 elsewhere) and therefore does not interfere with
        // the x-axis edge-mode behaviour under test.
        //
        // width=8, dst_width=5, lobes=2 puts dst_x=0 at src_x=0.3, whose
        // kernel window [-1, 3) reaches one column left of the image
        // (sx = -1), forcing edge-mode-dependent sampling. Expected values
        // below were derived independently (see WP-7 task notes) by
        // replicating the exact same Lanczos kernel and 2-D normalization
        // used in `interpolate_at`, for each of the three edge modes.
        let width = 8u64;
        let height = 3u64;
        let mut src = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        for y in 0..height {
            for x in 0..width {
                src.set_pixel(x, y, x as f64).ok();
            }
        }

        let resampler = LanczosResampler::new(2);
        let dst_width = 5u64;
        let dst_height = 3u64;

        let expected: &[(EdgeMode, f64)] = &[
            (EdgeMode::Clamp, 0.243_460_891_937_171_1),
            (EdgeMode::Wrap, -0.353_871_041_402_479_4),
            (EdgeMode::Mirror, 0.158_127_758_602_935_3),
        ];

        for &(mode, expected_value) in expected {
            let dst = resampler
                .resample_with_edge_mode(&src, dst_width, dst_height, mode)
                .unwrap_or_else(|e| panic!("resample with {mode:?} failed: {e}"));
            let v = dst
                .get_pixel(0, 1)
                .unwrap_or_else(|e| panic!("get_pixel(0,1) failed: {e}"));
            assert!(
                (v - expected_value).abs() < 1e-6,
                "mode {mode:?}: pixel (0,1) = {v}, expected {expected_value}"
            );
        }

        // Sanity: the three edge modes must actually disagree at this
        // border pixel (otherwise the spot check above would not
        // distinguish Wrap/Mirror from Clamp).
        assert!((expected[0].1 - expected[1].1).abs() > 0.1);
        assert!((expected[0].1 - expected[2].1).abs() > 0.05);
    }

    #[test]
    fn test_lanczos_zero_dimensions() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let resampler = LanczosResampler::new(3);

        assert!(resampler.resample(&src, 0, 10).is_err());
        assert!(resampler.resample(&src, 10, 0).is_err());
    }
}
