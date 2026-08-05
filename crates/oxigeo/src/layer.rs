//! Vector layer access — the OGR half of the [`Dataset`] API.
//!
//! [`Dataset::layers`] enumerates the vector layers of an opened dataset and
//! [`Layer::features`] reads their features, each carrying an
//! [`oxigeo_core::vector::Geometry`] and a map of attribute
//! [`FieldValue`](oxigeo_core::vector::FieldValue)s.  This is the equivalent of
//! `GDALDataset::GetLayer()` / `OGRLayer::GetNextFeature()`.
//!
//! ```rust,no_run
//! use oxigeo::Dataset;
//!
//! # fn main() -> oxigeo::Result<()> {
//! let ds = Dataset::open("cities.gpkg")?;
//! for layer in ds.layers()? {
//!     println!("{} ({:?} features)", layer.name(), layer.feature_count());
//!     for feature in layer.features()? {
//!         println!("  {:?} {:?}", feature.geometry, feature.properties);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Supported formats
//!
//! | Format | Feature flag | Layers |
//! |---|---|---|
//! | ESRI Shapefile | `shapefile` (default) | one, named after the file stem |
//! | GeoJSON | `geojson` (default) | one, named after the file stem |
//! | GeoPackage | `gpkg` | one per `gpkg_contents` row of type `features` |
//!
//! Every other format returns [`OxiGeoError::NotSupported`] naming the driver,
//! rather than silently reporting an empty layer list.
//!
//! # Reading model
//!
//! [`Layer::features`] is **eager**: it reads the layer once and hands back an
//! iterator over the materialised features, so iteration itself cannot fail.
//! For files too large to hold in memory, use the streaming API
//! ([`crate::streaming::StreamingExt`]) or the `oxigeo-streaming` crate.

use oxigeo_core::error::OxiGeoError;
use oxigeo_core::vector::Feature;

use crate::{BoundingBox, Dataset, DatasetFormat, Result};

// ─── Public types ────────────────────────────────────────────────────────────

/// A vector layer of a [`Dataset`] — analogous to `OGRLayer`.
///
/// Obtained from [`Dataset::layers`], [`Dataset::layer`] or
/// [`Dataset::layer_by_name`].  The handle is cheap: it carries the layer's
/// metadata and enough information to re-open the source when
/// [`Layer::features`] is called.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Source file this layer lives in.
    path: String,
    /// Format of the source file, which selects the reader in
    /// [`Layer::features`].
    format: DatasetFormat,
    /// Name of the table / file stem backing this layer.
    name: String,
    /// Zero-based position of this layer within its dataset.
    index: usize,
    /// Declared OGC geometry type, when the format records one.
    geometry_type: Option<String>,
    /// Number of features, when it can be obtained without reading them all.
    feature_count: Option<u64>,
    /// CRS of this layer, as `"EPSG:<code>"` or a WKT string.
    crs: Option<String>,
    /// Declared spatial extent of the layer.
    bounds: Option<BoundingBox>,
    /// Attribute field names, in declaration order.
    field_names: Vec<String>,
}

impl Layer {
    /// Layer name — the GeoPackage table name, or the file stem for
    /// single-layer formats.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Zero-based index of this layer within its dataset.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Path of the file backing this layer.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Declared OGC geometry type of the layer, e.g. `"Point"` or
    /// `"MultiPolygon"`.
    ///
    /// This is the type recorded in the file's metadata (the Shapefile header's
    /// shape type, `gpkg_geometry_columns.geometry_type_name`), normalised to
    /// the OGC spelling.  Z/M dimensionality is not reflected here, and
    /// individual features may still carry a compatible sub-type — a Shapefile
    /// `Polygon` layer yields both `Polygon` and `MultiPolygon` features, as in
    /// GDAL.  `None` when the format does not declare one (GeoJSON with mixed
    /// geometry types, or a layer with no features).
    #[must_use]
    pub fn geometry_type(&self) -> Option<&str> {
        self.geometry_type.as_deref()
    }

    /// Number of features in the layer, when the format can report it without
    /// reading every feature.
    #[must_use]
    pub fn feature_count(&self) -> Option<u64> {
        self.feature_count
    }

    /// Coordinate reference system of the layer — `"EPSG:<code>"` for
    /// GeoPackage, the `.prj` WKT for Shapefile.
    #[must_use]
    pub fn crs(&self) -> Option<&str> {
        self.crs.as_deref()
    }

    /// Declared spatial extent of the layer in its own CRS.
    #[must_use]
    pub fn bounds(&self) -> Option<&BoundingBox> {
        self.bounds.as_ref()
    }

    /// Attribute field names in declaration order (the geometry column is not
    /// one of them).
    ///
    /// These are exactly the keys of [`Feature::properties`] for features of
    /// this layer, except that a feature omits fields it has no value for.
    #[must_use]
    pub fn field_names(&self) -> &[String] {
        &self.field_names
    }

    /// Read every feature of this layer.
    ///
    /// The layer is read once, up front, so the returned iterator is
    /// infallible — a decode error surfaces here rather than mid-iteration.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGeoError::Io`] when the source file cannot be read and
    /// [`OxiGeoError::Format`] when a geometry or record cannot be decoded.
    pub fn features(&self) -> Result<LayerFeatures> {
        let features = match self.format {
            DatasetFormat::Shapefile => shapefile_features(&self.path),
            DatasetFormat::GeoJson => geojson_features(&self.path),
            DatasetFormat::GeoPackage => gpkg_features(&self.path, &self.name),
            other => Err(unsupported_format(other, &self.path)),
        }?;

        Ok(LayerFeatures {
            inner: features.into_iter(),
        })
    }
}

/// Iterator over the features of a [`Layer`], as returned by
/// [`Layer::features`].
#[derive(Debug)]
pub struct LayerFeatures {
    inner: std::vec::IntoIter<Feature>,
}

impl Iterator for LayerFeatures {
    type Item = Feature;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for LayerFeatures {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

// ─── Dataset entry points ────────────────────────────────────────────────────

impl Dataset {
    /// Every vector layer in this dataset — the equivalent of iterating
    /// `GDALDataset::GetLayer(0..GetLayerCount())`.
    ///
    /// Single-layer formats (Shapefile, GeoJSON) return exactly one layer named
    /// after the file stem; a GeoPackage returns one layer per feature table
    /// registered in `gpkg_contents`, in table order.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGeoError::NotSupported`] when the dataset's format has no
    /// vector layer reader (raster formats, or a vector driver whose feature
    /// flag is disabled), and [`OxiGeoError::Io`] / [`OxiGeoError::Format`]
    /// when the file cannot be read or parsed.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigeo::Dataset;
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("roads.shp")?;
    /// for layer in ds.layers()? {
    ///     println!("{}: {:?}", layer.name(), layer.geometry_type());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn layers(&self) -> Result<Vec<Layer>> {
        match self.format() {
            DatasetFormat::Shapefile => shapefile_layers(self.path()),
            DatasetFormat::GeoJson => geojson_layers(self.path()),
            DatasetFormat::GeoPackage => gpkg_layers(self.path()),
            other => Err(unsupported_format(other, self.path())),
        }
    }

    /// The layer at `index`, counting from zero.
    ///
    /// # Errors
    ///
    /// In addition to the errors of [`Dataset::layers`], returns
    /// [`OxiGeoError::InvalidParameter`] when `index` is out of range.
    pub fn layer(&self, index: usize) -> Result<Layer> {
        let mut layers = self.layers()?;
        if index >= layers.len() {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "index",
                message: format!(
                    "layer index {index} is out of range: '{}' has {} layer(s)",
                    self.path(),
                    layers.len()
                ),
            });
        }
        Ok(layers.swap_remove(index))
    }

    /// The layer named `name` (ASCII case-insensitive).
    ///
    /// # Errors
    ///
    /// In addition to the errors of [`Dataset::layers`], returns
    /// [`OxiGeoError::InvalidParameter`] when no layer has that name.
    pub fn layer_by_name(&self, name: &str) -> Result<Layer> {
        let mut layers = self.layers()?;
        match layers
            .iter()
            .position(|layer| layer.name.eq_ignore_ascii_case(name))
        {
            Some(position) => Ok(layers.swap_remove(position)),
            None => {
                let available: Vec<&str> = layers.iter().map(Layer::name).collect();
                Err(OxiGeoError::InvalidParameter {
                    parameter: "name",
                    message: format!(
                        "'{}' has no layer named '{name}' (available: {available:?})",
                        self.path()
                    ),
                })
            }
        }
    }

    /// Names of every layer in this dataset, in layer order.
    ///
    /// # Errors
    ///
    /// Same as [`Dataset::layers`].
    pub fn layer_names(&self) -> Result<Vec<String>> {
        Ok(self.layers()?.into_iter().map(|layer| layer.name).collect())
    }
}

// ─── Shared helpers ──────────────────────────────────────────────────────────

/// Error for a format that has no layer reader compiled in.
fn unsupported_format(format: DatasetFormat, path: &str) -> OxiGeoError {
    OxiGeoError::NotSupported {
        operation: format!(
            "vector layer access for format '{}' ('{path}') — supported: ESRI Shapefile \
             (feature `shapefile`), GeoJSON (feature `geojson`), GeoPackage (feature `gpkg`)",
            format.driver_name()
        ),
    }
}

/// Error for a driver whose feature flag is disabled.
#[cfg(any(
    not(feature = "shapefile"),
    not(feature = "geojson"),
    not(feature = "gpkg")
))]
fn driver_disabled(driver: &str, feature: &str) -> OxiGeoError {
    OxiGeoError::NotSupported {
        operation: format!(
            "vector layer access for {driver}: rebuild oxigeo with the `{feature}` feature enabled"
        ),
    }
}

/// Layer name for single-layer formats: the file stem.
#[cfg(any(feature = "shapefile", feature = "geojson"))]
fn file_stem_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("layer")
        .to_string()
}

/// Normalise a driver-specific geometry-type spelling to the OGC one.
///
/// GeoPackage stores `"MULTIPOLYGON"`, the Shapefile header says `"PolyLine"`;
/// both become `"MultiLineString"`-style OGC names so that
/// [`Layer::geometry_type`] reads the same whatever the source.  Unrecognised
/// names are returned unchanged.
#[cfg(any(feature = "shapefile", feature = "gpkg"))]
fn canonical_geometry_type(name: &str) -> String {
    match name.to_ascii_uppercase().as_str() {
        "POINT" => "Point".to_string(),
        "LINESTRING" | "POLYLINE" => "LineString".to_string(),
        "POLYGON" => "Polygon".to_string(),
        "MULTIPOINT" => "MultiPoint".to_string(),
        "MULTILINESTRING" => "MultiLineString".to_string(),
        "MULTIPOLYGON" => "MultiPolygon".to_string(),
        "GEOMETRYCOLLECTION" => "GeometryCollection".to_string(),
        "GEOMETRY" => "Geometry".to_string(),
        _ => name.to_string(),
    }
}

// ─── Shapefile ───────────────────────────────────────────────────────────────

#[cfg(feature = "shapefile")]
fn shapefile_layers(path: &str) -> Result<Vec<Layer>> {
    let reader = open_shapefile(path)?;
    let header = reader.header();

    let geometry_type = shapefile_geometry_type(header.shape_type);
    let bounds = BoundingBox::new(
        header.bbox.x_min,
        header.bbox.y_min,
        header.bbox.x_max,
        header.bbox.y_max,
    )
    .ok();

    Ok(vec![Layer {
        path: path.to_string(),
        format: DatasetFormat::Shapefile,
        name: file_stem_name(path),
        index: 0,
        geometry_type,
        feature_count: reader.index_entries().map(|entries| entries.len() as u64),
        crs: reader.crs().map(str::to_string),
        bounds,
        field_names: reader
            .field_descriptors()
            .iter()
            .map(|descriptor| descriptor.name.clone())
            .collect(),
    }])
}

#[cfg(not(feature = "shapefile"))]
fn shapefile_layers(_path: &str) -> Result<Vec<Layer>> {
    Err(driver_disabled("ESRI Shapefile", "shapefile"))
}

#[cfg(feature = "shapefile")]
fn shapefile_features(path: &str) -> Result<Vec<Feature>> {
    use oxigeo_core::vector::FeatureId;

    let reader = open_shapefile(path)?;
    let records = reader.read_features().map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!("cannot read Shapefile features from '{path}': {e}"),
        })
    })?;

    Ok(records
        .into_iter()
        .map(|record| Feature {
            id: Some(FeatureId::Integer(i64::from(record.record_number))),
            geometry: record.geometry,
            properties: record.attributes,
        })
        .collect())
}

#[cfg(not(feature = "shapefile"))]
fn shapefile_features(_path: &str) -> Result<Vec<Feature>> {
    Err(driver_disabled("ESRI Shapefile", "shapefile"))
}

/// Open the Shapefile triplet (`.shp` / `.dbf` / `.shx`) behind `path`.
#[cfg(feature = "shapefile")]
fn open_shapefile(path: &str) -> Result<oxigeo_shapefile::ShapefileReader> {
    // `ShapefileReader::open` takes the base path shared by the sidecar files.
    let base = std::path::Path::new(path).with_extension("");
    oxigeo_shapefile::ShapefileReader::open(&base).map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!("cannot open Shapefile '{path}': {e}"),
        })
    })
}

/// Map a Shapefile header shape type to an OGC geometry-type name.
#[cfg(feature = "shapefile")]
fn shapefile_geometry_type(shape_type: oxigeo_shapefile::shp::shapes::ShapeType) -> Option<String> {
    use oxigeo_shapefile::shp::shapes::ShapeType;

    let name = match shape_type {
        ShapeType::Null => return None,
        ShapeType::Point | ShapeType::PointZ | ShapeType::PointM => "Point",
        ShapeType::PolyLine | ShapeType::PolyLineZ | ShapeType::PolyLineM => "PolyLine",
        ShapeType::Polygon | ShapeType::PolygonZ | ShapeType::PolygonM => "Polygon",
        ShapeType::MultiPoint | ShapeType::MultiPointZ | ShapeType::MultiPointM => "MultiPoint",
        ShapeType::MultiPatch => "MultiPatch",
    };
    Some(canonical_geometry_type(name))
}

// ─── GeoJSON ─────────────────────────────────────────────────────────────────

#[cfg(feature = "geojson")]
fn geojson_layers(path: &str) -> Result<Vec<Layer>> {
    let features = read_geojson_features(path)?;

    // Field names: the union of every feature's property keys, in first-seen
    // order — GeoJSON has no schema, so this is the closest honest answer.
    let mut field_names: Vec<String> = Vec::new();
    for feature in &features {
        for key in feature.properties.keys() {
            if !field_names.iter().any(|existing| existing == key) {
                field_names.push(key.clone());
            }
        }
    }
    field_names.sort_unstable();

    // A single geometry type is only declared when every feature agrees on one.
    let mut geometry_type: Option<String> = None;
    let mut mixed = false;
    for feature in &features {
        if let Some(geometry) = feature.geometry.as_ref() {
            let name = geometry_type_name(geometry).to_string();
            match &geometry_type {
                None => geometry_type = Some(name),
                Some(existing) if *existing != name => mixed = true,
                Some(_) => {}
            }
        }
    }
    if mixed {
        geometry_type = None;
    }

    let bounds = features
        .iter()
        .filter_map(Feature::bounds)
        .fold(None, |accumulator: Option<(f64, f64, f64, f64)>, bbox| {
            Some(match accumulator {
                None => bbox,
                Some(current) => (
                    current.0.min(bbox.0),
                    current.1.min(bbox.1),
                    current.2.max(bbox.2),
                    current.3.max(bbox.3),
                ),
            })
        })
        .and_then(|(min_x, min_y, max_x, max_y)| BoundingBox::new(min_x, min_y, max_x, max_y).ok());

    Ok(vec![Layer {
        path: path.to_string(),
        format: DatasetFormat::GeoJson,
        name: file_stem_name(path),
        index: 0,
        geometry_type,
        feature_count: Some(features.len() as u64),
        // RFC 7946 fixes the CRS of every GeoJSON document at WGS 84.
        crs: Some("EPSG:4326".to_string()),
        bounds,
        field_names,
    }])
}

#[cfg(not(feature = "geojson"))]
fn geojson_layers(_path: &str) -> Result<Vec<Layer>> {
    Err(driver_disabled("GeoJSON", "geojson"))
}

#[cfg(feature = "geojson")]
fn geojson_features(path: &str) -> Result<Vec<Feature>> {
    read_geojson_features(path)
}

#[cfg(not(feature = "geojson"))]
fn geojson_features(_path: &str) -> Result<Vec<Feature>> {
    Err(driver_disabled("GeoJSON", "geojson"))
}

/// Read a GeoJSON document — `FeatureCollection`, bare `Feature` or bare
/// geometry — as core features.
#[cfg(feature = "geojson")]
fn read_geojson_features(path: &str) -> Result<Vec<Feature>> {
    use oxigeo_core::error::IoError;
    use oxigeo_core::vector::FeatureId;
    use oxigeo_geojson::GeoJsonReader;
    use oxigeo_geojson::reader::GeoJsonDocument;

    let file = std::fs::File::open(path).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot open GeoJSON '{path}': {e}"),
        })
    })?;

    let mut reader = GeoJsonReader::without_validation(std::io::BufReader::new(file));
    let document = reader.read().map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!("cannot parse GeoJSON '{path}': {e}"),
        })
    })?;

    let source = match document {
        GeoJsonDocument::FeatureCollection(collection) => collection.features,
        GeoJsonDocument::Feature(feature) => vec![feature],
        GeoJsonDocument::Geometry(geometry) => {
            let converted = geojson_geometry_to_core(&geometry)?;
            return Ok(vec![Feature {
                id: None,
                geometry: Some(converted),
                properties: std::collections::HashMap::new(),
            }]);
        }
    };

    source
        .into_iter()
        .map(|feature| {
            let geometry = feature
                .geometry
                .as_ref()
                .map(geojson_geometry_to_core)
                .transpose()?;

            let properties = feature
                .properties
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, json_to_field_value(&value)))
                .collect();

            let id = feature.id.map(|id| match id {
                oxigeo_geojson::types::FeatureId::Number(n) => FeatureId::Integer(n),
                oxigeo_geojson::types::FeatureId::String(s) => FeatureId::String(s),
            });

            Ok(Feature {
                id,
                geometry,
                properties,
            })
        })
        .collect()
}

/// Convert a `serde_json` value to a core [`FieldValue`].
#[cfg(feature = "geojson")]
fn json_to_field_value(value: &serde_json::Value) -> oxigeo_core::vector::FieldValue {
    use oxigeo_core::vector::FieldValue;
    use serde_json::Value;

    match value {
        Value::Null => FieldValue::Null,
        Value::Bool(b) => FieldValue::Bool(*b),
        Value::Number(n) => n.as_i64().map_or_else(
            || n.as_f64().map_or(FieldValue::Null, FieldValue::Float),
            FieldValue::Integer,
        ),
        Value::String(s) => FieldValue::String(s.clone()),
        Value::Array(items) => FieldValue::Array(items.iter().map(json_to_field_value).collect()),
        Value::Object(map) => FieldValue::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), json_to_field_value(item)))
                .collect(),
        ),
    }
}

/// Convert a GeoJSON geometry to the core geometry model.
#[cfg(feature = "geojson")]
fn geojson_geometry_to_core(
    geometry: &oxigeo_geojson::Geometry,
) -> Result<oxigeo_core::vector::Geometry> {
    use oxigeo_core::vector::{
        Coordinate, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
    };
    use oxigeo_geojson::Geometry as GeoJsonGeometry;

    /// A GeoJSON position is `[x, y]` or `[x, y, z]`.
    fn position(coordinates: &[f64]) -> Result<Coordinate> {
        match coordinates {
            [x, y] => Ok(Coordinate::new_2d(*x, *y)),
            [x, y, z, ..] => Ok(Coordinate::new_3d(*x, *y, *z)),
            _ => Err(OxiGeoError::InvalidParameter {
                parameter: "coordinates",
                message: format!(
                    "GeoJSON position needs at least 2 ordinates, got {}",
                    coordinates.len()
                ),
            }),
        }
    }

    fn ring(coordinates: &[Vec<f64>]) -> Result<LineString> {
        let coords = coordinates
            .iter()
            .map(|p| position(p))
            .collect::<Result<Vec<_>>>()?;
        LineString::new(coords)
    }

    fn polygon(rings: &[Vec<Vec<f64>>]) -> Result<Polygon> {
        let mut converted = rings.iter().map(|r| ring(r));
        let exterior =
            converted
                .next()
                .transpose()?
                .ok_or_else(|| OxiGeoError::InvalidParameter {
                    parameter: "coordinates",
                    message: "GeoJSON Polygon has no exterior ring".to_string(),
                })?;
        let interiors = converted.collect::<Result<Vec<_>>>()?;
        Polygon::new(exterior, interiors)
    }

    Ok(match geometry {
        GeoJsonGeometry::Point(p) => Geometry::Point(Point::from_coord(position(&p.coordinates)?)),
        GeoJsonGeometry::LineString(ls) => Geometry::LineString(ring(&ls.coordinates)?),
        GeoJsonGeometry::Polygon(p) => Geometry::Polygon(polygon(&p.coordinates)?),
        GeoJsonGeometry::MultiPoint(mp) => Geometry::MultiPoint(MultiPoint::new(
            mp.coordinates
                .iter()
                .map(|p| position(p).map(Point::from_coord))
                .collect::<Result<Vec<_>>>()?,
        )),
        GeoJsonGeometry::MultiLineString(mls) => Geometry::MultiLineString(MultiLineString::new(
            mls.coordinates
                .iter()
                .map(|line| ring(line))
                .collect::<Result<Vec<_>>>()?,
        )),
        GeoJsonGeometry::MultiPolygon(mp) => Geometry::MultiPolygon(MultiPolygon::new(
            mp.coordinates
                .iter()
                .map(|rings| polygon(rings))
                .collect::<Result<Vec<_>>>()?,
        )),
        GeoJsonGeometry::GeometryCollection(gc) => {
            Geometry::GeometryCollection(oxigeo_core::vector::GeometryCollection::new(
                gc.geometries
                    .iter()
                    .map(geojson_geometry_to_core)
                    .collect::<Result<Vec<_>>>()?,
            ))
        }
    })
}

/// OGC type name of a core geometry.
#[cfg(feature = "geojson")]
fn geometry_type_name(geometry: &oxigeo_core::vector::Geometry) -> &'static str {
    use oxigeo_core::vector::Geometry;

    match geometry {
        Geometry::Point(_) => "Point",
        Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
    }
}

// ─── GeoPackage ──────────────────────────────────────────────────────────────

#[cfg(feature = "gpkg")]
fn gpkg_layers(path: &str) -> Result<Vec<Layer>> {
    use oxigeo_gpkg::GpkgDataType;

    let mut gpkg = crate::open::open_geopackage(path)?;
    gpkg.load_contents().map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!("cannot read gpkg_contents of '{path}': {e}"),
        })
    })?;

    let feature_tables: Vec<oxigeo_gpkg::GpkgContents> = gpkg
        .contents
        .iter()
        .filter(|entry| entry.data_type == GpkgDataType::Features)
        .cloned()
        .collect();

    let mut layers = Vec::with_capacity(feature_tables.len());
    for (index, entry) in feature_tables.into_iter().enumerate() {
        let schema = crate::gpkg_schema::TableSchema::load(&gpkg, &entry.table_name);
        let geometry_column = crate::gpkg_schema::geometry_column_name(&gpkg, &entry.table_name);

        let field_names = schema
            .columns()
            .iter()
            .filter(|column| {
                geometry_column
                    .as_ref()
                    .is_none_or(|geom| !column.name.eq_ignore_ascii_case(geom))
            })
            .map(|column| column.name.clone())
            .collect();

        let feature_count = gpkg.count_table_rows(&entry.table_name).map_err(|e| {
            OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
                message: format!(
                    "cannot count rows of feature table '{}' in '{path}': {e}",
                    entry.table_name
                ),
            })
        })?;

        layers.push(Layer {
            path: path.to_string(),
            format: DatasetFormat::GeoPackage,
            name: entry.table_name.clone(),
            index,
            geometry_type: crate::gpkg_schema::geometry_type_name(&gpkg, &entry.table_name)
                .as_deref()
                .map(canonical_geometry_type),
            feature_count,
            crs: gpkg_crs(entry.srs_id),
            bounds: BoundingBox::new(entry.min_x, entry.min_y, entry.max_x, entry.max_y).ok(),
            field_names,
        });
    }

    Ok(layers)
}

#[cfg(not(feature = "gpkg"))]
fn gpkg_layers(_path: &str) -> Result<Vec<Layer>> {
    Err(driver_disabled("GeoPackage", "gpkg"))
}

#[cfg(feature = "gpkg")]
fn gpkg_features(path: &str, table_name: &str) -> Result<Vec<Feature>> {
    use oxigeo_core::vector::{FeatureId, FieldValue};
    use oxigeo_gpkg::GpkgBinaryParser;

    use crate::gpkg_schema::{ColumnValue, TableSchema, geometry_column_name};

    let gpkg = crate::open::open_geopackage(path)?;
    let schema = TableSchema::load(&gpkg, table_name);
    let geometry_column = geometry_column_name(&gpkg, table_name);
    let geometry_index = geometry_column
        .as_deref()
        .and_then(|name| schema.index_of(name));

    let rows = gpkg
        .scan_table_by_name(table_name)
        .map_err(|e| {
            OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
                message: format!("cannot scan feature table '{table_name}' in '{path}': {e}"),
            })
        })?
        .ok_or_else(|| OxiGeoError::InvalidParameter {
            parameter: "name",
            message: format!("'{path}' has no table named '{table_name}'"),
        })?;

    let mut features = Vec::with_capacity(rows.len());
    for (rowid, cells) in rows {
        let mut geometry = None;
        if let Some(index) = geometry_index
            && let ColumnValue::Cell(oxigeo_gpkg::CellValue::Blob(blob)) =
                schema.resolve(index, &cells, rowid)
        {
            let parsed = GpkgBinaryParser::parse(blob).map_err(|e| {
                OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
                    message: format!(
                        "cannot decode geometry of feature {rowid} in '{table_name}' \
                         of '{path}': {e}"
                    ),
                })
            })?;
            geometry = gpkg_geometry_to_core(&parsed)?;
        }

        let mut properties = std::collections::HashMap::new();
        for (index, column) in schema.columns().iter().enumerate() {
            if Some(index) == geometry_index {
                continue;
            }
            let value = match schema.resolve(index, &cells, rowid) {
                ColumnValue::Cell(cell) => cell_to_field_value(cell),
                ColumnValue::RowId(id) => FieldValue::Integer(id),
                ColumnValue::Missing => FieldValue::Null,
            };
            properties.insert(column.name.clone(), value);
        }

        features.push(Feature {
            id: Some(FeatureId::Integer(rowid)),
            geometry,
            properties,
        });
    }

    Ok(features)
}

#[cfg(not(feature = "gpkg"))]
fn gpkg_features(_path: &str, _table_name: &str) -> Result<Vec<Feature>> {
    Err(driver_disabled("GeoPackage", "gpkg"))
}

/// CRS string for a `gpkg_contents.srs_id`.
///
/// `0` (undefined geographic) and `-1` (undefined cartesian) are the two
/// OGC-mandated "no CRS" sentinels, so they map to `None` rather than to a
/// nonsensical `EPSG:0`.
#[cfg(feature = "gpkg")]
fn gpkg_crs(srs_id: i32) -> Option<String> {
    (srs_id > 0).then(|| format!("EPSG:{srs_id}"))
}

/// Convert a SQLite cell to a core [`FieldValue`].
#[cfg(feature = "gpkg")]
fn cell_to_field_value(cell: &oxigeo_gpkg::CellValue) -> oxigeo_core::vector::FieldValue {
    use oxigeo_core::vector::FieldValue;
    use oxigeo_gpkg::CellValue;

    match cell {
        CellValue::Null => FieldValue::Null,
        CellValue::Integer(i) => FieldValue::Integer(*i),
        CellValue::Float(f) => FieldValue::Float(*f),
        CellValue::Text(s) => FieldValue::String(s.clone()),
        CellValue::Blob(b) => FieldValue::Blob(b.clone()),
    }
}

/// Convert a decoded GeoPackage geometry to the core geometry model.
///
/// Returns `Ok(None)` for [`oxigeo_gpkg::GpkgGeometry::Empty`], which has no
/// counterpart in the core model — an empty geometry becomes a feature without
/// one.  Z and M ordinates are preserved on the coordinates.
#[cfg(feature = "gpkg")]
fn gpkg_geometry_to_core(
    geometry: &oxigeo_gpkg::GpkgGeometry,
) -> Result<Option<oxigeo_core::vector::Geometry>> {
    use oxigeo_core::vector::{
        Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
        MultiPolygon, Point, Polygon,
    };
    use oxigeo_gpkg::{GpkgGeometry as G, Point4D};

    fn line(coords: Vec<Coordinate>) -> Result<LineString> {
        LineString::new(coords)
    }

    fn polygon_from(rings: Vec<Vec<Coordinate>>) -> Result<Polygon> {
        let mut iter = rings.into_iter();
        let exterior = iter
            .next()
            .ok_or_else(|| OxiGeoError::InvalidParameter {
                parameter: "rings",
                message: "GeoPackage Polygon has no exterior ring".to_string(),
            })
            .and_then(line)?;
        let interiors = iter.map(line).collect::<Result<Vec<_>>>()?;
        Polygon::new(exterior, interiors)
    }

    let xy = |&(x, y): &(f64, f64)| Coordinate::new_2d(x, y);
    let xyz = |&(x, y, z): &(f64, f64, f64)| Coordinate::new_3d(x, y, z);
    let xym = |&(x, y, m): &(f64, f64, f64)| Coordinate::new_2dm(x, y, m);
    let zm = |p: &Point4D| match (p.z, p.m) {
        (Some(z), Some(m)) => Coordinate::new_3dm(p.x, p.y, z, m),
        (Some(z), None) => Coordinate::new_3d(p.x, p.y, z),
        (None, Some(m)) => Coordinate::new_2dm(p.x, p.y, m),
        (None, None) => Coordinate::new_2d(p.x, p.y),
    };

    /// Build a `MultiPoint` from already-converted coordinates.
    fn multi_point(coords: Vec<Coordinate>) -> Geometry {
        Geometry::MultiPoint(MultiPoint::new(
            coords.into_iter().map(Point::from_coord).collect(),
        ))
    }

    let converted = match geometry {
        G::Empty => return Ok(None),

        // ── XY ───────────────────────────────────────────────────────────────
        G::Point { x, y } => Geometry::Point(Point::new(*x, *y)),
        G::LineString { coords } => Geometry::LineString(line(coords.iter().map(xy).collect())?),
        G::Polygon { rings } => Geometry::Polygon(polygon_from(
            rings.iter().map(|r| r.iter().map(xy).collect()).collect(),
        )?),
        G::MultiPoint { points } => multi_point(points.iter().map(xy).collect()),
        G::MultiLineString { lines } => Geometry::MultiLineString(MultiLineString::new(
            lines
                .iter()
                .map(|l| line(l.iter().map(xy).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),
        G::MultiPolygon { polygons } => Geometry::MultiPolygon(MultiPolygon::new(
            polygons
                .iter()
                .map(|p| polygon_from(p.iter().map(|r| r.iter().map(xy).collect()).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),

        // ── XYZ ──────────────────────────────────────────────────────────────
        G::PointZ { x, y, z } => Geometry::Point(Point::new_3d(*x, *y, *z)),
        G::LineStringZ { coords } => Geometry::LineString(line(coords.iter().map(xyz).collect())?),
        G::PolygonZ { rings } => Geometry::Polygon(polygon_from(
            rings.iter().map(|r| r.iter().map(xyz).collect()).collect(),
        )?),
        G::MultiPointZ { points } => multi_point(points.iter().map(xyz).collect()),
        G::MultiLineStringZ { lines } => Geometry::MultiLineString(MultiLineString::new(
            lines
                .iter()
                .map(|l| line(l.iter().map(xyz).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),
        G::MultiPolygonZ { polygons } => Geometry::MultiPolygon(MultiPolygon::new(
            polygons
                .iter()
                .map(|p| polygon_from(p.iter().map(|r| r.iter().map(xyz).collect()).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),

        // ── XYM ──────────────────────────────────────────────────────────────
        G::PointM { x, y, m } => {
            Geometry::Point(Point::from_coord(Coordinate::new_2dm(*x, *y, *m)))
        }
        G::LineStringM { coords } => Geometry::LineString(line(coords.iter().map(xym).collect())?),
        G::PolygonM { rings } => Geometry::Polygon(polygon_from(
            rings.iter().map(|r| r.iter().map(xym).collect()).collect(),
        )?),
        G::MultiPointM { points } => multi_point(points.iter().map(xym).collect()),
        G::MultiLineStringM { lines } => Geometry::MultiLineString(MultiLineString::new(
            lines
                .iter()
                .map(|l| line(l.iter().map(xym).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),
        G::MultiPolygonM { polygons } => Geometry::MultiPolygon(MultiPolygon::new(
            polygons
                .iter()
                .map(|p| polygon_from(p.iter().map(|r| r.iter().map(xym).collect()).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),

        // ── XYZM ─────────────────────────────────────────────────────────────
        G::PointZM(p) => Geometry::Point(Point::from_coord(zm(p))),
        G::LineStringZM { coords } => Geometry::LineString(line(coords.iter().map(zm).collect())?),
        G::PolygonZM { rings } => Geometry::Polygon(polygon_from(
            rings.iter().map(|r| r.iter().map(zm).collect()).collect(),
        )?),
        G::MultiPointZM { points } => multi_point(points.iter().map(zm).collect()),
        G::MultiLineStringZM { lines } => Geometry::MultiLineString(MultiLineString::new(
            lines
                .iter()
                .map(|l| line(l.iter().map(zm).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),
        G::MultiPolygonZM { polygons } => Geometry::MultiPolygon(MultiPolygon::new(
            polygons
                .iter()
                .map(|p| polygon_from(p.iter().map(|r| r.iter().map(zm).collect()).collect()))
                .collect::<Result<Vec<_>>>()?,
        )),

        // ── Collections ──────────────────────────────────────────────────────
        G::GeometryCollection(parts)
        | G::GeometryCollectionZ(parts)
        | G::GeometryCollectionM(parts)
        | G::GeometryCollectionZM(parts) => Geometry::GeometryCollection(GeometryCollection::new(
            parts
                .iter()
                .map(gpkg_geometry_to_core)
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect(),
        )),
    };

    Ok(Some(converted))
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(any(feature = "shapefile", feature = "gpkg"))]
    #[test]
    fn canonicalises_driver_geometry_names() {
        assert_eq!(canonical_geometry_type("MULTIPOLYGON"), "MultiPolygon");
        assert_eq!(canonical_geometry_type("point"), "Point");
        assert_eq!(canonical_geometry_type("PolyLine"), "LineString");
        assert_eq!(canonical_geometry_type("CIRCULARSTRING"), "CIRCULARSTRING");
    }

    #[cfg(any(feature = "shapefile", feature = "geojson"))]
    #[test]
    fn layer_name_falls_back_to_file_stem() {
        assert_eq!(file_stem_name("/data/roads.shp"), "roads");
        assert_eq!(file_stem_name("cities.geojson"), "cities");
    }

    #[cfg(feature = "gpkg")]
    #[test]
    fn undefined_srs_ids_have_no_crs() {
        assert_eq!(gpkg_crs(4326).as_deref(), Some("EPSG:4326"));
        assert_eq!(gpkg_crs(0), None);
        assert_eq!(gpkg_crs(-1), None);
    }
}
