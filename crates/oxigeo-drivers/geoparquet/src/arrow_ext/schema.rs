//! Arrow schema utilities for GeoParquet.
//!
//! GeoParquet 1.1 uses the Arrow extension-name protocol on a column's
//! [`arrow_schema::Field`] metadata to declare both the encoding family (WKB
//! or one of six GeoArrow native encodings) and any per-column metadata
//! (CRS, edges interpretation, coord_type).  This module centralises:
//!
//! * The mapping from [`EncodingType`] to extension-name string
//!   ([`geoarrow_extension_name`]) — formerly the static
//!   `GEOPARQUET_EXTENSION_NAME` constant.
//! * The construction of nested Arrow types appropriate for each encoding
//!   ([`create_geometry_field_for`]).
//!
//! Older code that referenced the legacy WKB-only constant continues to work
//! because [`is_geometry_column`] now matches the full set of extension names.

use crate::error::{GeoParquetError, Result};
use crate::metadata::{CoordDim, Crs, EncodingType, GeometryColumnMetadata};
use arrow_schema::{DataType, Field, Schema};
use std::collections::HashMap;
use std::sync::Arc;

/// Metadata key for GeoParquet geometry column marker.
pub const GEO_COLUMN_MARKER: &str = "ARROW:extension:name";

/// Metadata key for the per-field GeoArrow metadata JSON object.
pub const GEO_COLUMN_METADATA_MARKER: &str = "ARROW:extension:metadata";

/// Returns the GeoArrow extension-name string for a given encoding.
///
/// The string is what gets stored in `ARROW:extension:name` field metadata and
/// is the *only* way to disambiguate `geoarrow.linestring` vs
/// `geoarrow.multipoint` (which share the same nested Arrow shape).
pub fn geoarrow_extension_name(e: EncodingType) -> &'static str {
    match e {
        EncodingType::Wkb => "geoarrow.wkb",
        EncodingType::Point => "geoarrow.point",
        EncodingType::LineString => "geoarrow.linestring",
        EncodingType::Polygon => "geoarrow.polygon",
        EncodingType::MultiPoint => "geoarrow.multipoint",
        EncodingType::MultiLineString => "geoarrow.multilinestring",
        EncodingType::MultiPolygon => "geoarrow.multipolygon",
    }
}

/// Inverse of [`geoarrow_extension_name`].
///
/// Returns `None` for any extension name that is not a recognised GeoArrow
/// encoding (lets callers gracefully ignore non-geometry extension fields).
pub fn encoding_from_extension_name(name: &str) -> Option<EncodingType> {
    Some(match name {
        "geoarrow.wkb" => EncodingType::Wkb,
        "geoarrow.point" => EncodingType::Point,
        "geoarrow.linestring" => EncodingType::LineString,
        "geoarrow.polygon" => EncodingType::Polygon,
        "geoarrow.multipoint" => EncodingType::MultiPoint,
        "geoarrow.multilinestring" => EncodingType::MultiLineString,
        "geoarrow.multipolygon" => EncodingType::MultiPolygon,
        _ => return None,
    })
}

// ── Field constructors ──────────────────────────────────────────────────────────

/// Builds the inner *coord* type used by every native GeoArrow encoding —
/// `FixedSizeList<f64, N>` where `N = dim.arity()`.
///
/// The element field is named `xy`/`xyz`/`xym`/`xyzm` per GeoArrow convention.
fn coord_type(dim: CoordDim) -> DataType {
    let element_name = match dim {
        CoordDim::Xy => "xy",
        CoordDim::Xyz => "xyz",
        CoordDim::Xym => "xym",
        CoordDim::Xyzm => "xyzm",
    };
    DataType::FixedSizeList(
        Arc::new(Field::new(element_name, DataType::Float64, false)),
        dim.arity() as i32,
    )
}

/// Builds the Arrow `DataType` for a given encoding + coord dimensionality.
///
/// Note that [`EncodingType::LineString`] / [`EncodingType::MultiPoint`] and
/// [`EncodingType::Polygon`] / [`EncodingType::MultiLineString`] share an
/// Arrow shape — disambiguation comes from the field's extension-name
/// metadata, not its `DataType`.
fn data_type_for(encoding: EncodingType, dim: CoordDim) -> DataType {
    let coord = coord_type(dim);
    let inner_field = |name: &str, dt: DataType| Field::new(name, dt, false);
    match encoding {
        EncodingType::Wkb => DataType::Binary,
        EncodingType::Point => coord,
        EncodingType::LineString | EncodingType::MultiPoint => {
            DataType::List(Arc::new(inner_field("vertices", coord)))
        }
        EncodingType::Polygon | EncodingType::MultiLineString => {
            let ring = DataType::List(Arc::new(inner_field("vertices", coord)));
            DataType::List(Arc::new(inner_field("rings", ring)))
        }
        EncodingType::MultiPolygon => {
            let ring = DataType::List(Arc::new(inner_field("vertices", coord_type(dim))));
            let polygon = DataType::List(Arc::new(inner_field("rings", ring)));
            DataType::List(Arc::new(inner_field("polygons", polygon)))
        }
    }
}

/// Constructs the JSON value for the `ARROW:extension:metadata` field-level
/// metadata key.  Only emits keys that are actually set.
fn extension_metadata_json(
    encoding: EncodingType,
    dim: CoordDim,
    crs: Option<&Crs>,
) -> Option<String> {
    // For WKB the GeoParquet 1.0 convention is that field-level extension
    // metadata is empty; the column-level metadata in the file's `geo` JSON
    // carries CRS and other info.  Native encodings benefit from per-field
    // metadata because the GeoArrow spec encodes coord_type there.
    if encoding.is_wkb() && crs.is_none() {
        return None;
    }
    let mut obj = serde_json::Map::new();
    if encoding.is_native() {
        obj.insert(
            "coord_type".to_string(),
            serde_json::Value::String("interleaved".to_string()),
        );
        // `dim` is implicit in the FixedSizeList arity, but writing it
        // explicitly aids tooling that doesn't introspect the array shape.
        let dim_str = match dim {
            CoordDim::Xy => "xy",
            CoordDim::Xyz => "xyz",
            CoordDim::Xym => "xym",
            CoordDim::Xyzm => "xyzm",
        };
        obj.insert(
            "dim".to_string(),
            serde_json::Value::String(dim_str.to_string()),
        );
    }
    if let Some(c) = crs {
        match c {
            Crs::ProjJson(v) => {
                obj.insert("crs".to_string(), v.clone());
            }
            Crs::Wkt2(s) => {
                obj.insert("crs".to_string(), serde_json::Value::String(s.clone()));
            }
        }
    }
    if obj.is_empty() {
        None
    } else {
        serde_json::to_string(&serde_json::Value::Object(obj)).ok()
    }
}

/// Build a geometry [`Field`] for a specific encoding + coordinate
/// dimensionality.
///
/// This is the canonical writer-side constructor for all geometry columns —
/// both WKB and native GeoArrow encodings.  It sets:
///
/// * The Arrow `DataType` per `data_type_for`.
/// * `ARROW:extension:name` to [`geoarrow_extension_name`] for the encoding.
/// * `ARROW:extension:metadata` to a JSON object containing `coord_type`,
///   `dim`, and optionally `crs`, when relevant.
///
/// Pass `crs: None` to omit the CRS from per-field metadata; a per-column CRS
/// can still be written into the file-level `geo` metadata blob.
pub fn create_geometry_field_for(
    name: &str,
    encoding: EncodingType,
    dim: CoordDim,
    nullable: bool,
    crs: Option<&Crs>,
) -> Field {
    let data_type = data_type_for(encoding, dim);
    let mut field_metadata = HashMap::new();
    field_metadata.insert(
        GEO_COLUMN_MARKER.to_string(),
        geoarrow_extension_name(encoding).to_string(),
    );
    if let Some(meta_json) = extension_metadata_json(encoding, dim, crs) {
        field_metadata.insert(GEO_COLUMN_METADATA_MARKER.to_string(), meta_json);
    }
    Field::new(name, data_type, nullable).with_metadata(field_metadata)
}

/// Extension of Arrow Field for geometry columns
pub struct GeoArrowField {
    /// The underlying Arrow field
    field: Field,
    /// Geometry column metadata
    metadata: GeometryColumnMetadata,
}

impl GeoArrowField {
    /// Creates a new geometry field.
    ///
    /// For backwards compatibility this defaults to WKB / 2D when the metadata
    /// declares the legacy `EncodingType::Wkb`.  Callers who want native
    /// encoding should invoke [`Self::new_with_dim`] directly.
    pub fn new(name: impl Into<String>, metadata: GeometryColumnMetadata) -> Self {
        Self::new_with_dim(name, metadata, CoordDim::Xy)
    }

    /// Creates a new geometry field with explicit coordinate dimensionality.
    pub fn new_with_dim(
        name: impl Into<String>,
        metadata: GeometryColumnMetadata,
        dim: CoordDim,
    ) -> Self {
        let name = name.into();
        let field =
            create_geometry_field_for(&name, metadata.encoding, dim, true, metadata.crs.as_ref());
        Self { field, metadata }
    }

    /// Returns the Arrow field
    pub fn field(&self) -> &Field {
        &self.field
    }

    /// Returns the geometry metadata
    pub fn metadata(&self) -> &GeometryColumnMetadata {
        &self.metadata
    }

    /// Consumes self and returns the Arrow field
    pub fn into_field(self) -> Field {
        self.field
    }
}

/// Schema builder with GeoParquet support
pub struct SchemaBuilder {
    fields: Vec<Field>,
    geometry_columns: HashMap<String, GeometryColumnMetadata>,
    primary_column: Option<String>,
}

impl SchemaBuilder {
    /// Creates a new schema builder
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            geometry_columns: HashMap::new(),
            primary_column: None,
        }
    }

    /// Adds a regular field
    pub fn add_field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// Adds a geometry column
    pub fn add_geometry_column(
        mut self,
        name: impl Into<String>,
        metadata: GeometryColumnMetadata,
        is_primary: bool,
    ) -> Self {
        let name_str = name.into();

        let geo_field = GeoArrowField::new(name_str.clone(), metadata.clone());
        self.fields.push(geo_field.into_field());
        self.geometry_columns.insert(name_str.clone(), metadata);

        if is_primary || self.primary_column.is_none() {
            self.primary_column = Some(name_str);
        }

        self
    }

    /// Builds the Arrow schema with GeoParquet metadata
    pub fn build(self) -> Result<(Schema, crate::metadata::GeoParquetMetadata)> {
        if self.geometry_columns.is_empty() {
            return Err(GeoParquetError::invalid_schema(
                "Schema must contain at least one geometry column",
            ));
        }

        let primary_column = self
            .primary_column
            .ok_or_else(|| GeoParquetError::invalid_schema("No primary geometry column set"))?;

        // Create GeoParquet metadata
        let mut geo_metadata = crate::metadata::GeoParquetMetadata::new(primary_column);
        for (name, metadata) in self.geometry_columns {
            geo_metadata.add_column(name, metadata);
        }

        // Create Arrow schema
        let schema = Schema::new(self.fields);

        Ok((schema, geo_metadata))
    }
}

impl Default for SchemaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if `field` carries any GeoArrow extension-name marker.
///
/// This recognises every native GeoArrow encoding *and* the legacy
/// `geoarrow.wkb` marker, so existing WKB-based code paths still match.  The
/// underlying Arrow `DataType` is intentionally **not** checked here — native
/// encodings have nested types, and the extension name is the contract.
pub fn is_geometry_column(field: &Field) -> bool {
    field
        .metadata()
        .get(GEO_COLUMN_MARKER)
        .and_then(|s| encoding_from_extension_name(s))
        .is_some()
}

/// Returns the encoding declared by `field`'s `ARROW:extension:name`, or
/// `None` if the field is not a GeoArrow geometry column.
pub fn field_encoding(field: &Field) -> Option<EncodingType> {
    field
        .metadata()
        .get(GEO_COLUMN_MARKER)
        .and_then(|s| encoding_from_extension_name(s))
}

/// Returns the coordinate dimensionality declared by the field.
///
/// Inferred from the FixedSizeList arity of the (possibly nested) coord array.
/// Returns `None` for WKB or any field that cannot be classified.
pub fn field_coord_dim(field: &Field) -> Option<CoordDim> {
    fn drill(dt: &DataType) -> Option<CoordDim> {
        match dt {
            DataType::FixedSizeList(_, n) => CoordDim::from_arity(*n as usize),
            DataType::List(inner) => drill(inner.data_type()),
            _ => None,
        }
    }
    drill(field.data_type())
}

/// Adds a geometry column to an existing schema (defaults to 2-D)
pub fn add_geometry_column(
    schema: &Schema,
    name: impl Into<String>,
    metadata: GeometryColumnMetadata,
) -> Result<Schema> {
    let geo_field = GeoArrowField::new(name, metadata);

    let mut fields: Vec<Arc<Field>> = schema.fields().iter().cloned().collect();
    fields.push(Arc::new(geo_field.into_field()));

    Ok(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

/// Extracts geometry column metadata from a field.
///
/// Returns `None` if the field is not a recognised geometry column.  The
/// returned [`GeometryColumnMetadata`] always uses the encoding declared on
/// the field (Wkb, Point, LineString, …); CRS and other rich metadata still
/// have to be merged from the file-level `geo` JSON.
pub fn extract_geometry_metadata(field: &Field) -> Result<Option<GeometryColumnMetadata>> {
    let Some(encoding) = field_encoding(field) else {
        return Ok(None);
    };
    Ok(Some(GeometryColumnMetadata::new_native(encoding)))
}

/// Creates a simple schema with a single (WKB) geometry column.
pub fn create_simple_geometry_schema(
    geometry_column_name: impl Into<String>,
    crs: Option<Crs>,
) -> Result<(Schema, crate::metadata::GeoParquetMetadata)> {
    let name = geometry_column_name.into();

    let mut metadata = GeometryColumnMetadata::new_wkb();
    if let Some(crs) = crs {
        metadata = metadata.with_crs(crs);
    }

    SchemaBuilder::new()
        .add_geometry_column(name, metadata, true)
        .build()
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::metadata::Crs;

    #[test]
    fn test_geo_arrow_field_wkb() {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
        let geo_field = GeoArrowField::new("geometry", metadata);

        assert_eq!(geo_field.field().name(), "geometry");
        assert_eq!(geo_field.field().data_type(), &DataType::Binary);
        assert!(is_geometry_column(geo_field.field()));
        assert_eq!(field_encoding(geo_field.field()), Some(EncodingType::Wkb));
    }

    #[test]
    fn test_geo_arrow_field_native_point_2d() {
        let metadata = GeometryColumnMetadata::new_native(EncodingType::Point);
        let geo_field = GeoArrowField::new_with_dim("geom", metadata, CoordDim::Xy);
        match geo_field.field().data_type() {
            DataType::FixedSizeList(_, 2) => {}
            other => panic!("expected FixedSizeList<f64,2>, got {other:?}"),
        }
        assert_eq!(field_encoding(geo_field.field()), Some(EncodingType::Point));
        assert_eq!(field_coord_dim(geo_field.field()), Some(CoordDim::Xy));
    }

    #[test]
    fn test_geo_arrow_field_native_polygon_xyz() {
        let metadata = GeometryColumnMetadata::new_native(EncodingType::Polygon);
        let geo_field = GeoArrowField::new_with_dim("geom", metadata, CoordDim::Xyz);
        // List<List<FixedSizeList<f64, 3>>>
        match geo_field.field().data_type() {
            DataType::List(rings) => match rings.data_type() {
                DataType::List(verts) => match verts.data_type() {
                    DataType::FixedSizeList(_, 3) => {}
                    other => panic!("inner: {other:?}"),
                },
                other => panic!("rings: {other:?}"),
            },
            other => panic!("outer: {other:?}"),
        }
        assert_eq!(
            field_encoding(geo_field.field()),
            Some(EncodingType::Polygon)
        );
        assert_eq!(field_coord_dim(geo_field.field()), Some(CoordDim::Xyz));
    }

    #[test]
    fn test_extension_name_lookup() {
        assert_eq!(geoarrow_extension_name(EncodingType::Wkb), "geoarrow.wkb");
        assert_eq!(
            geoarrow_extension_name(EncodingType::MultiPolygon),
            "geoarrow.multipolygon"
        );
        assert_eq!(
            encoding_from_extension_name("geoarrow.linestring"),
            Some(EncodingType::LineString)
        );
        assert_eq!(encoding_from_extension_name("foo"), None);
    }

    #[test]
    fn test_schema_builder() {
        let metadata = GeometryColumnMetadata::new_wkb();

        let result = SchemaBuilder::new()
            .add_field(Field::new("id", DataType::Int64, false))
            .add_field(Field::new("name", DataType::Utf8, true))
            .add_geometry_column("geometry", metadata, true)
            .build();

        assert!(result.is_ok());
        let (schema, geo_meta) = result.expect("should build");
        assert_eq!(schema.fields().len(), 3);
        assert_eq!(geo_meta.primary_column, "geometry");
    }

    #[test]
    fn test_schema_builder_no_geometry() {
        let result = SchemaBuilder::new()
            .add_field(Field::new("id", DataType::Int64, false))
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_create_simple_geometry_schema() {
        let result = create_simple_geometry_schema("geom", Some(Crs::wgs84()));
        assert!(result.is_ok());

        let (schema, metadata) = result.expect("should create");
        assert_eq!(schema.fields().len(), 1);
        assert_eq!(metadata.primary_column, "geom");
    }

    #[test]
    fn test_is_geometry_column() {
        let metadata = GeometryColumnMetadata::new_wkb();
        let geo_field = GeoArrowField::new("geometry", metadata);
        assert!(is_geometry_column(geo_field.field()));

        let regular_field = Field::new("name", DataType::Utf8, true);
        assert!(!is_geometry_column(&regular_field));
    }
}
