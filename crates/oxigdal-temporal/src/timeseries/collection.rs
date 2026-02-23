//! Time Series Collection Management
//!
//! This module provides utilities for managing collections of time series,
//! including multi-sensor collections, bulk operations, merging, and splitting.

use super::{TemporalMetadata, TemporalRasterEntry, TemporalResolution, TimeSeriesRaster};
use crate::error::{Result, TemporalError};
use chrono::{DateTime, Utc};
// serde traits available if needed for future serialization
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

/// Multi-sensor time series collection
#[derive(Debug, Clone)]
pub struct TimeSeriesCollection {
    /// Collections organized by sensor
    sensors: HashMap<String, TimeSeriesRaster>,
    /// Global temporal resolution
    resolution: Option<TemporalResolution>,
}

impl TimeSeriesCollection {
    /// Create a new empty collection
    #[must_use]
    pub fn new() -> Self {
        Self {
            sensors: HashMap::new(),
            resolution: None,
        }
    }

    /// Create with expected resolution
    #[must_use]
    pub fn with_resolution(resolution: TemporalResolution) -> Self {
        Self {
            sensors: HashMap::new(),
            resolution: Some(resolution),
        }
    }

    /// Add a time series for a specific sensor
    pub fn add_sensor(&mut self, sensor: impl Into<String>, ts: TimeSeriesRaster) {
        self.sensors.insert(sensor.into(), ts);
    }

    /// Get time series for a specific sensor
    #[must_use]
    pub fn get_sensor(&self, sensor: &str) -> Option<&TimeSeriesRaster> {
        self.sensors.get(sensor)
    }

    /// Get mutable time series for a specific sensor
    pub fn get_sensor_mut(&mut self, sensor: &str) -> Option<&mut TimeSeriesRaster> {
        self.sensors.get_mut(sensor)
    }

    /// Get all sensor names
    #[must_use]
    pub fn sensors(&self) -> Vec<String> {
        self.sensors.keys().cloned().collect()
    }

    /// Get total number of rasters across all sensors
    #[must_use]
    pub fn total_rasters(&self) -> usize {
        self.sensors.values().map(|ts| ts.len()).sum()
    }

    /// Merge all sensors into a single time series
    ///
    /// # Errors
    /// Returns error if dimensions don't match between sensors
    pub fn merge_sensors(&self) -> Result<TimeSeriesRaster> {
        if self.sensors.is_empty() {
            return Ok(TimeSeriesRaster::new());
        }

        let mut merged = TimeSeriesRaster::new();
        if let Some(res) = self.resolution {
            merged = TimeSeriesRaster::with_resolution(res);
        }

        for (sensor_name, ts) in &self.sensors {
            debug!("Merging sensor: {}", sensor_name);
            for (timestamp, entry) in ts.iter() {
                // Validate timestamp is parseable
                let _dt = DateTime::from_timestamp(*timestamp, 0).ok_or_else(|| {
                    TemporalError::datetime_parse_error(format!("Invalid timestamp: {}", timestamp))
                })?;

                if let Some(data) = &entry.data {
                    merged.add_raster(entry.metadata.clone(), data.clone())?;
                } else if let Some(path) = &entry.source_path {
                    merged.add_raster_lazy(entry.metadata.clone(), path.clone())?;
                }
            }
        }

        info!(
            "Merged {} sensors into single time series with {} rasters",
            self.sensors.len(),
            merged.len()
        );
        Ok(merged)
    }

    /// Split a time series by sensor
    ///
    /// # Errors
    /// Returns error if sensor information is missing
    pub fn split_by_sensor(ts: &TimeSeriesRaster) -> Result<Self> {
        let mut collection = Self::new();

        for entry in ts.entries().values() {
            let sensor = entry
                .metadata
                .sensor
                .as_ref()
                .ok_or_else(|| TemporalError::metadata_error("Sensor information missing"))?
                .clone();

            if !collection.sensors.contains_key(&sensor) {
                collection.add_sensor(&sensor, TimeSeriesRaster::new());
            }

            let sensor_ts = collection.sensors.get_mut(&sensor).ok_or_else(|| {
                TemporalError::invalid_input(format!("Failed to get sensor: {}", sensor))
            })?;

            if let Some(data) = &entry.data {
                sensor_ts.add_raster(entry.metadata.clone(), data.clone())?;
            } else if let Some(path) = &entry.source_path {
                sensor_ts.add_raster_lazy(entry.metadata.clone(), path.clone())?;
            }
        }

        info!(
            "Split time series into {} sensors",
            collection.sensors.len()
        );
        Ok(collection)
    }

    /// Get time range across all sensors
    #[must_use]
    pub fn time_range(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let mut min_time: Option<DateTime<Utc>> = None;
        let mut max_time: Option<DateTime<Utc>> = None;

        for ts in self.sensors.values() {
            if let Some((start, end)) = ts.time_range() {
                min_time = Some(match min_time {
                    None => start,
                    Some(current) => {
                        if start < current {
                            start
                        } else {
                            current
                        }
                    }
                });
                max_time = Some(match max_time {
                    None => end,
                    Some(current) => {
                        if end > current {
                            end
                        } else {
                            current
                        }
                    }
                });
            }
        }

        min_time.zip(max_time)
    }

    /// Filter all sensors by cloud cover
    ///
    /// # Errors
    /// Returns error if cloud cover threshold is invalid
    pub fn filter_by_cloud_cover(&mut self, max_cloud_cover: f32) -> Result<usize> {
        let mut total_removed = 0;
        for ts in self.sensors.values_mut() {
            total_removed += ts.filter_by_cloud_cover(max_cloud_cover)?;
        }
        Ok(total_removed)
    }

    /// Filter all sensors by quality
    ///
    /// # Errors
    /// Returns error if quality threshold is invalid
    pub fn filter_by_quality(&mut self, min_quality: f32) -> Result<usize> {
        let mut total_removed = 0;
        for ts in self.sensors.values_mut() {
            total_removed += ts.filter_by_quality(min_quality)?;
        }
        Ok(total_removed)
    }
}

impl Default for TimeSeriesCollection {
    fn default() -> Self {
        Self::new()
    }
}

/// Utilities for loading time series from file paths
pub struct TimeSeriesLoader;

impl TimeSeriesLoader {
    /// Load time series from a list of file paths with metadata extractor
    ///
    /// # Errors
    /// Returns error if metadata extraction fails
    pub fn from_paths<F, P>(paths: &[P], metadata_extractor: F) -> Result<TimeSeriesRaster>
    where
        F: Fn(&Path) -> Result<TemporalMetadata>,
        P: AsRef<Path>,
    {
        let mut ts = TimeSeriesRaster::new();

        for path in paths {
            let path_ref = path.as_ref();
            let metadata = metadata_extractor(path_ref)?;
            let path_string = path_ref
                .to_str()
                .ok_or_else(|| TemporalError::invalid_input("Invalid path"))?
                .to_string();
            ts.add_raster_lazy(metadata, path_string)?;
        }

        info!("Loaded {} rasters from paths", ts.len());
        Ok(ts)
    }

    /// Load time series from directory with pattern matching
    ///
    /// # Errors
    /// Returns error if directory reading fails or metadata extraction fails
    pub fn from_directory<F>(
        dir: impl AsRef<Path>,
        pattern: &str,
        metadata_extractor: F,
    ) -> Result<TimeSeriesRaster>
    where
        F: Fn(&Path) -> Result<TemporalMetadata>,
    {
        let dir_path = dir.as_ref();
        if !dir_path.is_dir() {
            return Err(TemporalError::invalid_input(format!(
                "Not a directory: {:?}",
                dir_path
            )));
        }

        let mut paths = Vec::new();
        let entries = std::fs::read_dir(dir_path).map_err(|e| {
            TemporalError::invalid_input(format!("Failed to read directory: {}", e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                TemporalError::invalid_input(format!("Failed to read entry: {}", e))
            })?;
            let path = entry.path();

            if path.is_file() {
                if let Some(filename) = path.file_name() {
                    if let Some(name) = filename.to_str() {
                        if name.contains(pattern) {
                            paths.push(path);
                        }
                    }
                }
            }
        }

        paths.sort();
        Self::from_paths(&paths, metadata_extractor)
    }
}

/// Utilities for splitting and merging time series
pub struct TimeSeriesSplitter;

impl TimeSeriesSplitter {
    /// Split time series into chunks by time range
    ///
    /// # Errors
    /// Returns error if chunk_size is invalid
    pub fn split_by_time(ts: &TimeSeriesRaster, chunk_size: i64) -> Result<Vec<TimeSeriesRaster>> {
        if chunk_size <= 0 {
            return Err(TemporalError::invalid_parameter(
                "chunk_size",
                "must be positive",
            ));
        }

        let (start_time, end_time) = ts
            .time_range()
            .ok_or_else(|| TemporalError::insufficient_data("Empty time series"))?;

        let mut chunks = Vec::new();
        let mut current_start = start_time;

        while current_start < end_time {
            let current_end = DateTime::from_timestamp(
                (current_start.timestamp() + chunk_size).min(end_time.timestamp()),
                0,
            )
            .ok_or_else(|| TemporalError::datetime_parse_error("Invalid timestamp"))?;

            let chunk_entries = ts.query_range(&current_start, &current_end);

            if !chunk_entries.is_empty() {
                let mut chunk_ts = TimeSeriesRaster::new();
                for entry in chunk_entries {
                    if let Some(data) = &entry.data {
                        chunk_ts.add_raster(entry.metadata.clone(), data.clone())?;
                    } else if let Some(path) = &entry.source_path {
                        chunk_ts.add_raster_lazy(entry.metadata.clone(), path.clone())?;
                    }
                }
                chunks.push(chunk_ts);
            }

            current_start = current_end;
        }

        info!("Split time series into {} chunks", chunks.len());
        Ok(chunks)
    }

    /// Split time series into fixed number of parts
    ///
    /// # Errors
    /// Returns error if n_parts is invalid or time series is empty
    pub fn split_into_parts(
        ts: &TimeSeriesRaster,
        n_parts: usize,
    ) -> Result<Vec<TimeSeriesRaster>> {
        if n_parts == 0 {
            return Err(TemporalError::invalid_parameter(
                "n_parts",
                "must be greater than 0",
            ));
        }

        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let total_count = ts.len();
        let chunk_size = total_count.div_ceil(n_parts); // Ceiling division

        let mut chunks = Vec::new();
        let entries: Vec<&TemporalRasterEntry> = ts.entries().values().collect();

        for i in 0..n_parts {
            let start_idx = i * chunk_size;
            if start_idx >= total_count {
                break;
            }

            let end_idx = ((i + 1) * chunk_size).min(total_count);
            let mut chunk_ts = TimeSeriesRaster::new();

            for entry in &entries[start_idx..end_idx] {
                if let Some(data) = &entry.data {
                    chunk_ts.add_raster(entry.metadata.clone(), data.clone())?;
                } else if let Some(path) = &entry.source_path {
                    chunk_ts.add_raster_lazy(entry.metadata.clone(), path.clone())?;
                }
            }

            chunks.push(chunk_ts);
        }

        info!("Split time series into {} parts", chunks.len());
        Ok(chunks)
    }

    /// Merge multiple time series into one
    ///
    /// # Errors
    /// Returns error if time series have incompatible dimensions
    pub fn merge(time_series: Vec<TimeSeriesRaster>) -> Result<TimeSeriesRaster> {
        if time_series.is_empty() {
            return Ok(TimeSeriesRaster::new());
        }

        let mut merged = TimeSeriesRaster::new();

        for ts in time_series {
            for entry in ts.entries().values() {
                if let Some(data) = &entry.data {
                    merged.add_raster(entry.metadata.clone(), data.clone())?;
                } else if let Some(path) = &entry.source_path {
                    merged.add_raster_lazy(entry.metadata.clone(), path.clone())?;
                }
            }
        }

        info!("Merged into time series with {} rasters", merged.len());
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use scirs2_core::ndarray::Array3;

    #[test]
    fn test_collection_creation() {
        let collection = TimeSeriesCollection::new();
        assert_eq!(collection.sensors().len(), 0);
        assert_eq!(collection.total_rasters(), 0);
    }

    #[test]
    fn test_add_sensor() {
        let mut collection = TimeSeriesCollection::new();
        let ts = TimeSeriesRaster::new();
        collection.add_sensor("Sentinel-2", ts);

        assert_eq!(collection.sensors().len(), 1);
        assert!(collection.get_sensor("Sentinel-2").is_some());
    }

    #[test]
    fn test_merge_sensors() {
        let mut collection = TimeSeriesCollection::new();

        let mut ts1 = TimeSeriesRaster::new();
        let dt1 = DateTime::from_timestamp(1640995200, 0).expect("valid");
        let date1 = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid");
        let metadata1 = TemporalMetadata::new(dt1, date1).with_sensor("Sentinel-2");
        ts1.add_raster(metadata1, Array3::zeros((10, 10, 3)))
            .expect("should add");

        let mut ts2 = TimeSeriesRaster::new();
        let dt2 = DateTime::from_timestamp(1641081600, 0).expect("valid");
        let date2 = NaiveDate::from_ymd_opt(2022, 1, 2).expect("valid");
        let metadata2 = TemporalMetadata::new(dt2, date2).with_sensor("Landsat-8");
        ts2.add_raster(metadata2, Array3::zeros((10, 10, 3)))
            .expect("should add");

        collection.add_sensor("Sentinel-2", ts1);
        collection.add_sensor("Landsat-8", ts2);

        let merged = collection.merge_sensors().expect("should merge");
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_split_into_parts() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            ts.add_raster(metadata, Array3::zeros((5, 5, 1)))
                .expect("should add");
        }

        let parts = TimeSeriesSplitter::split_into_parts(&ts, 3).expect("should split");
        assert_eq!(parts.len(), 3);
        assert!(parts[0].len() >= 3);
    }
}
