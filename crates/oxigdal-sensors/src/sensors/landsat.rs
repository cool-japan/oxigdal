//! Landsat sensor definitions
//!
//! Comprehensive sensor characteristics for Landsat missions:
//! - Landsat 5 Thematic Mapper (TM)
//! - Landsat 7 Enhanced Thematic Mapper Plus (ETM+)
//! - Landsat 8/9 Operational Land Imager (OLI) and Thermal Infrared Sensor (TIRS)

use super::{Band, Sensor};

/// Create Landsat 5 TM sensor definition
///
/// Landsat 5 operated from 1984-2013 with the Thematic Mapper sensor
pub fn landsat5_tm() -> Sensor {
    Sensor::new("TM", "Landsat-5", "Optical")
        .with_temporal_resolution(16.0)
        .with_swath_width(185.0)
        // Band 1 - Blue (30m)
        .add_band(
            Band::new("B1", 1, 0.485, 0.070, 30.0)
                .with_common_name("Blue")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1957.0),
        )
        // Band 2 - Green (30m)
        .add_band(
            Band::new("B2", 2, 0.560, 0.080, 30.0)
                .with_common_name("Green")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1826.0),
        )
        // Band 3 - Red (30m)
        .add_band(
            Band::new("B3", 3, 0.660, 0.060, 30.0)
                .with_common_name("Red")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1554.0),
        )
        // Band 4 - NIR (30m)
        .add_band(
            Band::new("B4", 4, 0.830, 0.140, 30.0)
                .with_common_name("NIR")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1036.0),
        )
        // Band 5 - SWIR1 (30m)
        .add_band(
            Band::new("B5", 5, 1.650, 0.200, 30.0)
                .with_common_name("SWIR1")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(215.0),
        )
        // Band 6 - Thermal (120m -> 30m resampled)
        .add_band(
            Band::new("B6", 6, 11.450, 2.100, 120.0)
                .with_common_name("TIR")
                .with_radiometric_resolution(8),
        )
        // Band 7 - SWIR2 (30m)
        .add_band(
            Band::new("B7", 7, 2.215, 0.270, 30.0)
                .with_common_name("SWIR2")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(80.67),
        )
}

/// Create Landsat 7 ETM+ sensor definition
///
/// Landsat 7 launched in 1999, includes panchromatic band
/// Note: SLC failure occurred in 2003 causing scan line gaps
pub fn landsat7_etm_plus() -> Sensor {
    Sensor::new("ETM+", "Landsat-7", "Optical")
        .with_temporal_resolution(16.0)
        .with_swath_width(185.0)
        // Band 1 - Blue (30m)
        .add_band(
            Band::new("B1", 1, 0.485, 0.070, 30.0)
                .with_common_name("Blue")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1970.0),
        )
        // Band 2 - Green (30m)
        .add_band(
            Band::new("B2", 2, 0.560, 0.080, 30.0)
                .with_common_name("Green")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1842.0),
        )
        // Band 3 - Red (30m)
        .add_band(
            Band::new("B3", 3, 0.660, 0.060, 30.0)
                .with_common_name("Red")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1547.0),
        )
        // Band 4 - NIR (30m)
        .add_band(
            Band::new("B4", 4, 0.835, 0.130, 30.0)
                .with_common_name("NIR")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1044.0),
        )
        // Band 5 - SWIR1 (30m)
        .add_band(
            Band::new("B5", 5, 1.650, 0.200, 30.0)
                .with_common_name("SWIR1")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(225.7),
        )
        // Band 6 - Thermal (60m)
        .add_band(
            Band::new("B6", 6, 11.450, 2.100, 60.0)
                .with_common_name("TIR")
                .with_radiometric_resolution(8),
        )
        // Band 7 - SWIR2 (30m)
        .add_band(
            Band::new("B7", 7, 2.220, 0.260, 30.0)
                .with_common_name("SWIR2")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(82.06),
        )
        // Band 8 - Panchromatic (15m)
        .add_band(
            Band::new("B8", 8, 0.710, 0.380, 15.0)
                .with_common_name("Pan")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1369.0),
        )
}

/// Create Landsat 8 OLI/TIRS sensor definition
///
/// Landsat 8 launched in 2013 with improved radiometric resolution (12-bit)
/// and additional bands for coastal/aerosol and cirrus detection
pub fn landsat8_oli_tirs() -> Sensor {
    Sensor::new("OLI/TIRS", "Landsat-8", "Optical")
        .with_temporal_resolution(16.0)
        .with_swath_width(185.0)
        // Band 1 - Coastal Aerosol (30m)
        .add_band(
            Band::new("B1", 1, 0.443, 0.020, 30.0)
                .with_common_name("Coastal")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(2067.0),
        )
        // Band 2 - Blue (30m)
        .add_band(
            Band::new("B2", 2, 0.482, 0.065, 30.0)
                .with_common_name("Blue")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(2067.0),
        )
        // Band 3 - Green (30m)
        .add_band(
            Band::new("B3", 3, 0.562, 0.080, 30.0)
                .with_common_name("Green")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1893.0),
        )
        // Band 4 - Red (30m)
        .add_band(
            Band::new("B4", 4, 0.655, 0.038, 30.0)
                .with_common_name("Red")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1603.0),
        )
        // Band 5 - NIR (30m)
        .add_band(
            Band::new("B5", 5, 0.865, 0.030, 30.0)
                .with_common_name("NIR")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(972.6),
        )
        // Band 6 - SWIR1 (30m)
        .add_band(
            Band::new("B6", 6, 1.610, 0.090, 30.0)
                .with_common_name("SWIR1")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(245.0),
        )
        // Band 7 - SWIR2 (30m)
        .add_band(
            Band::new("B7", 7, 2.200, 0.190, 30.0)
                .with_common_name("SWIR2")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(79.72),
        )
        // Band 8 - Panchromatic (15m)
        .add_band(
            Band::new("B8", 8, 0.590, 0.180, 15.0)
                .with_common_name("Pan")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1723.0),
        )
        // Band 9 - Cirrus (30m)
        .add_band(
            Band::new("B9", 9, 1.375, 0.030, 30.0)
                .with_common_name("Cirrus")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(366.97),
        )
        // Band 10 - TIRS1 (100m)
        .add_band(
            Band::new("B10", 10, 10.895, 0.590, 100.0)
                .with_common_name("TIR1")
                .with_radiometric_resolution(12),
        )
        // Band 11 - TIRS2 (100m)
        .add_band(
            Band::new("B11", 11, 12.005, 1.010, 100.0)
                .with_common_name("TIR2")
                .with_radiometric_resolution(12),
        )
}

/// Create Landsat 9 OLI-2/TIRS-2 sensor definition
///
/// Landsat 9 launched in 2021, nearly identical to Landsat 8 with minor improvements
pub fn landsat9_oli_tirs() -> Sensor {
    // Landsat 9 has identical band configuration to Landsat 8
    let mut sensor = landsat8_oli_tirs();
    sensor.platform = "Landsat-9".to_string();
    sensor.name = "OLI-2/TIRS-2".to_string();
    sensor
}

/// Get Landsat sensor by platform name
pub fn get_landsat_sensor(platform: &str) -> Option<Sensor> {
    match platform.to_lowercase().as_str() {
        "landsat5" | "landsat-5" | "l5" => Some(landsat5_tm()),
        "landsat7" | "landsat-7" | "l7" => Some(landsat7_etm_plus()),
        "landsat8" | "landsat-8" | "l8" => Some(landsat8_oli_tirs()),
        "landsat9" | "landsat-9" | "l9" => Some(landsat9_oli_tirs()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_landsat5_tm() {
        let sensor = landsat5_tm();
        assert_eq!(sensor.name, "TM");
        assert_eq!(sensor.platform, "Landsat-5");
        assert_eq!(sensor.bands.len(), 7);

        let nir = sensor.get_band_by_common_name("NIR");
        assert!(nir.is_some());
        if let Some(nir) = nir {
            assert_eq!(nir.name, "B4");
            assert_eq!(nir.spatial_resolution, 30.0);
        }
    }

    #[test]
    fn test_landsat7_etm_plus() {
        let sensor = landsat7_etm_plus();
        assert_eq!(sensor.name, "ETM+");
        assert_eq!(sensor.platform, "Landsat-7");
        assert_eq!(sensor.bands.len(), 8);

        let pan = sensor.get_band_by_common_name("Pan");
        assert!(pan.is_some());
        if let Some(pan) = pan {
            assert_eq!(pan.spatial_resolution, 15.0);
        }
    }

    #[test]
    fn test_landsat8_oli_tirs() {
        let sensor = landsat8_oli_tirs();
        assert_eq!(sensor.name, "OLI/TIRS");
        assert_eq!(sensor.platform, "Landsat-8");
        assert_eq!(sensor.bands.len(), 11);

        let coastal = sensor.get_band_by_common_name("Coastal");
        assert!(coastal.is_some());

        let cirrus = sensor.get_band_by_common_name("Cirrus");
        assert!(cirrus.is_some());

        let red = sensor.get_band_by_common_name("Red");
        assert!(red.is_some());
        if let Some(red) = red {
            assert_eq!(red.radiometric_resolution, 12);
        }
    }

    #[test]
    fn test_get_landsat_sensor() {
        assert!(get_landsat_sensor("landsat8").is_some());
        assert!(get_landsat_sensor("L8").is_some());
        assert!(get_landsat_sensor("unknown").is_none());
    }
}
