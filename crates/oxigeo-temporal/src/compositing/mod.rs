//! Temporal Compositing Module
//!
//! Implements various temporal compositing methods for creating representative
//! rasters from time series including median, mean, max NDVI, and quality-weighted composites.

use crate::error::{Result, TemporalError};
use crate::timeseries::{TemporalRasterEntry, TimeSeriesRaster};
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

#[cfg(feature = "parallel")]
#[allow(unused_imports)]
use rayon::prelude::*;

pub mod max_ndvi;
pub mod mean;
pub mod median;

/// Compositing method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompositingMethod {
    /// Median composite (per band)
    Median,
    /// Mean composite (per band)
    Mean,
    /// Maximum value composite (MVC)
    Maximum,
    /// Minimum value composite
    Minimum,
    /// Maximum NDVI composite
    MaxNDVI,
    /// Quality-weighted composite
    QualityWeighted,
    /// First valid value
    FirstValid,
    /// Last valid value
    LastValid,
}

/// Compositing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositingConfig {
    /// Compositing method
    pub method: CompositingMethod,
    /// Maximum cloud cover threshold
    pub max_cloud_cover: Option<f32>,
    /// Minimum quality score
    pub min_quality: Option<f32>,
    /// NoData value
    pub nodata: Option<f64>,
    /// Red band index for NDVI (0-based)
    pub red_band: Option<usize>,
    /// NIR band index for NDVI (0-based)
    pub nir_band: Option<usize>,
}

impl Default for CompositingConfig {
    fn default() -> Self {
        Self {
            method: CompositingMethod::Median,
            max_cloud_cover: None,
            min_quality: None,
            nodata: Some(f64::NAN),
            red_band: Some(0),
            nir_band: Some(1),
        }
    }
}

/// Composite result
#[derive(Debug, Clone)]
pub struct CompositeResult {
    /// Composited raster data
    pub data: Array3<f64>,
    /// Number of valid observations per pixel
    pub count: Array3<usize>,
    /// Quality scores (if applicable)
    pub quality: Option<Array3<f64>>,
}

impl CompositeResult {
    /// Create new composite result
    #[must_use]
    pub fn new(data: Array3<f64>, count: Array3<usize>) -> Self {
        Self {
            data,
            count,
            quality: None,
        }
    }

    /// Add quality scores
    #[must_use]
    pub fn with_quality(mut self, quality: Array3<f64>) -> Self {
        self.quality = Some(quality);
        self
    }
}

/// Returns `true` if an entry passes the configured cloud-cover filter.
///
/// Entries with no recorded cloud cover, or when no threshold is configured,
/// always pass.
fn entry_passes_cloud_filter(entry: &TemporalRasterEntry, config: &CompositingConfig) -> bool {
    match (config.max_cloud_cover, entry.metadata.cloud_cover) {
        (Some(max_cc), Some(cc)) => cc <= max_cc,
        _ => true,
    }
}

/// Returns `true` if `value` is a valid (non-NaN, non-nodata) sample under the
/// configured `nodata` value.
fn value_is_valid(value: f64, config: &CompositingConfig) -> bool {
    if value.is_nan() {
        return false;
    }
    match config.nodata {
        Some(nodata) if !nodata.is_nan() => value != nodata,
        _ => true,
    }
}

/// Replaces any pixel that received zero valid observations (still holding the
/// `±infinity` seed used by max/min reductions) with the configured `nodata`
/// value, or `NaN` if none is configured, so downstream consumers never see a
/// raw infinity sentinel.
fn replace_empty_pixels(
    composite: &mut Array3<f64>,
    count: &Array3<usize>,
    config: &CompositingConfig,
) {
    let fill = config.nodata.unwrap_or(f64::NAN);
    let (h, w, b) = composite.dim();
    for i in 0..h {
        for j in 0..w {
            for k in 0..b {
                if count[[i, j, k]] == 0 {
                    composite[[i, j, k]] = fill;
                }
            }
        }
    }
}

/// Temporal compositor
pub struct TemporalCompositor;

impl TemporalCompositor {
    /// Create temporal composite
    ///
    /// # Errors
    /// Returns error if compositing fails
    pub fn composite(ts: &TimeSeriesRaster, config: &CompositingConfig) -> Result<CompositeResult> {
        match config.method {
            CompositingMethod::Median => Self::median_composite(ts, config),
            CompositingMethod::Mean => Self::mean_composite(ts, config),
            CompositingMethod::Maximum => Self::max_composite(ts, config),
            CompositingMethod::Minimum => Self::min_composite(ts, config),
            CompositingMethod::MaxNDVI => Self::max_ndvi_composite(ts, config),
            CompositingMethod::QualityWeighted => Self::quality_weighted_composite(ts, config),
            CompositingMethod::FirstValid => Self::first_valid_composite(ts, config),
            CompositingMethod::LastValid => Self::last_valid_composite(ts, config),
        }
    }

    /// Median composite
    fn median_composite(
        ts: &TimeSeriesRaster,
        config: &CompositingConfig,
    ) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let mut values = Vec::new();

                    for entry in ts.entries().values() {
                        // Apply filters
                        if let Some(max_cc) = config.max_cloud_cover
                            && let Some(cc) = entry.metadata.cloud_cover
                            && cc > max_cc
                        {
                            continue;
                        }

                        if let Some(data) = &entry.data {
                            let value = data[[i, j, k]];
                            if let Some(nodata) = config.nodata {
                                if !value.is_nan() && value != nodata {
                                    values.push(value);
                                }
                            } else if !value.is_nan() {
                                values.push(value);
                            }
                        }
                    }

                    if !values.is_empty() {
                        values
                            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        let median = if values.len() % 2 == 0 {
                            (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
                        } else {
                            values[values.len() / 2]
                        };
                        composite[[i, j, k]] = median;
                        count[[i, j, k]] = values.len();
                    }
                }
            }
        }

        info!("Created median composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Mean composite
    fn mean_composite(
        ts: &TimeSeriesRaster,
        config: &CompositingConfig,
    ) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let mut sum = 0.0;
                    let mut n = 0;

                    for entry in ts.entries().values() {
                        if let Some(max_cc) = config.max_cloud_cover
                            && let Some(cc) = entry.metadata.cloud_cover
                            && cc > max_cc
                        {
                            continue;
                        }

                        if let Some(data) = &entry.data {
                            let value = data[[i, j, k]];
                            if let Some(nodata) = config.nodata {
                                if !value.is_nan() && value != nodata {
                                    sum += value;
                                    n += 1;
                                }
                            } else if !value.is_nan() {
                                sum += value;
                                n += 1;
                            }
                        }
                    }

                    if n > 0 {
                        composite[[i, j, k]] = sum / n as f64;
                        count[[i, j, k]] = n;
                    }
                }
            }
        }

        info!("Created mean composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Maximum value composite
    fn max_composite(ts: &TimeSeriesRaster, config: &CompositingConfig) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite = Array3::from_elem((height, width, n_bands), f64::NEG_INFINITY);
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    for entry in ts.entries().values() {
                        if !entry_passes_cloud_filter(entry, config) {
                            continue;
                        }
                        if let Some(data) = &entry.data {
                            let value = data[[i, j, k]];
                            if value_is_valid(value, config) {
                                if value > composite[[i, j, k]] {
                                    composite[[i, j, k]] = value;
                                }
                                count[[i, j, k]] += 1;
                            }
                        }
                    }
                }
            }
        }

        // Any pixel with no valid observations still holds NEG_INFINITY; map it
        // back to the configured nodata (or NaN) so callers never see a raw
        // -inf sentinel.
        replace_empty_pixels(&mut composite, &count, config);

        info!("Created maximum value composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Minimum value composite
    fn min_composite(ts: &TimeSeriesRaster, config: &CompositingConfig) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite = Array3::from_elem((height, width, n_bands), f64::INFINITY);
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    for entry in ts.entries().values() {
                        if !entry_passes_cloud_filter(entry, config) {
                            continue;
                        }
                        if let Some(data) = &entry.data {
                            let value = data[[i, j, k]];
                            if value_is_valid(value, config) {
                                if value < composite[[i, j, k]] {
                                    composite[[i, j, k]] = value;
                                }
                                count[[i, j, k]] += 1;
                            }
                        }
                    }
                }
            }
        }

        // Any pixel with no valid observations still holds INFINITY; map it back
        // to the configured nodata (or NaN) so callers never see a raw +inf
        // sentinel.
        replace_empty_pixels(&mut composite, &count, config);

        info!("Created minimum value composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Maximum NDVI composite
    fn max_ndvi_composite(
        ts: &TimeSeriesRaster,
        config: &CompositingConfig,
    ) -> Result<CompositeResult> {
        let red_band = config.red_band.ok_or_else(|| {
            TemporalError::invalid_parameter("red_band", "required for MaxNDVI composite")
        })?;

        let nir_band = config.nir_band.ok_or_else(|| {
            TemporalError::invalid_parameter("nir_band", "required for MaxNDVI composite")
        })?;

        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        if red_band >= n_bands || nir_band >= n_bands {
            return Err(TemporalError::invalid_parameter(
                "band_indices",
                "band indices out of range",
            ));
        }

        let mut composite = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));
        let mut max_ndvi = Array3::from_elem((height, width, 1), f64::NEG_INFINITY);

        for entry in ts.entries().values() {
            if let Some(data) = &entry.data {
                for i in 0..height {
                    for j in 0..width {
                        let red = data[[i, j, red_band]];
                        let nir = data[[i, j, nir_band]];

                        if !red.is_nan() && !nir.is_nan() && (red + nir) != 0.0 {
                            let ndvi = (nir - red) / (nir + red);

                            if ndvi > max_ndvi[[i, j, 0]] {
                                max_ndvi[[i, j, 0]] = ndvi;
                                // Copy all bands from this observation
                                for k in 0..n_bands {
                                    composite[[i, j, k]] = data[[i, j, k]];
                                }
                                count[[i, j, 0]] += 1;
                            }
                        }
                    }
                }
            }
        }

        info!("Created maximum NDVI composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Quality-weighted composite
    fn quality_weighted_composite(
        ts: &TimeSeriesRaster,
        config: &CompositingConfig,
    ) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite: Array3<f64> = Array3::zeros((height, width, n_bands));
        let mut count: Array3<usize> = Array3::zeros((height, width, n_bands));
        let mut weight_sum: Array3<f64> = Array3::zeros((height, width, n_bands));

        for entry in ts.entries().values() {
            if !entry_passes_cloud_filter(entry, config) {
                continue;
            }

            let weight = entry.metadata.quality_score.unwrap_or(1.0) as f64;

            if let Some(data) = &entry.data {
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            let value = data[[i, j, k]];
                            if value_is_valid(value, config) {
                                composite[[i, j, k]] += value * weight;
                                weight_sum[[i, j, k]] += weight;
                                count[[i, j, k]] += 1;
                            }
                        }
                    }
                }
            }
        }

        // Normalize by weights
        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    if weight_sum[[i, j, k]] > 0.0 {
                        composite[[i, j, k]] /= weight_sum[[i, j, k]];
                    }
                }
            }
        }

        // Pixels with no valid observations still hold the 0.0 accumulator seed,
        // which is indistinguishable from a genuine zero measurement; map them
        // to the configured nodata (or NaN) instead.
        replace_empty_pixels(&mut composite, &count, config);

        info!("Created quality-weighted composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// First valid value composite
    fn first_valid_composite(
        ts: &TimeSeriesRaster,
        _config: &CompositingConfig,
    ) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));
        let mut filled = Array3::from_elem((height, width, n_bands), false);

        for entry in ts.entries().values() {
            if let Some(data) = &entry.data {
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            if !filled[[i, j, k]] {
                                let value = data[[i, j, k]];
                                if !value.is_nan() {
                                    composite[[i, j, k]] = value;
                                    count[[i, j, k]] = 1;
                                    filled[[i, j, k]] = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        info!("Created first valid value composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Last valid value composite
    fn last_valid_composite(
        ts: &TimeSeriesRaster,
        _config: &CompositingConfig,
    ) -> Result<CompositeResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut composite = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));

        for entry in ts.entries().values() {
            if let Some(data) = &entry.data {
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            let value = data[[i, j, k]];
                            if !value.is_nan() {
                                composite[[i, j, k]] = value;
                                count[[i, j, k]] = 1;
                            }
                        }
                    }
                }
            }
        }

        info!("Created last valid value composite");
        Ok(CompositeResult::new(composite, count))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::timeseries::TemporalMetadata;
    use chrono::{DateTime, NaiveDate, Utc};
    use scirs2_core::ndarray::Array3;

    fn ts_at(day: u32) -> DateTime<Utc> {
        let date = NaiveDate::from_ymd_opt(2024, 1, day).expect("valid date");
        let ndt = date.and_hms_opt(0, 0, 0).expect("valid time");
        DateTime::from_naive_utc_and_offset(ndt, Utc)
    }

    fn meta(day: u32, cloud: Option<f32>, quality: Option<f32>) -> TemporalMetadata {
        let date = NaiveDate::from_ymd_opt(2024, 1, day).expect("valid date");
        let mut m = TemporalMetadata::new(ts_at(day), date);
        if let Some(c) = cloud {
            m = m.with_cloud_cover(c);
        }
        if let Some(q) = quality {
            m = m.with_quality_score(q);
        }
        m
    }

    /// 1x1x1 raster holding a single value.
    fn scalar_raster(v: f64) -> Array3<f64> {
        Array3::from_elem((1, 1, 1), v)
    }

    #[test]
    fn test_max_composite_respects_cloud_filter() {
        let mut ts = TimeSeriesRaster::new();
        // Cloudy scene has the highest value; it must be filtered out.
        ts.add_raster(meta(1, Some(90.0), None), scalar_raster(100.0))
            .unwrap();
        ts.add_raster(meta(2, Some(5.0), None), scalar_raster(42.0))
            .unwrap();

        let config = CompositingConfig {
            method: CompositingMethod::Maximum,
            max_cloud_cover: Some(20.0),
            nodata: None,
            ..CompositingConfig::default()
        };

        let result = TemporalCompositor::composite(&ts, &config).unwrap();
        // Only the clear scene (42.0) should have been considered.
        assert_eq!(result.data[[0, 0, 0]], 42.0);
        assert_eq!(result.count[[0, 0, 0]], 1);
    }

    #[test]
    fn test_min_composite_respects_cloud_filter() {
        let mut ts = TimeSeriesRaster::new();
        ts.add_raster(meta(1, Some(90.0), None), scalar_raster(1.0))
            .unwrap();
        ts.add_raster(meta(2, Some(5.0), None), scalar_raster(42.0))
            .unwrap();

        let config = CompositingConfig {
            method: CompositingMethod::Minimum,
            max_cloud_cover: Some(20.0),
            nodata: None,
            ..CompositingConfig::default()
        };

        let result = TemporalCompositor::composite(&ts, &config).unwrap();
        assert_eq!(result.data[[0, 0, 0]], 42.0);
        assert_eq!(result.count[[0, 0, 0]], 1);
    }

    #[test]
    fn test_max_composite_all_invalid_yields_nodata_not_infinity() {
        let mut ts = TimeSeriesRaster::new();
        ts.add_raster(meta(1, None, None), scalar_raster(f64::NAN))
            .unwrap();
        ts.add_raster(meta(2, None, None), scalar_raster(f64::NAN))
            .unwrap();

        let config = CompositingConfig {
            method: CompositingMethod::Maximum,
            max_cloud_cover: None,
            nodata: Some(-9999.0),
            ..CompositingConfig::default()
        };

        let result = TemporalCompositor::composite(&ts, &config).unwrap();
        let v = result.data[[0, 0, 0]];
        assert!(
            v.is_finite() && (v - (-9999.0)).abs() < 1e-9,
            "all-invalid pixel must become nodata, got {v}"
        );
        assert_eq!(result.count[[0, 0, 0]], 0);
    }

    #[test]
    fn test_min_composite_all_invalid_yields_nan_when_no_nodata() {
        let mut ts = TimeSeriesRaster::new();
        ts.add_raster(meta(1, None, None), scalar_raster(f64::NAN))
            .unwrap();

        let config = CompositingConfig {
            method: CompositingMethod::Minimum,
            max_cloud_cover: None,
            nodata: None,
            ..CompositingConfig::default()
        };

        let result = TemporalCompositor::composite(&ts, &config).unwrap();
        let v = result.data[[0, 0, 0]];
        assert!(
            v.is_nan(),
            "all-invalid pixel with no nodata must become NaN, not +inf, got {v}"
        );
    }

    #[test]
    fn test_max_composite_honors_nodata_sentinel() {
        let mut ts = TimeSeriesRaster::new();
        // The nodata sentinel (999.0) is the numerically largest value but must
        // be ignored so the real maximum (10.0) wins.
        ts.add_raster(meta(1, None, None), scalar_raster(999.0))
            .unwrap();
        ts.add_raster(meta(2, None, None), scalar_raster(10.0))
            .unwrap();

        let config = CompositingConfig {
            method: CompositingMethod::Maximum,
            max_cloud_cover: None,
            nodata: Some(999.0),
            ..CompositingConfig::default()
        };

        let result = TemporalCompositor::composite(&ts, &config).unwrap();
        assert_eq!(result.data[[0, 0, 0]], 10.0);
        assert_eq!(result.count[[0, 0, 0]], 1);
    }

    #[test]
    fn test_quality_weighted_respects_cloud_filter_and_nodata() {
        let mut ts = TimeSeriesRaster::new();
        ts.add_raster(meta(1, Some(95.0), Some(1.0)), scalar_raster(100.0))
            .unwrap();
        ts.add_raster(meta(2, Some(2.0), Some(1.0)), scalar_raster(20.0))
            .unwrap();

        let config = CompositingConfig {
            method: CompositingMethod::QualityWeighted,
            max_cloud_cover: Some(50.0),
            nodata: None,
            ..CompositingConfig::default()
        };

        let result = TemporalCompositor::composite(&ts, &config).unwrap();
        // Cloudy scene filtered; only 20.0 with weight 1.0 remains.
        assert!((result.data[[0, 0, 0]] - 20.0).abs() < 1e-9);
        assert_eq!(result.count[[0, 0, 0]], 1);
    }
}
