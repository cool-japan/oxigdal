//! Vector I/O bindings for Node.js
//!
//! This module provides comprehensive vector dataset operations including
//! GeoJSON reading/writing, geometry operations, and feature management.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigeo_core::vector::{Coordinate as CoreCoord, Geometry, LineString, Point, Polygon};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use crate::error::{NodeError, ToNapiResult};

/// Vector feature with geometry and properties
#[napi]
pub struct Feature {
    geometry: Option<Geometry>,
    properties: HashMap<String, String>,
    id: Option<String>,
}

#[napi]
impl Feature {
    /// Creates a new feature
    #[napi(constructor)]
    pub fn new(geometry: Option<&GeometryWrapper>, properties: Option<Object>) -> Result<Self> {
        let geom = geometry.map(|g| g.inner.clone());

        let mut props = HashMap::new();
        if let Some(obj) = properties {
            let keys = Object::keys(&obj)?;
            for key in keys {
                if let Some(value) = obj.get::<String>(&key)? {
                    props.insert(key, value);
                }
            }
        }

        Ok(Self {
            geometry: geom,
            properties: props,
            id: None,
        })
    }

    /// Gets the feature ID
    #[napi(getter)]
    pub fn id(&self) -> Option<String> {
        self.id.clone()
    }

    /// Sets the feature ID
    #[napi(setter)]
    pub fn set_id(&mut self, id: Option<String>) {
        self.id = id;
    }

    /// Gets the geometry
    #[napi]
    pub fn get_geometry(&self) -> Option<GeometryWrapper> {
        self.geometry
            .as_ref()
            .map(|g| GeometryWrapper { inner: g.clone() })
    }

    /// Sets the geometry
    #[napi]
    pub fn set_geometry(&mut self, geometry: Option<&GeometryWrapper>) {
        self.geometry = geometry.map(|g| g.inner.clone());
    }

    /// Gets a property value
    #[napi]
    pub fn get_property(&self, key: String) -> Option<String> {
        self.properties.get(&key).cloned()
    }

    /// Sets a property value
    #[napi]
    pub fn set_property(&mut self, key: String, value: String) {
        self.properties.insert(key, value);
    }

    /// Gets all property keys
    #[napi]
    pub fn get_property_keys(&self) -> Vec<String> {
        self.properties.keys().cloned().collect()
    }

    /// Converts to GeoJSON object
    #[napi]
    pub fn to_geojson(&self) -> Result<String> {
        let mut feature_obj = serde_json::Map::new();
        feature_obj.insert("type".to_string(), JsonValue::String("Feature".to_string()));

        if let Some(ref id) = self.id {
            feature_obj.insert("id".to_string(), JsonValue::String(id.clone()));
        }

        if let Some(ref geom) = self.geometry {
            let geom_json = geometry_to_geojson(geom)?;
            feature_obj.insert("geometry".to_string(), geom_json);
        } else {
            feature_obj.insert("geometry".to_string(), JsonValue::Null);
        }

        let props: serde_json::Map<String, JsonValue> = self
            .properties
            .iter()
            .map(|(k, v)| (k.clone(), JsonValue::String(v.clone())))
            .collect();
        feature_obj.insert("properties".to_string(), JsonValue::Object(props));

        serde_json::to_string(&JsonValue::Object(feature_obj)).map_err(|e| {
            NodeError {
                code: "SERIALIZATION_ERROR".to_string(),
                message: format!("Failed to serialize feature: {}", e),
            }
            .into()
        })
    }

    /// Creates a feature from GeoJSON string
    #[napi(factory)]
    pub fn from_geojson(geojson: String) -> Result<Self> {
        let value: JsonValue = serde_json::from_str(&geojson).map_err(|e| NodeError {
            code: "PARSE_ERROR".to_string(),
            message: format!("Failed to parse GeoJSON: {}", e),
        })?;

        let obj = value.as_object().ok_or_else(|| NodeError {
            code: "INVALID_GEOJSON".to_string(),
            message: "GeoJSON must be an object".to_string(),
        })?;

        let feature_type = obj
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: "Missing 'type' field".to_string(),
            })?;

        if feature_type != "Feature" {
            return Err(NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: format!("Expected Feature, got {}", feature_type),
            }
            .into());
        }

        let id = obj.get("id").and_then(|v| v.as_str()).map(String::from);

        let geometry = if let Some(geom_val) = obj.get("geometry") {
            if !geom_val.is_null() {
                Some(geometry_from_geojson(geom_val)?)
            } else {
                None
            }
        } else {
            None
        };

        let mut properties = HashMap::new();
        if let Some(props_val) = obj.get("properties")
            && let Some(props_obj) = props_val.as_object()
        {
            for (key, value) in props_obj {
                if let Some(str_val) = value.as_str() {
                    properties.insert(key.clone(), str_val.to_string());
                } else {
                    properties.insert(key.clone(), value.to_string());
                }
            }
        }

        Ok(Self {
            geometry,
            properties,
            id,
        })
    }
}

/// Geometry wrapper for Node.js
#[napi]
pub struct GeometryWrapper {
    pub(crate) inner: Geometry,
}

impl GeometryWrapper {
    /// Gets the inner geometry
    #[allow(dead_code)]
    pub(crate) fn inner(&self) -> &Geometry {
        &self.inner
    }
}

#[napi]
impl GeometryWrapper {
    /// Creates a Point geometry
    #[napi(factory)]
    pub fn point(x: f64, y: f64, z: Option<f64>) -> Self {
        let coord = if let Some(z_val) = z {
            CoreCoord::new_3d(x, y, z_val)
        } else {
            CoreCoord::new_2d(x, y)
        };
        Self {
            inner: Geometry::Point(Point::from_coord(coord)),
        }
    }

    /// Creates a LineString geometry
    #[napi(factory)]
    pub fn linestring(coordinates: Vec<Vec<f64>>) -> Result<Self> {
        let coords: Result<Vec<CoreCoord>> = coordinates
            .into_iter()
            .map(|c| {
                if c.len() < 2 {
                    Err(NodeError {
                        code: "INVALID_COORDINATES".to_string(),
                        message: "Coordinate must have at least 2 values".to_string(),
                    }
                    .into())
                } else if c.len() == 2 {
                    Ok(CoreCoord::new_2d(c[0], c[1]))
                } else {
                    Ok(CoreCoord::new_3d(c[0], c[1], c[2]))
                }
            })
            .collect();

        let linestring = LineString::new(coords?).to_napi()?;
        Ok(Self {
            inner: Geometry::LineString(linestring),
        })
    }

    /// Creates a Polygon geometry
    #[napi(factory)]
    pub fn polygon(rings: Vec<Vec<Vec<f64>>>) -> Result<Self> {
        if rings.is_empty() {
            return Err(NodeError {
                code: "INVALID_GEOMETRY".to_string(),
                message: "Polygon must have at least one ring".to_string(),
            }
            .into());
        }

        let exterior_coords: Result<Vec<CoreCoord>> = rings[0]
            .iter()
            .map(|c| {
                if c.len() < 2 {
                    Err(NodeError {
                        code: "INVALID_COORDINATES".to_string(),
                        message: "Coordinate must have at least 2 values".to_string(),
                    }
                    .into())
                } else if c.len() == 2 {
                    Ok(CoreCoord::new_2d(c[0], c[1]))
                } else {
                    Ok(CoreCoord::new_3d(c[0], c[1], c[2]))
                }
            })
            .collect();

        let exterior = LineString::new(exterior_coords?).to_napi()?;

        let mut holes = Vec::new();
        for ring in &rings[1..] {
            let hole_coords: Result<Vec<CoreCoord>> = ring
                .iter()
                .map(|c| {
                    if c.len() < 2 {
                        Err(NodeError {
                            code: "INVALID_COORDINATES".to_string(),
                            message: "Coordinate must have at least 2 values".to_string(),
                        }
                        .into())
                    } else if c.len() == 2 {
                        Ok(CoreCoord::new_2d(c[0], c[1]))
                    } else {
                        Ok(CoreCoord::new_3d(c[0], c[1], c[2]))
                    }
                })
                .collect();

            holes.push(LineString::new(hole_coords?).to_napi()?);
        }

        let polygon = Polygon::new(exterior, holes).to_napi()?;
        Ok(Self {
            inner: Geometry::Polygon(polygon),
        })
    }

    /// Gets the geometry type
    #[napi(getter)]
    pub fn geometry_type(&self) -> String {
        match &self.inner {
            Geometry::Point(_) => "Point".to_string(),
            Geometry::LineString(_) => "LineString".to_string(),
            Geometry::Polygon(_) => "Polygon".to_string(),
            Geometry::MultiPoint(_) => "MultiPoint".to_string(),
            Geometry::MultiLineString(_) => "MultiLineString".to_string(),
            Geometry::MultiPolygon(_) => "MultiPolygon".to_string(),
            Geometry::GeometryCollection(_) => "GeometryCollection".to_string(),
        }
    }

    /// Converts to GeoJSON string
    #[napi]
    pub fn to_geojson(&self) -> Result<String> {
        let json = geometry_to_geojson(&self.inner)?;
        serde_json::to_string(&json).map_err(|e| {
            NodeError {
                code: "SERIALIZATION_ERROR".to_string(),
                message: format!("Failed to serialize geometry: {}", e),
            }
            .into()
        })
    }

    /// Creates geometry from GeoJSON string
    #[napi(factory)]
    pub fn from_geojson(geojson: String) -> Result<Self> {
        let value: JsonValue = serde_json::from_str(&geojson).map_err(|e| NodeError {
            code: "PARSE_ERROR".to_string(),
            message: format!("Failed to parse GeoJSON: {}", e),
        })?;

        let geometry = geometry_from_geojson(&value)?;
        Ok(Self { inner: geometry })
    }

    /// Gets the bounding box [minX, minY, maxX, maxY]
    #[napi]
    pub fn bounds(&self) -> Result<Vec<f64>> {
        let bounds = match &self.inner {
            Geometry::Point(p) => {
                let c = p.coord;
                vec![c.x, c.y, c.x, c.y]
            }
            Geometry::LineString(ls) => {
                let coords = &ls.coords;
                if coords.is_empty() {
                    return Err(NodeError {
                        code: "EMPTY_GEOMETRY".to_string(),
                        message: "Cannot compute bounds of empty linestring".to_string(),
                    }
                    .into());
                }
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for coord in coords {
                    min_x = min_x.min(coord.x);
                    min_y = min_y.min(coord.y);
                    max_x = max_x.max(coord.x);
                    max_y = max_y.max(coord.y);
                }
                vec![min_x, min_y, max_x, max_y]
            }
            Geometry::Polygon(p) => {
                let coords = &p.exterior.coords;
                if coords.is_empty() {
                    return Err(NodeError {
                        code: "EMPTY_GEOMETRY".to_string(),
                        message: "Cannot compute bounds of empty polygon".to_string(),
                    }
                    .into());
                }
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for coord in coords {
                    min_x = min_x.min(coord.x);
                    min_y = min_y.min(coord.y);
                    max_x = max_x.max(coord.x);
                    max_y = max_y.max(coord.y);
                }
                vec![min_x, min_y, max_x, max_y]
            }
            _ => {
                return Err(NodeError {
                    code: "NOT_IMPLEMENTED".to_string(),
                    message: "Bounds not implemented for this geometry type".to_string(),
                }
                .into());
            }
        };

        Ok(bounds)
    }

    /// Clones the geometry
    #[napi]
    pub fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Feature collection
#[napi]
pub struct FeatureCollection {
    features: Vec<Feature>,
}

#[napi]
impl FeatureCollection {
    /// Creates a new feature collection
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            features: Vec::new(),
        }
    }

    /// Adds a feature to the collection
    #[napi]
    pub fn add_feature(&mut self, feature: &Feature) {
        self.features.push(feature.clone());
    }

    /// Gets the number of features
    #[napi(getter)]
    pub fn count(&self) -> u32 {
        self.features.len() as u32
    }

    /// Gets a feature by index
    #[napi]
    pub fn get_feature(&self, index: u32) -> Option<Feature> {
        self.features.get(index as usize).cloned()
    }

    /// Converts to GeoJSON FeatureCollection string
    #[napi]
    pub fn to_geojson(&self) -> Result<String> {
        let mut collection = serde_json::Map::new();
        collection.insert(
            "type".to_string(),
            JsonValue::String("FeatureCollection".to_string()),
        );

        let features: Result<Vec<JsonValue>> = self
            .features
            .iter()
            .map(|f| {
                let json_str = f.to_geojson()?;
                serde_json::from_str(&json_str).map_err(|e| {
                    NodeError {
                        code: "SERIALIZATION_ERROR".to_string(),
                        message: format!("Failed to parse feature: {}", e),
                    }
                    .into()
                })
            })
            .collect();

        collection.insert("features".to_string(), JsonValue::Array(features?));

        serde_json::to_string(&JsonValue::Object(collection)).map_err(|e| {
            NodeError {
                code: "SERIALIZATION_ERROR".to_string(),
                message: format!("Failed to serialize feature collection: {}", e),
            }
            .into()
        })
    }

    /// Creates from GeoJSON string
    #[napi(factory)]
    pub fn from_geojson(geojson: String) -> Result<Self> {
        let value: JsonValue = serde_json::from_str(&geojson).map_err(|e| NodeError {
            code: "PARSE_ERROR".to_string(),
            message: format!("Failed to parse GeoJSON: {}", e),
        })?;

        let obj = value.as_object().ok_or_else(|| NodeError {
            code: "INVALID_GEOJSON".to_string(),
            message: "GeoJSON must be an object".to_string(),
        })?;

        let collection_type =
            obj.get("type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| NodeError {
                    code: "INVALID_GEOJSON".to_string(),
                    message: "Missing 'type' field".to_string(),
                })?;

        if collection_type != "FeatureCollection" {
            return Err(NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: format!("Expected FeatureCollection, got {}", collection_type),
            }
            .into());
        }

        let features_array = obj
            .get("features")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: "Missing or invalid 'features' array".to_string(),
            })?;

        let features: Result<Vec<Feature>> = features_array
            .iter()
            .map(|f| {
                let feature_str = serde_json::to_string(f).map_err(|e| NodeError {
                    code: "SERIALIZATION_ERROR".to_string(),
                    message: format!("Failed to serialize feature: {}", e),
                })?;
                Feature::from_geojson(feature_str)
            })
            .collect();

        Ok(Self {
            features: features?,
        })
    }
}

// Helper functions

/// Serializes a single core coordinate to a GeoJSON position array,
/// preserving the Z ordinate when present.
fn coord_to_json(c: &CoreCoord) -> JsonValue {
    let arr = if c.has_z() {
        vec![c.x, c.y, c.z.unwrap_or(0.0)]
    } else {
        vec![c.x, c.y]
    };
    JsonValue::Array(arr.into_iter().map(JsonValue::from).collect())
}

/// Serializes a line/ring's coordinates to a GeoJSON array of positions.
fn linestring_coords_to_json(ls: &LineString) -> JsonValue {
    JsonValue::Array(ls.coords.iter().map(coord_to_json).collect())
}

/// Serializes a polygon's rings (exterior first, then holes) to a GeoJSON
/// array of linear rings.
fn polygon_coords_to_json(p: &Polygon) -> JsonValue {
    let mut rings = Vec::with_capacity(1 + p.interiors.len());
    rings.push(linestring_coords_to_json(&p.exterior));
    for hole in &p.interiors {
        rings.push(linestring_coords_to_json(hole));
    }
    JsonValue::Array(rings)
}

/// Builds a `{ "type": ty, "coordinates": coords }` GeoJSON object.
fn typed_geometry(ty: &str, coords: JsonValue) -> JsonValue {
    let mut obj = serde_json::Map::new();
    obj.insert("type".to_string(), JsonValue::String(ty.to_string()));
    obj.insert("coordinates".to_string(), coords);
    JsonValue::Object(obj)
}

fn geometry_to_geojson(geom: &Geometry) -> Result<JsonValue> {
    match geom {
        Geometry::Point(p) => Ok(typed_geometry("Point", coord_to_json(&p.coord))),
        Geometry::LineString(ls) => Ok(typed_geometry("LineString", linestring_coords_to_json(ls))),
        Geometry::Polygon(p) => Ok(typed_geometry("Polygon", polygon_coords_to_json(p))),
        Geometry::MultiPoint(mp) => {
            let coords = JsonValue::Array(
                mp.points
                    .iter()
                    .map(|pt| coord_to_json(&pt.coord))
                    .collect(),
            );
            Ok(typed_geometry("MultiPoint", coords))
        }
        Geometry::MultiLineString(mls) => {
            let coords = JsonValue::Array(
                mls.line_strings
                    .iter()
                    .map(linestring_coords_to_json)
                    .collect(),
            );
            Ok(typed_geometry("MultiLineString", coords))
        }
        Geometry::MultiPolygon(mp) => {
            let coords = JsonValue::Array(mp.polygons.iter().map(polygon_coords_to_json).collect());
            Ok(typed_geometry("MultiPolygon", coords))
        }
        Geometry::GeometryCollection(gc) => {
            let geometries: Result<Vec<JsonValue>> =
                gc.geometries.iter().map(geometry_to_geojson).collect();
            let mut obj = serde_json::Map::new();
            obj.insert(
                "type".to_string(),
                JsonValue::String("GeometryCollection".to_string()),
            );
            obj.insert("geometries".to_string(), JsonValue::Array(geometries?));
            Ok(JsonValue::Object(obj))
        }
    }
}

/// Parses a single GeoJSON position array (`[x, y]` or `[x, y, z]`) into a
/// core coordinate. Positions with more than three ordinates keep only x/y/z
/// (the GeoJSON spec permits extra ordinates whose meaning is undefined).
fn coord_from_json(value: &JsonValue) -> Result<CoreCoord> {
    let arr = value.as_array().ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Position must be an array of numbers".to_string(),
    })?;

    if arr.len() < 2 {
        return Err(NodeError {
            code: "INVALID_COORDINATES".to_string(),
            message: "Position must have at least 2 ordinates".to_string(),
        }
        .into());
    }

    let x = arr[0].as_f64().ok_or_else(|| NodeError {
        code: "INVALID_COORDINATES".to_string(),
        message: "Invalid x coordinate".to_string(),
    })?;

    let y = arr[1].as_f64().ok_or_else(|| NodeError {
        code: "INVALID_COORDINATES".to_string(),
        message: "Invalid y coordinate".to_string(),
    })?;

    if arr.len() > 2 {
        let z = arr[2].as_f64().ok_or_else(|| NodeError {
            code: "INVALID_COORDINATES".to_string(),
            message: "Invalid z coordinate".to_string(),
        })?;
        Ok(CoreCoord::new_3d(x, y, z))
    } else {
        Ok(CoreCoord::new_2d(x, y))
    }
}

/// Parses a GeoJSON array of positions into a vector of core coordinates.
fn coords_from_json(value: &JsonValue) -> Result<Vec<CoreCoord>> {
    let arr = value.as_array().ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Expected an array of positions".to_string(),
    })?;
    arr.iter().map(coord_from_json).collect()
}

/// Parses a GeoJSON linear-ring / line array into a core `LineString`.
fn linestring_from_json(value: &JsonValue) -> Result<LineString> {
    let coords = coords_from_json(value)?;
    LineString::new(coords).to_napi()
}

/// Parses a GeoJSON polygon coordinate array (an array of linear rings, the
/// first being the exterior and the rest holes) into a core `Polygon`.
fn polygon_from_json(value: &JsonValue) -> Result<Polygon> {
    let rings = value.as_array().ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Polygon coordinates must be an array of rings".to_string(),
    })?;

    let mut ring_iter = rings.iter();
    let exterior_json = ring_iter.next().ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Polygon must have at least one (exterior) ring".to_string(),
    })?;
    let exterior = linestring_from_json(exterior_json)?;

    let mut holes = Vec::new();
    for hole_json in ring_iter {
        holes.push(linestring_from_json(hole_json)?);
    }

    Polygon::new(exterior, holes).to_napi()
}

fn geometry_from_geojson(value: &JsonValue) -> Result<Geometry> {
    use oxigeo_core::vector::{MultiLineString, MultiPoint, MultiPolygon};

    let obj = value.as_object().ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Geometry must be an object".to_string(),
    })?;

    let geom_type = obj
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| NodeError {
            code: "INVALID_GEOJSON".to_string(),
            message: "Missing geometry 'type' field".to_string(),
        })?;

    // GeometryCollection carries "geometries" rather than "coordinates"; every
    // other type is coordinate-based, so only fetch "coordinates" for those.
    if geom_type == "GeometryCollection" {
        let geometries = obj
            .get("geometries")
            .and_then(|g| g.as_array())
            .ok_or_else(|| NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: "GeometryCollection must have a 'geometries' array".to_string(),
            })?;
        let parsed: Result<Vec<Geometry>> = geometries.iter().map(geometry_from_geojson).collect();
        return Ok(Geometry::GeometryCollection(
            oxigeo_core::vector::GeometryCollection::new(parsed?),
        ));
    }

    let coords = obj.get("coordinates").ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Missing 'coordinates' field".to_string(),
    })?;

    match geom_type {
        "Point" => {
            let coord = coord_from_json(coords)?;
            Ok(Geometry::Point(Point::from_coord(coord)))
        }
        "LineString" => Ok(Geometry::LineString(linestring_from_json(coords)?)),
        "Polygon" => Ok(Geometry::Polygon(polygon_from_json(coords)?)),
        "MultiPoint" => {
            let coords = coords_from_json(coords)?;
            let points = coords.into_iter().map(Point::from_coord).collect();
            Ok(Geometry::MultiPoint(MultiPoint::new(points)))
        }
        "MultiLineString" => {
            let arr = coords.as_array().ok_or_else(|| NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: "MultiLineString coordinates must be an array".to_string(),
            })?;
            let line_strings: Result<Vec<LineString>> =
                arr.iter().map(linestring_from_json).collect();
            Ok(Geometry::MultiLineString(MultiLineString::new(
                line_strings?,
            )))
        }
        "MultiPolygon" => {
            let arr = coords.as_array().ok_or_else(|| NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: "MultiPolygon coordinates must be an array".to_string(),
            })?;
            let polygons: Result<Vec<Polygon>> = arr.iter().map(polygon_from_json).collect();
            Ok(Geometry::MultiPolygon(MultiPolygon::new(polygons?)))
        }
        _ => Err(NodeError {
            code: "INVALID_GEOJSON".to_string(),
            message: format!("Unknown geometry type '{}'", geom_type),
        }
        .into()),
    }
}

/// Reads a GeoJSON file
#[allow(dead_code)]
#[napi]
pub fn read_geojson(path: String) -> Result<FeatureCollection> {
    let content = std::fs::read_to_string(&path).map_err(|e| NodeError {
        code: "IO_ERROR".to_string(),
        message: format!("Failed to read file: {}", e),
    })?;

    FeatureCollection::from_geojson(content)
}

/// Writes a GeoJSON file
#[allow(dead_code)]
#[napi]
pub fn write_geojson(path: String, collection: &FeatureCollection) -> Result<()> {
    let content = collection.to_geojson()?;
    std::fs::write(&path, content).map_err(|e| {
        NodeError {
            code: "IO_ERROR".to_string(),
            message: format!("Failed to write file: {}", e),
        }
        .into()
    })
}

// Allow Feature to be cloned
impl Clone for Feature {
    fn clone(&self) -> Self {
        Self {
            geometry: self.geometry.clone(),
            properties: self.properties.clone(),
            id: self.id.clone(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Parses a GeoJSON geometry string into a core `Geometry`.
    fn parse(geojson: &str) -> Geometry {
        let value: JsonValue = serde_json::from_str(geojson).expect("valid json");
        geometry_from_geojson(&value).expect("geometry should parse")
    }

    #[test]
    fn parses_linestring() {
        let geom = parse(r#"{"type":"LineString","coordinates":[[0,0],[1,1],[2,3]]}"#);
        match geom {
            Geometry::LineString(ls) => {
                assert_eq!(ls.coords.len(), 3);
                assert_eq!(ls.coords[2].x, 2.0);
                assert_eq!(ls.coords[2].y, 3.0);
            }
            other => panic!("expected LineString, got {:?}", other),
        }
    }

    #[test]
    fn parses_polygon_with_hole() {
        let geom = parse(
            r#"{"type":"Polygon","coordinates":[
                [[0,0],[10,0],[10,10],[0,10],[0,0]],
                [[2,2],[4,2],[4,4],[2,4],[2,2]]
            ]}"#,
        );
        match geom {
            Geometry::Polygon(p) => {
                assert_eq!(p.exterior.coords.len(), 5);
                assert_eq!(p.interiors.len(), 1);
                assert_eq!(p.interiors[0].coords.len(), 5);
            }
            other => panic!("expected Polygon, got {:?}", other),
        }
    }

    #[test]
    fn parses_multipoint() {
        let geom = parse(r#"{"type":"MultiPoint","coordinates":[[0,0],[1,1]]}"#);
        match geom {
            Geometry::MultiPoint(mp) => assert_eq!(mp.points.len(), 2),
            other => panic!("expected MultiPoint, got {:?}", other),
        }
    }

    #[test]
    fn parses_multilinestring() {
        let geom = parse(
            r#"{"type":"MultiLineString","coordinates":[[[0,0],[1,1]],[[2,2],[3,3],[4,4]]]}"#,
        );
        match geom {
            Geometry::MultiLineString(mls) => {
                assert_eq!(mls.line_strings.len(), 2);
                assert_eq!(mls.line_strings[1].coords.len(), 3);
            }
            other => panic!("expected MultiLineString, got {:?}", other),
        }
    }

    #[test]
    fn parses_multipolygon() {
        let geom = parse(
            r#"{"type":"MultiPolygon","coordinates":[
                [[[0,0],[1,0],[1,1],[0,0]]],
                [[[5,5],[6,5],[6,6],[5,5]]]
            ]}"#,
        );
        match geom {
            Geometry::MultiPolygon(mp) => assert_eq!(mp.polygons.len(), 2),
            other => panic!("expected MultiPolygon, got {:?}", other),
        }
    }

    #[test]
    fn parses_geometry_collection() {
        let geom = parse(
            r#"{"type":"GeometryCollection","geometries":[
                {"type":"Point","coordinates":[0,0]},
                {"type":"LineString","coordinates":[[0,0],[1,1]]}
            ]}"#,
        );
        match geom {
            Geometry::GeometryCollection(gc) => {
                assert_eq!(gc.geometries.len(), 2);
                assert!(matches!(gc.geometries[0], Geometry::Point(_)));
                assert!(matches!(gc.geometries[1], Geometry::LineString(_)));
            }
            other => panic!("expected GeometryCollection, got {:?}", other),
        }
    }

    #[test]
    fn parses_3d_point() {
        let geom = parse(r#"{"type":"Point","coordinates":[1,2,3]}"#);
        match geom {
            Geometry::Point(p) => {
                assert!(p.coord.has_z());
                assert_eq!(p.coord.z, Some(3.0));
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    #[test]
    fn roundtrips_all_geometry_types_through_geojson() {
        let cases = [
            r#"{"type":"Point","coordinates":[1.0,2.0]}"#,
            r#"{"type":"LineString","coordinates":[[0.0,0.0],[1.0,1.0]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]}"#,
            r#"{"type":"MultiPoint","coordinates":[[0.0,0.0],[1.0,1.0]]}"#,
            r#"{"type":"MultiLineString","coordinates":[[[0.0,0.0],[1.0,1.0]]]}"#,
            r#"{"type":"MultiPolygon","coordinates":[[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]]}"#,
        ];

        for original in cases {
            let geom = parse(original);
            let json = geometry_to_geojson(&geom).expect("serialize");
            let reparsed = geometry_from_geojson(&json).expect("reparse");
            assert_eq!(
                geom, reparsed,
                "geometry did not survive a to/from GeoJSON round-trip: {original}"
            );
        }
    }

    #[test]
    fn feature_collection_reads_polygon_features() {
        // Regression: reading a practical polygon-based FeatureCollection used
        // to throw NOT_IMPLEMENTED on the first non-Point feature.
        let fc = FeatureCollection::from_geojson(
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","properties":{"name":"parcel"},
                 "geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}}
            ]}"#
            .to_string(),
        )
        .expect("feature collection should parse polygon geometry");
        assert_eq!(fc.count(), 1);
        let feature = fc.get_feature(0).expect("feature present");
        let geom = feature.get_geometry().expect("geometry present");
        assert_eq!(geom.geometry_type(), "Polygon");
    }

    #[test]
    fn rejects_unknown_geometry_type() {
        let value: JsonValue =
            serde_json::from_str(r#"{"type":"Nonsense","coordinates":[]}"#).unwrap();
        assert!(geometry_from_geojson(&value).is_err());
    }
}
