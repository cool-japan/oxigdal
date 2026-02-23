//! # DataFormat - Trait Implementations
//!
//! This module contains trait implementations for `DataFormat`.
//!
//! ## Implemented Traits
//!
//! - `Display`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::DataFormat;

impl std::fmt::Display for DataFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GeoTiff => write!(f, "GeoTIFF"),
            Self::Cog => write!(f, "COG"),
            Self::NetCdf => write!(f, "NetCDF"),
            Self::Hdf5 => write!(f, "HDF5"),
            Self::GeoJson => write!(f, "GeoJSON"),
            Self::Shapefile => write!(f, "Shapefile"),
            Self::GeoPackage => write!(f, "GeoPackage"),
            Self::FlatGeobuf => write!(f, "FlatGeobuf"),
            Self::GeoParquet => write!(f, "GeoParquet"),
            Self::PostGis => write!(f, "PostGIS"),
            Self::Zarr => write!(f, "Zarr"),
        }
    }
}
