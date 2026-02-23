//! Core geospatial types for `OxiGDAL`
//!
//! This module provides fundamental types used throughout the `OxiGDAL` ecosystem:
//!
//! - [`BoundingBox`] - 2D spatial extent
//! - [`BoundingBox3D`] - 3D spatial extent with elevation
//! - [`GeoTransform`] - Affine transformation for georeferencing
//! - [`RasterDataType`] - Pixel data types
//! - [`SampleInterpretation`] - How samples should be interpreted
//! - [`ColorInterpretation`] - Band color meanings
//! - [`PixelLayout`] - Memory organization of raster data
//! - [`NoDataValue`] - Representation of missing data

mod bbox;
mod data_type;
mod geo_transform;

pub use bbox::{BoundingBox, BoundingBox3D, PixelExtent};
pub use data_type::{
    ColorInterpretation, NoDataValue, PixelLayout, RasterDataType, SampleInterpretation,
};
pub use geo_transform::GeoTransform;

/// Coordinate pair (X, Y)
pub type Coordinate = (f64, f64);

/// Coordinate triple (X, Y, Z)
pub type Coordinate3D = (f64, f64, f64);

/// Size in pixels (width, height)
pub type PixelSize = (u32, u32);

/// Resolution (`x_resolution`, `y_resolution`)
pub type Resolution = (f64, f64);

/// Overview level (0 = full resolution, 1 = half, etc.)
pub type OverviewLevel = u8;

/// Band index (1-based like GDAL)
pub type BandIndex = u32;

/// Tile index (column, row)
pub type TileIndex = (u32, u32);

/// Pixel statistics
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RasterStatistics {
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Mean value
    pub mean: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Valid pixel count
    pub valid_count: u64,
}

impl RasterStatistics {
    /// Creates new raster statistics
    #[must_use]
    pub const fn new(min: f64, max: f64, mean: f64, std_dev: f64, valid_count: u64) -> Self {
        Self {
            min,
            max,
            mean,
            std_dev,
            valid_count,
        }
    }

    /// Returns the range (max - min)
    #[must_use]
    pub fn range(&self) -> f64 {
        self.max - self.min
    }
}

/// Raster metadata
#[derive(Debug, Clone, Default)]
pub struct RasterMetadata {
    /// Width in pixels
    pub width: u64,
    /// Height in pixels
    pub height: u64,
    /// Number of bands
    pub band_count: u32,
    /// Data type
    pub data_type: RasterDataType,
    /// Geotransform
    pub geo_transform: Option<GeoTransform>,
    /// CRS as WKT
    pub crs_wkt: Option<String>,
    /// `NoData` value
    pub nodata: NoDataValue,
    /// Color interpretation for each band
    pub color_interpretation: Vec<ColorInterpretation>,
    /// Pixel layout
    pub layout: PixelLayout,
    /// Driver-specific metadata
    pub driver_metadata: Vec<(String, String)>,
}

impl RasterMetadata {
    /// Returns the total pixel count
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.width * self.height
    }

    /// Returns the bounding box if geotransform is available
    #[must_use]
    pub fn bounds(&self) -> Option<BoundingBox> {
        self.geo_transform
            .map(|gt| gt.compute_bounds(self.width, self.height))
    }

    /// Returns the resolution if geotransform is available
    #[must_use]
    pub fn resolution(&self) -> Option<Resolution> {
        self.geo_transform.map(|gt| gt.resolution())
    }
}

/// Vector geometry types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometryType {
    /// Point geometry
    Point,
    /// Line string geometry
    LineString,
    /// Polygon geometry
    Polygon,
    /// Multi-point geometry
    MultiPoint,
    /// Multi-line string geometry
    MultiLineString,
    /// Multi-polygon geometry
    MultiPolygon,
    /// Geometry collection
    GeometryCollection,
    /// Unknown geometry type
    Unknown,
}

impl GeometryType {
    /// Returns true if this is a multi-geometry type
    #[must_use]
    pub const fn is_multi(&self) -> bool {
        matches!(
            self,
            Self::MultiPoint
                | Self::MultiLineString
                | Self::MultiPolygon
                | Self::GeometryCollection
        )
    }

    /// Returns the simple (non-multi) version of this type
    #[must_use]
    pub const fn to_simple(&self) -> Self {
        match self {
            Self::MultiPoint => Self::Point,
            Self::MultiLineString => Self::LineString,
            Self::MultiPolygon => Self::Polygon,
            other => *other,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn test_raster_statistics() {
        let stats = RasterStatistics::new(0.0, 255.0, 127.5, 50.0, 1_000_000);
        assert_eq!(stats.range(), 255.0);
    }

    #[test]
    fn test_raster_metadata() {
        let metadata = RasterMetadata {
            width: 1000,
            height: 500,
            band_count: 3,
            data_type: RasterDataType::UInt8,
            geo_transform: Some(GeoTransform::north_up(-180.0, 90.0, 0.36, -0.36)),
            crs_wkt: Some("GEOGCS[\"WGS 84\"]".to_string()),
            nodata: NoDataValue::Integer(0),
            ..Default::default()
        };

        assert_eq!(metadata.pixel_count(), 500_000);
        assert!(metadata.bounds().is_some());
    }

    #[test]
    fn test_geometry_type() {
        assert!(!GeometryType::Point.is_multi());
        assert!(GeometryType::MultiPoint.is_multi());
        assert_eq!(
            GeometryType::MultiPolygon.to_simple(),
            GeometryType::Polygon
        );
    }
}
