//! SAR (Synthetic Aperture Radar) Extension.
//!
//! This module implements the STAC SAR Extension for describing radar data.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// SAR Extension for STAC Items.
///
/// This extension describes synthetic aperture radar (SAR) data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SarExtension {
    /// Required frequency band of the instrument used to produce the data.
    #[serde(rename = "sar:frequency_band")]
    pub frequency_band: FrequencyBand,

    /// Center wavelength of the instrument used to produce the data (in cm).
    #[serde(
        rename = "sar:center_frequency",
        skip_serializing_if = "Option::is_none"
    )]
    pub center_frequency: Option<f64>,

    /// List of polarizations.
    #[serde(rename = "sar:polarizations")]
    pub polarizations: Vec<Polarization>,

    /// Product type.
    #[serde(rename = "sar:product_type", skip_serializing_if = "Option::is_none")]
    pub product_type: Option<String>,

    /// Resolution in azimuth (in meters).
    #[serde(
        rename = "sar:resolution_azimuth",
        skip_serializing_if = "Option::is_none"
    )]
    pub resolution_azimuth: Option<f64>,

    /// Resolution in range (in meters).
    #[serde(
        rename = "sar:resolution_range",
        skip_serializing_if = "Option::is_none"
    )]
    pub resolution_range: Option<f64>,

    /// Pixel spacing in azimuth direction (in meters).
    #[serde(
        rename = "sar:pixel_spacing_azimuth",
        skip_serializing_if = "Option::is_none"
    )]
    pub pixel_spacing_azimuth: Option<f64>,

    /// Pixel spacing in range direction (in meters).
    #[serde(
        rename = "sar:pixel_spacing_range",
        skip_serializing_if = "Option::is_none"
    )]
    pub pixel_spacing_range: Option<f64>,

    /// Looks in azimuth direction.
    #[serde(rename = "sar:looks_azimuth", skip_serializing_if = "Option::is_none")]
    pub looks_azimuth: Option<u32>,

    /// Looks in range direction.
    #[serde(rename = "sar:looks_range", skip_serializing_if = "Option::is_none")]
    pub looks_range: Option<u32>,

    /// Equivalent number of looks (ENL).
    #[serde(
        rename = "sar:looks_equivalent_number",
        skip_serializing_if = "Option::is_none"
    )]
    pub looks_equivalent_number: Option<f64>,

    /// Observation direction (right or left).
    #[serde(
        rename = "sar:observation_direction",
        skip_serializing_if = "Option::is_none"
    )]
    pub observation_direction: Option<ObservationDirection>,

    /// Instrument mode.
    #[serde(
        rename = "sar:instrument_mode",
        skip_serializing_if = "Option::is_none"
    )]
    pub instrument_mode: Option<String>,

    /// Additional properties.
    #[serde(flatten)]
    pub additional_properties: HashMap<String, serde_json::Value>,
}

/// SAR frequency bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FrequencyBand {
    /// P band (0.3-1 GHz).
    P,
    /// L band (1-2 GHz).
    L,
    /// S band (2-4 GHz).
    S,
    /// C band (4-8 GHz).
    C,
    /// X band (8-12 GHz).
    X,
    /// Ku band (12-18 GHz).
    Ku,
    /// K band (18-27 GHz).
    K,
    /// Ka band (27-40 GHz).
    Ka,
}

/// SAR polarizations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Polarization {
    /// Horizontal transmit, horizontal receive.
    HH,
    /// Horizontal transmit, vertical receive.
    HV,
    /// Vertical transmit, horizontal receive.
    VH,
    /// Vertical transmit, vertical receive.
    VV,
}

/// SAR observation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationDirection {
    /// Right-looking.
    Right,
    /// Left-looking.
    Left,
}

impl SarExtension {
    /// Creates a new SAR extension.
    pub fn new(frequency_band: FrequencyBand, polarizations: Vec<Polarization>) -> Self {
        Self {
            frequency_band,
            center_frequency: None,
            polarizations,
            product_type: None,
            resolution_azimuth: None,
            resolution_range: None,
            pixel_spacing_azimuth: None,
            pixel_spacing_range: None,
            looks_azimuth: None,
            looks_range: None,
            looks_equivalent_number: None,
            observation_direction: None,
            instrument_mode: None,
            additional_properties: HashMap::new(),
        }
    }

    /// Sets the center frequency.
    pub fn with_center_frequency(mut self, frequency: f64) -> Self {
        self.center_frequency = Some(frequency);
        self
    }

    /// Sets the product type.
    pub fn with_product_type(mut self, product_type: impl Into<String>) -> Self {
        self.product_type = Some(product_type.into());
        self
    }

    /// Sets the resolution (azimuth and range).
    pub fn with_resolution(mut self, azimuth: f64, range: f64) -> Self {
        self.resolution_azimuth = Some(azimuth);
        self.resolution_range = Some(range);
        self
    }

    /// Sets the pixel spacing (azimuth and range).
    pub fn with_pixel_spacing(mut self, azimuth: f64, range: f64) -> Self {
        self.pixel_spacing_azimuth = Some(azimuth);
        self.pixel_spacing_range = Some(range);
        self
    }

    /// Sets the looks (azimuth and range).
    pub fn with_looks(mut self, azimuth: u32, range: u32) -> Self {
        self.looks_azimuth = Some(azimuth);
        self.looks_range = Some(range);
        self
    }

    /// Sets the equivalent number of looks.
    pub fn with_equivalent_number_of_looks(mut self, enl: f64) -> Self {
        self.looks_equivalent_number = Some(enl);
        self
    }

    /// Sets the observation direction.
    pub fn with_observation_direction(mut self, direction: ObservationDirection) -> Self {
        self.observation_direction = Some(direction);
        self
    }

    /// Sets the instrument mode.
    pub fn with_instrument_mode(mut self, mode: impl Into<String>) -> Self {
        self.instrument_mode = Some(mode.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sar_extension_new() {
        let sar = SarExtension::new(FrequencyBand::C, vec![Polarization::VV, Polarization::VH]);

        assert_eq!(sar.frequency_band, FrequencyBand::C);
        assert_eq!(sar.polarizations.len(), 2);
        assert!(sar.center_frequency.is_none());
    }

    #[test]
    fn test_sar_extension_builder() {
        let sar = SarExtension::new(FrequencyBand::C, vec![Polarization::VV])
            .with_center_frequency(5.405)
            .with_product_type("GRD")
            .with_resolution(10.0, 10.0)
            .with_pixel_spacing(10.0, 10.0)
            .with_looks(1, 1)
            .with_observation_direction(ObservationDirection::Right)
            .with_instrument_mode("IW");

        assert_eq!(sar.center_frequency, Some(5.405));
        assert_eq!(sar.product_type, Some("GRD".to_string()));
        assert_eq!(sar.resolution_azimuth, Some(10.0));
        assert_eq!(sar.observation_direction, Some(ObservationDirection::Right));
    }

    #[test]
    fn test_frequency_band_serialization() {
        let band = FrequencyBand::C;
        let json = serde_json::to_string(&band).expect("Failed to serialize");
        assert_eq!(json, "\"C\"");

        let deserialized: FrequencyBand =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized, band);
    }

    #[test]
    fn test_polarization_serialization() {
        let pol = Polarization::VV;
        let json = serde_json::to_string(&pol).expect("Failed to serialize");
        assert_eq!(json, "\"VV\"");

        let deserialized: Polarization =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized, pol);
    }

    #[test]
    fn test_observation_direction_serialization() {
        let dir = ObservationDirection::Right;
        let json = serde_json::to_string(&dir).expect("Failed to serialize");
        assert_eq!(json, "\"right\"");

        let deserialized: ObservationDirection =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized, dir);
    }
}
