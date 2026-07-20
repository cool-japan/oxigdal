//! Floating-point compression
//!
//! Specialized compression for floating-point geospatial data with
//! configurable error bounds and precision control.

pub mod sz;
pub mod zfp;

pub use self::{
    sz::{SzCodec, SzConfig, SzMode},
    zfp::{ZfpCodec, ZfpConfig, ZfpMode},
};

/// Floating-point compression mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FpMode {
    /// Fixed rate (bits per value)
    FixedRate(usize),
    /// Fixed precision (bit planes to keep)
    FixedPrecision(usize),
    /// Fixed accuracy (maximum error bound)
    FixedAccuracy(f64),
    /// Reversible (lossless)
    Reversible,
}

/// Floating-point compression statistics
#[derive(Debug, Clone)]
pub struct FpStats {
    /// Original data range
    pub min_value: f64,
    /// Maximum value
    pub max_value: f64,
    /// Mean value
    pub mean: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Actual compression ratio
    pub compression_ratio: f64,
    /// Maximum absolute error
    pub max_error: f64,
    /// Root mean square error
    pub rmse: f64,
    /// Peak signal-to-noise ratio (dB)
    pub psnr: f64,
}

impl FpStats {
    /// Create new statistics
    pub fn new() -> Self {
        Self {
            min_value: 0.0,
            max_value: 0.0,
            mean: 0.0,
            std_dev: 0.0,
            compression_ratio: 1.0,
            max_error: 0.0,
            rmse: 0.0,
            psnr: 0.0,
        }
    }

    /// Compute statistics from original and decompressed data
    pub fn compute(original: &[f64], decompressed: &[f64]) -> Self {
        let mut stats = Self::new();

        if original.is_empty() {
            return stats;
        }

        // Compute range and mean
        stats.min_value = original[0];
        stats.max_value = original[0];
        let mut sum = 0.0;

        for &val in original {
            stats.min_value = stats.min_value.min(val);
            stats.max_value = stats.max_value.max(val);
            sum += val;
        }

        stats.mean = sum / original.len() as f64;

        // Compute standard deviation
        let mut var_sum = 0.0;
        for &val in original {
            let diff = val - stats.mean;
            var_sum += diff * diff;
        }
        stats.std_dev = (var_sum / original.len() as f64).sqrt();

        // Compute error metrics
        if original.len() == decompressed.len() {
            let mut max_err: f64 = 0.0;
            let mut mse: f64 = 0.0;

            for (orig, decomp) in original.iter().zip(decompressed.iter()) {
                let err = (orig - decomp).abs();
                max_err = max_err.max(err);
                mse += err * err;
            }

            stats.max_error = max_err;
            stats.rmse = (mse / original.len() as f64).sqrt();

            // PSNR calculation
            let range = stats.max_value - stats.min_value;
            if mse > 0.0 && range > 0.0 {
                stats.psnr = 20.0 * (range / stats.rmse).log10();
            }
        }

        stats
    }
}

impl Default for FpStats {
    fn default() -> Self {
        Self::new()
    }
}
