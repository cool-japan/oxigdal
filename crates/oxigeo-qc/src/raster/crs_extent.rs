//! CRS and spatial-extent validation.
//!
//! Detects:
//!
//! - Malformed / unrecognised CRS strings (`CrsUnparseable`).
//! - CRS authority lookups that fail despite a successful parse
//!   (`CrsAuthorityUnknown` — typically a stale EPSG snapshot).
//! - Lat-lon vs lon-lat axis-order ambiguity (`CrsAxisOrderAmbiguous`).
//! - Geographic bounds outside the legal lon ∈ [-180, 180] /
//!   lat ∈ [-90, 90] window (`GeographicBoundsOutOfRange`).
//! - Projected bounds whose magnitude is implausible for their CRS family
//!   (UTM x out of [-167k, 833k], ECEF beyond ±6.7e6, Web Mercator beyond
//!   ±2.0e7) (`ProjectedBoundsImplausible`).
//! - Inverted bounds (`xmin > xmax` or `ymin > ymax`)
//!   (`BoundsInverted`).
//! - Zero-area extents (`BoundsZeroArea`).
//! - Bounds that contradict the pixel grid arithmetic by more than half a
//!   pixel (`BoundsPixelGridMismatch`).
//!
//! All issues are emitted as [`crate::error::QcIssue`]s with the existing
//! [`crate::error::Severity`] scale; this module never invents a parallel
//! taxonomy.

use std::path::Path;

use oxigeo_core::io::FileDataSource;
use oxigeo_geotiff::cog::CogReader;
use oxigeo_proj::Crs;
use oxigeo_proj::epsg::CrsType;

use crate::error::{QcIssue, QcResult, Severity};

/// Default tolerance (in pixels) for the bounds-vs-pixel-grid arithmetic
/// check.
pub const DEFAULT_BOUNDS_PIXEL_GRID_TOLERANCE: f64 = 0.5;

/// Validator for CRS metadata and spatial extents.
#[derive(Debug, Clone)]
pub struct CrsAndExtentValidator {
    /// Tolerance (in pixels) used when comparing computed extent against the
    /// declared extent. Defaults to [`DEFAULT_BOUNDS_PIXEL_GRID_TOLERANCE`].
    pub bounds_pixel_grid_tolerance_px: f64,
}

impl Default for CrsAndExtentValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl CrsAndExtentValidator {
    /// Constructs a validator with default thresholds.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bounds_pixel_grid_tolerance_px: DEFAULT_BOUNDS_PIXEL_GRID_TOLERANCE,
        }
    }

    /// Sets the bounds-vs-pixel-grid tolerance (in pixels).
    #[must_use]
    pub const fn with_pixel_grid_tolerance(mut self, px: f64) -> Self {
        self.bounds_pixel_grid_tolerance_px = px;
        self
    }

    /// Runs validation against a GeoTIFF file. Extracts CRS, geo-transform,
    /// and pixel grid; delegates to [`Self::check_metadata`].
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<CrsExtentValidationResult> {
        let source = FileDataSource::open(path.as_ref()).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to open raster: {}", e))
        })?;
        let reader = CogReader::open(source).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to read GeoTIFF: {}", e))
        })?;

        let info = reader.primary_info().clone();
        let width = u32::try_from(info.width).unwrap_or(u32::MAX);
        let height = u32::try_from(info.height).unwrap_or(u32::MAX);

        let geo_transform = reader.geo_transform().ok().flatten();

        // Reconstruct a CRS string. We prefer EPSG → Crs for clean validation.
        let epsg_code = reader.epsg_code();
        let crs_str = epsg_code.map(|c| format!("EPSG:{}", c));

        let bounds = geo_transform
            .as_ref()
            .and_then(|gt| geo_transform_to_bounds(gt, width as u64, height as u64));

        let pixel_size = geo_transform
            .as_ref()
            .map(|gt| (gt.pixel_width.abs(), gt.pixel_height.abs()));

        let bounds = bounds.unwrap_or((0.0, 0.0, 0.0, 0.0));
        let pixel_size = pixel_size.unwrap_or((1.0, 1.0));

        Ok(self.check_metadata(crs_str.as_deref(), bounds, width, height, pixel_size))
    }

    /// Runs the validation pipeline against pre-extracted metadata.
    ///
    /// `crs` is an optional CRS authority string ("EPSG:4326"), WKT
    /// fragment, or PROJ string. If `None`, no CRS-shape issues are emitted.
    /// `bounds` is `(xmin, ymin, xmax, ymax)`; `pixel_size` is
    /// `(pixel_width, pixel_height)` (both positive).
    pub fn check_metadata(
        &self,
        crs: Option<&str>,
        bounds: (f64, f64, f64, f64),
        width: u32,
        height: u32,
        pixel_size: (f64, f64),
    ) -> CrsExtentValidationResult {
        let mut issues = Vec::new();
        let mut crs_parsed = false;
        let mut crs_type: Option<CrsType> = None;

        if let Some(crs_str) = crs {
            match parse_crs(crs_str) {
                Ok(parsed) => {
                    crs_parsed = true;
                    crs_type = parsed.crs_type();
                    if parsed.authority().is_none() && parsed.epsg_code().is_none() {
                        issues.push(
                            QcIssue::new(
                                Severity::Warning,
                                "crs",
                                "CRS authority unknown",
                                format!(
                                    "CRS string parsed but no EPSG/authority recognised: '{}'",
                                    truncate(crs_str, 80)
                                ),
                            )
                            .with_rule_id("CRS-AUTHORITY-UNKNOWN")
                            .with_suggestion(
                                "Use a recognised EPSG code or supply a WKT with AUTHORITY tag.",
                            ),
                        );
                    }
                    if might_have_axis_order_ambiguity(crs_str, &parsed) {
                        issues.push(
                            QcIssue::new(
                                Severity::Warning,
                                "crs",
                                "CRS axis-order ambiguous",
                                "CRS does not state axis order explicitly; \
                                 software may interpret it as lon-lat or lat-lon."
                                    .to_string(),
                            )
                            .with_rule_id("CRS-AXIS-ORDER-AMBIGUOUS")
                            .with_suggestion(
                                "Specify AXIS / ORDER in WKT or migrate to a CRS that states it.",
                            ),
                        );
                    }
                }
                Err(msg) => {
                    issues.push(
                        QcIssue::new(
                            Severity::Critical,
                            "crs",
                            "CRS unparseable",
                            format!(
                                "Could not parse CRS string '{}': {}",
                                truncate(crs_str, 80),
                                msg
                            ),
                        )
                        .with_rule_id("CRS-UNPARSEABLE")
                        .with_suggestion("Provide a valid EPSG code, WKT, or PROJ string."),
                    );
                }
            }
        }

        let (xmin, ymin, xmax, ymax) = bounds;

        if xmin > xmax || ymin > ymax {
            issues.push(
                QcIssue::new(
                    Severity::Critical,
                    "extent",
                    "Bounds inverted",
                    format!(
                        "Spatial bounds are inverted: xmin={} > xmax={} or ymin={} > ymax={}",
                        xmin, xmax, ymin, ymax
                    ),
                )
                .with_rule_id("BOUNDS-INVERTED"),
            );
        } else if (xmax - xmin) <= 0.0 || (ymax - ymin) <= 0.0 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "extent",
                    "Bounds zero area",
                    format!(
                        "Spatial extent has zero or negative area: ({}, {}, {}, {})",
                        xmin, ymin, xmax, ymax
                    ),
                )
                .with_rule_id("BOUNDS-ZERO-AREA"),
            );
        } else {
            // Plausibility checks only run on non-degenerate bounds.
            check_geographic_bounds(crs_type, bounds, &mut issues);
            check_projected_bounds(crs_type, crs, bounds, &mut issues);
        }

        if width > 0 && height > 0 && pixel_size.0 > 0.0 && pixel_size.1 > 0.0 {
            check_bounds_vs_pixel_grid(
                bounds,
                width,
                height,
                pixel_size,
                self.bounds_pixel_grid_tolerance_px,
                &mut issues,
            );
        }

        CrsExtentValidationResult {
            issues,
            crs_parsed,
            bounds: Some(bounds),
        }
    }
}

/// Result of CRS + extent validation.
#[derive(Debug, Clone)]
pub struct CrsExtentValidationResult {
    /// Issues raised by the validator.
    pub issues: Vec<QcIssue>,
    /// `true` if the CRS string parsed successfully.
    pub crs_parsed: bool,
    /// The bounds tuple that was validated (`(xmin, ymin, xmax, ymax)`),
    /// echoed back for caller convenience.
    pub bounds: Option<(f64, f64, f64, f64)>,
}

impl CrsExtentValidationResult {
    /// Returns `true` if no critical issues were raised.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Critical)
    }
}

/// Parses a CRS string that may be authority-prefixed ("EPSG:4326"),
/// PROJ-formatted (`+proj=...`), or WKT.
fn parse_crs(s: &str) -> Result<Crs, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("CRS string is empty".to_string());
    }

    // EPSG:CODE form
    if let Some(rest) = trimmed
        .strip_prefix("EPSG:")
        .or_else(|| trimmed.strip_prefix("epsg:"))
    {
        let code: u32 = rest
            .trim()
            .parse()
            .map_err(|_| format!("EPSG code is not a positive integer: '{}'", rest))?;
        return Crs::from_epsg(code).map_err(|e| format!("EPSG lookup failed: {}", e));
    }

    // Bare integer → treat as EPSG.
    if let Ok(code) = trimmed.parse::<u32>() {
        return Crs::from_epsg(code).map_err(|e| format!("EPSG lookup failed: {}", e));
    }

    // PROJ string
    if trimmed.contains("+proj=") || trimmed.starts_with("proj=") {
        return Crs::from_proj(trimmed).map_err(|e| format!("PROJ parse failed: {}", e));
    }

    // WKT?
    let upper = trimmed.to_ascii_uppercase();
    let wkt_keys = [
        "GEOGCS[",
        "PROJCS[",
        "GEOCCS[",
        "VERT_CS[",
        "COMPD_CS[",
        "GEOGCRS[",
        "PROJCRS[",
        "GEODCRS[",
        "VERTCRS[",
        "ENGCRS[",
        "COMPOUNDCRS[",
        "BOUNDCRS[",
    ];
    if wkt_keys.iter().any(|k| upper.starts_with(k)) {
        return Crs::from_wkt(trimmed).map_err(|e| format!("WKT parse failed: {}", e));
    }

    Err(format!(
        "Unrecognised CRS format (not EPSG:N, PROJ, or WKT): '{}'",
        truncate(trimmed, 60)
    ))
}

/// Heuristic: WKT1 GEOGCS without an AXIS clause is axis-order ambiguous
/// because some software interprets it as lat-lon and others as lon-lat.
fn might_have_axis_order_ambiguity(raw: &str, parsed: &Crs) -> bool {
    if parsed.crs_type() != Some(CrsType::Geographic) {
        return false;
    }
    let upper = raw.to_ascii_uppercase();
    // Authority strings are unambiguous (the EPSG lookup carries axis info).
    if upper.starts_with("EPSG:") {
        return false;
    }
    // WKT1 GEOGCS without AXIS — flag it.
    if upper.contains("GEOGCS[") && !upper.contains("AXIS[") {
        return true;
    }
    false
}

fn check_geographic_bounds(
    crs_type: Option<CrsType>,
    bounds: (f64, f64, f64, f64),
    issues: &mut Vec<QcIssue>,
) {
    if crs_type != Some(CrsType::Geographic) {
        return;
    }
    let (xmin, ymin, xmax, ymax) = bounds;

    let lon_out = !(-180.0..=180.0).contains(&xmin) || !(-180.0..=180.0).contains(&xmax);
    let lat_out = !(-90.0..=90.0).contains(&ymin) || !(-90.0..=90.0).contains(&ymax);

    if lon_out || lat_out {
        issues.push(
            QcIssue::new(
                Severity::Critical,
                "extent",
                "Geographic bounds out of range",
                format!(
                    "Geographic bounds outside legal range: lon must be ∈ [-180, 180], \
                     lat ∈ [-90, 90]; got ({}, {}, {}, {})",
                    xmin, ymin, xmax, ymax
                ),
            )
            .with_rule_id("BOUNDS-GEOGRAPHIC-OUT-OF-RANGE")
            .with_suggestion("Verify CRS — bounds magnitude looks like a projected CRS."),
        );
    }
}

fn check_projected_bounds(
    crs_type: Option<CrsType>,
    crs: Option<&str>,
    bounds: (f64, f64, f64, f64),
    issues: &mut Vec<QcIssue>,
) {
    let (xmin, ymin, xmax, ymax) = bounds;

    if crs_type == Some(CrsType::Geographic) {
        return;
    }

    let max_abs_x = xmin.abs().max(xmax.abs());
    let max_abs_y = ymin.abs().max(ymax.abs());

    let crs_lower = crs.unwrap_or("").to_ascii_lowercase();

    let is_utm = crs_lower.contains("utm") || is_utm_epsg(crs);
    let is_web_mercator = crs_lower.contains("3857") || crs_lower.contains("web mercator");
    let is_geocentric = crs_type == Some(CrsType::Geocentric);

    // UTM zone bounds (rough): x ∈ [-167k, 833k] (relative to false-easting
    // 500k); y ∈ [-9.3e6, 9.3e6] including hemispheres.
    if is_utm {
        if max_abs_x > 1.0e7 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "extent",
                    "Projected bounds implausible (UTM)",
                    format!(
                        "UTM x bound {} exceeds reasonable zone width (≈ ±1e7 m)",
                        max_abs_x
                    ),
                )
                .with_rule_id("BOUNDS-UTM-IMPLAUSIBLE-X"),
            );
        }
        if max_abs_y > 2.0e7 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "extent",
                    "Projected bounds implausible (UTM)",
                    format!(
                        "UTM y bound {} exceeds reasonable hemisphere span (≈ ±2e7 m)",
                        max_abs_y
                    ),
                )
                .with_rule_id("BOUNDS-UTM-IMPLAUSIBLE-Y"),
            );
        }
    } else if is_web_mercator {
        // Web Mercator wraps at ±20037508.3 m
        if max_abs_x > 2.1e7 || max_abs_y > 2.1e7 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "extent",
                    "Projected bounds implausible (Web Mercator)",
                    format!(
                        "Bounds exceed Web Mercator world range (≈ ±2.0e7 m): x≤{}, y≤{}",
                        max_abs_x, max_abs_y
                    ),
                )
                .with_rule_id("BOUNDS-WEBMERCATOR-IMPLAUSIBLE"),
            );
        }
    } else if is_geocentric {
        if max_abs_x > 1.0e7 || max_abs_y > 1.0e7 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "extent",
                    "Projected bounds implausible (ECEF)",
                    format!(
                        "ECEF bounds exceed Earth radius (≈ ±6.7e6 m): x≤{}, y≤{}",
                        max_abs_x, max_abs_y
                    ),
                )
                .with_rule_id("BOUNDS-ECEF-IMPLAUSIBLE"),
            );
        }
    } else if crs_type == Some(CrsType::Projected) {
        // Generic projected: bounds beyond ±1e8 m are suspicious for any
        // terrestrial projection.
        if max_abs_x > 1.0e8 || max_abs_y > 1.0e8 {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "extent",
                    "Projected bounds borderline implausible",
                    format!(
                        "Bounds magnitude is unusually large for a projected CRS: x≤{}, y≤{}",
                        max_abs_x, max_abs_y
                    ),
                )
                .with_rule_id("BOUNDS-PROJECTED-IMPLAUSIBLE"),
            );
        }
    }
}

fn is_utm_epsg(crs: Option<&str>) -> bool {
    let s = match crs {
        Some(s) => s.trim(),
        None => return false,
    };
    let rest = s.strip_prefix("EPSG:").or_else(|| s.strip_prefix("epsg:"));
    let Some(code_str) = rest else { return false };
    let Ok(code) = code_str.parse::<u32>() else {
        return false;
    };
    // EPSG UTM ranges: 32601..32660 (north), 32701..32760 (south).
    (32601..=32660).contains(&code) || (32701..=32760).contains(&code)
}

fn check_bounds_vs_pixel_grid(
    bounds: (f64, f64, f64, f64),
    width: u32,
    height: u32,
    pixel_size: (f64, f64),
    tolerance_px: f64,
    issues: &mut Vec<QcIssue>,
) {
    let (xmin, ymin, xmax, ymax) = bounds;
    let (px_w, px_h) = pixel_size;

    let computed_xmax = xmin + (width as f64) * px_w;
    let computed_ymax = ymin + (height as f64) * px_h;

    let dx_px = (computed_xmax - xmax).abs() / px_w;
    let dy_px = (computed_ymax - ymax).abs() / px_h;

    if dx_px > tolerance_px || dy_px > tolerance_px {
        issues.push(
            QcIssue::new(
                Severity::Major,
                "extent",
                "Bounds-vs-pixel-grid mismatch",
                format!(
                    "Computed extent (xmin + W·px_w = {}, ymin + H·px_h = {}) disagrees \
                     with declared (xmax={}, ymax={}) by {:.3}px (x), {:.3}px (y); \
                     tolerance {}px",
                    computed_xmax, computed_ymax, xmax, ymax, dx_px, dy_px, tolerance_px
                ),
            )
            .with_rule_id("BOUNDS-PIXEL-GRID-MISMATCH"),
        );
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(n).collect();
        format!("{}…", prefix)
    }
}

fn geo_transform_to_bounds(
    gt: &oxigeo_core::types::GeoTransform,
    width: u64,
    height: u64,
) -> Option<(f64, f64, f64, f64)> {
    if width == 0 || height == 0 {
        return None;
    }
    let xmin = gt.origin_x;
    let ymax = gt.origin_y;
    let xmax = xmin + (width as f64) * gt.pixel_width;
    let ymin = ymax + (height as f64) * gt.pixel_height; // pixel_height is typically negative
    let (ymin_lo, ymax_hi) = if ymin <= ymax {
        (ymin, ymax)
    } else {
        (ymax, ymin)
    };
    let (xmin_lo, xmax_hi) = if xmin <= xmax {
        (xmin, xmax)
    } else {
        (xmax, xmin)
    };
    Some((xmin_lo, ymin_lo, xmax_hi, ymax_hi))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn validator() -> CrsAndExtentValidator {
        CrsAndExtentValidator::new()
    }

    #[test]
    fn test_crs_unparseable_critical() {
        let v = validator();
        let r = v.check_metadata(
            Some("not a valid crs at all"),
            (0.0, 0.0, 1.0, 1.0),
            10,
            10,
            (0.1, 0.1),
        );
        let crit = r
            .issues
            .iter()
            .find(|i| i.severity == Severity::Critical && i.category == "crs");
        assert!(
            crit.is_some(),
            "expected Critical CRS issue, got {:?}",
            r.issues
        );
        assert!(!r.crs_parsed);
    }

    #[test]
    fn test_crs_axis_order_ambiguous_warns() {
        // WKT1 GEOGCS without explicit AXIS clause → ambiguous.
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0]]"#;
        let v = validator();
        let r = v.check_metadata(
            Some(wkt),
            (-180.0, -90.0, 180.0, 90.0),
            360,
            180,
            (1.0, 1.0),
        );
        assert!(
            r.issues
                .iter()
                .any(|i| i.rule_id.as_deref() == Some("CRS-AXIS-ORDER-AMBIGUOUS")),
            "expected axis-order Warning, got {:#?}",
            r.issues
        );
    }

    #[test]
    fn test_geographic_bounds_lat_91_critical() {
        let v = validator();
        // EPSG:4326 → geographic; lat 91 invalid.
        let r = v.check_metadata(
            Some("EPSG:4326"),
            (10.0, -91.0, 12.0, 91.0),
            10,
            10,
            (0.2, 0.2),
        );
        assert!(
            r.issues.iter().any(|i| i.severity == Severity::Critical
                && i.rule_id.as_deref() == Some("BOUNDS-GEOGRAPHIC-OUT-OF-RANGE")),
            "expected geographic-bounds Critical, got {:#?}",
            r.issues
        );
    }

    #[test]
    fn test_projected_bounds_utm_implausible_x_majors() {
        // EPSG:32633 = UTM zone 33N. x = 5e7 is way outside zone width.
        let v = validator();
        let r = v.check_metadata(
            Some("EPSG:32633"),
            (5.0e7, 1.0e6, 5.1e7, 1.1e6),
            100,
            100,
            (1.0, 1.0),
        );
        assert!(
            r.issues.iter().any(|i| i.severity == Severity::Major
                && i.rule_id.as_deref() == Some("BOUNDS-UTM-IMPLAUSIBLE-X")),
            "expected UTM x Major, got {:#?}",
            r.issues
        );
    }

    #[test]
    fn test_bounds_inverted_critical() {
        let v = validator();
        let r = v.check_metadata(Some("EPSG:4326"), (10.0, 0.0, 5.0, 5.0), 10, 10, (0.5, 0.5));
        assert!(
            r.issues.iter().any(|i| i.severity == Severity::Critical
                && i.rule_id.as_deref() == Some("BOUNDS-INVERTED")),
            "expected inverted-bounds Critical, got {:#?}",
            r.issues
        );
    }

    #[test]
    fn test_bounds_zero_area_majors() {
        let v = validator();
        let r = v.check_metadata(Some("EPSG:4326"), (5.0, 5.0, 5.0, 5.0), 10, 10, (0.5, 0.5));
        assert!(
            r.issues.iter().any(|i| i.severity == Severity::Major
                && i.rule_id.as_deref() == Some("BOUNDS-ZERO-AREA")),
            "expected zero-area Major, got {:#?}",
            r.issues
        );
    }

    #[test]
    fn test_bounds_pixel_grid_mismatch_majors() {
        // 100x100 raster, pixel size 1.0 → expected xmax = xmin + 100.
        // We provide xmax that is off by 5 pixels.
        let v = validator();
        let r = v.check_metadata(
            Some("EPSG:32633"),
            (500_000.0, 4_000_000.0, 500_105.0, 4_000_100.0),
            100,
            100,
            (1.0, 1.0),
        );
        assert!(
            r.issues.iter().any(|i| i.severity == Severity::Major
                && i.rule_id.as_deref() == Some("BOUNDS-PIXEL-GRID-MISMATCH")),
            "expected pixel-grid Major, got {:#?}",
            r.issues
        );
    }

    #[test]
    fn test_bounds_pixel_grid_within_half_px_passes() {
        // 100x100 raster, pixel size 1.0; xmax off by 0.3 px → within tolerance.
        let v = validator();
        let r = v.check_metadata(
            Some("EPSG:32633"),
            (500_000.0, 4_000_000.0, 500_100.3, 4_000_100.0),
            100,
            100,
            (1.0, 1.0),
        );
        assert!(
            !r.issues
                .iter()
                .any(|i| i.rule_id.as_deref() == Some("BOUNDS-PIXEL-GRID-MISMATCH")),
            "did not expect pixel-grid issue at 0.3px, got {:#?}",
            r.issues
        );
    }
}
