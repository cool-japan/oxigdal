//! STAC (SpatioTemporal Asset Catalog) quality checker.
//!
//! Validates STAC Items, Collections, and Catalogs from JSON files, checking
//! structural integrity, version compliance, bbox correctness, datetime
//! validity, and EO extension constraints.

use std::path::Path;

use crate::error::{QcIssue, QcResult, Severity};

/// Validator for STAC files (Items, Collections, Catalogs).
#[derive(Debug, Clone)]
pub struct StacValidator {
    /// Whether to apply EO-extension checks (cloud_cover range, etc.).
    pub validate_eo_extension: bool,
    /// Whether to emit warnings for deprecated STAC fields.
    pub warn_deprecated: bool,
}

impl StacValidator {
    /// Create a new validator with default settings.
    pub const fn new() -> Self {
        Self {
            validate_eo_extension: true,
            warn_deprecated: false,
        }
    }
}

impl Default for StacValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of validating a single STAC file.
#[derive(Debug, Clone)]
pub struct StacValidationResult {
    /// All issues found during validation.
    pub issues: Vec<QcIssue>,
    /// The STAC `type` field value found in the file (e.g. `"Feature"`).
    pub stac_type: Option<String>,
    /// The STAC `stac_version` field value found in the file.
    pub stac_version: Option<String>,
}

impl StacValidationResult {
    /// Returns `true` when no Major or Critical issues were found.
    pub fn is_valid(&self) -> bool {
        self.issues.iter().all(|i| i.severity < Severity::Major)
    }
}

impl StacValidator {
    /// Validate a STAC file at `path`.
    ///
    /// Reads the file, determines its STAC type, and runs all applicable checks.
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<StacValidationResult> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let loc = path.to_string_lossy().into_owned();

        let root: serde_json::Value = serde_json::from_slice(&bytes)?;

        let mut issues: Vec<QcIssue> = Vec::new();

        // ── 1. Determine STAC type ────────────────────────────────────────────
        let stac_type = root.get("type").and_then(|v| v.as_str()).map(String::from);

        if stac_type.is_none() {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "STAC",
                    "Missing type field",
                    "A STAC object must contain a 'type' field (Feature/Collection/Catalog)",
                )
                .with_location(loc.clone())
                .with_rule_id("STAC-001"),
            );
        }

        // ── 2. Validate stac_version ──────────────────────────────────────────
        let stac_version = root
            .get("stac_version")
            .and_then(|v| v.as_str())
            .map(String::from);

        if let Some(ref ver) = stac_version {
            if oxigdal_stac::StacVersion::parse(ver).is_err() {
                issues.push(
                    QcIssue::new(
                        Severity::Critical,
                        "STAC",
                        "Unsupported stac_version",
                        format!(
                            "stac_version '{}' is not supported; accepted values: 1.0.0, 1.1.0",
                            ver
                        ),
                    )
                    .with_location(loc.clone())
                    .with_rule_id("STAC-002"),
                );
            }
        } else {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "STAC",
                    "Missing stac_version field",
                    "All STAC objects must carry a 'stac_version' field",
                )
                .with_location(loc.clone())
                .with_rule_id("STAC-002"),
            );
        }

        // ── 3. Type-specific checks ───────────────────────────────────────────
        match stac_type.as_deref() {
            Some("Feature") => {
                self.check_item(&root, &bytes, &loc, &mut issues)?;
            }
            Some("Collection") => {
                check_collection_fields(&root, &loc, &mut issues);
            }
            Some("Catalog") => {
                check_catalog_fields(&root, &loc, &mut issues);
            }
            Some(other) => {
                issues.push(
                    QcIssue::new(
                        Severity::Critical,
                        "STAC",
                        "Unknown STAC type",
                        format!(
                            "Expected 'Feature', 'Collection', or 'Catalog'; found '{}'",
                            other
                        ),
                    )
                    .with_location(loc.clone())
                    .with_rule_id("STAC-003"),
                );
            }
            None => {
                // Already reported above; nothing more to check.
            }
        }

        Ok(StacValidationResult {
            issues,
            stac_type,
            stac_version,
        })
    }

    /// Run Item-specific validation (bbox, datetime, EO fields, full parse).
    fn check_item(
        &self,
        root: &serde_json::Value,
        bytes: &[u8],
        loc: &str,
        issues: &mut Vec<QcIssue>,
    ) -> QcResult<()> {
        // ── 3a. Try full Item parse & built-in validate() ────────────────────
        match serde_json::from_slice::<oxigdal_stac::Item>(bytes) {
            Ok(item) => {
                if let Err(e) = item.validate() {
                    issues.push(
                        QcIssue::new(
                            Severity::Major,
                            "STAC",
                            "Item validation failed",
                            format!("{}", e),
                        )
                        .with_location(loc.to_owned())
                        .with_rule_id("STAC-010"),
                    );
                }
            }
            Err(e) => {
                issues.push(
                    QcIssue::new(
                        Severity::Major,
                        "STAC",
                        "Item deserialization error",
                        format!("Could not parse STAC Item: {}", e),
                    )
                    .with_location(loc.to_owned())
                    .with_rule_id("STAC-011"),
                );
                // Fall through to best-effort JSON-level checks
            }
        }

        // ── 3b. bbox – 4 or 6 elements, min < max ────────────────────────────
        if let Some(bbox) = root.get("bbox") {
            check_bbox(bbox, loc, issues);
        }

        // ── 3c. datetime in properties ────────────────────────────────────────
        if let Some(props) = root.get("properties") {
            // A string `datetime` field must be valid RFC-3339.
            if let Some(dt_val) = props.get("datetime") {
                if let Some(dt_str) = dt_val.as_str() {
                    if dt_str != "null" && dt_str.parse::<chrono::DateTime<chrono::Utc>>().is_err()
                    {
                        issues.push(
                            QcIssue::new(
                                Severity::Major,
                                "STAC",
                                "Invalid datetime value",
                                format!(
                                    "properties.datetime '{}' is not a valid RFC-3339 timestamp",
                                    dt_str
                                ),
                            )
                            .with_location(loc.to_owned())
                            .with_rule_id("STAC-020"),
                        );
                    }
                }
            }

            // ── 3d. EO extension: eo:cloud_cover ∈ [0, 100] ──────────────────
            if self.validate_eo_extension {
                check_eo_cloud_cover(props, loc, issues);
            }
        }

        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Standalone check functions
// ─────────────────────────────────────────────────────────────────────────────

/// Validate a JSON bbox array: must have 4 or 6 elements, and min < max for each axis.
fn check_bbox(bbox: &serde_json::Value, loc: &str, issues: &mut Vec<QcIssue>) {
    match bbox.as_array() {
        None => {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "STAC",
                    "Invalid bbox format",
                    "bbox must be a JSON array",
                )
                .with_location(loc.to_owned())
                .with_rule_id("STAC-030"),
            );
        }
        Some(arr) if arr.len() != 4 && arr.len() != 6 => {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "STAC",
                    "Invalid bbox length",
                    format!("bbox must have 4 or 6 elements, found {}", arr.len()),
                )
                .with_location(loc.to_owned())
                .with_rule_id("STAC-031"),
            );
        }
        Some(arr) => {
            let vals: Vec<f64> = arr.iter().filter_map(|v| v.as_f64()).collect();
            if vals.len() != arr.len() {
                issues.push(
                    QcIssue::new(
                        Severity::Major,
                        "STAC",
                        "Non-numeric bbox element",
                        "All bbox elements must be finite numbers",
                    )
                    .with_location(loc.to_owned())
                    .with_rule_id("STAC-032"),
                );
                return;
            }
            // [west, south, east, north] or [west, south, min_z, east, north, max_z]
            let (min_lon, max_lon, min_lat, max_lat) = if vals.len() == 4 {
                (vals[0], vals[2], vals[1], vals[3])
            } else {
                (vals[0], vals[3], vals[1], vals[4])
            };
            if min_lon > max_lon {
                issues.push(
                    QcIssue::new(
                        Severity::Major,
                        "STAC",
                        "Invalid bbox: min_lon > max_lon",
                        format!("bbox west ({}) > east ({})", min_lon, max_lon),
                    )
                    .with_location(loc.to_owned())
                    .with_rule_id("STAC-033"),
                );
            }
            if min_lat > max_lat {
                issues.push(
                    QcIssue::new(
                        Severity::Major,
                        "STAC",
                        "Invalid bbox: min_lat > max_lat",
                        format!("bbox south ({}) > north ({})", min_lat, max_lat),
                    )
                    .with_location(loc.to_owned())
                    .with_rule_id("STAC-034"),
                );
            }
        }
    }
}

/// Validate `eo:cloud_cover` in Item properties.
fn check_eo_cloud_cover(props: &serde_json::Value, loc: &str, issues: &mut Vec<QcIssue>) {
    if let Some(cc_val) = props.get("eo:cloud_cover") {
        if let Some(cc) = cc_val.as_f64() {
            if !(0.0..=100.0).contains(&cc) {
                issues.push(
                    QcIssue::new(
                        Severity::Warning,
                        "STAC",
                        "eo:cloud_cover out of range",
                        format!("eo:cloud_cover {} is outside the valid range [0, 100]", cc),
                    )
                    .with_location(loc.to_owned())
                    .with_rule_id("STAC-040"),
                );
            }
        }
    }
}

/// Check required fields for a STAC Collection.
fn check_collection_fields(root: &serde_json::Value, loc: &str, issues: &mut Vec<QcIssue>) {
    for field in &["id", "extent", "links"] {
        if root.get(field).is_none() {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "STAC",
                    "Missing required Collection field",
                    format!("STAC Collection is missing required field '{}'", field),
                )
                .with_location(loc.to_owned())
                .with_rule_id("STAC-050"),
            );
        }
    }
}

/// Check required fields for a STAC Catalog.
fn check_catalog_fields(root: &serde_json::Value, loc: &str, issues: &mut Vec<QcIssue>) {
    for field in &["id", "stac_version", "links"] {
        if root.get(field).is_none() {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "STAC",
                    "Missing required Catalog field",
                    format!("STAC Catalog is missing required field '{}'", field),
                )
                .with_location(loc.to_owned())
                .with_rule_id("STAC-060"),
            );
        }
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

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new_in(std::env::temp_dir()).expect("temp file");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    #[test]
    fn test_stac_valid_item() {
        let json = serde_json::json!({
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "test-item-001",
            "geometry": null,
            "bbox": [-122.5, 37.5, -122.0, 38.0],
            "properties": {
                "datetime": "2023-01-01T00:00:00Z"
            },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let validator = StacValidator::new();
        let result = validator.check_file(tmp.path()).expect("check_file");
        let critical_or_major: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.severity >= Severity::Major)
            .collect();
        assert!(
            critical_or_major.is_empty(),
            "Expected no Major/Critical issues, got: {:?}",
            critical_or_major
        );
        assert!(result.is_valid());
    }

    #[test]
    fn test_stac_bad_version() {
        let json = serde_json::json!({
            "type": "Feature",
            "stac_version": "0.9.0",
            "id": "bad-ver",
            "geometry": null,
            "properties": { "datetime": "2023-01-01T00:00:00Z" },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        let has_critical = result
            .issues
            .iter()
            .any(|i| i.severity == Severity::Critical && i.rule_id.as_deref() == Some("STAC-002"));
        assert!(
            has_critical,
            "Expected STAC-002 Critical; got: {:?}",
            result.issues
        );
        assert!(!result.is_valid());
    }

    #[test]
    fn test_stac_bbox_wrong_length() {
        let json = serde_json::json!({
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "bad-bbox",
            "geometry": null,
            "bbox": [-122.5, 37.5, -122.0],
            "properties": { "datetime": "2023-01-01T00:00:00Z" },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        let has_major = result
            .issues
            .iter()
            .any(|i| i.severity >= Severity::Major && i.rule_id.as_deref() == Some("STAC-031"));
        assert!(
            has_major,
            "Expected STAC-031 Major; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn test_stac_bad_datetime() {
        let json = serde_json::json!({
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "bad-dt",
            "geometry": null,
            "properties": { "datetime": "not-a-date" },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        let has_major = result
            .issues
            .iter()
            .any(|i| i.severity >= Severity::Major && i.rule_id.as_deref() == Some("STAC-020"));
        assert!(
            has_major,
            "Expected STAC-020 Major; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn test_stac_cloud_cover_oor() {
        let json = serde_json::json!({
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "cc-oor",
            "geometry": null,
            "properties": {
                "datetime": "2023-01-01T00:00:00Z",
                "eo:cloud_cover": 150.0
            },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        let has_warning = result
            .issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.rule_id.as_deref() == Some("STAC-040"));
        assert!(
            has_warning,
            "Expected STAC-040 Warning; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn test_stac_missing_type() {
        let json = serde_json::json!({
            "stac_version": "1.0.0",
            "id": "no-type",
            "geometry": null,
            "properties": { "datetime": "2023-01-01T00:00:00Z" },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        let has_critical = result
            .issues
            .iter()
            .any(|i| i.severity == Severity::Critical && i.rule_id.as_deref() == Some("STAC-001"));
        assert!(
            has_critical,
            "Expected STAC-001 Critical; got: {:?}",
            result.issues
        );
        assert!(!result.is_valid());
    }

    #[test]
    fn test_stac_collection_valid() {
        let json = serde_json::json!({
            "type": "Collection",
            "stac_version": "1.0.0",
            "id": "my-collection",
            "description": "Test collection",
            "license": "proprietary",
            "extent": {
                "spatial": { "bbox": [[-180.0, -90.0, 180.0, 90.0]] },
                "temporal": { "interval": [["2020-01-01T00:00:00Z", null]] }
            },
            "links": []
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        assert!(
            result.is_valid(),
            "Expected valid collection; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn test_stac_catalog_valid() {
        let json = serde_json::json!({
            "type": "Catalog",
            "stac_version": "1.0.0",
            "id": "my-catalog",
            "description": "Test catalog",
            "links": []
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let result = StacValidator::new().check_file(tmp.path()).expect("ok");
        assert!(
            result.is_valid(),
            "Expected valid catalog; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn test_stac_eo_disabled_skips_cloud_cover() {
        let json = serde_json::json!({
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "cc-disabled",
            "geometry": null,
            "properties": {
                "datetime": "2023-01-01T00:00:00Z",
                "eo:cloud_cover": 999.0
            },
            "links": [],
            "assets": {}
        });
        let tmp = write_tmp(&serde_json::to_string(&json).expect("serialize"));
        let mut validator = StacValidator::new();
        validator.validate_eo_extension = false;
        let result = validator.check_file(tmp.path()).expect("ok");
        let has_warning = result
            .issues
            .iter()
            .any(|i| i.rule_id.as_deref() == Some("STAC-040"));
        assert!(
            !has_warning,
            "STAC-040 should be suppressed when eo validation is off"
        );
    }
}
