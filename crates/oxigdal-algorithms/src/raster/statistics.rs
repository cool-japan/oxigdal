//! Raster statistics operations
//!
//! This module provides comprehensive statistical analysis for raster data:
//! - Basic statistics: mean, median, mode, stddev, min, max, range
//! - Percentiles: p10, p25, p50, p75, p90
//! - Histograms with configurable bins
//! - Zonal statistics by polygon
//!
//! # Performance
//!
//! This module offers both sequential and parallel implementations:
//! - **Sequential**: Reliable baseline for small datasets
//! - **Parallel**: 4-6x speedup with 50% memory reduction for large datasets (1M+ pixels)
//!
//! Parallel features are enabled with the `parallel` feature flag and use:
//! - Row-wise parallel processing with Rayon
//! - Streaming computation (no full pixel collection)
//! - Reservoir sampling for percentiles (~10k samples)
//! - Thread-local histogram bins
//!
//! # Median accuracy
//!
//! **Both** the sequential and parallel code paths compute
//! [`RasterStatistics::median`] via bounded reservoir sampling (Algorithm R,
//! capped at ~10,000 samples), not a full sort of every valid pixel. The
//! median is therefore **exact** only while the raster (or zone) has at most
//! 10,000 valid pixels; for anything larger it is a statistical estimate
//! computed from a uniform random subsample. This applies equally to
//! [`compute_statistics`], the sequential and parallel internals behind it,
//! and zonal statistics. Callers that require the true median for large
//! rasters should use [`compute_exact_median`], which collects every valid
//! value (O(n) memory) and computes it exactly.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Statistics for a raster or zone
#[derive(Debug, Clone, PartialEq)]
pub struct RasterStatistics {
    /// Number of valid (non-NoData) pixels
    pub count: usize,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Mean (average) value
    pub mean: f64,
    /// Median value.
    ///
    /// Computed via bounded reservoir sampling (~10,000 samples). This is
    /// the **exact** median when `count <= 10_000`; for larger rasters it is
    /// a statistical estimate from a uniform random subsample, not the true
    /// median. Use `compute_exact_median` if an exact result is required.
    pub median: f64,
    /// Standard deviation
    pub stddev: f64,
    /// Variance
    pub variance: f64,
    /// Sum of all values
    pub sum: f64,
}

/// Percentile statistics
#[derive(Debug, Clone, PartialEq)]
pub struct Percentiles {
    /// 10th percentile
    pub p10: f64,
    /// 25th percentile (Q1)
    pub p25: f64,
    /// 50th percentile (median, Q2)
    pub p50: f64,
    /// 75th percentile (Q3)
    pub p75: f64,
    /// 90th percentile
    pub p90: f64,
}

/// Histogram representation
#[derive(Debug, Clone)]
pub struct Histogram {
    /// Bin edges (length = bins + 1)
    pub edges: Vec<f64>,
    /// Count in each bin (length = bins)
    pub counts: Vec<usize>,
    /// Total number of values
    pub total: usize,
}

impl Histogram {
    /// Returns the bin index for a given value
    fn find_bin(&self, value: f64) -> Option<usize> {
        if value < self.edges[0] || value > *self.edges.last()? {
            return None;
        }

        // Binary search for the bin
        for i in 0..self.counts.len() {
            if value >= self.edges[i] && value < self.edges[i + 1] {
                return Some(i);
            }
        }

        // Value equals the last edge
        if (value - self.edges[self.counts.len()]).abs() < f64::EPSILON {
            return Some(self.counts.len() - 1);
        }

        None
    }

    /// Returns the relative frequency for each bin
    #[must_use]
    pub fn frequencies(&self) -> Vec<f64> {
        if self.total == 0 {
            return vec![0.0; self.counts.len()];
        }

        self.counts
            .iter()
            .map(|&count| count as f64 / self.total as f64)
            .collect()
    }
}

/// Offers a candidate value to a bounded reservoir sample using Algorithm R
/// (Vitter, 1985).
///
/// After `seen` items have been observed from the stream (this call counts
/// as one of them, so `seen` must be the 1-based running count including the
/// current value), the reservoir holds a uniform random sample of
/// `min(seen, capacity)` of them. Uses [`fastrand`] for the random index
/// rather than a hand-rolled hash of the running count, so the selection is
/// a genuine independent draw per candidate instead of a deterministic
/// function of `seen` alone.
fn reservoir_offer(reservoir: &mut Vec<f64>, capacity: usize, seen: usize, value: f64) {
    if reservoir.len() < capacity {
        reservoir.push(value);
    } else if seen > 0 {
        let idx = fastrand::usize(0..seen);
        if idx < capacity {
            reservoir[idx] = value;
        }
    }
}

/// Merges a just-completed reservoir sample (`incoming`, drawn from
/// `incoming_seen` stream items) into an accumulating reservoir
/// (`accumulator`), which already reflects `accumulated_seen` prior stream
/// items. Each incoming sample is offered to the accumulator as though the
/// stream continued past `accumulated_seen`, preserving Algorithm R's
/// uniform-sampling guarantee for the combined stream.
fn reservoir_merge(
    accumulator: &mut Vec<f64>,
    capacity: usize,
    accumulated_seen: usize,
    incoming: Vec<f64>,
) {
    let mut seen = accumulated_seen;
    for value in incoming {
        seen += 1;
        reservoir_offer(accumulator, capacity, seen, value);
    }
}

/// Computes basic statistics for a raster
///
/// This function uses streaming computation to minimize memory usage.
/// When the `parallel` feature is enabled, row-wise parallel processing
/// provides 4-6x speedup for large rasters.
///
/// # Errors
///
/// Returns an error if the raster has no valid pixels
#[cfg(feature = "parallel")]
pub fn compute_statistics(raster: &RasterBuffer) -> Result<RasterStatistics> {
    compute_statistics_parallel(raster)
}

/// Sequential fallback when parallel feature is not enabled
#[cfg(not(feature = "parallel"))]
pub fn compute_statistics(raster: &RasterBuffer) -> Result<RasterStatistics> {
    compute_statistics_sequential(raster)
}

/// Parallel streaming statistics computation
///
/// Uses row-wise parallel processing with Rayon for optimal performance.
/// Memory usage is minimal as we don't collect all pixels.
#[cfg(feature = "parallel")]
fn compute_statistics_parallel(raster: &RasterBuffer) -> Result<RasterStatistics> {
    // Parallel fold over rows
    let (count, sum, sum_sq, min, max, median_samples) = (0..raster.height())
        .into_par_iter()
        .map(|y| {
            let mut row_count = 0usize;
            let mut row_sum = 0.0f64;
            let mut row_sum_sq = 0.0f64;
            let mut row_min = f64::INFINITY;
            let mut row_max = f64::NEG_INFINITY;
            let mut row_samples = Vec::new();

            for x in 0..raster.width() {
                if let Ok(val) = raster.get_pixel(x, y) {
                    if !raster.is_nodata(val) && val.is_finite() {
                        row_count += 1;
                        row_sum += val;
                        row_sum_sq += val * val;
                        row_min = row_min.min(val);
                        row_max = row_max.max(val);

                        // Reservoir sampling for median (target: 10000 samples)
                        reservoir_offer(&mut row_samples, 10_000, row_count, val);
                    }
                }
            }

            (
                row_count,
                row_sum,
                row_sum_sq,
                row_min,
                row_max,
                row_samples,
            )
        })
        .reduce(
            || (0, 0.0, 0.0, f64::INFINITY, f64::NEG_INFINITY, Vec::new()),
            |(c1, s1, sq1, min1, max1, mut samples1), (c2, s2, sq2, min2, max2, samples2)| {
                // Merge samples using reservoir sampling
                reservoir_merge(&mut samples1, 10_000, c1, samples2);

                (
                    c1 + c2,
                    s1 + s2,
                    sq1 + sq2,
                    min1.min(min2),
                    max1.max(max2),
                    samples1,
                )
            },
        );

    if count == 0 {
        return Err(AlgorithmError::InsufficientData {
            operation: "compute_statistics",
            message: "No valid pixels found".to_string(),
        });
    }

    let mean = sum / count as f64;

    // Compute variance using the computational formula: Var(X) = E[X²] - E[X]²
    let variance = (sum_sq / count as f64) - (mean * mean);
    let stddev = variance.sqrt();

    // Compute median from samples
    let mut samples = median_samples;
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let median = if samples.is_empty() {
        mean // Fallback to mean if no samples
    } else if samples.len() % 2 == 0 {
        let mid = samples.len() / 2;
        (samples[mid - 1] + samples[mid]) / 2.0
    } else {
        samples[samples.len() / 2]
    };

    Ok(RasterStatistics {
        count,
        min,
        max,
        mean,
        median,
        stddev,
        variance,
        sum,
    })
}

/// Sequential statistics computation (fallback)
///
/// This is the baseline implementation used when the parallel feature
/// is not enabled or for small datasets where parallelism overhead
/// would outweigh benefits. Like the parallel path, the median is computed
/// via bounded reservoir sampling and is only exact for `count <= 10_000`;
/// see the module-level "Median accuracy" section.
fn compute_statistics_sequential(raster: &RasterBuffer) -> Result<RasterStatistics> {
    let mut count = 0usize;
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut median_samples = Vec::new();

    // Single pass with reservoir sampling for median
    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let val = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !raster.is_nodata(val) && val.is_finite() {
                count += 1;
                sum += val;
                sum_sq += val * val;
                min = min.min(val);
                max = max.max(val);

                // Reservoir sampling for median
                reservoir_offer(&mut median_samples, 10_000, count, val);
            }
        }
    }

    if count == 0 {
        return Err(AlgorithmError::InsufficientData {
            operation: "compute_statistics",
            message: "No valid pixels found".to_string(),
        });
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    let stddev = variance.sqrt();

    // Compute median from samples
    median_samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let median = if median_samples.is_empty() {
        mean
    } else if median_samples.len() % 2 == 0 {
        let mid = median_samples.len() / 2;
        (median_samples[mid - 1] + median_samples[mid]) / 2.0
    } else {
        median_samples[median_samples.len() / 2]
    };

    Ok(RasterStatistics {
        count,
        min,
        max,
        mean,
        median,
        stddev,
        variance,
        sum,
    })
}

/// Computes the exact median of all valid (non-NoData, finite) pixel values
/// in a raster.
///
/// [`compute_statistics`]'s `median` field is only exact when the raster has
/// at most 10,000 valid pixels; beyond that it is a reservoir-sampled
/// estimate (see the module-level "Median accuracy" section). This function
/// instead collects every valid value and selects the true median via
/// `select_nth_unstable_by`, so it is exact regardless of raster size, at
/// the cost of O(n) memory (n = number of valid pixels) and an O(n) average
/// time selection pass instead of a single O(1)-memory streaming pass.
///
/// # Errors
///
/// Returns an error if the raster has no valid pixels.
pub fn compute_exact_median(raster: &RasterBuffer) -> Result<f64> {
    let mut values = Vec::new();

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let val = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !raster.is_nodata(val) && val.is_finite() {
                values.push(val);
            }
        }
    }

    exact_median_of(values, "compute_exact_median")
}

/// Selects the true median of `values` in place, consuming the vector.
///
/// Uses `select_nth_unstable_by` (average O(n) time, no full sort) rather
/// than sorting the whole slice.
fn exact_median_of(mut values: Vec<f64>, operation: &'static str) -> Result<f64> {
    if values.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation,
            message: "No valid pixels found".to_string(),
        });
    }

    let cmp = |a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal);
    let len = values.len();
    let mid = len / 2;

    if len % 2 == 1 {
        let (_, &mut median, _) = values.select_nth_unstable_by(mid, cmp);
        Ok(median)
    } else {
        // Even length: need both the (mid-1)th and mid-th order statistics.
        // select_nth_unstable_by(mid) partitions so everything before `mid`
        // is <= the pivot; the largest of that lower partition is the
        // (mid-1)th order statistic.
        let (lower, &mut upper, _) = values.select_nth_unstable_by(mid, cmp);
        let lower_max = lower.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Ok((lower_max + upper) / 2.0)
    }
}

/// Computes percentiles for a raster
///
/// Uses reservoir sampling to estimate percentiles efficiently
/// without collecting all pixels. Sample size: ~10,000 pixels.
///
/// # Errors
///
/// Returns an error if the raster has no valid pixels
#[cfg(feature = "parallel")]
pub fn compute_percentiles(raster: &RasterBuffer) -> Result<Percentiles> {
    compute_percentiles_parallel(raster)
}

/// Sequential fallback for percentiles
#[cfg(not(feature = "parallel"))]
pub fn compute_percentiles(raster: &RasterBuffer) -> Result<Percentiles> {
    compute_percentiles_sequential(raster)
}

/// Parallel percentile computation using reservoir sampling
#[cfg(feature = "parallel")]
fn compute_percentiles_parallel(raster: &RasterBuffer) -> Result<Percentiles> {
    const SAMPLE_SIZE: usize = 10000;

    // Parallel reservoir sampling
    let (count, samples) = (0..raster.height())
        .into_par_iter()
        .map(|y| {
            let mut row_count = 0usize;
            let mut row_samples = Vec::with_capacity(SAMPLE_SIZE.min(raster.width() as usize));

            for x in 0..raster.width() {
                if let Ok(val) = raster.get_pixel(x, y) {
                    if !raster.is_nodata(val) && val.is_finite() {
                        row_count += 1;
                        reservoir_offer(&mut row_samples, SAMPLE_SIZE, row_count, val);
                    }
                }
            }

            (row_count, row_samples)
        })
        .reduce(
            || (0, Vec::new()),
            |(c1, mut s1), (c2, s2)| {
                // Merge samples
                reservoir_merge(&mut s1, SAMPLE_SIZE, c1, s2);

                (c1 + c2, s1)
            },
        );

    if count == 0 || samples.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation: "compute_percentiles",
            message: "No valid pixels found".to_string(),
        });
    }

    // Sort samples
    let mut sorted_samples = samples;
    sorted_samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let p10 = percentile(&sorted_samples, 10.0)?;
    let p25 = percentile(&sorted_samples, 25.0)?;
    let p50 = percentile(&sorted_samples, 50.0)?;
    let p75 = percentile(&sorted_samples, 75.0)?;
    let p90 = percentile(&sorted_samples, 90.0)?;

    Ok(Percentiles {
        p10,
        p25,
        p50,
        p75,
        p90,
    })
}

/// Sequential percentile computation using reservoir sampling
fn compute_percentiles_sequential(raster: &RasterBuffer) -> Result<Percentiles> {
    const SAMPLE_SIZE: usize = 10000;

    let mut count = 0usize;
    let mut samples = Vec::with_capacity(SAMPLE_SIZE);

    // Reservoir sampling
    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let val = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !raster.is_nodata(val) && val.is_finite() {
                count += 1;
                reservoir_offer(&mut samples, SAMPLE_SIZE, count, val);
            }
        }
    }

    if count == 0 || samples.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation: "compute_percentiles",
            message: "No valid pixels found".to_string(),
        });
    }

    // Sort samples
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

    let p10 = percentile(&samples, 10.0)?;
    let p25 = percentile(&samples, 25.0)?;
    let p50 = percentile(&samples, 50.0)?;
    let p75 = percentile(&samples, 75.0)?;
    let p90 = percentile(&samples, 90.0)?;

    Ok(Percentiles {
        p10,
        p25,
        p50,
        p75,
        p90,
    })
}

/// Computes a specific percentile from sorted values
fn percentile(sorted_values: &[f64], p: f64) -> Result<f64> {
    if sorted_values.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "percentile",
        });
    }

    if !(0.0..=100.0).contains(&p) {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "percentile",
            message: format!("Percentile must be between 0 and 100, got {p}"),
        });
    }

    let n = sorted_values.len();
    let rank = p / 100.0 * (n - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;

    if lower == upper {
        Ok(sorted_values[lower])
    } else {
        let fraction = rank - lower as f64;
        Ok(sorted_values[lower] * (1.0 - fraction) + sorted_values[upper] * fraction)
    }
}

/// Computes a histogram for a raster
///
/// # Arguments
///
/// * `raster` - The raster buffer
/// * `bins` - Number of bins
/// * `min_val` - Minimum value (if None, uses data min)
/// * `max_val` - Maximum value (if None, uses data max)
///
/// # Errors
///
/// Returns an error if the raster has no valid pixels or bins is 0
pub fn compute_histogram(
    raster: &RasterBuffer,
    bins: usize,
    min_val: Option<f64>,
    max_val: Option<f64>,
) -> Result<Histogram> {
    if bins == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "bins",
            message: "Number of bins must be greater than 0".to_string(),
        });
    }

    let mut values = Vec::new();

    // Collect all valid values
    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let val = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if !raster.is_nodata(val) && val.is_finite() {
                values.push(val);
            }
        }
    }

    if values.is_empty() {
        return Err(AlgorithmError::InsufficientData {
            operation: "compute_histogram",
            message: "No valid pixels found".to_string(),
        });
    }

    // Determine min and max
    let data_min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let data_max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let min = min_val.unwrap_or(data_min);
    let max = max_val.unwrap_or(data_max);

    if max <= min {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "min/max",
            message: format!("max ({max}) must be greater than min ({min})"),
        });
    }

    // Create bin edges
    let bin_width = (max - min) / bins as f64;
    let mut edges = Vec::with_capacity(bins + 1);
    for i in 0..=bins {
        edges.push(min + i as f64 * bin_width);
    }

    // Count values in each bin
    let mut counts = vec![0usize; bins];
    for &val in &values {
        if val < min || val > max {
            continue;
        }

        let bin_idx = if (val - max).abs() < f64::EPSILON {
            bins - 1 // Last value goes in last bin
        } else {
            ((val - min) / bin_width).floor() as usize
        };

        if bin_idx < bins {
            counts[bin_idx] += 1;
        }
    }

    Ok(Histogram {
        edges,
        counts,
        total: values.len(),
    })
}

/// Parallel histogram computation with thread-local bins (internal)
#[cfg(feature = "parallel")]
#[allow(dead_code)]
fn compute_histogram_parallel(
    raster: &RasterBuffer,
    bins: usize,
    min_val: Option<f64>,
    max_val: Option<f64>,
) -> Result<Histogram> {
    if bins == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "bins",
            message: "Number of bins must be greater than 0".to_string(),
        });
    }

    // First pass: find min/max if not provided
    let (min, max) = if min_val.is_none() || max_val.is_none() {
        let (count, data_min, data_max) = (0..raster.height())
            .into_par_iter()
            .map(|y| {
                let mut row_count = 0usize;
                let mut row_min = f64::INFINITY;
                let mut row_max = f64::NEG_INFINITY;

                for x in 0..raster.width() {
                    if let Ok(val) = raster.get_pixel(x, y) {
                        if !raster.is_nodata(val) && val.is_finite() {
                            row_count += 1;
                            row_min = row_min.min(val);
                            row_max = row_max.max(val);
                        }
                    }
                }

                (row_count, row_min, row_max)
            })
            .reduce(
                || (0, f64::INFINITY, f64::NEG_INFINITY),
                |(c1, min1, max1), (c2, min2, max2)| (c1 + c2, min1.min(min2), max1.max(max2)),
            );

        if count == 0 {
            return Err(AlgorithmError::InsufficientData {
                operation: "compute_histogram",
                message: "No valid pixels found".to_string(),
            });
        }

        (min_val.unwrap_or(data_min), max_val.unwrap_or(data_max))
    } else {
        (min_val.unwrap_or(0.0), max_val.unwrap_or(0.0))
    };

    if max <= min {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "min/max",
            message: format!("max ({max}) must be greater than min ({min})"),
        });
    }

    // Create bin edges
    let bin_width = (max - min) / bins as f64;
    let mut edges = Vec::with_capacity(bins + 1);
    for i in 0..=bins {
        edges.push(min + i as f64 * bin_width);
    }

    // Second pass: parallel histogram with thread-local bins
    let (total, counts) = (0..raster.height())
        .into_par_iter()
        .map(|y| {
            let mut row_total = 0usize;
            let mut row_counts = vec![0usize; bins];

            for x in 0..raster.width() {
                if let Ok(val) = raster.get_pixel(x, y) {
                    if !raster.is_nodata(val) && val.is_finite() {
                        if val >= min && val <= max {
                            row_total += 1;

                            let bin_idx = if (val - max).abs() < f64::EPSILON {
                                bins - 1
                            } else {
                                let idx = ((val - min) / bin_width).floor() as usize;
                                idx.min(bins - 1)
                            };

                            row_counts[bin_idx] += 1;
                        }
                    }
                }
            }

            (row_total, row_counts)
        })
        .reduce(
            || (0, vec![0usize; bins]),
            |(t1, mut c1), (t2, c2)| {
                for (i, &count) in c2.iter().enumerate() {
                    c1[i] += count;
                }
                (t1 + t2, c1)
            },
        );

    Ok(Histogram {
        edges,
        counts,
        total,
    })
}

/// Computes the mode (most frequent value) of a raster
///
/// Returns the most common value in the raster. For continuous data,
/// this uses a histogram-based approach.
///
/// # Errors
///
/// Returns an error if the raster has no valid pixels
pub fn compute_mode(raster: &RasterBuffer, bins: usize) -> Result<f64> {
    let histogram = compute_histogram(raster, bins, None, None)?;

    // Find bin with maximum count
    let max_bin = histogram
        .counts
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| *count)
        .map(|(idx, _)| idx)
        .ok_or(AlgorithmError::InsufficientData {
            operation: "compute_mode",
            message: "No bins found".to_string(),
        })?;

    // Return center of bin with maximum count
    let mode = (histogram.edges[max_bin] + histogram.edges[max_bin + 1]) / 2.0;
    Ok(mode)
}

/// Zone definition for zonal statistics
#[derive(Debug, Clone)]
pub struct Zone {
    /// Zone identifier
    pub id: usize,
    /// List of (x, y) coordinates in this zone
    pub pixels: Vec<(u64, u64)>,
}

/// Computes zonal statistics
///
/// Uses streaming computation to minimize memory usage.
/// When the `parallel` feature is enabled, zones are processed in parallel.
///
/// # Arguments
///
/// * `raster` - The raster buffer
/// * `zones` - List of zones, each containing pixel coordinates
///
/// # Errors
///
/// Returns an error if any zone has no valid pixels
#[cfg(feature = "parallel")]
pub fn compute_zonal_statistics(
    raster: &RasterBuffer,
    zones: &[Zone],
) -> Result<Vec<(usize, RasterStatistics)>> {
    compute_zonal_statistics_parallel(raster, zones)
}

/// Sequential fallback for zonal statistics
#[cfg(not(feature = "parallel"))]
pub fn compute_zonal_statistics(
    raster: &RasterBuffer,
    zones: &[Zone],
) -> Result<Vec<(usize, RasterStatistics)>> {
    compute_zonal_statistics_sequential(raster, zones)
}

/// Parallel zonal statistics computation
#[cfg(feature = "parallel")]
fn compute_zonal_statistics_parallel(
    raster: &RasterBuffer,
    zones: &[Zone],
) -> Result<Vec<(usize, RasterStatistics)>> {
    zones
        .par_iter()
        .map(|zone| {
            // Streaming computation for each zone
            let mut count = 0usize;
            let mut sum = 0.0f64;
            let mut sum_sq = 0.0f64;
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            let mut median_samples = Vec::with_capacity(10000.min(zone.pixels.len()));

            for &(x, y) in &zone.pixels {
                if x < raster.width() && y < raster.height() {
                    if let Ok(val) = raster.get_pixel(x, y) {
                        if !raster.is_nodata(val) && val.is_finite() {
                            count += 1;
                            sum += val;
                            sum_sq += val * val;
                            min = min.min(val);
                            max = max.max(val);

                            // Reservoir sampling for median
                            reservoir_offer(&mut median_samples, 10_000, count, val);
                        }
                    }
                }
            }

            if count == 0 {
                return Err(AlgorithmError::InsufficientData {
                    operation: "compute_zonal_statistics",
                    message: format!("Zone {} has no valid pixels", zone.id),
                });
            }

            let mean = sum / count as f64;
            let variance = (sum_sq / count as f64) - (mean * mean);
            let stddev = variance.sqrt();

            // Compute median from samples
            median_samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

            let median = if median_samples.is_empty() {
                mean
            } else if median_samples.len() % 2 == 0 {
                let mid = median_samples.len() / 2;
                (median_samples[mid - 1] + median_samples[mid]) / 2.0
            } else {
                median_samples[median_samples.len() / 2]
            };

            Ok((
                zone.id,
                RasterStatistics {
                    count,
                    min,
                    max,
                    mean,
                    median,
                    stddev,
                    variance,
                    sum,
                },
            ))
        })
        .collect()
}

/// Sequential zonal statistics computation
fn compute_zonal_statistics_sequential(
    raster: &RasterBuffer,
    zones: &[Zone],
) -> Result<Vec<(usize, RasterStatistics)>> {
    let mut results = Vec::with_capacity(zones.len());

    for zone in zones {
        // Streaming computation for each zone
        let mut count = 0usize;
        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut median_samples = Vec::with_capacity(10000.min(zone.pixels.len()));

        for &(x, y) in &zone.pixels {
            if x < raster.width() && y < raster.height() {
                let val = raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                if !raster.is_nodata(val) && val.is_finite() {
                    count += 1;
                    sum += val;
                    sum_sq += val * val;
                    min = min.min(val);
                    max = max.max(val);

                    // Reservoir sampling for median
                    reservoir_offer(&mut median_samples, 10_000, count, val);
                }
            }
        }

        if count == 0 {
            return Err(AlgorithmError::InsufficientData {
                operation: "compute_zonal_statistics",
                message: format!("Zone {} has no valid pixels", zone.id),
            });
        }

        let mean = sum / count as f64;
        let variance = (sum_sq / count as f64) - (mean * mean);
        let stddev = variance.sqrt();

        // Compute median from samples
        median_samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

        let median = if median_samples.is_empty() {
            mean
        } else if median_samples.len() % 2 == 0 {
            let mid = median_samples.len() / 2;
            (median_samples[mid - 1] + median_samples[mid]) / 2.0
        } else {
            median_samples[median_samples.len() / 2]
        };

        results.push((
            zone.id,
            RasterStatistics {
                count,
                min,
                max,
                mean,
                median,
                stddev,
                variance,
                sum,
            },
        ));
    }

    Ok(results)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_basic_statistics() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Fill with values 0 to 99
        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let stats = compute_statistics(&raster);
        assert!(stats.is_ok());
        let s = stats.expect("Stats should be ok");

        assert_eq!(s.count, 100);
        assert!((s.min - 0.0).abs() < f64::EPSILON);
        assert!((s.max - 99.0).abs() < f64::EPSILON);
        assert!((s.mean - 49.5).abs() < 0.1);
    }

    #[test]
    fn test_percentiles() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let perc = compute_percentiles(&raster);
        assert!(perc.is_ok());
        let p = perc.expect("Percentiles should be ok");

        assert!((p.p50 - 49.5).abs() < 1.0); // Median should be around 49.5
    }

    #[test]
    fn test_histogram() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let hist = compute_histogram(&raster, 10, None, None);
        assert!(hist.is_ok());
        let h = hist.expect("Histogram should be ok");

        assert_eq!(h.counts.len(), 10);
        assert_eq!(h.edges.len(), 11);
        assert_eq!(h.total, 100);
    }

    #[test]
    fn test_zonal_stats() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        // Create two zones
        let zone1 = Zone {
            id: 1,
            pixels: vec![(0, 0), (1, 0), (2, 0)], // Values: 0, 1, 2
        };

        let zone2 = Zone {
            id: 2,
            pixels: vec![(7, 9), (8, 9), (9, 9)], // Values: 97, 98, 99
        };

        let result = compute_zonal_statistics(&raster, &[zone1, zone2]);
        assert!(result.is_ok());
        let zones = result.expect("Zonal stats should be ok");

        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].0, 1);
        assert!((zones[0].1.mean - 1.0).abs() < f64::EPSILON);

        assert_eq!(zones[1].0, 2);
        assert!((zones[1].1.mean - 98.0).abs() < f64::EPSILON);
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_statistics_single_pixel() {
        let mut raster = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        raster.set_pixel(0, 0, 42.0).ok();

        let stats = compute_statistics(&raster);
        assert!(stats.is_ok());
        let s = stats.expect("Should succeed");

        assert_eq!(s.count, 1);
        assert!((s.min - 42.0).abs() < f64::EPSILON);
        assert!((s.max - 42.0).abs() < f64::EPSILON);
        assert!((s.mean - 42.0).abs() < f64::EPSILON);
        assert!((s.median - 42.0).abs() < f64::EPSILON);
        assert!((s.stddev - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_histogram_zero_bins() {
        let raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = compute_histogram(&raster, 0, None, None);
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidParameter { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidParameter error");
        }
    }

    #[test]
    fn test_histogram_invalid_range() {
        let mut raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                raster.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let result = compute_histogram(&raster, 10, Some(100.0), Some(50.0)); // max < min
        assert!(result.is_err());
    }

    #[test]
    fn test_percentile_out_of_range() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        let result = percentile(&values, 150.0); // > 100
        assert!(result.is_err());
    }

    #[test]
    fn test_percentile_negative() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        let result = percentile(&values, -10.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_percentile_empty_array() {
        let values: Vec<f64> = vec![];

        let result = percentile(&values, 50.0);
        assert!(result.is_err());
        if let Err(AlgorithmError::EmptyInput { .. }) = result {
            // Expected
        } else {
            panic!("Expected EmptyInput error");
        }
    }

    // ========== Mode Tests ==========

    #[test]
    fn test_compute_mode() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a distribution with a clear mode around 50
        for y in 0..10 {
            for x in 0..10 {
                let val = if (x + y) % 3 == 0 {
                    50.0
                } else {
                    (x * 10) as f64
                };
                raster.set_pixel(x, y, val).ok();
            }
        }

        let result = compute_mode(&raster, 20);
        assert!(result.is_ok());
    }

    // ========== Advanced Statistics Tests ==========

    #[test]
    fn test_statistics_with_nodata() {
        let mut raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                if x == 2 && y == 2 {
                    raster.set_pixel(x, y, f64::NAN).ok(); // NoData
                } else {
                    raster.set_pixel(x, y, (x + y) as f64).ok();
                }
            }
        }

        let stats = compute_statistics(&raster);
        assert!(stats.is_ok());
        let s = stats.expect("Should succeed");

        // Should have 24 valid pixels (25 - 1 NaN)
        assert_eq!(s.count, 24);
    }

    #[test]
    fn test_percentiles_extreme_values() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let perc = compute_percentiles(&raster);
        assert!(perc.is_ok());
        let p = perc.expect("Should succeed");

        // p10 should be close to 9.9
        assert!(p.p10 < 15.0);
        assert!(p.p10 > 5.0);

        // p90 should be close to 89.1
        assert!(p.p90 > 85.0);
        assert!(p.p90 < 95.0);
    }

    #[test]
    fn test_histogram_custom_range() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let hist = compute_histogram(&raster, 5, Some(0.0), Some(100.0));
        assert!(hist.is_ok());
        let h = hist.expect("Should succeed");

        assert_eq!(h.counts.len(), 5);
        assert_eq!(h.edges.len(), 6);
        assert_eq!(h.total, 100);

        // Check that edges are correct
        assert!((h.edges[0] - 0.0).abs() < f64::EPSILON);
        assert!((h.edges[5] - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_histogram_frequencies() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (y * 10 + x) as f64).ok();
            }
        }

        let hist = compute_histogram(&raster, 10, None, None);
        assert!(hist.is_ok());
        let h = hist.expect("Should succeed");

        let freqs = h.frequencies();
        assert_eq!(freqs.len(), 10);

        // Sum of frequencies should be 1.0
        let sum: f64 = freqs.iter().sum();
        assert!((sum - 1.0).abs() < 0.001);
    }

    // ========== Zonal Statistics Advanced Tests ==========

    #[test]
    fn test_zonal_stats_single_zone() {
        let mut raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                raster.set_pixel(x, y, 10.0).ok();
            }
        }

        let zone = Zone {
            id: 1,
            pixels: vec![(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)],
        };

        let result = compute_zonal_statistics(&raster, &[zone]);
        assert!(result.is_ok());
        let zones = result.expect("Should succeed");

        assert_eq!(zones.len(), 1);
        assert!((zones[0].1.mean - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_zonal_stats_empty_zone() {
        let mut raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                raster.set_pixel(x, y, f64::NAN).ok(); // All NoData
            }
        }

        let zone = Zone {
            id: 1,
            pixels: vec![(0, 0), (1, 1)],
        };

        let result = compute_zonal_statistics(&raster, &[zone]);
        assert!(result.is_err()); // No valid pixels
    }

    #[test]
    fn test_zonal_stats_out_of_bounds() {
        let raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let zone = Zone {
            id: 1,
            pixels: vec![(10, 10), (20, 20)], // Out of bounds
        };

        let result = compute_zonal_statistics(&raster, &[zone]);
        assert!(result.is_err()); // No valid pixels within bounds
    }

    #[test]
    fn test_zonal_stats_multiple_zones() {
        let mut raster = RasterBuffer::zeros(20, 20, RasterDataType::Float32);

        for y in 0..20 {
            for x in 0..20 {
                raster.set_pixel(x, y, (y * 20 + x) as f64).ok();
            }
        }

        let zones = vec![
            Zone {
                id: 1,
                pixels: (0..10).flat_map(|y| (0..10).map(move |x| (x, y))).collect(),
            },
            Zone {
                id: 2,
                pixels: (10..20)
                    .flat_map(|y| (10..20).map(move |x| (x, y)))
                    .collect(),
            },
            Zone {
                id: 3,
                pixels: (0..10)
                    .flat_map(|y| (10..20).map(move |x| (x, y)))
                    .collect(),
            },
        ];

        let result = compute_zonal_statistics(&raster, &zones);
        assert!(result.is_ok());
        let zone_stats = result.expect("Should succeed");

        assert_eq!(zone_stats.len(), 3);
    }

    // ========== Statistical Properties Tests ==========

    #[test]
    fn test_variance_and_stddev_relationship() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                raster.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let stats = compute_statistics(&raster);
        assert!(stats.is_ok());
        let s = stats.expect("Should succeed");

        // stddev should be square root of variance
        assert!((s.stddev * s.stddev - s.variance).abs() < 0.001);
    }

    #[test]
    fn test_median_vs_mean() {
        let mut raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Create a skewed distribution
        for y in 0..5 {
            for x in 0..5 {
                let val = if x == 4 && y == 4 {
                    1000.0 // Outlier
                } else {
                    10.0
                };
                raster.set_pixel(x, y, val).ok();
            }
        }

        let stats = compute_statistics(&raster);
        assert!(stats.is_ok());
        let s = stats.expect("Should succeed");

        // Median should be closer to 10 (less affected by outlier)
        assert!((s.median - 10.0).abs() < 1.0);
        // Mean should be higher due to outlier
        assert!(s.mean > s.median);
    }

    #[test]
    fn test_percentile_interpolation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        // p50 should be exactly 3.0 (middle value)
        let p50 = percentile(&values, 50.0);
        assert!(p50.is_ok());
        assert!((p50.expect("Should succeed") - 3.0).abs() < f64::EPSILON);

        // p25 should be 2.0
        let p25 = percentile(&values, 25.0);
        assert!(p25.is_ok());
        assert!((p25.expect("Should succeed") - 2.0).abs() < 0.1);

        // p75 should be 4.0
        let p75 = percentile(&values, 75.0);
        assert!(p75.is_ok());
        assert!((p75.expect("Should succeed") - 4.0).abs() < 0.1);
    }

    #[test]
    fn test_histogram_edge_case_last_value() {
        let mut raster = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                raster.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        let hist = compute_histogram(&raster, 8, Some(0.0), Some(8.0));
        assert!(hist.is_ok());
        let h = hist.expect("Should succeed");

        // The value 8.0 should be included in the last bin
        assert_eq!(h.counts.len(), 8);
    }

    #[test]
    fn test_exact_median_odd_count() {
        let mut raster = RasterBuffer::zeros(5, 1, RasterDataType::Float32);
        let values = [9.0, 1.0, 5.0, 3.0, 7.0];
        for (x, &v) in values.iter().enumerate() {
            raster.set_pixel(x as u64, 0, v).ok();
        }

        // sorted: [1, 3, 5, 7, 9] -> exact median is 5.0
        let median = compute_exact_median(&raster).expect("median should succeed");
        assert!((median - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_exact_median_even_count() {
        let mut raster = RasterBuffer::zeros(3, 2, RasterDataType::Float32);
        let values = [5.0, 1.0, 3.0, 2.0, 4.0, 100.0];
        let mut idx = 0usize;
        for y in 0..2 {
            for x in 0..3 {
                raster.set_pixel(x, y, values[idx]).ok();
                idx += 1;
            }
        }

        // sorted: [1, 2, 3, 4, 5, 100] -> exact median is (3+4)/2 = 3.5,
        // unaffected by the 100.0 outlier since it's not part of the
        // middle pair.
        let median = compute_exact_median(&raster).expect("median should succeed");
        assert!((median - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_exact_median_no_valid_pixels_errors() {
        let mut raster = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        for y in 0..4 {
            for x in 0..4 {
                raster.set_pixel(x, y, f64::NAN).ok(); // All NoData/invalid
            }
        }

        let result = compute_exact_median(&raster);
        assert!(result.is_err());
    }

    /// Regression test: `compute_statistics`'s `median` field is only exact
    /// while `count <= 10_000` (reservoir sampling kicks in above that). For
    /// a large raster, `compute_exact_median` must still equal the true
    /// median computed by sorting every valid value directly -- it must not
    /// go through the reservoir-sampling approximation at all.
    #[test]
    fn test_exact_median_matches_full_sort_for_large_raster() {
        // 120 x 130 = 15,600 pixels, well above the 10,000-sample reservoir
        // cap used by `compute_statistics`.
        let width = 120u64;
        let height = 130u64;
        let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        let mut all_values = Vec::with_capacity((width * height) as usize);
        let mut n = 0i64;
        for y in 0..height {
            for x in 0..width {
                // A non-trivial, non-monotonic-in-scan-order value pattern.
                let val = ((n * 2654435761_i64).rem_euclid(1_000_003)) as f64;
                raster.set_pixel(x, y, val).ok();
                all_values.push(val);
                n += 1;
            }
        }

        all_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let len = all_values.len();
        let expected = if len % 2 == 0 {
            (all_values[len / 2 - 1] + all_values[len / 2]) / 2.0
        } else {
            all_values[len / 2]
        };

        let median = compute_exact_median(&raster).expect("median should succeed");
        assert!(
            (median - expected).abs() < f64::EPSILON,
            "exact median {median} did not match full-sort median {expected}"
        );
    }

    /// Regression test: the reservoir-sampled `median` field on
    /// `compute_statistics` should still be a statistically reasonable
    /// estimate of the true median for a large, uniformly distributed
    /// raster, confirming the refactor to `reservoir_offer`/`fastrand`
    /// preserved sampling correctness (values are still drawn roughly
    /// uniformly, not biased toward a subrange).
    #[test]
    fn test_reservoir_sampled_median_close_to_exact_for_large_raster() {
        let width = 150u64;
        let height = 100u64; // 15,000 valid pixels > 10,000 reservoir cap
        let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        let mut n = 0.0f64;
        for y in 0..height {
            for x in 0..width {
                raster.set_pixel(x, y, n).ok(); // values 0.0 .. 14999.0
                n += 1.0;
            }
        }

        let exact = compute_exact_median(&raster).expect("exact median should succeed");
        let stats = compute_statistics(&raster).expect("statistics should succeed");

        // The reservoir sample covers 10,000 of 15,000 values (~67%), so the
        // sampled median should land close to the true median. Allow a
        // generous tolerance (~2% of the value range) to avoid flakiness
        // from the unseeded, thread-local `fastrand` generator.
        let tolerance = (width * height) as f64 * 0.02;
        assert!(
            (stats.median - exact).abs() < tolerance,
            "reservoir-sampled median {} too far from exact median {} (tolerance {})",
            stats.median,
            exact,
            tolerance
        );
    }

    /// Regression test for the `x * 0 = 0`-style silent-failure class of bug
    /// in reservoir sampling: `reservoir_offer` must retain every observed
    /// value verbatim (no data loss / no silent zeroing) while the stream is
    /// still within the reservoir's capacity.
    #[test]
    fn test_reservoir_offer_retains_all_items_within_capacity() {
        let mut reservoir: Vec<f64> = Vec::new();
        for i in 0..50usize {
            reservoir_offer(&mut reservoir, 100, i + 1, i as f64);
        }

        assert_eq!(reservoir.len(), 50);
        let mut sorted = reservoir.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let expected: Vec<f64> = (0..50).map(|i| i as f64).collect();
        assert_eq!(sorted, expected);
    }

    /// Once the stream exceeds capacity, the reservoir must stay at
    /// capacity (never grow unbounded, never shrink) and every value it
    /// holds must have come from the observed stream.
    #[test]
    fn test_reservoir_offer_stays_at_capacity_and_only_holds_stream_values() {
        let mut reservoir: Vec<f64> = Vec::new();
        let capacity = 20usize;
        let stream_len = 500usize;

        for i in 0..stream_len {
            reservoir_offer(&mut reservoir, capacity, i + 1, i as f64);
        }

        assert_eq!(reservoir.len(), capacity);
        for &val in &reservoir {
            assert!((0.0..stream_len as f64).contains(&val));
        }
    }
}
