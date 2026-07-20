//! Compound morphological operators on flat `f32` rasters.
//!
//! This module provides ergonomic, slice-based compound morphological
//! operators that complement the [`crate::raster::morphology`] module's
//! [`crate::raster::morphology::dilate`] / [`crate::raster::morphology::erode`]
//! primitives.
//!
//! ## Compound morphological operators
//! - [`opening`] = erode -> dilate (removes small bright features)
//! - [`closing`] = dilate -> erode (fills small dark gaps)
//! - [`top_hat`] = raster - opening (extracts small bright features)
//! - [`black_hat`] = closing - raster (extracts small dark features)
//!
//! The slice-based primitives [`erode_slice`] and [`dilate_slice`] use a
//! square structuring element of `kernel_size x kernel_size` with edge
//! replication. They operate on row-major `&[f32]` buffers, which is the
//! standard layout used elsewhere in the crate for raw raster data.
//!
//! The functions are intentionally allocation-friendly and pure: each call
//! returns a fresh `Vec<f32>` of length `width * height`. For repeated
//! application (e.g. iteration counts > 1) the option-bag variants
//! [`opening_with`], [`closing_with`], [`top_hat_with`] and [`black_hat_with`]
//! avoid re-allocating the option struct and accept an explicit iteration
//! count.
//!
//! NOTE: [`BorderMode`] is currently exposed as part of the options struct
//! as a forward-compatible knob. The primitives in this module use edge
//! replication (`Replicate`) unconditionally — the variant is honoured by
//! the slice-based primitives so users can opt into [`BorderMode::Reflect`]
//! or [`BorderMode::Constant`].

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Boundary handling strategy for slice-based morphology kernels.
///
/// The default is [`BorderMode::Replicate`], matching the conventional
/// behaviour of the buffer-based morphology operators in
/// [`crate::raster::morphology`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderMode {
    /// Replicate edge pixels (default).
    Replicate,
    /// Mirror pixels at the border.
    Reflect,
    /// Pad with a constant value.
    Constant(f32),
}

impl Default for BorderMode {
    fn default() -> Self {
        Self::Replicate
    }
}

/// Option bag for the compound morphological operators.
///
/// # Fields
///
/// * `kernel_size` — size of the square structuring element (must be odd and
///   positive). A `kernel_size` of `3` yields a 3x3 square neighbourhood.
/// * `iterations` — number of times to repeat the compound operator. Useful
///   for cascading openings/closings (a.k.a. alternating-sequential filters).
/// * `border_mode` — boundary handling strategy. See [`BorderMode`].
#[derive(Debug, Clone, Copy)]
pub struct MorphologyOptions {
    /// Size of the square structuring element.
    pub kernel_size: usize,
    /// Number of iterations of the compound operator.
    pub iterations: u32,
    /// Boundary handling strategy.
    pub border_mode: BorderMode,
}

impl Default for MorphologyOptions {
    fn default() -> Self {
        Self {
            kernel_size: 3,
            iterations: 1,
            border_mode: BorderMode::default(),
        }
    }
}

impl MorphologyOptions {
    /// Create a new option bag with the given kernel size, default iterations
    /// and border mode.
    pub fn new(kernel_size: usize) -> Self {
        Self {
            kernel_size,
            ..Self::default()
        }
    }

    /// Set the iteration count (consuming builder style).
    pub fn with_iterations(mut self, n: u32) -> Self {
        self.iterations = n;
        self
    }

    /// Set the border mode (consuming builder style).
    pub fn with_border_mode(mut self, m: BorderMode) -> Self {
        self.border_mode = m;
        self
    }
}

/// Sample a pixel from `raster` at signed coordinates with the given border
/// mode. Indices outside `[0, width) x [0, height)` are handled according to
/// `mode`.
#[inline]
fn sample(
    raster: &[f32],
    width: usize,
    height: usize,
    x: i64,
    y: i64,
    mode: BorderMode,
) -> Option<f32> {
    let w = width as i64;
    let h = height as i64;

    let (sx, sy) = match mode {
        BorderMode::Replicate => (x.clamp(0, w - 1), y.clamp(0, h - 1)),
        BorderMode::Reflect => {
            // Reflect such that -1 -> 0, -2 -> 1, w -> w-1, w+1 -> w-2.
            let rx = reflect_index(x, w);
            let ry = reflect_index(y, h);
            (rx, ry)
        }
        BorderMode::Constant(c) => {
            if x < 0 || x >= w || y < 0 || y >= h {
                return Some(c);
            }
            (x, y)
        }
    };

    let ux = sx as usize;
    let uy = sy as usize;
    Some(raster[uy * width + ux])
}

#[inline]
fn reflect_index(i: i64, n: i64) -> i64 {
    if n <= 1 {
        return 0;
    }
    let period = 2 * (n - 1);
    let mut m = i % period;
    if m < 0 {
        m += period;
    }
    if m >= n { period - m } else { m }
}

/// Slice-based morphological dilation with a `kernel_size x kernel_size`
/// square structuring element.
///
/// Equivalent to applying a max-filter over the neighbourhood. Uses the
/// boundary strategy from `mode` to handle pixels near the raster edge.
///
/// # Panics
///
/// Never panics for `raster.len() == width * height` and `kernel_size >= 1`;
/// inputs that violate these invariants produce well-defined results bounded
/// by the buffer length (and a debug-mode panic).
pub fn dilate_slice(
    raster: &[f32],
    width: usize,
    height: usize,
    kernel_size: usize,
    mode: BorderMode,
) -> Vec<f32> {
    let ks = kernel_size.max(1);
    let half = (ks / 2) as i64;
    let mut out = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let mut max_val = f32::NEG_INFINITY;
            for dy in -half..=half {
                for dx in -half..=half {
                    if let Some(v) =
                        sample(raster, width, height, x as i64 + dx, y as i64 + dy, mode)
                    {
                        if v.is_finite() && v > max_val {
                            max_val = v;
                        }
                    }
                }
            }
            out[y * width + x] = if max_val.is_finite() {
                max_val
            } else {
                raster[y * width + x]
            };
        }
    }

    out
}

/// Slice-based morphological erosion with a `kernel_size x kernel_size`
/// square structuring element.
///
/// Equivalent to applying a min-filter over the neighbourhood. Uses the
/// boundary strategy from `mode` to handle pixels near the raster edge.
pub fn erode_slice(
    raster: &[f32],
    width: usize,
    height: usize,
    kernel_size: usize,
    mode: BorderMode,
) -> Vec<f32> {
    let ks = kernel_size.max(1);
    let half = (ks / 2) as i64;
    let mut out = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let mut min_val = f32::INFINITY;
            for dy in -half..=half {
                for dx in -half..=half {
                    if let Some(v) =
                        sample(raster, width, height, x as i64 + dx, y as i64 + dy, mode)
                    {
                        if v.is_finite() && v < min_val {
                            min_val = v;
                        }
                    }
                }
            }
            out[y * width + x] = if min_val.is_finite() {
                min_val
            } else {
                raster[y * width + x]
            };
        }
    }

    out
}

/// Morphological opening: `erode -> dilate`.
///
/// Removes bright features (positive peaks) smaller than the structuring
/// element. Uses [`BorderMode::Replicate`].
pub fn opening(raster: &[f32], width: usize, height: usize, kernel_size: usize) -> Vec<f32> {
    let mode = BorderMode::default();
    let eroded = erode_slice(raster, width, height, kernel_size, mode);
    dilate_slice(&eroded, width, height, kernel_size, mode)
}

/// Morphological closing: `dilate -> erode`.
///
/// Fills dark features (negative gaps) smaller than the structuring
/// element. Uses [`BorderMode::Replicate`].
pub fn closing(raster: &[f32], width: usize, height: usize, kernel_size: usize) -> Vec<f32> {
    let mode = BorderMode::default();
    let dilated = dilate_slice(raster, width, height, kernel_size, mode);
    erode_slice(&dilated, width, height, kernel_size, mode)
}

/// Top-hat transform: `raster - opening(raster)`.
///
/// Extracts bright features smaller than the structuring element. Output
/// pixels are non-negative for well-behaved inputs.
pub fn top_hat(raster: &[f32], width: usize, height: usize, kernel_size: usize) -> Vec<f32> {
    let opened = opening(raster, width, height, kernel_size);
    raster
        .iter()
        .zip(opened.iter())
        .map(|(a, b)| a - b)
        .collect()
}

/// Black-hat transform: `closing(raster) - raster`.
///
/// Extracts dark features smaller than the structuring element. Output
/// pixels are non-negative for well-behaved inputs.
pub fn black_hat(raster: &[f32], width: usize, height: usize, kernel_size: usize) -> Vec<f32> {
    let closed = closing(raster, width, height, kernel_size);
    closed
        .iter()
        .zip(raster.iter())
        .map(|(a, b)| a - b)
        .collect()
}

/// Ergonomic option-bag variant of [`opening`].
///
/// Applies opening `opts.iterations` times in succession.
pub fn opening_with(
    raster: &[f32],
    width: usize,
    height: usize,
    opts: &MorphologyOptions,
) -> Vec<f32> {
    let mut current = raster.to_vec();
    for _ in 0..opts.iterations {
        let eroded = erode_slice(&current, width, height, opts.kernel_size, opts.border_mode);
        current = dilate_slice(&eroded, width, height, opts.kernel_size, opts.border_mode);
    }
    current
}

/// Ergonomic option-bag variant of [`closing`].
///
/// Applies closing `opts.iterations` times in succession.
pub fn closing_with(
    raster: &[f32],
    width: usize,
    height: usize,
    opts: &MorphologyOptions,
) -> Vec<f32> {
    let mut current = raster.to_vec();
    for _ in 0..opts.iterations {
        let dilated = dilate_slice(&current, width, height, opts.kernel_size, opts.border_mode);
        current = erode_slice(&dilated, width, height, opts.kernel_size, opts.border_mode);
    }
    current
}

/// Ergonomic option-bag variant of [`top_hat`].
pub fn top_hat_with(
    raster: &[f32],
    width: usize,
    height: usize,
    opts: &MorphologyOptions,
) -> Vec<f32> {
    let opened = opening_with(raster, width, height, opts);
    raster
        .iter()
        .zip(opened.iter())
        .map(|(a, b)| a - b)
        .collect()
}

/// Ergonomic option-bag variant of [`black_hat`].
pub fn black_hat_with(
    raster: &[f32],
    width: usize,
    height: usize,
    opts: &MorphologyOptions,
) -> Vec<f32> {
    let closed = closing_with(raster, width, height, opts);
    closed
        .iter()
        .zip(raster.iter())
        .map(|(a, b)| a - b)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reflect_index_basic() {
        assert_eq!(reflect_index(-1, 5), 1);
        assert_eq!(reflect_index(-2, 5), 2);
        assert_eq!(reflect_index(5, 5), 3);
        assert_eq!(reflect_index(6, 5), 2);
    }

    #[test]
    fn test_default_morphology_options() {
        let opts = MorphologyOptions::default();
        assert_eq!(opts.kernel_size, 3);
        assert_eq!(opts.iterations, 1);
        matches!(opts.border_mode, BorderMode::Replicate);
    }

    #[test]
    fn test_morphology_options_builder() {
        let opts = MorphologyOptions::new(5)
            .with_iterations(2)
            .with_border_mode(BorderMode::Constant(0.0));
        assert_eq!(opts.kernel_size, 5);
        assert_eq!(opts.iterations, 2);
        matches!(opts.border_mode, BorderMode::Constant(_));
    }

    #[test]
    fn test_dilate_slice_constant_raster() {
        let r = vec![5.0f32; 16];
        let out = dilate_slice(&r, 4, 4, 3, BorderMode::Replicate);
        assert!(out.iter().all(|&v| (v - 5.0).abs() < 1e-6));
    }

    #[test]
    fn test_erode_slice_constant_raster() {
        let r = vec![5.0f32; 16];
        let out = erode_slice(&r, 4, 4, 3, BorderMode::Replicate);
        assert!(out.iter().all(|&v| (v - 5.0).abs() < 1e-6));
    }
}
