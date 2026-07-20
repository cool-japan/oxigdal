//! Verbatim DDL strings and default row data for the OGC GeoPackage system tables.
//!
//! All strings here are taken from OGC 12-128r19 and must match exactly what
//! SQLite will write into `sqlite_master` when the GeoPackage is created by a
//! conforming implementation.

// ─────────────────────────────────────────────────────────────────────────────
// DDL strings — stored verbatim in sqlite_master.sql column
// ─────────────────────────────────────────────────────────────────────────────

/// CREATE TABLE DDL for `gpkg_spatial_ref_sys` (OGC 12-128r19 §1.1.2).
pub const DDL_GPKG_SPATIAL_REF_SYS: &str = "CREATE TABLE gpkg_spatial_ref_sys (\n\
  srs_name TEXT NOT NULL,\n\
  srs_id INTEGER NOT NULL PRIMARY KEY,\n\
  organization TEXT NOT NULL,\n\
  organization_coordsys_id INTEGER NOT NULL,\n\
  definition TEXT NOT NULL,\n\
  description TEXT\n\
)";

/// CREATE TABLE DDL for `gpkg_contents` (OGC 12-128r19 §1.1.3).
pub const DDL_GPKG_CONTENTS: &str = "CREATE TABLE gpkg_contents (\n\
  table_name TEXT NOT NULL PRIMARY KEY,\n\
  data_type TEXT NOT NULL,\n\
  identifier TEXT UNIQUE,\n\
  description TEXT DEFAULT '',\n\
  last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),\n\
  min_x DOUBLE,\n\
  min_y DOUBLE,\n\
  max_x DOUBLE,\n\
  max_y DOUBLE,\n\
  srs_id INTEGER,\n\
  CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)\n\
)";

/// CREATE TABLE DDL for `gpkg_geometry_columns` (OGC 12-128r19 §2.1.5).
pub const DDL_GPKG_GEOMETRY_COLUMNS: &str = "CREATE TABLE gpkg_geometry_columns (\n\
  table_name TEXT NOT NULL,\n\
  column_name TEXT NOT NULL,\n\
  geometry_type_name TEXT NOT NULL,\n\
  srs_id INTEGER NOT NULL,\n\
  z TINYINT NOT NULL,\n\
  m TINYINT NOT NULL,\n\
  CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),\n\
  CONSTRAINT uk_gc_table_name UNIQUE (table_name),\n\
  CONSTRAINT fk_gc_tn FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),\n\
  CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)\n\
)";

/// Generate the CREATE TABLE DDL for a user feature table.
///
/// Produces a minimal table with an integer primary key `fid` and a single
/// geometry column named `geom`.
pub fn ddl_feature_table(table_name: &str) -> String {
    format!(
        "CREATE TABLE {table_name} (\n\
  fid INTEGER PRIMARY KEY AUTOINCREMENT,\n\
  geom BLOB\n\
)"
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Default SRS rows — OGC 12-128r19 Requirement 11
// ─────────────────────────────────────────────────────────────────────────────

/// A default SRS row for `gpkg_spatial_ref_sys`.
///
/// Columns: `(srs_name, srs_id, organization, organization_coordsys_id,
///            definition, description)`.
pub struct DefaultSrs {
    /// Human-readable name of the SRS.
    pub srs_name: &'static str,
    /// Numeric SRS identifier (primary key).
    pub srs_id: i64,
    /// Defining organisation (e.g. `"EPSG"`).
    pub organization: &'static str,
    /// Organisation-assigned CRS code.
    pub organization_coordsys_id: i64,
    /// WKT definition of the SRS (or `"undefined"` for the two mandatory stubs).
    pub definition: &'static str,
    /// Human-readable description.
    pub description: &'static str,
}

/// Undefined cartesian SRS (srs_id = −1).
pub const SRS_UNDEFINED_CARTESIAN: DefaultSrs = DefaultSrs {
    srs_name: "Undefined cartesian SRS",
    srs_id: -1,
    organization: "NONE",
    organization_coordsys_id: -1,
    definition: "undefined",
    description: "undefined cartesian coordinate reference system",
};

/// Undefined geographic SRS (srs_id = 0).
pub const SRS_UNDEFINED_GEOGRAPHIC: DefaultSrs = DefaultSrs {
    srs_name: "Undefined geographic SRS",
    srs_id: 0,
    organization: "NONE",
    organization_coordsys_id: 0,
    definition: "undefined",
    description: "undefined geographic coordinate reference system",
};

/// WGS 84 geodetic SRS (srs_id = 4326, EPSG).
pub const SRS_WGS84: DefaultSrs = DefaultSrs {
    srs_name: "WGS 84 geodetic",
    srs_id: 4326,
    organization: "EPSG",
    organization_coordsys_id: 4326,
    definition: "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",\
                 SPHEROID[\"WGS 84\",6378137,298.257223563,\
                 AUTHORITY[\"EPSG\",\"7030\"]],\
                 AUTHORITY[\"EPSG\",\"6326\"]],\
                 PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],\
                 UNIT[\"degree\",0.0174532925199433,\
                 AUTHORITY[\"EPSG\",\"9122\"]],\
                 AUTHORITY[\"EPSG\",\"4326\"]]",
    description: "longitude/latitude coordinates in decimal degrees on the WGS 84 spheroid",
};

/// Return all three default SRS rows in OGC-mandated rowid order.
pub fn default_srs_rows() -> [&'static DefaultSrs; 3] {
    [
        &SRS_UNDEFINED_CARTESIAN,
        &SRS_UNDEFINED_GEOGRAPHIC,
        &SRS_WGS84,
    ]
}
