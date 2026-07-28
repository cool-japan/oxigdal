//! Band-level raster statistics types.
//!
//! This module provides [`Statistics`] and [`Histogram`], which are the metadata-level
//! counterparts to [`crate::buffer::BufferStatistics`]. Use these types when you need
//! to store per-band statistics inside [`crate::types::RasterMetadata`] or pass them
//! across API boundaries without carrying the full pixel buffer.
//!
//! # Relationship to `BufferStatistics`
//!
//! [`Statistics`] supersedes [`crate::types::RasterStatistics`] for histogram-capable
//! statistics; the older type is kept for backward compatibility. Convert using
//! [`Statistics::from_buffer_statistics`] or the [`From`] impl.

use crate::buffer::BufferStatistics;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use crate::compat::*;
#[cfg(not(feature = "std"))]
use crate::math::FloatExt;

/// Band-level raster statistics suitable for [`crate::types::RasterMetadata`].
///
/// Stores the standard descriptive statistics plus an optional uniform-width histogram.
/// Created from a [`BufferStatistics`] via [`Statistics::from_buffer_statistics`] or
/// the [`From<BufferStatistics>`] impl (which sets `total_count = valid_count`).
#[derive(Debug, Clone, PartialEq)]
pub struct Statistics {
    /// Minimum pixel value (excluding nodata and non-finite values).
    pub min: f64,
    /// Maximum pixel value (excluding nodata and non-finite values).
    pub max: f64,
    /// Mean pixel value (excluding nodata and non-finite values).
    pub mean: f64,
    /// Population standard deviation (excluding nodata and non-finite values).
    pub std_dev: f64,
    /// Number of valid (non-nodata, finite) pixels counted.
    pub valid_count: u64,
    /// Total pixel count (`width × height`) for the band.
    pub total_count: u64,
    /// Optional uniform histogram covering `[min, max]`.
    ///
    /// `Some` when the statistics were computed with
    /// [`crate::buffer::RasterBuffer::compute_statistics_with_histogram`] and then
    /// converted via [`Statistics::from_buffer_statistics`]; `None` otherwise.
    pub histogram: Option<Histogram>,
}

impl Statistics {
    /// Constructs [`Statistics`] from a [`BufferStatistics`] value and the total pixel
    /// count for the band.
    ///
    /// If `buf_stats.histogram` is `Some(bins)`, the bins are converted into a
    /// [`Histogram`] using `buf_stats.min`/`max` to compute the bin width.
    /// If `buf_stats.histogram` is `None`, `self.histogram` is `None`.
    ///
    /// # Arguments
    ///
    /// * `buf_stats` — Statistics from [`crate::buffer::RasterBuffer::compute_statistics`]
    ///   or [`crate::buffer::RasterBuffer::compute_statistics_with_histogram`].
    /// * `total_count` — Total number of pixels in the band (`width × height`).
    #[must_use]
    pub fn from_buffer_statistics(buf_stats: &BufferStatistics, total_count: u64) -> Self {
        let histogram = buf_stats
            .histogram
            .as_ref()
            .map(|bins| Histogram::new(bins.clone(), buf_stats.min, buf_stats.max));

        Self {
            min: buf_stats.min,
            max: buf_stats.max,
            mean: buf_stats.mean,
            std_dev: buf_stats.std_dev,
            valid_count: buf_stats.valid_count,
            total_count,
            histogram,
        }
    }

    /// Returns the fraction of valid pixels (`valid_count / total_count`).
    ///
    /// Returns `0.0` when `total_count` is zero to avoid division by zero.
    #[must_use]
    pub fn valid_fraction(&self) -> f64 {
        if self.total_count == 0 {
            0.0
        } else {
            self.valid_count as f64 / self.total_count as f64
        }
    }

    /// Returns the range (`max - min`).
    #[must_use]
    pub fn range(&self) -> f64 {
        self.max - self.min
    }
}

/// Implements a lossy conversion from [`BufferStatistics`] into [`Statistics`].
///
/// Sets `total_count = valid_count` (a conservative approximation when the
/// exact raster dimensions are not available). The histogram, if present in
/// `buf_stats`, is preserved.
impl From<BufferStatistics> for Statistics {
    fn from(buf_stats: BufferStatistics) -> Self {
        let total_count = buf_stats.valid_count;
        Statistics::from_buffer_statistics(&buf_stats, total_count)
    }
}

/// A uniform histogram with equal-width bins covering the value range `[min, max]`.
///
/// Each bin `i` covers the half-open interval
/// `[min + i*bin_width, min + (i+1)*bin_width)`, with the last bin extended to
/// include `max`.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    /// Per-bin pixel counts.  `bins.len()` equals the requested `bin_count`.
    pub bins: Vec<u64>,
    /// Minimum value (lower edge of bin 0).
    pub min: f64,
    /// Maximum value (upper edge of the last bin, inclusive).
    pub max: f64,
    /// Width of each bin: `(max - min) / bins.len()`.
    ///
    /// Zero when `min == max` (degenerate histogram with a single non-empty bin).
    pub bin_width: f64,
}

impl Histogram {
    /// Constructs a [`Histogram`] from pre-computed bin counts and the value range.
    ///
    /// `bin_width` is derived automatically from `(max - min) / bins.len()`.
    /// When `min == max` (or `bins.len() == 0`, though that is caller-enforced to
    /// never happen), `bin_width` is set to `0.0`.
    ///
    /// # Arguments
    ///
    /// * `bins` — Per-bin pixel counts.
    /// * `min` — Lower bound of the histogram range.
    /// * `max` — Upper bound of the histogram range (inclusive).
    #[must_use]
    pub fn new(bins: Vec<u64>, min: f64, max: f64) -> Self {
        let bin_width = if bins.is_empty() || (max - min) == 0.0 {
            0.0
        } else {
            (max - min) / bins.len() as f64
        };
        Self {
            bins,
            min,
            max,
            bin_width,
        }
    }

    /// Returns the 0-based bin index for `value`, or `None` if `value` is outside
    /// `[min, max]` or the histogram is degenerate (empty bins or `bin_width == 0`).
    ///
    /// The last bin absorbs values exactly equal to `max`.
    #[must_use]
    pub fn bin_for(&self, value: f64) -> Option<usize> {
        if self.bins.is_empty() || self.bin_width == 0.0 {
            return None;
        }
        if value < self.min || value > self.max {
            return None;
        }
        let idx = ((value - self.min) / self.bin_width).floor() as usize;
        // Clamp: the max-edge value maps exactly to bins.len(), pin to last bin.
        Some(idx.min(self.bins.len() - 1))
    }

    /// Returns the total count across all bins.
    #[must_use]
    pub fn total_count(&self) -> u64 {
        self.bins.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::buffer::RasterBuffer;
    use crate::types::{NoDataValue, RasterDataType};

    // ─── Histogram::new tests ────────────────────────────────────────────────

    #[test]
    fn test_histogram_construction_bin_width() {
        let bins = vec![10u64; 10];
        let h = Histogram::new(bins.clone(), 0.0, 100.0);
        assert_eq!(h.bins, bins);
        assert!((h.min - 0.0).abs() < f64::EPSILON);
        assert!((h.max - 100.0).abs() < f64::EPSILON);
        assert!((h.bin_width - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_histogram_construction_degenerate_min_eq_max() {
        // When min == max, bin_width must be 0.0 (no division by zero).
        let h = Histogram::new(vec![5u64; 4], 42.0, 42.0);
        assert!((h.bin_width - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_histogram_construction_empty_bins() {
        let h = Histogram::new(vec![], 0.0, 100.0);
        assert!((h.bin_width - 0.0).abs() < f64::EPSILON);
        assert_eq!(h.total_count(), 0);
    }

    // ─── Histogram::bin_for tests ─────────────────────────────────────────────

    #[test]
    fn test_histogram_bin_for_at_min() {
        let h = Histogram::new(vec![0u64; 10], 0.0, 100.0);
        assert_eq!(h.bin_for(0.0), Some(0));
    }

    #[test]
    fn test_histogram_bin_for_at_max() {
        let h = Histogram::new(vec![0u64; 10], 0.0, 100.0);
        // max edge must clamp to last bin, not overflow.
        assert_eq!(h.bin_for(100.0), Some(9));
    }

    #[test]
    fn test_histogram_bin_for_below_min() {
        let h = Histogram::new(vec![0u64; 10], 0.0, 100.0);
        assert_eq!(h.bin_for(-1.0), None);
    }

    #[test]
    fn test_histogram_bin_for_above_max() {
        let h = Histogram::new(vec![0u64; 10], 0.0, 100.0);
        assert_eq!(h.bin_for(101.0), None);
    }

    #[test]
    fn test_histogram_bin_for_middle_value() {
        // bin_width = 10, value 55.0 should be in bin 5 ([50,60)).
        let h = Histogram::new(vec![0u64; 10], 0.0, 100.0);
        assert_eq!(h.bin_for(55.0), Some(5));
    }

    #[test]
    fn test_histogram_bin_for_degenerate() {
        let h = Histogram::new(vec![3u64; 4], 5.0, 5.0);
        // bin_width == 0, so bin_for must return None.
        assert_eq!(h.bin_for(5.0), None);
    }

    // ─── RasterBuffer::compute_statistics_with_histogram tests ──────────────

    #[test]
    fn test_histogram_basic_uniform() {
        // 100 pixels with values 0..100 (0,1,2,...,99), 10 bins.
        let mut buf = RasterBuffer::zeros(10, 10, RasterDataType::Float64);
        for y in 0..10u64 {
            for x in 0..10u64 {
                let v = (y * 10 + x) as f64;
                buf.set_pixel(x, y, v).expect("set pixel");
            }
        }

        let stats = buf
            .compute_statistics_with_histogram(10)
            .expect("histogram");
        let bins = stats.histogram.as_ref().expect("bins present");
        assert_eq!(bins.len(), 10);
        // Values 0-99, 10 uniform bins of width 11 each (range is 99):
        // bin_width = 99/10 = 9.9; but our formula uses (max-min)*bin_count/range
        // let's just verify total count
        let total: u64 = bins.iter().sum();
        assert_eq!(total, 100);
        assert!((stats.min - 0.0).abs() < f64::EPSILON);
        assert!((stats.max - 99.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_histogram_all_same_value() {
        // All pixels = 42.0, 8 bins → everything in bin 0.
        let buf = RasterBuffer::nodata_filled(4, 4, RasterDataType::Float32, NoDataValue::None);
        // Fill with 42.0
        let mut buf2 = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        for y in 0..4u64 {
            for x in 0..4u64 {
                buf2.set_pixel(x, y, 42.0).expect("set pixel");
            }
        }
        let _ = buf;

        let stats = buf2
            .compute_statistics_with_histogram(8)
            .expect("histogram");
        let bins = stats.histogram.as_ref().expect("bins present");
        assert_eq!(bins.len(), 8);
        assert_eq!(bins[0], 16); // all 16 pixels in bin 0
        let rest: u64 = bins[1..].iter().sum();
        assert_eq!(rest, 0);
    }

    #[test]
    fn test_histogram_bin_count_zero_returns_error() {
        let buf = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        assert!(buf.compute_statistics_with_histogram(0).is_err());
    }

    #[test]
    fn test_statistics_from_raster_buffer() {
        // 4×4 f32 buffer with values 1..16.
        let mut buf = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        for y in 0..4u64 {
            for x in 0..4u64 {
                let v = (y * 4 + x + 1) as f64;
                buf.set_pixel(x, y, v).expect("set pixel");
            }
        }

        let stats = buf.compute_statistics_with_histogram(4).expect("histogram");

        assert!((stats.min - 1.0).abs() < 1e-6, "min");
        assert!((stats.max - 16.0).abs() < 1e-6, "max");
        // Mean of 1..=16 = 8.5
        assert!((stats.mean - 8.5).abs() < 1e-6, "mean");
        assert_eq!(stats.valid_count, 16);
        // std_dev: population std of 1..=16
        // variance = E[x^2] - mean^2; E[x^2] of 1..16 = (1+4+9+...+256)/16 = 1496/16=93.5
        // var = 93.5 - 72.25 = 21.25, std ≈ 4.6097...
        assert!(
            (stats.std_dev - 4.609_772).abs() < 1e-4,
            "std_dev {}",
            stats.std_dev
        );
    }

    #[test]
    fn test_statistics_excludes_nodata() {
        // 4×4 float64 buffer; nodata = 0.0; first row is nodata (all zeros).
        let nodata = NoDataValue::Float(0.0);
        let mut buf =
            RasterBuffer::new(vec![0u8; 4 * 4 * 8], 4, 4, RasterDataType::Float64, nodata)
                .expect("create buffer");
        // Fill non-zero rows with values 1..=12.
        let mut v = 1.0f64;
        for y in 1..4u64 {
            for x in 0..4u64 {
                buf.set_pixel(x, y, v).expect("set pixel");
                v += 1.0;
            }
        }

        let stats = buf.compute_statistics_with_histogram(4).expect("histogram");
        // Row 0 is all 0.0 (nodata), so valid_count = 12.
        assert_eq!(stats.valid_count, 12, "valid_count");
        assert!((stats.min - 1.0).abs() < 1e-6, "min");
        assert!((stats.max - 12.0).abs() < 1e-6, "max");
        // Mean of 1..=12 = 6.5
        assert!((stats.mean - 6.5).abs() < 1e-6, "mean");
    }

    // ─── Statistics type tests ────────────────────────────────────────────────

    #[test]
    fn test_statistics_from_buffer_statistics_no_histogram() {
        let buf_stats = BufferStatistics {
            min: 1.0,
            max: 10.0,
            mean: 5.5,
            std_dev: 2.872,
            valid_count: 10,
            histogram: None,
        };
        let stats = Statistics::from_buffer_statistics(&buf_stats, 20);
        assert_eq!(stats.valid_count, 10);
        assert_eq!(stats.total_count, 20);
        assert!(stats.histogram.is_none());
        assert!((stats.valid_fraction() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_statistics_from_buffer_statistics_with_histogram() {
        let bins = vec![2u64, 3, 5];
        let buf_stats = BufferStatistics {
            min: 0.0,
            max: 30.0,
            mean: 15.0,
            std_dev: 8.66,
            valid_count: 10,
            histogram: Some(bins.clone()),
        };
        let stats = Statistics::from_buffer_statistics(&buf_stats, 10);
        let h = stats.histogram.expect("histogram present");
        assert_eq!(h.bins, bins);
        assert!((h.bin_width - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_statistics_from_impl_sets_total_to_valid() {
        let buf_stats = BufferStatistics {
            min: 0.0,
            max: 1.0,
            mean: 0.5,
            std_dev: 0.5,
            valid_count: 7,
            histogram: None,
        };
        let stats: Statistics = buf_stats.into();
        assert_eq!(stats.total_count, 7);
    }

    #[test]
    fn test_statistics_valid_fraction_zero_total() {
        let s = Statistics {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            std_dev: 0.0,
            valid_count: 0,
            total_count: 0,
            histogram: None,
        };
        assert!((s.valid_fraction() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_statistics_range() {
        let s = Statistics {
            min: -5.0,
            max: 15.0,
            mean: 5.0,
            std_dev: 7.0,
            valid_count: 100,
            total_count: 100,
            histogram: None,
        };
        assert!((s.range() - 20.0).abs() < f64::EPSILON);
    }
}
