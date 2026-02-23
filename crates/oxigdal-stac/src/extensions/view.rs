//! View Extension.
//!
//! This module implements the STAC View Geometry Extension for describing viewing angles.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// View Extension for STAC Items.
///
/// This extension describes the viewing geometry of remotely sensed imagery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ViewExtension {
    /// Angle from the sensor between nadir and the scene center (in degrees).
    #[serde(rename = "view:off_nadir", skip_serializing_if = "Option::is_none")]
    pub off_nadir: Option<f64>,

    /// Incidence angle at the scene center (in degrees).
    #[serde(
        rename = "view:incidence_angle",
        skip_serializing_if = "Option::is_none"
    )]
    pub incidence_angle: Option<f64>,

    /// Angle measured clockwise from the north direction (in degrees).
    #[serde(rename = "view:azimuth", skip_serializing_if = "Option::is_none")]
    pub azimuth: Option<f64>,

    /// Sun azimuth angle at the scene center (in degrees).
    #[serde(rename = "view:sun_azimuth", skip_serializing_if = "Option::is_none")]
    pub sun_azimuth: Option<f64>,

    /// Sun elevation angle at the scene center (in degrees).
    #[serde(rename = "view:sun_elevation", skip_serializing_if = "Option::is_none")]
    pub sun_elevation: Option<f64>,

    /// Additional properties.
    #[serde(flatten)]
    pub additional_properties: HashMap<String, serde_json::Value>,
}

impl ViewExtension {
    /// Creates a new View extension.
    pub fn new() -> Self {
        Self {
            off_nadir: None,
            incidence_angle: None,
            azimuth: None,
            sun_azimuth: None,
            sun_elevation: None,
            additional_properties: HashMap::new(),
        }
    }

    /// Sets the off-nadir angle.
    pub fn with_off_nadir(mut self, angle: f64) -> Self {
        self.off_nadir = Some(angle);
        self
    }

    /// Sets the incidence angle.
    pub fn with_incidence_angle(mut self, angle: f64) -> Self {
        self.incidence_angle = Some(angle);
        self
    }

    /// Sets the azimuth angle.
    pub fn with_azimuth(mut self, angle: f64) -> Self {
        self.azimuth = Some(angle);
        self
    }

    /// Sets the sun azimuth angle.
    pub fn with_sun_azimuth(mut self, angle: f64) -> Self {
        self.sun_azimuth = Some(angle);
        self
    }

    /// Sets the sun elevation angle.
    pub fn with_sun_elevation(mut self, angle: f64) -> Self {
        self.sun_elevation = Some(angle);
        self
    }

    /// Validates that all angles are within valid ranges.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error message.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(off_nadir) = self.off_nadir {
            if !(0.0..=90.0).contains(&off_nadir) {
                return Err(format!(
                    "off_nadir angle must be between 0 and 90 degrees, got {}",
                    off_nadir
                ));
            }
        }

        if let Some(incidence_angle) = self.incidence_angle {
            if !(0.0..=90.0).contains(&incidence_angle) {
                return Err(format!(
                    "incidence_angle must be between 0 and 90 degrees, got {}",
                    incidence_angle
                ));
            }
        }

        if let Some(azimuth) = self.azimuth {
            if !(0.0..=360.0).contains(&azimuth) {
                return Err(format!(
                    "azimuth must be between 0 and 360 degrees, got {}",
                    azimuth
                ));
            }
        }

        if let Some(sun_azimuth) = self.sun_azimuth {
            if !(0.0..=360.0).contains(&sun_azimuth) {
                return Err(format!(
                    "sun_azimuth must be between 0 and 360 degrees, got {}",
                    sun_azimuth
                ));
            }
        }

        if let Some(sun_elevation) = self.sun_elevation {
            if !(-90.0..=90.0).contains(&sun_elevation) {
                return Err(format!(
                    "sun_elevation must be between -90 and 90 degrees, got {}",
                    sun_elevation
                ));
            }
        }

        Ok(())
    }
}

impl Default for ViewExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_extension_new() {
        let view = ViewExtension::new();
        assert!(view.off_nadir.is_none());
        assert!(view.incidence_angle.is_none());
        assert!(view.azimuth.is_none());
    }

    #[test]
    fn test_view_extension_builder() {
        let view = ViewExtension::new()
            .with_off_nadir(15.0)
            .with_incidence_angle(20.0)
            .with_azimuth(135.0)
            .with_sun_azimuth(150.0)
            .with_sun_elevation(45.0);

        assert_eq!(view.off_nadir, Some(15.0));
        assert_eq!(view.incidence_angle, Some(20.0));
        assert_eq!(view.azimuth, Some(135.0));
        assert_eq!(view.sun_azimuth, Some(150.0));
        assert_eq!(view.sun_elevation, Some(45.0));
    }

    #[test]
    fn test_view_extension_validation() {
        let valid = ViewExtension::new()
            .with_off_nadir(15.0)
            .with_sun_elevation(45.0);
        assert!(valid.validate().is_ok());

        let invalid_off_nadir = ViewExtension::new().with_off_nadir(95.0);
        assert!(invalid_off_nadir.validate().is_err());

        let invalid_azimuth = ViewExtension::new().with_azimuth(365.0);
        assert!(invalid_azimuth.validate().is_err());

        let invalid_sun_elevation = ViewExtension::new().with_sun_elevation(-95.0);
        assert!(invalid_sun_elevation.validate().is_err());
    }

    #[test]
    fn test_view_extension_serialization() {
        let view = ViewExtension::new()
            .with_off_nadir(15.0)
            .with_sun_azimuth(150.0);

        let json = serde_json::to_string(&view).expect("Failed to serialize");
        assert!(json.contains("view:off_nadir"));
        assert!(json.contains("view:sun_azimuth"));

        let deserialized: ViewExtension =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized, view);
    }
}
