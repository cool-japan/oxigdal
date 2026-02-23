//! GeoPackage database schema management.

use super::connection::GpkgConnection;
use crate::error::Result;

/// GeoPackage table type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableType {
    /// Feature (vector) table
    Features,
    /// Tile (raster) table
    Tiles,
    /// Attribute table
    Attributes,
}

impl TableType {
    /// Get type as string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Features => "features",
            Self::Tiles => "tiles",
            Self::Attributes => "attributes",
        }
    }
}

/// Content information from gpkg_contents table.
#[derive(Debug, Clone)]
pub struct ContentInfo {
    /// Table name
    pub table_name: String,
    /// Data type
    pub data_type: TableType,
    /// Identifier
    pub identifier: Option<String>,
    /// Description
    pub description: Option<String>,
    /// SRS ID
    pub srs_id: i32,
    /// Minimum X
    pub min_x: Option<f64>,
    /// Minimum Y
    pub min_y: Option<f64>,
    /// Maximum X
    pub max_x: Option<f64>,
    /// Maximum Y
    pub max_y: Option<f64>,
}

/// Initialize GeoPackage schema.
pub fn initialize_schema(conn: &GpkgConnection) -> Result<()> {
    conn.execute_batch(GPKG_SPATIAL_REF_SYS_TABLE)?;
    conn.execute_batch(GPKG_CONTENTS_TABLE)?;
    conn.execute_batch(GPKG_GEOMETRY_COLUMNS_TABLE)?;
    conn.execute_batch(GPKG_TILE_MATRIX_SET_TABLE)?;
    conn.execute_batch(GPKG_TILE_MATRIX_TABLE)?;
    conn.execute_batch(GPKG_EXTENSIONS_TABLE)?;

    // Insert required SRS entries
    insert_required_srs(conn)?;

    Ok(())
}

/// Insert required SRS definitions.
fn insert_required_srs(conn: &GpkgConnection) -> Result<()> {
    // EPSG:4326 (WGS 84)
    conn.execute(
        "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_name, srs_id, organization, organization_coordsys_id, definition, description) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            &"WGS 84" as &dyn rusqlite::ToSql,
            &4326,
            &"EPSG",
            &4326,
            &"GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563]],PRIMEM[\"Greenwich\",0],UNIT[\"degree\",0.0174532925199433]]",
            &"WGS 84 geographic coordinate system",
        ],
    )?;

    // Undefined Cartesian SRS
    conn.execute(
        "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_name, srs_id, organization, organization_coordsys_id, definition, description) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            &"Undefined Cartesian SRS" as &dyn rusqlite::ToSql,
            &-1,
            &"NONE",
            &-1,
            &"undefined",
            &"undefined cartesian coordinate reference system",
        ],
    )?;

    // Undefined Geographic SRS
    conn.execute(
        "INSERT OR IGNORE INTO gpkg_spatial_ref_sys (srs_name, srs_id, organization, organization_coordsys_id, definition, description) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            &"Undefined Geographic SRS" as &dyn rusqlite::ToSql,
            &0,
            &"NONE",
            &0,
            &"undefined",
            &"undefined geographic coordinate reference system",
        ],
    )?;

    Ok(())
}

const GPKG_SPATIAL_REF_SYS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS gpkg_spatial_ref_sys (
    srs_name TEXT NOT NULL,
    srs_id INTEGER NOT NULL PRIMARY KEY,
    organization TEXT NOT NULL,
    organization_coordsys_id INTEGER NOT NULL,
    definition TEXT NOT NULL,
    description TEXT
);
"#;

const GPKG_CONTENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS gpkg_contents (
    table_name TEXT NOT NULL PRIMARY KEY,
    data_type TEXT NOT NULL,
    identifier TEXT UNIQUE,
    description TEXT DEFAULT '',
    last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    min_x DOUBLE,
    min_y DOUBLE,
    max_x DOUBLE,
    max_y DOUBLE,
    srs_id INTEGER,
    CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
);
"#;

const GPKG_GEOMETRY_COLUMNS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS gpkg_geometry_columns (
    table_name TEXT NOT NULL,
    column_name TEXT NOT NULL,
    geometry_type_name TEXT NOT NULL,
    srs_id INTEGER NOT NULL,
    z TINYINT NOT NULL,
    m TINYINT NOT NULL,
    CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),
    CONSTRAINT fk_gc_tn FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
    CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
);
"#;

const GPKG_TILE_MATRIX_SET_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS gpkg_tile_matrix_set (
    table_name TEXT NOT NULL PRIMARY KEY,
    srs_id INTEGER NOT NULL,
    min_x DOUBLE NOT NULL,
    min_y DOUBLE NOT NULL,
    max_x DOUBLE NOT NULL,
    max_y DOUBLE NOT NULL,
    CONSTRAINT fk_gtms_table_name FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
    CONSTRAINT fk_gtms_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
);
"#;

const GPKG_TILE_MATRIX_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS gpkg_tile_matrix (
    table_name TEXT NOT NULL,
    zoom_level INTEGER NOT NULL,
    matrix_width INTEGER NOT NULL,
    matrix_height INTEGER NOT NULL,
    tile_width INTEGER NOT NULL,
    tile_height INTEGER NOT NULL,
    pixel_x_size DOUBLE NOT NULL,
    pixel_y_size DOUBLE NOT NULL,
    CONSTRAINT pk_ttm PRIMARY KEY (table_name, zoom_level),
    CONSTRAINT fk_tmm_table_name FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name)
);
"#;

const GPKG_EXTENSIONS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS gpkg_extensions (
    table_name TEXT,
    column_name TEXT,
    extension_name TEXT NOT NULL,
    definition TEXT NOT NULL,
    scope TEXT NOT NULL,
    CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name)
);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_table_type() {
        assert_eq!(TableType::Features.as_str(), "features");
        assert_eq!(TableType::Tiles.as_str(), "tiles");
    }

    #[test]
    fn test_schema_initialization() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(crate::error::Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        initialize_schema(&conn)?;

        assert!(conn.table_exists("gpkg_spatial_ref_sys")?);
        assert!(conn.table_exists("gpkg_contents")?);
        assert!(conn.table_exists("gpkg_geometry_columns")?);

        Ok(())
    }
}
