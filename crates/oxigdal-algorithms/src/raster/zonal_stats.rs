//! Zonal statistics computation

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

/// Statistics for a single zone
#[derive(Debug, Clone, Copy, Default)]
pub struct ZonalStatistics {
    /// Zone ID
    pub zone_id: i32,
    /// Count of valid pixels
    pub count: u64,
    /// Sum of values
    pub sum: f64,
    /// Mean value
    pub mean: f64,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Standard deviation
    pub std_dev: f64,
}

/// Computes zonal statistics
pub fn compute_zonal_stats(
    values: &RasterBuffer,
    zones: &RasterBuffer,
) -> Result<Vec<ZonalStatistics>> {
    if values.width() != zones.width() || values.height() != zones.height() {
        return Err(AlgorithmError::InvalidDimensions {
            message: "Rasters must have same dimensions",
            actual: values.width() as usize,
            expected: zones.width() as usize,
        });
    }

    let mut stats_map: std::collections::HashMap<i32, ZonalStatistics> =
        std::collections::HashMap::new();

    // First pass: collect sums and counts
    for y in 0..values.height() {
        for x in 0..values.width() {
            let zone_id = zones.get_pixel(x, y).map_err(AlgorithmError::Core)? as i32;
            let value = values.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            let stats = stats_map.entry(zone_id).or_insert_with(|| ZonalStatistics {
                zone_id,
                min: f64::MAX,
                max: f64::MIN,
                ..Default::default()
            });

            stats.count += 1;
            stats.sum += value;
            stats.min = stats.min.min(value);
            stats.max = stats.max.max(value);
        }
    }

    // Compute means
    for stats in stats_map.values_mut() {
        stats.mean = stats.sum / stats.count as f64;
    }

    // Second pass: compute standard deviation
    for y in 0..values.height() {
        for x in 0..values.width() {
            let zone_id = zones.get_pixel(x, y).map_err(AlgorithmError::Core)? as i32;
            let value = values.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            if let Some(stats) = stats_map.get_mut(&zone_id) {
                let diff = value - stats.mean;
                stats.std_dev += diff * diff;
            }
        }
    }

    // Finalize standard deviations
    for stats in stats_map.values_mut() {
        stats.std_dev = (stats.std_dev / stats.count as f64).sqrt();
    }

    Ok(stats_map.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_zonal_stats() {
        let mut values = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut zones = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Fill with test data
        for y in 0..5 {
            for x in 0..5 {
                values.set_pixel(x, y, (x + y) as f64).ok();
                zones.set_pixel(x, y, (x / 2) as f64).ok();
            }
        }

        let result = compute_zonal_stats(&values, &zones);
        assert!(result.is_ok());
    }
}
