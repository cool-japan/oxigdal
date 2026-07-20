//! GeoParquet metadata structures
//!
//! This module implements the GeoParquet 1.0 metadata specification,
//! which defines how geospatial metadata is stored in Parquet file metadata.
//!
//! The GeoParquet metadata is stored in the file-level key-value metadata
//! under the "geo" key as a JSON object.

use crate::error::{GeoParquetError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// GeoParquet format version emitted by writers in this crate.
///
/// Bumped to `"1.1.0"` to enable `covering.bbox` columns, GeoArrow native
/// encodings, and predicate pushdown features added in OxiGeo 0.1.5.  Files
/// written by older OxiGeo versions declaring `"1.0.0"` continue to read
/// because [`GeoParquetMetadata::validate`] accepts any 1.x version string.
pub const GEOPARQUET_VERSION: &str = "1.1.0";

/// The minimum supported GeoParquet specification version.
///
/// Any 1.x file is accepted on read; only the major version is significant.
pub const GEOPARQUET_VERSION_MIN: &str = "1.0.0";

/// Metadata key in Parquet file metadata
pub const GEOPARQUET_METADATA_KEY: &str = "geo";

/// GeoParquet metadata structure (root object)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoParquetMetadata {
    /// GeoParquet specification version
    pub version: String,

    /// Primary geometry column name
    pub primary_column: String,

    /// Metadata for each geometry column
    pub columns: HashMap<String, GeometryColumnMetadata>,
}

impl GeoParquetMetadata {
    /// Creates new GeoParquet metadata
    pub fn new(primary_column: impl Into<String>) -> Self {
        Self {
            version: GEOPARQUET_VERSION.to_string(),
            primary_column: primary_column.into(),
            columns: HashMap::new(),
        }
    }

    /// Adds a geometry column
    pub fn add_column(
        &mut self,
        name: impl Into<String>,
        metadata: GeometryColumnMetadata,
    ) -> &mut Self {
        self.columns.insert(name.into(), metadata);
        self
    }

    /// Gets metadata for a geometry column
    pub fn get_column(&self, name: &str) -> Option<&GeometryColumnMetadata> {
        self.columns.get(name)
    }

    /// Gets metadata for the primary geometry column
    pub fn primary_column_metadata(&self) -> Result<&GeometryColumnMetadata> {
        self.columns
            .get(&self.primary_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.primary_column))
    }

    /// Validates the metadata.
    ///
    /// The version check accepts any GeoParquet `1.x` version string (currently
    /// `"1.0.0"` and `"1.1.0"`).  Files declaring `"2.x"` or higher are
    /// rejected.  This forward compatibility lets readers in this crate open
    /// pre-existing 1.0 files even after we bump our writer to 1.1.
    pub fn validate(&self) -> Result<()> {
        // Major-version compatibility check.  Accepts any 1.x.y version string.
        if !is_compatible_version(&self.version) {
            return Err(GeoParquetError::invalid_metadata(format!(
                "Unsupported GeoParquet version: {} (expected 1.x)",
                self.version
            )));
        }

        // Check primary column exists
        if !self.columns.contains_key(&self.primary_column) {
            return Err(GeoParquetError::missing_field(&self.primary_column));
        }

        // Validate each column
        for (name, column) in &self.columns {
            column.validate().map_err(|e| {
                GeoParquetError::invalid_metadata(format!("Invalid column '{}': {}", name, e))
            })?;
        }

        Ok(())
    }

    /// Serializes to JSON string
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    /// Deserializes from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        let metadata: Self = serde_json::from_str(json)?;
        metadata.validate()?;
        Ok(metadata)
    }
}

/// Metadata for a single geometry column
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeometryColumnMetadata {
    /// Encoding format (currently only "WKB" is supported)
    pub encoding: EncodingType,

    /// Geometry types present in this column
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub geometry_types: Vec<String>,

    /// Coordinate Reference System
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<Crs>,

    /// Column-level bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<f64>>,

    /// Edges interpretation (planar or spherical)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<EdgesInterpretation>,

    /// Orientation (counter-clockwise for polygons)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<Orientation>,

    /// Epoch for coordinate reference system
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<f64>,

    /// GeoParquet 1.1 `covering` object.
    ///
    /// Points at the auxiliary bounding-box columns (`covering.bbox.{xmin,
    /// ymin, xmax, ymax}`) that let a reader prune row groups and rows without
    /// decoding WKB.  Absent for files written by GeoParquet 1.0 writers or by
    /// writers that don't emit covering columns.
    ///
    /// This is an additive field — the struct carries no `deny_unknown_fields`
    /// so deserializing a document lacking `covering` yields `None`, and older
    /// serialized documents remain readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covering: Option<Covering>,
}

/// GeoParquet 1.1 `covering` object, naming the auxiliary bounding-box columns
/// that "cover" a geometry column.
///
/// Currently the specification defines exactly one covering kind, `bbox`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Covering {
    /// The bounding-box covering: paths to the four extent columns.
    pub bbox: CoveringBbox,
}

/// Paths to the four covering bounding-box columns, per GeoParquet 1.1.
///
/// Each field is a column path expressed as an array of path components, e.g.
/// `["bbox", "xmin"]` for a struct-nested bbox column named `bbox`, or
/// `["geometry_bbox_xmin"]` for a flat top-level column.  The paths are matched
/// verbatim against the Parquet leaf column paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoveringBbox {
    /// Path to the column holding each row's minimum X extent.
    pub xmin: Vec<String>,
    /// Path to the column holding each row's minimum Y extent.
    pub ymin: Vec<String>,
    /// Path to the column holding each row's maximum X extent.
    pub xmax: Vec<String>,
    /// Path to the column holding each row's maximum Y extent.
    pub ymax: Vec<String>,
}

impl Covering {
    /// Constructs a `covering.bbox` object from a single struct-root column
    /// name whose children are `xmin`, `ymin`, `xmax`, `ymax`.
    ///
    /// This mirrors the common VIDA / GeoParquet 1.1 layout where the covering
    /// lives in a struct column literally named `bbox`.
    pub fn bbox_struct(root: &str) -> Self {
        Self {
            bbox: CoveringBbox {
                xmin: vec![root.to_string(), "xmin".to_string()],
                ymin: vec![root.to_string(), "ymin".to_string()],
                xmax: vec![root.to_string(), "xmax".to_string()],
                ymax: vec![root.to_string(), "ymax".to_string()],
            },
        }
    }
}

impl GeometryColumnMetadata {
    /// Creates new geometry column metadata with WKB encoding
    pub fn new_wkb() -> Self {
        Self {
            encoding: EncodingType::Wkb,
            geometry_types: Vec::new(),
            crs: None,
            bbox: None,
            edges: None,
            orientation: None,
            epoch: None,
            covering: None,
        }
    }

    /// Creates new geometry column metadata with a GeoArrow native encoding.
    ///
    /// `encoding` selects the geometry shape (`Point`, `LineString`, `Polygon`,
    /// `MultiPoint`, `MultiLineString`, `MultiPolygon`).  Native encodings are
    /// only meaningful for non-mixed columns; passing [`EncodingType::Wkb`] is
    /// equivalent to [`Self::new_wkb`].
    pub fn new_native(encoding: EncodingType) -> Self {
        Self {
            encoding,
            geometry_types: Vec::new(),
            crs: None,
            bbox: None,
            edges: None,
            orientation: None,
            epoch: None,
            covering: None,
        }
    }

    /// Sets the CRS
    pub fn with_crs(mut self, crs: Crs) -> Self {
        self.crs = Some(crs);
        self
    }

    /// Sets the bounding box
    pub fn with_bbox(mut self, bbox: Vec<f64>) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Sets geometry types
    pub fn with_geometry_types(mut self, types: Vec<String>) -> Self {
        self.geometry_types = types;
        self
    }

    /// Sets edges interpretation
    pub fn with_edges(mut self, edges: EdgesInterpretation) -> Self {
        self.edges = Some(edges);
        self
    }

    /// Sets polygon orientation
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = Some(orientation);
        self
    }

    /// Sets the GeoParquet 1.1 `covering` object (auxiliary bbox columns).
    pub fn with_covering(mut self, covering: Covering) -> Self {
        self.covering = Some(covering);
        self
    }

    /// Validates the column metadata.
    ///
    /// All `EncodingType` variants — WKB and the GeoArrow native encodings —
    /// are accepted.  Bbox values, when present, must satisfy the standard
    /// min ≤ max ordering on every axis.  The CRS, if present, is validated
    /// recursively.
    pub fn validate(&self) -> Result<()> {
        // All EncodingType variants are valid in GeoParquet 1.1: WKB plus the
        // six native GeoArrow geometry shapes.  The encoding is enforced at
        // schema-construction time (`create_geometry_field_for`) and at
        // write-batch time (mixed types in a native column → reject), not
        // here at metadata-validation time.

        // Validate bbox if present
        if let Some(ref bbox) = self.bbox {
            if bbox.len() != 4 && bbox.len() != 6 {
                return Err(GeoParquetError::invalid_bbox(format!(
                    "Bounding box must have 4 or 6 elements, got {}",
                    bbox.len()
                )));
            }

            // Check min/max ordering
            if bbox.len() == 4 {
                if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
                    return Err(GeoParquetError::invalid_bbox(
                        "Min values must be <= max values",
                    ));
                }
            } else if bbox.len() == 6
                && (bbox[0] > bbox[3] || bbox[1] > bbox[4] || bbox[2] > bbox[5])
            {
                return Err(GeoParquetError::invalid_bbox(
                    "Min values must be <= max values",
                ));
            }
        }

        // Validate CRS if present
        if let Some(ref crs) = self.crs {
            crs.validate()?;
        }

        Ok(())
    }
}

/// Geometry encoding type as declared in the GeoParquet `geo` metadata
/// `columns.<name>.encoding` field.
///
/// `WKB` is the legacy GeoParquet 1.0 encoding (each row is an opaque WKB
/// `BinaryArray` blob, mixing geometry types is allowed).  The remaining
/// variants are GeoArrow 1.1 native encodings — each binds the column to a
/// single uniform geometry type and a structured Arrow array shape:
///
/// | Variant | Arrow shape (interleaved) |
/// |---|---|
/// | `Point` | `FixedSizeList<f64, N>` |
/// | `LineString` | `List<FixedSizeList<f64, N>>` |
/// | `Polygon` | `List<List<FixedSizeList<f64, N>>>` |
/// | `MultiPoint` | `List<FixedSizeList<f64, N>>` |
/// | `MultiLineString` | `List<List<FixedSizeList<f64, N>>>` |
/// | `MultiPolygon` | `List<List<List<FixedSizeList<f64, N>>>>` |
///
/// `N` is the coordinate arity (`CoordDim::arity()`): 2 for XY, 3 for XYZ/XYM,
/// 4 for XYZM.  Note that `LineString`/`MultiPoint` and
/// `Polygon`/`MultiLineString` share an Arrow shape — disambiguation comes from
/// the `ARROW:extension:name` field metadata, never from the array structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EncodingType {
    /// Well-Known Binary encoding (GeoParquet 1.0; mixed types allowed).
    #[serde(rename = "WKB")]
    Wkb,
    /// GeoArrow 1.1 native `point` encoding.
    #[serde(rename = "point")]
    Point,
    /// GeoArrow 1.1 native `linestring` encoding.
    #[serde(rename = "linestring")]
    LineString,
    /// GeoArrow 1.1 native `polygon` encoding.
    #[serde(rename = "polygon")]
    Polygon,
    /// GeoArrow 1.1 native `multipoint` encoding.
    #[serde(rename = "multipoint")]
    MultiPoint,
    /// GeoArrow 1.1 native `multilinestring` encoding.
    #[serde(rename = "multilinestring")]
    MultiLineString,
    /// GeoArrow 1.1 native `multipolygon` encoding.
    #[serde(rename = "multipolygon")]
    MultiPolygon,
}

impl EncodingType {
    /// Returns `true` if this is the WKB encoding (the legacy 1.0 path).
    pub const fn is_wkb(self) -> bool {
        matches!(self, Self::Wkb)
    }

    /// Returns `true` if this is a GeoArrow native encoding.
    pub const fn is_native(self) -> bool {
        !self.is_wkb()
    }
}

/// Coordinate dimensionality / arity for native GeoArrow geometry encodings.
///
/// GeoArrow stores coordinates *interleaved* in a `FixedSizeList<f64, N>`,
/// where `N` is the value returned by [`Self::arity`].  The variant chosen at
/// the writer determines `N` for every coordinate in the column — native
/// encodings cannot mix dimensionalities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoordDim {
    /// 2D coordinates (`x, y`); arity = 2.
    Xy,
    /// 3D coordinates with elevation (`x, y, z`); arity = 3.
    Xyz,
    /// 2D coordinates with measure (`x, y, m`); arity = 3.
    Xym,
    /// 4D coordinates (`x, y, z, m`); arity = 4.
    Xyzm,
}

impl CoordDim {
    /// Returns the number of f64 values per coordinate (the FixedSizeList size).
    pub const fn arity(self) -> usize {
        match self {
            Self::Xy => 2,
            Self::Xyz | Self::Xym => 3,
            Self::Xyzm => 4,
        }
    }

    /// Returns the [`CoordDim`] for the given arity, or `None` if unsupported.
    ///
    /// Arity 3 maps to [`Self::Xyz`] by default — the writer should set the
    /// field metadata explicitly to disambiguate XYZ from XYM if needed.
    pub const fn from_arity(n: usize) -> Option<Self> {
        match n {
            2 => Some(Self::Xy),
            3 => Some(Self::Xyz),
            4 => Some(Self::Xyzm),
            _ => None,
        }
    }

    /// Returns `true` if this dimensionality has a Z component.
    pub const fn has_z(self) -> bool {
        matches!(self, Self::Xyz | Self::Xyzm)
    }

    /// Returns `true` if this dimensionality has an M component.
    pub const fn has_m(self) -> bool {
        matches!(self, Self::Xym | Self::Xyzm)
    }
}

/// Returns `true` if `version` is a 1.x GeoParquet specification version that
/// this crate accepts on read.
fn is_compatible_version(version: &str) -> bool {
    // Strip any pre-release / build suffix, then check the major component.
    let core = version
        .split('-')
        .next()
        .unwrap_or(version)
        .split('+')
        .next()
        .unwrap_or(version);
    let major = core.split('.').next().unwrap_or("");
    major == "1"
}

/// Coordinate Reference System
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Crs {
    /// PROJJSON CRS definition
    ProjJson(serde_json::Value),
    /// WKT2 CRS string
    Wkt2(String),
}

impl Crs {
    /// Creates a CRS from WKT2 string
    pub fn from_wkt2(wkt: impl Into<String>) -> Self {
        Self::Wkt2(wkt.into())
    }

    /// Creates a CRS from EPSG code
    pub fn from_epsg(code: u32) -> Self {
        Self::ProjJson(serde_json::json!({
            "type": "GeographicCRS",
            "id": {
                "authority": "EPSG",
                "code": code
            }
        }))
    }

    /// Returns WGS 84 (EPSG:4326)
    pub fn wgs84() -> Self {
        Self::from_epsg(4326)
    }

    /// Validates the CRS
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Wkt2(wkt) => {
                if wkt.is_empty() {
                    return Err(GeoParquetError::invalid_crs("Empty WKT2 string"));
                }
                Ok(())
            }
            Self::ProjJson(json) => {
                if !json.is_object() {
                    return Err(GeoParquetError::invalid_crs("PROJJSON must be an object"));
                }
                Ok(())
            }
        }
    }
}

/// Edges interpretation for geodetic coordinate systems
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgesInterpretation {
    /// Planar edges (straight lines in projected coordinates)
    Planar,
    /// Spherical edges (great circle arcs)
    Spherical,
}

/// Polygon ring orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    /// Counter-clockwise orientation (exterior rings)
    CounterClockwise,
}

/// Statistics for a geometry column
#[derive(Debug, Clone, Default)]
pub struct GeometryStatistics {
    /// Total number of geometries
    pub count: u64,
    /// Number of null geometries
    pub null_count: u64,
    /// Bounding box covering all geometries
    pub bbox: Option<Vec<f64>>,
    /// Geometry types encountered
    pub geometry_types: Vec<String>,
}

impl GeometryStatistics {
    /// Creates new empty statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates statistics with a new geometry
    pub fn update(&mut self, geometry_type: Option<&str>, bbox: Option<&[f64]>) {
        self.count += 1;

        if let Some(geom_type) = geometry_type {
            if !self.geometry_types.contains(&geom_type.to_string()) {
                self.geometry_types.push(geom_type.to_string());
            }

            if let Some(new_bbox) = bbox {
                if let Some(ref mut existing_bbox) = self.bbox {
                    // Merge bounding boxes
                    Self::merge_bbox(existing_bbox, new_bbox);
                } else {
                    self.bbox = Some(new_bbox.to_vec());
                }
            }
        } else {
            self.null_count += 1;
        }
    }

    /// Merges two bounding boxes (expands existing to include new)
    fn merge_bbox(existing: &mut [f64], new: &[f64]) {
        if existing.len() == new.len() {
            if existing.len() == 4 {
                // 2D bbox: [minx, miny, maxx, maxy]
                existing[0] = existing[0].min(new[0]); // minx
                existing[1] = existing[1].min(new[1]); // miny
                existing[2] = existing[2].max(new[2]); // maxx
                existing[3] = existing[3].max(new[3]); // maxy
            } else if existing.len() == 6 {
                // 3D bbox: [minx, miny, minz, maxx, maxy, maxz]
                existing[0] = existing[0].min(new[0]); // minx
                existing[1] = existing[1].min(new[1]); // miny
                existing[2] = existing[2].min(new[2]); // minz
                existing[3] = existing[3].max(new[3]); // maxx
                existing[4] = existing[4].max(new[4]); // maxy
                existing[5] = existing[5].max(new[5]); // maxz
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geoparquet_metadata_creation() {
        let mut metadata = GeoParquetMetadata::new("geometry");
        let column = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0]);
        metadata.add_column("geometry", column);

        assert_eq!(metadata.version, GEOPARQUET_VERSION);
        assert_eq!(metadata.primary_column, "geometry");
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_geometry_column_metadata() {
        let metadata = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0])
            .with_geometry_types(vec!["Point".to_string(), "Polygon".to_string()]);

        assert_eq!(metadata.encoding, EncodingType::Wkb);
        assert!(metadata.crs.is_some());
        assert_eq!(metadata.geometry_types.len(), 2);
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_invalid_bbox() {
        let metadata = GeometryColumnMetadata::new_wkb().with_bbox(vec![1.0, 2.0, 3.0]); // Only 3 elements

        assert!(metadata.validate().is_err());
    }

    #[test]
    fn test_bbox_ordering() {
        let metadata = GeometryColumnMetadata::new_wkb().with_bbox(vec![10.0, 20.0, 5.0, 15.0]); // min > max

        assert!(metadata.validate().is_err());
    }

    #[test]
    fn test_crs_creation() {
        let wgs84 = Crs::wgs84();
        assert!(wgs84.validate().is_ok());

        let wkt = Crs::from_wkt2("GEOGCS[\"WGS 84\"]");
        assert!(wkt.validate().is_ok());

        let empty_wkt = Crs::from_wkt2("");
        assert!(empty_wkt.validate().is_err());
    }

    #[test]
    fn test_metadata_serialization() {
        let mut metadata = GeoParquetMetadata::new("geometry");
        let column = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0]);
        metadata.add_column("geometry", column);

        let json = metadata.to_json();
        assert!(json.is_ok());

        let deserialized = GeoParquetMetadata::from_json(&json.expect("json should serialize"));
        assert!(deserialized.is_ok());
        assert_eq!(deserialized.expect("should deserialize"), metadata);
    }

    // ── New EncodingType / CoordDim / version tests ──────────────────────────

    /// A GeoParquet `1.0.0` file written by an older OxiGeo must still load.
    #[test]
    fn test_legacy_1_0_0_metadata_validates() {
        // Hand-craft a metadata document that declares version 1.0.0
        // (older writers will produce this).
        let json = r#"{
            "version": "1.0.0",
            "primary_column": "geometry",
            "columns": {
                "geometry": {"encoding": "WKB"}
            }
        }"#;
        let parsed = GeoParquetMetadata::from_json(json).expect("legacy 1.0 should parse");
        assert_eq!(parsed.version, "1.0.0");
    }

    #[test]
    fn test_geoparquet_version_is_1_1_0() {
        // The writer constant is bumped, but the reader still accepts 1.0.
        assert_eq!(GEOPARQUET_VERSION, "1.1.0");
        assert_eq!(GEOPARQUET_VERSION_MIN, "1.0.0");
    }

    #[test]
    fn test_encoding_type_point_serde_roundtrip() {
        let enc = EncodingType::Point;
        let json = serde_json::to_string(&enc).expect("ser");
        assert_eq!(json, "\"point\"");
        let back: EncodingType = serde_json::from_str(&json).expect("de");
        assert_eq!(back, EncodingType::Point);
    }

    #[test]
    fn test_encoding_type_polygon_serde_roundtrip() {
        let enc = EncodingType::Polygon;
        let json = serde_json::to_string(&enc).expect("ser");
        assert_eq!(json, "\"polygon\"");
        let back: EncodingType = serde_json::from_str(&json).expect("de");
        assert_eq!(back, EncodingType::Polygon);
    }

    #[test]
    fn test_encoding_type_validate_native_passes() {
        // Native point column should validate cleanly.
        let column = GeometryColumnMetadata::new_native(EncodingType::Point);
        assert!(column.validate().is_ok());

        // The metadata wrapper should also pass with version bumped.
        let mut metadata = GeoParquetMetadata::new("geometry");
        metadata.add_column("geometry", column);
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_encoding_type_all_native_variants_serde() {
        let cases = [
            (EncodingType::Wkb, "WKB"),
            (EncodingType::Point, "point"),
            (EncodingType::LineString, "linestring"),
            (EncodingType::Polygon, "polygon"),
            (EncodingType::MultiPoint, "multipoint"),
            (EncodingType::MultiLineString, "multilinestring"),
            (EncodingType::MultiPolygon, "multipolygon"),
        ];
        for (enc, expected) in cases {
            let s = serde_json::to_string(&enc).expect("ser");
            assert_eq!(s, format!("\"{expected}\""));
            let back: EncodingType = serde_json::from_str(&s).expect("de");
            assert_eq!(back, enc);
        }
    }

    #[test]
    fn test_coord_dim_arity() {
        assert_eq!(CoordDim::Xy.arity(), 2);
        assert_eq!(CoordDim::Xyz.arity(), 3);
        assert_eq!(CoordDim::Xym.arity(), 3);
        assert_eq!(CoordDim::Xyzm.arity(), 4);

        assert_eq!(CoordDim::from_arity(2), Some(CoordDim::Xy));
        assert_eq!(CoordDim::from_arity(3), Some(CoordDim::Xyz));
        assert_eq!(CoordDim::from_arity(4), Some(CoordDim::Xyzm));
        assert_eq!(CoordDim::from_arity(5), None);

        assert!(!CoordDim::Xy.has_z() && !CoordDim::Xy.has_m());
        assert!(CoordDim::Xyz.has_z() && !CoordDim::Xyz.has_m());
        assert!(!CoordDim::Xym.has_z() && CoordDim::Xym.has_m());
        assert!(CoordDim::Xyzm.has_z() && CoordDim::Xyzm.has_m());
    }

    #[test]
    fn test_geometry_statistics() {
        let mut stats = GeometryStatistics::new();

        stats.update(Some("Point"), Some(&[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(stats.count, 1);
        assert_eq!(stats.null_count, 0);
        assert!(stats.bbox.is_some());

        stats.update(Some("Polygon"), Some(&[0.0, 0.0, 5.0, 5.0]));
        assert_eq!(stats.count, 2);
        assert_eq!(stats.geometry_types.len(), 2);

        // Check bbox was expanded
        let bbox = stats.bbox.as_ref().expect("bbox should exist");
        assert_eq!(bbox, &vec![0.0, 0.0, 5.0, 5.0]);

        stats.update(None, None);
        assert_eq!(stats.null_count, 1);
    }
}
