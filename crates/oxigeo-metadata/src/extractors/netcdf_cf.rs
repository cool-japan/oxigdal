//! NetCDF / CF-1.10 metadata extractor.
//!
//! Implements pluggable, trait-based extraction of [CF Conventions]
//! (Climate and Forecast) metadata from NetCDF datasets. The
//! [`HasAttributes`] and [`HasVariables`] traits decouple parsing logic
//! from the underlying NetCDF reader implementation, enabling unit tests
//! to use lightweight in-memory fakes.
//!
//! # CF Globals
//!
//! The CF specification reserves a small set of canonical global
//! attributes — `title`, `summary`, `keywords`, `institution`,
//! `Conventions`, `history`, `source`, `references`, and `comment` —
//! used to describe the dataset overall. These are extracted into a
//! [`CfGlobals`] struct via [`extract_cf_globals`].
//!
//! # Coordinate Variables
//!
//! CF declares longitude/latitude coordinate variables either by
//! `standard_name="longitude"` / `"latitude"` or by `axis="X"` /
//! `axis="Y"`. [`extract_bbox_from_lon_lat`] tries both. The temporal
//! axis is found via `standard_name="time"` or a variable literally
//! named `time` (case-insensitive). Time units are parsed from
//! CF unit strings of the form `"<unit> since <reference-date>"`
//! using [`parse_cf_time_units`].
//!
//! # Grid Mapping
//!
//! A CF grid_mapping variable describes the dataset's CRS via the
//! `grid_mapping_name` attribute. [`extract_grid_mapping_crs`] returns
//! an EPSG/WKT/sentinel string for several common projections; unknown
//! mappings produce `GRID_MAPPING:<name>` so the metadata is not lost.
//!
//! # Real-Dataset Wrapper
//!
//! When the `netcdf` feature is enabled, a thin
//! `real_dataset::NetCdfReaderShim` wraps `oxigeo_netcdf::NetCdfReader`
//! and implements the trait shims. It uses only the global-attribute and
//! variable-attribute accessors of the reader; the variable min/max
//! query falls back to reading the underlying float arrays.
//!
//! [CF Conventions]: https://cfconventions.org/

use crate::common::{BoundingBox, TemporalExtent};
use crate::error::{MetadataError, Result};
use crate::extract::ExtractedMetadata;
use std::path::Path;

/// Trait shim for global-attribute access on a NetCDF-like dataset.
///
/// Allows the CF extractor to operate on any backend (real reader,
/// in-memory fake) so long as it can return attribute values as
/// `String` and enumerate attribute names.
pub trait HasAttributes {
    /// Look up a global attribute by name and return its value
    /// rendered as a `String`. Numeric attributes should be formatted
    /// with [`std::fmt::Display`] semantics.
    fn get_attribute_string(&self, name: &str) -> Option<String>;

    /// Return the list of all global attribute names.
    fn attribute_names(&self) -> Vec<String>;
}

/// Trait shim for variable access on a NetCDF-like dataset.
///
/// Provides the minimal surface required by the CF extractor:
/// variable enumeration, variable-attribute lookup, and a per-variable
/// numeric min/max for coordinate-range extraction.
pub trait HasVariables {
    /// Return the list of variable names in the dataset.
    fn variable_names(&self) -> Vec<String>;

    /// Look up an attribute on a specific variable.
    fn variable_attribute_string(&self, var: &str, attr: &str) -> Option<String>;

    /// Return the (min, max) pair of a numeric variable's values, if
    /// the variable exists and contains at least one finite value.
    fn variable_min_max(&self, var: &str) -> Option<(f64, f64)>;
}

/// CF-1.10 canonical global attributes.
///
/// Each field maps directly to a CF-reserved global attribute name.
/// Missing attributes are represented as `None`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CfGlobals {
    /// `title` — a short, human-readable description of the dataset.
    pub title: Option<String>,
    /// `summary` — a paragraph-length abstract of the dataset.
    pub summary: Option<String>,
    /// `keywords` — comma-separated keywords describing the dataset.
    pub keywords: Option<String>,
    /// `institution` — the producing institution.
    pub institution: Option<String>,
    /// `Conventions` — the CF convention version (e.g. `"CF-1.10"`).
    pub conventions: Option<String>,
    /// `history` — provenance / processing-history audit trail.
    pub history: Option<String>,
    /// `source` — original data source description.
    pub source: Option<String>,
    /// `references` — bibliographic references.
    pub references: Option<String>,
    /// `comment` — free-form comment.
    pub comment: Option<String>,
}

/// Extract the canonical CF global attributes from a dataset.
///
/// Returns a [`CfGlobals`] with every CF-reserved global attribute that
/// the underlying dataset exposes; absent attributes become `None`.
pub fn extract_cf_globals<D: HasAttributes + ?Sized>(ds: &D) -> CfGlobals {
    CfGlobals {
        title: ds.get_attribute_string("title"),
        summary: ds.get_attribute_string("summary"),
        keywords: ds.get_attribute_string("keywords"),
        institution: ds.get_attribute_string("institution"),
        conventions: ds.get_attribute_string("Conventions"),
        history: ds.get_attribute_string("history"),
        source: ds.get_attribute_string("source"),
        references: ds.get_attribute_string("references"),
        comment: ds.get_attribute_string("comment"),
    }
}

/// Extract a bounding box from longitude/latitude coordinate variables.
///
/// The function scans the dataset's variables for those identified as
/// longitude or latitude — first by `standard_name`, then by `axis`. If
/// both axes are found, their numeric min/max define the bounding box.
///
/// Returns `None` when either axis is absent or has no numeric range.
pub fn extract_bbox_from_lon_lat<D: HasVariables + ?Sized>(ds: &D) -> Option<BoundingBox> {
    let mut lon_var: Option<String> = None;
    let mut lat_var: Option<String> = None;

    // First pass: prefer standard_name (CF preferred identifier).
    for name in ds.variable_names() {
        if let Some(sn) = ds.variable_attribute_string(&name, "standard_name") {
            match sn.as_str() {
                "longitude" if lon_var.is_none() => lon_var = Some(name.clone()),
                "latitude" if lat_var.is_none() => lat_var = Some(name.clone()),
                _ => {}
            }
        }
    }

    // Second pass: fall back to axis="X" / axis="Y" if not yet resolved.
    if lon_var.is_none() || lat_var.is_none() {
        for name in ds.variable_names() {
            if let Some(ax) = ds.variable_attribute_string(&name, "axis") {
                match ax.as_str() {
                    "X" if lon_var.is_none() => lon_var = Some(name.clone()),
                    "Y" if lat_var.is_none() => lat_var = Some(name.clone()),
                    _ => {}
                }
            }
        }
    }

    let lv = lon_var?;
    let la = lat_var?;
    let (xmin, xmax) = ds.variable_min_max(&lv)?;
    let (ymin, ymax) = ds.variable_min_max(&la)?;

    // BoundingBox::new(west, east, south, north).
    Some(BoundingBox::new(xmin, xmax, ymin, ymax))
}

/// Extract a temporal extent from the dataset's time axis.
///
/// Finds the time variable by `standard_name="time"` or by a literal
/// name matching `"time"` (case-insensitive), parses its `units`
/// attribute (e.g. `"days since 2000-01-01"`), then converts the
/// variable's numeric min/max into UTC datetimes.
pub fn extract_temporal_extent<D: HasVariables + ?Sized>(ds: &D) -> Option<TemporalExtent> {
    let names = ds.variable_names();
    let time_var = names.iter().find(|n| {
        ds.variable_attribute_string(n, "standard_name").as_deref() == Some("time")
            || n.eq_ignore_ascii_case("time")
    })?;

    let units = ds.variable_attribute_string(time_var, "units")?;
    let (min, max) = ds.variable_min_max(time_var)?;
    let (multiplier_secs, ref_date) = parse_cf_time_units(&units)?;

    use chrono::{Duration, TimeZone, Utc};
    let start_naive = ref_date + Duration::seconds((min * multiplier_secs as f64) as i64);
    let end_naive = ref_date + Duration::seconds((max * multiplier_secs as f64) as i64);
    let start = Utc.from_utc_datetime(&start_naive);
    let end = Utc.from_utc_datetime(&end_naive);

    Some(TemporalExtent {
        start: Some(start),
        end: Some(end),
    })
}

/// Parse a CF time-units string of the form `"<unit> since <reference>"`.
///
/// Returns the per-unit conversion factor in seconds and the parsed
/// reference datetime. Accepted units (case-insensitive):
///
/// - `days`, `day`
/// - `hours`, `hour`, `hr`, `hrs`
/// - `minutes`, `minute`, `min`, `mins`
/// - `seconds`, `second`, `sec`, `secs`, `s`
///
/// Reference timestamps are tried against several formats:
/// `%Y-%m-%d %H:%M:%S`, `%Y-%m-%d`, and `%Y-%m-%dT%H:%M:%S`. Date-only
/// references are padded with midnight (`00:00:00`).
pub fn parse_cf_time_units(units: &str) -> Option<(i64, chrono::NaiveDateTime)> {
    use chrono::{NaiveDate, NaiveDateTime};

    let lower = units.to_lowercase();
    let parts: Vec<&str> = lower.splitn(2, " since ").collect();
    if parts.len() != 2 {
        return None;
    }
    let unit = parts[0].trim();
    let multiplier = match unit {
        "days" | "day" => 86_400_i64,
        "hours" | "hour" | "hr" | "hrs" => 3_600_i64,
        "minutes" | "minute" | "min" | "mins" => 60_i64,
        "seconds" | "second" | "sec" | "secs" | "s" => 1_i64,
        _ => return None,
    };

    let ref_str = parts[1].trim();

    // Try datetime formats first.
    for fmt in &["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(ref_str, fmt) {
            return Some((multiplier, dt));
        }
    }

    // Date-only: pad with midnight.
    if let Ok(d) = NaiveDate::parse_from_str(ref_str, "%Y-%m-%d") {
        let dt = d.and_hms_opt(0, 0, 0)?;
        return Some((multiplier, dt));
    }

    None
}

/// Extract a CRS string from the CF `grid_mapping` variable.
///
/// Searches all variables for a `grid_mapping` attribute, follows it to
/// the named grid-mapping variable, and reads its `grid_mapping_name`.
/// Known mappings are returned as EPSG codes or assembled WKT strings;
/// unknown mappings are returned as `GRID_MAPPING:<name>` so the
/// metadata is preserved.
pub fn extract_grid_mapping_crs<D: HasVariables + ?Sized>(ds: &D) -> Option<String> {
    for name in ds.variable_names() {
        if let Some(gm) = ds.variable_attribute_string(&name, "grid_mapping")
            && let Some(gmn) = ds.variable_attribute_string(&gm, "grid_mapping_name")
        {
            return Some(match gmn.as_str() {
                "latitude_longitude" => "EPSG:4326".to_string(),
                "transverse_mercator" => TRANSVERSE_MERCATOR_WKT.to_string(),
                "mercator" => "EPSG:3395".to_string(),
                "polar_stereographic" => "EPSG:3413".to_string(),
                "lambert_conformal_conic" => "EPSG:9802".to_string(),
                "albers_conical_equal_area" => "EPSG:9822".to_string(),
                "rotated_latitude_longitude" => "EPSG:4326".to_string(),
                "stereographic" => "EPSG:3995".to_string(),
                other => format!("GRID_MAPPING:{}", other),
            });
        }
    }
    None
}

/// Canonical WKT for the CF `transverse_mercator` grid mapping.
///
/// The CF specification defines `transverse_mercator` parametrically;
/// without specific projection parameters we emit a generic WGS84-based
/// Transverse Mercator WKT envelope so downstream tools recognise the
/// projection family.
const TRANSVERSE_MERCATOR_WKT: &str = concat!(
    "PROJCS[\"Transverse_Mercator\",",
    "GEOGCS[\"WGS 84\",",
    "DATUM[\"WGS_1984\",",
    "SPHEROID[\"WGS 84\",6378137,298.257223563]],",
    "PRIMEM[\"Greenwich\",0],",
    "UNIT[\"degree\",0.0174532925199433]],",
    "PROJECTION[\"Transverse_Mercator\"]]"
);

/// High-level CF metadata extractor over a NetCDF file path.
///
/// When the `netcdf` feature is enabled, calls into the real
/// `oxigeo_netcdf::NetCdfReader` and assembles a complete
/// [`ExtractedMetadata`]. Without the feature, [`NetCdfCfExtractor::extract`]
/// returns [`MetadataError::Unsupported`] rather than a metadata-poor
/// success value, so that callers cannot mistake "the `netcdf` feature is
/// disabled" for "this file genuinely has no CF metadata".
pub struct NetCdfCfExtractor;

impl NetCdfCfExtractor {
    /// Extract metadata from a NetCDF file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::ExtractionError`] when the file cannot
    /// be opened or the NetCDF reader rejects it. Returns
    /// [`MetadataError::Unsupported`] when this crate was built without the
    /// `netcdf` feature (in which case no NetCDF file, valid or not, can be
    /// read at all -- this is never returned as a false-successful,
    /// near-empty [`ExtractedMetadata`]).
    pub fn extract<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();

        #[cfg(feature = "netcdf")]
        {
            match real_dataset::NetCdfReaderShim::open(path_ref) {
                Ok(shim) => Ok(build_extracted_metadata(&shim, &path_str)),
                Err(e) => Err(MetadataError::ExtractionError(format!(
                    "Cannot open NetCDF '{}': {}",
                    path_str, e
                ))),
            }
        }

        #[cfg(not(feature = "netcdf"))]
        {
            Err(MetadataError::Unsupported(format!(
                "NetCDF extraction not available for '{}': enable the 'netcdf' feature \
                 (oxigeo-metadata was built without it, so no CF metadata can be read -- \
                 this is not the same as the file having no metadata)",
                path_str
            )))
        }
    }
}

/// Assemble an [`ExtractedMetadata`] from anything that implements
/// both shim traits, plus a file path string for the attributes map.
///
/// Public to the crate so the real-dataset wrapper and tests can both
/// exercise the end-to-end shape of the result. Only reachable when the
/// `netcdf` feature enables its sole production caller
/// ([`NetCdfCfExtractor::extract`]'s real-reader branch) or under `cfg(test)`
/// (which exercises it directly against an in-memory fake); cfg-gated the
/// same way so a `netcdf`-less, non-test build does not carry genuinely dead
/// code.
#[cfg(any(feature = "netcdf", test))]
pub(crate) fn build_extracted_metadata<D: HasAttributes + HasVariables + ?Sized>(
    ds: &D,
    path_str: &str,
) -> ExtractedMetadata {
    let globals = extract_cf_globals(ds);
    let bbox = extract_bbox_from_lon_lat(ds);
    let temporal_extent = extract_temporal_extent(ds);
    let crs = extract_grid_mapping_crs(ds);

    let keywords = globals
        .keywords
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut attributes = std::collections::HashMap::new();
    attributes.insert("file_path".to_string(), path_str.to_string());
    if let Some(v) = &globals.conventions {
        attributes.insert("Conventions".to_string(), v.clone());
    }
    if let Some(v) = &globals.institution {
        attributes.insert("institution".to_string(), v.clone());
    }
    if let Some(v) = &globals.history {
        attributes.insert("history".to_string(), v.clone());
    }
    if let Some(v) = &globals.source {
        attributes.insert("source".to_string(), v.clone());
    }
    if let Some(v) = &globals.references {
        attributes.insert("references".to_string(), v.clone());
    }
    if let Some(v) = &globals.comment {
        attributes.insert("comment".to_string(), v.clone());
    }

    ExtractedMetadata {
        title: globals.title,
        abstract_text: globals.summary,
        bbox,
        temporal_extent,
        crs,
        spatial_resolution: None,
        format: Some("NetCDF".to_string()),
        keywords,
        attributes,
    }
}

/// Wrapper around `oxigeo_netcdf::NetCdfReader` implementing the shim
/// traits. Independently feature-gated so disabling `netcdf` removes
/// the wrapper without touching the rest of the module.
#[cfg(feature = "netcdf")]
pub(crate) mod real_dataset {
    use super::{HasAttributes, HasVariables};
    use oxigeo_netcdf::{AttributeValue, NetCdfReader};
    use std::path::Path;

    /// Trait-shim adapter for `oxigeo_netcdf::NetCdfReader`.
    pub struct NetCdfReaderShim {
        reader: NetCdfReader,
    }

    impl NetCdfReaderShim {
        /// Open a NetCDF file via the underlying reader.
        pub fn open(path: &Path) -> Result<Self, String> {
            NetCdfReader::open(path)
                .map(|reader| Self { reader })
                .map_err(|e| e.to_string())
        }
    }

    /// Render an [`AttributeValue`] as a human-readable string.
    ///
    /// Text values are returned as-is. Numeric values are stored as
    /// `Vec<T>` in `oxigeo_netcdf`; scalars are length-1 vectors and
    /// are rendered without separators, while genuine arrays are
    /// comma-joined.
    fn attribute_value_to_string(value: &AttributeValue) -> String {
        match value {
            AttributeValue::Text(s) => s.clone(),
            AttributeValue::I8(v) => render_array_i64(v.iter().map(|x| i64::from(*x))),
            AttributeValue::U8(v) => render_array_u64(v.iter().map(|x| u64::from(*x))),
            AttributeValue::I16(v) => render_array_i64(v.iter().map(|x| i64::from(*x))),
            AttributeValue::U16(v) => render_array_u64(v.iter().map(|x| u64::from(*x))),
            AttributeValue::I32(v) => render_array_i64(v.iter().map(|x| i64::from(*x))),
            AttributeValue::U32(v) => render_array_u64(v.iter().map(|x| u64::from(*x))),
            AttributeValue::I64(v) => render_array_i64(v.iter().copied()),
            AttributeValue::U64(v) => render_array_u64(v.iter().copied()),
            AttributeValue::F32(v) => render_array_f64(v.iter().map(|x| f64::from(*x))),
            AttributeValue::F64(v) => render_array_f64(v.iter().copied()),
        }
    }

    fn render_array_i64<I: IntoIterator<Item = i64>>(iter: I) -> String {
        iter.into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn render_array_u64<I: IntoIterator<Item = u64>>(iter: I) -> String {
        iter.into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn render_array_f64<I: IntoIterator<Item = f64>>(iter: I) -> String {
        iter.into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    impl HasAttributes for NetCdfReaderShim {
        fn get_attribute_string(&self, name: &str) -> Option<String> {
            self.reader
                .global_attributes()
                .get_value(name)
                .map(attribute_value_to_string)
        }

        fn attribute_names(&self) -> Vec<String> {
            self.reader
                .global_attributes()
                .iter()
                .map(|a| a.name().to_string())
                .collect()
        }
    }

    impl HasVariables for NetCdfReaderShim {
        fn variable_names(&self) -> Vec<String> {
            self.reader
                .variables()
                .iter()
                .map(|v| v.name().to_string())
                .collect()
        }

        fn variable_attribute_string(&self, var: &str, attr: &str) -> Option<String> {
            self.reader
                .variables()
                .get(var)
                .and_then(|v| v.attributes().get_value(attr))
                .map(attribute_value_to_string)
        }

        fn variable_min_max(&self, var: &str) -> Option<(f64, f64)> {
            let variable = self.reader.variables().get(var)?;
            match variable.data_type() {
                oxigeo_netcdf::DataType::F32 => {
                    let data = self.reader.read_f32(var).ok()?;
                    min_max_f64(data.iter().map(|v| f64::from(*v)))
                }
                oxigeo_netcdf::DataType::F64 => {
                    let data = self.reader.read_f64(var).ok()?;
                    min_max_f64(data.iter().copied())
                }
                oxigeo_netcdf::DataType::I32 => {
                    let data = self.reader.read_i32(var).ok()?;
                    min_max_f64(data.iter().map(|v| f64::from(*v)))
                }
                _ => None,
            }
        }
    }

    fn min_max_f64<I: IntoIterator<Item = f64>>(iter: I) -> Option<(f64, f64)> {
        let mut iter = iter.into_iter().filter(|v| v.is_finite());
        let first = iter.next()?;
        let (min, max) = iter.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v)));
        Some((min, max))
    }

    // Accessor for the inner reader, used by integration code that needs
    // direct access (kept pub(crate) to avoid leaking the dependency).
    impl NetCdfReaderShim {
        #[allow(dead_code)]
        pub(crate) fn reader(&self) -> &NetCdfReader {
            &self.reader
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-module helper to construct a CfGlobals quickly for non-test
    /// assertions; kept here so the doc-tests don't depend on test
    /// modules.
    fn make_minimal_ds() -> InModuleFake {
        InModuleFake::default()
    }

    #[derive(Default)]
    struct InModuleFake {
        attrs: HashMap<String, String>,
    }

    impl HasAttributes for InModuleFake {
        fn get_attribute_string(&self, name: &str) -> Option<String> {
            self.attrs.get(name).cloned()
        }
        fn attribute_names(&self) -> Vec<String> {
            self.attrs.keys().cloned().collect()
        }
    }

    // No variables/coordinate axes: exercises `build_extracted_metadata`'s
    // bbox/temporal_extent/crs = None paths without requiring the `netcdf`
    // feature or a real reader.
    impl HasVariables for InModuleFake {
        fn variable_names(&self) -> Vec<String> {
            Vec::new()
        }
        fn variable_attribute_string(&self, _var: &str, _attr: &str) -> Option<String> {
            None
        }
        fn variable_min_max(&self, _var: &str) -> Option<(f64, f64)> {
            None
        }
    }

    #[test]
    fn parse_cf_time_units_days_since() {
        let parsed = parse_cf_time_units("days since 2000-01-01");
        assert!(parsed.is_some());
        let (multiplier, _) = parsed.expect("days unit must parse");
        assert_eq!(multiplier, 86_400);
    }

    #[test]
    fn parse_cf_time_units_unknown_unit_returns_none() {
        assert!(parse_cf_time_units("years since 2000-01-01").is_none());
    }

    #[test]
    fn extract_cf_globals_returns_defaults_when_empty() {
        let ds = make_minimal_ds();
        let g = extract_cf_globals(&ds);
        assert!(g.title.is_none());
        assert!(g.conventions.is_none());
    }

    /// `build_extracted_metadata` is the shared assembly path used by the real
    /// `netcdf`-feature reader; it is pure logic over the trait shims and does
    /// not itself require the `netcdf` feature, so it is exercised directly
    /// here against an in-memory fake.
    #[test]
    fn build_extracted_metadata_assembles_globals_and_file_path() {
        let mut ds = InModuleFake::default();
        ds.attrs
            .insert("title".to_string(), "Test Dataset".to_string());
        ds.attrs
            .insert("keywords".to_string(), "ocean, temperature".to_string());
        ds.attrs
            .insert("institution".to_string(), "OxiGeo Test Lab".to_string());

        let metadata = build_extracted_metadata(&ds, "/data/sample.nc");

        assert_eq!(metadata.format.as_deref(), Some("NetCDF"));
        assert_eq!(metadata.title.as_deref(), Some("Test Dataset"));
        assert_eq!(
            metadata.attributes.get("file_path").map(String::as_str),
            Some("/data/sample.nc")
        );
        assert_eq!(
            metadata.attributes.get("institution").map(String::as_str),
            Some("OxiGeo Test Lab")
        );
        assert_eq!(
            metadata.keywords,
            vec!["ocean".to_string(), "temperature".to_string()]
        );
        // No coordinate variables in the fake -> no bbox/temporal extent/crs.
        assert!(metadata.bbox.is_none());
        assert!(metadata.temporal_extent.is_none());
        assert!(metadata.crs.is_none());
    }

    /// Without the `netcdf` feature, `NetCdfCfExtractor::extract` must return a
    /// typed error rather than a near-empty "successful" [`ExtractedMetadata`]
    /// that is indistinguishable from a genuinely metadata-poor file -- for
    /// *any* input path, valid NetCDF or not, since the file is never opened.
    #[cfg(not(feature = "netcdf"))]
    #[test]
    fn extract_without_netcdf_feature_returns_unsupported_error() {
        let result = NetCdfCfExtractor::extract("/nonexistent/does-not-matter.nc");
        match result {
            Err(MetadataError::Unsupported(msg)) => {
                assert!(
                    msg.contains("netcdf"),
                    "message should name the feature: {msg}"
                );
            }
            Err(other) => panic!("expected MetadataError::Unsupported, got: {other:?}"),
            Ok(_) => panic!(
                "extract() must not silently succeed with a hollow result when the \
                 'netcdf' feature is disabled"
            ),
        }
    }
}
