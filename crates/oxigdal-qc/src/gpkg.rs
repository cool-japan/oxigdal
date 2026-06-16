//! GeoPackage quality checker.
//!
//! Validates GeoPackage files by checking the SQLite magic header, GeoPackage
//! application ID, user version, and structural integrity via `check_integrity`.

use std::path::Path;

use crate::error::{QcError, QcIssue, QcResult, Severity};

/// GeoPackage application ID — ASCII "GPKG" packed big-endian.
const GPKG_APP_ID: u32 = oxigdal_gpkg::GPKG_APP_ID;

/// Minimum acceptable user_version (GeoPackage 1.3.0 = 10_300).
const MIN_USER_VERSION: u32 = oxigdal_gpkg::MIN_USER_VERSION;

/// SQLite magic bytes present at the beginning of every valid SQLite database.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\x00";

/// Validator for GeoPackage files.
#[derive(Debug, Clone)]
pub struct GpkgValidator {
    /// When `true`, verify that `gpkg_contents` exists in `sqlite_master`.
    pub verify_table_existence: bool,
}

impl GpkgValidator {
    /// Create a new validator with default settings.
    pub const fn new() -> Self {
        Self {
            verify_table_existence: true,
        }
    }
}

impl Default for GpkgValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of validating a single GeoPackage file.
#[derive(Debug, Clone)]
pub struct GpkgValidationResult {
    /// All issues found during validation.
    pub issues: Vec<QcIssue>,
    /// The `application_id` read from the SQLite header (offset 68).
    pub application_id: Option<u32>,
    /// The `user_version` read from the SQLite header (offset 60).
    pub user_version: Option<u32>,
    /// Number of rows found in `gpkg_contents` (0 when the table is absent).
    pub table_count: usize,
}

impl GpkgValidationResult {
    /// Returns `true` when no Major or Critical issues were found.
    pub fn is_valid(&self) -> bool {
        self.issues.iter().all(|i| i.severity < Severity::Major)
    }
}

impl GpkgValidator {
    /// Validate the GeoPackage file at `path`.
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<GpkgValidationResult> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let loc = path.to_string_lossy().into_owned();

        let mut issues: Vec<QcIssue> = Vec::new();

        // ── 1. SQLite magic check ─────────────────────────────────────────────
        if bytes.len() < 16 || &bytes[..16] != SQLITE_MAGIC.as_ref() {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "GPKG",
                    "Not a valid SQLite file",
                    "File does not start with the SQLite magic bytes ('SQLite format 3\\0')",
                )
                .with_location(loc.clone())
                .with_rule_id("GPKG-001"),
            );
            // Without a valid SQLite header we cannot proceed further.
            return Ok(GpkgValidationResult {
                issues,
                application_id: None,
                user_version: None,
                table_count: 0,
            });
        }

        // ── 2. Parse the GeoPackage ───────────────────────────────────────────
        let mut gpkg = oxigdal_gpkg::GeoPackage::from_bytes(bytes.clone())
            .map_err(|e| QcError::InvalidInput(format!("GeoPackage parse error: {}", e)))?;

        let application_id = gpkg.reader.header.application_id;
        let user_version = gpkg.reader.header.user_version;

        // ── 3. application_id must be 0x47504B47 ("GPKG") ────────────────────
        if application_id != GPKG_APP_ID {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "GPKG",
                    "Wrong application_id",
                    format!(
                        "SQLite application_id is 0x{:08X}; a GeoPackage must use 0x{:08X} (\"GPKG\")",
                        application_id, GPKG_APP_ID
                    ),
                )
                .with_location(loc.clone())
                .with_rule_id("GPKG-002"),
            );
        }

        // ── 4. user_version must be at least 10_300 (GeoPackage 1.3.0) ───────
        if user_version < MIN_USER_VERSION {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "GPKG",
                    "user_version too old",
                    format!(
                        "GeoPackage user_version is {} but {} (1.3.0) or higher is required",
                        user_version, MIN_USER_VERSION
                    ),
                )
                .with_location(loc.clone())
                .with_rule_id("GPKG-003"),
            );
        }

        // ── 5. Structural integrity via check_integrity ───────────────────────
        let _ = gpkg.load_contents(); // best-effort; ignore errors
        let report = oxigdal_gpkg::check_integrity(&gpkg);
        for issue in &report.issues {
            let (severity, rule_id) = integrity_issue_severity(issue);
            issues.push(
                QcIssue::new(
                    severity,
                    "GPKG",
                    "Integrity check failed",
                    issue.description(),
                )
                .with_location(loc.clone())
                .with_rule_id(rule_id),
            );
        }

        // ── 6. gpkg_contents table existence check ────────────────────────────
        let table_count = if self.verify_table_existence {
            check_contents_table(&gpkg, &loc, &mut issues)?
        } else {
            0
        };

        Ok(GpkgValidationResult {
            issues,
            application_id: Some(application_id),
            user_version: Some(user_version),
            table_count,
        })
    }
}

/// Map an [`oxigdal_gpkg::IntegrityIssue`] variant to a QC severity and rule ID.
fn integrity_issue_severity(issue: &oxigdal_gpkg::IntegrityIssue) -> (Severity, &'static str) {
    use oxigdal_gpkg::IntegrityIssue;
    match issue {
        IntegrityIssue::AppIdMismatch { .. } => (Severity::Critical, "GPKG-010"),
        IntegrityIssue::UserVersionTooOld { .. } => (Severity::Major, "GPKG-011"),
        IntegrityIssue::MissingRequiredTable(_) => (Severity::Critical, "GPKG-012"),
        IntegrityIssue::MissingRequiredSrs { .. } => (Severity::Major, "GPKG-013"),
        IntegrityIssue::ContentsRefsMissingTable { .. } => (Severity::Major, "GPKG-014"),
        IntegrityIssue::GeometryColumnsRefsMissingTable { .. } => (Severity::Major, "GPKG-015"),
        IntegrityIssue::GeometryColumnsRefsMissingSrs { .. } => (Severity::Minor, "GPKG-016"),
        IntegrityIssue::OrphanedExtensionRow { .. } => (Severity::Warning, "GPKG-017"),
    }
}

/// Check that `gpkg_contents` exists and return the row count.
fn check_contents_table(
    gpkg: &oxigdal_gpkg::GeoPackage,
    loc: &str,
    issues: &mut Vec<QcIssue>,
) -> QcResult<usize> {
    match gpkg
        .scan_table_by_name("gpkg_contents")
        .map_err(|e| QcError::InvalidInput(format!("gpkg_contents scan error: {}", e)))?
    {
        None => {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "GPKG",
                    "Missing gpkg_contents table",
                    "Required system table 'gpkg_contents' was not found in sqlite_master",
                )
                .with_location(loc.to_owned())
                .with_rule_id("GPKG-020"),
            );
            Ok(0)
        }
        Some(rows) => Ok(rows.len()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// Write `bytes` to a temp file and return it (keeping the handle alive).
    fn write_tmp_bytes(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new_in(std::env::temp_dir()).expect("temp file");
        f.write_all(bytes).expect("write bytes");
        f
    }

    /// Build the 100-byte SQLite header with configurable app_id and user_version.
    ///
    /// The header is padded to `page_size` bytes (default 4096) so the SQLite
    /// reader can parse it without `InvalidFormat` on page boundaries.
    fn make_minimal_sqlite(app_id: u32, user_version: u32) -> Vec<u8> {
        const PAGE_SIZE: usize = 4096;
        let mut data = vec![0u8; PAGE_SIZE];
        // Magic
        data[..16].copy_from_slice(b"SQLite format 3\x00");
        // Page size (big-endian u16 at offset 16), 4096 = 0x1000
        data[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
        // file format read/write versions
        data[18] = 1;
        data[19] = 1;
        // max/min embedded payload fractions + leaf payload fraction
        data[21] = 64;
        data[22] = 32;
        data[23] = 32;
        // change counter = 1
        data[24..28].copy_from_slice(&1u32.to_be_bytes());
        // db size in pages = 1
        data[28..32].copy_from_slice(&1u32.to_be_bytes());
        // schema cookie = 1
        data[40..44].copy_from_slice(&1u32.to_be_bytes());
        // schema format = 4
        data[44] = 4;
        // text encoding = UTF-8 (1) at offset 56
        data[56..60].copy_from_slice(&1u32.to_be_bytes());
        // user_version at offset 60
        data[60..64].copy_from_slice(&user_version.to_be_bytes());
        // application_id at offset 68
        data[68..72].copy_from_slice(&app_id.to_be_bytes());
        // version-valid-for at offset 92, sqlite version at 96
        data[92..96].copy_from_slice(&1u32.to_be_bytes());
        data[96..100].copy_from_slice(&3_040_001u32.to_be_bytes());
        // Page 1 B-tree header: leaf table page (type 13) with 0 cells
        data[100] = 13; // leaf table B-tree page
        data[101..103].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes()); // first freeblock = end (none)
        // cell count = 0 at offset 103
        data[103..105].copy_from_slice(&0u16.to_be_bytes());
        // cell content area start: 0 means 65536 but we use page_size
        data[105..107].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
        data
    }

    #[test]
    fn test_gpkg_not_sqlite() {
        let garbage = b"This is not a SQLite file at all, just garbage bytes!!!!!";
        let tmp = write_tmp_bytes(garbage);
        let result = GpkgValidator::new()
            .check_file(tmp.path())
            .expect("check_file ok");
        let has_critical = result
            .issues
            .iter()
            .any(|i| i.severity == Severity::Critical && i.rule_id.as_deref() == Some("GPKG-001"));
        assert!(
            has_critical,
            "Expected GPKG-001 Critical; got: {:?}",
            result.issues
        );
        assert!(!result.is_valid());
    }

    #[test]
    fn test_gpkg_wrong_app_id() {
        // Valid SQLite header but wrong application_id
        let bytes = make_minimal_sqlite(0xDEAD_BEEF, MIN_USER_VERSION);
        let tmp = write_tmp_bytes(&bytes);
        let result = GpkgValidator::new()
            .check_file(tmp.path())
            .expect("check_file ok");
        let has_critical = result
            .issues
            .iter()
            .any(|i| i.severity == Severity::Critical && i.rule_id.as_deref() == Some("GPKG-002"));
        assert!(
            has_critical,
            "Expected GPKG-002 Critical; got: {:?}",
            result.issues
        );
        assert!(!result.is_valid());
    }

    #[test]
    fn test_gpkg_old_user_version() {
        // Valid app_id but user_version = 10_200 (GeoPackage 1.2.0) — below 10_300
        let bytes = make_minimal_sqlite(GPKG_APP_ID, 10_200);
        let tmp = write_tmp_bytes(&bytes);
        let result = GpkgValidator::new()
            .check_file(tmp.path())
            .expect("check_file ok");
        let has_major = result
            .issues
            .iter()
            .any(|i| i.severity >= Severity::Major && i.rule_id.as_deref() == Some("GPKG-003"));
        assert!(
            has_major,
            "Expected GPKG-003 Major; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn test_gpkg_minimal_valid_from_builder() {
        // Use GeoPackageBuilder to create a structurally valid empty GeoPackage
        let bytes = oxigdal_gpkg::GeoPackageBuilder::new(4326)
            .build()
            .expect("builder");
        let tmp = write_tmp_bytes(&bytes);
        let result = GpkgValidator::new()
            .check_file(tmp.path())
            .expect("check_file ok");
        // A freshly-built GeoPackage should have no Major or Critical issues
        let major_or_critical: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.severity >= Severity::Major)
            .collect();
        assert!(
            major_or_critical.is_empty(),
            "Expected no Major/Critical on a fresh GeoPackage; got: {:?}",
            major_or_critical
        );
        assert!(result.is_valid());
        assert_eq!(result.application_id, Some(GPKG_APP_ID));
    }

    #[test]
    fn test_gpkg_validation_result_is_valid_logic() {
        // A result with only warnings should be considered valid
        let result = GpkgValidationResult {
            issues: vec![QcIssue::new(
                Severity::Warning,
                "GPKG",
                "test",
                "test warning",
            )],
            application_id: Some(GPKG_APP_ID),
            user_version: Some(MIN_USER_VERSION),
            table_count: 0,
        };
        assert!(result.is_valid());

        // A result with a Major issue should not be valid
        let result_bad = GpkgValidationResult {
            issues: vec![QcIssue::new(Severity::Major, "GPKG", "test", "test major")],
            application_id: Some(GPKG_APP_ID),
            user_version: Some(MIN_USER_VERSION),
            table_count: 0,
        };
        assert!(!result_bad.is_valid());
    }
}
