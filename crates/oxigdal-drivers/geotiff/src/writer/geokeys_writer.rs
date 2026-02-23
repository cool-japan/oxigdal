//! GeoKey writing for GeoTIFF georeferencing
//!
//! This module handles writing GeoTIFF GeoKeys for CRS information.

use oxigdal_core::types::GeoTransform;

use crate::geokeys::{GeoKey, ModelType, RasterType};
use crate::tiff::TiffTag;
use crate::writer::ifd_writer::IfdBuilder;

/// Builder for GeoKey directory
#[derive(Debug)]
pub struct GeoKeysBuilder {
    /// GeoKey entries
    entries: Vec<GeoKeyEntry>,
    /// Double parameters
    double_params: Vec<f64>,
    /// ASCII parameters
    ascii_params: String,
}

/// A single GeoKey entry
#[derive(Debug, Clone)]
struct GeoKeyEntry {
    /// Key ID
    key_id: u16,
    /// Tag location (0 = inline, 34736 = GeoDoubleParams, 34737 = GeoAsciiParams)
    tag_location: u16,
    /// Count
    count: u16,
    /// Value or offset
    value_offset: u16,
}

impl GeoKeysBuilder {
    /// Creates a new GeoKeys builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            double_params: Vec::new(),
            ascii_params: String::new(),
        }
    }

    /// Adds a short (u16) GeoKey value
    pub fn add_short(&mut self, key: GeoKey, value: u16) {
        self.entries.push(GeoKeyEntry {
            key_id: key as u16,
            tag_location: 0, // Inline
            count: 1,
            value_offset: value,
        });
    }

    /// Adds a double GeoKey value
    pub fn add_double(&mut self, key: GeoKey, value: f64) {
        let offset = self.double_params.len() as u16;
        self.double_params.push(value);

        self.entries.push(GeoKeyEntry {
            key_id: key as u16,
            tag_location: TiffTag::GeoDoubleParams as u16,
            count: 1,
            value_offset: offset,
        });
    }

    /// Adds an ASCII GeoKey value
    pub fn add_ascii(&mut self, key: GeoKey, value: &str) {
        let offset = self.ascii_params.len() as u16;
        self.ascii_params.push_str(value);
        self.ascii_params.push('|'); // GeoTIFF string terminator

        self.entries.push(GeoKeyEntry {
            key_id: key as u16,
            tag_location: TiffTag::GeoAsciiParams as u16,
            count: value.len() as u16 + 1, // Include terminator
            value_offset: offset,
        });
    }

    /// Sets up GeoKeys for an EPSG code
    pub fn set_epsg_code(&mut self, epsg_code: u32, is_projected: bool) {
        if is_projected {
            // Projected CRS
            self.add_short(GeoKey::GtModelType, ModelType::Projected as u16);
            self.add_short(GeoKey::ProjectedCsType, epsg_code as u16);
        } else {
            // Geographic CRS
            self.add_short(GeoKey::GtModelType, ModelType::Geographic as u16);
            self.add_short(GeoKey::GeographicType, epsg_code as u16);
        }

        // Default raster type
        self.add_short(GeoKey::GtRasterType, RasterType::PixelIsArea as u16);
    }

    /// Sets up GeoKeys for a simple projected CRS
    pub fn set_simple_projected(&mut self, epsg_code: u32) {
        self.set_epsg_code(epsg_code, true);
    }

    /// Sets up GeoKeys for a simple geographic CRS
    pub fn set_simple_geographic(&mut self, epsg_code: u32) {
        self.set_epsg_code(epsg_code, false);
    }

    /// Adds GeoKeys to an IFD builder
    pub fn add_to_ifd(&self, ifd: &mut IfdBuilder) {
        if self.entries.is_empty() {
            return;
        }

        // Build GeoKey directory
        let mut directory = Vec::new();

        // Header: version, revision, minor revision, key count
        directory.push(1u16); // Version
        directory.push(1u16); // Revision
        directory.push(0u16); // Minor revision
        directory.push(self.entries.len() as u16);

        // Add entries
        for entry in &self.entries {
            directory.push(entry.key_id);
            directory.push(entry.tag_location);
            directory.push(entry.count);
            directory.push(entry.value_offset);
        }

        // Add to IFD
        ifd.add_short_array(TiffTag::GeoKeyDirectory, directory);

        // Add double params if any
        if !self.double_params.is_empty() {
            ifd.add_double_array(TiffTag::GeoDoubleParams, self.double_params.clone());
        }

        // Add ASCII params if any
        if !self.ascii_params.is_empty() {
            ifd.add_ascii(TiffTag::GeoAsciiParams, self.ascii_params.clone());
        }
    }
}

impl Default for GeoKeysBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Adds GeoTransform to an IFD
pub fn add_geo_transform(ifd: &mut IfdBuilder, geo_transform: &GeoTransform) {
    // Check if this is a simple north-up transform
    if geo_transform.is_north_up() {
        // Use ModelPixelScale + ModelTiepoint (simpler, more common)
        let pixel_scale = vec![
            geo_transform.pixel_width,
            geo_transform.pixel_height.abs(),
            0.0,
        ];
        ifd.add_double_array(TiffTag::ModelPixelScale, pixel_scale);

        // Tiepoint: pixel (0,0,0) maps to geo (origin_x, origin_y, 0)
        let tiepoint = vec![
            0.0,
            0.0,
            0.0,
            geo_transform.origin_x,
            geo_transform.origin_y,
            0.0,
        ];
        ifd.add_double_array(TiffTag::ModelTiepoint, tiepoint);
    } else {
        // Use ModelTransformation (full affine transform)
        let matrix = vec![
            geo_transform.pixel_width,
            geo_transform.row_rotation,
            0.0,
            geo_transform.origin_x,
            geo_transform.col_rotation,
            geo_transform.pixel_height,
            0.0,
            geo_transform.origin_y,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        ifd.add_double_array(TiffTag::ModelTransformation, matrix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::{ByteOrderType, TiffVariant};

    #[test]
    fn test_geokeys_builder() {
        let mut builder = GeoKeysBuilder::new();
        builder.set_simple_projected(32632); // UTM Zone 32N

        assert!(!builder.entries.is_empty());
    }

    #[test]
    fn test_geokeys_with_doubles() {
        let mut builder = GeoKeysBuilder::new();
        builder.add_double(GeoKey::ProjNatOriginLong, -123.0);
        builder.add_double(GeoKey::ProjNatOriginLat, 45.0);

        assert_eq!(builder.double_params.len(), 2);
    }

    #[test]
    fn test_geokeys_with_ascii() {
        let mut builder = GeoKeysBuilder::new();
        builder.add_ascii(GeoKey::GtCitation, "Test CRS");

        assert!(!builder.ascii_params.is_empty());
    }

    #[test]
    fn test_add_to_ifd() {
        let mut geokeys = GeoKeysBuilder::new();
        geokeys.set_simple_geographic(4326); // WGS84

        let mut ifd = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);
        geokeys.add_to_ifd(&mut ifd);

        // IFD should now have GeoKey entries
        // (This is a basic test - a real test would verify the IFD contents)
    }

    #[test]
    fn test_geo_transform_north_up() {
        let gt = GeoTransform::north_up(100.0, 200.0, 0.5, -0.5);
        let mut ifd = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);

        add_geo_transform(&mut ifd, &gt);

        // Should add ModelPixelScale and ModelTiepoint
    }

    #[test]
    fn test_geo_transform_rotated() {
        let gt = GeoTransform::new(100.0, 0.5, 0.1, 200.0, 0.1, -0.5);
        let mut ifd = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);

        add_geo_transform(&mut ifd, &gt);

        // Should add ModelTransformation
    }
}
