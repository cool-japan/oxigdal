//! GeoTIFF GeoKey parsing
//!
//! This module handles parsing of GeoTIFF geospatial metadata stored in GeoKeys.
//! GeoKeys provide CRS information, coordinate system parameters, and more.

use oxigdal_core::error::{FormatError, OxiGdalError, Result};
use oxigdal_core::io::DataSource;
use oxigdal_core::types::GeoTransform;

use crate::tiff::{ByteOrderType, Ifd, TiffTag, TiffVariant};

/// GeoTIFF key IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum GeoKey {
    // GeoTIFF Configuration Keys
    /// GeoTIFF version
    GtModelType = 1024,
    /// Raster type
    GtRasterType = 1025,
    /// Citation
    GtCitation = 1026,

    // Geographic CS Parameter Keys
    /// Geographic type (EPSG code)
    GeographicType = 2048,
    /// Geographic citation
    GeogCitation = 2049,
    /// Geodetic datum
    GeogGeodeticDatum = 2050,
    /// Prime meridian
    GeogPrimeMeridian = 2051,
    /// Linear units
    GeogLinearUnits = 2052,
    /// Linear unit size
    GeogLinearUnitSize = 2053,
    /// Angular units
    GeogAngularUnits = 2054,
    /// Angular unit size
    GeogAngularUnitSize = 2055,
    /// Ellipsoid
    GeogEllipsoid = 2056,
    /// Semi-major axis
    GeogSemiMajorAxis = 2057,
    /// Semi-minor axis
    GeogSemiMinorAxis = 2058,
    /// Inverse flattening
    GeogInvFlattening = 2059,
    /// Azimuth units
    GeogAzimuthUnits = 2060,
    /// Prime meridian longitude
    GeogPrimeMeridianLong = 2061,

    // Projected CS Parameter Keys
    /// Projected CS type (EPSG code)
    ProjectedCsType = 3072,
    /// PCS citation
    PcsCitation = 3073,
    /// Projection
    Projection = 3074,
    /// Proj coord trans (projection method)
    ProjCoordTrans = 3075,
    /// Linear units
    ProjLinearUnits = 3076,
    /// Linear unit size
    ProjLinearUnitSize = 3077,
    /// Standard parallel 1
    ProjStdParallel1 = 3078,
    /// Standard parallel 2
    ProjStdParallel2 = 3079,
    /// Natural origin longitude
    ProjNatOriginLong = 3080,
    /// Natural origin latitude
    ProjNatOriginLat = 3081,
    /// False easting
    ProjFalseEasting = 3082,
    /// False northing
    ProjFalseNorthing = 3083,
    /// False origin longitude
    ProjFalseOriginLong = 3084,
    /// False origin latitude
    ProjFalseOriginLat = 3085,
    /// False origin easting
    ProjFalseOriginEasting = 3086,
    /// False origin northing
    ProjFalseOriginNorthing = 3087,
    /// Center longitude
    ProjCenterLong = 3088,
    /// Center latitude
    ProjCenterLat = 3089,
    /// Center easting
    ProjCenterEasting = 3090,
    /// Center northing
    ProjCenterNorthing = 3091,
    /// Scale at natural origin
    ProjScaleAtNatOrigin = 3092,
    /// Scale at center
    ProjScaleAtCenter = 3093,
    /// Azimuth angle
    ProjAzimuthAngle = 3094,
    /// Straight vertical pole longitude
    ProjStraightVertPoleLong = 3095,

    // Vertical CS Parameter Keys
    /// Vertical CS type
    VerticalCsType = 4096,
    /// Vertical citation
    VerticalCitation = 4097,
    /// Vertical datum
    VerticalDatum = 4098,
    /// Vertical units
    VerticalUnits = 4099,
}

impl GeoKey {
    /// Creates a GeoKey from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1024 => Some(Self::GtModelType),
            1025 => Some(Self::GtRasterType),
            1026 => Some(Self::GtCitation),
            2048 => Some(Self::GeographicType),
            2049 => Some(Self::GeogCitation),
            2050 => Some(Self::GeogGeodeticDatum),
            2051 => Some(Self::GeogPrimeMeridian),
            2052 => Some(Self::GeogLinearUnits),
            2053 => Some(Self::GeogLinearUnitSize),
            2054 => Some(Self::GeogAngularUnits),
            2055 => Some(Self::GeogAngularUnitSize),
            2056 => Some(Self::GeogEllipsoid),
            2057 => Some(Self::GeogSemiMajorAxis),
            2058 => Some(Self::GeogSemiMinorAxis),
            2059 => Some(Self::GeogInvFlattening),
            2060 => Some(Self::GeogAzimuthUnits),
            2061 => Some(Self::GeogPrimeMeridianLong),
            3072 => Some(Self::ProjectedCsType),
            3073 => Some(Self::PcsCitation),
            3074 => Some(Self::Projection),
            3075 => Some(Self::ProjCoordTrans),
            3076 => Some(Self::ProjLinearUnits),
            3077 => Some(Self::ProjLinearUnitSize),
            3078 => Some(Self::ProjStdParallel1),
            3079 => Some(Self::ProjStdParallel2),
            3080 => Some(Self::ProjNatOriginLong),
            3081 => Some(Self::ProjNatOriginLat),
            3082 => Some(Self::ProjFalseEasting),
            3083 => Some(Self::ProjFalseNorthing),
            3084 => Some(Self::ProjFalseOriginLong),
            3085 => Some(Self::ProjFalseOriginLat),
            3086 => Some(Self::ProjFalseOriginEasting),
            3087 => Some(Self::ProjFalseOriginNorthing),
            3088 => Some(Self::ProjCenterLong),
            3089 => Some(Self::ProjCenterLat),
            3090 => Some(Self::ProjCenterEasting),
            3091 => Some(Self::ProjCenterNorthing),
            3092 => Some(Self::ProjScaleAtNatOrigin),
            3093 => Some(Self::ProjScaleAtCenter),
            3094 => Some(Self::ProjAzimuthAngle),
            3095 => Some(Self::ProjStraightVertPoleLong),
            4096 => Some(Self::VerticalCsType),
            4097 => Some(Self::VerticalCitation),
            4098 => Some(Self::VerticalDatum),
            4099 => Some(Self::VerticalUnits),
            _ => None,
        }
    }
}

/// Model type (GTModelTypeGeoKey)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ModelType {
    /// Projected coordinate system
    Projected = 1,
    /// Geographic (lat/lon)
    Geographic = 2,
    /// Geocentric (3D)
    Geocentric = 3,
}

impl ModelType {
    /// Creates a ModelType from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Projected),
            2 => Some(Self::Geographic),
            3 => Some(Self::Geocentric),
            _ => None,
        }
    }
}

/// Raster type (GTRasterTypeGeoKey)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum RasterType {
    /// Pixel is area
    PixelIsArea = 1,
    /// Pixel is point
    PixelIsPoint = 2,
}

impl RasterType {
    /// Creates a RasterType from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::PixelIsArea),
            2 => Some(Self::PixelIsPoint),
            _ => None,
        }
    }
}

/// A single GeoKey entry
#[derive(Debug, Clone)]
pub struct GeoKeyEntry {
    /// Key ID
    pub key_id: u16,
    /// TIFF tag location (0 = inline, 34736 = GeoDoubleParams, 34737 = GeoAsciiParams)
    pub tiff_tag_location: u16,
    /// Count (for strings/arrays)
    pub count: u16,
    /// Value or index
    pub value_offset: u16,
}

/// Parsed GeoKey directory
#[derive(Debug, Clone)]
pub struct GeoKeyDirectory {
    /// GeoTIFF version (typically 1)
    pub version: u16,
    /// Key revision (major)
    pub key_revision_major: u16,
    /// Key revision (minor)
    pub key_revision_minor: u16,
    /// Individual GeoKey entries
    pub entries: Vec<GeoKeyEntry>,
    /// Double-precision parameters
    pub double_params: Vec<f64>,
    /// ASCII parameters
    pub ascii_params: String,
}

impl GeoKeyDirectory {
    /// Parses GeoKeys from an IFD
    pub fn from_ifd<S: DataSource>(
        ifd: &Ifd,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
    ) -> Result<Option<Self>> {
        // Get the GeoKeyDirectory tag
        let directory_entry = match ifd.get_entry(TiffTag::GeoKeyDirectory) {
            Some(e) => e,
            None => return Ok(None),
        };

        let directory_values = directory_entry.get_u64_vec(source, byte_order, variant)?;
        if directory_values.len() < 4 {
            return Err(OxiGdalError::Format(FormatError::InvalidGeoKey {
                key_id: 0,
                message: "GeoKeyDirectory too short".to_string(),
            }));
        }

        let version = directory_values[0] as u16;
        let key_revision_major = directory_values[1] as u16;
        let key_revision_minor = directory_values[2] as u16;
        let key_count = directory_values[3] as usize;

        if directory_values.len() < 4 + key_count * 4 {
            return Err(OxiGdalError::Format(FormatError::InvalidGeoKey {
                key_id: 0,
                message: format!(
                    "GeoKeyDirectory too short: {} values, need {} for {} keys",
                    directory_values.len(),
                    4 + key_count * 4,
                    key_count
                ),
            }));
        }

        let mut entries = Vec::with_capacity(key_count);
        for i in 0..key_count {
            let base = 4 + i * 4;
            entries.push(GeoKeyEntry {
                key_id: directory_values[base] as u16,
                tiff_tag_location: directory_values[base + 1] as u16,
                count: directory_values[base + 2] as u16,
                value_offset: directory_values[base + 3] as u16,
            });
        }

        // Get double params
        let double_params = if let Some(entry) = ifd.get_entry(TiffTag::GeoDoubleParams) {
            entry.get_f64_vec(source, byte_order, variant)?
        } else {
            Vec::new()
        };

        // Get ASCII params
        let ascii_params = if let Some(entry) = ifd.get_entry(TiffTag::GeoAsciiParams) {
            entry.get_ascii(source, variant)?
        } else {
            String::new()
        };

        Ok(Some(Self {
            version,
            key_revision_major,
            key_revision_minor,
            entries,
            double_params,
            ascii_params,
        }))
    }

    /// Gets a short (u16) value for a key
    #[must_use]
    pub fn get_short(&self, key: GeoKey) -> Option<u16> {
        for entry in &self.entries {
            if entry.key_id == key as u16 && entry.tiff_tag_location == 0 {
                return Some(entry.value_offset);
            }
        }
        None
    }

    /// Gets a double value for a key
    #[must_use]
    pub fn get_double(&self, key: GeoKey) -> Option<f64> {
        for entry in &self.entries {
            if entry.key_id == key as u16
                && entry.tiff_tag_location == TiffTag::GeoDoubleParams as u16
            {
                let index = entry.value_offset as usize;
                return self.double_params.get(index).copied();
            }
        }
        None
    }

    /// Gets an ASCII string for a key
    #[must_use]
    pub fn get_ascii(&self, key: GeoKey) -> Option<&str> {
        for entry in &self.entries {
            if entry.key_id == key as u16
                && entry.tiff_tag_location == TiffTag::GeoAsciiParams as u16
            {
                let start = entry.value_offset as usize;
                let end = start + entry.count as usize;
                if end <= self.ascii_params.len() {
                    let s = &self.ascii_params[start..end];
                    // Remove trailing pipe character (GeoTIFF string terminator)
                    return Some(s.trim_end_matches('|'));
                }
            }
        }
        None
    }

    /// Gets the model type
    #[must_use]
    pub fn model_type(&self) -> Option<ModelType> {
        self.get_short(GeoKey::GtModelType)
            .and_then(ModelType::from_u16)
    }

    /// Gets the raster type
    #[must_use]
    pub fn raster_type(&self) -> Option<RasterType> {
        self.get_short(GeoKey::GtRasterType)
            .and_then(RasterType::from_u16)
    }

    /// Gets the EPSG code for the CRS
    #[must_use]
    pub fn epsg_code(&self) -> Option<u32> {
        // Try projected first, then geographic
        self.get_short(GeoKey::ProjectedCsType)
            .filter(|&v| v != 32767) // User-defined
            .map(u32::from)
            .or_else(|| {
                self.get_short(GeoKey::GeographicType)
                    .filter(|&v| v != 32767)
                    .map(u32::from)
            })
    }
}

/// Extracts the GeoTransform from GeoTIFF tags
pub fn extract_geo_transform<S: DataSource>(
    ifd: &Ifd,
    source: &S,
    byte_order: ByteOrderType,
    variant: TiffVariant,
) -> Result<Option<GeoTransform>> {
    // Try ModelTransformation first (4x4 matrix)
    if let Some(entry) = ifd.get_entry(TiffTag::ModelTransformation) {
        let values = entry.get_f64_vec(source, byte_order, variant)?;
        if values.len() >= 16 {
            // Extract affine parameters from 4x4 matrix
            // The matrix is stored row-major:
            // [a, b, c, d]    where (x', y') = (a*x + b*y + d, e*x + f*y + h)
            // [e, f, g, h]
            // [i, j, k, l]
            // [m, n, o, p]
            return Ok(Some(GeoTransform::new(
                values[3], // origin_x (d)
                values[0], // pixel_width (a)
                values[1], // row_rotation (b)
                values[7], // origin_y (h)
                values[4], // col_rotation (e)
                values[5], // pixel_height (f)
            )));
        }
    }

    // Try ModelPixelScale + ModelTiepoint
    let pixel_scale = ifd
        .get_entry(TiffTag::ModelPixelScale)
        .map(|e| e.get_f64_vec(source, byte_order, variant))
        .transpose()?;

    let tiepoint = ifd
        .get_entry(TiffTag::ModelTiepoint)
        .map(|e| e.get_f64_vec(source, byte_order, variant))
        .transpose()?;

    if let (Some(scale), Some(tie)) = (pixel_scale, tiepoint) {
        if scale.len() >= 2 && tie.len() >= 6 {
            // Scale: [ScaleX, ScaleY, ScaleZ]
            // Tiepoint: [I, J, K, X, Y, Z] (pixel I,J,K maps to geo X,Y,Z)
            let pixel_x = tie[0];
            let pixel_y = tie[1];
            let geo_x = tie[3];
            let geo_y = tie[4];

            let pixel_width = scale[0];
            let pixel_height = -scale[1]; // Negative for north-up

            let origin_x = geo_x - pixel_x * pixel_width;
            let origin_y = geo_y - pixel_y * pixel_height;

            return Ok(Some(GeoTransform::north_up(
                origin_x,
                origin_y,
                pixel_width,
                pixel_height,
            )));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_type() {
        assert_eq!(ModelType::from_u16(1), Some(ModelType::Projected));
        assert_eq!(ModelType::from_u16(2), Some(ModelType::Geographic));
        assert_eq!(ModelType::from_u16(99), None);
    }

    #[test]
    fn test_raster_type() {
        assert_eq!(RasterType::from_u16(1), Some(RasterType::PixelIsArea));
        assert_eq!(RasterType::from_u16(2), Some(RasterType::PixelIsPoint));
    }

    #[test]
    fn test_geokey_lookup() {
        let dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![
                GeoKeyEntry {
                    key_id: GeoKey::GtModelType as u16,
                    tiff_tag_location: 0,
                    count: 1,
                    value_offset: 1, // Projected
                },
                GeoKeyEntry {
                    key_id: GeoKey::ProjectedCsType as u16,
                    tiff_tag_location: 0,
                    count: 1,
                    value_offset: 32632, // UTM zone 32N
                },
            ],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        assert_eq!(dir.model_type(), Some(ModelType::Projected));
        assert_eq!(dir.epsg_code(), Some(32632));
    }
}
