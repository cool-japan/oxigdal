//! Raster Stack Operations Module
//!
//! This module provides operations for stacking multiple rasters together,
//! including multi-band stacking, temporal stacking, and stack transformations.

use crate::error::{Result, TemporalError};
#[cfg(feature = "timeseries")]
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::{Array3, Array4, Axis};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Stack configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackConfig {
    /// Stack along which axis (0=temporal, 1=bands, 2=height, 3=width)
    pub axis: usize,
    /// Interpolation method for mismatched dimensions
    pub interpolation: InterpolationMethod,
    /// Fill value for missing data
    pub fill_value: Option<f64>,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            axis: 0,
            interpolation: InterpolationMethod::Nearest,
            fill_value: Some(f64::NAN),
        }
    }
}

/// Interpolation method for resampling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterpolationMethod {
    /// Nearest neighbor
    Nearest,
    /// Bilinear interpolation
    Bilinear,
    /// Cubic interpolation
    Cubic,
}

/// Multi-dimensional raster stack
///
/// Represents a stack of rasters organized in a 4D array:
/// (time, height, width, bands)
#[derive(Debug, Clone)]
pub struct RasterStack {
    /// 4D data array: (time, height, width, bands)
    data: Array4<f64>,
    /// Stack metadata
    metadata: StackMetadata,
}

/// Stack metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackMetadata {
    /// Number of time steps
    pub n_time: usize,
    /// Height (rows)
    pub height: usize,
    /// Width (columns)
    pub width: usize,
    /// Number of bands
    pub n_bands: usize,
    /// Band names
    pub band_names: Vec<String>,
    /// NoData value
    pub nodata: Option<f64>,
}

impl RasterStack {
    /// Create new raster stack from 4D array
    ///
    /// # Errors
    /// Returns error if array dimensions are invalid
    pub fn new(data: Array4<f64>) -> Result<Self> {
        let shape = data.shape();
        if shape.len() != 4 {
            return Err(TemporalError::dimension_mismatch(
                "4D array",
                format!("{}D array", shape.len()),
            ));
        }

        let metadata = StackMetadata {
            n_time: shape[0],
            height: shape[1],
            width: shape[2],
            n_bands: shape[3],
            band_names: (0..shape[3]).map(|i| format!("Band_{}", i + 1)).collect(),
            nodata: None,
        };

        Ok(Self { data, metadata })
    }

    /// Create raster stack from time series
    ///
    /// # Errors
    /// Returns error if time series is empty or data not loaded
    #[cfg(feature = "timeseries")]
    pub fn from_timeseries(ts: &TimeSeriesRaster) -> Result<Self> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Time series is empty"));
        }

        // Get shape from first entry
        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let n_time = ts.len();

        // Initialize 4D array
        let mut data = Array4::zeros((n_time, height, width, n_bands));

        // Fill data from time series
        for (t, (_, entry)) in ts.iter().enumerate() {
            let entry_data = entry.data.as_ref().ok_or_else(|| {
                TemporalError::invalid_input("Data not loaded in time series entry")
            })?;

            // Copy data to stack
            for i in 0..height {
                for j in 0..width {
                    for k in 0..n_bands {
                        data[[t, i, j, k]] = entry_data[[i, j, k]];
                    }
                }
            }
        }

        let metadata = StackMetadata {
            n_time,
            height,
            width,
            n_bands,
            band_names: (0..n_bands).map(|i| format!("Band_{}", i + 1)).collect(),
            nodata: None,
        };

        info!(
            "Created raster stack with shape ({}, {}, {}, {})",
            n_time, height, width, n_bands
        );

        Ok(Self { data, metadata })
    }

    /// Get stack shape (time, height, width, bands)
    #[must_use]
    pub fn shape(&self) -> (usize, usize, usize, usize) {
        (
            self.metadata.n_time,
            self.metadata.height,
            self.metadata.width,
            self.metadata.n_bands,
        )
    }

    /// Get reference to underlying data
    #[must_use]
    pub fn data(&self) -> &Array4<f64> {
        &self.data
    }

    /// Get mutable reference to underlying data
    pub fn data_mut(&mut self) -> &mut Array4<f64> {
        &mut self.data
    }

    /// Get metadata
    #[must_use]
    pub fn metadata(&self) -> &StackMetadata {
        &self.metadata
    }

    /// Set band names
    pub fn set_band_names(&mut self, names: Vec<String>) -> Result<()> {
        if names.len() != self.metadata.n_bands {
            return Err(TemporalError::dimension_mismatch(
                format!("{} bands", self.metadata.n_bands),
                format!("{} names", names.len()),
            ));
        }
        self.metadata.band_names = names;
        Ok(())
    }

    /// Set nodata value
    pub fn set_nodata(&mut self, nodata: f64) {
        self.metadata.nodata = Some(nodata);
    }

    /// Extract temporal slice at specific time index
    ///
    /// # Errors
    /// Returns error if time index is out of bounds
    pub fn get_time_slice(&self, time_index: usize) -> Result<Array3<f64>> {
        if time_index >= self.metadata.n_time {
            return Err(TemporalError::time_index_out_of_bounds(
                time_index,
                0,
                self.metadata.n_time,
            ));
        }

        Ok(self.data.index_axis(Axis(0), time_index).to_owned())
    }

    /// Extract spatial slice for specific band across all time
    ///
    /// # Errors
    /// Returns error if band index is out of bounds
    pub fn get_band_timeseries(&self, band_index: usize) -> Result<Array3<f64>> {
        if band_index >= self.metadata.n_bands {
            return Err(TemporalError::invalid_parameter(
                "band_index",
                format!(
                    "index {} out of bounds (max: {})",
                    band_index,
                    self.metadata.n_bands - 1
                ),
            ));
        }

        // Extract (time, height, width) for specific band
        let mut result = Array3::zeros((
            self.metadata.n_time,
            self.metadata.height,
            self.metadata.width,
        ));

        for t in 0..self.metadata.n_time {
            for i in 0..self.metadata.height {
                for j in 0..self.metadata.width {
                    result[[t, i, j]] = self.data[[t, i, j, band_index]];
                }
            }
        }

        Ok(result)
    }

    /// Extract pixel time series at specific location for specific band
    ///
    /// # Errors
    /// Returns error if coordinates are out of bounds
    pub fn get_pixel_timeseries(&self, row: usize, col: usize, band: usize) -> Result<Vec<f64>> {
        if row >= self.metadata.height {
            return Err(TemporalError::invalid_parameter(
                "row",
                format!(
                    "index {} out of bounds (max: {})",
                    row,
                    self.metadata.height - 1
                ),
            ));
        }
        if col >= self.metadata.width {
            return Err(TemporalError::invalid_parameter(
                "col",
                format!(
                    "index {} out of bounds (max: {})",
                    col,
                    self.metadata.width - 1
                ),
            ));
        }
        if band >= self.metadata.n_bands {
            return Err(TemporalError::invalid_parameter(
                "band",
                format!(
                    "index {} out of bounds (max: {})",
                    band,
                    self.metadata.n_bands - 1
                ),
            ));
        }

        let mut values = Vec::with_capacity(self.metadata.n_time);
        for t in 0..self.metadata.n_time {
            values.push(self.data[[t, row, col, band]]);
        }

        Ok(values)
    }

    /// Stack multiple bands together
    ///
    /// # Errors
    /// Returns error if shapes don't match
    pub fn stack_bands(bands: Vec<Array3<f64>>) -> Result<Self> {
        if bands.is_empty() {
            return Err(TemporalError::insufficient_data("No bands to stack"));
        }

        // Check all bands have same shape
        let first_shape = bands[0].shape();
        for (i, band) in bands.iter().enumerate().skip(1) {
            if band.shape() != first_shape {
                return Err(TemporalError::dimension_mismatch(
                    format!("{:?}", first_shape),
                    format!("{:?} (band {})", band.shape(), i),
                ));
            }
        }

        let n_time = first_shape[0];
        let height = first_shape[1];
        let width = first_shape[2];
        let n_bands = bands.len();

        // Create 4D array
        let mut data = Array4::zeros((n_time, height, width, n_bands));

        for (band_idx, band_data) in bands.iter().enumerate() {
            for t in 0..n_time {
                for i in 0..height {
                    for j in 0..width {
                        data[[t, i, j, band_idx]] = band_data[[t, i, j]];
                    }
                }
            }
        }

        let metadata = StackMetadata {
            n_time,
            height,
            width,
            n_bands,
            band_names: (0..n_bands).map(|i| format!("Band_{}", i + 1)).collect(),
            nodata: None,
        };

        debug!(
            "Stacked {} bands into shape ({}, {}, {}, {})",
            n_bands, n_time, height, width, n_bands
        );

        Ok(Self { data, metadata })
    }

    /// Concatenate stacks along time axis
    ///
    /// # Errors
    /// Returns error if spatial dimensions don't match
    pub fn concatenate_time(stacks: Vec<Self>) -> Result<Self> {
        if stacks.is_empty() {
            return Err(TemporalError::insufficient_data("No stacks to concatenate"));
        }

        // Check all stacks have same spatial dimensions and bands
        let first = &stacks[0];
        let (_, height, width, n_bands) = first.shape();

        for (i, stack) in stacks.iter().enumerate().skip(1) {
            let (_, h, w, b) = stack.shape();
            if h != height || w != width || b != n_bands {
                return Err(TemporalError::dimension_mismatch(
                    format!("(?, {}, {}, {})", height, width, n_bands),
                    format!("(?, {}, {}, {}) at stack {}", h, w, b, i),
                ));
            }
        }

        // Calculate total time steps
        let total_time: usize = stacks.iter().map(|s| s.metadata.n_time).sum();

        // Create concatenated array
        let mut data = Array4::zeros((total_time, height, width, n_bands));
        let mut current_time = 0;

        for stack in &stacks {
            let stack_time = stack.metadata.n_time;
            for t in 0..stack_time {
                for i in 0..height {
                    for j in 0..width {
                        for k in 0..n_bands {
                            data[[current_time + t, i, j, k]] = stack.data[[t, i, j, k]];
                        }
                    }
                }
            }
            current_time += stack_time;
        }

        let metadata = StackMetadata {
            n_time: total_time,
            height,
            width,
            n_bands,
            band_names: first.metadata.band_names.clone(),
            nodata: first.metadata.nodata,
        };

        info!(
            "Concatenated {} stacks into shape ({}, {}, {}, {})",
            stacks.len(),
            total_time,
            height,
            width,
            n_bands
        );

        Ok(Self { data, metadata })
    }

    /// Subset stack by time range
    ///
    /// # Errors
    /// Returns error if indices are out of bounds
    pub fn subset_time(&self, start: usize, end: usize) -> Result<Self> {
        if start >= end {
            return Err(TemporalError::invalid_time_range(
                start.to_string(),
                end.to_string(),
            ));
        }
        if end > self.metadata.n_time {
            return Err(TemporalError::time_index_out_of_bounds(
                end,
                0,
                self.metadata.n_time,
            ));
        }

        let n_time = end - start;
        let mut data = Array4::zeros((
            n_time,
            self.metadata.height,
            self.metadata.width,
            self.metadata.n_bands,
        ));

        for (t_out, t_in) in (start..end).enumerate() {
            for i in 0..self.metadata.height {
                for j in 0..self.metadata.width {
                    for k in 0..self.metadata.n_bands {
                        data[[t_out, i, j, k]] = self.data[[t_in, i, j, k]];
                    }
                }
            }
        }

        let metadata = StackMetadata {
            n_time,
            height: self.metadata.height,
            width: self.metadata.width,
            n_bands: self.metadata.n_bands,
            band_names: self.metadata.band_names.clone(),
            nodata: self.metadata.nodata,
        };

        Ok(Self { data, metadata })
    }

    /// Subset stack by band indices
    ///
    /// # Errors
    /// Returns error if any band index is out of bounds
    pub fn subset_bands(&self, band_indices: &[usize]) -> Result<Self> {
        if band_indices.is_empty() {
            return Err(TemporalError::insufficient_data("No bands selected"));
        }

        // Validate all indices
        for &idx in band_indices {
            if idx >= self.metadata.n_bands {
                return Err(TemporalError::invalid_parameter(
                    "band_index",
                    format!(
                        "index {} out of bounds (max: {})",
                        idx,
                        self.metadata.n_bands - 1
                    ),
                ));
            }
        }

        let n_bands = band_indices.len();
        let mut data = Array4::zeros((
            self.metadata.n_time,
            self.metadata.height,
            self.metadata.width,
            n_bands,
        ));

        for t in 0..self.metadata.n_time {
            for i in 0..self.metadata.height {
                for j in 0..self.metadata.width {
                    for (k_out, &k_in) in band_indices.iter().enumerate() {
                        data[[t, i, j, k_out]] = self.data[[t, i, j, k_in]];
                    }
                }
            }
        }

        let band_names = band_indices
            .iter()
            .map(|&i| self.metadata.band_names[i].clone())
            .collect();

        let metadata = StackMetadata {
            n_time: self.metadata.n_time,
            height: self.metadata.height,
            width: self.metadata.width,
            n_bands,
            band_names,
            nodata: self.metadata.nodata,
        };

        Ok(Self { data, metadata })
    }

    /// Apply function to each pixel time series
    ///
    /// # Errors
    /// Returns error if function fails
    pub fn apply_temporal<F>(&self, func: F) -> Result<Array3<f64>>
    where
        F: Fn(&[f64]) -> f64,
    {
        let mut result = Array3::zeros((
            self.metadata.height,
            self.metadata.width,
            self.metadata.n_bands,
        ));

        for i in 0..self.metadata.height {
            for j in 0..self.metadata.width {
                for k in 0..self.metadata.n_bands {
                    let timeseries: Vec<f64> = (0..self.metadata.n_time)
                        .map(|t| self.data[[t, i, j, k]])
                        .collect();
                    result[[i, j, k]] = func(&timeseries);
                }
            }
        }

        Ok(result)
    }

    /// Calculate mean across time dimension
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn mean_temporal(&self) -> Result<Array3<f64>> {
        self.apply_temporal(|values| values.iter().sum::<f64>() / values.len() as f64)
    }

    /// Calculate median across time dimension
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn median_temporal(&self) -> Result<Array3<f64>> {
        self.apply_temporal(|values| {
            let mut sorted = values.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = sorted.len() / 2;
            if sorted.len() % 2 == 0 {
                (sorted[mid - 1] + sorted[mid]) / 2.0
            } else {
                sorted[mid]
            }
        })
    }

    /// Calculate standard deviation across time dimension
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn std_temporal(&self) -> Result<Array3<f64>> {
        self.apply_temporal(|values| {
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            let variance =
                values.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
            variance.sqrt()
        })
    }

    /// Calculate minimum across time dimension
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn min_temporal(&self) -> Result<Array3<f64>> {
        self.apply_temporal(|values| {
            values
                .iter()
                .copied()
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(f64::NAN)
        })
    }

    /// Calculate maximum across time dimension
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn max_temporal(&self) -> Result<Array3<f64>> {
        self.apply_temporal(|values| {
            values
                .iter()
                .copied()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(f64::NAN)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_raster_stack_creation() {
        let data = Array4::zeros((10, 100, 100, 3));
        let stack = RasterStack::new(data).expect("should create stack");
        assert_eq!(stack.shape(), (10, 100, 100, 3));
    }

    #[test]
    fn test_get_time_slice() {
        let mut data = Array4::zeros((10, 5, 5, 2));
        data[[3, 2, 2, 0]] = 42.0;

        let stack = RasterStack::new(data).expect("should create stack");
        let slice = stack.get_time_slice(3).expect("should get slice");

        assert_eq!(slice.shape(), &[5, 5, 2]);
        assert_abs_diff_eq!(slice[[2, 2, 0]], 42.0);
    }

    #[test]
    fn test_get_pixel_timeseries() {
        let mut data = Array4::zeros((10, 5, 5, 2));
        for t in 0..10 {
            data[[t, 2, 3, 1]] = t as f64;
        }

        let stack = RasterStack::new(data).expect("should create stack");
        let ts = stack
            .get_pixel_timeseries(2, 3, 1)
            .expect("should get timeseries");

        assert_eq!(ts.len(), 10);
        for (i, &val) in ts.iter().enumerate() {
            assert_abs_diff_eq!(val, i as f64);
        }
    }

    #[test]
    fn test_stack_bands() {
        let band1 = Array3::from_elem((5, 10, 10), 1.0);
        let band2 = Array3::from_elem((5, 10, 10), 2.0);
        let band3 = Array3::from_elem((5, 10, 10), 3.0);

        let stack = RasterStack::stack_bands(vec![band1, band2, band3]).expect("should stack");

        assert_eq!(stack.shape(), (5, 10, 10, 3));
        assert_abs_diff_eq!(stack.data()[[0, 0, 0, 0]], 1.0);
        assert_abs_diff_eq!(stack.data()[[0, 0, 0, 1]], 2.0);
        assert_abs_diff_eq!(stack.data()[[0, 0, 0, 2]], 3.0);
    }

    #[test]
    fn test_concatenate_time() {
        let data1 = Array4::from_elem((5, 10, 10, 2), 1.0);
        let stack1 = RasterStack::new(data1).expect("should create");

        let data2 = Array4::from_elem((3, 10, 10, 2), 2.0);
        let stack2 = RasterStack::new(data2).expect("should create");

        let concatenated =
            RasterStack::concatenate_time(vec![stack1, stack2]).expect("should concatenate");

        assert_eq!(concatenated.shape(), (8, 10, 10, 2));
    }

    #[test]
    fn test_subset_time() {
        let data = Array4::zeros((10, 5, 5, 2));
        let stack = RasterStack::new(data).expect("should create");

        let subset = stack.subset_time(2, 7).expect("should subset");
        assert_eq!(subset.shape(), (5, 5, 5, 2));
    }

    #[test]
    fn test_subset_bands() {
        let data = Array4::zeros((10, 5, 5, 5));
        let stack = RasterStack::new(data).expect("should create");

        let subset = stack.subset_bands(&[0, 2, 4]).expect("should subset");
        assert_eq!(subset.shape(), (10, 5, 5, 3));
    }

    #[test]
    fn test_mean_temporal() {
        let mut data = Array4::zeros((3, 2, 2, 1));
        data[[0, 0, 0, 0]] = 1.0;
        data[[1, 0, 0, 0]] = 2.0;
        data[[2, 0, 0, 0]] = 3.0;

        let stack = RasterStack::new(data).expect("should create");
        let mean = stack.mean_temporal().expect("should calculate mean");

        assert_abs_diff_eq!(mean[[0, 0, 0]], 2.0);
    }

    #[test]
    fn test_median_temporal() {
        let mut data = Array4::zeros((5, 2, 2, 1));
        data[[0, 0, 0, 0]] = 1.0;
        data[[1, 0, 0, 0]] = 2.0;
        data[[2, 0, 0, 0]] = 3.0;
        data[[3, 0, 0, 0]] = 4.0;
        data[[4, 0, 0, 0]] = 5.0;

        let stack = RasterStack::new(data).expect("should create");
        let median = stack.median_temporal().expect("should calculate median");

        assert_abs_diff_eq!(median[[0, 0, 0]], 3.0);
    }

    #[test]
    fn test_min_max_temporal() {
        let mut data = Array4::zeros((5, 2, 2, 1));
        data[[0, 0, 0, 0]] = 1.0;
        data[[1, 0, 0, 0]] = 5.0;
        data[[2, 0, 0, 0]] = 3.0;
        data[[3, 0, 0, 0]] = 2.0;
        data[[4, 0, 0, 0]] = 4.0;

        let stack = RasterStack::new(data).expect("should create");
        let min = stack.min_temporal().expect("should calculate min");
        let max = stack.max_temporal().expect("should calculate max");

        assert_abs_diff_eq!(min[[0, 0, 0]], 1.0);
        assert_abs_diff_eq!(max[[0, 0, 0]], 5.0);
    }
}
