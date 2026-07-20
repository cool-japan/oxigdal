//! Bidirectional conversion between GeoPackage types and `oxigeo-geojson-stream` types.
//!
//! # Functions
//!
//! - [`gpkg_geom_to_geojson`] — convert a [`GpkgGeometry`] to a [`GeoJsonGeometry`]
//! - [`geojson_geom_to_gpkg`] — convert a [`GeoJsonGeometry`] to a [`GpkgGeometry`]
//! - [`feature_table_to_geojson`] — convert a [`FeatureTable`] to a [`FeatureCollection`]
//! - [`feature_table_from_geojson`] — build a [`FeatureTable`] from a [`FeatureCollection`]

use std::collections::HashMap;

use oxigeo_geojson_stream::{FeatureCollection, FeatureId, GeoJsonFeature, GeoJsonGeometry};

use crate::error::GpkgError;
use crate::vector::feature::{FeatureRow, FeatureTable};
use crate::vector::types::{FieldDefinition, FieldType, FieldValue, GpkgGeometry};

// ─────────────────────────────────────────────────────────────────────────────
// GpkgGeometry → GeoJsonGeometry
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a [`GpkgGeometry`] to the equivalent [`GeoJsonGeometry`].
///
/// # Coordinate handling
/// - 2-D geometry variants map to their 2-D GeoJSON counterparts.
/// - Z variants map to the corresponding Z GeoJSON variant (`[x, y, z]`).
/// - M and ZM variants: M is dropped (GeoJSON RFC 7946 has no M dimension).
///   ZM maps to Z-only (M discarded). M-only maps to XY (M discarded).
/// - [`GpkgGeometry::Empty`] maps to [`GeoJsonGeometry::Null`].
/// - `GpkgGeometry::GeometryCollection*` maps to [`GeoJsonGeometry::GeometryCollection`]
///   with each sub-geometry recursively converted.
///
/// # Errors
///
/// Returns [`GpkgError::UnsupportedGeometry`] if an unrecognised geometry kind
/// is encountered (should not happen with a well-formed [`GpkgGeometry`]).
pub fn gpkg_geom_to_geojson(g: &GpkgGeometry) -> Result<GeoJsonGeometry, GpkgError> {
    match g {
        // ── 2-D ──────────────────────────────────────────────────────────────
        GpkgGeometry::Point { x, y } => Ok(GeoJsonGeometry::Point([*x, *y])),

        GpkgGeometry::LineString { coords } => {
            let pts: Vec<[f64; 2]> = coords.iter().map(|(x, y)| [*x, *y]).collect();
            Ok(GeoJsonGeometry::LineString(pts))
        }

        GpkgGeometry::Polygon { rings } => {
            let gj_rings: Vec<Vec<[f64; 2]>> = rings
                .iter()
                .map(|ring| ring.iter().map(|(x, y)| [*x, *y]).collect())
                .collect();
            Ok(GeoJsonGeometry::Polygon(gj_rings))
        }

        GpkgGeometry::MultiPoint { points } => {
            let pts: Vec<[f64; 2]> = points.iter().map(|(x, y)| [*x, *y]).collect();
            Ok(GeoJsonGeometry::MultiPoint(pts))
        }

        GpkgGeometry::MultiLineString { lines } => {
            let gj_lines: Vec<Vec<[f64; 2]>> = lines
                .iter()
                .map(|l| l.iter().map(|(x, y)| [*x, *y]).collect())
                .collect();
            Ok(GeoJsonGeometry::MultiLineString(gj_lines))
        }

        GpkgGeometry::MultiPolygon { polygons } => {
            let gj_polys: Vec<Vec<Vec<[f64; 2]>>> = polygons
                .iter()
                .map(|poly| {
                    poly.iter()
                        .map(|ring| ring.iter().map(|(x, y)| [*x, *y]).collect())
                        .collect()
                })
                .collect();
            Ok(GeoJsonGeometry::MultiPolygon(gj_polys))
        }

        // ── 3-D (Z) ──────────────────────────────────────────────────────────
        GpkgGeometry::PointZ { x, y, z } => Ok(GeoJsonGeometry::PointZ([*x, *y, *z])),

        GpkgGeometry::LineStringZ { coords } => {
            let pts: Vec<[f64; 3]> = coords.iter().map(|(x, y, z)| [*x, *y, *z]).collect();
            Ok(GeoJsonGeometry::LineStringZ(pts))
        }

        GpkgGeometry::PolygonZ { rings } => {
            let gj_rings: Vec<Vec<[f64; 3]>> = rings
                .iter()
                .map(|ring| ring.iter().map(|(x, y, z)| [*x, *y, *z]).collect())
                .collect();
            Ok(GeoJsonGeometry::PolygonZ(gj_rings))
        }

        GpkgGeometry::MultiPointZ { points } => {
            let pts: Vec<[f64; 3]> = points.iter().map(|(x, y, z)| [*x, *y, *z]).collect();
            Ok(GeoJsonGeometry::MultiPointZ(pts))
        }

        GpkgGeometry::MultiLineStringZ { lines } => {
            let gj_lines: Vec<Vec<[f64; 3]>> = lines
                .iter()
                .map(|l| l.iter().map(|(x, y, z)| [*x, *y, *z]).collect())
                .collect();
            Ok(GeoJsonGeometry::MultiLineStringZ(gj_lines))
        }

        GpkgGeometry::MultiPolygonZ { polygons } => {
            let gj_polys: Vec<Vec<Vec<[f64; 3]>>> = polygons
                .iter()
                .map(|poly| {
                    poly.iter()
                        .map(|ring| ring.iter().map(|(x, y, z)| [*x, *y, *z]).collect())
                        .collect()
                })
                .collect();
            Ok(GeoJsonGeometry::MultiPolygonZ(gj_polys))
        }

        // ── M variants — GeoJSON has no M; project to XY ─────────────────────
        GpkgGeometry::PointM { x, y, .. } => Ok(GeoJsonGeometry::Point([*x, *y])),

        GpkgGeometry::LineStringM { coords } => {
            let pts: Vec<[f64; 2]> = coords.iter().map(|(x, y, _m)| [*x, *y]).collect();
            Ok(GeoJsonGeometry::LineString(pts))
        }

        GpkgGeometry::PolygonM { rings } => {
            let gj_rings: Vec<Vec<[f64; 2]>> = rings
                .iter()
                .map(|ring| ring.iter().map(|(x, y, _m)| [*x, *y]).collect())
                .collect();
            Ok(GeoJsonGeometry::Polygon(gj_rings))
        }

        GpkgGeometry::MultiPointM { points } => {
            let pts: Vec<[f64; 2]> = points.iter().map(|(x, y, _m)| [*x, *y]).collect();
            Ok(GeoJsonGeometry::MultiPoint(pts))
        }

        GpkgGeometry::MultiLineStringM { lines } => {
            let gj_lines: Vec<Vec<[f64; 2]>> = lines
                .iter()
                .map(|l| l.iter().map(|(x, y, _m)| [*x, *y]).collect())
                .collect();
            Ok(GeoJsonGeometry::MultiLineString(gj_lines))
        }

        GpkgGeometry::MultiPolygonM { polygons } => {
            let gj_polys: Vec<Vec<Vec<[f64; 2]>>> = polygons
                .iter()
                .map(|poly| {
                    poly.iter()
                        .map(|ring| ring.iter().map(|(x, y, _m)| [*x, *y]).collect())
                        .collect()
                })
                .collect();
            Ok(GeoJsonGeometry::MultiPolygon(gj_polys))
        }

        // ── ZM variants — keep XYZ, discard M ────────────────────────────────
        GpkgGeometry::PointZM(p) => Ok(GeoJsonGeometry::PointZ([p.x, p.y, p.z.unwrap_or(0.0)])),

        GpkgGeometry::LineStringZM { coords } => {
            let pts: Vec<[f64; 3]> = coords
                .iter()
                .map(|p| [p.x, p.y, p.z.unwrap_or(0.0)])
                .collect();
            Ok(GeoJsonGeometry::LineStringZ(pts))
        }

        GpkgGeometry::PolygonZM { rings } => {
            let gj_rings: Vec<Vec<[f64; 3]>> = rings
                .iter()
                .map(|ring| {
                    ring.iter()
                        .map(|p| [p.x, p.y, p.z.unwrap_or(0.0)])
                        .collect()
                })
                .collect();
            Ok(GeoJsonGeometry::PolygonZ(gj_rings))
        }

        GpkgGeometry::MultiPointZM { points } => {
            let pts: Vec<[f64; 3]> = points
                .iter()
                .map(|p| [p.x, p.y, p.z.unwrap_or(0.0)])
                .collect();
            Ok(GeoJsonGeometry::MultiPointZ(pts))
        }

        GpkgGeometry::MultiLineStringZM { lines } => {
            let gj_lines: Vec<Vec<[f64; 3]>> = lines
                .iter()
                .map(|l| l.iter().map(|p| [p.x, p.y, p.z.unwrap_or(0.0)]).collect())
                .collect();
            Ok(GeoJsonGeometry::MultiLineStringZ(gj_lines))
        }

        GpkgGeometry::MultiPolygonZM { polygons } => {
            let gj_polys: Vec<Vec<Vec<[f64; 3]>>> = polygons
                .iter()
                .map(|poly| {
                    poly.iter()
                        .map(|ring| {
                            ring.iter()
                                .map(|p| [p.x, p.y, p.z.unwrap_or(0.0)])
                                .collect()
                        })
                        .collect()
                })
                .collect();
            Ok(GeoJsonGeometry::MultiPolygonZ(gj_polys))
        }

        // ── Geometry collections ──────────────────────────────────────────────
        GpkgGeometry::GeometryCollection(geoms)
        | GpkgGeometry::GeometryCollectionZ(geoms)
        | GpkgGeometry::GeometryCollectionM(geoms)
        | GpkgGeometry::GeometryCollectionZM(geoms) => {
            let converted: Result<Vec<GeoJsonGeometry>, GpkgError> =
                geoms.iter().map(gpkg_geom_to_geojson).collect();
            Ok(GeoJsonGeometry::GeometryCollection(converted?))
        }

        // ── Empty / null ──────────────────────────────────────────────────────
        GpkgGeometry::Empty => Ok(GeoJsonGeometry::Null),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GeoJsonGeometry → GpkgGeometry
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a [`GeoJsonGeometry`] to the equivalent [`GpkgGeometry`].
///
/// # Coordinate handling
/// - 2-D GeoJSON variants produce 2-D GPKG variants.
/// - 3-D (`Z`) GeoJSON variants produce the corresponding Z GPKG variant.
/// - [`GeoJsonGeometry::Null`] produces [`GpkgGeometry::Empty`].
/// - [`GeoJsonGeometry::GeometryCollection`] recursively converts sub-geometries.
///
/// # Errors
///
/// Returns [`GpkgError::UnsupportedGeometry`] for any geometry kind that
/// cannot be represented (should not occur for well-formed GeoJSON).
pub fn geojson_geom_to_gpkg(g: &GeoJsonGeometry) -> Result<GpkgGeometry, GpkgError> {
    match g {
        // ── 2-D ──────────────────────────────────────────────────────────────
        GeoJsonGeometry::Point([x, y]) => Ok(GpkgGeometry::Point { x: *x, y: *y }),

        GeoJsonGeometry::LineString(pts) => {
            let coords: Vec<(f64, f64)> = pts.iter().map(|[x, y]| (*x, *y)).collect();
            Ok(GpkgGeometry::LineString { coords })
        }

        GeoJsonGeometry::Polygon(rings) => {
            let gpkg_rings: Vec<Vec<(f64, f64)>> = rings
                .iter()
                .map(|ring| ring.iter().map(|[x, y]| (*x, *y)).collect())
                .collect();
            Ok(GpkgGeometry::Polygon { rings: gpkg_rings })
        }

        GeoJsonGeometry::MultiPoint(pts) => {
            let points: Vec<(f64, f64)> = pts.iter().map(|[x, y]| (*x, *y)).collect();
            Ok(GpkgGeometry::MultiPoint { points })
        }

        GeoJsonGeometry::MultiLineString(lines) => {
            let gpkg_lines: Vec<Vec<(f64, f64)>> = lines
                .iter()
                .map(|l| l.iter().map(|[x, y]| (*x, *y)).collect())
                .collect();
            Ok(GpkgGeometry::MultiLineString { lines: gpkg_lines })
        }

        GeoJsonGeometry::MultiPolygon(polys) => {
            let gpkg_polys: Vec<Vec<Vec<(f64, f64)>>> = polys
                .iter()
                .map(|poly| {
                    poly.iter()
                        .map(|ring| ring.iter().map(|[x, y]| (*x, *y)).collect())
                        .collect()
                })
                .collect();
            Ok(GpkgGeometry::MultiPolygon {
                polygons: gpkg_polys,
            })
        }

        // ── 3-D (Z) ──────────────────────────────────────────────────────────
        GeoJsonGeometry::PointZ([x, y, z]) => Ok(GpkgGeometry::PointZ {
            x: *x,
            y: *y,
            z: *z,
        }),

        GeoJsonGeometry::LineStringZ(pts) => {
            let coords: Vec<(f64, f64, f64)> = pts.iter().map(|[x, y, z]| (*x, *y, *z)).collect();
            Ok(GpkgGeometry::LineStringZ { coords })
        }

        GeoJsonGeometry::PolygonZ(rings) => {
            let gpkg_rings: Vec<Vec<(f64, f64, f64)>> = rings
                .iter()
                .map(|ring| ring.iter().map(|[x, y, z]| (*x, *y, *z)).collect())
                .collect();
            Ok(GpkgGeometry::PolygonZ { rings: gpkg_rings })
        }

        GeoJsonGeometry::MultiPointZ(pts) => {
            let points: Vec<(f64, f64, f64)> = pts.iter().map(|[x, y, z]| (*x, *y, *z)).collect();
            Ok(GpkgGeometry::MultiPointZ { points })
        }

        GeoJsonGeometry::MultiLineStringZ(lines) => {
            let gpkg_lines: Vec<Vec<(f64, f64, f64)>> = lines
                .iter()
                .map(|l| l.iter().map(|[x, y, z]| (*x, *y, *z)).collect())
                .collect();
            Ok(GpkgGeometry::MultiLineStringZ { lines: gpkg_lines })
        }

        GeoJsonGeometry::MultiPolygonZ(polys) => {
            let gpkg_polys: Vec<Vec<Vec<(f64, f64, f64)>>> = polys
                .iter()
                .map(|poly| {
                    poly.iter()
                        .map(|ring| ring.iter().map(|[x, y, z]| (*x, *y, *z)).collect())
                        .collect()
                })
                .collect();
            Ok(GpkgGeometry::MultiPolygonZ {
                polygons: gpkg_polys,
            })
        }

        // ── Collection ────────────────────────────────────────────────────────
        GeoJsonGeometry::GeometryCollection(geoms) => {
            let converted: Result<Vec<GpkgGeometry>, GpkgError> =
                geoms.iter().map(geojson_geom_to_gpkg).collect();
            Ok(GpkgGeometry::GeometryCollection(converted?))
        }

        // ── Null ──────────────────────────────────────────────────────────────
        GeoJsonGeometry::Null => Ok(GpkgGeometry::Empty),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureTable → FeatureCollection
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a [`FeatureTable`] to a GeoJSON [`FeatureCollection`].
///
/// Each [`FeatureRow`] becomes a [`GeoJsonFeature`] with:
/// - `id` set to [`FeatureId::Number`] carrying the row's `fid` value.
/// - `geometry` converted via [`gpkg_geom_to_geojson`]; `None` geometry becomes
///   a `Some(GeoJsonGeometry::Null)` (GeoJSON mandates the geometry key present).
/// - `properties` built from the row's [`FieldValue`] map (see
///   `field_value_to_json` for the per-type encoding).
///
/// SRS metadata stored on the [`FeatureTable`] is intentionally **not** included
/// in the output, following RFC 7946 which assumes WGS 84 and omits CRS objects.
///
/// # Errors
///
/// Propagates any [`GpkgError`] returned by [`gpkg_geom_to_geojson`].
pub fn feature_table_to_geojson(table: &FeatureTable) -> Result<FeatureCollection, GpkgError> {
    let mut features = Vec::with_capacity(table.features.len());

    for row in &table.features {
        let geometry = match &row.geometry {
            Some(g) => Some(gpkg_geom_to_geojson(g)?),
            None => Some(GeoJsonGeometry::Null),
        };

        let properties = Some(build_properties_serde_map(&row.fields));

        features.push(GeoJsonFeature {
            id: Some(FeatureId::Number(row.fid as f64)),
            geometry,
            properties,
        });
    }

    Ok(FeatureCollection {
        features,
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: Some(table.name.clone()),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureCollection → FeatureTable
// ─────────────────────────────────────────────────────────────────────────────

/// Build a [`FeatureTable`] from a GeoJSON [`FeatureCollection`].
///
/// # Schema inference
///
/// The schema (column definitions) is inferred by scanning the **first feature
/// that has a non-empty properties object**.  All discovered property keys
/// become columns with types derived from the first non-null JSON value seen
/// for each key.  Features processed after schema inference have any absent
/// properties filled with [`FieldValue::Null`].
///
/// # FID assignment
///
/// If a GeoJSON feature carries a numeric `id` ([`FeatureId::Number`]), the
/// integer part of that value is used as the FID.  String IDs and absent IDs
/// both fall back to a 1-based sequential counter (`1, 2, 3, …`).
///
/// # Arguments
///
/// * `fc` — source [`FeatureCollection`].
/// * `name` — table name for the resulting [`FeatureTable`].
/// * `geometry_column` — name of the geometry column (stored in metadata only;
///   geometry is stored on [`FeatureRow`], not as a regular field).
///
/// # Errors
///
/// Propagates any [`GpkgError`] returned by [`geojson_geom_to_gpkg`].
pub fn feature_table_from_geojson(
    fc: &FeatureCollection,
    name: &str,
    geometry_column: &str,
) -> Result<FeatureTable, GpkgError> {
    let mut table = FeatureTable::new(name, geometry_column);

    // ── Schema inference: first feature with non-empty properties ─────────────
    let schema = infer_schema_from_features(&fc.features);
    table.schema = schema;

    // ── Convert features ──────────────────────────────────────────────────────
    let mut sequential_fid: i64 = 1;

    for feat in &fc.features {
        // Determine FID
        let fid = match &feat.id {
            Some(FeatureId::Number(n)) => {
                let candidate = *n as i64;
                sequential_fid = sequential_fid.max(candidate + 1);
                candidate
            }
            _ => {
                let fid = sequential_fid;
                sequential_fid += 1;
                fid
            }
        };

        // Convert geometry
        let geometry = match &feat.geometry {
            Some(gj_geom) => {
                let gpkg = geojson_geom_to_gpkg(gj_geom)?;
                // Empty geometry (from GeoJsonGeometry::Null) → None
                if matches!(gpkg, GpkgGeometry::Empty) {
                    None
                } else {
                    Some(gpkg)
                }
            }
            None => None,
        };

        // Convert properties
        let fields = extract_fields_from_properties(&feat.properties, &table.schema);

        table.features.push(FeatureRow {
            fid,
            geometry,
            fields,
        });
    }

    Ok(table)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a [`serde_json::Value::Object`] from a [`FieldValue`] map.
fn build_properties_serde_map(fields: &HashMap<String, FieldValue>) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(fields.len());
    for (key, val) in fields {
        map.insert(key.clone(), field_value_to_json(val));
    }
    serde_json::Value::Object(map)
}

/// Convert a single [`FieldValue`] to a [`serde_json::Value`].
///
/// - [`FieldValue::Blob`] is encoded as a lowercase hexadecimal string with a
///   `"0x"` prefix (mirrors the existing `to_json` helper in `types.rs`).
/// - Non-finite reals (`NaN`, `±∞`) map to `null`.
fn field_value_to_json(val: &FieldValue) -> serde_json::Value {
    match val {
        FieldValue::Integer(i) => serde_json::Value::Number((*i).into()),
        FieldValue::Real(f) => {
            match serde_json::Number::from_f64(*f) {
                Some(n) => serde_json::Value::Number(n),
                None => serde_json::Value::Null, // NaN / ±∞ → null
            }
        }
        FieldValue::Text(s) => serde_json::Value::String(s.clone()),
        FieldValue::Blob(b) => {
            let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
            serde_json::Value::String(format!("0x{hex}"))
        }
        FieldValue::Boolean(b) => serde_json::Value::Bool(*b),
        FieldValue::Null => serde_json::Value::Null,
    }
}

/// Convert a [`serde_json::Value`] to a [`FieldValue`], guided by the expected
/// [`FieldType`].  When `expected_type` is `None` the type is inferred from the
/// JSON value.
fn json_value_to_field_value(
    val: &serde_json::Value,
    expected_type: Option<FieldType>,
) -> FieldValue {
    match val {
        serde_json::Value::Null => FieldValue::Null,
        serde_json::Value::Bool(b) => match expected_type {
            Some(FieldType::Integer) => FieldValue::Integer(if *b { 1 } else { 0 }),
            _ => FieldValue::Boolean(*b),
        },
        serde_json::Value::Number(n) => {
            match expected_type {
                Some(FieldType::Real) => FieldValue::Real(n.as_f64().unwrap_or(0.0)),
                Some(FieldType::Text) => FieldValue::Text(n.to_string()),
                _ => {
                    // Prefer integer representation when possible
                    if let Some(i) = n.as_i64() {
                        FieldValue::Integer(i)
                    } else {
                        FieldValue::Real(n.as_f64().unwrap_or(0.0))
                    }
                }
            }
        }
        serde_json::Value::String(s) => match expected_type {
            Some(FieldType::Integer) => s
                .parse::<i64>()
                .map(FieldValue::Integer)
                .unwrap_or_else(|_| FieldValue::Text(s.clone())),
            Some(FieldType::Real) => s
                .parse::<f64>()
                .map(FieldValue::Real)
                .unwrap_or_else(|_| FieldValue::Text(s.clone())),
            _ => FieldValue::Text(s.clone()),
        },
        // Arrays / objects → serialise back to text
        other => FieldValue::Text(other.to_string()),
    }
}

/// Infer the [`FieldType`] from a [`serde_json::Value`].
fn json_value_to_field_type(val: &serde_json::Value) -> FieldType {
    match val {
        serde_json::Value::Bool(_) => FieldType::Boolean,
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                FieldType::Integer
            } else {
                FieldType::Real
            }
        }
        serde_json::Value::String(_) => FieldType::Text,
        serde_json::Value::Null => FieldType::Null,
        _ => FieldType::Text, // arrays / objects
    }
}

/// Infer a schema (ordered [`FieldDefinition`] list) from the first feature
/// in `features` that has a non-empty properties object.
///
/// Returns an empty `Vec` when no such feature exists.
fn infer_schema_from_features(features: &[GeoJsonFeature]) -> Vec<FieldDefinition> {
    // Find first feature with non-trivial properties
    let props_obj = features.iter().find_map(|feat| {
        let props = feat.properties.as_ref()?;
        let map = props.as_object()?;
        if map.is_empty() { None } else { Some(map) }
    });

    let Some(map) = props_obj else {
        return Vec::new();
    };

    // Preserve insertion order for reproducibility (serde_json::Map is ordered)
    map.iter()
        .map(|(key, val)| {
            let field_type = if val.is_null() {
                // Null value — fall back to Text; we will refine by scanning
                // further features below.
                FieldType::Text
            } else {
                json_value_to_field_type(val)
            };
            FieldDefinition {
                name: key.clone(),
                field_type,
                not_null: false,
                primary_key: false,
                default_value: None,
            }
        })
        .collect()
}

/// Build a [`HashMap<String, FieldValue>`] from an optional properties
/// [`serde_json::Value`], filling absent schema columns with [`FieldValue::Null`].
fn extract_fields_from_properties(
    properties: &Option<serde_json::Value>,
    schema: &[FieldDefinition],
) -> HashMap<String, FieldValue> {
    let mut fields: HashMap<String, FieldValue> = HashMap::with_capacity(schema.len());

    // Pre-fill all schema columns with Null
    for col in schema {
        fields.insert(col.name.clone(), FieldValue::Null);
    }

    let Some(props) = properties else {
        return fields;
    };

    let Some(map) = props.as_object() else {
        return fields;
    };

    for (key, val) in map {
        // Look up expected type from schema (if this key is known)
        let expected = schema
            .iter()
            .find(|col| col.name == *key)
            .map(|col| col.field_type);

        let field_value = json_value_to_field_value(val, expected);
        fields.insert(key.clone(), field_value);
    }

    fields
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_gpkg_point_to_geojson() {
        let g = GpkgGeometry::Point { x: 1.0, y: 2.0 };
        let gj = gpkg_geom_to_geojson(&g).expect("conversion should succeed");
        assert_eq!(gj, GeoJsonGeometry::Point([1.0, 2.0]));
    }

    #[test]
    fn test_geojson_point_to_gpkg() {
        let gj = GeoJsonGeometry::Point([3.0, 4.0]);
        let g = geojson_geom_to_gpkg(&gj).expect("conversion should succeed");
        assert_eq!(g, GpkgGeometry::Point { x: 3.0, y: 4.0 });
    }

    #[test]
    fn test_null_geometry_round_trip() {
        let gpkg = GpkgGeometry::Empty;
        let gj = gpkg_geom_to_geojson(&gpkg).expect("empty → Null should succeed");
        assert_eq!(gj, GeoJsonGeometry::Null);
        let back = geojson_geom_to_gpkg(&gj).expect("Null → Empty should succeed");
        assert_eq!(back, GpkgGeometry::Empty);
    }

    #[test]
    fn test_m_variant_projects_to_xy() {
        let g = GpkgGeometry::PointM {
            x: 5.0,
            y: 6.0,
            m: 99.0,
        };
        let gj = gpkg_geom_to_geojson(&g).expect("PointM → Point");
        assert_eq!(gj, GeoJsonGeometry::Point([5.0, 6.0]));
    }
}
