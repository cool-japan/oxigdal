//! GeoPackage schema layer.
//!
//! Provides typed representations of the core GeoPackage tables defined by the
//! OGC GeoPackage Encoding Standard v1.3.1.

use crate::error::GpkgError;
use crate::sqlite_reader::SqliteReader;

/// The content type stored in a GeoPackage table.
#[derive(Debug, Clone, PartialEq)]
pub enum GpkgDataType {
    /// OGC Simple Features vector data.
    Features,
    /// Raster tile pyramid (imagery or elevation).
    Tiles,
    /// Non-spatial attribute data.
    Attributes,
}

impl GpkgDataType {
    /// Parse the `data_type` column value from `gpkg_contents`.
    ///
    /// Unknown strings fall back to [`GpkgDataType::Features`].
    pub fn parse_type(s: &str) -> Self {
        match s {
            "features" => Self::Features,
            "tiles" => Self::Tiles,
            "attributes" => Self::Attributes,
            _ => Self::Features,
        }
    }

    /// Return the canonical string used in `gpkg_contents`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Features => "features",
            Self::Tiles => "tiles",
            Self::Attributes => "attributes",
        }
    }
}

/// A row from the `gpkg_contents` table.
#[derive(Debug, Clone)]
pub struct GpkgContents {
    /// Name of the user-data table.
    pub table_name: String,
    /// Logical content type.
    pub data_type: GpkgDataType,
    /// Human-readable identifier (may be the same as `table_name`).
    pub identifier: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// Bounding box — western longitude.
    pub min_x: f64,
    /// Bounding box — southern latitude.
    pub min_y: f64,
    /// Bounding box — eastern longitude.
    pub max_x: f64,
    /// Bounding box — northern latitude.
    pub max_y: f64,
    /// Spatial reference system ID (references `gpkg_spatial_ref_sys`).
    pub srs_id: i32,
}

/// A row from the `gpkg_geometry_columns` table.
#[derive(Debug, Clone)]
pub struct GpkgGeometryColumn {
    /// User-data table that owns this geometry column.
    pub table_name: String,
    /// Name of the geometry column in `table_name`.
    pub column_name: String,
    /// OGC geometry type name, e.g. `"POINT"`, `"MULTIPOLYGON"`.
    pub geometry_type_name: String,
    /// Spatial reference system ID.
    pub srs_id: i32,
    /// Z coordinate rule: 0 = prohibited, 1 = mandatory, 2 = optional.
    pub z: u8,
    /// M coordinate rule: 0 = prohibited, 1 = mandatory, 2 = optional.
    pub m: u8,
}

/// A row from the `gpkg_spatial_ref_sys` table.
#[derive(Debug, Clone)]
pub struct GpkgSrs {
    /// Human-readable name of the SRS.
    pub srs_name: String,
    /// Numeric SRS identifier (primary key).
    pub srs_id: i32,
    /// Defining organisation (e.g. `"EPSG"`).
    pub organization: String,
    /// Organisation-assigned CRS code.
    pub organization_coordsys_id: i32,
    /// WKT definition of the SRS.
    pub definition: String,
    /// Optional human-readable description.
    pub description: Option<String>,
}

/// A parsed GeoPackage file.
///
/// Wraps the underlying [`SqliteReader`] and exposes GeoPackage-specific
/// metadata discovered from the standard system tables.
pub struct GeoPackage {
    /// Low-level SQLite reader.
    pub reader: SqliteReader,
    /// Rows from `gpkg_contents` populated by [`GeoPackage::scan_contents`].
    pub contents: Vec<GpkgContents>,
}

impl GeoPackage {
    /// Open a GeoPackage from its raw file bytes.
    ///
    /// # Errors
    /// Returns an error when the bytes do not represent a valid SQLite file.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, GpkgError> {
        let reader = SqliteReader::from_bytes(data)?;
        Ok(Self {
            reader,
            contents: Vec::new(),
        })
    }

    /// Return `true` when the file appears to be a well-formed GeoPackage.
    ///
    /// Accepts files whose application_id matches `"GPKG"` *or* whose SQLite
    /// structure is valid, to accommodate pre-1.2 files.
    pub fn is_valid_gpkg(&self) -> bool {
        self.reader.header.is_geopackage() || self.reader.is_valid()
    }

    /// Return the page size of the underlying SQLite database.
    pub fn page_size(&self) -> u32 {
        self.reader.header.page_size
    }

    /// Return the total page count of the underlying SQLite database.
    pub fn page_count(&self) -> u32 {
        self.reader.page_count()
    }

    /// Return `true` if the `application_id` field equals the GeoPackage magic.
    pub fn has_gpkg_application_id(&self) -> bool {
        self.reader.header.application_id == 0x4750_4B47
    }
}
