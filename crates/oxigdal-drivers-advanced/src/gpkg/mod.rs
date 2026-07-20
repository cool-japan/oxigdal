//! GeoPackage (GPKG) format driver.
//!
//! This module provides support for reading and writing GeoPackage files:
//! - Vector features with multiple geometry types
//! - Raster tile matrices
//! - Spatial indexing with R-tree
//! - GeoPackage 1.3 specification compliance
//! - Extensions support

mod connection;
pub(crate) mod geom_envelope;
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

/// Decode a `user_version` PRAGMA integer (`MMMmmmPP` per OGC GeoPackage
/// spec §1.1.1.1.1) into the corresponding [`GpkgVersion`].
fn decode_user_version(user_version: i64) -> Result<GpkgVersion> {
    match user_version {
        10000 => Ok(GpkgVersion::V1_0),
        10100 => Ok(GpkgVersion::V1_1),
        10200 => Ok(GpkgVersion::V1_2),
        10300 => Ok(GpkgVersion::V1_3),
        // Forward-compatible: an unrecognized but plausible 1.x patch/point
        // release (e.g. a future 1.3.x or 1.4 written by another tool) is
        // reported as the newest version this crate understands rather than
        // erroring, since the gpkg_* table schemas are additive across
        // point releases.
        v if (10000..10400).contains(&v) => Ok(GpkgVersion::V1_3),
        v => Err(Error::geopackage(format!(
            "unrecognized GeoPackage user_version: {v}"
        ))),
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

    /// Detect GeoPackage version from the on-disk SQLite header, per the OGC
    /// GeoPackage spec §1.1.1.1.1 Requirement 2.
    ///
    /// GeoPackage 1.2+ writers set `application_id` to `'GPKG'`
    /// (`0x47504B47`) and encode the specification version into
    /// `user_version` as `MMMmmmPP` (e.g. `10300` = 1.3.0). GeoPackage
    /// 1.0/1.1 predate that convention and instead use the `'GP10'`
    /// (`0x47503130`) application_id marker for both versions; the spec
    /// provides no further header-level signal to distinguish them, so the
    /// higher of the two (1.1) is reported.
    fn detect_version(connection: &GpkgConnection) -> Result<GpkgVersion> {
        const GPKG_APPLICATION_ID: i64 = 0x4750_4B47; // 'GPKG'
        const GP10_APPLICATION_ID: i64 = 0x4750_3130; // 'GP10' (1.0/1.1 marker)

        let application_id = connection.query_scalar_i64("PRAGMA application_id;")?;

        if application_id == GPKG_APPLICATION_ID {
            let user_version = connection.query_scalar_i64("PRAGMA user_version;")?;
            return decode_user_version(user_version);
        }

        if application_id == GP10_APPLICATION_ID {
            return Ok(GpkgVersion::V1_1);
        }

        Err(Error::geopackage(format!(
            "not a recognized GeoPackage: unexpected application_id 0x{application_id:08X}"
        )))
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

    #[test]
    fn test_decode_user_version() {
        assert_eq!(decode_user_version(10000).ok(), Some(GpkgVersion::V1_0));
        assert_eq!(decode_user_version(10100).ok(), Some(GpkgVersion::V1_1));
        assert_eq!(decode_user_version(10200).ok(), Some(GpkgVersion::V1_2));
        assert_eq!(decode_user_version(10300).ok(), Some(GpkgVersion::V1_3));
        // Unknown but plausible 1.x point release: forward-compatible fallback.
        assert_eq!(decode_user_version(10310).ok(), Some(GpkgVersion::V1_3));
        // Nonsense value: genuine error, not a silent default.
        assert!(decode_user_version(99999).is_err());
    }

    /// Regression test: `detect_version` must actually read the on-disk
    /// `user_version`, not hardcode 1.3 regardless of file contents.
    #[test]
    fn test_detect_version_reads_actual_user_version() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;

        // create() already wrote application_id='GPKG' + user_version=10300;
        // overwrite user_version to simulate a GeoPackage 1.2 file.
        conn.execute_batch("PRAGMA user_version = 10200;")?;

        let version = GeoPackage::detect_version(&conn)?;
        assert_eq!(version, GpkgVersion::V1_2);
        Ok(())
    }

    /// A file carrying the legacy 'GP10' application_id (1.0/1.1, which the
    /// spec provides no further way to disambiguate from the header alone)
    /// must report the newer of the two candidates rather than 1.3.
    #[test]
    fn test_detect_version_gp10_marker_reports_v1_1() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;

        conn.execute_batch("PRAGMA application_id = 0x47503130;")?;

        let version = GeoPackage::detect_version(&conn)?;
        assert_eq!(version, GpkgVersion::V1_1);
        Ok(())
    }

    /// A file whose application_id is neither 'GPKG' nor 'GP10' is not a
    /// recognized GeoPackage and must error rather than silently reporting
    /// a fabricated 1.3.
    #[test]
    fn test_detect_version_unrecognized_application_id_errors() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;

        conn.execute_batch("PRAGMA application_id = 0;")?;

        assert!(GeoPackage::detect_version(&conn).is_err());
        Ok(())
    }

    #[test]
    fn test_gpkg_open_rw_detects_real_version() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        {
            let _gpkg = GeoPackage::create(temp_file.path())?;
        }
        let gpkg = GeoPackage::open_rw(temp_file.path())?;
        assert_eq!(gpkg.version(), GpkgVersion::V1_3);
        Ok(())
    }
}
