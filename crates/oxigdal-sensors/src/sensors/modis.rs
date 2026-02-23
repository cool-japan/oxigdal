//! MODIS sensor definitions
//!
//! Comprehensive sensor characteristics for MODIS (Moderate Resolution Imaging Spectroradiometer)
//! on Terra and Aqua satellites with 36 spectral bands

use super::{Band, Sensor};

/// Create MODIS sensor definition
///
/// MODIS has 36 spectral bands designed for land, ocean, and atmosphere observations
/// - Bands 1-2: 250m resolution (Red, NIR)
/// - Bands 3-7: 500m resolution (Blue, Green, NIR, SWIR)
/// - Bands 8-36: 1000m resolution (Various applications)
pub fn modis() -> Sensor {
    Sensor::new("MODIS", "Terra/Aqua", "Optical")
        .with_temporal_resolution(1.0) // Daily coverage (combined Terra + Aqua)
        .with_swath_width(2330.0)
        // 250m bands
        // Band 1 - Red (250m)
        .add_band(
            Band::new("B1", 1, 0.645, 0.050, 250.0)
                .with_common_name("Red")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1548.0),
        )
        // Band 2 - NIR (250m)
        .add_band(
            Band::new("B2", 2, 0.858, 0.035, 250.0)
                .with_common_name("NIR")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1028.0),
        )
        // 500m bands
        // Band 3 - Blue (500m)
        .add_band(
            Band::new("B3", 3, 0.469, 0.020, 500.0)
                .with_common_name("Blue")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(2023.0),
        )
        // Band 4 - Green (500m)
        .add_band(
            Band::new("B4", 4, 0.555, 0.020, 500.0)
                .with_common_name("Green")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1851.0),
        )
        // Band 5 - NIR2 (500m)
        .add_band(
            Band::new("B5", 5, 1.240, 0.020, 500.0)
                .with_common_name("NIR2")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(467.7),
        )
        // Band 6 - SWIR1 (500m)
        .add_band(
            Band::new("B6", 6, 1.640, 0.024, 500.0)
                .with_common_name("SWIR1")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(248.1),
        )
        // Band 7 - SWIR2 (500m)
        .add_band(
            Band::new("B7", 7, 2.130, 0.050, 500.0)
                .with_common_name("SWIR2")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(86.63),
        )
        // 1000m ocean bands
        // Band 8 (1000m)
        .add_band(
            Band::new("B8", 8, 0.412, 0.015, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(2094.0),
        )
        // Band 9 (1000m)
        .add_band(
            Band::new("B9", 9, 0.443, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(2006.0),
        )
        // Band 10 (1000m)
        .add_band(
            Band::new("B10", 10, 0.488, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1956.0),
        )
        // Band 11 (1000m)
        .add_band(
            Band::new("B11", 11, 0.531, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1809.0),
        )
        // Band 12 (1000m)
        .add_band(
            Band::new("B12", 12, 0.551, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1756.0),
        )
        // Band 13 (1000m)
        .add_band(
            Band::new("B13", 13, 0.667, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1533.0),
        )
        // Band 14 (1000m)
        .add_band(
            Band::new("B14", 14, 0.678, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1498.0),
        )
        // Band 15 (1000m)
        .add_band(
            Band::new("B15", 15, 0.748, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1280.0),
        )
        // Band 16 (1000m)
        .add_band(
            Band::new("B16", 16, 0.869, 0.015, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1004.0),
        )
        // Atmospheric bands
        // Band 17 (1000m)
        .add_band(
            Band::new("B17", 17, 0.905, 0.030, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(955.0),
        )
        // Band 18 (1000m)
        .add_band(
            Band::new("B18", 18, 0.936, 0.010, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(908.0),
        )
        // Band 19 (1000m)
        .add_band(
            Band::new("B19", 19, 0.940, 0.050, 1000.0)
                .with_radiometric_resolution(12)
                .with_solar_irradiance(897.0),
        )
        // Band 20 (1000m) - Thermal IR
        .add_band(Band::new("B20", 20, 3.750, 0.180, 1000.0).with_radiometric_resolution(12))
        // Band 21 (1000m) - Thermal IR
        .add_band(Band::new("B21", 21, 3.959, 0.060, 1000.0).with_radiometric_resolution(12))
        // Band 22 (1000m) - Thermal IR
        .add_band(Band::new("B22", 22, 3.959, 0.060, 1000.0).with_radiometric_resolution(12))
        // Band 23 (1000m) - Thermal IR
        .add_band(Band::new("B23", 23, 4.050, 0.060, 1000.0).with_radiometric_resolution(12))
        // Band 24 (1000m) - Thermal IR
        .add_band(Band::new("B24", 24, 4.465, 0.067, 1000.0).with_radiometric_resolution(12))
        // Band 25 (1000m) - Thermal IR
        .add_band(Band::new("B25", 25, 4.515, 0.067, 1000.0).with_radiometric_resolution(12))
        // Band 26 (1000m) - Cirrus
        .add_band(
            Band::new("B26", 26, 1.375, 0.030, 1000.0)
                .with_common_name("Cirrus")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(366.0),
        )
        // Band 27 (1000m) - Water Vapor
        .add_band(Band::new("B27", 27, 6.715, 0.360, 1000.0).with_radiometric_resolution(12))
        // Band 28 (1000m) - Thermal IR
        .add_band(Band::new("B28", 28, 7.325, 0.300, 1000.0).with_radiometric_resolution(12))
        // Band 29 (1000m) - Thermal IR
        .add_band(Band::new("B29", 29, 8.550, 0.300, 1000.0).with_radiometric_resolution(12))
        // Band 30 (1000m) - Thermal IR
        .add_band(Band::new("B30", 30, 9.730, 0.300, 1000.0).with_radiometric_resolution(12))
        // Band 31 (1000m) - Thermal IR
        .add_band(
            Band::new("B31", 31, 11.030, 0.500, 1000.0)
                .with_common_name("TIR1")
                .with_radiometric_resolution(12),
        )
        // Band 32 (1000m) - Thermal IR
        .add_band(
            Band::new("B32", 32, 12.020, 0.500, 1000.0)
                .with_common_name("TIR2")
                .with_radiometric_resolution(12),
        )
        // Band 33 (1000m) - Thermal IR
        .add_band(Band::new("B33", 33, 13.335, 0.300, 1000.0).with_radiometric_resolution(12))
        // Band 34 (1000m) - Thermal IR
        .add_band(Band::new("B34", 34, 13.635, 0.300, 1000.0).with_radiometric_resolution(12))
        // Band 35 (1000m) - Thermal IR
        .add_band(Band::new("B35", 35, 13.935, 0.300, 1000.0).with_radiometric_resolution(12))
        // Band 36 (1000m) - Thermal IR
        .add_band(Band::new("B36", 36, 14.235, 0.300, 1000.0).with_radiometric_resolution(12))
}

/// Get MODIS sensor
pub fn get_modis_sensor(platform: &str) -> Option<Sensor> {
    match platform.to_lowercase().as_str() {
        "modis" | "terra" | "aqua" | "terra-modis" | "aqua-modis" => Some(modis()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modis() {
        let sensor = modis();
        assert_eq!(sensor.name, "MODIS");
        assert_eq!(sensor.platform, "Terra/Aqua");
        assert_eq!(sensor.bands.len(), 36);

        // Check 250m bands
        let red = sensor.get_band_by_common_name("Red");
        assert!(red.is_some());
        if let Some(red) = red {
            assert_eq!(red.spatial_resolution, 250.0);
        }

        let nir = sensor.get_band_by_common_name("NIR");
        assert!(nir.is_some());
        if let Some(nir) = nir {
            assert_eq!(nir.spatial_resolution, 250.0);
        }

        // Check 500m bands
        let blue = sensor.get_band_by_common_name("Blue");
        assert!(blue.is_some());
        if let Some(blue) = blue {
            assert_eq!(blue.spatial_resolution, 500.0);
        }

        // Check temporal resolution
        assert_eq!(sensor.temporal_resolution, Some(1.0));
    }

    #[test]
    fn test_get_modis_sensor() {
        assert!(get_modis_sensor("modis").is_some());
        assert!(get_modis_sensor("terra").is_some());
        assert!(get_modis_sensor("aqua").is_some());
        assert!(get_modis_sensor("unknown").is_none());
    }
}
