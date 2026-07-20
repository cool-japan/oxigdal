//! Integration tests for the NetCDF / CF-1.10 metadata extractor.
//!
//! These tests exercise the trait-shim path
//! ([`HasAttributes`]/[`HasVariables`]) with an in-memory
//! `FakeDataset`, so they do not depend on a real NetCDF file on disk.
//! Gated behind the `netcdf` feature because the public extractor
//! module is itself gated.

#![cfg(feature = "netcdf")]

use chrono::{TimeZone, Utc};
use oxigeo_metadata::extractors::netcdf_cf::{
    HasAttributes, HasVariables, extract_bbox_from_lon_lat, extract_cf_globals,
    extract_grid_mapping_crs, extract_temporal_extent,
};
use std::collections::HashMap;

/// In-memory NetCDF stand-in implementing both shim traits.
#[derive(Default)]
struct FakeDataset {
    attrs: HashMap<String, String>,
    var_names: Vec<String>,
    var_attrs: HashMap<(String, String), String>,
    var_minmax: HashMap<String, (f64, f64)>,
}

impl FakeDataset {
    fn with_global(mut self, name: &str, value: &str) -> Self {
        self.attrs.insert(name.to_string(), value.to_string());
        self
    }

    fn with_variable(mut self, name: &str) -> Self {
        self.var_names.push(name.to_string());
        self
    }

    fn with_var_attr(mut self, var: &str, attr: &str, value: &str) -> Self {
        self.var_attrs
            .insert((var.to_string(), attr.to_string()), value.to_string());
        // Auto-register the variable if not yet present so callers
        // don't have to thread `with_variable` for each attribute.
        if !self.var_names.iter().any(|n| n == var) {
            self.var_names.push(var.to_string());
        }
        self
    }

    fn with_minmax(mut self, var: &str, min: f64, max: f64) -> Self {
        self.var_minmax.insert(var.to_string(), (min, max));
        if !self.var_names.iter().any(|n| n == var) {
            self.var_names.push(var.to_string());
        }
        self
    }
}

impl HasAttributes for FakeDataset {
    fn get_attribute_string(&self, name: &str) -> Option<String> {
        self.attrs.get(name).cloned()
    }
    fn attribute_names(&self) -> Vec<String> {
        self.attrs.keys().cloned().collect()
    }
}

impl HasVariables for FakeDataset {
    fn variable_names(&self) -> Vec<String> {
        self.var_names.clone()
    }
    fn variable_attribute_string(&self, var: &str, attr: &str) -> Option<String> {
        self.var_attrs
            .get(&(var.to_string(), attr.to_string()))
            .cloned()
    }
    fn variable_min_max(&self, var: &str) -> Option<(f64, f64)> {
        self.var_minmax.get(var).copied()
    }
}

#[test]
fn test_extract_cf_globals_canonical_keys() {
    let ds = FakeDataset::default()
        .with_global("title", "Sentinel-2 Imagery")
        .with_global("summary", "Optical satellite imagery")
        .with_global("keywords", "satellite,sentinel-2,earth observation")
        .with_global("institution", "ESA")
        .with_global("Conventions", "CF-1.10");

    let globals = extract_cf_globals(&ds);
    assert_eq!(globals.title.as_deref(), Some("Sentinel-2 Imagery"));
    assert_eq!(
        globals.summary.as_deref(),
        Some("Optical satellite imagery")
    );
    assert_eq!(
        globals.keywords.as_deref(),
        Some("satellite,sentinel-2,earth observation")
    );
    assert_eq!(globals.institution.as_deref(), Some("ESA"));
    assert_eq!(globals.conventions.as_deref(), Some("CF-1.10"));
}

#[test]
fn test_extract_cf_globals_missing_optional_keys() {
    let ds = FakeDataset::default().with_global("title", "Only Title");
    let globals = extract_cf_globals(&ds);
    assert_eq!(globals.title.as_deref(), Some("Only Title"));
    assert!(globals.summary.is_none());
    assert!(globals.keywords.is_none());
    assert!(globals.institution.is_none());
    assert!(globals.conventions.is_none());
    assert!(globals.history.is_none());
    assert!(globals.source.is_none());
    assert!(globals.references.is_none());
    assert!(globals.comment.is_none());
}

#[test]
fn test_extract_bbox_from_lon_lat_standard_name() {
    let ds = FakeDataset::default()
        .with_var_attr("lon", "standard_name", "longitude")
        .with_var_attr("lat", "standard_name", "latitude")
        .with_minmax("lon", -180.0, 180.0)
        .with_minmax("lat", -90.0, 90.0);

    let bbox = extract_bbox_from_lon_lat(&ds).expect("bbox must resolve via standard_name");
    assert_eq!(bbox.west, -180.0);
    assert_eq!(bbox.east, 180.0);
    assert_eq!(bbox.south, -90.0);
    assert_eq!(bbox.north, 90.0);
}

#[test]
fn test_extract_bbox_from_axis_x_y_fallback() {
    let ds = FakeDataset::default()
        .with_var_attr("x_coord", "axis", "X")
        .with_var_attr("y_coord", "axis", "Y")
        .with_minmax("x_coord", 100.0, 200.0)
        .with_minmax("y_coord", 30.0, 50.0);

    let bbox = extract_bbox_from_lon_lat(&ds).expect("bbox must resolve via axis attributes");
    assert_eq!(bbox.west, 100.0);
    assert_eq!(bbox.east, 200.0);
    assert_eq!(bbox.south, 30.0);
    assert_eq!(bbox.north, 50.0);
}

#[test]
fn test_extract_temporal_extent_days_since() {
    let ds = FakeDataset::default()
        .with_var_attr("time", "standard_name", "time")
        .with_var_attr("time", "units", "days since 2000-01-01")
        .with_minmax("time", 0.0, 365.0);

    let extent = extract_temporal_extent(&ds).expect("temporal extent must be computed");
    let start = extent.start.expect("start datetime must be populated");
    let end = extent.end.expect("end datetime must be populated");

    let expected_start = Utc
        .with_ymd_and_hms(2000, 1, 1, 0, 0, 0)
        .single()
        .expect("expected start must be constructible");
    let expected_end = Utc
        .with_ymd_and_hms(2000, 12, 31, 0, 0, 0)
        .single()
        .expect("expected end must be constructible");

    assert_eq!(start, expected_start);
    assert_eq!(end, expected_end);
}

#[test]
fn test_extract_temporal_extent_hours_since() {
    let ds = FakeDataset::default()
        .with_var_attr("time", "standard_name", "time")
        .with_var_attr("time", "units", "hours since 2020-06-15")
        .with_minmax("time", 0.0, 48.0);

    let extent = extract_temporal_extent(&ds).expect("temporal extent must be computed");
    let start = extent.start.expect("start must be set");
    let end = extent.end.expect("end must be set");

    let expected_start = Utc
        .with_ymd_and_hms(2020, 6, 15, 0, 0, 0)
        .single()
        .expect("start construction");
    let expected_end = Utc
        .with_ymd_and_hms(2020, 6, 17, 0, 0, 0)
        .single()
        .expect("end construction");

    assert_eq!(start, expected_start);
    assert_eq!(end, expected_end);
}

#[test]
fn test_extract_temporal_extent_seconds_since() {
    let ds = FakeDataset::default()
        .with_var_attr("time", "standard_name", "time")
        .with_var_attr("time", "units", "seconds since 1970-01-01")
        .with_minmax("time", 0.0, 86_400.0);

    let extent = extract_temporal_extent(&ds).expect("temporal extent must be computed");
    let start = extent.start.expect("start must be set");
    let end = extent.end.expect("end must be set");

    let expected_start = Utc
        .with_ymd_and_hms(1970, 1, 1, 0, 0, 0)
        .single()
        .expect("epoch construction");
    let expected_end = Utc
        .with_ymd_and_hms(1970, 1, 2, 0, 0, 0)
        .single()
        .expect("epoch+1d construction");

    assert_eq!(start, expected_start);
    assert_eq!(end, expected_end);
}

#[test]
fn test_extract_grid_mapping_latitude_longitude() {
    let ds = FakeDataset::default()
        .with_var_attr("tas", "grid_mapping", "crs")
        .with_var_attr("crs", "grid_mapping_name", "latitude_longitude");

    let crs = extract_grid_mapping_crs(&ds).expect("CRS must be derived");
    assert_eq!(crs, "EPSG:4326");
}

#[test]
fn test_extract_grid_mapping_transverse_mercator() {
    let ds = FakeDataset::default()
        .with_var_attr("temperature", "grid_mapping", "projection")
        .with_var_attr("projection", "grid_mapping_name", "transverse_mercator");

    let crs = extract_grid_mapping_crs(&ds).expect("CRS must be derived");
    assert!(
        crs.contains("Transverse_Mercator"),
        "expected WKT containing Transverse_Mercator, got: {}",
        crs
    );
    assert!(
        crs.contains("WGS 84"),
        "expected WKT to reference WGS 84 datum, got: {}",
        crs
    );
}

#[test]
fn test_netcdf_extractor_without_grid_mapping_no_crs() {
    let ds = FakeDataset::default()
        .with_variable("data")
        .with_var_attr("data", "units", "K");
    assert!(extract_grid_mapping_crs(&ds).is_none());
}
