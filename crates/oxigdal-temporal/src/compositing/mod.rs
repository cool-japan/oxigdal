//! Temporal Compositing Module
//!
//! Implements various temporal compositing methods for creating representative
//! rasters from time series including median, mean, max NDVI, and quality-weighted composites.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
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
                        if let Some(max_cc) = config.max_cloud_cover {
                            if let Some(cc) = entry.metadata.cloud_cover {
                                if cc > max_cc {
                                    continue;
                                }
                            }
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
                        if let Some(max_cc) = config.max_cloud_cover {
                            if let Some(cc) = entry.metadata.cloud_cover {
                                if cc > max_cc {
                                    continue;
                                }
                            }
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
    fn max_composite(
        ts: &TimeSeriesRaster,
        _config: &CompositingConfig,
    ) -> Result<CompositeResult> {
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
                        if let Some(data) = &entry.data {
                            let value = data[[i, j, k]];
                            if !value.is_nan() {
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

        info!("Created maximum value composite");
        Ok(CompositeResult::new(composite, count))
    }

    /// Minimum value composite
    fn min_composite(
        ts: &TimeSeriesRaster,
        _config: &CompositingConfig,
    ) -> Result<CompositeResult> {
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
                        if let Some(data) = &entry.data {
                            let value = data[[i, j, k]];
                            if !value.is_nan() {
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
        _config: &CompositingConfig,
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
            let weight = entry.metadata.quality_score.unwrap_or(1.0) as f64;

            if let Some(data) = &entry.data {
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            let value = data[[i, j, k]];
                            if !value.is_nan() {
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
