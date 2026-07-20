//! ASTER sensor definitions
//!
//! Comprehensive sensor characteristics for ASTER (Advanced Spaceborne Thermal Emission and Reflection Radiometer)
//! on the Terra satellite with 14 bands across VNIR, SWIR, and TIR regions

use super::{Band, Sensor};

/// Create ASTER sensor definition
///
/// ASTER has 14 bands in three subsystems:
/// - VNIR (Visible and Near Infrared): 3 bands at 15m resolution
/// - SWIR (Short Wave Infrared): 6 bands at 30m resolution
/// - TIR (Thermal Infrared): 5 bands at 90m resolution
pub fn aster() -> Sensor {
    Sensor::new("ASTER", "Terra", "Optical")
        .with_temporal_resolution(16.0)
        .with_swath_width(60.0)
        // VNIR subsystem (15m)
        // Band 1 - Green (15m)
        .add_band(
            Band::new("B1", 1, 0.560, 0.080, 15.0)
                .with_common_name("Green")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1848.0),
        )
        // Band 2 - Red (15m)
        .add_band(
            Band::new("B2", 2, 0.660, 0.060, 15.0)
                .with_common_name("Red")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1549.0),
        )
        // Band 3N - NIR Nadir (15m)
        .add_band(
            Band::new("B3N", 3, 0.820, 0.080, 15.0)
                .with_common_name("NIR")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1114.0),
        )
        // Band 3B - NIR Backward (15m, for stereo)
        .add_band(
            Band::new("B3B", 4, 0.820, 0.080, 15.0)
                .with_common_name("NIR_Back")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(1114.0),
        )
        // SWIR subsystem (30m)
        // Band 4 - SWIR1 (30m)
        .add_band(
            Band::new("B4", 5, 1.650, 0.100, 30.0)
                .with_common_name("SWIR1")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(225.4),
        )
        // Band 5 - SWIR2 (30m)
        .add_band(
            Band::new("B5", 6, 2.165, 0.040, 30.0)
                .with_common_name("SWIR2")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(86.63),
        )
        // Band 6 - SWIR3 (30m)
        .add_band(
            Band::new("B6", 7, 2.205, 0.040, 30.0)
                .with_common_name("SWIR3")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(81.85),
        )
        // Band 7 - SWIR4 (30m)
        .add_band(
            Band::new("B7", 8, 2.260, 0.050, 30.0)
                .with_common_name("SWIR4")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(74.85),
        )
        // Band 8 - SWIR5 (30m)
        .add_band(
            Band::new("B8", 9, 2.330, 0.070, 30.0)
                .with_common_name("SWIR5")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(66.49),
        )
        // Band 9 - SWIR6 (30m)
        .add_band(
            Band::new("B9", 10, 2.395, 0.070, 30.0)
                .with_common_name("SWIR6")
                .with_radiometric_resolution(8)
                .with_solar_irradiance(59.85),
        )
        // TIR subsystem (90m)
        // Band 10 - TIR1 (90m)
        .add_band(
            Band::new("B10", 11, 8.300, 0.350, 90.0)
                .with_common_name("TIR1")
                .with_radiometric_resolution(12),
        )
        // Band 11 - TIR2 (90m)
        .add_band(
            Band::new("B11", 12, 8.650, 0.350, 90.0)
                .with_common_name("TIR2")
                .with_radiometric_resolution(12),
        )
        // Band 12 - TIR3 (90m)
        .add_band(
            Band::new("B12", 13, 9.100, 0.350, 90.0)
                .with_common_name("TIR3")
                .with_radiometric_resolution(12),
        )
        // Band 13 - TIR4 (90m)
        .add_band(
            Band::new("B13", 14, 10.600, 0.700, 90.0)
                .with_common_name("TIR4")
                .with_radiometric_resolution(12),
        )
        // Band 14 - TIR5 (90m)
        .add_band(
            Band::new("B14", 15, 11.300, 0.700, 90.0)
                .with_common_name("TIR5")
                .with_radiometric_resolution(12),
        )
}

/// Get ASTER sensor
pub fn get_aster_sensor(platform: &str) -> Option<Sensor> {
    match platform.to_lowercase().as_str() {
        "aster" | "terra-aster" => Some(aster()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aster() {
        let sensor = aster();
        assert_eq!(sensor.name, "ASTER");
        assert_eq!(sensor.platform, "Terra");
        assert_eq!(sensor.bands.len(), 15); // 3N, 3B counted separately

        // Check VNIR bands (15m)
        let red = sensor.get_band_by_common_name("Red");
        assert!(red.is_some());
        if let Some(red) = red {
            assert_eq!(red.spatial_resolution, 15.0);
        }

        let nir = sensor.get_band_by_common_name("NIR");
        assert!(nir.is_some());
        if let Some(nir) = nir {
            assert_eq!(nir.spatial_resolution, 15.0);
        }

        // Check SWIR bands (30m)
        let swir1 = sensor.get_band_by_common_name("SWIR1");
        assert!(swir1.is_some());
        if let Some(swir1) = swir1 {
            assert_eq!(swir1.spatial_resolution, 30.0);
        }

        // Check TIR bands (90m)
        let tir1 = sensor.get_band_by_common_name("TIR1");
        assert!(tir1.is_some());
        if let Some(tir1) = tir1 {
            assert_eq!(tir1.spatial_resolution, 90.0);
        }
    }

    #[test]
    fn test_get_aster_sensor() {
        assert!(get_aster_sensor("aster").is_some());
        assert!(get_aster_sensor("terra-aster").is_some());
        assert!(get_aster_sensor("unknown").is_none());
    }

    #[test]
    fn test_aster_swir_bands() {
        let sensor = aster();

        // ASTER has 6 SWIR bands (B4-B9)
        let swir_bands: Vec<_> = sensor
            .bands
            .iter()
            .filter(|b| {
                b.common_name
                    .as_ref()
                    .is_some_and(|cn| cn.starts_with("SWIR"))
            })
            .collect();

        assert_eq!(swir_bands.len(), 6);
    }

    #[test]
    fn test_aster_tir_bands() {
        let sensor = aster();

        // ASTER has 5 TIR bands (B10-B14)
        let tir_bands: Vec<_> = sensor
            .bands
            .iter()
            .filter(|b| {
                b.common_name
                    .as_ref()
                    .is_some_and(|cn| cn.starts_with("TIR"))
            })
            .collect();

        assert_eq!(tir_bands.len(), 5);
    }
}
