//! GeoPackage (GPKG) format driver.
//!
//! This module provides support for reading and writing GeoPackage files:
//! - Vector features with multiple geometry types
//! - Raster tile matrices
//! - Spatial indexing with R-tree
//! - GeoPackage 1.3 specification compliance
//! - Extensions support

mod connection;
mod metadata;
mod raster;
mod schema;
mod spatial_index;
mod vector;

pub use connection::{ConnectionMode, GpkgConnection};
pub use metadata::{Extent, GpkgMetadata, Srs};
pub use raster::{Tile, TileMatrix, TileMatrixSet};
pub use schema::{ContentInfo, TableType};
pub use spatial_index::{RTreeIndex, SpatialIndex};
pub use vector::{Feature, FeatureTable, GeometryType};

use crate::error::{Error, Result};
use std::path::Path;
use std::str::FromStr;

/// GeoPackage version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpkgVersion {
    /// Version 1.0
    V1_0,
    /// Version 1.1
    V1_1,
    /// Version 1.2
    V1_2,
    /// Version 1.3
    V1_3,
}

impl GpkgVersion {
    /// Get version string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::V1_0 => "1.0",
            Self::V1_1 => "1.1",
            Self::V1_2 => "1.2",
            Self::V1_3 => "1.3",
        }
    }
}

impl FromStr for GpkgVersion {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "1.0" => Ok(Self::V1_0),
            "1.1" => Ok(Self::V1_1),
            "1.2" => Ok(Self::V1_2),
            "1.3" => Ok(Self::V1_3),
            _ => Err(Error::geopackage(format!(
                "Unknown GeoPackage version: {}",
                s
            ))),
        }
    }
}

/// GeoPackage file handle.
pub struct GeoPackage {
    connection: GpkgConnection,
    version: GpkgVersion,
    metadata: GpkgMetadata,
}

impl GeoPackage {
    /// Open an existing GeoPackage file.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let connection = GpkgConnection::open(path, ConnectionMode::ReadOnly)?;
        let version = Self::detect_version(&connection)?;
        let metadata = GpkgMetadata::read(&connection)?;

        Ok(Self {
            connection,
            version,
            metadata,
        })
    }

    /// Create a new GeoPackage file.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let connection = GpkgConnection::create(path)?;
        let version = GpkgVersion::V1_3;

        // Initialize GeoPackage schema
        schema::initialize_schema(&connection)?;

        let metadata = GpkgMetadata::default();

        Ok(Self {
            connection,
            version,
            metadata,
        })
    }

    /// Open with read-write access.
    pub fn open_rw<P: AsRef<Path>>(path: P) -> Result<Self> {
        let connection = GpkgConnection::open(path, ConnectionMode::ReadWrite)?;
        let version = Self::detect_version(&connection)?;
        let metadata = GpkgMetadata::read(&connection)?;

        Ok(Self {
            connection,
            version,
            metadata,
        })
    }

    /// Detect GeoPackage version.
    fn detect_version(_connection: &GpkgConnection) -> Result<GpkgVersion> {
        // Query gpkg_spatial_ref_sys table for version info
        // For now, default to 1.3
        Ok(GpkgVersion::V1_3)
    }

    /// Get GeoPackage version.
    pub fn version(&self) -> GpkgVersion {
        self.version
    }

    /// Get metadata.
    pub fn metadata(&self) -> &GpkgMetadata {
        &self.metadata
    }

    /// List all feature tables.
    pub fn feature_tables(&self) -> Result<Vec<String>> {
        self.connection.list_tables(TableType::Features)
    }

    /// List all tile matrix sets.
    pub fn tile_matrix_sets(&self) -> Result<Vec<String>> {
        self.connection.list_tables(TableType::Tiles)
    }

    /// Open a feature table.
    pub fn open_feature_table(&self, name: &str) -> Result<FeatureTable> {
        FeatureTable::open(&self.connection, name)
    }

    /// Create a feature table.
    pub fn create_feature_table(
        &mut self,
        name: &str,
        geometry_type: GeometryType,
        srs_id: i32,
    ) -> Result<FeatureTable> {
        FeatureTable::create(&self.connection, name, geometry_type, srs_id)
    }

    /// Open a tile matrix set.
    pub fn open_tile_matrix_set(&self, name: &str) -> Result<TileMatrixSet> {
        TileMatrixSet::open(&self.connection, name)
    }

    /// Create a tile matrix set.
    pub fn create_tile_matrix_set(
        &mut self,
        name: &str,
        srs_id: i32,
        extent: Extent,
    ) -> Result<TileMatrixSet> {
        TileMatrixSet::create(&self.connection, name, srs_id, extent)
    }

    /// Get database connection.
    pub fn connection(&self) -> &GpkgConnection {
        &self.connection
    }

    /// Flush changes to disk.
    pub fn flush(&mut self) -> Result<()> {
        self.connection.flush()
    }

    /// Vacuum database (compact and optimize).
    pub fn vacuum(&mut self) -> Result<()> {
        self.connection.vacuum()
    }

    /// Check database integrity.
    pub fn check_integrity(&self) -> Result<bool> {
        self.connection.check_integrity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use tempfile::NamedTempFile;

    #[test]
    fn test_gpkg_version() {
        assert_eq!(GpkgVersion::V1_3.as_str(), "1.3");
        let v = GpkgVersion::from_str("1.3");
        assert!(v.is_ok());
        if let Ok(ver) = v {
            assert_eq!(ver, GpkgVersion::V1_3);
        }
        assert!(GpkgVersion::from_str("2.0").is_err());
    }

    #[test]
    fn test_gpkg_creation() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let gpkg = GeoPackage::create(temp_file.path())?;
        assert_eq!(gpkg.version(), GpkgVersion::V1_3);
        Ok(())
    }

    #[test]
    fn test_gpkg_tables() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let gpkg = GeoPackage::create(temp_file.path())?;
        let tables = gpkg.feature_tables()?;
        assert!(tables.is_empty());
        Ok(())
    }
}
