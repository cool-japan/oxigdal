//! Batch directory QC orchestration.
//!
//! Walks a directory (optionally recursively), dispatches each file to the
//! appropriate validator based on its extension, and aggregates all
//! [`crate::error::QcIssue`] entries into a [`BatchReport`].
//!
//! ## Dispatch table
//!
//! | Extension            | Validators run                                           |
//! |----------------------|----------------------------------------------------------|
//! | `.tif` / `.tiff`     | [`CogComplianceChecker`] + [`RadiometricValidator`]      |
//! | `.gpkg`              | [`GpkgValidator`]                                        |
//! | `.json` (STAC only)  | [`StacValidator`] (if `"stac_version"` found in file)    |
//! | anything else        | skipped (counted in [`BatchReport::skipped_files`])      |
//!
//! No external directory-walking crate is used; only [`std::fs::read_dir`] is
//! called, recursively when [`BatchConfig::recursive`] is set.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::error::{QcIssue, QcResult, Severity};
use crate::gpkg::GpkgValidator;
use crate::raster::cog::CogComplianceChecker;
use crate::raster::radiometric::RadiometricValidator;
use crate::stac::StacValidator;

// ── Severity counts ───────────────────────────────────────────────────────────

/// Aggregated counts of issues by severity level.
#[derive(Debug, Clone, Default)]
pub struct SeverityCounts {
    /// Number of Critical issues.
    pub critical: usize,
    /// Number of Major issues.
    pub major: usize,
    /// Number of Minor issues.
    pub minor: usize,
    /// Number of Warning issues.
    pub warning: usize,
    /// Number of Info issues.
    pub info: usize,
}

impl SeverityCounts {
    /// Accumulates counts from a slice of issues.
    fn accumulate(&mut self, issues: &[QcIssue]) {
        for issue in issues {
            match issue.severity {
                Severity::Critical => self.critical += 1,
                Severity::Major => self.major += 1,
                Severity::Minor => self.minor += 1,
                Severity::Warning => self.warning += 1,
                Severity::Info => self.info += 1,
            }
        }
    }

    /// Merges another `SeverityCounts` into this one.
    fn merge(&mut self, other: &SeverityCounts) {
        self.critical += other.critical;
        self.major += other.major;
        self.minor += other.minor;
        self.warning += other.warning;
        self.info += other.info;
    }
}

// ── Per-file result ───────────────────────────────────────────────────────────

/// QC result for a single file.
#[derive(Debug, Clone)]
pub struct FileQcResult {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// All issues raised for this file.
    pub issues: Vec<QcIssue>,
    /// Issue counts by severity.
    pub severity_counts: SeverityCounts,
}

// ── Batch report ──────────────────────────────────────────────────────────────

/// Aggregated QC report for all files in a directory.
#[derive(Debug, Clone)]
pub struct BatchReport {
    /// Per-file results (only for files that were dispatched to a validator).
    pub per_file: Vec<FileQcResult>,
    /// Aggregated severity counts across all processed files.
    pub total_severity_counts: SeverityCounts,
    /// Total number of files dispatched to a validator.
    pub total_files: usize,
    /// Total number of issues across all files.
    pub total_issues: usize,
    /// Number of files skipped (unknown/filtered extension).
    pub skipped_files: usize,
}

// ── Config ────────────────────────────────────────────────────────────────────

/// Configuration for the batch QC runner.
#[derive(Debug, Clone, Default)]
pub struct BatchConfig {
    /// If `true`, recurse into subdirectories.
    pub recursive: bool,
    /// If non-empty, only process files whose (lowercase, dot-less) extension
    /// matches one of these strings.  Example: `["tif", "gpkg"]`.
    pub extensions: Vec<String>,
}

// ── Runner ────────────────────────────────────────────────────────────────────

/// Batch QC runner.
///
/// Use [`BatchRunner::run`] to walk a directory and validate all recognised
/// file types.
pub struct BatchRunner {
    /// Runner configuration.
    pub config: BatchConfig,
}

impl BatchRunner {
    /// Creates a new `BatchRunner` with the given configuration.
    #[must_use]
    pub fn new(config: BatchConfig) -> Self {
        Self { config }
    }

    /// Walks `dir`, validates each recognised file, and returns a
    /// [`BatchReport`].
    ///
    /// Files whose extension does not match any known validator (or is filtered
    /// out by [`BatchConfig::extensions`]) are counted in
    /// [`BatchReport::skipped_files`] but do not produce issues.
    pub fn run<P: AsRef<Path>>(&self, dir: P) -> QcResult<BatchReport> {
        let mut per_file = Vec::new();
        let mut skipped = 0usize;

        self.walk(dir.as_ref(), &mut per_file, &mut skipped)?;

        let total_files = per_file.len();
        let total_issues: usize = per_file.iter().map(|r| r.issues.len()).sum();
        let mut total_severity_counts = SeverityCounts::default();
        for r in &per_file {
            total_severity_counts.merge(&r.severity_counts);
        }

        Ok(BatchReport {
            per_file,
            total_severity_counts,
            total_files,
            total_issues,
            skipped_files: skipped,
        })
    }

    /// Recursive directory walker.
    fn walk(
        &self,
        dir: &Path,
        results: &mut Vec<FileQcResult>,
        skipped: &mut usize,
    ) -> QcResult<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                return Err(crate::error::QcError::Io(err));
            }
        };

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;

            if ft.is_dir() {
                if self.config.recursive {
                    self.walk(&path, results, skipped)?;
                }
                continue;
            }

            if !ft.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();

            // Extension filter (if configured).
            if !self.config.extensions.is_empty() && !self.config.extensions.contains(&ext) {
                *skipped += 1;
                continue;
            }

            // Dispatch by extension.
            match ext.as_str() {
                "tif" | "tiff" => {
                    let issues = validate_geotiff(&path);
                    results.push(make_file_result(path, issues));
                }
                "gpkg" => {
                    let issues = validate_gpkg(&path);
                    results.push(make_file_result(path, issues));
                }
                "json" if is_stac_json(&path) => {
                    let issues = validate_stac(&path);
                    results.push(make_file_result(path, issues));
                }
                "json" => {
                    *skipped += 1;
                }
                _ => {
                    *skipped += 1;
                }
            }
        }

        Ok(())
    }
}

// ── Dispatch helpers ──────────────────────────────────────────────────────────

/// Runs COG compliance + radiometric validation on a GeoTIFF file.
///
/// Failures from either checker are converted to a Critical issue rather than
/// propagated as errors, so a single broken file does not abort the whole batch.
fn validate_geotiff(path: &Path) -> Vec<QcIssue> {
    let mut issues = Vec::new();

    // COG compliance check.
    let cog = CogComplianceChecker::default();
    match cog.check_file(path) {
        Ok(r) => issues.extend(r.issues),
        Err(e) => issues.push(open_error_issue(path, "geotiff-cog", &e.to_string())),
    }

    // Radiometric range validation.
    let radio = RadiometricValidator::default();
    match radio.check_file(path) {
        Ok(r) => issues.extend(r.issues),
        Err(e) => issues.push(open_error_issue(
            path,
            "geotiff-radiometric",
            &e.to_string(),
        )),
    }

    issues
}

/// Runs `GpkgValidator` on a GeoPackage file.
fn validate_gpkg(path: &Path) -> Vec<QcIssue> {
    let validator = GpkgValidator::new();
    match validator.check_file(path) {
        Ok(r) => r.issues,
        Err(e) => vec![open_error_issue(path, "gpkg", &e.to_string())],
    }
}

/// Runs `StacValidator` on a STAC JSON file.
fn validate_stac(path: &Path) -> Vec<QcIssue> {
    let validator = StacValidator::new();
    match validator.check_file(path) {
        Ok(r) => r.issues,
        Err(e) => vec![open_error_issue(path, "stac", &e.to_string())],
    }
}

/// Builds a Critical issue for a file that could not be opened or parsed.
fn open_error_issue(path: &Path, category: &str, detail: &str) -> QcIssue {
    QcIssue::new(
        Severity::Critical,
        category,
        "File could not be validated",
        format!(
            "{}: {}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>"),
            detail,
        ),
    )
    .with_rule_id("BATCH-OPEN-ERROR")
}

// ── STAC fingerprint ──────────────────────────────────────────────────────────

/// Returns `true` if the first 4 KiB of `path` contain `"stac_version"` or
/// `"stac_extensions"`, indicating the file is likely a STAC JSON object.
fn is_stac_json(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 4096];
    let n = file.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    let text = std::str::from_utf8(&buf).unwrap_or("");
    text.contains("\"stac_version\"") || text.contains("\"stac_extensions\"")
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn make_file_result(path: PathBuf, issues: Vec<QcIssue>) -> FileQcResult {
    let mut severity_counts = SeverityCounts::default();
    severity_counts.accumulate(&issues);
    FileQcResult {
        path,
        issues,
        severity_counts,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir(suffix: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("oxigdal_batch_test_{}", suffix));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn test_batch_empty_dir() {
        let dir = make_temp_dir("empty_dir");
        // Ensure the dir is empty.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        let runner = BatchRunner::new(BatchConfig::default());
        let report = runner.run(&dir).expect("batch run should succeed");
        assert_eq!(report.total_files, 0, "empty dir should have 0 files");
        assert_eq!(report.total_issues, 0);
        assert_eq!(report.skipped_files, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_skips_unknown_extension() {
        let dir = make_temp_dir("skip_unknown");
        let txt_path = dir.join("readme.txt");
        std::fs::write(&txt_path, b"hello").expect("write txt");

        let runner = BatchRunner::new(BatchConfig::default());
        let report = runner.run(&dir).expect("batch run");
        assert_eq!(report.total_files, 0, ".txt should not be processed");
        assert_eq!(report.skipped_files, 1, ".txt should be skipped");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_json_stac_dispatch() {
        let dir = make_temp_dir("stac_dispatch");
        let json_path = dir.join("item.json");
        let stac_json = br#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "test-item",
            "geometry": null,
            "bbox": null,
            "properties": { "datetime": null },
            "links": [],
            "assets": {}
        }"#;
        std::fs::write(&json_path, stac_json).expect("write stac json");

        let runner = BatchRunner::new(BatchConfig::default());
        let report = runner.run(&dir).expect("batch run");
        assert_eq!(
            report.total_files, 1,
            "STAC json should be processed as 1 file"
        );
        assert_eq!(report.skipped_files, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_non_stac_json_skipped() {
        let dir = make_temp_dir("non_stac_json");
        let json_path = dir.join("config.json");
        std::fs::write(&json_path, br#"{"foo": "bar"}"#).expect("write json");

        let runner = BatchRunner::new(BatchConfig::default());
        let report = runner.run(&dir).expect("batch run");
        assert_eq!(
            report.total_files, 0,
            "non-STAC json should not be processed"
        );
        assert_eq!(report.skipped_files, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_extension_filter() {
        let dir = make_temp_dir("ext_filter");
        let json_path = dir.join("item.json");
        let stac_json = br#"{"stac_version": "1.0.0", "type": "Catalog", "id": "c", "links": [], "description": "test"}"#;
        std::fs::write(&json_path, stac_json).expect("write stac json");
        std::fs::write(dir.join("data.tif"), b"not a real tiff").expect("write tif");

        // Only process .tif files → JSON should be skipped.
        let config = BatchConfig {
            recursive: false,
            extensions: vec!["tif".to_string()],
        };
        let runner = BatchRunner::new(config);
        let report = runner.run(&dir).expect("batch run");

        // The .tif will fail to open (not a real TIFF) but will be attempted,
        // meaning it will appear in per_file with error issues.
        // The .json will be counted as skipped.
        assert_eq!(
            report.skipped_files, 1,
            "json should be skipped with tif-only filter"
        );
        assert_eq!(report.total_files, 1, "only the tif should be attempted");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_recursive_true() {
        let dir = make_temp_dir("recursive_true");
        let subdir = dir.join("sub");
        std::fs::create_dir_all(&subdir).expect("create subdir");
        let json_path = subdir.join("item.json");
        let stac_json = br#"{"stac_version": "1.0.0", "type": "Catalog", "id": "x", "links": [], "description": "d"}"#;
        std::fs::write(&json_path, stac_json).expect("write stac json");

        let config = BatchConfig {
            recursive: true,
            ..Default::default()
        };
        let runner = BatchRunner::new(config);
        let report = runner.run(&dir).expect("batch run with recursive");
        assert_eq!(
            report.total_files, 1,
            "recursive should find file in subdir"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_recursive_false() {
        let dir = make_temp_dir("recursive_false");
        let subdir = dir.join("sub");
        std::fs::create_dir_all(&subdir).expect("create subdir");
        let json_path = subdir.join("item.json");
        let stac_json = br#"{"stac_version": "1.0.0", "type": "Catalog", "id": "y", "links": [], "description": "d"}"#;
        std::fs::write(&json_path, stac_json).expect("write stac json");

        let config = BatchConfig {
            recursive: false,
            ..Default::default()
        };
        let runner = BatchRunner::new(config);
        let report = runner.run(&dir).expect("batch run non-recursive");
        assert_eq!(
            report.total_files, 0,
            "non-recursive should not find file in subdir"
        );
        assert_eq!(
            report.skipped_files, 0,
            "subdirs are not counted as skipped"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_severity_counts_accumulate() {
        let mut counts = SeverityCounts::default();
        let issues = vec![
            QcIssue::new(Severity::Critical, "test", "c", "c"),
            QcIssue::new(Severity::Major, "test", "m", "m"),
            QcIssue::new(Severity::Warning, "test", "w", "w"),
            QcIssue::new(Severity::Info, "test", "i", "i"),
        ];
        counts.accumulate(&issues);
        assert_eq!(counts.critical, 1);
        assert_eq!(counts.major, 1);
        assert_eq!(counts.warning, 1);
        assert_eq!(counts.info, 1);
        assert_eq!(counts.minor, 0);
    }

    #[test]
    fn test_severity_counts_merge() {
        let mut a = SeverityCounts {
            critical: 2,
            major: 1,
            minor: 0,
            warning: 0,
            info: 0,
        };
        let b = SeverityCounts {
            critical: 1,
            major: 0,
            minor: 3,
            warning: 0,
            info: 0,
        };
        a.merge(&b);
        assert_eq!(a.critical, 3);
        assert_eq!(a.major, 1);
        assert_eq!(a.minor, 3);
    }
}
