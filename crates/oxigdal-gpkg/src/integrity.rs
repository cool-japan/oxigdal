//! GeoPackage file-integrity validator (OGC 12-128r19 §1.1.3).
//!
//! Runs a sequence of structural checks against an opened [`GeoPackage`] and
//! reports every issue discovered as a typed [`IntegrityIssue`].  Checks include:
//!
//! 1. SQLite file header carries the GeoPackage application_id (`"GPKG"`).
//! 2. The `user_version` field is at least 10300 (GeoPackage 1.3.0).
//! 3. The three OGC-mandated default SRS rows (`-1`, `0`, `4326`) are present
//!    in `gpkg_spatial_ref_sys`.
//! 4. Every `gpkg_contents.table_name` references an existing user table.
//! 5. Every `gpkg_geometry_columns.table_name` references an existing user
//!    table and every `srs_id` references a row in `gpkg_spatial_ref_sys`.
//! 6. Every `gpkg_extensions` row with a non-NULL `table_name` references an
//!    existing user table.
//!
//! All checks are non-fatal: a missing optional system table (such as
//! `gpkg_extensions` or `gpkg_geometry_columns`) is not itself an error.  The
//! returned [`IntegrityReport`] aggregates every issue so callers can render a
//! complete diagnostic to the user rather than failing at the first problem.
//!
//! # Example
//! ```no_run
//! use oxigdal_gpkg::{GeoPackage, check_integrity};
//!
//! let bytes: Vec<u8> = std::fs::read("/path/to/file.gpkg").expect("read");
//! let mut gpkg = GeoPackage::from_bytes(bytes).expect("parse");
//! gpkg.load_contents().expect("load contents");
//!
//! let report = check_integrity(&gpkg);
//! if !report.passed {
//!     for issue in &report.issues {
//!         eprintln!("{}", issue.description());
//!     }
//! }
//! ```

use std::collections::HashSet;

use crate::gpkg::GeoPackage;

// ─────────────────────────────────────────────────────────────────────────────
// Public constants — OGC §1.1 mandatory values
// ─────────────────────────────────────────────────────────────────────────────

/// Application ID embedded in the SQLite file header at offset 68.
///
/// ASCII `"GPKG"` packed big-endian = `0x4750_4B47`.  GeoPackages produced by a
/// conforming implementation must always carry this value.
pub const GPKG_APP_ID: u32 = 0x4750_4B47;

/// Minimum acceptable `user_version` per OGC 12-128r19 (1.3.0 = `10_300`).
///
/// The convention is `100 * major + minor` with the minor encoded as the
/// hundreds digit of the patch level.  Earlier versions are rejected because
/// they may use incompatible WKB encodings or missing system tables.
pub const MIN_USER_VERSION: u32 = 10_300;

/// SRS identifiers that must always be present in `gpkg_spatial_ref_sys`
/// per OGC 12-128r19 Requirement 11.
pub const REQUIRED_SRS: &[i32] = &[-1, 0, 4326];

// ─────────────────────────────────────────────────────────────────────────────
// IntegrityIssue
// ─────────────────────────────────────────────────────────────────────────────

/// A single issue discovered during integrity validation.
///
/// Each variant carries the identifying information needed to point the user
/// at the offending row, table, or header field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityIssue {
    /// A required system table (e.g. `gpkg_spatial_ref_sys`) is missing from
    /// `sqlite_master`.  Without it, dependent checks cannot proceed.
    MissingRequiredTable(String),
    /// One of the three OGC-mandated SRS rows is missing from
    /// `gpkg_spatial_ref_sys`.
    MissingRequiredSrs {
        /// Numeric SRS identifier that was not found.
        code: i32,
    },
    /// A `gpkg_contents` row names a table that does not exist in the
    /// underlying SQLite database.
    ContentsRefsMissingTable {
        /// Name of the user-data table referenced by the contents row.
        table_name: String,
    },
    /// A `gpkg_geometry_columns` row names a table that does not exist in the
    /// underlying SQLite database.
    GeometryColumnsRefsMissingTable {
        /// Name of the user-data table referenced by the geometry-columns row.
        table_name: String,
    },
    /// A `gpkg_geometry_columns` row references an `srs_id` that is not
    /// present in `gpkg_spatial_ref_sys`.
    GeometryColumnsRefsMissingSrs {
        /// Owning table name.
        table_name: String,
        /// The dangling SRS identifier.
        srs_id: i32,
    },
    /// A `gpkg_extensions` row references a user-data table that does not
    /// exist in the underlying SQLite database.
    OrphanedExtensionRow {
        /// Extension identifier (e.g. `"gpkg_rtree_index"`).
        extension_name: String,
        /// The table_name column value (may be `None` for file-scoped
        /// extensions, but those are not reported here).
        table_name: Option<String>,
    },
    /// The SQLite header's `application_id` field does not match
    /// [`GPKG_APP_ID`].
    AppIdMismatch {
        /// Actual value found in the header at offset 68.
        actual: u32,
    },
    /// The `user_version` field is below [`MIN_USER_VERSION`].
    UserVersionTooOld {
        /// Actual value found in the header at offset 60.
        actual: u32,
        /// Minimum required value.
        minimum: u32,
    },
}

impl IntegrityIssue {
    /// Render a human-readable, single-line description of this issue.
    ///
    /// The string is intended for diagnostic output and is stable enough to
    /// match against in tests but should not be used for programmatic
    /// dispatch — use pattern-matching on the variant for that.
    pub fn description(&self) -> String {
        match self {
            IntegrityIssue::MissingRequiredTable(name) => {
                format!("required system table '{name}' is missing from sqlite_master")
            }
            IntegrityIssue::MissingRequiredSrs { code } => {
                format!("mandatory SRS row with srs_id={code} is missing from gpkg_spatial_ref_sys")
            }
            IntegrityIssue::ContentsRefsMissingTable { table_name } => {
                format!(
                    "gpkg_contents references table '{table_name}' which does not exist in the database"
                )
            }
            IntegrityIssue::GeometryColumnsRefsMissingTable { table_name } => {
                format!(
                    "gpkg_geometry_columns references table '{table_name}' which does not exist in the database"
                )
            }
            IntegrityIssue::GeometryColumnsRefsMissingSrs { table_name, srs_id } => {
                format!(
                    "gpkg_geometry_columns row for table '{table_name}' references srs_id={srs_id} which is not present in gpkg_spatial_ref_sys"
                )
            }
            IntegrityIssue::OrphanedExtensionRow {
                extension_name,
                table_name,
            } => match table_name {
                Some(t) => format!(
                    "gpkg_extensions row for extension '{extension_name}' references table '{t}' which does not exist in the database"
                ),
                None => format!(
                    "gpkg_extensions row for extension '{extension_name}' has a NULL table_name but is flagged as orphaned (should not happen)"
                ),
            },
            IntegrityIssue::AppIdMismatch { actual } => {
                format!(
                    "SQLite application_id is 0x{actual:08X} but a GeoPackage must use 0x{GPKG_APP_ID:08X}"
                )
            }
            IntegrityIssue::UserVersionTooOld { actual, minimum } => {
                format!("GeoPackage user_version is {actual} but {minimum} or higher is required")
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IntegrityReport
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate result of [`check_integrity`].
///
/// The `passed` field is a convenience cache: it is exactly equal to
/// `issues.is_empty()` and is set during construction so callers do not need to
/// re-derive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    /// `true` when no issues were detected (i.e. `issues.is_empty()`).
    pub passed: bool,
    /// Every issue found, in the order in which the underlying check was run.
    pub issues: Vec<IntegrityIssue>,
}

impl IntegrityReport {
    /// Return the number of issues recorded.
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// Return `true` if any recorded issue matches the supplied predicate.
    ///
    /// Useful in tests to assert that a particular variant was produced
    /// without depending on its position in the issue vector.
    pub fn has_issue_of<F>(&self, pred: F) -> bool
    where
        F: Fn(&IntegrityIssue) -> bool,
    {
        self.issues.iter().any(pred)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry points
// ─────────────────────────────────────────────────────────────────────────────

/// Run every integrity check and return a complete [`IntegrityReport`].
///
/// All check functions are invoked unconditionally so the resulting report
/// captures *every* discoverable issue.  Individual check failures never
/// short-circuit subsequent checks.
///
/// This function performs no I/O — it operates entirely on data already
/// loaded into the [`GeoPackage`] wrapper.
pub fn check_integrity(gpkg: &GeoPackage) -> IntegrityReport {
    let mut issues: Vec<IntegrityIssue> = Vec::new();

    check_app_id(gpkg, &mut issues);
    check_user_version(gpkg, &mut issues);
    check_required_srs(gpkg, &mut issues);
    check_contents_refs(gpkg, &mut issues);
    check_geometry_columns_refs(gpkg, &mut issues);
    check_extensions_refs(gpkg, &mut issues);

    IntegrityReport {
        passed: issues.is_empty(),
        issues,
    }
}

/// Convenience wrapper that returns `Ok(())` for a clean GeoPackage and
/// `Err(issues)` otherwise.
///
/// The error vector is the same one carried in
/// [`IntegrityReport::issues`].  Use [`check_integrity`] when partial reports
/// (passed + issues together) are useful.
pub fn check_integrity_strict(gpkg: &GeoPackage) -> Result<(), Vec<IntegrityIssue>> {
    let report = check_integrity(gpkg);
    if report.passed {
        Ok(())
    } else {
        Err(report.issues)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Individual check functions
// ─────────────────────────────────────────────────────────────────────────────

/// Check 1: SQLite header carries the GeoPackage application_id.
///
/// OGC 12-128r19 Req 2 mandates that the `application_id` field at offset 68
/// equals ASCII `"GPKG"`.  The reader stores the value as a parsed `u32`.
fn check_app_id(gpkg: &GeoPackage, issues: &mut Vec<IntegrityIssue>) {
    let actual = gpkg.reader.header.application_id;
    if actual != GPKG_APP_ID {
        issues.push(IntegrityIssue::AppIdMismatch { actual });
    }
}

/// Check 2: SQLite header `user_version` is at least 1.3.0 (`10_300`).
///
/// OGC 12-128r19 Req 2 ties the `user_version` field at offset 60 to the
/// GeoPackage specification version.  Files predating 1.3.0 may use legacy
/// WKB encodings or omit mandatory system tables.
fn check_user_version(gpkg: &GeoPackage, issues: &mut Vec<IntegrityIssue>) {
    let actual = gpkg.reader.header.user_version;
    if actual < MIN_USER_VERSION {
        issues.push(IntegrityIssue::UserVersionTooOld {
            actual,
            minimum: MIN_USER_VERSION,
        });
    }
}

/// Check 3: the three OGC-mandated default SRS rows are present.
///
/// OGC 12-128r19 Requirement 11 lists `srs_id` values `-1`, `0`, and `4326`
/// as always-present rows in `gpkg_spatial_ref_sys`.  If the table itself is
/// missing this is reported as a single
/// [`IntegrityIssue::MissingRequiredTable`] without iterating the individual
/// SRS codes (one diagnostic is more actionable than three follow-up errors).
fn check_required_srs(gpkg: &GeoPackage, issues: &mut Vec<IntegrityIssue>) {
    let rows = match gpkg.scan_table_by_name("gpkg_spatial_ref_sys") {
        Ok(Some(rows)) => rows,
        // Treat scan errors the same as a missing table for diagnostic purposes.
        Ok(None) | Err(_) => {
            issues.push(IntegrityIssue::MissingRequiredTable(
                "gpkg_spatial_ref_sys".to_string(),
            ));
            return;
        }
    };

    // Column layout: srs_name(0), srs_id(1), organization(2), org_id(3),
    // definition(4), description(5).  We need column 1.
    let present: HashSet<i32> = rows
        .iter()
        .filter_map(|(_rowid, cols)| {
            if cols.len() < 2 {
                return None;
            }
            cell_to_i32(&cols[1])
        })
        .collect();

    for code in REQUIRED_SRS {
        if !present.contains(code) {
            issues.push(IntegrityIssue::MissingRequiredSrs { code: *code });
        }
    }
}

/// Check 4: every `gpkg_contents.table_name` resolves to an existing table.
///
/// The contents rows are loaded from `self.contents` if they have been
/// populated via [`GeoPackage::load_contents`]; otherwise we scan the table
/// directly so callers do not need to remember to call it first.
fn check_contents_refs(gpkg: &GeoPackage, issues: &mut Vec<IntegrityIssue>) {
    let table_names = match collect_existing_table_names(gpkg) {
        Some(s) => s,
        None => return, // sqlite_master scan failed; cannot make any judgements
    };

    // Prefer in-memory contents (load_contents may have been called), but fall
    // back to a fresh scan when the vec is empty.
    if !gpkg.contents.is_empty() {
        for row in &gpkg.contents {
            if !table_names.contains(row.table_name.as_str()) {
                issues.push(IntegrityIssue::ContentsRefsMissingTable {
                    table_name: row.table_name.clone(),
                });
            }
        }
        return;
    }

    let rows = match gpkg.scan_table_by_name("gpkg_contents") {
        Ok(Some(r)) => r,
        // No gpkg_contents table at all is unusual but not strictly fatal for
        // the integrity validator — other checks will likely have flagged it.
        Ok(None) | Err(_) => return,
    };

    for (_rowid, cols) in &rows {
        if cols.is_empty() {
            continue;
        }
        let referenced = cell_to_string(&cols[0]);
        if referenced.is_empty() {
            continue;
        }
        if !table_names.contains(referenced.as_str()) {
            issues.push(IntegrityIssue::ContentsRefsMissingTable {
                table_name: referenced,
            });
        }
    }
}

/// Check 5: `gpkg_geometry_columns` references are consistent.
///
/// Each row must point at an existing user table and at an SRS row in
/// `gpkg_spatial_ref_sys`.  A missing `gpkg_geometry_columns` table is fine —
/// attribute-only GeoPackages are valid per OGC 12-128r19.
fn check_geometry_columns_refs(gpkg: &GeoPackage, issues: &mut Vec<IntegrityIssue>) {
    let rows = match gpkg.scan_table_by_name("gpkg_geometry_columns") {
        Ok(Some(r)) => r,
        // Attribute-only GPKGs are valid.  A scan error is also tolerated
        // because we cannot distinguish "missing" from "corrupt" here without
        // duplicating MissingRequiredTable diagnostics already covered above.
        Ok(None) | Err(_) => return,
    };

    let table_names = match collect_existing_table_names(gpkg) {
        Some(s) => s,
        None => return,
    };
    let srs_ids = collect_srs_ids(gpkg);

    for (_rowid, cols) in &rows {
        // Columns: table_name(0), column_name(1), geometry_type_name(2),
        //          srs_id(3), z(4), m(5).
        if cols.len() < 4 {
            continue;
        }
        let referenced_table = cell_to_string(&cols[0]);
        if !referenced_table.is_empty() && !table_names.contains(referenced_table.as_str()) {
            issues.push(IntegrityIssue::GeometryColumnsRefsMissingTable {
                table_name: referenced_table.clone(),
            });
        }
        if let Some(srs_id) = cell_to_i32(&cols[3])
            && !srs_ids.is_empty()
            && !srs_ids.contains(&srs_id)
        {
            issues.push(IntegrityIssue::GeometryColumnsRefsMissingSrs {
                table_name: referenced_table,
                srs_id,
            });
        }
    }
}

/// Check 6: every `gpkg_extensions` row with a non-NULL `table_name` points
/// at an existing user table.
///
/// Rows with NULL `table_name` are file-scoped extensions and do not need a
/// reference check.  Missing `gpkg_extensions` is allowed — many GeoPackages
/// do not use any extensions.
fn check_extensions_refs(gpkg: &GeoPackage, issues: &mut Vec<IntegrityIssue>) {
    let rows = match gpkg.scan_table_by_name("gpkg_extensions") {
        Ok(Some(r)) => r,
        Ok(None) | Err(_) => return,
    };

    let table_names = match collect_existing_table_names(gpkg) {
        Some(s) => s,
        None => return,
    };

    for (_rowid, cols) in &rows {
        // Columns: table_name(0), column_name(1), extension_name(2),
        //          definition(3), scope(4).
        if cols.len() < 3 {
            continue;
        }
        let table_name_opt = cell_to_optional_string(&cols[0]);
        let extension_name = cell_to_string(&cols[2]);
        if let Some(t) = table_name_opt
            && !table_names.contains(t.as_str())
        {
            issues.push(IntegrityIssue::OrphanedExtensionRow {
                extension_name: extension_name.clone(),
                table_name: Some(t),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Collect the names of every `table`-type entry in `sqlite_master`.
///
/// Returns `None` only when the scan itself fails (corrupt sqlite_master
/// pages, etc.).
fn collect_existing_table_names(gpkg: &GeoPackage) -> Option<HashSet<String>> {
    let entries = gpkg.scan_sqlite_master().ok()?;
    Some(
        entries
            .into_iter()
            .filter(|e| e.entry_type == "table")
            .map(|e| e.name)
            .collect(),
    )
}

/// Collect every `srs_id` present in `gpkg_spatial_ref_sys`.
///
/// Returns an empty set when the table is missing or unreadable — the caller
/// uses `is_empty()` as a sentinel to decide whether to perform SRS-reference
/// checks (cannot reliably flag a missing SRS reference when the SRS table
/// itself is unreadable).
fn collect_srs_ids(gpkg: &GeoPackage) -> HashSet<i32> {
    let rows = match gpkg.scan_table_by_name("gpkg_spatial_ref_sys") {
        Ok(Some(r)) => r,
        _ => return HashSet::new(),
    };
    rows.iter()
        .filter_map(|(_rowid, cols)| {
            if cols.len() < 2 {
                None
            } else {
                cell_to_i32(&cols[1])
            }
        })
        .collect()
}

/// Coerce a [`crate::btree::CellValue`] to `String`, mirroring the logic in
/// `gpkg.rs` but kept local so this module does not depend on private items.
fn cell_to_string(v: &crate::btree::CellValue) -> String {
    use crate::btree::CellValue;
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

/// Coerce a [`crate::btree::CellValue`] to `Option<String>`, returning `None`
/// for SQL NULL or for empty text values (matching the convention used
/// elsewhere in this crate).
fn cell_to_optional_string(v: &crate::btree::CellValue) -> Option<String> {
    use crate::btree::CellValue;
    match v {
        CellValue::Null => None,
        CellValue::Text(s) if s.is_empty() => None,
        other => Some(cell_to_string(other)),
    }
}

/// Coerce a [`crate::btree::CellValue`] to `Option<i32>`.
///
/// Returns `None` for non-integer types (including NULL).  Values outside the
/// `i32` range are saturated to `i32::MIN` or `i32::MAX` rather than rejected
/// — SRS identifiers in real-world GPKG files never exceed 6 digits.
fn cell_to_i32(v: &crate::btree::CellValue) -> Option<i32> {
    use crate::btree::CellValue;
    match v {
        CellValue::Integer(i) => {
            if *i > i32::MAX as i64 {
                Some(i32::MAX)
            } else if *i < i32::MIN as i64 {
                Some(i32::MIN)
            } else {
                Some(*i as i32)
            }
        }
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for the helper / description logic
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_description_app_id_mismatch_contains_actual_hex() {
        let issue = IntegrityIssue::AppIdMismatch {
            actual: 0xDEAD_BEEF,
        };
        let s = issue.description();
        assert!(s.contains("DEADBEEF"), "description = {s}");
        assert!(
            s.contains("47504B47"),
            "expected the GPKG magic in the message: {s}"
        );
    }

    #[test]
    fn test_description_user_version_too_old_includes_numbers() {
        let issue = IntegrityIssue::UserVersionTooOld {
            actual: 10_200,
            minimum: MIN_USER_VERSION,
        };
        let s = issue.description();
        assert!(s.contains("10200"));
        assert!(s.contains("10300"));
    }

    #[test]
    fn test_description_missing_required_srs() {
        let issue = IntegrityIssue::MissingRequiredSrs { code: 4326 };
        let s = issue.description();
        assert!(s.contains("4326"));
        assert!(s.contains("gpkg_spatial_ref_sys"));
    }

    #[test]
    fn test_description_geometry_columns_refs_missing_table() {
        let issue = IntegrityIssue::GeometryColumnsRefsMissingTable {
            table_name: "ghost".to_string(),
        };
        let s = issue.description();
        assert!(s.contains("ghost"));
    }

    #[test]
    fn test_description_orphan_extension_row_with_table() {
        let issue = IntegrityIssue::OrphanedExtensionRow {
            extension_name: "gpkg_rtree_index".to_string(),
            table_name: Some("vanished_table".to_string()),
        };
        let s = issue.description();
        assert!(s.contains("gpkg_rtree_index"));
        assert!(s.contains("vanished_table"));
    }

    #[test]
    fn test_integrity_report_passed_is_true_when_empty() {
        let report = IntegrityReport {
            passed: true,
            issues: Vec::new(),
        };
        assert!(report.passed);
        assert_eq!(report.issue_count(), 0);
        assert!(!report.has_issue_of(|_| true));
    }

    #[test]
    fn test_integrity_report_has_issue_of_matches() {
        let report = IntegrityReport {
            passed: false,
            issues: vec![IntegrityIssue::AppIdMismatch { actual: 0 }],
        };
        assert!(report.has_issue_of(|i| matches!(i, IntegrityIssue::AppIdMismatch { .. })));
        assert!(!report.has_issue_of(|i| matches!(i, IntegrityIssue::UserVersionTooOld { .. })));
    }

    #[test]
    fn test_constants_match_spec() {
        assert_eq!(GPKG_APP_ID, 0x4750_4B47);
        assert_eq!(MIN_USER_VERSION, 10_300);
        assert_eq!(REQUIRED_SRS, &[-1, 0, 4326]);
    }
}
