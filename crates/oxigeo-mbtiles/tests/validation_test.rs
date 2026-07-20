//! MBTiles 1.3 metadata compliance validator tests.
//!
//! Every [`MBTilesMetadata`] under test is built through the public
//! [`MBTilesMetadata::from_map`] constructor (never a struct literal), so the
//! tests remain valid as the struct gains fields.

use std::collections::HashMap;

use oxigeo_mbtiles::mbtiles::MBTilesMetadata;
use oxigeo_mbtiles::validation::{IssueSeverity, ValidationIssue};
use oxigeo_mbtiles::writer::TileScheme;

/// Build metadata from `(key, value)` string pairs via the real constructor.
fn meta_from(pairs: &[(&str, &str)]) -> MBTilesMetadata {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    MBTilesMetadata::from_map(map)
}

/// `true` if any issue equals `expected`.
fn contains_issue(issues: &[ValidationIssue], expected: &ValidationIssue) -> bool {
    issues.iter().any(|i| i == expected)
}

/// `true` if any issue is an error.
fn has_error(issues: &[ValidationIssue]) -> bool {
    issues.iter().any(|i| i.severity() == IssueSeverity::Error)
}

/// A complete, spec-compliant metadata key set.
fn valid_full_pairs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("name", "Example Tiles"),
        ("format", "pbf"),
        ("bounds", "-180.0,-85.0,180.0,85.0"),
        ("center", "0.0,0.0,3.0"),
        ("minzoom", "0"),
        ("maxzoom", "14"),
        ("type", "overlay"),
        ("version", "1.3.0"),
        ("attribution", "© OxiGeo"),
        ("description", "A test tileset"),
    ]
}

#[test]
fn test_validator_accepts_valid_min_set() {
    // Minimal compliant set: required keys + recommended keys all present.
    let meta = meta_from(&[
        ("name", "Minimal"),
        ("format", "png"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    assert!(
        issues.is_empty(),
        "expected no issues for a minimal valid set, got: {issues:?}"
    );
}

#[test]
fn test_validator_flags_missing_format() {
    let meta = meta_from(&[
        ("name", "No Format"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let expected = ValidationIssue::MissingRequiredKey("format".to_string());
    assert!(
        contains_issue(&issues, &expected),
        "expected missing-format error, got: {issues:?}"
    );
    assert_eq!(expected.severity(), IssueSeverity::Error);
    assert!(has_error(&issues));
}

#[test]
fn test_validator_flags_missing_name() {
    let meta = meta_from(&[
        ("format", "png"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let expected = ValidationIssue::MissingRequiredKey("name".to_string());
    assert!(
        contains_issue(&issues, &expected),
        "expected missing-name error, got: {issues:?}"
    );
    assert_eq!(expected.severity(), IssueSeverity::Error);
}

#[test]
fn test_validator_flags_bounds_outside_180() {
    // maxlon = 190 is outside [-180, 180].
    let meta = meta_from(&[
        ("name", "Bad Bounds"),
        ("format", "png"),
        ("bounds", "-10.0,-10.0,190.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let bounds_errors = issues
        .iter()
        .filter(|i| matches!(i, ValidationIssue::InvalidBounds(_)))
        .count();
    assert!(
        bounds_errors >= 1,
        "expected an InvalidBounds error for longitude 190, got: {issues:?}"
    );
    for issue in &issues {
        if let ValidationIssue::InvalidBounds(_) = issue {
            assert_eq!(issue.severity(), IssueSeverity::Error);
        }
    }
}

#[test]
fn test_validator_flags_lat_outside_90() {
    // minlat = -95 is outside [-90, 90].
    let meta = meta_from(&[
        ("name", "Bad Lat"),
        ("format", "png"),
        ("bounds", "-10.0,-95.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, ValidationIssue::InvalidBounds(_))),
        "expected an InvalidBounds error for latitude -95, got: {issues:?}"
    );
    assert!(has_error(&issues));
}

#[test]
fn test_validator_flags_minzoom_greater_than_maxzoom() {
    let meta = meta_from(&[
        ("name", "Inverted Zoom"),
        ("format", "png"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "12"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let expected = ValidationIssue::MinZoomGreaterThanMaxZoom {
        minzoom: 12,
        maxzoom: 5,
    };
    assert!(
        contains_issue(&issues, &expected),
        "expected MinZoomGreaterThanMaxZoom, got: {issues:?}"
    );
    assert_eq!(expected.severity(), IssueSeverity::Error);
}

#[test]
fn test_validator_flags_zoom_above_30() {
    // maxzoom = 31 exceeds the supported 0..=30 range.
    let meta = meta_from(&[
        ("name", "Too Deep"),
        ("format", "png"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "31"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let expected = ValidationIssue::ZoomOutOfRange { value: 31 };
    assert!(
        contains_issue(&issues, &expected),
        "expected ZoomOutOfRange {{ value: 31 }}, got: {issues:?}"
    );
    assert_eq!(expected.severity(), IssueSeverity::Error);
}

#[test]
fn test_validator_warns_unknown_format() {
    // `gif` is not a recognised MBTiles 1.3 format.
    let meta = meta_from(&[
        ("name", "Weird Format"),
        ("format", "gif"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let expected = ValidationIssue::UnknownFormat("gif".to_string());
    assert!(
        contains_issue(&issues, &expected),
        "expected UnknownFormat(\"gif\"), got: {issues:?}"
    );
    assert_eq!(expected.severity(), IssueSeverity::Warning);
    // An unknown (but present) format is a warning, not a hard error.
    assert!(
        !has_error(&issues),
        "unknown format alone must not be an error: {issues:?}"
    );
}

#[test]
fn test_validator_flags_invalid_type() {
    let meta = meta_from(&[
        ("name", "Bad Type"),
        ("format", "png"),
        ("type", "satellite"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    let expected = ValidationIssue::InvalidType("satellite".to_string());
    assert!(
        contains_issue(&issues, &expected),
        "expected InvalidType(\"satellite\"), got: {issues:?}"
    );
    assert_eq!(expected.severity(), IssueSeverity::Warning);
}

#[test]
fn test_validator_center_outside_bounds() {
    // Center longitude 50 lies east of maxlon 10.
    let meta = meta_from(&[
        ("name", "Off-Center"),
        ("format", "png"),
        ("bounds", "-10.0,-10.0,10.0,10.0"),
        ("center", "50.0,0.0,3.0"),
        ("minzoom", "0"),
        ("maxzoom", "5"),
    ]);
    let issues = meta.validate(TileScheme::Xyz);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, ValidationIssue::InvalidCenter(_))),
        "expected an InvalidCenter issue, got: {issues:?}"
    );
    for issue in &issues {
        if let ValidationIssue::InvalidCenter(_) = issue {
            assert_eq!(issue.severity(), IssueSeverity::Warning);
        }
    }
}

#[test]
fn test_validator_valid_full_metadata_no_errors() {
    let meta = meta_from(&valid_full_pairs());
    let issues = meta.validate(TileScheme::Xyz);
    assert!(
        issues.is_empty(),
        "expected fully valid metadata to produce no issues, got: {issues:?}"
    );
    assert!(!has_error(&issues));
}

#[test]
fn test_validator_missing_recommended_keys_are_not_errors() {
    // Only the required keys are present; recommended keys are absent and must
    // surface as Warning/Info, never Error.
    let meta = meta_from(&[("name", "Required Only"), ("format", "pbf")]);
    let issues = meta.validate(TileScheme::Xyz);
    assert!(
        !has_error(&issues),
        "required-only metadata must have no errors, got: {issues:?}"
    );
    assert!(contains_issue(
        &issues,
        &ValidationIssue::MissingRequiredKey("bounds".to_string())
    ));
    assert_eq!(
        ValidationIssue::MissingRequiredKey("minzoom".to_string()).severity(),
        IssueSeverity::Info
    );
    assert_eq!(
        ValidationIssue::MissingRequiredKey("bounds".to_string()).severity(),
        IssueSeverity::Warning
    );
}

#[test]
fn test_validator_display_messages_are_nonempty() {
    // Each issue must render a human-readable, non-empty message.
    let meta = meta_from(&[("format", "tiff"), ("minzoom", "9"), ("maxzoom", "2")]);
    let issues = meta.validate(TileScheme::Xyz);
    assert!(!issues.is_empty());
    for issue in &issues {
        let rendered = issue.to_string();
        assert!(
            !rendered.trim().is_empty(),
            "issue {issue:?} rendered an empty message"
        );
        // Severity also renders to a known token.
        let sev = issue.severity().to_string();
        assert!(matches!(sev.as_str(), "error" | "warning" | "info"));
    }
}

#[test]
fn test_validator_scheme_does_not_change_results() {
    // The WGS84 range checks are identical for both schemes.
    let pairs = valid_full_pairs();
    let meta = meta_from(&pairs);
    let xyz = meta.validate(TileScheme::Xyz);
    let tms = meta.validate(TileScheme::Tms);
    assert_eq!(xyz, tms);
}
