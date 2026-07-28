//! Time Series Raster Module
//!
//! This module provides time-indexed raster collections with temporal metadata,
//! efficient storage with lazy loading, temporal querying, and gap detection.

use crate::error::{Result, TemporalError};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use scirs2_core::ndarray::{Array2, Array3, s};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info, warn};

pub mod collection;
pub mod datacube;

pub use collection::*;
pub use datacube::*;

/// Loads raster data for a lazily-referenced [`TemporalRasterEntry`].
///
/// The temporal crate is format-agnostic: rather than hard-coding a GeoTIFF or
/// NetCDF reader (and pulling those heavy dependencies into every consumer),
/// lazy entries created via [`TemporalRasterEntry::new_lazy`] /
/// [`TimeSeriesRaster::add_raster_lazy`] are resolved through a caller-supplied
/// loader. The loader receives the entry's stored `source_path` and returns the
/// decoded `(height, width, bands)` array.
///
/// Any closure `Fn(&str) -> Result<Array3<f64>>` implements this trait, so a
/// caller can wire in `oxigeo-geotiff`, `oxigeo-zarr`, or any other reader
/// without this crate depending on it:
///
/// ```
/// use oxigeo_temporal::timeseries::RasterLoader;
/// use oxigeo_temporal::error::Result;
/// use scirs2_core::ndarray::Array3;
///
/// let loader = |path: &str| -> Result<Array3<f64>> {
///     // ... decode `path` with your format reader ...
///     Ok(Array3::zeros((2, 2, 1)))
/// };
/// let _ = RasterLoader::load(&loader, "scene.tif");
/// ```
pub trait RasterLoader {
    /// Decode the raster stored at `source_path` into an `(h, w, bands)` array.
    ///
    /// # Errors
    /// Returns an error if the source cannot be read or decoded.
    fn load(&self, source_path: &str) -> Result<Array3<f64>>;
}

impl<F> RasterLoader for F
where
    F: Fn(&str) -> Result<Array3<f64>>,
{
    fn load(&self, source_path: &str) -> Result<Array3<f64>> {
        (self)(source_path)
    }
}

/// Temporal resolution for time series
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporalResolution {
    /// Daily resolution
    Daily,
    /// Weekly resolution
    Weekly,
    /// Monthly resolution
    Monthly,
    /// Yearly resolution
    Yearly,
    /// Custom interval in seconds
    Custom(i64),
}

impl TemporalResolution {
    /// Get the duration in seconds
    #[must_use]
    pub fn as_seconds(&self) -> i64 {
        match self {
            Self::Daily => 86400,
            Self::Weekly => 604800,
            Self::Monthly => 2592000, // Approximate 30 days
            Self::Yearly => 31536000, // 365 days
            Self::Custom(secs) => *secs,
        }
    }

    /// Get the duration in days
    #[must_use]
    pub fn as_days(&self) -> f64 {
        self.as_seconds() as f64 / 86400.0
    }
}

/// Temporal metadata for a single raster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalMetadata {
    /// Timestamp (UTC)
    pub timestamp: DateTime<Utc>,
    /// Acquisition date
    pub acquisition_date: NaiveDate,
    /// Sensor/satellite identifier
    pub sensor: Option<String>,
    /// Scene identifier
    pub scene_id: Option<String>,
    /// Cloud cover percentage (0-100)
    pub cloud_cover: Option<f32>,
    /// Quality score (0-1)
    pub quality_score: Option<f32>,
    /// Processing level
    pub processing_level: Option<String>,
    /// Custom metadata
    pub custom: BTreeMap<String, String>,
}

impl TemporalMetadata {
    /// Create new temporal metadata
    #[must_use]
    pub fn new(timestamp: DateTime<Utc>, acquisition_date: NaiveDate) -> Self {
        Self {
            timestamp,
            acquisition_date,
            sensor: None,
            scene_id: None,
            cloud_cover: None,
            quality_score: None,
            processing_level: None,
            custom: BTreeMap::new(),
        }
    }

    /// Create from naive datetime
    pub fn from_naive_datetime(dt: NaiveDateTime) -> Result<Self> {
        let timestamp = DateTime::from_naive_utc_and_offset(dt, Utc);
        let acquisition_date = dt.date();
        Ok(Self::new(timestamp, acquisition_date))
    }

    /// Set sensor information
    #[must_use]
    pub fn with_sensor(mut self, sensor: impl Into<String>) -> Self {
        self.sensor = Some(sensor.into());
        self
    }

    /// Set scene ID
    #[must_use]
    pub fn with_scene_id(mut self, scene_id: impl Into<String>) -> Self {
        self.scene_id = Some(scene_id.into());
        self
    }

    /// Set cloud cover percentage
    #[must_use]
    pub fn with_cloud_cover(mut self, cloud_cover: f32) -> Self {
        self.cloud_cover = Some(cloud_cover.clamp(0.0, 100.0));
        self
    }

    /// Set quality score
    #[must_use]
    pub fn with_quality_score(mut self, quality_score: f32) -> Self {
        self.quality_score = Some(quality_score.clamp(0.0, 1.0));
        self
    }

    /// Set processing level
    #[must_use]
    pub fn with_processing_level(mut self, level: impl Into<String>) -> Self {
        self.processing_level = Some(level.into());
        self
    }

    /// Add custom metadata field
    #[must_use]
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }
}

/// Time-indexed raster entry
#[derive(Debug, Clone)]
pub struct TemporalRasterEntry {
    /// Temporal metadata
    pub metadata: TemporalMetadata,
    /// Raster data (lazy-loaded)
    pub data: Option<Array3<f64>>,
    /// Data source path (for lazy loading)
    pub source_path: Option<String>,
}

impl TemporalRasterEntry {
    /// Create new entry with in-memory data
    #[must_use]
    pub fn new_loaded(metadata: TemporalMetadata, data: Array3<f64>) -> Self {
        Self {
            metadata,
            data: Some(data),
            source_path: None,
        }
    }

    /// Create new entry with lazy loading.
    ///
    /// The data is not read until [`Self::load_data`] / [`Self::ensure_loaded`]
    /// is called with a [`RasterLoader`] that knows how to decode
    /// `source_path`. Until then the entry holds no pixel data and
    /// pixel-extraction APIs will report that the data is not yet loaded.
    #[must_use]
    pub fn new_lazy(metadata: TemporalMetadata, source_path: String) -> Self {
        Self {
            metadata,
            data: None,
            source_path: Some(source_path),
        }
    }

    /// Check if data is loaded
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.data.is_some()
    }

    /// Ensure the entry's raster data is loaded, decoding it via `loader` from
    /// the stored `source_path` if it is not already in memory.
    ///
    /// Returns a reference to the loaded data. This is idempotent: if the data
    /// is already present, `loader` is not invoked.
    ///
    /// # Errors
    /// Returns an error if the entry is lazy but has no `source_path`, or if the
    /// loader fails to decode the source.
    pub fn ensure_loaded(&mut self, loader: &dyn RasterLoader) -> Result<&Array3<f64>> {
        if self.data.is_none() {
            let path = self.source_path.as_deref().ok_or_else(|| {
                TemporalError::invalid_input("Lazy raster entry has no source_path to load from")
            })?;
            let array = loader.load(path)?;
            self.data = Some(array);
        }
        // `data` is guaranteed `Some` here.
        self.data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Raster data unexpectedly missing"))
    }

    /// Load (or reload) the entry's raster data via `loader`, replacing any
    /// existing in-memory data.
    ///
    /// # Errors
    /// Returns an error if the entry has no `source_path` or the loader fails.
    pub fn load_data(&mut self, loader: &dyn RasterLoader) -> Result<()> {
        let path = self.source_path.as_deref().ok_or_else(|| {
            TemporalError::invalid_input("Lazy raster entry has no source_path to load from")
        })?;
        self.data = Some(loader.load(path)?);
        Ok(())
    }

    /// Drop the in-memory raster data, keeping the metadata and `source_path`
    /// so the entry can be reloaded later. No-op if the entry has no
    /// `source_path` to reload from (to avoid losing the only copy of the data).
    pub fn unload(&mut self) {
        if self.source_path.is_some() {
            self.data = None;
        }
    }

    /// Get data dimensions (if loaded)
    #[must_use]
    pub fn shape(&self) -> Option<(usize, usize, usize)> {
        self.data
            .as_ref()
            .map(|d| (d.shape()[0], d.shape()[1], d.shape()[2]))
    }
}

/// Time series raster collection
///
/// This struct represents a collection of rasters indexed by time,
/// with support for temporal queries, gap detection, and lazy loading.
#[derive(Debug, Clone)]
pub struct TimeSeriesRaster {
    /// Time-indexed rasters (sorted by timestamp)
    entries: BTreeMap<i64, TemporalRasterEntry>,
    /// Temporal resolution
    resolution: Option<TemporalResolution>,
    /// Expected spatial dimensions (height, width, bands)
    expected_shape: Option<(usize, usize, usize)>,
}

impl TimeSeriesRaster {
    /// Create a new empty time series raster collection
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            resolution: None,
            expected_shape: None,
        }
    }

    /// Create with expected resolution
    #[must_use]
    pub fn with_resolution(resolution: TemporalResolution) -> Self {
        Self {
            entries: BTreeMap::new(),
            resolution: Some(resolution),
            expected_shape: None,
        }
    }

    /// Create with expected shape
    #[must_use]
    pub fn with_shape(height: usize, width: usize, bands: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            resolution: None,
            expected_shape: Some((height, width, bands)),
        }
    }

    /// Add a raster with timestamp
    ///
    /// # Errors
    /// Returns error if shape doesn't match expected dimensions
    pub fn add_raster(&mut self, metadata: TemporalMetadata, data: Array3<f64>) -> Result<()> {
        // Validate shape if expected shape is set
        if let Some((exp_h, exp_w, exp_b)) = self.expected_shape {
            let shape = data.shape();
            if shape[0] != exp_h || shape[1] != exp_w || shape[2] != exp_b {
                return Err(TemporalError::dimension_mismatch(
                    format!("{}x{}x{}", exp_h, exp_w, exp_b),
                    format!("{}x{}x{}", shape[0], shape[1], shape[2]),
                ));
            }
        } else {
            // Set expected shape from first raster
            let shape = data.shape();
            self.expected_shape = Some((shape[0], shape[1], shape[2]));
        }

        let timestamp_key = metadata.timestamp.timestamp();
        let entry = TemporalRasterEntry::new_loaded(metadata, data);
        self.entries.insert(timestamp_key, entry);

        debug!("Added raster at timestamp {}", timestamp_key);
        Ok(())
    }

    /// Add a raster with lazy loading
    ///
    /// # Errors
    /// Returns error if metadata is invalid
    pub fn add_raster_lazy(
        &mut self,
        metadata: TemporalMetadata,
        source_path: String,
    ) -> Result<()> {
        let timestamp_key = metadata.timestamp.timestamp();
        let entry = TemporalRasterEntry::new_lazy(metadata, source_path);
        self.entries.insert(timestamp_key, entry);

        debug!("Added lazy raster at timestamp {}", timestamp_key);
        Ok(())
    }

    /// Load all lazily-referenced entries using the supplied [`RasterLoader`].
    ///
    /// Entries that already hold in-memory data are left untouched. After a
    /// successful call every entry has its data resolved, so pixel-extraction
    /// APIs (e.g. [`Self::extract_pixel_timeseries`]) will succeed. If the
    /// collection has an expected shape, each loaded raster is validated
    /// against it.
    ///
    /// # Errors
    /// Returns an error on the first entry that fails to load or whose loaded
    /// shape does not match the collection's expected shape.
    pub fn load_all(&mut self, loader: &dyn RasterLoader) -> Result<usize> {
        let expected = self.expected_shape;
        let mut loaded = 0usize;

        for entry in self.entries.values_mut() {
            if entry.is_loaded() {
                continue;
            }
            let data = entry.ensure_loaded(loader)?;
            if let Some((exp_h, exp_w, exp_b)) = expected {
                let shape = data.shape();
                if shape[0] != exp_h || shape[1] != exp_w || shape[2] != exp_b {
                    return Err(TemporalError::dimension_mismatch(
                        format!("{exp_h}x{exp_w}x{exp_b}"),
                        format!("{}x{}x{}", shape[0], shape[1], shape[2]),
                    ));
                }
            }
            loaded += 1;
        }

        // Adopt an expected shape from the first loaded entry if none was set.
        if self.expected_shape.is_none()
            && let Some(entry) = self.entries.values().next()
            && let Some(shape) = entry.shape()
        {
            self.expected_shape = Some(shape);
        }

        debug!("Loaded {loaded} lazy raster entries");
        Ok(loaded)
    }

    /// Get the number of rasters in the time series
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if time series is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get temporal resolution
    #[must_use]
    pub fn resolution(&self) -> Option<TemporalResolution> {
        self.resolution
    }

    /// Set temporal resolution
    pub fn set_resolution(&mut self, resolution: TemporalResolution) {
        self.resolution = Some(resolution);
    }

    /// Get expected shape
    #[must_use]
    pub fn expected_shape(&self) -> Option<(usize, usize, usize)> {
        self.expected_shape
    }

    /// Get time range (start, end)
    #[must_use]
    pub fn time_range(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        if self.is_empty() {
            return None;
        }

        let start_ts = self.entries.keys().next()?;
        let end_ts = self.entries.keys().next_back()?;

        let start = DateTime::from_timestamp(*start_ts, 0)?;
        let end = DateTime::from_timestamp(*end_ts, 0)?;

        Some((start, end))
    }

    /// Get all timestamps
    #[must_use]
    pub fn timestamps(&self) -> Vec<DateTime<Utc>> {
        self.entries
            .keys()
            .filter_map(|&ts| DateTime::from_timestamp(ts, 0))
            .collect()
    }

    /// Get raster at specific timestamp
    #[must_use]
    pub fn get_at_timestamp(&self, timestamp: &DateTime<Utc>) -> Option<&TemporalRasterEntry> {
        self.entries.get(&timestamp.timestamp())
    }

    /// Get mutable raster at specific timestamp
    pub fn get_at_timestamp_mut(
        &mut self,
        timestamp: &DateTime<Utc>,
    ) -> Option<&mut TemporalRasterEntry> {
        self.entries.get_mut(&timestamp.timestamp())
    }

    /// Get raster by index
    ///
    /// # Errors
    /// Returns error if index is out of bounds
    pub fn get_by_index(&self, index: usize) -> Result<&TemporalRasterEntry> {
        self.entries
            .values()
            .nth(index)
            .ok_or_else(|| TemporalError::time_index_out_of_bounds(index, 0, self.len()))
    }

    /// Query rasters within a time range
    #[must_use]
    pub fn query_range(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Vec<&TemporalRasterEntry> {
        let start_ts = start.timestamp();
        let end_ts = end.timestamp();

        self.entries
            .range(start_ts..=end_ts)
            .map(|(_, entry)| entry)
            .collect()
    }

    /// Detect gaps in the time series based on expected resolution
    ///
    /// # Errors
    /// Returns error if resolution is not set
    pub fn detect_gaps(&self) -> Result<Vec<(DateTime<Utc>, DateTime<Utc>)>> {
        let resolution = self
            .resolution
            .ok_or_else(|| TemporalError::invalid_input("Temporal resolution not set"))?;

        let mut gaps = Vec::new();
        let timestamps: Vec<i64> = self.entries.keys().copied().collect();

        if timestamps.len() < 2 {
            return Ok(gaps);
        }

        let expected_interval = resolution.as_seconds();

        for i in 0..timestamps.len() - 1 {
            let current = timestamps[i];
            let next = timestamps[i + 1];
            let actual_interval = next - current;

            // Allow 10% tolerance
            let tolerance = (expected_interval as f64 * 0.1) as i64;
            if actual_interval > expected_interval + tolerance
                && let (Some(gap_start), Some(gap_end)) = (
                    DateTime::from_timestamp(current, 0),
                    DateTime::from_timestamp(next, 0),
                )
            {
                gaps.push((gap_start, gap_end));
                warn!(
                    "Gap detected between {} and {} ({} seconds)",
                    gap_start, gap_end, actual_interval
                );
            }
        }

        info!("Detected {} gaps in time series", gaps.len());
        Ok(gaps)
    }

    /// Get statistics about the time series
    #[must_use]
    pub fn stats(&self) -> TimeSeriesStats {
        let count = self.len();
        let loaded_count = self.entries.values().filter(|e| e.is_loaded()).count();
        let lazy_count = count - loaded_count;

        let (time_range_start, time_range_end) = self
            .time_range()
            .map(|(s, e)| (Some(s), Some(e)))
            .unwrap_or((None, None));

        let avg_cloud_cover = if count > 0 {
            let sum: f32 = self
                .entries
                .values()
                .filter_map(|e| e.metadata.cloud_cover)
                .sum();
            let cloud_count = self
                .entries
                .values()
                .filter(|e| e.metadata.cloud_cover.is_some())
                .count();
            if cloud_count > 0 {
                Some(sum / cloud_count as f32)
            } else {
                None
            }
        } else {
            None
        };

        TimeSeriesStats {
            count,
            loaded_count,
            lazy_count,
            time_range_start,
            time_range_end,
            resolution: self.resolution,
            avg_cloud_cover,
        }
    }

    /// Filter rasters by cloud cover threshold
    ///
    /// # Errors
    /// Returns error if cloud cover threshold is invalid
    pub fn filter_by_cloud_cover(&mut self, max_cloud_cover: f32) -> Result<usize> {
        if !(0.0..=100.0).contains(&max_cloud_cover) {
            return Err(TemporalError::invalid_parameter(
                "max_cloud_cover",
                "must be between 0 and 100",
            ));
        }

        let original_count = self.len();
        self.entries.retain(|_, entry| {
            entry
                .metadata
                .cloud_cover
                .is_none_or(|cc| cc <= max_cloud_cover)
        });

        let removed = original_count - self.len();
        info!(
            "Filtered {} rasters with cloud cover > {}%",
            removed, max_cloud_cover
        );
        Ok(removed)
    }

    /// Filter rasters by quality score threshold
    ///
    /// # Errors
    /// Returns error if quality threshold is invalid
    pub fn filter_by_quality(&mut self, min_quality: f32) -> Result<usize> {
        if !(0.0..=1.0).contains(&min_quality) {
            return Err(TemporalError::invalid_parameter(
                "min_quality",
                "must be between 0 and 1",
            ));
        }

        let original_count = self.len();
        self.entries.retain(|_, entry| {
            entry
                .metadata
                .quality_score
                .is_none_or(|qs| qs >= min_quality)
        });

        let removed = original_count - self.len();
        info!(
            "Filtered {} rasters with quality < {}",
            removed, min_quality
        );
        Ok(removed)
    }

    /// Extract pixel time series at specific location
    ///
    /// # Errors
    /// Returns error if coordinates are out of bounds or data not loaded
    pub fn extract_pixel_timeseries(
        &self,
        row: usize,
        col: usize,
        band: usize,
    ) -> Result<Vec<f64>> {
        let mut values = Vec::with_capacity(self.len());

        for entry in self.entries.values() {
            let data = entry.data.as_ref().ok_or_else(|| {
                TemporalError::invalid_input(
                    "Raster data not loaded. Call TimeSeriesRaster::load_all (or \
                     TemporalRasterEntry::load_data) with a RasterLoader first",
                )
            })?;

            // Validate bounds
            let shape = data.shape();
            if row >= shape[0] || col >= shape[1] || band >= shape[2] {
                return Err(TemporalError::invalid_parameter(
                    "coordinates",
                    format!(
                        "({},{},{}) out of bounds for shape ({},{},{})",
                        row, col, band, shape[0], shape[1], shape[2]
                    ),
                ));
            }

            values.push(data[[row, col, band]]);
        }

        Ok(values)
    }

    /// Extract spatial slice at specific time
    ///
    /// # Errors
    /// Returns error if timestamp not found or data not loaded
    pub fn extract_spatial_slice(&self, timestamp: &DateTime<Utc>) -> Result<Array3<f64>> {
        let entry = self.get_at_timestamp(timestamp).ok_or_else(|| {
            TemporalError::invalid_input(format!("Timestamp {} not found", timestamp))
        })?;

        entry
            .data
            .as_ref()
            .cloned()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))
    }

    /// Extract temporal slice for specific band
    ///
    /// # Errors
    /// Returns error if band is out of bounds or data not loaded
    pub fn extract_temporal_slice(&self, band: usize) -> Result<Vec<Array2<f64>>> {
        let mut slices = Vec::with_capacity(self.len());

        for entry in self.entries.values() {
            let data = entry
                .data
                .as_ref()
                .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

            if band >= data.shape()[2] {
                return Err(TemporalError::invalid_parameter(
                    "band",
                    format!("band {} out of bounds (max: {})", band, data.shape()[2] - 1),
                ));
            }

            // Extract 2D slice for this band
            let slice = data.slice(s![.., .., band]).to_owned();
            slices.push(slice);
        }

        Ok(slices)
    }

    /// Calculate temporal statistics at each pixel
    ///
    /// # Errors
    /// Returns error if data not loaded or insufficient data
    pub fn pixel_statistics(&self) -> Result<PixelStatistics> {
        if self.is_empty() {
            return Err(TemporalError::insufficient_data(
                "No rasters in time series",
            ));
        }

        // Get shape from first entry
        let first_entry = self
            .entries
            .values()
            .next()
            .ok_or_else(|| TemporalError::insufficient_data("No entries"))?;
        let data = first_entry
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;
        let (height, width, bands) = (data.shape()[0], data.shape()[1], data.shape()[2]);

        let mut mean = Array3::zeros((height, width, bands));
        let mut min = Array3::from_elem((height, width, bands), f64::INFINITY);
        let mut max = Array3::from_elem((height, width, bands), f64::NEG_INFINITY);
        let mut std_dev = Array3::zeros((height, width, bands));

        let count = self.len() as f64;

        // First pass: calculate mean, min, max
        for entry in self.entries.values() {
            let data = entry
                .data
                .as_ref()
                .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

            for i in 0..height {
                for j in 0..width {
                    for k in 0..bands {
                        let val = data[[i, j, k]];
                        mean[[i, j, k]] += val / count;
                        if val < min[[i, j, k]] {
                            min[[i, j, k]] = val;
                        }
                        if val > max[[i, j, k]] {
                            max[[i, j, k]] = val;
                        }
                    }
                }
            }
        }

        // Second pass: calculate standard deviation
        for entry in self.entries.values() {
            let data = entry
                .data
                .as_ref()
                .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

            for i in 0..height {
                for j in 0..width {
                    for k in 0..bands {
                        let diff = data[[i, j, k]] - mean[[i, j, k]];
                        std_dev[[i, j, k]] += diff * diff / count;
                    }
                }
            }
        }

        // Take square root for std_dev
        std_dev.mapv_inplace(f64::sqrt);

        Ok(PixelStatistics {
            mean,
            min,
            max,
            std_dev,
        })
    }

    /// Get all entries
    #[must_use]
    pub fn entries(&self) -> &BTreeMap<i64, TemporalRasterEntry> {
        &self.entries
    }

    /// Get mutable entries
    pub fn entries_mut(&mut self) -> &mut BTreeMap<i64, TemporalRasterEntry> {
        &mut self.entries
    }

    /// Iterate over entries in chronological order
    pub fn iter(&self) -> impl Iterator<Item = (&i64, &TemporalRasterEntry)> {
        self.entries.iter()
    }
}

impl Default for TimeSeriesRaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics computed across temporal dimension for each pixel
#[derive(Debug, Clone)]
pub struct PixelStatistics {
    /// Mean value over time for each pixel
    pub mean: Array3<f64>,
    /// Minimum value over time for each pixel
    pub min: Array3<f64>,
    /// Maximum value over time for each pixel
    pub max: Array3<f64>,
    /// Standard deviation over time for each pixel
    pub std_dev: Array3<f64>,
}

/// Time series statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    /// Total number of rasters
    pub count: usize,
    /// Number of loaded rasters
    pub loaded_count: usize,
    /// Number of lazy-loaded rasters
    pub lazy_count: usize,
    /// Start of time range
    pub time_range_start: Option<DateTime<Utc>>,
    /// End of time range
    pub time_range_end: Option<DateTime<Utc>>,
    /// Temporal resolution
    pub resolution: Option<TemporalResolution>,
    /// Average cloud cover
    pub avg_cloud_cover: Option<f32>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod lazy_loading_tests {
    use super::*;

    fn meta(day: u32) -> TemporalMetadata {
        let date = NaiveDate::from_ymd_opt(2024, 1, day).expect("valid date");
        let ndt = date.and_hms_opt(0, 0, 0).expect("valid time");
        let ts = DateTime::from_naive_utc_and_offset(ndt, Utc);
        TemporalMetadata::new(ts, date)
    }

    #[test]
    fn test_lazy_entry_loads_via_loader() {
        let mut ts = TimeSeriesRaster::with_shape(1, 1, 1);
        ts.add_raster_lazy(meta(1), "scene_1".to_string()).unwrap();
        ts.add_raster_lazy(meta(2), "scene_2".to_string()).unwrap();

        // Before loading, pixel extraction must fail (data not loaded).
        assert!(ts.extract_pixel_timeseries(0, 0, 0).is_err());

        // A loader that maps a source path to a deterministic value.
        let loader = |path: &str| -> Result<Array3<f64>> {
            let v = match path {
                "scene_1" => 1.0,
                "scene_2" => 2.0,
                other => {
                    return Err(TemporalError::invalid_input(format!(
                        "unknown source {other}"
                    )));
                }
            };
            Ok(Array3::from_elem((1, 1, 1), v))
        };

        let loaded = ts.load_all(&loader).unwrap();
        assert_eq!(loaded, 2, "both lazy entries should have loaded");

        // Now extraction works and returns the loaded values in time order.
        let series = ts.extract_pixel_timeseries(0, 0, 0).unwrap();
        assert_eq!(series, vec![1.0, 2.0]);
    }

    #[test]
    fn test_load_all_idempotent_and_shape_validated() {
        let mut ts = TimeSeriesRaster::with_shape(2, 2, 1);
        ts.add_raster_lazy(meta(1), "ok".to_string()).unwrap();

        let good = |_: &str| -> Result<Array3<f64>> { Ok(Array3::zeros((2, 2, 1))) };
        assert_eq!(ts.load_all(&good).unwrap(), 1);
        // Second call is a no-op (already loaded).
        assert_eq!(ts.load_all(&good).unwrap(), 0);

        // A loader returning the wrong shape must be rejected.
        let mut ts2 = TimeSeriesRaster::with_shape(2, 2, 1);
        ts2.add_raster_lazy(meta(1), "bad".to_string()).unwrap();
        let bad = |_: &str| -> Result<Array3<f64>> { Ok(Array3::zeros((3, 3, 1))) };
        assert!(ts2.load_all(&bad).is_err());
    }

    #[test]
    fn test_ensure_loaded_and_unload_roundtrip() {
        let mut entry = TemporalRasterEntry::new_lazy(meta(1), "src".to_string());
        assert!(!entry.is_loaded());

        let loader = |_: &str| -> Result<Array3<f64>> { Ok(Array3::from_elem((1, 1, 1), 7.0)) };
        {
            let data = entry.ensure_loaded(&loader).unwrap();
            assert_eq!(data[[0, 0, 0]], 7.0);
        }
        assert!(entry.is_loaded());

        // Unload keeps source_path so it can be reloaded.
        entry.unload();
        assert!(!entry.is_loaded());
        entry.load_data(&loader).unwrap();
        assert!(entry.is_loaded());
    }

    #[test]
    fn test_load_data_without_source_path_errors() {
        // A loaded (non-lazy) entry with no source_path cannot be reloaded.
        let mut entry = TemporalRasterEntry::new_loaded(meta(1), Array3::from_elem((1, 1, 1), 1.0));
        entry.unload(); // no-op: has no source_path
        assert!(entry.is_loaded(), "unload must not drop the only copy");

        let loader = |_: &str| -> Result<Array3<f64>> { Ok(Array3::zeros((1, 1, 1))) };
        let mut lazy_no_path =
            TemporalRasterEntry::new_loaded(meta(2), Array3::from_elem((1, 1, 1), 1.0));
        lazy_no_path.data = None; // simulate missing data without a source_path
        assert!(lazy_no_path.load_data(&loader).is_err());
    }
}
