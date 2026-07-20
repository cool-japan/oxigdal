//! Electro-Optical (EO) Extension for STAC.
//!
//! This extension provides metadata for electro-optical data, particularly
//! satellite imagery with spectral bands.

use crate::error::{Result, StacError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema URI for the EO extension.
pub const SCHEMA_URI: &str = "https://stac-extensions.github.io/eo/v1.1.0/schema.json";

/// Electro-Optical extension data for a STAC Item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EoExtension {
    /// Bands in the asset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bands: Option<Vec<Band>>,

    /// Cloud cover percentage (0-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_cover: Option<f64>,
}

/// A spectral band.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Band {
    /// Name of the band (e.g., "B01", "red", "nir").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Common name of the band (e.g., "red", "green", "blue", "nir").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_name: Option<CommonBandName>,

    /// Description of the band.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Center wavelength in micrometers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_wavelength: Option<f64>,

    /// Full width at half maximum in micrometers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_width_half_max: Option<f64>,

    /// Solar illumination in W/m²/μm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solar_illumination: Option<f64>,
}

/// Common band names as defined in the EO extension.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommonBandName {
    /// Coastal aerosol band.
    Coastal,
    /// Blue band.
    Blue,
    /// Green band.
    Green,
    /// Red band.
    Red,
    /// Yellow band.
    Yellow,
    /// Pan band (panchromatic).
    Pan,
    /// Red edge band.
    Rededge,
    /// Near infrared band.
    Nir,
    /// Near infrared 08 band.
    Nir08,
    /// Near infrared 09 band.
    Nir09,
    /// Cirrus band.
    Cirrus,
    /// Short-wave infrared 16 band.
    Swir16,
    /// Short-wave infrared 22 band.
    Swir22,
    /// Long-wave infrared 11 band.
    Lwir11,
    /// Long-wave infrared 12 band.
    Lwir12,
}

impl EoExtension {
    /// Creates a new EO extension.
    ///
    /// # Returns
    ///
    /// A new EO extension instance
    pub fn new() -> Self {
        Self {
            bands: None,
            cloud_cover: None,
        }
    }

    /// Sets the bands.
    ///
    /// # Arguments
    ///
    /// * `bands` - Vector of bands
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_bands(mut self, bands: Vec<Band>) -> Self {
        self.bands = Some(bands);
        self
    }

    /// Adds a band.
    ///
    /// # Arguments
    ///
    /// * `band` - Band to add
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_band(mut self, band: Band) -> Self {
        match &mut self.bands {
            Some(bands) => bands.push(band),
            None => self.bands = Some(vec![band]),
        }
        self
    }

    /// Sets the cloud cover percentage.
    ///
    /// # Arguments
    ///
    /// * `cloud_cover` - Cloud cover percentage (0-100)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_cloud_cover(mut self, cloud_cover: f64) -> Self {
        self.cloud_cover = Some(cloud_cover);
        self
    }

    /// Validates the EO extension data.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        // Validate cloud cover
        if let Some(cc) = self.cloud_cover {
            if !(0.0..=100.0).contains(&cc) {
                return Err(StacError::InvalidExtension {
                    extension: "eo".to_string(),
                    reason: format!("cloud_cover must be between 0 and 100, found {}", cc),
                });
            }
        }

        // Validate bands
        if let Some(bands) = &self.bands {
            for (i, band) in bands.iter().enumerate() {
                band.validate().map_err(|e| StacError::InvalidExtension {
                    extension: format!("eo.bands[{}]", i),
                    reason: e.to_string(),
                })?;
            }
        }

        Ok(())
    }

    /// Converts the EO extension to a JSON value.
    ///
    /// # Returns
    ///
    /// JSON value representation
    pub fn to_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(|e| StacError::Serialization(e.to_string()))
    }

    /// Creates an EO extension from a JSON value.
    ///
    /// # Arguments
    ///
    /// * `value` - JSON value
    ///
    /// # Returns
    ///
    /// EO extension instance
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| StacError::Deserialization(e.to_string()))
    }
}

impl Default for EoExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl Band {
    /// Creates a new band.
    ///
    /// # Returns
    ///
    /// A new band instance
    pub fn new() -> Self {
        Self {
            name: None,
            common_name: None,
            description: None,
            center_wavelength: None,
            full_width_half_max: None,
            solar_illumination: None,
        }
    }

    /// Sets the name of the band.
    ///
    /// # Arguments
    ///
    /// * `name` - Band name
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the common name of the band.
    ///
    /// # Arguments
    ///
    /// * `common_name` - Common band name
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_common_name(mut self, common_name: CommonBandName) -> Self {
        self.common_name = Some(common_name);
        self
    }

    /// Sets the description of the band.
    ///
    /// # Arguments
    ///
    /// * `description` - Band description
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the center wavelength in micrometers.
    ///
    /// # Arguments
    ///
    /// * `wavelength` - Center wavelength
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_center_wavelength(mut self, wavelength: f64) -> Self {
        self.center_wavelength = Some(wavelength);
        self
    }

    /// Sets the full width at half maximum in micrometers.
    ///
    /// # Arguments
    ///
    /// * `fwhm` - Full width at half maximum
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_full_width_half_max(mut self, fwhm: f64) -> Self {
        self.full_width_half_max = Some(fwhm);
        self
    }

    /// Sets the solar illumination in W/m²/μm.
    ///
    /// # Arguments
    ///
    /// * `illumination` - Solar illumination
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_solar_illumination(mut self, illumination: f64) -> Self {
        self.solar_illumination = Some(illumination);
        self
    }

    /// Validates the band.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        // Validate center wavelength
        if let Some(wl) = self.center_wavelength {
            if wl <= 0.0 {
                return Err(StacError::InvalidFieldValue {
                    field: "center_wavelength".to_string(),
                    reason: "must be positive".to_string(),
                });
            }
        }

        // Validate full width at half maximum
        if let Some(fwhm) = self.full_width_half_max {
            if fwhm <= 0.0 {
                return Err(StacError::InvalidFieldValue {
                    field: "full_width_half_max".to_string(),
                    reason: "must be positive".to_string(),
                });
            }
        }

        // Validate solar illumination
        if let Some(si) = self.solar_illumination {
            if si < 0.0 {
                return Err(StacError::InvalidFieldValue {
                    field: "solar_illumination".to_string(),
                    reason: "must be non-negative".to_string(),
                });
            }
        }

        Ok(())
    }
}

impl Default for Band {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eo_extension_new() {
        let eo = EoExtension::new();
        assert!(eo.bands.is_none());
        assert!(eo.cloud_cover.is_none());
    }

    #[test]
    fn test_eo_extension_with_cloud_cover() {
        let eo = EoExtension::new().with_cloud_cover(25.5);
        assert_eq!(eo.cloud_cover, Some(25.5));
    }

    #[test]
    fn test_eo_extension_validate_cloud_cover() {
        let valid = EoExtension::new().with_cloud_cover(50.0);
        assert!(valid.validate().is_ok());

        let invalid = EoExtension::new().with_cloud_cover(150.0);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_band_new() {
        let band = Band::new();
        assert!(band.name.is_none());
        assert!(band.common_name.is_none());
    }

    #[test]
    fn test_band_builder() {
        let band = Band::new()
            .with_name("B04")
            .with_common_name(CommonBandName::Red)
            .with_center_wavelength(0.665)
            .with_full_width_half_max(0.038);

        assert_eq!(band.name, Some("B04".to_string()));
        assert_eq!(band.common_name, Some(CommonBandName::Red));
        assert_eq!(band.center_wavelength, Some(0.665));
        assert_eq!(band.full_width_half_max, Some(0.038));
    }

    #[test]
    fn test_band_validate() {
        let valid = Band::new().with_center_wavelength(0.665);
        assert!(valid.validate().is_ok());

        let invalid = Band::new().with_center_wavelength(-0.1);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_eo_extension_serialization() {
        let eo = EoExtension::new()
            .with_cloud_cover(10.0)
            .add_band(Band::new().with_common_name(CommonBandName::Red));

        let json = serde_json::to_string(&eo);
        assert!(json.is_ok());

        let deserialized: EoExtension =
            serde_json::from_str(&json.expect("JSON serialization failed"))
                .expect("Deserialization failed");
        assert_eq!(eo, deserialized);
    }
}
