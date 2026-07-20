//! Sensor definitions and characteristics
//!
//! This module provides comprehensive sensor definitions for various satellite platforms,
//! including band characteristics, radiometric calibration parameters, and metadata.

pub mod aster;
pub mod landsat;
pub mod modis;
pub mod sentinel;

use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Represents a spectral band with its characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Band {
    /// Band name (e.g., "B1", "Red", "NIR")
    pub name: String,

    /// Band number
    pub number: usize,

    /// Common name (e.g., "Red", "NIR", "SWIR1")
    pub common_name: Option<String>,

    /// Center wavelength in micrometers
    pub center_wavelength: f64,

    /// Bandwidth (full width at half maximum) in micrometers
    pub bandwidth: f64,

    /// Spatial resolution in meters
    pub spatial_resolution: f64,

    /// Radiometric resolution in bits
    pub radiometric_resolution: u8,

    /// Gain (multiplicative) for DN to radiance conversion
    pub gain: Option<f64>,

    /// Bias (additive) for DN to radiance conversion
    pub bias: Option<f64>,

    /// Solar irradiance at top of atmosphere (W/m²/μm)
    pub solar_irradiance: Option<f64>,
}

impl Band {
    /// Create a new band with basic parameters
    pub fn new(
        name: impl Into<String>,
        number: usize,
        center_wavelength: f64,
        bandwidth: f64,
        spatial_resolution: f64,
    ) -> Self {
        Self {
            name: name.into(),
            number,
            common_name: None,
            center_wavelength,
            bandwidth,
            spatial_resolution,
            radiometric_resolution: 16,
            gain: None,
            bias: None,
            solar_irradiance: None,
        }
    }

    /// Set common name
    pub fn with_common_name(mut self, name: impl Into<String>) -> Self {
        self.common_name = Some(name.into());
        self
    }

    /// Set radiometric resolution
    pub fn with_radiometric_resolution(mut self, bits: u8) -> Self {
        self.radiometric_resolution = bits;
        self
    }

    /// Set gain and bias for calibration
    pub fn with_calibration(mut self, gain: f64, bias: f64) -> Self {
        self.gain = Some(gain);
        self.bias = Some(bias);
        self
    }

    /// Set solar irradiance
    pub fn with_solar_irradiance(mut self, irradiance: f64) -> Self {
        self.solar_irradiance = Some(irradiance);
        self
    }

    /// Convert DN (Digital Number) to radiance
    ///
    /// Returns error if gain/bias are not set
    pub fn dn_to_radiance(&self, dn: f64) -> Result<f64> {
        let gain = self
            .gain
            .ok_or_else(|| crate::error::SensorError::calibration_error("Gain not set for band"))?;
        let bias = self
            .bias
            .ok_or_else(|| crate::error::SensorError::calibration_error("Bias not set for band"))?;

        Ok(gain * dn + bias)
    }

    /// Convert radiance to TOA (Top of Atmosphere) reflectance
    ///
    /// # Parameters
    /// - `radiance`: At-sensor radiance (W/m²/sr/μm)
    /// - `solar_zenith`: Solar zenith angle in degrees
    /// - `earth_sun_distance`: Earth-Sun distance in AU
    ///
    /// Returns error if solar irradiance is not set
    pub fn radiance_to_reflectance(
        &self,
        radiance: f64,
        solar_zenith: f64,
        earth_sun_distance: f64,
    ) -> Result<f64> {
        let esun = self.solar_irradiance.ok_or_else(|| {
            crate::error::SensorError::calibration_error("Solar irradiance not set for band")
        })?;

        // Convert solar zenith to radians
        let sz_rad = solar_zenith.to_radians();

        // TOA reflectance formula
        let reflectance =
            (std::f64::consts::PI * radiance * earth_sun_distance * earth_sun_distance)
                / (esun * sz_rad.cos());

        Ok(reflectance)
    }
}

/// Represents a satellite sensor with its characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensor {
    /// Sensor name
    pub name: String,

    /// Platform/satellite name
    pub platform: String,

    /// Sensor type (e.g., "Optical", "SAR", "Thermal")
    pub sensor_type: String,

    /// List of bands
    pub bands: Vec<Band>,

    /// Temporal resolution in days
    pub temporal_resolution: Option<f64>,

    /// Swath width in kilometers
    pub swath_width: Option<f64>,
}

impl Sensor {
    /// Create a new sensor
    pub fn new(
        name: impl Into<String>,
        platform: impl Into<String>,
        sensor_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            platform: platform.into(),
            sensor_type: sensor_type.into(),
            bands: Vec::new(),
            temporal_resolution: None,
            swath_width: None,
        }
    }

    /// Add a band to the sensor
    pub fn add_band(mut self, band: Band) -> Self {
        self.bands.push(band);
        self
    }

    /// Set temporal resolution
    pub fn with_temporal_resolution(mut self, days: f64) -> Self {
        self.temporal_resolution = Some(days);
        self
    }

    /// Set swath width
    pub fn with_swath_width(mut self, km: f64) -> Self {
        self.swath_width = Some(km);
        self
    }

    /// Get a band by name
    pub fn get_band(&self, name: &str) -> Option<&Band> {
        self.bands.iter().find(|b| b.name == name)
    }

    /// Get a band by common name
    pub fn get_band_by_common_name(&self, common_name: &str) -> Option<&Band> {
        self.bands
            .iter()
            .find(|b| b.common_name.as_ref().is_some_and(|cn| cn == common_name))
    }

    /// Get a band by number
    pub fn get_band_by_number(&self, number: usize) -> Option<&Band> {
        self.bands.iter().find(|b| b.number == number)
    }

    /// Get all band names
    pub fn band_names(&self) -> Vec<&str> {
        self.bands.iter().map(|b| b.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_band_creation() {
        let band = Band::new("B4", 4, 0.665, 0.038, 30.0)
            .with_common_name("Red")
            .with_radiometric_resolution(12)
            .with_calibration(0.00002, 0.0)
            .with_solar_irradiance(1554.0);

        assert_eq!(band.name, "B4");
        assert_eq!(band.number, 4);
        assert_eq!(band.common_name, Some("Red".to_string()));
        assert_eq!(band.center_wavelength, 0.665);
        assert_eq!(band.radiometric_resolution, 12);
    }

    #[test]
    fn test_dn_to_radiance() {
        let band = Band::new("B4", 4, 0.665, 0.038, 30.0).with_calibration(0.00002, 0.0);

        let radiance = band.dn_to_radiance(1000.0);
        assert!(radiance.is_ok());
        if let Ok(rad) = radiance {
            assert!((rad - 0.02).abs() < 1e-6);
        }
    }

    #[test]
    fn test_sensor_creation() {
        let sensor = Sensor::new("OLI", "Landsat-8", "Optical")
            .with_temporal_resolution(16.0)
            .with_swath_width(185.0)
            .add_band(Band::new("B4", 4, 0.665, 0.038, 30.0).with_common_name("Red"));

        assert_eq!(sensor.name, "OLI");
        assert_eq!(sensor.platform, "Landsat-8");
        assert_eq!(sensor.bands.len(), 1);
        assert!(sensor.get_band("B4").is_some());
        assert!(sensor.get_band_by_common_name("Red").is_some());
    }
}
