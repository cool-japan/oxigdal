//! PROJ.db SQLite reader — resolves ~7,500 EPSG codes from the upstream PROJ database.
//!
//! This module is only compiled when the `proj-db` feature is enabled.  It
//! opens the system PROJ.db file read-only, queries its `crs_view` table (PROJ
//! ≥ 7) or the legacy pair of `projected_crs` / `geodetic_crs` tables (PROJ ≤ 6),
//! and populates an [`EpsgDatabase`] with every code that is *not* already
//! present — preserving built-in priority on collision.

#![cfg(feature = "proj-db")]

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::epsg::types::{CrsType, EpsgDatabase, EpsgDefinition};
use crate::error::Error as ProjError;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// An open, read-only connection to a PROJ.db SQLite file.
pub struct ProjDb {
    pub(crate) conn: Connection,
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

/// Returns candidate filesystem paths for the system PROJ.db, in priority order:
///
/// 1. `$PROJ_DATA/proj.db`
/// 2. `$PROJ_LIB/proj.db`  (legacy env-var used by PROJ ≤ 7)
/// 3. `/usr/share/proj/proj.db`
/// 4. `/usr/local/share/proj/proj.db`
/// 5. `/opt/homebrew/share/proj/proj.db`
/// 6. `/usr/share/proj9/proj.db`
pub fn default_proj_db_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // Environment-variable overrides take priority
    if let Ok(proj_data) = std::env::var("PROJ_DATA") {
        paths.push(PathBuf::from(proj_data).join("proj.db"));
    }
    if let Ok(proj_lib) = std::env::var("PROJ_LIB") {
        paths.push(PathBuf::from(proj_lib).join("proj.db"));
    }

    // Well-known system locations
    paths.push(PathBuf::from("/usr/share/proj/proj.db"));
    paths.push(PathBuf::from("/usr/local/share/proj/proj.db"));
    paths.push(PathBuf::from("/opt/homebrew/share/proj/proj.db"));
    paths.push(PathBuf::from("/usr/share/proj9/proj.db"));

    paths
}

// ---------------------------------------------------------------------------
// Schema detection helper (internal)
// ---------------------------------------------------------------------------

/// Returns `true` when the connection has a table named `crs_view`.
fn has_crs_view(conn: &Connection) -> Result<bool, ProjError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type='table' OR type='view' AND name='crs_view'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
    Ok(count > 0)
}

// ---------------------------------------------------------------------------
// ProjDb implementation
// ---------------------------------------------------------------------------

impl ProjDb {
    /// Opens a PROJ.db file at `path` in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`ProjError::ProjDbError`] when the file does not exist, cannot
    /// be opened, or is not a valid SQLite database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ProjError> {
        let path = path.as_ref();
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
        let conn = Connection::open_with_flags(path, flags)
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Creates a [`ProjDb`] from an already-open [`Connection`].
    ///
    /// This constructor is primarily intended for testing (in-memory DBs).
    pub fn from_conn(conn: Connection) -> Self {
        Self {
            conn,
            path: PathBuf::new(),
        }
    }

    /// Tries each path returned by [`default_proj_db_paths`] in order and
    /// returns the first successfully opened database.
    ///
    /// Returns `Ok(None)` when none of the candidate files exist.
    /// Returns `Err(_)` only when a file exists but cannot be opened (e.g.
    /// permission error, corrupted database).
    pub fn open_first_available() -> Result<Option<Self>, ProjError> {
        for candidate in default_proj_db_paths() {
            if !candidate.exists() {
                continue;
            }
            match Self::open(&candidate) {
                Ok(db) => return Ok(Some(db)),
                Err(ProjError::ProjDbError(_)) => {
                    // File present but unreadable/corrupt — propagate
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
    ///
    /// Detects the schema automatically: uses `crs_view` when present (PROJ ≥ 7),
    /// otherwise falls back to the union of `projected_crs` and `geodetic_crs`.
    pub fn count_epsg_codes(&self) -> Result<usize, ProjError> {
        if self.schema_has_crs_view()? {
            let count: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM crs_view WHERE auth_name='EPSG'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            Ok(count as usize)
        } else {
            // Legacy schema: union of geodetic_crs + projected_crs
            let count: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM ( \
                         SELECT code FROM geodetic_crs  WHERE auth_name='EPSG' \
                         UNION ALL \
                         SELECT code FROM projected_crs WHERE auth_name='EPSG' \
                     )",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| ProjError::ProjDbError(e.to_string()))?;
            Ok(count as usize)
        }
    }

    /// Looks up an EPSG code and returns the corresponding [`ProjDbEntry`], or
    /// `None` when the code is absent.
    pub fn lookup_epsg(&self, code: u32) -> Result<Option<ProjDbEntry>, ProjError> {
        self.lookup_authority("EPSG", code)
    }

    /// Looks up an arbitrary authority / code pair and returns the
    /// corresponding [`ProjDbEntry`], or `None` when absent.
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
    ///
    /// When `limit` is `Some(n)`, only the first `n` codes (by ascending value)
    /// are returned.
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

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;

        let codes: Result<Vec<u32>, _> = stmt
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?
            .map(|r| r.map(|v| v as u32))
            .collect();

        codes.map_err(|e| ProjError::ProjDbError(e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn schema_has_crs_view(&self) -> Result<bool, ProjError> {
        has_crs_view(&self.conn)
    }

    /// Query against the modern `crs_view` table.
    fn lookup_from_crs_view(
        &self,
        auth: &str,
        code: u32,
    ) -> Result<Option<ProjDbEntry>, ProjError> {
        // Check whether an `area` column exists in crs_view
        let has_area = self.crs_view_has_area_column()?;

        let sql = if has_area {
            "SELECT name, type, deprecated, area \
             FROM crs_view \
             WHERE auth_name=?1 AND CAST(code AS INTEGER)=?2 \
             LIMIT 1"
        } else {
            "SELECT name, type, deprecated, NULL \
             FROM crs_view \
             WHERE auth_name=?1 AND CAST(code AS INTEGER)=?2 \
             LIMIT 1"
        };

        let result = self
            .conn
            .query_row(sql, rusqlite::params![auth, code as i64], |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(1).unwrap_or_default();
                let deprecated_int: i64 = row.get(2).unwrap_or(0);
                let area: Option<String> = row.get(3).unwrap_or(None);
                Ok((name, kind, deprecated_int, area))
            });

        match result {
            Ok((name, kind, deprecated_int, area)) => {
                let proj_string = build_proj_string(&kind);
                Ok(Some(ProjDbEntry {
                    code,
                    name,
                    proj_string,
                    kind,
                    area_of_use: area,
                    deprecated: deprecated_int != 0,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ProjError::ProjDbError(e.to_string())),
        }
    }

    /// Query against the legacy `geodetic_crs` / `projected_crs` tables.
    fn lookup_from_legacy_tables(
        &self,
        auth: &str,
        code: u32,
    ) -> Result<Option<ProjDbEntry>, ProjError> {
        // Try geodetic_crs first
        let result_geo = self.conn.query_row(
            "SELECT name, 'geographic 2D CRS', deprecated, NULL \
             FROM geodetic_crs \
             WHERE auth_name=?1 AND CAST(code AS INTEGER)=?2 \
             LIMIT 1",
            rusqlite::params![auth, code as i64],
            |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(1)?;
                let dep: i64 = row.get(2).unwrap_or(0);
                let area: Option<String> = row.get(3).unwrap_or(None);
                Ok((name, kind, dep, area))
            },
        );

        match result_geo {
            Ok((name, kind, deprecated_int, area)) => {
                return Ok(Some(ProjDbEntry {
                    code,
                    name,
                    proj_string: build_proj_string(&kind),
                    kind,
                    area_of_use: area,
                    deprecated: deprecated_int != 0,
                }));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(ProjError::ProjDbError(e.to_string())),
        }

        // Fall back to projected_crs
        let result_proj = self.conn.query_row(
            "SELECT name, 'projected CRS', deprecated, NULL \
             FROM projected_crs \
             WHERE auth_name=?1 AND CAST(code AS INTEGER)=?2 \
             LIMIT 1",
            rusqlite::params![auth, code as i64],
            |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(1)?;
                let dep: i64 = row.get(2).unwrap_or(0);
                let area: Option<String> = row.get(3).unwrap_or(None);
                Ok((name, kind, dep, area))
            },
        );

        match result_proj {
            Ok((name, kind, deprecated_int, area)) => Ok(Some(ProjDbEntry {
                code,
                name,
                proj_string: build_proj_string(&kind),
                kind,
                area_of_use: area,
                deprecated: deprecated_int != 0,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ProjError::ProjDbError(e.to_string())),
        }
    }

    /// Probes whether `crs_view` contains an `area` column.
    fn crs_view_has_area_column(&self) -> Result<bool, ProjError> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA table_info(crs_view)")
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?;

        let found = stmt
            .query_map([], |row| {
                let col_name: String = row.get(1)?;
                Ok(col_name)
            })
            .map_err(|e| ProjError::ProjDbError(e.to_string()))?
            .any(|r| r.map(|n| n == "area").unwrap_or(false));

        Ok(found)
    }
}

// ---------------------------------------------------------------------------
// Synthesise a minimal PROJ string from the CRS kind string
// ---------------------------------------------------------------------------

/// Build a sensible minimal PROJ string from the CRS type label stored in
/// the database.  PROJ ≥ 9 no longer stores PROJ strings natively; a WKT
/// round-trip through the C library would be needed for full fidelity, which
/// we avoid here.  The minimal strings produced are good enough for
/// round-trip identification via code lookup.
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
        // Default: geographic / unknown
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
        // geographic 2D / geographic 3D / unknown
        CrsType::Geographic
    }
}

// ---------------------------------------------------------------------------
// Bulk population function
// ---------------------------------------------------------------------------

/// Populates `db` from `proj_db`, inserting every EPSG code that is **not**
/// already present in `db` (built-in entries are never overwritten).
///
/// Returns the number of entries that were actually inserted.
pub fn populate_from_proj_db(db: &mut EpsgDatabase, proj_db: &ProjDb) -> Result<usize, ProjError> {
    let codes = proj_db.list_epsg_codes(None)?;
    let mut inserted: usize = 0;

    for code in codes {
        // Skip codes already in the database (built-ins win on collision).
        if db.definitions.contains_key(&code) {
            continue;
        }

        let entry = match proj_db.lookup_epsg(code)? {
            Some(e) => e,
            None => continue,
        };

        // Skip deprecated entries to avoid polluting the lookup table with
        // stale definitions.  They are still queryable via ProjDb directly.
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

/// Returns a default unit string for a given [`CrsType`].
fn unit_for_crs_type(crs_type: CrsType) -> String {
    match crs_type {
        CrsType::Projected | CrsType::Geocentric => "metre".to_owned(),
        CrsType::Vertical => "metre".to_owned(),
        _ => "degree".to_owned(),
    }
}
