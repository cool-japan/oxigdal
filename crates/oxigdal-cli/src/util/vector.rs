//! Vector format conversion utilities
//!
//! Provides format-to-format vector conversion with optional attribute filtering.
//! Supports GeoJSON, Shapefile, and FlatGeobuf as input and output formats.

use anyhow::{Context, Result, anyhow};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

// ─── Public enums / structs ───────────────────────────────────────────────────

/// Supported vector output formats for CLI conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorFormat {
    /// GeoJSON format (`.geojson`, `.json`).
    GeoJson,
    /// ESRI Shapefile format (`.shp`).
    Shapefile,
    /// FlatGeobuf format (`.fgb`).
    FlatGeobuf,
}

impl VectorFormat {
    /// Detect format from file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(|e| match e.to_lowercase().as_str() {
                "geojson" | "json" => Some(Self::GeoJson),
                "shp" => Some(Self::Shapefile),
                "fgb" => Some(Self::FlatGeobuf),
                _ => None,
            })
    }
}

/// Attribute filter operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterOp {
    /// Exact string equality (case-insensitive).
    Eq,
    /// Inequality (case-insensitive).
    Ne,
    /// Substring containment (case-insensitive).
    Contains,
}

/// Attribute filter: matches a named field against a value using an operator.
#[derive(Debug, Clone)]
pub struct AttributeFilter {
    /// The name of the feature attribute field to filter on.
    pub field: String,
    /// The comparison operator to apply.
    pub op: FilterOp,
    /// The value to compare against.
    pub value: String,
}

impl AttributeFilter {
    /// Test whether a JSON properties map matches this filter.
    pub fn matches_json(&self, props: &serde_json::Map<String, serde_json::Value>) -> bool {
        match props.get(&self.field) {
            None => false,
            Some(v) => {
                let candidate = json_value_to_string(v).to_lowercase();
                let target = self.value.to_lowercase();
                match self.op {
                    FilterOp::Eq => candidate == target,
                    FilterOp::Ne => candidate != target,
                    FilterOp::Contains => candidate.contains(&target),
                }
            }
        }
    }

    /// Test whether an `oxigdal_core::vector::FieldValue` map matches this filter.
    pub fn matches_field_map(
        &self,
        props: &std::collections::HashMap<String, oxigdal_core::vector::FieldValue>,
    ) -> bool {
        match props.get(&self.field) {
            None => false,
            Some(v) => {
                let candidate = field_value_to_string(v).to_lowercase();
                let target = self.value.to_lowercase();
                match self.op {
                    FilterOp::Eq => candidate == target,
                    FilterOp::Ne => candidate != target,
                    FilterOp::Contains => candidate.contains(&target),
                }
            }
        }
    }
}

// ─── Helper: stringify values ─────────────────────────────────────────────────

fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => format!("{a:?}"),
        serde_json::Value::Object(o) => format!("{o:?}"),
    }
}

fn field_value_to_string(v: &oxigdal_core::vector::FieldValue) -> String {
    use oxigdal_core::vector::FieldValue;
    match v {
        FieldValue::Null => String::new(),
        FieldValue::Bool(b) => b.to_string(),
        FieldValue::Integer(i) => i.to_string(),
        FieldValue::UInteger(u) => u.to_string(),
        FieldValue::Float(f) => f.to_string(),
        FieldValue::String(s) => s.clone(),
        FieldValue::Array(a) => format!("{a:?}"),
        FieldValue::Object(o) => format!("{o:?}"),
        FieldValue::Date(d) => d.to_string(),
        FieldValue::Blob(b) => format!("{b:?}"),
    }
}

// ─── GeoJSON Geometry ↔ Core Geometry converters ─────────────────────────────

/// Convert a GeoJSON geometry (`oxigdal_geojson::Geometry`) to a core geometry
/// (`oxigdal_core::vector::Geometry`).  Returns an error if coordinates are malformed.
pub fn geojson_geom_to_core(
    geom: &oxigdal_geojson::Geometry,
) -> Result<oxigdal_core::vector::Geometry> {
    use oxigdal_core::vector::{
        Geometry as CoreGeom, GeometryCollection as CoreGC, LineString as CoreLS,
        MultiLineString as CoreMLS, MultiPoint as CoreMP, MultiPolygon as CoreMPoly,
        Point as CorePoint, Polygon as CorePoly,
    };
    use oxigdal_geojson::Geometry as GjGeom;

    match geom {
        GjGeom::Point(p) => {
            let coord = pos_to_coord(&p.coordinates)?;
            Ok(CoreGeom::Point(CorePoint::from_coord(coord)))
        }
        GjGeom::LineString(ls) => {
            let coords = positions_to_coords(&ls.coordinates)?;
            let core_ls =
                CoreLS::new(coords).map_err(|e| anyhow!("LineString conversion error: {e}"))?;
            Ok(CoreGeom::LineString(core_ls))
        }
        GjGeom::Polygon(p) => {
            let rings = p
                .coordinates
                .iter()
                .map(|ring| positions_to_coords(ring))
                .collect::<Result<Vec<_>>>()?;
            let (exterior, interiors) = rings_to_exterior_interiors(rings)?;
            let core_poly = CorePoly::new(exterior, interiors)
                .map_err(|e| anyhow!("Polygon conversion error: {e}"))?;
            Ok(CoreGeom::Polygon(core_poly))
        }
        GjGeom::MultiPoint(mp) => {
            let points = mp
                .coordinates
                .iter()
                .map(|pos| {
                    let coord = pos_to_coord(pos)?;
                    Ok(CorePoint::from_coord(coord))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::MultiPoint(CoreMP::new(points)))
        }
        GjGeom::MultiLineString(mls) => {
            let lines = mls
                .coordinates
                .iter()
                .map(|ring| {
                    let coords = positions_to_coords(ring)?;
                    CoreLS::new(coords).map_err(|e| anyhow!("MultiLineString line error: {e}"))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::MultiLineString(CoreMLS {
                line_strings: lines,
            }))
        }
        GjGeom::MultiPolygon(mpoly) => {
            let polygons = mpoly
                .coordinates
                .iter()
                .map(|rings| {
                    let coord_rings = rings
                        .iter()
                        .map(|ring| positions_to_coords(ring))
                        .collect::<Result<Vec<_>>>()?;
                    let (exterior, interiors) = rings_to_exterior_interiors(coord_rings)?;
                    CorePoly::new(exterior, interiors)
                        .map_err(|e| anyhow!("MultiPolygon polygon error: {e}"))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::MultiPolygon(CoreMPoly { polygons }))
        }
        GjGeom::GeometryCollection(gc) => {
            let geoms = gc
                .geometries
                .iter()
                .map(geojson_geom_to_core)
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::GeometryCollection(CoreGC { geometries: geoms }))
        }
    }
}

/// Convert a core geometry to a GeoJSON geometry.
pub fn core_geom_to_geojson(
    geom: &oxigdal_core::vector::Geometry,
) -> Result<oxigdal_geojson::Geometry> {
    use oxigdal_core::vector::Geometry as CoreGeom;
    use oxigdal_geojson::types::{
        Geometry as GjGeom, GeometryCollection as GjGC, LineString as GjLS,
        MultiLineString as GjMLS, MultiPoint as GjMP, MultiPolygon as GjMPoly, Point as GjPoint,
        Polygon as GjPoly,
    };

    match geom {
        CoreGeom::Point(p) => {
            let pos = coord_to_pos(&p.coord);
            let gj_point = GjPoint::new(pos).map_err(|e| anyhow!("Point conversion error: {e}"))?;
            Ok(GjGeom::Point(gj_point))
        }
        CoreGeom::LineString(ls) => {
            let coords = ls.coords.iter().map(coord_to_pos).collect();
            let gj_ls =
                GjLS::new(coords).map_err(|e| anyhow!("LineString conversion error: {e}"))?;
            Ok(GjGeom::LineString(gj_ls))
        }
        CoreGeom::Polygon(p) => {
            let exterior: Vec<Vec<f64>> = p.exterior.coords.iter().map(coord_to_pos).collect();
            let mut rings = vec![exterior];
            for interior in &p.interiors {
                rings.push(interior.coords.iter().map(coord_to_pos).collect());
            }
            let gj_poly =
                GjPoly::new(rings).map_err(|e| anyhow!("Polygon conversion error: {e}"))?;
            Ok(GjGeom::Polygon(gj_poly))
        }
        CoreGeom::MultiPoint(mp) => {
            let positions = mp.points.iter().map(|p| coord_to_pos(&p.coord)).collect();
            let gj_mp = GjMP {
                coordinates: positions,
                bbox: None,
            };
            Ok(GjGeom::MultiPoint(gj_mp))
        }
        CoreGeom::MultiLineString(mls) => {
            let lines = mls
                .line_strings
                .iter()
                .map(|ls| ls.coords.iter().map(coord_to_pos).collect::<Vec<_>>())
                .collect();
            let gj_mls = GjMLS {
                coordinates: lines,
                bbox: None,
            };
            Ok(GjGeom::MultiLineString(gj_mls))
        }
        CoreGeom::MultiPolygon(mpoly) => {
            let polys = mpoly
                .polygons
                .iter()
                .map(|p| {
                    let ext: Vec<Vec<f64>> = p.exterior.coords.iter().map(coord_to_pos).collect();
                    let mut rings = vec![ext];
                    for interior in &p.interiors {
                        rings.push(interior.coords.iter().map(coord_to_pos).collect());
                    }
                    rings
                })
                .collect();
            let gj_mpoly = GjMPoly {
                coordinates: polys,
                bbox: None,
            };
            Ok(GjGeom::MultiPolygon(gj_mpoly))
        }
        CoreGeom::GeometryCollection(gc) => {
            let geoms = gc
                .geometries
                .iter()
                .map(core_geom_to_geojson)
                .collect::<Result<Vec<_>>>()?;
            let gj_gc = GjGC {
                geometries: geoms,
                bbox: None,
            };
            Ok(GjGeom::GeometryCollection(gj_gc))
        }
    }
}

// ─── Small coordinate helpers ─────────────────────────────────────────────────

fn pos_to_coord(pos: &[f64]) -> Result<oxigdal_core::vector::Coordinate> {
    if pos.len() < 2 {
        return Err(anyhow!(
            "position needs at least 2 elements, got {}",
            pos.len()
        ));
    }
    Ok(oxigdal_core::vector::Coordinate {
        x: pos[0],
        y: pos[1],
        z: pos.get(2).copied(),
        m: None,
    })
}

fn positions_to_coords(positions: &[Vec<f64>]) -> Result<Vec<oxigdal_core::vector::Coordinate>> {
    positions.iter().map(|p| pos_to_coord(p)).collect()
}

fn coord_to_pos(c: &oxigdal_core::vector::Coordinate) -> Vec<f64> {
    match c.z {
        Some(z) => vec![c.x, c.y, z],
        None => vec![c.x, c.y],
    }
}

/// Split a vec of rings into (exterior, interiors).
/// The exterior ring is the first ring; the rest are interior holes.
fn rings_to_exterior_interiors(
    mut rings: Vec<Vec<oxigdal_core::vector::Coordinate>>,
) -> Result<(
    oxigdal_core::vector::LineString,
    Vec<oxigdal_core::vector::LineString>,
)> {
    use oxigdal_core::vector::LineString as CoreLS;

    if rings.is_empty() {
        return Err(anyhow!("polygon has no rings"));
    }

    let exterior_coords = rings.remove(0);
    let exterior = CoreLS::new(exterior_coords).map_err(|e| anyhow!("exterior ring error: {e}"))?;

    let interiors = rings
        .into_iter()
        .map(|ring| CoreLS::new(ring).map_err(|e| anyhow!("interior ring error: {e}")))
        .collect::<Result<Vec<_>>>()?;

    Ok((exterior, interiors))
}

// ─── Shapefile schema inference ───────────────────────────────────────────────

/// Infer the `ShapeType` and `FieldDescriptor`s from a slice of Shapefile features.
/// Used when the input is GeoJSON and output is Shapefile.
pub fn infer_shapefile_schema_from_geojson(
    features: &[oxigdal_geojson::Feature],
) -> Result<(
    oxigdal_shapefile::shp::shapes::ShapeType,
    Vec<oxigdal_shapefile::dbf::FieldDescriptor>,
)> {
    use oxigdal_geojson::Geometry as GjGeom;
    use oxigdal_shapefile::{
        dbf::{FieldDescriptor, FieldType},
        shp::shapes::ShapeType,
    };

    // Determine shape type from first feature with geometry
    let shape_type = features
        .iter()
        .find_map(|f| f.geometry.as_ref())
        .map(|g| match g {
            GjGeom::Point(_) => ShapeType::Point,
            GjGeom::LineString(_) | GjGeom::MultiLineString(_) => ShapeType::PolyLine,
            GjGeom::Polygon(_) | GjGeom::MultiPolygon(_) => ShapeType::Polygon,
            GjGeom::MultiPoint(_) => ShapeType::MultiPoint,
            GjGeom::GeometryCollection(_) => ShapeType::Point, // fallback
        })
        .unwrap_or(ShapeType::Point);

    // Scan all features for property names and their max string width
    let mut field_widths: std::collections::HashMap<String, u8> = std::collections::HashMap::new();

    for feature in features {
        if let Some(props) = &feature.properties {
            for (key, value) in props {
                let key_short = truncate_field_name(key);
                let width = json_value_str_width(value);
                let entry = field_widths.entry(key_short).or_insert(0);
                if width > *entry {
                    *entry = width;
                }
            }
        }
    }

    let mut descriptors = Vec::new();
    for (name, width) in &field_widths {
        let length = width.clamp(&1, &254);
        let desc = FieldDescriptor::new(name.clone(), FieldType::Character, *length, 0)
            .with_context(|| format!("invalid field descriptor for '{name}'"))?;
        descriptors.push(desc);
    }

    // Sort deterministically
    descriptors.sort_by(|a, b| a.name.cmp(&b.name));

    Ok((shape_type, descriptors))
}

/// Infer the `ShapeType` and `FieldDescriptor`s from core vector features
/// (used for Shapefile-to-Shapefile passthrough is not needed here; used for
/// Shapefile→FGB schema inference via core).
pub fn infer_shapefile_schema_from_shapefiles(
    features: &[oxigdal_shapefile::reader::ShapefileFeature],
) -> Result<(
    oxigdal_shapefile::shp::shapes::ShapeType,
    Vec<oxigdal_shapefile::dbf::FieldDescriptor>,
)> {
    use oxigdal_core::vector::Geometry as CoreGeom;
    use oxigdal_shapefile::{
        dbf::{FieldDescriptor, FieldType},
        shp::shapes::ShapeType,
    };

    let shape_type = features
        .iter()
        .find_map(|f| f.geometry.as_ref())
        .map(|g| match g {
            CoreGeom::Point(_) => ShapeType::Point,
            CoreGeom::LineString(_) | CoreGeom::MultiLineString(_) => ShapeType::PolyLine,
            CoreGeom::Polygon(_) | CoreGeom::MultiPolygon(_) => ShapeType::Polygon,
            CoreGeom::MultiPoint(_) => ShapeType::MultiPoint,
            CoreGeom::GeometryCollection(_) => ShapeType::Point,
        })
        .unwrap_or(ShapeType::Point);

    let mut field_widths: std::collections::HashMap<String, u8> = std::collections::HashMap::new();

    for feature in features {
        for (key, value) in &feature.attributes {
            let key_short = truncate_field_name(key);
            let width = field_value_str_width(value);
            let entry = field_widths.entry(key_short).or_insert(0);
            if width > *entry {
                *entry = width;
            }
        }
    }

    let mut descriptors = Vec::new();
    for (name, width) in &field_widths {
        let length = width.clamp(&1, &254);
        let desc = FieldDescriptor::new(name.clone(), FieldType::Character, *length, 0)
            .with_context(|| format!("invalid field descriptor for '{name}'"))?;
        descriptors.push(desc);
    }

    descriptors.sort_by(|a, b| a.name.cmp(&b.name));

    Ok((shape_type, descriptors))
}

/// Truncate a field name to the 10-character DBF limit.
fn truncate_field_name(name: &str) -> String {
    name.chars().take(10).collect()
}

fn json_value_str_width(v: &serde_json::Value) -> u8 {
    let s = json_value_to_string(v);
    s.len().min(254) as u8
}

fn field_value_str_width(v: &oxigdal_core::vector::FieldValue) -> u8 {
    let s = field_value_to_string(v);
    s.len().min(254) as u8
}

// ─── GeoJSON features → Shapefile features ───────────────────────────────────

fn geojson_feature_to_shapefile(
    feature: &oxigdal_geojson::Feature,
    record_number: i32,
    field_names: &[String],
) -> Result<oxigdal_shapefile::reader::ShapefileFeature> {
    use oxigdal_core::vector::FieldValue;
    use oxigdal_shapefile::reader::ShapefileFeature;

    let geometry = match &feature.geometry {
        Some(gj_geom) => Some(geojson_geom_to_core(gj_geom)?),
        None => None,
    };

    let mut attributes = std::collections::HashMap::new();

    if let Some(props) = &feature.properties {
        for name in field_names {
            // find original key (may have been truncated to 10 chars)
            let original_key = props
                .keys()
                .find(|k| truncate_field_name(k) == *name)
                .cloned();

            if let Some(key) = original_key {
                let json_val = props.get(&key).cloned().unwrap_or(serde_json::Value::Null);
                let fv = json_to_field_value(&json_val);
                attributes.insert(name.clone(), fv);
            } else {
                attributes.insert(name.clone(), FieldValue::Null);
            }
        }
    } else {
        for name in field_names {
            attributes.insert(name.clone(), FieldValue::Null);
        }
    }

    Ok(ShapefileFeature::new(record_number, geometry, attributes))
}

fn json_to_field_value(v: &serde_json::Value) -> oxigdal_core::vector::FieldValue {
    use oxigdal_core::vector::FieldValue;
    match v {
        serde_json::Value::Null => FieldValue::Null,
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Integer(i)
            } else if let Some(u) = n.as_u64() {
                FieldValue::UInteger(u)
            } else {
                FieldValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        other => FieldValue::String(other.to_string()),
    }
}

// ─── Shapefile features → GeoJSON FeatureCollection ──────────────────────────

fn shapefile_feature_to_geojson(
    sf: &oxigdal_shapefile::reader::ShapefileFeature,
) -> Result<oxigdal_geojson::Feature> {
    use oxigdal_geojson::Feature as GjFeature;

    let geometry = match &sf.geometry {
        Some(core_geom) => Some(core_geom_to_geojson(core_geom)?),
        None => None,
    };

    let mut props = serde_json::Map::new();
    for (key, value) in &sf.attributes {
        props.insert(key.clone(), value.to_json_value());
    }

    Ok(GjFeature::new(geometry, Some(props)))
}

// ─── Main conversion entry point ─────────────────────────────────────────────

/// Convert a vector dataset between supported formats.
///
/// Returns the number of features written.
pub fn convert_vector(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    let input_fmt = VectorFormat::from_path(input)
        .ok_or_else(|| anyhow!("Unknown input vector format: {}", input.display()))?;
    let output_fmt = VectorFormat::from_path(output)
        .ok_or_else(|| anyhow!("Cannot determine output format from: {}", output.display()))?;

    match (input_fmt, output_fmt) {
        (VectorFormat::GeoJson, VectorFormat::GeoJson) => {
            convert_geojson_to_geojson(input, output, filter)
        }
        (VectorFormat::GeoJson, VectorFormat::Shapefile) => {
            convert_geojson_to_shapefile(input, output, filter)
        }
        (VectorFormat::Shapefile, VectorFormat::GeoJson) => {
            convert_shapefile_to_geojson(input, output, filter)
        }
        (VectorFormat::Shapefile, VectorFormat::Shapefile) => {
            convert_shapefile_to_shapefile(input, output, filter)
        }
        (VectorFormat::GeoJson, VectorFormat::FlatGeobuf) => {
            convert_geojson_to_fgb(input, output, filter)
        }
        (VectorFormat::Shapefile, VectorFormat::FlatGeobuf) => {
            convert_shapefile_to_fgb(input, output, filter)
        }
        (VectorFormat::FlatGeobuf, VectorFormat::GeoJson) => {
            convert_fgb_to_geojson(input, output, filter)
        }
        (VectorFormat::FlatGeobuf, VectorFormat::Shapefile) => {
            convert_fgb_to_shapefile(input, output, filter)
        }
        (VectorFormat::FlatGeobuf, VectorFormat::FlatGeobuf) => {
            convert_fgb_to_fgb(input, output, filter)
        }
    }
}

// ─── GeoJSON → GeoJSON ────────────────────────────────────────────────────────

fn convert_geojson_to_geojson(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_geojson::{FeatureCollection, GeoJsonReader, GeoJsonWriter};

    let file =
        File::open(input).with_context(|| format!("Failed to open input: {}", input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let fc = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON feature collection")?;

    let features: Vec<_> = fc
        .features
        .into_iter()
        .filter(|f| match filter {
            None => true,
            Some(filt) => {
                let props = f
                    .properties
                    .as_ref()
                    .map_or(serde_json::Map::new(), |p| p.clone());
                filt.matches_json(&props)
            }
        })
        .collect();

    let count = features.len();

    let out_fc = FeatureCollection::new(features);

    let out_file = File::create(output)
        .with_context(|| format!("Failed to create output: {}", output.display()))?;
    let buf_writer = BufWriter::new(out_file);
    let mut writer = GeoJsonWriter::pretty(buf_writer);

    writer
        .write_feature_collection(&out_fc)
        .context("Failed to write GeoJSON")?;

    Ok(count)
}

// ─── GeoJSON → Shapefile ──────────────────────────────────────────────────────

fn convert_geojson_to_shapefile(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_geojson::GeoJsonReader;
    use oxigdal_shapefile::ShapefileWriter;

    let file =
        File::open(input).with_context(|| format!("Failed to open input: {}", input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let fc = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON feature collection")?;

    let features: Vec<_> = fc
        .features
        .into_iter()
        .filter(|f| match filter {
            None => true,
            Some(filt) => {
                let props = f
                    .properties
                    .as_ref()
                    .map_or(serde_json::Map::new(), |p| p.clone());
                filt.matches_json(&props)
            }
        })
        .collect();

    if features.is_empty() {
        anyhow::bail!("No features remain after filtering; cannot write empty Shapefile");
    }

    let count = features.len();

    // Infer schema from filtered features
    let (shape_type, field_descriptors) = infer_shapefile_schema_from_geojson(&features)?;

    let field_names: Vec<String> = field_descriptors.iter().map(|d| d.name.clone()).collect();

    // Output base path (strip .shp extension)
    let base_path = output.with_extension("");

    let mut writer = ShapefileWriter::new(&base_path, shape_type, field_descriptors)
        .context("Failed to create Shapefile writer")?;

    // Convert features to ShapefileFeature
    let sf_features: Vec<_> = features
        .iter()
        .enumerate()
        .map(|(i, f)| geojson_feature_to_shapefile(f, (i + 1) as i32, &field_names))
        .collect::<Result<Vec<_>>>()?;

    writer
        .write_features(&sf_features)
        .context("Failed to write Shapefile")?;

    Ok(count)
}

// ─── Shapefile → GeoJSON ──────────────────────────────────────────────────────

fn convert_shapefile_to_geojson(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_geojson::{FeatureCollection, GeoJsonWriter};
    use oxigdal_shapefile::ShapefileReader;

    // Strip .shp extension for base path
    let base_path = input.with_extension("");

    let reader = ShapefileReader::open(&base_path)
        .with_context(|| format!("Failed to open Shapefile: {}", input.display()))?;

    let sf_features = reader
        .read_features()
        .context("Failed to read Shapefile features")?;

    let filtered: Vec<_> = sf_features
        .into_iter()
        .filter(|f| match filter {
            None => true,
            Some(filt) => filt.matches_field_map(&f.attributes),
        })
        .collect();

    let count = filtered.len();

    // Convert to GeoJSON features
    let gj_features = filtered
        .iter()
        .map(shapefile_feature_to_geojson)
        .collect::<Result<Vec<_>>>()?;

    let out_fc = FeatureCollection::new(gj_features);

    let out_file = File::create(output)
        .with_context(|| format!("Failed to create output: {}", output.display()))?;
    let buf_writer = BufWriter::new(out_file);
    let mut writer = GeoJsonWriter::pretty(buf_writer);

    writer
        .write_feature_collection(&out_fc)
        .context("Failed to write GeoJSON")?;

    Ok(count)
}

// ─── Shapefile → Shapefile ────────────────────────────────────────────────────

fn convert_shapefile_to_shapefile(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_shapefile::{ShapefileReader, ShapefileWriter};

    let base_in = input.with_extension("");
    let reader = ShapefileReader::open(&base_in)
        .with_context(|| format!("Failed to open Shapefile: {}", input.display()))?;

    let sf_features = reader
        .read_features()
        .context("Failed to read Shapefile features")?;

    let filtered: Vec<_> = sf_features
        .into_iter()
        .filter(|f| match filter {
            None => true,
            Some(filt) => filt.matches_field_map(&f.attributes),
        })
        .collect();

    if filtered.is_empty() {
        anyhow::bail!("No features remain after filtering; cannot write empty Shapefile");
    }

    let count = filtered.len();

    let (shape_type, field_descriptors) = infer_shapefile_schema_from_shapefiles(&filtered)?;

    let base_out = output.with_extension("");
    let mut writer = ShapefileWriter::new(&base_out, shape_type, field_descriptors)
        .context("Failed to create Shapefile writer")?;

    writer
        .write_features(&filtered)
        .context("Failed to write Shapefile")?;

    Ok(count)
}

// ─── GeoJSON → FlatGeobuf ─────────────────────────────────────────────────────

fn convert_geojson_to_fgb(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_flatgeobuf::{FlatGeobufWriterBuilder, GeometryType};
    use oxigdal_geojson::GeoJsonReader;

    let file =
        File::open(input).with_context(|| format!("Failed to open input: {}", input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let fc = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON feature collection")?;

    // Apply attribute filter
    let features: Vec<_> = fc
        .features
        .into_iter()
        .filter(|f| match filter {
            None => true,
            Some(filt) => {
                let props = f
                    .properties
                    .as_ref()
                    .map_or(serde_json::Map::new(), |p| p.clone());
                filt.matches_json(&props)
            }
        })
        .collect();

    let count = features.len();

    // Convert to core features, collecting schema info
    let core_features: Vec<oxigdal_core::vector::Feature> = features
        .iter()
        .map(geojson_driver_feature_to_core)
        .collect::<Result<Vec<_>>>()?;

    // Infer geometry type from first feature with geometry
    let fgb_geom_type = core_features
        .iter()
        .find_map(|f| f.geometry.as_ref())
        .map(core_geom_to_fgb_geometry_type)
        .unwrap_or(GeometryType::Unknown);

    // Collect column definitions from properties of all features
    let columns = infer_fgb_columns(&core_features);

    // Build the writer with index enabled
    let mut builder = FlatGeobufWriterBuilder::new(fgb_geom_type).with_index();
    for col in &columns {
        builder = builder.with_column(col.clone());
    }

    let out_file = File::create(output)
        .with_context(|| format!("Failed to create output: {}", output.display()))?;
    let buf_writer = BufWriter::new(out_file);
    let mut writer = builder
        .build(buf_writer)
        .context("Failed to create FlatGeobuf writer")?;

    for feature in &core_features {
        writer
            .add_feature(feature)
            .context("Failed to add feature to FlatGeobuf writer")?;
    }

    writer
        .finish()
        .context("Failed to finalise FlatGeobuf file")?;

    Ok(count)
}

// ─── Shapefile → FlatGeobuf ───────────────────────────────────────────────────

fn convert_shapefile_to_fgb(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_flatgeobuf::{FlatGeobufWriterBuilder, GeometryType};
    use oxigdal_shapefile::ShapefileReader;

    let base_path = input.with_extension("");
    let reader = ShapefileReader::open(&base_path)
        .with_context(|| format!("Failed to open Shapefile: {}", input.display()))?;

    let sf_features = reader
        .read_features()
        .context("Failed to read Shapefile features")?;

    let filtered: Vec<_> = sf_features
        .into_iter()
        .filter(|f| match filter {
            None => true,
            Some(filt) => filt.matches_field_map(&f.attributes),
        })
        .collect();

    let count = filtered.len();

    // Convert ShapefileFeatures to core Features
    let core_features: Vec<oxigdal_core::vector::Feature> = filtered
        .iter()
        .map(shapefile_feature_to_core)
        .collect::<Result<Vec<_>>>()?;

    let fgb_geom_type = core_features
        .iter()
        .find_map(|f| f.geometry.as_ref())
        .map(core_geom_to_fgb_geometry_type)
        .unwrap_or(GeometryType::Unknown);

    let columns = infer_fgb_columns(&core_features);

    let mut builder = FlatGeobufWriterBuilder::new(fgb_geom_type).with_index();
    for col in &columns {
        builder = builder.with_column(col.clone());
    }

    let out_file = File::create(output)
        .with_context(|| format!("Failed to create output: {}", output.display()))?;
    let buf_writer = BufWriter::new(out_file);
    let mut writer = builder
        .build(buf_writer)
        .context("Failed to create FlatGeobuf writer")?;

    for feature in &core_features {
        writer
            .add_feature(feature)
            .context("Failed to add feature to FlatGeobuf writer")?;
    }

    writer
        .finish()
        .context("Failed to finalise FlatGeobuf file")?;

    Ok(count)
}

// ─── FlatGeobuf → GeoJSON ─────────────────────────────────────────────────────

fn convert_fgb_to_geojson(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_flatgeobuf::FlatGeobufReader;
    use oxigdal_geojson::{Feature as GjFeature, FeatureCollection, GeoJsonWriter};

    let file =
        File::open(input).with_context(|| format!("Failed to open input: {}", input.display()))?;
    let mut reader = FlatGeobufReader::new(file)
        .with_context(|| format!("Failed to parse FlatGeobuf header: {}", input.display()))?;

    let mut gj_features: Vec<GjFeature> = Vec::new();

    let iter = reader
        .features()
        .context("Failed to get FlatGeobuf feature iterator")?;

    for result in iter {
        let core_feat = result.context("Failed to read FlatGeobuf feature")?;

        // Apply attribute filter using the core feature's properties
        if let Some(filt) = filter
            && !filt.matches_field_map(&core_feat.properties)
        {
            continue;
        }

        let gj_feat = core_feature_to_geojson_driver_feature(&core_feat)?;
        gj_features.push(gj_feat);
    }

    let count = gj_features.len();

    let out_fc = FeatureCollection::new(gj_features);

    let out_file = File::create(output)
        .with_context(|| format!("Failed to create output: {}", output.display()))?;
    let buf_writer = BufWriter::new(out_file);
    let mut writer = GeoJsonWriter::pretty(buf_writer);

    writer
        .write_feature_collection(&out_fc)
        .context("Failed to write GeoJSON")?;

    Ok(count)
}

// ─── FlatGeobuf → Shapefile ───────────────────────────────────────────────────

fn convert_fgb_to_shapefile(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_flatgeobuf::FlatGeobufReader;
    use oxigdal_shapefile::{ShapefileWriter, reader::ShapefileFeature};

    let file =
        File::open(input).with_context(|| format!("Failed to open input: {}", input.display()))?;
    let mut reader = FlatGeobufReader::new(file)
        .with_context(|| format!("Failed to parse FlatGeobuf header: {}", input.display()))?;

    let mut core_features: Vec<oxigdal_core::vector::Feature> = Vec::new();

    let iter = reader
        .features()
        .context("Failed to get FlatGeobuf feature iterator")?;

    for result in iter {
        let core_feat = result.context("Failed to read FlatGeobuf feature")?;
        if let Some(filt) = filter
            && !filt.matches_field_map(&core_feat.properties)
        {
            continue;
        }
        core_features.push(core_feat);
    }

    if core_features.is_empty() {
        anyhow::bail!("No features remain after filtering; cannot write empty Shapefile");
    }

    let count = core_features.len();

    // Convert to ShapefileFeature list for schema inference
    let sf_features: Vec<ShapefileFeature> = core_features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let attrs = f.properties.clone();
            ShapefileFeature::new((i + 1) as i32, f.geometry.clone(), attrs)
        })
        .collect();

    let (shape_type, field_descriptors) = infer_shapefile_schema_from_shapefiles(&sf_features)?;

    let base_out = output.with_extension("");
    let mut writer = ShapefileWriter::new(&base_out, shape_type, field_descriptors)
        .context("Failed to create Shapefile writer")?;

    writer
        .write_features(&sf_features)
        .context("Failed to write Shapefile")?;

    Ok(count)
}

// ─── FlatGeobuf → FlatGeobuf (identity copy) ─────────────────────────────────

fn convert_fgb_to_fgb(
    input: &Path,
    output: &Path,
    filter: Option<&AttributeFilter>,
) -> Result<usize> {
    use oxigdal_flatgeobuf::{FlatGeobufReader, FlatGeobufWriterBuilder};

    let file =
        File::open(input).with_context(|| format!("Failed to open input: {}", input.display()))?;
    let mut reader = FlatGeobufReader::new(file)
        .with_context(|| format!("Failed to parse FlatGeobuf header: {}", input.display()))?;

    let source_header = reader.header().clone();

    let mut core_features: Vec<oxigdal_core::vector::Feature> = Vec::new();

    let iter = reader
        .features()
        .context("Failed to get FlatGeobuf feature iterator")?;

    for result in iter {
        let core_feat = result.context("Failed to read FlatGeobuf feature")?;
        if let Some(filt) = filter
            && !filt.matches_field_map(&core_feat.properties)
        {
            continue;
        }
        core_features.push(core_feat);
    }

    let count = core_features.len();

    let mut builder = FlatGeobufWriterBuilder::new(source_header.geometry_type).with_index();
    for col in &source_header.columns {
        builder = builder.with_column(col.clone());
    }

    let out_file = File::create(output)
        .with_context(|| format!("Failed to create output: {}", output.display()))?;
    let buf_writer = BufWriter::new(out_file);
    let mut writer = builder
        .build(buf_writer)
        .context("Failed to create FlatGeobuf writer")?;

    for feature in &core_features {
        writer
            .add_feature(feature)
            .context("Failed to add feature to FlatGeobuf writer")?;
    }

    writer
        .finish()
        .context("Failed to finalise FlatGeobuf file")?;

    Ok(count)
}

// ─── FlatGeobuf schema / geometry helpers ────────────────────────────────────

/// Map a core `Geometry` variant to the corresponding FlatGeobuf `GeometryType`.
fn core_geom_to_fgb_geometry_type(
    geom: &oxigdal_core::vector::Geometry,
) -> oxigdal_flatgeobuf::GeometryType {
    use oxigdal_core::vector::Geometry as CoreGeom;
    use oxigdal_flatgeobuf::GeometryType as FgbGT;
    match geom {
        CoreGeom::Point(_) => FgbGT::Point,
        CoreGeom::LineString(_) => FgbGT::LineString,
        CoreGeom::Polygon(_) => FgbGT::Polygon,
        CoreGeom::MultiPoint(_) => FgbGT::MultiPoint,
        CoreGeom::MultiLineString(_) => FgbGT::MultiLineString,
        CoreGeom::MultiPolygon(_) => FgbGT::MultiPolygon,
        CoreGeom::GeometryCollection(_) => FgbGT::GeometryCollection,
    }
}

/// Infer FlatGeobuf column definitions from a slice of core features.
///
/// Scans all features' property maps and emits one `Column` per unique key.
/// All properties are stored as `ColumnType::String` because the FGB format
/// requires all values to be stored consistently across features.
fn infer_fgb_columns(
    features: &[oxigdal_core::vector::Feature],
) -> Vec<oxigdal_flatgeobuf::Column> {
    use oxigdal_core::vector::FieldValue;
    use oxigdal_flatgeobuf::{Column, ColumnType};

    // Collect unique column names (preserving first-seen insertion order)
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut ordered: Vec<String> = Vec::new();

    for feat in features {
        for key in feat.properties.keys() {
            if seen.insert(key.clone()) {
                ordered.push(key.clone());
            }
        }
    }

    // Choose column types based on the first non-null value for each key
    let mut col_types: std::collections::HashMap<String, ColumnType> =
        std::collections::HashMap::new();

    for feat in features {
        for (key, val) in &feat.properties {
            if col_types.contains_key(key) {
                continue;
            }
            let ct = match val {
                FieldValue::Null => continue,
                FieldValue::Bool(_) => ColumnType::Bool,
                FieldValue::Integer(_) => ColumnType::Long,
                FieldValue::UInteger(_) => ColumnType::ULong,
                FieldValue::Float(_) => ColumnType::Double,
                FieldValue::String(_) | FieldValue::Array(_) | FieldValue::Object(_) => {
                    ColumnType::String
                }
                FieldValue::Date(_) => ColumnType::DateTime,
                FieldValue::Blob(_) => ColumnType::Binary,
            };
            col_types.insert(key.clone(), ct);
        }
    }

    ordered
        .into_iter()
        .map(|name| {
            let ct = col_types.get(&name).copied().unwrap_or(ColumnType::String);
            Column::new(name, ct)
        })
        .collect()
}

/// Convert a GeoJSON driver `Feature` (from `oxigdal_geojson`) to a core `Feature`.
fn geojson_driver_feature_to_core(
    f: &oxigdal_geojson::Feature,
) -> Result<oxigdal_core::vector::Feature> {
    use oxigdal_core::vector::{Feature as CoreFeature, FieldValue};

    let geometry = match &f.geometry {
        Some(gj_geom) => Some(geojson_geom_to_core(gj_geom)?),
        None => None,
    };

    let mut core_feat = match geometry {
        Some(g) => CoreFeature::new(g),
        None => CoreFeature::new_attribute_only(),
    };

    if let Some(props) = &f.properties {
        for (k, v) in props {
            core_feat.set_property(k.clone(), FieldValue::from_json(v));
        }
    }

    Ok(core_feat)
}

/// Convert a core `Feature` (from `oxigdal_core`) to a GeoJSON driver `Feature`.
fn core_feature_to_geojson_driver_feature(
    core_feat: &oxigdal_core::vector::Feature,
) -> Result<oxigdal_geojson::Feature> {
    use oxigdal_geojson::Feature as GjFeature;

    let geometry = match &core_feat.geometry {
        Some(g) => Some(core_geom_to_geojson(g)?),
        None => None,
    };

    let properties: oxigdal_geojson::types::Properties = core_feat
        .properties
        .iter()
        .map(|(k, v)| (k.clone(), v.to_json_value()))
        .collect();

    let props_opt = if properties.is_empty() {
        None
    } else {
        Some(properties)
    };

    Ok(GjFeature::new(geometry, props_opt))
}

/// Convert a `ShapefileFeature` to a core `Feature` for FGB writing.
fn shapefile_feature_to_core(
    sf: &oxigdal_shapefile::reader::ShapefileFeature,
) -> Result<oxigdal_core::vector::Feature> {
    use oxigdal_core::vector::Feature as CoreFeature;

    let mut core_feat = match &sf.geometry {
        Some(g) => CoreFeature::new(g.clone()),
        None => CoreFeature::new_attribute_only(),
    };

    for (k, v) in &sf.attributes {
        core_feat.set_property(k.clone(), v.clone());
    }

    Ok(core_feat)
}
