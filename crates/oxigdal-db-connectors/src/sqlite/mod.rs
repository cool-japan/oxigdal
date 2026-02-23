//! SQLite/SpatiaLite spatial database connector.
//!
//! Provides support for reading and writing spatial data to SQLite databases
//! with SpatiaLite extension.

pub mod reader;
pub mod writer;

use crate::error::{Error, Result};
use geo_types::Geometry;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// SQLite connector configuration.
#[derive(Debug, Clone)]
pub struct SqliteConfig {
    /// Database file path.
    pub path: PathBuf,
    /// Enable SpatiaLite extension.
    pub spatialite: bool,
    /// Open read-only.
    pub read_only: bool,
    /// Create if not exists.
    pub create: bool,
    /// Enable WAL mode.
    pub wal_mode: bool,
    /// Cache size in KB.
    pub cache_size: i32,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from(":memory:"),
            spatialite: true,
            read_only: false,
            create: true,
            wal_mode: true,
            cache_size: 10240, // 10MB
        }
    }
}

/// SQLite spatial database connector.
pub struct SqliteConnector {
    conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    config: SqliteConfig,
}

impl SqliteConnector {
    /// Create a new SQLite connector.
    pub fn new(config: SqliteConfig) -> Result<Self> {
        let mut flags = OpenFlags::empty();

        if config.read_only {
            flags.insert(OpenFlags::SQLITE_OPEN_READ_ONLY);
        } else {
            flags.insert(OpenFlags::SQLITE_OPEN_READ_WRITE);
            if config.create {
                flags.insert(OpenFlags::SQLITE_OPEN_CREATE);
            }
        }

        let conn = Connection::open_with_flags(&config.path, flags)?;

        // Enable WAL mode only for file-based databases (not in-memory)
        // WAL mode doesn't work with in-memory databases and causes deadlocks
        let is_memory = config.path.to_str() == Some(":memory:");
        if config.wal_mode && !config.read_only && !is_memory {
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }

        // Set cache size
        conn.pragma_update(None, "cache_size", -config.cache_size)?;

        // Enable foreign keys
        conn.pragma_update(None, "foreign_keys", true)?;

        // Load SpatiaLite extension if requested
        if config.spatialite {
            // Try to load SpatiaLite extension
            // Note: This may fail if SpatiaLite is not installed
            // We ignore errors since SpatiaLite might not be available
            let _ =
                conn.query_row::<i32, _, _>("SELECT InitSpatialMetadata(1)", [], |row| row.get(0));
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            config,
        })
    }

    /// Open an in-memory database.
    pub fn memory() -> Result<Self> {
        Self::new(SqliteConfig::default())
    }

    /// Open a file-based database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config = SqliteConfig {
            path: path.as_ref().to_path_buf(),
            ..Default::default()
        };
        Self::new(config)
    }

    /// Get a connection lock.
    pub fn conn(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    /// Check if the connection is healthy.
    pub fn health_check(&self) -> Result<bool> {
        let conn = self.conn();
        let result: i32 = conn.query_row("SELECT 1", [], |row| row.get(0))?;
        Ok(result == 1)
    }

    /// Get database version.
    pub fn version(&self) -> Result<String> {
        let conn = self.conn();
        let version: String = conn.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
        Ok(version)
    }

    /// Check if SpatiaLite is loaded.
    pub fn has_spatialite(&self) -> Result<bool> {
        let conn = self.conn();
        let result = conn.query_row("SELECT spatialite_version()", [], |row| {
            let version: String = row.get(0)?;
            Ok(version)
        });

        Ok(result.is_ok())
    }

    /// Initialize spatial metadata.
    pub fn init_spatial_metadata(&self) -> Result<()> {
        let conn = self.conn();
        // InitSpatialMetadata returns a result, so use query_row instead of execute
        let _: i32 = conn.query_row("SELECT InitSpatialMetadata(1)", [], |row| row.get(0))?;
        Ok(())
    }

    /// Create a spatial table.
    pub fn create_spatial_table(
        &self,
        table_name: &str,
        geometry_column: &str,
        geometry_type: &str,
        srid: i32,
        additional_columns: &[(String, String)],
    ) -> Result<()> {
        // Check for SpatiaLite availability BEFORE acquiring the lock
        let has_spatialite = self.has_spatialite()?;

        let conn = self.conn();

        // Create the main table
        let mut columns = vec!["id INTEGER PRIMARY KEY AUTOINCREMENT".to_string()];
        for (col_name, col_type) in additional_columns {
            columns.push(format!("{} {}", col_name, col_type));
        }

        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            table_name,
            columns.join(", ")
        );

        conn.execute(&create_sql, [])?;

        // Add geometry column using SpatiaLite function
        if has_spatialite {
            let add_geom_sql = format!(
                "SELECT AddGeometryColumn('{}', '{}', {}, '{}', 'XY')",
                table_name, geometry_column, srid, geometry_type
            );

            // AddGeometryColumn returns a result, so use query_row instead of execute
            let _: i32 = conn.query_row(&add_geom_sql, [], |row| row.get(0))?;

            // Create spatial index
            let create_index_sql = format!(
                "SELECT CreateSpatialIndex('{}', '{}')",
                table_name, geometry_column
            );

            // CreateSpatialIndex returns a result, so use query_row instead of execute
            let _: i32 = conn.query_row(&create_index_sql, [], |row| row.get(0))?;
        } else {
            // Fallback without SpatiaLite
            let alter_sql = format!(
                "ALTER TABLE {} ADD COLUMN {} BLOB",
                table_name, geometry_column
            );
            conn.execute(&alter_sql, [])?;
        }

        Ok(())
    }

    /// Drop a table.
    pub fn drop_table(&self, table_name: &str) -> Result<()> {
        // Check for SpatiaLite availability BEFORE acquiring the lock
        let has_spatialite = self.has_spatialite()?;

        let conn = self.conn();

        // Drop spatial index if it exists
        if has_spatialite {
            // DisableSpatialIndex returns a result, so use query_row instead of execute
            let _: std::result::Result<i32, _> = conn.query_row(
                &format!("SELECT DisableSpatialIndex('{}', 'geometry')", table_name),
                [],
                |row| row.get(0),
            );
        }

        let sql = format!("DROP TABLE IF EXISTS {}", table_name);
        conn.execute(&sql, [])?;

        Ok(())
    }

    /// List all tables.
    pub fn list_tables(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )?;

        let tables = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;

        Ok(tables)
    }

    /// Get table schema.
    pub fn table_schema(&self, table_name: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let sql = format!("PRAGMA table_info({})", table_name);

        let mut stmt = conn.prepare(&sql)?;
        let schema = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let type_: String = row.get(2)?;
                Ok((name, type_))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(schema)
    }

    /// Execute raw SQL.
    pub fn execute(&self, sql: &str) -> Result<usize> {
        let conn = self.conn();
        let rows = conn.execute(sql, [])?;
        Ok(rows)
    }

    /// Begin a transaction.
    pub fn begin_transaction(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("BEGIN TRANSACTION", [])?;
        Ok(())
    }

    /// Commit a transaction.
    pub fn commit_transaction(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("COMMIT", [])?;
        Ok(())
    }

    /// Rollback a transaction.
    pub fn rollback_transaction(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("ROLLBACK", [])?;
        Ok(())
    }

    /// Vacuum the database.
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("VACUUM", [])?;
        Ok(())
    }

    /// Analyze the database for query optimization.
    pub fn analyze(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("ANALYZE", [])?;
        Ok(())
    }
}

/// Convert geo-types Geometry to WKB bytes.
pub fn geometry_to_wkb(geom: &Geometry<f64>) -> Result<Vec<u8>> {
    use std::io::Write;

    let mut wkb = Vec::new();

    // Write byte order (little-endian)
    wkb.write_all(&[1u8])
        .map_err(|e| Error::TypeConversion(e.to_string()))?;

    match geom {
        Geometry::Point(p) => {
            // WKB type for Point
            wkb.write_all(&1u32.to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            wkb.write_all(&p.x().to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            wkb.write_all(&p.y().to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
        }
        Geometry::LineString(ls) => {
            // WKB type for LineString
            wkb.write_all(&2u32.to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            wkb.write_all(&(ls.coords().count() as u32).to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            for coord in ls.coords() {
                wkb.write_all(&coord.x.to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
                wkb.write_all(&coord.y.to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
            }
        }
        Geometry::Polygon(poly) => {
            // WKB type for Polygon
            wkb.write_all(&3u32.to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;

            let num_rings = 1 + poly.interiors().len();
            wkb.write_all(&(num_rings as u32).to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;

            // Exterior ring
            let exterior = poly.exterior();
            wkb.write_all(&(exterior.coords().count() as u32).to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            for coord in exterior.coords() {
                wkb.write_all(&coord.x.to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
                wkb.write_all(&coord.y.to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
            }

            // Interior rings
            for interior in poly.interiors() {
                wkb.write_all(&(interior.coords().count() as u32).to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
                for coord in interior.coords() {
                    wkb.write_all(&coord.x.to_le_bytes())
                        .map_err(|e| Error::TypeConversion(e.to_string()))?;
                    wkb.write_all(&coord.y.to_le_bytes())
                        .map_err(|e| Error::TypeConversion(e.to_string()))?;
                }
            }
        }
        _ => {
            return Err(Error::TypeConversion(format!(
                "Unsupported geometry type for WKB: {:?}",
                geom
            )));
        }
    }

    Ok(wkb)
}

/// Convert WKB bytes to geo-types Geometry (simplified).
pub fn wkb_to_geometry(wkb: &[u8]) -> Result<Geometry<f64>> {
    use byteorder::{LittleEndian, ReadBytesExt};
    use geo_types::{Coord, LineString, Polygon, point};
    use std::io::Cursor;

    if wkb.is_empty() {
        return Err(Error::GeometryParsing("Empty WKB".to_string()));
    }

    let mut cursor = Cursor::new(wkb);

    // Read byte order
    let _byte_order = cursor
        .read_u8()
        .map_err(|e| Error::GeometryParsing(e.to_string()))?;

    // Read geometry type
    let geom_type = cursor
        .read_u32::<LittleEndian>()
        .map_err(|e| Error::GeometryParsing(e.to_string()))?;

    match geom_type {
        1 => {
            // Point
            let x = cursor
                .read_f64::<LittleEndian>()
                .map_err(|e| Error::GeometryParsing(e.to_string()))?;
            let y = cursor
                .read_f64::<LittleEndian>()
                .map_err(|e| Error::GeometryParsing(e.to_string()))?;
            Ok(Geometry::Point(point!(x: x, y: y)))
        }
        2 => {
            // LineString
            let num_points = cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| Error::GeometryParsing(e.to_string()))?;

            let mut coords = Vec::with_capacity(num_points as usize);
            for _ in 0..num_points {
                let x = cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| Error::GeometryParsing(e.to_string()))?;
                let y = cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| Error::GeometryParsing(e.to_string()))?;
                coords.push(Coord { x, y });
            }

            Ok(Geometry::LineString(LineString::from(coords)))
        }
        3 => {
            // Polygon
            let num_rings = cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| Error::GeometryParsing(e.to_string()))?;

            if num_rings == 0 {
                return Err(Error::GeometryParsing("Polygon has no rings".to_string()));
            }

            // Exterior ring
            let num_points = cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| Error::GeometryParsing(e.to_string()))?;

            let mut exterior_coords = Vec::with_capacity(num_points as usize);
            for _ in 0..num_points {
                let x = cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| Error::GeometryParsing(e.to_string()))?;
                let y = cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| Error::GeometryParsing(e.to_string()))?;
                exterior_coords.push(Coord { x, y });
            }

            let exterior = LineString::from(exterior_coords);
            let mut interiors = Vec::new();

            // Interior rings
            for _ in 1..num_rings {
                let num_points = cursor
                    .read_u32::<LittleEndian>()
                    .map_err(|e| Error::GeometryParsing(e.to_string()))?;

                let mut interior_coords = Vec::with_capacity(num_points as usize);
                for _ in 0..num_points {
                    let x = cursor
                        .read_f64::<LittleEndian>()
                        .map_err(|e| Error::GeometryParsing(e.to_string()))?;
                    let y = cursor
                        .read_f64::<LittleEndian>()
                        .map_err(|e| Error::GeometryParsing(e.to_string()))?;
                    interior_coords.push(Coord { x, y });
                }

                interiors.push(LineString::from(interior_coords));
            }

            Ok(Geometry::Polygon(Polygon::new(exterior, interiors)))
        }
        _ => Err(Error::GeometryParsing(format!(
            "Unsupported geometry type: {}",
            geom_type
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use geo_types::point;

    #[test]
    fn test_sqlite_memory() {
        let conn = SqliteConnector::memory().expect("Failed to create memory database");
        assert!(conn.health_check().expect("Health check failed"));
    }

    #[test]
    fn test_point_to_wkb() {
        let p = Geometry::Point(point!(x: 1.0, y: 2.0));
        let wkb = geometry_to_wkb(&p).expect("Failed to convert");
        assert!(!wkb.is_empty());

        let geom = wkb_to_geometry(&wkb).expect("Failed to parse");
        match geom {
            Geometry::Point(pt) => {
                assert_eq!(pt.x(), 1.0);
                assert_eq!(pt.y(), 2.0);
            }
            _ => panic!("Expected Point geometry"),
        }
    }
}
