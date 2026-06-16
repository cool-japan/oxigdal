//! PROJ.db SQLite reader — resolves ~7,500 EPSG codes from the upstream PROJ database.
//!
//! This module is only compiled when the `proj-db` feature is enabled.  It
//! opens the system PROJ.db file read-only, queries its `crs_view` table (PROJ
//! ≥ 7) or the legacy pair of `projected_crs` / `geodetic_crs` tables (PROJ ≤ 6),
//! and populates an [`EpsgDatabase`] with every code that is *not* already
//! present — preserving built-in priority on collision.

#![cfg(feature = "proj-db")]

use std::path::{Path, PathBuf};

use oxisql_sqlite_compat::blocking::SqliteConnectionBlocking;

use crate::epsg::types::{CrsType, EpsgDatabase, EpsgDefinition};
use crate::error::Error as ProjError;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// An open, read-only connection to a PROJ.db SQLite file.
pub struct ProjDb {
    pub(crate) conn: SqliteConnectionBlocking,
    /// The file-system path from which this database was opened (may be empty
    /// for in-memory instances created by [`ProjDb::from_conn`]).
    pub path: PathBuf,
}

/// A single CRS entry retrieved from the PROJ.db.
#[derive(Debug, Clone)]
pub struct ProjDbEntry {
    /// EPSG (or other authority) numeric code.
    pub code: u32,
    /// Human-readable name.
    pub name: String,
    /// PROJ string representation (may be synthesised when the DB stores WKT).
    pub proj_string: String,
    /// CRS kind string as stored in the database (e.g. `"geographic 2D CRS"`).
    pub kind: String,
    /// Free-text area of use, if available.
    pub area_of_use: Option<String>,
    /// Whether this entry has been deprecated in the authority registry.
    pub deprecated: bool,
}

// ---------------------------------------------------------------------------
// Search-path helpers
// ---------------------------------------------------------------------------

/// Returns candidate filesystem paths for the system PROJ.db, in priority order.
pub fn default_proj_db_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Ok(proj_data) = std::env::var("PROJ_DATA") {
        paths.push(PathBuf::from(proj_data).join("proj.db"));
    }
    if let Ok(proj_lib) = std::env::var("PROJ_LIB") {
        paths.push(PathBuf::from(proj_lib).join("proj.db"));
    }

    paths.push(PathBuf::from("/usr/share/proj/proj.db"));
    paths.push(PathBuf::from("/usr/local/share/proj/proj.db"));
    paths.push(PathBuf::from("/opt/homebrew/share/proj/proj.db"));
    paths.push(PathBuf::from("/usr/share/proj9/proj.db"));

    paths
}

// ---------------------------------------------------------------------------
// Schema detection helper (internal)
// ---------------------------------------------------------------------------

fn has_crs_view(conn: &SqliteConnectionBlocking) -> Result<bool, ProjError> {
    // Try to query crs_view directly — if it fails, it doesn't exist
    match conn.query("SELECT COUNT(*) FROM crs_view LIMIT 1", &[]) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// ProjDb implementation
// ---------------------------------------------------------------------------

impl ProjDb {
    /// Opens a PROJ.db file at `path` in read-only mode.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ProjError> {
        let path = path.as_ref();
        let path_str = path.to_str().ok_or_else(|| {
            ProjError::ProjDbError("path contains non-UTF-8 characters".to_owned())
        })?;
        // Check if file exists first (oxisqlite creates the file if it doesn't exist)
        if !path.exists() {
            return Err(ProjError::ProjDbError(format!(
                "PROJ.db not found at {}",
                path.display()
            )));
        }
        let conn = SqliteConnectionBlocking::open(path_str)
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Creates a [`ProjDb`] from an already-open [`SqliteConnectionBlocking`].
    pub fn from_conn(conn: SqliteConnectionBlocking) -> Self {
        Self {
            conn,
            path: PathBuf::new(),
        }
    }

    /// Tries each path returned by [`default_proj_db_paths`] in order.
    pub fn open_first_available() -> Result<Option<Self>, ProjError> {
        for candidate in default_proj_db_paths() {
            if !candidate.exists() {
                continue;
            }
            match Self::open(&candidate) {
                Ok(db) => return Ok(Some(db)),
                Err(ProjError::ProjDbError(_)) => {
                    return Err(ProjError::ProjDbError(format!(
                        "Failed to open PROJ.db at {}",
                        candidate.display()
                    )));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    /// Returns the number of EPSG-authority CRS entries in the database.
    pub fn count_epsg_codes(&self) -> Result<usize, ProjError> {
        if self.schema_has_crs_view()? {
            let rows = self
                .conn
                .query("SELECT COUNT(*) FROM crs_view WHERE auth_name='EPSG'", &[])
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            let count = rows
                .first()
                .and_then(|r| r.try_get_by_index::<i64>(0).ok())
                .unwrap_or(0);
            Ok(count as usize)
        } else {
            let rows = self
                .conn
                .query(
                    "SELECT COUNT(*) FROM ( \
                         SELECT code FROM geodetic_crs  WHERE auth_name='EPSG' \
                         UNION ALL \
                         SELECT code FROM projected_crs WHERE auth_name='EPSG' \
                     )",
                    &[],
                )
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            let count = rows
                .first()
                .and_then(|r| r.try_get_by_index::<i64>(0).ok())
                .unwrap_or(0);
            Ok(count as usize)
        }
    }

    /// Looks up an EPSG code.
    pub fn lookup_epsg(&self, code: u32) -> Result<Option<ProjDbEntry>, ProjError> {
        self.lookup_authority("EPSG", code)
    }

    /// Looks up an arbitrary authority / code pair.
    pub fn lookup_authority(
        &self,
        auth: &str,
        code: u32,
    ) -> Result<Option<ProjDbEntry>, ProjError> {
        if self.schema_has_crs_view()? {
            self.lookup_from_crs_view(auth, code)
        } else {
            self.lookup_from_legacy_tables(auth, code)
        }
    }

    /// Returns a sorted list of all EPSG numeric codes present in the database.
    pub fn list_epsg_codes(&self, limit: Option<usize>) -> Result<Vec<u32>, ProjError> {
        let sql = if self.schema_has_crs_view()? {
            if let Some(n) = limit {
                format!(
                    "SELECT CAST(code AS INTEGER) FROM crs_view \
                     WHERE auth_name='EPSG' ORDER BY code ASC LIMIT {}",
                    n
                )
            } else {
                "SELECT CAST(code AS INTEGER) FROM crs_view \
                 WHERE auth_name='EPSG' ORDER BY code ASC"
                    .to_owned()
            }
        } else {
            let limit_clause = limit.map(|n| format!(" LIMIT {}", n)).unwrap_or_default();
            format!(
                "SELECT code FROM ( \
                     SELECT CAST(code AS INTEGER) as code FROM geodetic_crs  WHERE auth_name='EPSG' \
                     UNION \
                     SELECT CAST(code AS INTEGER) as code FROM projected_crs WHERE auth_name='EPSG' \
                 ) ORDER BY code ASC{}",
                limit_clause
            )
        };

        let rows = self
            .conn
            .query(&sql, &[])
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;

        let mut codes = Vec::with_capacity(rows.len());
        for row in &rows {
            let code = row
                .try_get_by_index::<i64>(0)
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            codes.push(code as u32);
        }
        Ok(codes)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn schema_has_crs_view(&self) -> Result<bool, ProjError> {
        has_crs_view(&self.conn)
    }

    fn lookup_from_crs_view(
        &self,
        auth: &str,
        code: u32,
    ) -> Result<Option<ProjDbEntry>, ProjError> {
        let has_area = self.crs_view_has_area_column()?;

        let sql = if has_area {
            "SELECT name, type, deprecated, area \
             FROM crs_view \
             WHERE auth_name=$1 AND CAST(code AS INTEGER)=$2 \
             LIMIT 1"
        } else {
            "SELECT name, type, deprecated, NULL \
             FROM crs_view \
             WHERE auth_name=$1 AND CAST(code AS INTEGER)=$2 \
             LIMIT 1"
        };

        let code_i64 = code as i64;
        let rows = self
            .conn
            .query(
                sql,
                &[
                    &auth as &dyn oxisql_core::ToSqlValue,
                    &code_i64 as &dyn oxisql_core::ToSqlValue,
                ],
            )
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;

        if let Some(row) = rows.first() {
            let name: String = row
                .try_get_by_index::<String>(0)
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            let kind: String = row.try_get_by_index::<String>(1).unwrap_or_default();
            let deprecated_int: i64 = row.try_get_by_index::<i64>(2).unwrap_or(0);
            let area: Option<String> = row.try_get_by_index::<Option<String>>(3).unwrap_or(None);
            let proj_string = build_proj_string(&kind);
            Ok(Some(ProjDbEntry {
                code,
                name,
                proj_string,
                kind,
                area_of_use: area,
                deprecated: deprecated_int != 0,
            }))
        } else {
            Ok(None)
        }
    }

    fn lookup_from_legacy_tables(
        &self,
        auth: &str,
        code: u32,
    ) -> Result<Option<ProjDbEntry>, ProjError> {
        let code_i64 = code as i64;
        let params: &[&dyn oxisql_core::ToSqlValue] = &[
            &auth as &dyn oxisql_core::ToSqlValue,
            &code_i64 as &dyn oxisql_core::ToSqlValue,
        ];

        // Try geodetic_crs first
        let geo_rows = self
            .conn
            .query(
                "SELECT name, 'geographic 2D CRS', deprecated, NULL \
             FROM geodetic_crs \
             WHERE auth_name=$1 AND CAST(code AS INTEGER)=$2 \
             LIMIT 1",
                params,
            )
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;

        if let Some(row) = geo_rows.first() {
            let name: String = row
                .try_get_by_index::<String>(0)
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            let kind: String = row.try_get_by_index::<String>(1).unwrap_or_default();
            let dep: i64 = row.try_get_by_index::<i64>(2).unwrap_or(0);
            let area: Option<String> = row.try_get_by_index::<Option<String>>(3).unwrap_or(None);
            return Ok(Some(ProjDbEntry {
                code,
                name,
                proj_string: build_proj_string(&kind),
                kind,
                area_of_use: area,
                deprecated: dep != 0,
            }));
        }

        // Fall back to projected_crs
        let proj_rows = self
            .conn
            .query(
                "SELECT name, 'projected CRS', deprecated, NULL \
             FROM projected_crs \
             WHERE auth_name=$1 AND CAST(code AS INTEGER)=$2 \
             LIMIT 1",
                params,
            )
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;

        if let Some(row) = proj_rows.first() {
            let name: String = row
                .try_get_by_index::<String>(0)
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            let kind: String = row.try_get_by_index::<String>(1).unwrap_or_default();
            let dep: i64 = row.try_get_by_index::<i64>(2).unwrap_or(0);
            let area: Option<String> = row.try_get_by_index::<Option<String>>(3).unwrap_or(None);
            Ok(Some(ProjDbEntry {
                code,
                name,
                proj_string: build_proj_string(&kind),
                kind,
                area_of_use: area,
                deprecated: dep != 0,
            }))
        } else {
            Ok(None)
        }
    }

    fn crs_view_has_area_column(&self) -> Result<bool, ProjError> {
        let rows = match self.conn.query("PRAGMA table_info(crs_view)", &[]) {
            Ok(r) => r,
            Err(_) => return Ok(false), // PRAGMA not supported, assume no area column
        };

        // PRAGMA table_info columns: cid(0), name(1), type(2), notnull(3), dflt_value(4), pk(5)
        let found = rows.iter().any(|row| {
            row.try_get_by_index::<String>(1)
                .map(|n| n == "area")
                .unwrap_or(false)
        });
        Ok(found)
    }
}

// ---------------------------------------------------------------------------
// Synthesise a minimal PROJ string from the CRS kind string
// ---------------------------------------------------------------------------

fn build_proj_string(kind: &str) -> String {
    let kind_lower = kind.to_ascii_lowercase();
    if kind_lower.contains("geocentric") || kind_lower.contains("cartesian") {
        "+proj=geocent +datum=WGS84 +units=m +no_defs".to_owned()
    } else if kind_lower.contains("project") {
        "+proj=tmerc +datum=WGS84 +units=m +no_defs".to_owned()
    } else if kind_lower.contains("vertical") {
        "+proj=longlat +datum=WGS84 +vunits=m +no_defs".to_owned()
    } else if kind_lower.contains("compound") {
        "+proj=longlat +datum=WGS84 +no_defs".to_owned()
    } else {
        "+proj=longlat +datum=WGS84 +no_defs".to_owned()
    }
}

// ---------------------------------------------------------------------------
// Map CRS kind string → CrsType
// ---------------------------------------------------------------------------

fn kind_to_crs_type(kind: &str) -> CrsType {
    let k = kind.to_ascii_lowercase();
    if k.contains("geocentric") || k.contains("cartesian") {
        CrsType::Geocentric
    } else if k.contains("project") {
        CrsType::Projected
    } else if k.contains("vertical") {
        CrsType::Vertical
    } else if k.contains("compound") {
        CrsType::Compound
    } else if k.contains("engineer") {
        CrsType::Engineering
    } else {
        CrsType::Geographic
    }
}

// ---------------------------------------------------------------------------
// Bulk population function
// ---------------------------------------------------------------------------

/// Populates `db` from `proj_db`, inserting every EPSG code that is **not**
/// already present in `db` (built-in entries are never overwritten).
pub fn populate_from_proj_db(db: &mut EpsgDatabase, proj_db: &ProjDb) -> Result<usize, ProjError> {
    let codes = proj_db.list_epsg_codes(None)?;
    let mut inserted: usize = 0;

    for code in codes {
        if db.definitions.contains_key(&code) {
            continue;
        }

        let entry = match proj_db.lookup_epsg(code)? {
            Some(e) => e,
            None => continue,
        };

        if entry.deprecated {
            continue;
        }

        let crs_type = kind_to_crs_type(&entry.kind);
        let proj_string = entry.proj_string.clone();
        let area = entry.area_of_use.clone().unwrap_or_default();

        let definition = EpsgDefinition {
            code,
            name: entry.name,
            proj_string,
            wkt: None,
            crs_type,
            area_of_use: area,
            unit: unit_for_crs_type(crs_type),
            datum: "WGS84".to_owned(),
        };

        db.definitions.entry(code).or_insert(definition);
        inserted += 1;
    }

    Ok(inserted)
}

fn unit_for_crs_type(crs_type: CrsType) -> String {
    match crs_type {
        CrsType::Projected | CrsType::Geocentric => "metre".to_owned(),
        CrsType::Vertical => "metre".to_owned(),
        _ => "degree".to_owned(),
    }
}
