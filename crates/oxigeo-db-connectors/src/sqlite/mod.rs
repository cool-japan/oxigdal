//! SQLite/SpatiaLite spatial database connector.
//!
//! Provides support for reading and writing spatial data to SQLite databases
//! using the pure-Rust oxisql-sqlite-compat (limbo engine).

pub mod reader;
pub mod writer;

use crate::error::{Error, Result};
use geo_types::Geometry;
use oxisql_core::Value;
use oxisql_sqlite_compat::blocking::SqliteConnectionBlocking;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// SQLite connector configuration.
#[derive(Debug, Clone)]
pub struct SqliteConfig {
    /// Database file path (`:memory:` for in-memory).
    pub path: PathBuf,
    /// Enable SpatiaLite extension (no-op in pure-Rust mode — always false).
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
            spatialite: false,
            read_only: false,
            create: true,
            wal_mode: true,
            cache_size: 10240,
        }
    }
}

/// SQLite spatial database connector backed by the pure-Rust limbo engine.
///
/// The connector is [`Clone`] and every clone shares the same underlying
/// connection (`Arc<SqliteConnectionBlocking>`). It also carries a shared
/// `write_lock` used to make multi-statement write sequences (notably an
/// `INSERT` followed by reading `last_insert_rowid()`) atomic with respect to
/// other clones executing concurrently on the same connection.
#[derive(Clone)]
pub struct SqliteConnector {
    conn: Arc<SqliteConnectionBlocking>,
    /// Serializes write sequences that must not interleave across threads
    /// sharing this connection (e.g. `INSERT` + `last_insert_rowid()`).
    write_lock: Arc<Mutex<()>>,
    #[allow(dead_code)]
    config: SqliteConfig,
}

impl SqliteConnector {
    /// Create a new SQLite connector.
    pub fn new(config: SqliteConfig) -> Result<Self> {
        let path_str = config
            .path
            .to_str()
            .ok_or_else(|| Error::Configuration("Non-UTF-8 path".to_string()))?;
        let conn = SqliteConnectionBlocking::open(path_str)
            .map_err(|e| Error::Connection(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(conn),
            write_lock: Arc::new(Mutex::new(())),
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

    /// Check if the connection is healthy.
    pub fn health_check(&self) -> Result<bool> {
        self.conn
            .ping()
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(true)
    }

    /// Get database version.
    pub fn version(&self) -> Result<String> {
        let rows = self
            .conn
            .query("SELECT sqlite_version()", &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        let version = rows
            .first()
            .and_then(|row| row.get_by_index(0))
            .and_then(|v| {
                if let Value::Text(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string());
        Ok(version)
    }

    /// Check if SpatiaLite is loaded.
    ///
    /// Always returns `false` in pure-Rust mode (no SpatiaLite extension support).
    pub fn has_spatialite(&self) -> Result<bool> {
        Ok(false)
    }

    /// Initialize spatial metadata.
    ///
    /// No-op in pure-Rust mode (SpatiaLite not available).
    pub fn init_spatial_metadata(&self) -> Result<()> {
        Err(Error::Configuration(
            "SpatiaLite is not available in pure-Rust mode".to_string(),
        ))
    }

    /// Create a spatial table (pure-Rust fallback without SpatiaLite).
    pub fn create_spatial_table(
        &self,
        table_name: &str,
        geometry_column: &str,
        _geometry_type: &str,
        _srid: i32,
        additional_columns: &[(String, String)],
    ) -> Result<()> {
        let mut columns = vec![
            "id INTEGER PRIMARY KEY AUTOINCREMENT".to_string(),
            format!("{} BLOB", geometry_column),
        ];
        for (col_name, col_type) in additional_columns {
            columns.push(format!("{} {}", col_name, col_type));
        }

        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            table_name,
            columns.join(", ")
        );

        self.conn
            .execute(&create_sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Drop a table.
    pub fn drop_table(&self, table_name: &str) -> Result<()> {
        let sql = format!("DROP TABLE IF EXISTS {}", table_name);
        self.conn
            .execute(&sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// List all tables.
    pub fn list_tables(&self) -> Result<Vec<String>> {
        let rows = self
            .conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                &[],
            )
            .map_err(|e| Error::Query(e.to_string()))?;

        let tables = rows
            .iter()
            .filter_map(|row| {
                row.get_by_index(0).and_then(|v| {
                    if let Value::Text(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();

        Ok(tables)
    }

    /// Get table schema as (column_name, type) pairs.
    pub fn table_schema(&self, table_name: &str) -> Result<Vec<(String, String)>> {
        let sql = format!("PRAGMA table_info(\"{}\")", table_name);
        let rows = self
            .conn
            .query(&sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;

        let schema = rows
            .iter()
            .filter_map(|row| {
                let name = row.get_by_index(1).and_then(|v| {
                    if let Value::Text(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })?;
                let type_str = row
                    .get_by_index(2)
                    .and_then(|v| {
                        if let Value::Text(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                Some((name, type_str))
            })
            .collect();

        Ok(schema)
    }

    /// Execute raw SQL.
    pub fn execute(&self, sql: &str) -> Result<usize> {
        let affected = self
            .conn
            .execute(sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(affected as usize)
    }

    /// Begin a transaction.
    pub fn begin_transaction(&self) -> Result<()> {
        self.conn
            .execute("BEGIN", &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// Commit a transaction.
    pub fn commit_transaction(&self) -> Result<()> {
        self.conn
            .execute("COMMIT", &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// Rollback a transaction.
    pub fn rollback_transaction(&self) -> Result<()> {
        self.conn
            .execute("ROLLBACK", &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// Vacuum the database.
    pub fn vacuum(&self) -> Result<()> {
        self.conn
            .execute("VACUUM", &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// Analyze the database for query optimization.
    pub fn analyze(&self) -> Result<()> {
        self.conn
            .execute("ANALYZE", &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// Access the underlying blocking connection.
    pub fn blocking_conn(&self) -> &SqliteConnectionBlocking {
        &self.conn
    }

    /// Acquire the connector's write lock, serializing a multi-statement write
    /// sequence against other clones of this connector.
    ///
    /// A poisoned lock (a previous holder panicked) is recovered rather than
    /// propagated, since the guarded data is a unit `()` with no invariants to
    /// uphold.
    pub(crate) fn lock_writes(&self) -> MutexGuard<'_, ()> {
        self.write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            wkb.write_all(&1u32.to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            wkb.write_all(&p.x().to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            wkb.write_all(&p.y().to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
        }
        Geometry::LineString(ls) => {
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
            wkb.write_all(&3u32.to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            let num_rings = 1 + poly.interiors().len();
            wkb.write_all(&(num_rings as u32).to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            let exterior = poly.exterior();
            wkb.write_all(&(exterior.coords().count() as u32).to_le_bytes())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
            for coord in exterior.coords() {
                wkb.write_all(&coord.x.to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
                wkb.write_all(&coord.y.to_le_bytes())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
            }
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

/// Read a `u32` from `cursor` honouring the WKB byte-order flag (`true` =
/// big-endian / XDR, `false` = little-endian / NDR).
fn read_wkb_u32(cursor: &mut std::io::Cursor<&[u8]>, big_endian: bool) -> Result<u32> {
    use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
    let value = if big_endian {
        cursor.read_u32::<BigEndian>()
    } else {
        cursor.read_u32::<LittleEndian>()
    };
    value.map_err(|e| Error::GeometryParsing(e.to_string()))
}

/// Read an `f64` from `cursor` honouring the WKB byte-order flag.
fn read_wkb_f64(cursor: &mut std::io::Cursor<&[u8]>, big_endian: bool) -> Result<f64> {
    use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
    let value = if big_endian {
        cursor.read_f64::<BigEndian>()
    } else {
        cursor.read_f64::<LittleEndian>()
    };
    value.map_err(|e| Error::GeometryParsing(e.to_string()))
}

/// Convert WKB bytes to geo-types Geometry (simplified).
///
/// Honours the leading byte-order marker: `0x00` selects big-endian (XDR) and
/// `0x01` selects little-endian (NDR) for every subsequent multi-byte field,
/// per the WKB specification. Any other marker value is rejected.
pub fn wkb_to_geometry(wkb: &[u8]) -> Result<Geometry<f64>> {
    use byteorder::ReadBytesExt;
    use geo_types::{Coord, LineString, Polygon, point};
    use std::io::Cursor;

    if wkb.is_empty() {
        return Err(Error::GeometryParsing("Empty WKB".to_string()));
    }

    let mut cursor = Cursor::new(wkb);

    let byte_order = cursor
        .read_u8()
        .map_err(|e| Error::GeometryParsing(e.to_string()))?;
    let big_endian = match byte_order {
        0 => true,
        1 => false,
        other => {
            return Err(Error::GeometryParsing(format!(
                "Invalid WKB byte-order marker: {other} (expected 0x00 or 0x01)"
            )));
        }
    };

    let geom_type = read_wkb_u32(&mut cursor, big_endian)?;

    match geom_type {
        1 => {
            let x = read_wkb_f64(&mut cursor, big_endian)?;
            let y = read_wkb_f64(&mut cursor, big_endian)?;
            Ok(Geometry::Point(point!(x: x, y: y)))
        }
        2 => {
            let num_points = read_wkb_u32(&mut cursor, big_endian)?;
            let mut coords = Vec::with_capacity(num_points as usize);
            for _ in 0..num_points {
                let x = read_wkb_f64(&mut cursor, big_endian)?;
                let y = read_wkb_f64(&mut cursor, big_endian)?;
                coords.push(Coord { x, y });
            }
            Ok(Geometry::LineString(LineString::from(coords)))
        }
        3 => {
            let num_rings = read_wkb_u32(&mut cursor, big_endian)?;
            if num_rings == 0 {
                return Err(Error::GeometryParsing("Polygon has no rings".to_string()));
            }
            let num_points = read_wkb_u32(&mut cursor, big_endian)?;
            let mut exterior_coords = Vec::with_capacity(num_points as usize);
            for _ in 0..num_points {
                let x = read_wkb_f64(&mut cursor, big_endian)?;
                let y = read_wkb_f64(&mut cursor, big_endian)?;
                exterior_coords.push(Coord { x, y });
            }
            let exterior = LineString::from(exterior_coords);
            let mut interiors = Vec::new();
            for _ in 1..num_rings {
                let ring_points = read_wkb_u32(&mut cursor, big_endian)?;
                let mut interior_coords = Vec::with_capacity(ring_points as usize);
                for _ in 0..ring_points {
                    let x = read_wkb_f64(&mut cursor, big_endian)?;
                    let y = read_wkb_f64(&mut cursor, big_endian)?;
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

    #[test]
    fn test_wkb_big_endian_point_is_decoded_correctly() {
        // Hand-build a big-endian (XDR, marker 0x00) WKB Point(1.0, 2.0). The
        // decoder must honour the byte-order flag rather than assuming
        // little-endian (which would previously yield garbage coordinates).
        let mut wkb = vec![0x00];
        wkb.extend_from_slice(&1u32.to_be_bytes()); // geometry type = Point
        wkb.extend_from_slice(&1.0f64.to_be_bytes());
        wkb.extend_from_slice(&2.0f64.to_be_bytes());

        let geom = wkb_to_geometry(&wkb).expect("Failed to parse big-endian WKB");
        match geom {
            Geometry::Point(pt) => {
                assert_eq!(pt.x(), 1.0);
                assert_eq!(pt.y(), 2.0);
            }
            _ => panic!("Expected Point geometry"),
        }
    }

    #[test]
    fn test_wkb_invalid_byte_order_is_rejected() {
        let wkb = vec![0x02, 0, 0, 0, 1];
        assert!(wkb_to_geometry(&wkb).is_err());
    }

    #[test]
    fn test_wkb_big_endian_matches_little_endian() {
        // A big-endian and little-endian encoding of the same point must decode
        // to identical coordinates.
        let mut be = vec![0x00];
        be.extend_from_slice(&1u32.to_be_bytes());
        be.extend_from_slice(&3.5f64.to_be_bytes());
        be.extend_from_slice(&(-7.25f64).to_be_bytes());

        let le = geometry_to_wkb(&Geometry::Point(point!(x: 3.5, y: -7.25)))
            .expect("Failed to encode little-endian");

        assert_eq!(
            wkb_to_geometry(&be).expect("be parse"),
            wkb_to_geometry(&le).expect("le parse")
        );
    }
}
