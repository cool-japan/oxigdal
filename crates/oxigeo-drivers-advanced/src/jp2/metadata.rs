//! JPEG2000 metadata structures.

use serde::{Deserialize, Serialize};

/// JP2 metadata container.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Jp2Metadata {
    /// XML metadata boxes
    pub xml_metadata: Vec<String>,
    /// GeoJP2 metadata
    pub geojp2: Option<GeoJp2Metadata>,
    /// Color space information
    pub color_space: Option<ColorSpace>,
    /// Resolution information
    pub resolution: Option<Resolution>,
    /// ICC profile
    pub icc_profile: Option<Vec<u8>>,
}

impl Jp2Metadata {
    /// Add XML metadata.
    pub fn add_xml(&mut self, xml: String) {
        self.xml_metadata.push(xml);
    }

    /// Set GeoJP2 metadata.
    pub fn set_geojp2(&mut self, data: Vec<u8>) {
        self.geojp2 = Some(GeoJp2Metadata { data });
    }

    /// Set color space.
    pub fn set_color_space(&mut self, color_space: ColorSpace) {
        self.color_space = Some(color_space);
    }

    /// Set resolution.
    pub fn set_resolution(&mut self, resolution: Resolution) {
        self.resolution = Some(resolution);
    }

    /// Set ICC profile.
    pub fn set_icc_profile(&mut self, profile: Vec<u8>) {
        self.icc_profile = Some(profile);
    }

    /// Get all XML metadata as string.
    pub fn xml_as_string(&self) -> String {
        self.xml_metadata.join("\n")
    }

    /// Check if has GeoJP2 metadata.
    pub fn has_geojp2(&self) -> bool {
        self.geojp2.is_some()
    }

    /// Check if has ICC profile.
    pub fn has_icc_profile(&self) -> bool {
        self.icc_profile.is_some()
    }
}

/// GeoJP2 metadata (GeoTIFF-compatible georeferencing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoJp2Metadata {
    /// Raw GeoTIFF IFD data
    pub data: Vec<u8>,
}

impl GeoJp2Metadata {
    /// Create new GeoJP2 metadata.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Get data size.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Color space enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorSpace {
    /// sRGB color space
    Srgb,
    /// Grayscale
    Grayscale,
    /// YCbCr
    YCbCr,
    /// e-sRGB
    ESrgb,
    /// ROMM-RGB
    RommRgb,
    /// Unknown/other
    Unknown,
}

impl ColorSpace {
    /// Parse from enumerated color space value.
    pub fn from_enum(value: u32) -> Self {
        match value {
            16 => Self::Srgb,
            17 => Self::Grayscale,
            18 => Self::YCbCr,
            20 => Self::ESrgb,
            21 => Self::RommRgb,
            _ => Self::Unknown,
        }
    }

    /// Get enumerated value.
    pub fn to_enum(self) -> u32 {
        match self {
            Self::Srgb => 16,
            Self::Grayscale => 17,
            Self::YCbCr => 18,
            Self::ESrgb => 20,
            Self::RommRgb => 21,
            Self::Unknown => 0,
        }
    }

    /// Check if color space is RGB-based.
    pub fn is_rgb(&self) -> bool {
        matches!(self, Self::Srgb | Self::ESrgb | Self::RommRgb)
    }

    /// Check if color space is grayscale.
    pub fn is_grayscale(&self) -> bool {
        matches!(self, Self::Grayscale)
    }
}

/// Resolution information.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Resolution {
    /// Vertical capture resolution (pixels per meter)
    pub capture_vertical: f64,
    /// Horizontal capture resolution (pixels per meter)
    pub capture_horizontal: f64,
    /// Vertical display resolution (pixels per meter)
    pub display_vertical: Option<f64>,
    /// Horizontal display resolution (pixels per meter)
    pub display_horizontal: Option<f64>,
}

impl Resolution {
    /// Create new resolution info.
    pub fn new(vertical: f64, horizontal: f64) -> Self {
        Self {
            capture_vertical: vertical,
            capture_horizontal: horizontal,
            display_vertical: None,
            display_horizontal: None,
        }
    }

    /// Set display resolution.
    pub fn with_display(mut self, vertical: f64, horizontal: f64) -> Self {
        self.display_vertical = Some(vertical);
        self.display_horizontal = Some(horizontal);
        self
    }

    /// Convert to DPI (dots per inch).
    pub fn to_dpi(self) -> (f64, f64) {
        const METERS_PER_INCH: f64 = 0.0254;
        (
            self.capture_horizontal * METERS_PER_INCH,
            self.capture_vertical * METERS_PER_INCH,
        )
    }

    /// Create from DPI.
    pub fn from_dpi(horizontal_dpi: f64, vertical_dpi: f64) -> Self {
        const METERS_PER_INCH: f64 = 0.0254;
        Self {
            capture_horizontal: horizontal_dpi / METERS_PER_INCH,
            capture_vertical: vertical_dpi / METERS_PER_INCH,
            display_horizontal: None,
            display_vertical: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jp2_metadata_creation() {
        let mut metadata = Jp2Metadata::default();
        assert!(metadata.xml_metadata.is_empty());
        assert!(!metadata.has_geojp2());
        assert!(!metadata.has_icc_profile());

        metadata.add_xml("<test>data</test>".to_string());
        assert_eq!(metadata.xml_metadata.len(), 1);
    }

    #[test]
    fn test_geojp2_metadata() {
        let data = vec![1, 2, 3, 4];
        let geojp2 = GeoJp2Metadata::new(data.clone());
        assert_eq!(geojp2.size(), 4);
        assert_eq!(geojp2.data, data);
    }

    #[test]
    fn test_color_space_conversion() {
        assert_eq!(ColorSpace::from_enum(16), ColorSpace::Srgb);
        assert_eq!(ColorSpace::from_enum(17), ColorSpace::Grayscale);
        assert_eq!(ColorSpace::Srgb.to_enum(), 16);

        assert!(ColorSpace::Srgb.is_rgb());
        assert!(!ColorSpace::Srgb.is_grayscale());
        assert!(ColorSpace::Grayscale.is_grayscale());
    }

    #[test]
    fn test_resolution() {
        let res = Resolution::new(100.0, 100.0);
        let (h_dpi, v_dpi) = res.to_dpi();
        assert!((h_dpi - 2.54).abs() < 0.01);
        assert!((v_dpi - 2.54).abs() < 0.01);

        let res2 = Resolution::from_dpi(300.0, 300.0);
        assert!((res2.capture_horizontal - 11811.0).abs() < 1.0);
    }

    #[test]
    fn test_resolution_with_display() {
        let res = Resolution::new(100.0, 100.0).with_display(200.0, 200.0);
        assert_eq!(res.display_vertical, Some(200.0));
        assert_eq!(res.display_horizontal, Some(200.0));
    }
}
