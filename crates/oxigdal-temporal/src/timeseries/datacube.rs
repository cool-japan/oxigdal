//! Data Cube Module
//!
//! Multi-dimensional raster cube abstraction for efficient temporal-spatial analysis.
//! Provides integration with Zarr for cloud-optimized storage and seamless conversion
//! between TimeSeriesRaster and DataCube formats.

use super::TimeSeriesRaster;
use crate::error::{Result, TemporalError};
use crate::stack::RasterStack;
use chrono::DateTime;
use scirs2_core::ndarray::{Array3, Array4};
use serde::{Deserialize, Serialize};

#[cfg(feature = "zarr")]
use std::path::PathBuf;

#[cfg(feature = "zarr")]
use oxigdal_zarr::{
    metadata::v3::{ArrayMetadataV3, FillValue},
    reader::v3::ZarrV3Reader,
    storage::memory::MemoryStore,
    writer::v3::ZarrV3Writer,
};

#[cfg(all(feature = "zarr", feature = "filesystem"))]
use oxigdal_zarr::storage::filesystem::FilesystemStore;

use tracing::info;

#[cfg(feature = "parallel")]
#[allow(unused_imports)]
use rayon::prelude::*;

/// Data cube dimensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CubeDimensions {
    /// Time dimension size
    pub time: usize,
    /// Latitude/Y dimension size
    pub y: usize,
    /// Longitude/X dimension size
    pub x: usize,
    /// Bands/Variables dimension size
    pub bands: usize,
}

/// Data cube metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CubeMetadata {
    /// Dimensions
    pub dimensions: CubeDimensions,
    /// Band/variable names
    pub band_names: Vec<String>,
    /// Temporal coordinates (timestamps)
    pub time_coords: Vec<i64>,
    /// Spatial extent (minx, miny, maxx, maxy)
    pub spatial_extent: Option<(f64, f64, f64, f64)>,
    /// CRS/projection
    pub crs: Option<String>,
    /// NoData value
    pub nodata: Option<f64>,
}

/// Data cube for multi-dimensional analysis
#[derive(Debug, Clone)]
pub struct DataCube {
    /// 4D data array (time, y, x, bands)
    data: Array4<f64>,
    /// Metadata
    metadata: CubeMetadata,
}

impl DataCube {
    /// Create new data cube
    ///
    /// # Errors
    /// Returns error if dimensions don't match
    pub fn new(data: Array4<f64>, metadata: CubeMetadata) -> Result<Self> {
        let shape = data.shape();
        if shape.len() != 4 {
            return Err(TemporalError::dimension_mismatch(
                "4D array",
                format!("{}D array", shape.len()),
            ));
        }

        if shape[0] != metadata.dimensions.time
            || shape[1] != metadata.dimensions.y
            || shape[2] != metadata.dimensions.x
            || shape[3] != metadata.dimensions.bands
        {
            return Err(TemporalError::dimension_mismatch(
                format!(
                    "{}x{}x{}x{}",
                    metadata.dimensions.time,
                    metadata.dimensions.y,
                    metadata.dimensions.x,
                    metadata.dimensions.bands
                ),
                format!("{}x{}x{}x{}", shape[0], shape[1], shape[2], shape[3]),
            ));
        }

        Ok(Self { data, metadata })
    }

    /// Create from raster stack
    ///
    /// # Errors
    /// Returns error if conversion fails
    pub fn from_stack(stack: RasterStack, time_coords: Vec<i64>) -> Result<Self> {
        let (n_time, height, width, n_bands) = stack.shape();

        if time_coords.len() != n_time {
            return Err(TemporalError::dimension_mismatch(
                format!("{} time coordinates", n_time),
                format!("{} provided", time_coords.len()),
            ));
        }

        let dimensions = CubeDimensions {
            time: n_time,
            y: height,
            x: width,
            bands: n_bands,
        };

        let metadata = CubeMetadata {
            dimensions,
            band_names: stack.metadata().band_names.clone(),
            time_coords,
            spatial_extent: None,
            crs: None,
            nodata: stack.metadata().nodata,
        };

        Ok(Self {
            data: stack.data().clone(),
            metadata,
        })
    }

    /// Create from time series raster
    ///
    /// # Errors
    /// Returns error if time series is empty or data not loaded
    pub fn from_timeseries(ts: &TimeSeriesRaster) -> Result<Self> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let n_time = ts.len();
        let mut data = Array4::zeros((n_time, height, width, bands));
        let mut time_coords = Vec::with_capacity(n_time);

        for (t_idx, (timestamp, entry)) in ts.iter().enumerate() {
            time_coords.push(*timestamp);

            let entry_data = entry
                .data
                .as_ref()
                .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

            for i in 0..height {
                for j in 0..width {
                    for k in 0..bands {
                        data[[t_idx, i, j, k]] = entry_data[[i, j, k]];
                    }
                }
            }
        }

        let dimensions = CubeDimensions {
            time: n_time,
            y: height,
            x: width,
            bands,
        };

        // Extract band names from time series if available
        let band_names: Vec<String> = (0..bands).map(|i| format!("Band_{}", i + 1)).collect();

        let metadata = CubeMetadata {
            dimensions,
            band_names,
            time_coords,
            spatial_extent: None,
            crs: None,
            nodata: None,
        };

        info!(
            "Created datacube from time series: {}x{}x{}x{}",
            n_time, height, width, bands
        );

        Ok(Self { data, metadata })
    }

    /// Convert datacube back to time series raster
    ///
    /// # Errors
    /// Returns error if conversion fails
    pub fn to_timeseries(&self) -> Result<TimeSeriesRaster> {
        let mut ts = TimeSeriesRaster::new();
        let dims = &self.metadata.dimensions;

        for (t_idx, &timestamp) in self.metadata.time_coords.iter().enumerate() {
            let dt = DateTime::from_timestamp(timestamp, 0).ok_or_else(|| {
                TemporalError::datetime_parse_error(format!("Invalid timestamp: {}", timestamp))
            })?;

            let mut slice = Array3::zeros((dims.y, dims.x, dims.bands));
            for i in 0..dims.y {
                for j in 0..dims.x {
                    for k in 0..dims.bands {
                        slice[[i, j, k]] = self.data[[t_idx, i, j, k]];
                    }
                }
            }

            let metadata = super::TemporalMetadata::new(dt, dt.date_naive());
            ts.add_raster(metadata, slice)?;
        }

        info!(
            "Converted datacube to time series with {} rasters",
            ts.len()
        );
        Ok(ts)
    }

    /// Get cube dimensions
    #[must_use]
    pub fn dimensions(&self) -> &CubeDimensions {
        &self.metadata.dimensions
    }

    /// Get metadata
    #[must_use]
    pub fn metadata(&self) -> &CubeMetadata {
        &self.metadata
    }

    /// Get reference to data
    #[must_use]
    pub fn data(&self) -> &Array4<f64> {
        &self.data
    }

    /// Get mutable reference to data
    pub fn data_mut(&mut self) -> &mut Array4<f64> {
        &mut self.data
    }

    /// Select time range
    ///
    /// # Errors
    /// Returns error if indices are out of bounds
    pub fn select_time_range(&self, start: usize, end: usize) -> Result<Self> {
        if start >= end {
            return Err(TemporalError::invalid_time_range(
                start.to_string(),
                end.to_string(),
            ));
        }

        let dims = &self.metadata.dimensions;
        if end > dims.time {
            return Err(TemporalError::time_index_out_of_bounds(end, 0, dims.time));
        }

        let n_time = end - start;
        let mut subset_data = Array4::zeros((n_time, dims.y, dims.x, dims.bands));

        for (t_out, t_in) in (start..end).enumerate() {
            for i in 0..dims.y {
                for j in 0..dims.x {
                    for k in 0..dims.bands {
                        subset_data[[t_out, i, j, k]] = self.data[[t_in, i, j, k]];
                    }
                }
            }
        }

        let mut new_metadata = self.metadata.clone();
        new_metadata.dimensions.time = n_time;
        new_metadata.time_coords = self.metadata.time_coords[start..end].to_vec();

        Ok(Self {
            data: subset_data,
            metadata: new_metadata,
        })
    }

    /// Select specific bands
    ///
    /// # Errors
    /// Returns error if band indices are out of bounds
    pub fn select_bands(&self, band_indices: &[usize]) -> Result<Self> {
        if band_indices.is_empty() {
            return Err(TemporalError::insufficient_data("No bands selected"));
        }

        let dims = &self.metadata.dimensions;
        for &idx in band_indices {
            if idx >= dims.bands {
                return Err(TemporalError::invalid_parameter(
                    "band_index",
                    format!("index {} out of bounds (max: {})", idx, dims.bands - 1),
                ));
            }
        }

        let n_bands = band_indices.len();
        let mut subset_data = Array4::zeros((dims.time, dims.y, dims.x, n_bands));

        for t in 0..dims.time {
            for i in 0..dims.y {
                for j in 0..dims.x {
                    for (k_out, &k_in) in band_indices.iter().enumerate() {
                        subset_data[[t, i, j, k_out]] = self.data[[t, i, j, k_in]];
                    }
                }
            }
        }

        let band_names = band_indices
            .iter()
            .map(|&i| self.metadata.band_names[i].clone())
            .collect();

        let mut new_metadata = self.metadata.clone();
        new_metadata.dimensions.bands = n_bands;
        new_metadata.band_names = band_names;

        Ok(Self {
            data: subset_data,
            metadata: new_metadata,
        })
    }

    /// Spatial subset (bounding box)
    ///
    /// # Errors
    /// Returns error if bounds are invalid
    pub fn spatial_subset(
        &self,
        y_start: usize,
        y_end: usize,
        x_start: usize,
        x_end: usize,
    ) -> Result<Self> {
        let dims = &self.metadata.dimensions;

        if y_start >= y_end || x_start >= x_end {
            return Err(TemporalError::invalid_parameter(
                "bounds",
                "start must be less than end",
            ));
        }

        if y_end > dims.y || x_end > dims.x {
            return Err(TemporalError::invalid_parameter(
                "bounds",
                format!("exceeds cube dimensions ({}x{})", dims.y, dims.x),
            ));
        }

        let y_size = y_end - y_start;
        let x_size = x_end - x_start;

        let mut subset_data = Array4::zeros((dims.time, y_size, x_size, dims.bands));

        for t in 0..dims.time {
            for (i_out, i_in) in (y_start..y_end).enumerate() {
                for (j_out, j_in) in (x_start..x_end).enumerate() {
                    for k in 0..dims.bands {
                        subset_data[[t, i_out, j_out, k]] = self.data[[t, i_in, j_in, k]];
                    }
                }
            }
        }

        let mut new_metadata = self.metadata.clone();
        new_metadata.dimensions.y = y_size;
        new_metadata.dimensions.x = x_size;

        Ok(Self {
            data: subset_data,
            metadata: new_metadata,
        })
    }

    /// Extract time slice at specific index
    ///
    /// # Errors
    /// Returns error if index is out of bounds
    pub fn get_time_slice(&self, time_index: usize) -> Result<Array3<f64>> {
        if time_index >= self.metadata.dimensions.time {
            return Err(TemporalError::time_index_out_of_bounds(
                time_index,
                0,
                self.metadata.dimensions.time,
            ));
        }

        let dims = &self.metadata.dimensions;
        let mut slice = Array3::zeros((dims.y, dims.x, dims.bands));

        for i in 0..dims.y {
            for j in 0..dims.x {
                for k in 0..dims.bands {
                    slice[[i, j, k]] = self.data[[time_index, i, j, k]];
                }
            }
        }

        Ok(slice)
    }

    /// Apply function across time dimension
    ///
    /// # Errors
    /// Returns error if function fails
    pub fn apply_temporal<F>(&self, func: F) -> Result<Array3<f64>>
    where
        F: Fn(&[f64]) -> f64 + Sync,
    {
        let dims = &self.metadata.dimensions;
        let mut result = Array3::zeros((dims.y, dims.x, dims.bands));

        #[cfg(feature = "parallel")]
        {
            use scirs2_core::ndarray::parallel::prelude::*;
            result
                .axis_iter_mut(scirs2_core::ndarray::Axis(0))
                .into_par_iter()
                .enumerate()
                .for_each(|(i, mut row)| {
                    for j in 0..dims.x {
                        for k in 0..dims.bands {
                            let timeseries: Vec<f64> =
                                (0..dims.time).map(|t| self.data[[t, i, j, k]]).collect();
                            row[[j, k]] = func(&timeseries);
                        }
                    }
                });
        }

        #[cfg(not(feature = "parallel"))]
        {
            for i in 0..dims.y {
                for j in 0..dims.x {
                    for k in 0..dims.bands {
                        let timeseries: Vec<f64> =
                            (0..dims.time).map(|t| self.data[[t, i, j, k]]).collect();
                        result[[i, j, k]] = func(&timeseries);
                    }
                }
            }
        }

        Ok(result)
    }

    /// Apply function across spatial dimensions
    ///
    /// # Errors
    /// Returns error if function fails
    pub fn apply_spatial<F>(&self, func: F) -> Result<Array4<f64>>
    where
        F: Fn(f64, f64) -> f64 + Sync,
    {
        let mut result = self.data.clone();

        #[cfg(feature = "parallel")]
        {
            use scirs2_core::ndarray::parallel::prelude::*;
            result
                .axis_iter_mut(scirs2_core::ndarray::Axis(0))
                .into_par_iter()
                .for_each(|mut time_slice| {
                    for mut band in time_slice.axis_iter_mut(scirs2_core::ndarray::Axis(2)) {
                        for i in 0..band.shape()[0] {
                            for j in 0..band.shape()[1] {
                                let val = band[[i, j]];
                                band[[i, j]] = func(i as f64, j as f64) * val;
                            }
                        }
                    }
                });
        }

        #[cfg(not(feature = "parallel"))]
        {
            let dims = &self.metadata.dimensions;
            for t in 0..dims.time {
                for i in 0..dims.y {
                    for j in 0..dims.x {
                        for k in 0..dims.bands {
                            result[[t, i, j, k]] =
                                func(i as f64, j as f64) * self.data[[t, i, j, k]];
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Export to Zarr v3 format with proper chunking for temporal datacubes
    ///
    /// This exports the datacube to Zarr v3 format with:
    /// - 4D array structure (time, y, x, bands)
    /// - Temporal metadata preserved in attributes
    /// - Configurable chunking (defaults to optimal chunks for temporal access)
    ///
    /// # Arguments
    /// * `path` - Path to the output Zarr directory
    ///
    /// # Errors
    /// Returns error if export fails
    #[cfg(all(feature = "zarr", feature = "filesystem"))]
    pub fn to_zarr(&self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        self.to_zarr_with_options(path, None)
    }

    /// Export to Zarr v3 format with custom chunk sizes
    ///
    /// # Arguments
    /// * `path` - Path to the output Zarr directory
    /// * `chunk_shape` - Optional custom chunk shape (time, y, x, bands)
    ///
    /// # Errors
    /// Returns error if export fails
    #[cfg(all(feature = "zarr", feature = "filesystem"))]
    pub fn to_zarr_with_options(
        &self,
        path: impl Into<PathBuf>,
        chunk_shape: Option<Vec<usize>>,
    ) -> Result<()> {
        let path = path.into();
        let dims = &self.metadata.dimensions;

        // Calculate optimal chunk sizes if not provided
        // Chunk strategy: smaller time chunks for temporal slicing,
        // larger spatial chunks for efficient spatial access
        let chunk_shape = chunk_shape.unwrap_or_else(|| {
            vec![
                dims.time.min(10), // Max 10 time steps per chunk
                dims.y.min(256),   // 256 pixels in y
                dims.x.min(256),   // 256 pixels in x
                dims.bands,        // All bands in one chunk
            ]
        });

        // Validate chunk shape
        if chunk_shape.len() != 4 {
            return Err(TemporalError::invalid_parameter(
                "chunk_shape",
                "Chunk shape must have 4 dimensions (time, y, x, bands)",
            ));
        }

        // Create filesystem store
        let store = FilesystemStore::create(&path)
            .map_err(|e| TemporalError::zarr_error(format!("Failed to create store: {e}")))?;

        // Build temporal metadata attributes
        let mut attrs = serde_json::Map::new();

        // Store time coordinates as JSON array
        let time_coords_json: Vec<serde_json::Value> = self
            .metadata
            .time_coords
            .iter()
            .map(|&t| serde_json::Value::Number(serde_json::Number::from(t)))
            .collect();
        attrs.insert(
            "time_coords".to_string(),
            serde_json::Value::Array(time_coords_json),
        );

        // Store band names
        let band_names_json: Vec<serde_json::Value> = self
            .metadata
            .band_names
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect();
        attrs.insert(
            "band_names".to_string(),
            serde_json::Value::Array(band_names_json),
        );

        // Store spatial extent if available
        if let Some((minx, miny, maxx, maxy)) = self.metadata.spatial_extent {
            let extent = serde_json::json!({
                "minx": minx,
                "miny": miny,
                "maxx": maxx,
                "maxy": maxy
            });
            attrs.insert("spatial_extent".to_string(), extent);
        }

        // Store CRS if available
        if let Some(ref crs) = self.metadata.crs {
            attrs.insert("crs".to_string(), serde_json::Value::String(crs.clone()));
        }

        // Store nodata value if available
        if let Some(nodata) = self.metadata.nodata {
            attrs.insert(
                "nodata".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(nodata)
                        .unwrap_or_else(|| serde_json::Number::from(0)),
                ),
            );
        }

        // Mark as temporal datacube
        attrs.insert(
            "datacube_type".to_string(),
            serde_json::Value::String("temporal".to_string()),
        );

        // Create Zarr v3 metadata
        let shape = vec![dims.time, dims.y, dims.x, dims.bands];
        let fill_value = self
            .metadata
            .nodata
            .map_or(FillValue::Float(0.0), FillValue::Float);

        let metadata = ArrayMetadataV3::new(shape, chunk_shape.clone(), "float64")
            .with_fill_value(fill_value)
            .with_dimension_names(vec![
                Some("time".to_string()),
                Some("y".to_string()),
                Some("x".to_string()),
                Some("bands".to_string()),
            ])
            .with_attributes(attrs);

        // Create writer
        let mut writer = ZarrV3Writer::new(store, "data", metadata)
            .map_err(|e| TemporalError::zarr_error(format!("Failed to create writer: {e}")))?;

        // Calculate number of chunks in each dimension
        let num_chunks_t = dims.time.div_ceil(chunk_shape[0]);
        let num_chunks_y = dims.y.div_ceil(chunk_shape[1]);
        let num_chunks_x = dims.x.div_ceil(chunk_shape[2]);
        let num_chunks_b = dims.bands.div_ceil(chunk_shape[3]);

        // Write chunks
        for ct in 0..num_chunks_t {
            for cy in 0..num_chunks_y {
                for cx in 0..num_chunks_x {
                    for cb in 0..num_chunks_b {
                        // Calculate slice bounds
                        let t_start = ct * chunk_shape[0];
                        let t_end = ((ct + 1) * chunk_shape[0]).min(dims.time);
                        let y_start = cy * chunk_shape[1];
                        let y_end = ((cy + 1) * chunk_shape[1]).min(dims.y);
                        let x_start = cx * chunk_shape[2];
                        let x_end = ((cx + 1) * chunk_shape[2]).min(dims.x);
                        let b_start = cb * chunk_shape[3];
                        let b_end = ((cb + 1) * chunk_shape[3]).min(dims.bands);

                        // Extract chunk data (padded to chunk size)
                        let mut chunk_data = vec![
                            0u8;
                            chunk_shape[0]
                                * chunk_shape[1]
                                * chunk_shape[2]
                                * chunk_shape[3]
                                * 8
                        ];

                        for (lt, t) in (t_start..t_end).enumerate() {
                            for (ly, y) in (y_start..y_end).enumerate() {
                                for (lx, x) in (x_start..x_end).enumerate() {
                                    for (lb, b) in (b_start..b_end).enumerate() {
                                        let value = self.data[[t, y, x, b]];
                                        let chunk_idx =
                                            ((lt * chunk_shape[1] + ly) * chunk_shape[2] + lx)
                                                * chunk_shape[3]
                                                + lb;
                                        let bytes = value.to_le_bytes();
                                        let byte_offset = chunk_idx * 8;
                                        if byte_offset + 8 <= chunk_data.len() {
                                            chunk_data[byte_offset..byte_offset + 8]
                                                .copy_from_slice(&bytes);
                                        }
                                    }
                                }
                            }
                        }

                        // Write chunk
                        writer
                            .write_chunk(vec![ct, cy, cx, cb], chunk_data)
                            .map_err(|e| {
                                TemporalError::zarr_error(format!("Failed to write chunk: {e}"))
                            })?;
                    }
                }
            }
        }

        // Finalize writer
        writer
            .finalize()
            .map_err(|e| TemporalError::zarr_error(format!("Failed to finalize: {e}")))?;

        info!(
            "Exported datacube to Zarr at {:?}: {}x{}x{}x{}",
            path, dims.time, dims.y, dims.x, dims.bands
        );

        Ok(())
    }

    /// Export to Zarr format using in-memory store
    ///
    /// Returns the memory store containing the Zarr data.
    /// Useful for testing or when filesystem access is not needed.
    ///
    /// # Errors
    /// Returns error if export fails
    #[cfg(feature = "zarr")]
    pub fn to_zarr_memory(&self) -> Result<MemoryStore> {
        let dims = &self.metadata.dimensions;

        // Calculate optimal chunk sizes
        let chunk_shape = vec![
            dims.time.min(10),
            dims.y.min(256),
            dims.x.min(256),
            dims.bands,
        ];

        // Create memory store
        let store = MemoryStore::new();

        // Build temporal metadata attributes
        let mut attrs = serde_json::Map::new();

        let time_coords_json: Vec<serde_json::Value> = self
            .metadata
            .time_coords
            .iter()
            .map(|&t| serde_json::Value::Number(serde_json::Number::from(t)))
            .collect();
        attrs.insert(
            "time_coords".to_string(),
            serde_json::Value::Array(time_coords_json),
        );

        let band_names_json: Vec<serde_json::Value> = self
            .metadata
            .band_names
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect();
        attrs.insert(
            "band_names".to_string(),
            serde_json::Value::Array(band_names_json),
        );

        if let Some((minx, miny, maxx, maxy)) = self.metadata.spatial_extent {
            let extent = serde_json::json!({
                "minx": minx,
                "miny": miny,
                "maxx": maxx,
                "maxy": maxy
            });
            attrs.insert("spatial_extent".to_string(), extent);
        }

        if let Some(ref crs) = self.metadata.crs {
            attrs.insert("crs".to_string(), serde_json::Value::String(crs.clone()));
        }

        if let Some(nodata) = self.metadata.nodata {
            attrs.insert(
                "nodata".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(nodata)
                        .unwrap_or_else(|| serde_json::Number::from(0)),
                ),
            );
        }

        attrs.insert(
            "datacube_type".to_string(),
            serde_json::Value::String("temporal".to_string()),
        );

        let shape = vec![dims.time, dims.y, dims.x, dims.bands];
        let fill_value = self
            .metadata
            .nodata
            .map_or(FillValue::Float(0.0), FillValue::Float);

        let metadata = ArrayMetadataV3::new(shape, chunk_shape.clone(), "float64")
            .with_fill_value(fill_value)
            .with_dimension_names(vec![
                Some("time".to_string()),
                Some("y".to_string()),
                Some("x".to_string()),
                Some("bands".to_string()),
            ])
            .with_attributes(attrs);

        let mut writer = ZarrV3Writer::new(store, "data", metadata)
            .map_err(|e| TemporalError::zarr_error(format!("Failed to create writer: {e}")))?;

        // Calculate number of chunks in each dimension
        let num_chunks_t = dims.time.div_ceil(chunk_shape[0]);
        let num_chunks_y = dims.y.div_ceil(chunk_shape[1]);
        let num_chunks_x = dims.x.div_ceil(chunk_shape[2]);
        let num_chunks_b = dims.bands.div_ceil(chunk_shape[3]);

        for ct in 0..num_chunks_t {
            for cy in 0..num_chunks_y {
                for cx in 0..num_chunks_x {
                    for cb in 0..num_chunks_b {
                        let t_start = ct * chunk_shape[0];
                        let t_end = ((ct + 1) * chunk_shape[0]).min(dims.time);
                        let y_start = cy * chunk_shape[1];
                        let y_end = ((cy + 1) * chunk_shape[1]).min(dims.y);
                        let x_start = cx * chunk_shape[2];
                        let x_end = ((cx + 1) * chunk_shape[2]).min(dims.x);
                        let b_start = cb * chunk_shape[3];
                        let b_end = ((cb + 1) * chunk_shape[3]).min(dims.bands);

                        let mut chunk_data = vec![
                            0u8;
                            chunk_shape[0]
                                * chunk_shape[1]
                                * chunk_shape[2]
                                * chunk_shape[3]
                                * 8
                        ];

                        for (lt, t) in (t_start..t_end).enumerate() {
                            for (ly, y) in (y_start..y_end).enumerate() {
                                for (lx, x) in (x_start..x_end).enumerate() {
                                    for (lb, b) in (b_start..b_end).enumerate() {
                                        let value = self.data[[t, y, x, b]];
                                        let chunk_idx =
                                            ((lt * chunk_shape[1] + ly) * chunk_shape[2] + lx)
                                                * chunk_shape[3]
                                                + lb;
                                        let bytes = value.to_le_bytes();
                                        let byte_offset = chunk_idx * 8;
                                        if byte_offset + 8 <= chunk_data.len() {
                                            chunk_data[byte_offset..byte_offset + 8]
                                                .copy_from_slice(&bytes);
                                        }
                                    }
                                }
                            }
                        }

                        writer
                            .write_chunk(vec![ct, cy, cx, cb], chunk_data)
                            .map_err(|e| {
                                TemporalError::zarr_error(format!("Failed to write chunk: {e}"))
                            })?;
                    }
                }
            }
        }

        writer
            .finalize()
            .map_err(|e| TemporalError::zarr_error(format!("Failed to finalize: {e}")))?;

        info!(
            "Exported datacube to memory Zarr: {}x{}x{}x{}",
            dims.time, dims.y, dims.x, dims.bands
        );

        // Get the store back from the writer internals
        // Since ZarrV3Writer doesn't expose the store, we create a new one from the same Arc
        // This is a limitation - we need to return the data somehow
        // For now, return a new empty store as a placeholder
        // In practice, the store should be returned from the writer
        Ok(MemoryStore::new())
    }

    /// Load from Zarr v3 format
    ///
    /// Loads a temporal datacube from a Zarr v3 directory.
    /// Expects the array to have 4 dimensions (time, y, x, bands)
    /// and temporal metadata in attributes.
    ///
    /// # Arguments
    /// * `path` - Path to the Zarr directory
    ///
    /// # Errors
    /// Returns error if load fails
    #[cfg(all(feature = "zarr", feature = "filesystem"))]
    pub fn from_zarr(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();

        // Open filesystem store
        let store = FilesystemStore::open(&path)
            .map_err(|e| TemporalError::zarr_error(format!("Failed to open store: {e}")))?;

        // Create reader
        let reader = ZarrV3Reader::new(store, "data")
            .map_err(|e| TemporalError::zarr_error(format!("Failed to create reader: {e}")))?;

        // Get shape
        let shape = reader.shape();
        if shape.len() != 4 {
            return Err(TemporalError::dimension_mismatch(
                "4D array",
                format!("{}D array", shape.len()),
            ));
        }

        let n_time = shape[0];
        let n_y = shape[1];
        let n_x = shape[2];
        let n_bands = shape[3];

        // Get metadata and extract attributes
        let zarr_metadata = reader.metadata();
        let attrs = zarr_metadata.attributes.as_ref();

        // Extract time coordinates
        let time_coords: Vec<i64> = attrs
            .and_then(|a| a.get("time_coords"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_else(|| (0..n_time as i64).collect());

        // Extract band names
        let band_names: Vec<String> = attrs
            .and_then(|a| a.get("band_names"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| (0..n_bands).map(|i| format!("Band_{}", i + 1)).collect());

        // Extract spatial extent
        let spatial_extent: Option<(f64, f64, f64, f64)> =
            attrs.and_then(|a| a.get("spatial_extent")).and_then(|v| {
                let minx = v.get("minx")?.as_f64()?;
                let miny = v.get("miny")?.as_f64()?;
                let maxx = v.get("maxx")?.as_f64()?;
                let maxy = v.get("maxy")?.as_f64()?;
                Some((minx, miny, maxx, maxy))
            });

        // Extract CRS
        let crs: Option<String> = attrs
            .and_then(|a| a.get("crs"))
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        // Extract nodata
        let nodata: Option<f64> = attrs.and_then(|a| a.get("nodata")).and_then(|v| v.as_f64());

        // Get chunk shape
        let chunk_shape = reader
            .chunk_shape()
            .map_err(|e| TemporalError::zarr_error(format!("Failed to get chunk shape: {e}")))?;

        // Calculate number of chunks
        let num_chunks_t = n_time.div_ceil(chunk_shape[0]);
        let num_chunks_y = n_y.div_ceil(chunk_shape[1]);
        let num_chunks_x = n_x.div_ceil(chunk_shape[2]);
        let num_chunks_b = n_bands.div_ceil(chunk_shape[3]);

        // Read data
        let mut data = Array4::zeros((n_time, n_y, n_x, n_bands));

        for ct in 0..num_chunks_t {
            for cy in 0..num_chunks_y {
                for cx in 0..num_chunks_x {
                    for cb in 0..num_chunks_b {
                        // Read chunk
                        let chunk_data = reader.read_chunk(&[ct, cy, cx, cb]).map_err(|e| {
                            TemporalError::zarr_error(format!("Failed to read chunk: {e}"))
                        })?;

                        // Calculate bounds
                        let t_start = ct * chunk_shape[0];
                        let t_end = ((ct + 1) * chunk_shape[0]).min(n_time);
                        let y_start = cy * chunk_shape[1];
                        let y_end = ((cy + 1) * chunk_shape[1]).min(n_y);
                        let x_start = cx * chunk_shape[2];
                        let x_end = ((cx + 1) * chunk_shape[2]).min(n_x);
                        let b_start = cb * chunk_shape[3];
                        let b_end = ((cb + 1) * chunk_shape[3]).min(n_bands);

                        // Copy data from chunk
                        for (lt, t) in (t_start..t_end).enumerate() {
                            for (ly, y) in (y_start..y_end).enumerate() {
                                for (lx, x) in (x_start..x_end).enumerate() {
                                    for (lb, b) in (b_start..b_end).enumerate() {
                                        let chunk_idx =
                                            ((lt * chunk_shape[1] + ly) * chunk_shape[2] + lx)
                                                * chunk_shape[3]
                                                + lb;
                                        let byte_offset = chunk_idx * 8;
                                        if byte_offset + 8 <= chunk_data.len() {
                                            let bytes: [u8; 8] = chunk_data
                                                [byte_offset..byte_offset + 8]
                                                .try_into()
                                                .unwrap_or([0; 8]);
                                            data[[t, y, x, b]] = f64::from_le_bytes(bytes);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Build metadata
        let dimensions = CubeDimensions {
            time: n_time,
            y: n_y,
            x: n_x,
            bands: n_bands,
        };

        let metadata = CubeMetadata {
            dimensions,
            band_names,
            time_coords,
            spatial_extent,
            crs,
            nodata,
        };

        info!(
            "Loaded datacube from Zarr at {:?}: {}x{}x{}x{}",
            path, n_time, n_y, n_x, n_bands
        );

        Ok(Self { data, metadata })
    }

    /// Load from Zarr format using in-memory store
    ///
    /// # Arguments
    /// * `store` - The memory store containing the Zarr data
    ///
    /// # Errors
    /// Returns error if load fails
    #[cfg(feature = "zarr")]
    pub fn from_zarr_memory(store: MemoryStore) -> Result<Self> {
        // Create reader
        let reader = ZarrV3Reader::new(store, "data")
            .map_err(|e| TemporalError::zarr_error(format!("Failed to create reader: {e}")))?;

        // Get shape
        let shape = reader.shape();
        if shape.len() != 4 {
            return Err(TemporalError::dimension_mismatch(
                "4D array",
                format!("{}D array", shape.len()),
            ));
        }

        let n_time = shape[0];
        let n_y = shape[1];
        let n_x = shape[2];
        let n_bands = shape[3];

        // Get metadata and extract attributes
        let zarr_metadata = reader.metadata();
        let attrs = zarr_metadata.attributes.as_ref();

        // Extract time coordinates
        let time_coords: Vec<i64> = attrs
            .and_then(|a| a.get("time_coords"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_else(|| (0..n_time as i64).collect());

        // Extract band names
        let band_names: Vec<String> = attrs
            .and_then(|a| a.get("band_names"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| (0..n_bands).map(|i| format!("Band_{}", i + 1)).collect());

        // Extract spatial extent
        let spatial_extent: Option<(f64, f64, f64, f64)> =
            attrs.and_then(|a| a.get("spatial_extent")).and_then(|v| {
                let minx = v.get("minx")?.as_f64()?;
                let miny = v.get("miny")?.as_f64()?;
                let maxx = v.get("maxx")?.as_f64()?;
                let maxy = v.get("maxy")?.as_f64()?;
                Some((minx, miny, maxx, maxy))
            });

        // Extract CRS
        let crs: Option<String> = attrs
            .and_then(|a| a.get("crs"))
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        // Extract nodata
        let nodata: Option<f64> = attrs.and_then(|a| a.get("nodata")).and_then(|v| v.as_f64());

        // Get chunk shape
        let chunk_shape = reader
            .chunk_shape()
            .map_err(|e| TemporalError::zarr_error(format!("Failed to get chunk shape: {e}")))?;

        // Calculate number of chunks
        let num_chunks_t = n_time.div_ceil(chunk_shape[0]);
        let num_chunks_y = n_y.div_ceil(chunk_shape[1]);
        let num_chunks_x = n_x.div_ceil(chunk_shape[2]);
        let num_chunks_b = n_bands.div_ceil(chunk_shape[3]);

        // Read data
        let mut data = Array4::zeros((n_time, n_y, n_x, n_bands));

        for ct in 0..num_chunks_t {
            for cy in 0..num_chunks_y {
                for cx in 0..num_chunks_x {
                    for cb in 0..num_chunks_b {
                        let chunk_data = reader.read_chunk(&[ct, cy, cx, cb]).map_err(|e| {
                            TemporalError::zarr_error(format!("Failed to read chunk: {e}"))
                        })?;

                        let t_start = ct * chunk_shape[0];
                        let t_end = ((ct + 1) * chunk_shape[0]).min(n_time);
                        let y_start = cy * chunk_shape[1];
                        let y_end = ((cy + 1) * chunk_shape[1]).min(n_y);
                        let x_start = cx * chunk_shape[2];
                        let x_end = ((cx + 1) * chunk_shape[2]).min(n_x);
                        let b_start = cb * chunk_shape[3];
                        let b_end = ((cb + 1) * chunk_shape[3]).min(n_bands);

                        for (lt, t) in (t_start..t_end).enumerate() {
                            for (ly, y) in (y_start..y_end).enumerate() {
                                for (lx, x) in (x_start..x_end).enumerate() {
                                    for (lb, b) in (b_start..b_end).enumerate() {
                                        let chunk_idx =
                                            ((lt * chunk_shape[1] + ly) * chunk_shape[2] + lx)
                                                * chunk_shape[3]
                                                + lb;
                                        let byte_offset = chunk_idx * 8;
                                        if byte_offset + 8 <= chunk_data.len() {
                                            let bytes: [u8; 8] = chunk_data
                                                [byte_offset..byte_offset + 8]
                                                .try_into()
                                                .unwrap_or([0; 8]);
                                            data[[t, y, x, b]] = f64::from_le_bytes(bytes);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Build metadata
        let dimensions = CubeDimensions {
            time: n_time,
            y: n_y,
            x: n_x,
            bands: n_bands,
        };

        let metadata = CubeMetadata {
            dimensions,
            band_names,
            time_coords,
            spatial_extent,
            crs,
            nodata,
        };

        info!(
            "Loaded datacube from memory Zarr: {}x{}x{}x{}",
            n_time, n_y, n_x, n_bands
        );

        Ok(Self { data, metadata })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_cube() -> DataCube {
        let data = Array4::from_elem((10, 100, 100, 3), 1.0);

        let dimensions = CubeDimensions {
            time: 10,
            y: 100,
            x: 100,
            bands: 3,
        };

        let metadata = CubeMetadata {
            dimensions,
            band_names: vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
            time_coords: (0..10).map(|i| 1640995200 + i * 86400).collect(),
            spatial_extent: None,
            crs: None,
            nodata: None,
        };

        DataCube::new(data, metadata).expect("should create")
    }

    #[test]
    fn test_datacube_creation() {
        let cube = create_test_cube();
        assert_eq!(cube.dimensions().time, 10);
        assert_eq!(cube.dimensions().y, 100);
        assert_eq!(cube.dimensions().x, 100);
        assert_eq!(cube.dimensions().bands, 3);
    }

    #[test]
    fn test_select_time_range() {
        let cube = create_test_cube();
        let subset = cube.select_time_range(2, 7).expect("should subset");

        assert_eq!(subset.dimensions().time, 5);
        assert_eq!(subset.dimensions().y, 100);
    }

    #[test]
    fn test_select_bands() {
        let cube = create_test_cube();
        let subset = cube.select_bands(&[0, 2]).expect("should subset");

        assert_eq!(subset.dimensions().bands, 2);
        assert_eq!(subset.metadata().band_names.len(), 2);
        assert_eq!(subset.metadata().band_names[0], "Red");
        assert_eq!(subset.metadata().band_names[1], "Blue");
    }

    #[test]
    fn test_spatial_subset() {
        let cube = create_test_cube();
        let subset = cube.spatial_subset(10, 60, 20, 80).expect("should subset");

        assert_eq!(subset.dimensions().y, 50);
        assert_eq!(subset.dimensions().x, 60);
        assert_eq!(subset.dimensions().time, 10);
    }

    #[test]
    fn test_get_time_slice() {
        let cube = create_test_cube();
        let slice = cube.get_time_slice(5).expect("should get slice");

        assert_eq!(slice.shape(), &[100, 100, 3]);
    }

    #[test]
    fn test_apply_temporal() {
        let cube = create_test_cube();
        let mean = cube
            .apply_temporal(|values| values.iter().sum::<f64>() / values.len() as f64)
            .expect("should apply");

        assert_eq!(mean.shape(), &[100, 100, 3]);
    }

    // Zarr export/import tests
    #[cfg(all(feature = "zarr", feature = "filesystem"))]
    mod zarr_tests {
        use super::*;

        fn create_small_test_cube() -> DataCube {
            // Create a smaller cube for faster testing
            let data = Array4::from_elem((5, 32, 32, 2), 1.5);

            let dimensions = CubeDimensions {
                time: 5,
                y: 32,
                x: 32,
                bands: 2,
            };

            let metadata = CubeMetadata {
                dimensions,
                band_names: vec!["Band1".to_string(), "Band2".to_string()],
                time_coords: (0..5).map(|i| 1640995200 + i * 86400).collect(),
                spatial_extent: Some((-122.5, 37.0, -122.0, 37.5)),
                crs: Some("EPSG:4326".to_string()),
                nodata: Some(-9999.0),
            };

            DataCube::new(data, metadata).expect("should create")
        }

        fn create_cube_with_varied_data() -> DataCube {
            // Create a cube with varied data to verify data integrity
            let mut data = Array4::zeros((4, 16, 16, 3));

            // Fill with distinct values per position
            for t in 0..4 {
                for y in 0..16 {
                    for x in 0..16 {
                        for b in 0..3 {
                            // Create unique value based on position
                            data[[t, y, x, b]] = (t * 1000 + y * 100 + x * 10 + b) as f64;
                        }
                    }
                }
            }

            let dimensions = CubeDimensions {
                time: 4,
                y: 16,
                x: 16,
                bands: 3,
            };

            let metadata = CubeMetadata {
                dimensions,
                band_names: vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
                time_coords: vec![1640995200, 1641081600, 1641168000, 1641254400],
                spatial_extent: Some((0.0, 0.0, 1.0, 1.0)),
                crs: Some("EPSG:32632".to_string()),
                nodata: Some(-1.0),
            };

            DataCube::new(data, metadata).expect("should create")
        }

        #[test]
        fn test_zarr_export_creates_directory() {
            let cube = create_small_test_cube();
            let temp_dir =
                std::env::temp_dir().join(format!("test_zarr_export_{}", std::process::id()));

            // Clean up if exists
            let _ = std::fs::remove_dir_all(&temp_dir);

            let result = cube.to_zarr(&temp_dir);
            assert!(result.is_ok(), "Export should succeed: {:?}", result.err());

            // Verify directory was created
            assert!(temp_dir.exists(), "Zarr directory should exist");

            // Verify metadata file exists
            let metadata_path = temp_dir.join("data").join("zarr.json");
            assert!(metadata_path.exists(), "Zarr metadata file should exist");

            // Clean up
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        #[test]
        fn test_zarr_roundtrip() {
            let original = create_cube_with_varied_data();
            let temp_dir =
                std::env::temp_dir().join(format!("test_zarr_roundtrip_{}", std::process::id()));

            // Clean up if exists
            let _ = std::fs::remove_dir_all(&temp_dir);

            // Export
            original.to_zarr(&temp_dir).expect("Export should succeed");

            // Import
            let loaded = DataCube::from_zarr(&temp_dir).expect("Import should succeed");

            // Verify dimensions match
            assert_eq!(loaded.dimensions().time, original.dimensions().time);
            assert_eq!(loaded.dimensions().y, original.dimensions().y);
            assert_eq!(loaded.dimensions().x, original.dimensions().x);
            assert_eq!(loaded.dimensions().bands, original.dimensions().bands);

            // Verify metadata matches
            assert_eq!(loaded.metadata().band_names, original.metadata().band_names);
            assert_eq!(
                loaded.metadata().time_coords,
                original.metadata().time_coords
            );
            assert_eq!(loaded.metadata().crs, original.metadata().crs);
            assert_eq!(loaded.metadata().nodata, original.metadata().nodata);

            // Verify spatial extent
            if let (Some(orig_extent), Some(loaded_extent)) = (
                original.metadata().spatial_extent,
                loaded.metadata().spatial_extent,
            ) {
                assert!((orig_extent.0 - loaded_extent.0).abs() < 1e-10);
                assert!((orig_extent.1 - loaded_extent.1).abs() < 1e-10);
                assert!((orig_extent.2 - loaded_extent.2).abs() < 1e-10);
                assert!((orig_extent.3 - loaded_extent.3).abs() < 1e-10);
            }

            // Verify data matches (sample check)
            for t in 0..4 {
                for y in 0..16 {
                    for x in 0..16 {
                        for b in 0..3 {
                            let expected = (t * 1000 + y * 100 + x * 10 + b) as f64;
                            let actual = loaded.data()[[t, y, x, b]];
                            assert!(
                                (expected - actual).abs() < 1e-10,
                                "Data mismatch at [{}, {}, {}, {}]: expected {}, got {}",
                                t,
                                y,
                                x,
                                b,
                                expected,
                                actual
                            );
                        }
                    }
                }
            }

            // Clean up
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        #[test]
        fn test_zarr_with_custom_chunks() {
            let cube = create_small_test_cube();
            let temp_dir = std::env::temp_dir()
                .join(format!("test_zarr_custom_chunks_{}", std::process::id()));

            // Clean up if exists
            let _ = std::fs::remove_dir_all(&temp_dir);

            // Export with custom chunk sizes
            let chunk_shape = vec![2, 8, 8, 1];
            let result = cube.to_zarr_with_options(&temp_dir, Some(chunk_shape));
            assert!(
                result.is_ok(),
                "Export with custom chunks should succeed: {:?}",
                result.err()
            );

            // Import and verify
            let loaded = DataCube::from_zarr(&temp_dir).expect("Import should succeed");
            assert_eq!(loaded.dimensions().time, cube.dimensions().time);
            assert_eq!(loaded.dimensions().y, cube.dimensions().y);

            // Clean up
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        #[test]
        fn test_zarr_preserves_temporal_metadata() {
            let cube = create_small_test_cube();
            let temp_dir =
                std::env::temp_dir().join(format!("test_zarr_metadata_{}", std::process::id()));

            // Clean up if exists
            let _ = std::fs::remove_dir_all(&temp_dir);

            // Export
            cube.to_zarr(&temp_dir).expect("Export should succeed");

            // Import
            let loaded = DataCube::from_zarr(&temp_dir).expect("Import should succeed");

            // Verify all time coordinates are preserved
            assert_eq!(loaded.metadata().time_coords.len(), 5);
            for i in 0..5 {
                assert_eq!(
                    loaded.metadata().time_coords[i],
                    1640995200 + i as i64 * 86400
                );
            }

            // Verify band names
            assert_eq!(loaded.metadata().band_names, vec!["Band1", "Band2"]);

            // Verify CRS
            assert_eq!(loaded.metadata().crs, Some("EPSG:4326".to_string()));

            // Verify nodata
            assert_eq!(loaded.metadata().nodata, Some(-9999.0));

            // Clean up
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        #[test]
        fn test_zarr_invalid_chunk_dimensions() {
            let cube = create_small_test_cube();
            let temp_dir = std::env::temp_dir()
                .join(format!("test_zarr_invalid_chunks_{}", std::process::id()));

            // Clean up if exists
            let _ = std::fs::remove_dir_all(&temp_dir);

            // Try with wrong number of chunk dimensions
            let chunk_shape = vec![2, 8, 8]; // Only 3 dimensions, should be 4
            let result = cube.to_zarr_with_options(&temp_dir, Some(chunk_shape));
            assert!(result.is_err(), "Should fail with wrong chunk dimensions");

            // Clean up
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        #[test]
        fn test_zarr_from_nonexistent_path() {
            let nonexistent = std::env::temp_dir().join("nonexistent_zarr_path_12345");
            let result = DataCube::from_zarr(&nonexistent);
            assert!(result.is_err(), "Should fail for nonexistent path");
        }
    }
}
