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
    ddl_feature_table_with_extra_columns(table_name, &[])
}

/// Generate the CREATE TABLE DDL for a user feature table, additionally
/// declaring one `BLOB` column per name in `extra_column_names`.
///
/// This keeps the emitted `sqlite_master.sql` DDL consistent with any
/// additional geometry columns registered against this table via
/// [`crate::writer::builder::GeoPackageBuilder::add_geometry_column_def`] —
/// without this, `gpkg_geometry_columns` would reference a column that the
/// table's own `CREATE TABLE` statement never declares (a non-conformant,
/// internally inconsistent GeoPackage).
///
/// Row values for these extra columns are always written as `NULL` by the
/// current writer (there is no per-row value API for them yet); the DDL still
/// declares the column so metadata and schema agree, and callers can `UPDATE`
/// the column post-hoc via any SQLite-compatible tool.
pub fn ddl_feature_table_with_extra_columns(
    table_name: &str,
    extra_column_names: &[&str],
) -> String {
    let mut ddl = format!(
        "CREATE TABLE {table_name} (\n\
  fid INTEGER PRIMARY KEY AUTOINCREMENT,\n\
  geom BLOB"
    );
    for name in extra_column_names {
        ddl.push_str(",\n  ");
        ddl.push_str(name);
        ddl.push_str(" BLOB");
    }
    ddl.push_str("\n)");
    ddl
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

/// The three SRS ids the GeoPackage writer always emits regardless of what
/// the caller registers: -1 (undefined cartesian), 0 (undefined geographic),
/// 4326 (WGS 84).
pub const MANDATORY_SRS_IDS: [i32; 3] = [-1, 0, 4326];

// ─────────────────────────────────────────────────────────────────────────────
// Custom SRS rows — caller-supplied EPSG codes beyond the three mandatory ids
// ─────────────────────────────────────────────────────────────────────────────

/// A caller-supplied `gpkg_spatial_ref_sys` row for a custom (non-default)
/// coordinate reference system, registered via
/// [`crate::writer::builder::GeoPackageBuilder::add_custom_srs`].
///
/// Unlike [`DefaultSrs`], every field is an owned [`String`] since the value
/// is supplied at runtime rather than known at compile time.
#[derive(Debug, Clone)]
pub struct CustomSrs {
    /// Human-readable name of the SRS.
    pub srs_name: String,
    /// Numeric SRS identifier (primary key). Must not collide with a
    /// [`MANDATORY_SRS_IDS`] value or another registered custom SRS.
    pub srs_id: i32,
    /// Defining organisation (e.g. `"EPSG"`).
    pub organization: String,
    /// Organisation-assigned CRS code.
    pub organization_coordsys_id: i64,
    /// WKT definition of the SRS.
    pub definition: String,
    /// Human-readable description.
    pub description: String,
}

impl CustomSrs {
    /// Convenience constructor for a standard EPSG-authority SRS.
    ///
    /// Sets `organization = "EPSG"` and `organization_coordsys_id = srs_id`,
    /// which holds for the overwhelming majority of real-world EPSG codes
    /// (the code and the CRS's own id in the EPSG registry coincide).
    pub fn epsg(srs_id: i32, srs_name: impl Into<String>, definition: impl Into<String>) -> Self {
        Self {
            srs_name: srs_name.into(),
            srs_id,
            organization: "EPSG".to_string(),
            organization_coordsys_id: i64::from(srs_id),
            definition: definition.into(),
            description: String::new(),
        }
    }

    /// Builder-pattern setter for [`Self::description`].
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}
