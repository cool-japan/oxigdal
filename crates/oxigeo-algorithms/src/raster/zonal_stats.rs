//! Zonal statistics computation

use crate::error::{AlgorithmError, Result};
use oxigeo_core::buffer::RasterBuffer;

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
            let zone_raw = zones.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let value = values.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            // Skip pixels whose zone id is NoData/NaN. Casting a NaN directly
            // `as i32` saturates to 0 in Rust, which would silently bucket every
            // out-of-zone pixel into a spurious zone 0 and contaminate its stats.
            if zones.is_nodata(zone_raw) || !zone_raw.is_finite() {
                continue;
            }
            // Skip NoData/non-finite values so they do not poison sum/mean/stddev.
            if values.is_nodata(value) || !value.is_finite() {
                continue;
            }
            let zone_id = zone_raw as i32;

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

    // Second pass: compute standard deviation (same NoData filtering as pass 1
    // so the variance accumulator matches the mean/count computed above).
    for y in 0..values.height() {
        for x in 0..values.width() {
            let zone_raw = zones.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let value = values.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            if zones.is_nodata(zone_raw) || !zone_raw.is_finite() {
                continue;
            }
            if values.is_nodata(value) || !value.is_finite() {
                continue;
            }
            let zone_id = zone_raw as i32;

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
    use oxigeo_core::types::RasterDataType;

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

    #[test]
    fn test_zonal_stats_excludes_nodata_values() {
        use oxigeo_core::types::NoDataValue;
        // Value raster: zone 1 everywhere, all values 4.0 except one NaN NoData.
        let mut values = RasterBuffer::nodata_filled(
            4,
            4,
            RasterDataType::Float64,
            NoDataValue::Float(f64::NAN),
        );
        let mut zones = RasterBuffer::zeros(4, 4, RasterDataType::Float64);
        for y in 0..4 {
            for x in 0..4 {
                values
                    .set_pixel(x, y, 4.0)
                    .expect("set value pixel should succeed in test");
                zones
                    .set_pixel(x, y, 1.0)
                    .expect("set zone pixel should succeed in test");
            }
        }
        values
            .set_pixel(0, 0, f64::NAN)
            .expect("set NoData value should succeed in test");

        let result =
            compute_zonal_stats(&values, &zones).expect("zonal stats should succeed in test");
        assert_eq!(result.len(), 1);
        let zone = result[0];
        assert_eq!(zone.zone_id, 1);
        // 16 cells - 1 NoData = 15 valid cells, all 4.0.
        assert_eq!(zone.count, 15);
        assert!(zone.sum.is_finite(), "sum must not be NaN-poisoned");
        assert!((zone.sum - 60.0).abs() < 1e-9);
        assert!((zone.mean - 4.0).abs() < 1e-9);
        assert!((zone.std_dev - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_zonal_stats_skips_nodata_zone() {
        use oxigeo_core::types::NoDataValue;
        // Zone raster with a NoData sentinel; those pixels must NOT collapse into
        // a spurious zone 0.
        let mut values = RasterBuffer::zeros(3, 3, RasterDataType::Float64);
        let mut zones = RasterBuffer::nodata_filled(
            3,
            3,
            RasterDataType::Float64,
            NoDataValue::Float(f64::NAN),
        );
        for y in 0..3 {
            for x in 0..3 {
                values
                    .set_pixel(x, y, 10.0)
                    .expect("set value pixel should succeed in test");
                zones
                    .set_pixel(x, y, 5.0)
                    .expect("set zone pixel should succeed in test");
            }
        }
        // Mark one zone pixel as NoData (NaN).
        zones
            .set_pixel(1, 1, f64::NAN)
            .expect("set NoData zone should succeed in test");

        let result =
            compute_zonal_stats(&values, &zones).expect("zonal stats should succeed in test");
        // Only zone 5 should exist -- no spurious zone 0 from the NaN cast.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].zone_id, 5);
        assert_eq!(result[0].count, 8);
    }
}
