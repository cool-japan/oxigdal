//! MBTiles 1.3 metadata compliance validator.
//!
//! [`MBTilesMetadata::from_map`](crate::mbtiles::MBTilesMetadata::from_map)
//! silently accepts any subset of `metadata` keys. This module adds a
//! spec-compliance checker that reports violations of the
//! [MBTiles 1.3 specification](https://github.com/mapbox/mbtiles-spec/blob/master/1.3/spec.md)
//! as a list of [`ValidationIssue`]s.
//!
//! The structure mirrors the `GeoJsonValidator` in `oxigdal-geojson`: a
//! validation-issue enum, an [`IssueSeverity`] classification, a human
//! [`Display`](std::fmt::Display) rendering, and a set of small helper
//! checks driven from a single entry point.
//!
//! ## Severity policy (MBTiles 1.3)
//!
//! * `name` and `format` are **required** — their absence is an
//!   [`IssueSeverity::Error`].
//! * `bounds`, `minzoom`, `maxzoom` are **recommended** — their absence is an
//!   [`IssueSeverity::Warning`] (`bounds`) or [`IssueSeverity::Info`]
//!   (zoom hints), never an error.
//! * Values that are present but out of range / inconsistent are reported at
//!   the severity defined per rule below.

use std::fmt;

use crate::mbtiles::MBTilesMetadata;
use crate::tile_coords::TileFormat;
use crate::writer::TileScheme;

/// Classification of a [`ValidationIssue`].
///
/// `Error` marks a hard MBTiles 1.3 conformance violation; `Warning` marks a
/// value that is suspect but tolerated by most readers; `Info` marks a purely
/// advisory note (typically a missing *recommended* key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// A hard conformance violation — the archive is non-compliant.
    Error,
    /// A suspect value that most readers tolerate.
    Warning,
    /// An advisory note (e.g. a missing recommended key).
    Info,
}

impl fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        };
        f.write_str(s)
    }
}

/// A single MBTiles 1.3 metadata conformance finding.
///
/// Each variant carries enough context to render an actionable message via
/// its [`Display`](std::fmt::Display) implementation.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationIssue {
    /// A required (`name`/`format`) or recommended (`bounds`/`minzoom`/
    /// `maxzoom`) metadata key was absent. The string is the key name.
    MissingRequiredKey(String),
    /// The `bounds` value is malformed or out of the WGS84 range. The string
    /// describes the specific problem.
    InvalidBounds(String),
    /// The `center` value is malformed, out of range, or lies outside
    /// `bounds`. The string describes the specific problem.
    InvalidCenter(String),
    /// A zoom value lies outside the supported `0..=30` range.
    ZoomOutOfRange {
        /// The offending zoom value (carried as `i64` so callers can surface
        /// values that overflow the natural `u8` storage of a zoom level).
        value: i64,
    },
    /// `minzoom` is strictly greater than `maxzoom`.
    MinZoomGreaterThanMaxZoom {
        /// The declared minimum zoom level.
        minzoom: u8,
        /// The declared maximum zoom level.
        maxzoom: u8,
    },
    /// The `format` value is not one of the MBTiles 1.3 recognised formats
    /// (`pbf`, `jpg`, `png`, `webp`). The string is the offending value.
    UnknownFormat(String),
    /// The `type` value is not one of `overlay` / `baselayer`. The string is
    /// the offending value.
    InvalidType(String),
}

impl ValidationIssue {
    /// Return the [`IssueSeverity`] of this issue per the MBTiles 1.3 policy.
    #[must_use]
    pub fn severity(&self) -> IssueSeverity {
        match self {
            // Missing `name`/`format` are errors; missing recommended keys are
            // softer. The distinction is encoded by the key name carried in the
            // variant, so callers get the right severity without a second enum.
            Self::MissingRequiredKey(key) => match key.as_str() {
                "name" | "format" => IssueSeverity::Error,
                "bounds" => IssueSeverity::Warning,
                "minzoom" | "maxzoom" => IssueSeverity::Info,
                _ => IssueSeverity::Warning,
            },
            Self::InvalidBounds(_) => IssueSeverity::Error,
            Self::ZoomOutOfRange { .. } => IssueSeverity::Error,
            Self::MinZoomGreaterThanMaxZoom { .. } => IssueSeverity::Error,
            Self::InvalidCenter(_) => IssueSeverity::Warning,
            Self::UnknownFormat(_) => IssueSeverity::Warning,
            Self::InvalidType(_) => IssueSeverity::Warning,
        }
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredKey(key) => {
                write!(f, "missing metadata key `{key}`")
            }
            Self::InvalidBounds(detail) => {
                write!(f, "invalid `bounds`: {detail}")
            }
            Self::InvalidCenter(detail) => {
                write!(f, "invalid `center`: {detail}")
            }
            Self::ZoomOutOfRange { value } => {
                write!(
                    f,
                    "zoom level {value} is outside the supported range 0..=30"
                )
            }
            Self::MinZoomGreaterThanMaxZoom { minzoom, maxzoom } => {
                write!(f, "minzoom ({minzoom}) is greater than maxzoom ({maxzoom})")
            }
            Self::UnknownFormat(value) => {
                write!(
                    f,
                    "unrecognised `format` value `{value}` (expected one of pbf, jpg, png, webp)"
                )
            }
            Self::InvalidType(value) => {
                write!(
                    f,
                    "unrecognised `type` value `{value}` (expected `overlay` or `baselayer`)"
                )
            }
        }
    }
}

// ─── Geographic limits (WGS84) ─────────────────────────────────────────────────

/// Maximum supported tile zoom level per MBTiles 1.3 (inclusive).
const MAX_ZOOM: i64 = 30;
/// Longitude bound magnitude (degrees).
const LON_LIMIT: f64 = 180.0;
/// Latitude bound magnitude (degrees).
const LAT_LIMIT: f64 = 90.0;

// ─── Entry point ───────────────────────────────────────────────────────────────

/// Validate `metadata` against the MBTiles 1.3 spec, returning every issue
/// found. The result is empty for fully compliant metadata.
///
/// `scheme` records the tile-coordinate convention the archive declares; the
/// MBTiles 1.3 geographic checks (`bounds`/`center` ranges) are identical for
/// both [`TileScheme::Tms`] and [`TileScheme::Xyz`], so it is accepted for API
/// completeness and forward compatibility rather than to alter range limits.
///
/// Only `&self`-readable fields/accessors of [`MBTilesMetadata`] are consulted,
/// so this function is unaffected by additional fields the struct may gain.
#[must_use]
pub fn validate_metadata(metadata: &MBTilesMetadata, scheme: TileScheme) -> Vec<ValidationIssue> {
    // `scheme` does not change the WGS84 range limits below; bind it explicitly
    // so the parameter is documented as intentionally range-neutral.
    let _ = scheme;

    let mut issues = Vec::new();

    check_required_keys(metadata, &mut issues);
    check_format(metadata, &mut issues);
    check_type(metadata, &mut issues);
    check_bounds(metadata, &mut issues);
    check_zooms(metadata, &mut issues);
    check_center(metadata, &mut issues);

    issues
}

// ─── Individual rule checks ──────────────────────────────────────────────────────

/// Required keys (`name`, `format`) and recommended keys (`bounds`, `minzoom`,
/// `maxzoom`). Absence is reported via [`ValidationIssue::MissingRequiredKey`]
/// whose severity is resolved from the key name.
fn check_required_keys(metadata: &MBTilesMetadata, issues: &mut Vec<ValidationIssue>) {
    if metadata.name.is_none() {
        issues.push(ValidationIssue::MissingRequiredKey("name".to_string()));
    }
    if metadata.format.is_none() {
        issues.push(ValidationIssue::MissingRequiredKey("format".to_string()));
    }
    if metadata.bounds.is_none() {
        issues.push(ValidationIssue::MissingRequiredKey("bounds".to_string()));
    }
    if metadata.minzoom.is_none() {
        issues.push(ValidationIssue::MissingRequiredKey("minzoom".to_string()));
    }
    if metadata.maxzoom.is_none() {
        issues.push(ValidationIssue::MissingRequiredKey("maxzoom".to_string()));
    }
}

/// `format` must be one of the MBTiles 1.3 recognised values. A present-but-
/// unrecognised value parses to [`TileFormat::Unknown`]; a missing key is
/// already reported by [`check_required_keys`], so it is skipped here.
fn check_format(metadata: &MBTilesMetadata, issues: &mut Vec<ValidationIssue>) {
    // A present-but-unrecognised `format` parses to `TileFormat::Unknown`,
    // which carries the offending string. Recognised variants
    // (png/jpg/png/webp/pbf) are spec-compliant and need no report. We test
    // only for the `Unknown` bucket, which avoids an exhaustive `match` and
    // therefore stays robust if `TileFormat` gains further variants. A missing
    // `format` key is reported separately by `check_required_keys`.
    if let Some(TileFormat::Unknown(value)) = &metadata.format {
        issues.push(ValidationIssue::UnknownFormat(value.clone()));
    }
}

/// `type`, when present, must be `overlay` or `baselayer`.
fn check_type(metadata: &MBTilesMetadata, issues: &mut Vec<ValidationIssue>) {
    if let Some(tile_type) = &metadata.tile_type {
        let normalised = tile_type.trim().to_lowercase();
        if normalised != "overlay" && normalised != "baselayer" {
            issues.push(ValidationIssue::InvalidType(tile_type.clone()));
        }
    }
}

/// `bounds` is `minlon,minlat,maxlon,maxlat`. Each coordinate must sit within
/// the WGS84 range and the minimum corner must be strictly south-west of the
/// maximum corner.
fn check_bounds(metadata: &MBTilesMetadata, issues: &mut Vec<ValidationIssue>) {
    if let Some([min_lon, min_lat, max_lon, max_lat]) = bounds_of(metadata) {
        if !min_lon.is_finite()
            || !min_lat.is_finite()
            || !max_lon.is_finite()
            || !max_lat.is_finite()
        {
            issues.push(ValidationIssue::InvalidBounds(
                "coordinates must be finite numbers".to_string(),
            ));
            return;
        }
        if min_lon.abs() > LON_LIMIT || max_lon.abs() > LON_LIMIT {
            issues.push(ValidationIssue::InvalidBounds(format!(
                "longitude out of [-180, 180]: min={min_lon}, max={max_lon}"
            )));
        }
        if min_lat.abs() > LAT_LIMIT || max_lat.abs() > LAT_LIMIT {
            issues.push(ValidationIssue::InvalidBounds(format!(
                "latitude out of [-90, 90]: min={min_lat}, max={max_lat}"
            )));
        }
        if min_lon >= max_lon {
            issues.push(ValidationIssue::InvalidBounds(format!(
                "minlon ({min_lon}) must be less than maxlon ({max_lon})"
            )));
        }
        if min_lat >= max_lat {
            issues.push(ValidationIssue::InvalidBounds(format!(
                "minlat ({min_lat}) must be less than maxlat ({max_lat})"
            )));
        }
    }
}

/// `minzoom`/`maxzoom` must each be within `0..=30` and `minzoom ≤ maxzoom`.
fn check_zooms(metadata: &MBTilesMetadata, issues: &mut Vec<ValidationIssue>) {
    if let Some(minzoom) = metadata.minzoom {
        if i64::from(minzoom) > MAX_ZOOM {
            issues.push(ValidationIssue::ZoomOutOfRange {
                value: i64::from(minzoom),
            });
        }
    }
    if let Some(maxzoom) = metadata.maxzoom {
        if i64::from(maxzoom) > MAX_ZOOM {
            issues.push(ValidationIssue::ZoomOutOfRange {
                value: i64::from(maxzoom),
            });
        }
    }
    if let (Some(minzoom), Some(maxzoom)) = (metadata.minzoom, metadata.maxzoom) {
        if minzoom > maxzoom {
            issues.push(ValidationIssue::MinZoomGreaterThanMaxZoom { minzoom, maxzoom });
        }
    }
}

/// `center` is `lon,lat,zoom`. The longitude/latitude must be in range, the
/// zoom must be within `0..=30`, and the point must fall inside `bounds` when
/// `bounds` is present and itself valid.
fn check_center(metadata: &MBTilesMetadata, issues: &mut Vec<ValidationIssue>) {
    let Some([lon, lat, zoom]) = center_of(metadata) else {
        return;
    };

    if !lon.is_finite() || !lat.is_finite() || !zoom.is_finite() {
        issues.push(ValidationIssue::InvalidCenter(
            "values must be finite numbers".to_string(),
        ));
        return;
    }
    if lon.abs() > LON_LIMIT {
        issues.push(ValidationIssue::InvalidCenter(format!(
            "longitude {lon} out of [-180, 180]"
        )));
    }
    if lat.abs() > LAT_LIMIT {
        issues.push(ValidationIssue::InvalidCenter(format!(
            "latitude {lat} out of [-90, 90]"
        )));
    }
    if zoom < 0.0 || zoom > MAX_ZOOM as f64 {
        issues.push(ValidationIssue::InvalidCenter(format!(
            "zoom {zoom} is outside the supported range 0..=30"
        )));
    }

    // Only cross-check against `bounds` when the bounds themselves are
    // well-ordered, otherwise the comparison would be meaningless.
    if let Some([min_lon, min_lat, max_lon, max_lat]) = bounds_of(metadata) {
        let bounds_ordered = min_lon < max_lon && min_lat < max_lat;
        if bounds_ordered && (lon < min_lon || lon > max_lon || lat < min_lat || lat > max_lat) {
            issues.push(ValidationIssue::InvalidCenter(format!(
                "center ({lon}, {lat}) lies outside bounds [{min_lon}, {min_lat}, {max_lon}, {max_lat}]"
            )));
        }
    }
}

// ─── Field accessors ─────────────────────────────────────────────────────────────

/// Read the parsed `bounds` array, if present.
///
/// Centralised so the rule checks never touch struct internals directly, which
/// keeps them stable against additional fields on [`MBTilesMetadata`].
fn bounds_of(metadata: &MBTilesMetadata) -> Option<[f64; 4]> {
    metadata.bounds
}

/// Read the parsed `center` array, if present.
fn center_of(metadata: &MBTilesMetadata) -> Option<[f64; 3]> {
    metadata.center
}
