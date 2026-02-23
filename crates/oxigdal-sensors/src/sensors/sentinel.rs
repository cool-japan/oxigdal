//! Sentinel sensor definitions
//!
//! Comprehensive sensor characteristics for Sentinel missions:
//! - Sentinel-2A/B MultiSpectral Instrument (MSI)
//! - Sentinel-1A/B Synthetic Aperture Radar (SAR)

use super::{Band, Sensor};

/// Create Sentinel-2 MSI sensor definition
///
/// Sentinel-2 carries the MultiSpectral Instrument (MSI) with 13 bands
/// from visible to SWIR wavelengths at 10m, 20m, and 60m spatial resolution
pub fn sentinel2_msi() -> Sensor {
    Sensor::new("MSI", "Sentinel-2", "Optical")
        .with_temporal_resolution(5.0) // Combined S2A + S2B
        .with_swath_width(290.0)
        // Band 1 - Coastal Aerosol (60m)
        .add_band(
            Band::new("B1", 1, 0.443, 0.027, 60.0)
                .with_common_name("Coastal")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1913.57),
        )
        // Band 2 - Blue (10m)
        .add_band(
            Band::new("B2", 2, 0.490, 0.098, 10.0)
                .with_common_name("Blue")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1941.63),
        )
        // Band 3 - Green (10m)
        .add_band(
            Band::new("B3", 3, 0.560, 0.045, 10.0)
                .with_common_name("Green")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1822.61),
        )
        // Band 4 - Red (10m)
        .add_band(
            Band::new("B4", 4, 0.665, 0.038, 10.0)
                .with_common_name("Red")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1512.79),
        )
        // Band 5 - Red Edge 1 (20m)
        .add_band(
            Band::new("B5", 5, 0.705, 0.019, 20.0)
                .with_common_name("RedEdge1")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1425.56),
        )
        // Band 6 - Red Edge 2 (20m)
        .add_band(
            Band::new("B6", 6, 0.740, 0.018, 20.0)
                .with_common_name("RedEdge2")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1288.32),
        )
        // Band 7 - Red Edge 3 (20m)
        .add_band(
            Band::new("B7", 7, 0.783, 0.028, 20.0)
                .with_common_name("RedEdge3")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1163.19),
        )
        // Band 8 - NIR (10m)
        .add_band(
            Band::new("B8", 8, 0.842, 0.145, 10.0)
                .with_common_name("NIR")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(1036.39),
        )
        // Band 8A - Narrow NIR (20m)
        .add_band(
            Band::new("B8A", 9, 0.865, 0.033, 20.0)
                .with_common_name("NIR2")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(955.19),
        )
        // Band 9 - Water Vapor (60m)
        .add_band(
            Band::new("B9", 10, 0.945, 0.026, 60.0)
                .with_common_name("WaterVapor")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(813.04),
        )
        // Band 10 - Cirrus (60m)
        .add_band(
            Band::new("B10", 11, 1.375, 0.075, 60.0)
                .with_common_name("Cirrus")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(367.15),
        )
        // Band 11 - SWIR1 (20m)
        .add_band(
            Band::new("B11", 12, 1.610, 0.143, 20.0)
                .with_common_name("SWIR1")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(245.59),
        )
        // Band 12 - SWIR2 (20m)
        .add_band(
            Band::new("B12", 13, 2.190, 0.242, 20.0)
                .with_common_name("SWIR2")
                .with_radiometric_resolution(12)
                .with_solar_irradiance(85.25),
        )
}

/// Create Sentinel-1 SAR sensor definition
///
/// Sentinel-1 carries a C-band Synthetic Aperture Radar (SAR)
/// Operating in four exclusive imaging modes with different resolutions
pub fn sentinel1_sar() -> Sensor {
    Sensor::new("C-SAR", "Sentinel-1", "SAR")
        .with_temporal_resolution(6.0) // Combined S1A + S1B (before S1B failure)
        .with_swath_width(250.0) // IW mode
        // VV Polarization
        .add_band(
            Band::new("VV", 1, 55.0, 0.0, 10.0) // 5.405 GHz, ~55mm wavelength
                .with_common_name("VV")
                .with_radiometric_resolution(16),
        )
        // VH Polarization
        .add_band(
            Band::new("VH", 2, 55.0, 0.0, 10.0)
                .with_common_name("VH")
                .with_radiometric_resolution(16),
        )
        // HH Polarization
        .add_band(
            Band::new("HH", 3, 55.0, 0.0, 10.0)
                .with_common_name("HH")
                .with_radiometric_resolution(16),
        )
        // HV Polarization
        .add_band(
            Band::new("HV", 4, 55.0, 0.0, 10.0)
                .with_common_name("HV")
                .with_radiometric_resolution(16),
        )
}

/// Get Sentinel sensor by platform and instrument
pub fn get_sentinel_sensor(platform: &str) -> Option<Sensor> {
    match platform.to_lowercase().as_str() {
        "sentinel2" | "sentinel-2" | "s2" | "sentinel2a" | "sentinel2b" => Some(sentinel2_msi()),
        "sentinel1" | "sentinel-1" | "s1" | "sentinel1a" | "sentinel1b" => Some(sentinel1_sar()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentinel2_msi() {
        let sensor = sentinel2_msi();
        assert_eq!(sensor.name, "MSI");
        assert_eq!(sensor.platform, "Sentinel-2");
        assert_eq!(sensor.bands.len(), 13);

        // Check 10m bands
        let red = sensor.get_band_by_common_name("Red");
        assert!(red.is_some());
        if let Some(red) = red {
            assert_eq!(red.spatial_resolution, 10.0);
        }

        let nir = sensor.get_band_by_common_name("NIR");
        assert!(nir.is_some());
        if let Some(nir) = nir {
            assert_eq!(nir.spatial_resolution, 10.0);
        }

        // Check red edge bands (unique to Sentinel-2)
        let re1 = sensor.get_band_by_common_name("RedEdge1");
        assert!(re1.is_some());
        if let Some(re1) = re1 {
            assert_eq!(re1.spatial_resolution, 20.0);
        }

        // Check SWIR bands
        let swir1 = sensor.get_band_by_common_name("SWIR1");
        assert!(swir1.is_some());
        if let Some(swir1) = swir1 {
            assert_eq!(swir1.spatial_resolution, 20.0);
        }
    }

    #[test]
    fn test_sentinel1_sar() {
        let sensor = sentinel1_sar();
        assert_eq!(sensor.name, "C-SAR");
        assert_eq!(sensor.platform, "Sentinel-1");
        assert_eq!(sensor.sensor_type, "SAR");
        assert_eq!(sensor.bands.len(), 4);

        // Check polarizations
        let vv = sensor.get_band_by_common_name("VV");
        assert!(vv.is_some());

        let vh = sensor.get_band_by_common_name("VH");
        assert!(vh.is_some());
    }

    #[test]
    fn test_get_sentinel_sensor() {
        assert!(get_sentinel_sensor("sentinel2").is_some());
        assert!(get_sentinel_sensor("S2").is_some());
        assert!(get_sentinel_sensor("sentinel1").is_some());
        assert!(get_sentinel_sensor("unknown").is_none());
    }

    #[test]
    fn test_sentinel2_radiometric_resolution() {
        let sensor = sentinel2_msi();
        for band in &sensor.bands {
            assert_eq!(band.radiometric_resolution, 12);
        }
    }
}
