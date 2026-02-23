//! Vector I/O bindings for Node.js
//!
//! This module provides comprehensive vector dataset operations including
//! GeoJSON reading/writing, geometry operations, and feature management.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigdal_core::vector::{Coordinate as CoreCoord, Geometry, LineString, Point, Polygon};
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
                if let Some(value) = obj.get::<_, String>(&key)? {
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
        if let Some(props_val) = obj.get("properties") {
            if let Some(props_obj) = props_val.as_object() {
                for (key, value) in props_obj {
                    if let Some(str_val) = value.as_str() {
                        properties.insert(key.clone(), str_val.to_string());
                    } else {
                        properties.insert(key.clone(), value.to_string());
                    }
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

fn geometry_to_geojson(geom: &Geometry) -> Result<JsonValue> {
    match geom {
        Geometry::Point(p) => {
            let coord = p.coord;
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), JsonValue::String("Point".to_string()));
            let coords = if coord.has_z() {
                vec![coord.x, coord.y, coord.z.unwrap_or(0.0)]
            } else {
                vec![coord.x, coord.y]
            };
            obj.insert(
                "coordinates".to_string(),
                JsonValue::Array(coords.into_iter().map(JsonValue::from).collect()),
            );
            Ok(JsonValue::Object(obj))
        }
        Geometry::LineString(ls) => {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "type".to_string(),
                JsonValue::String("LineString".to_string()),
            );
            let coords: Vec<JsonValue> = ls
                .coords
                .iter()
                .map(|c| {
                    let arr = if c.has_z() {
                        vec![c.x, c.y, c.z.unwrap_or(0.0)]
                    } else {
                        vec![c.x, c.y]
                    };
                    JsonValue::Array(arr.into_iter().map(JsonValue::from).collect())
                })
                .collect();
            obj.insert("coordinates".to_string(), JsonValue::Array(coords));
            Ok(JsonValue::Object(obj))
        }
        Geometry::Polygon(p) => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), JsonValue::String("Polygon".to_string()));

            let mut rings = Vec::new();

            // Exterior ring
            let exterior_coords: Vec<JsonValue> = p
                .exterior
                .coords
                .iter()
                .map(|c| {
                    let arr = if c.has_z() {
                        vec![c.x, c.y, c.z.unwrap_or(0.0)]
                    } else {
                        vec![c.x, c.y]
                    };
                    JsonValue::Array(arr.into_iter().map(JsonValue::from).collect())
                })
                .collect();
            rings.push(JsonValue::Array(exterior_coords));

            // Interior rings (holes)
            for hole in &p.interiors {
                let hole_coords: Vec<JsonValue> = hole
                    .coords
                    .iter()
                    .map(|c| {
                        let arr = if c.has_z() {
                            vec![c.x, c.y, c.z.unwrap_or(0.0)]
                        } else {
                            vec![c.x, c.y]
                        };
                        JsonValue::Array(arr.into_iter().map(JsonValue::from).collect())
                    })
                    .collect();
                rings.push(JsonValue::Array(hole_coords));
            }

            obj.insert("coordinates".to_string(), JsonValue::Array(rings));
            Ok(JsonValue::Object(obj))
        }
        _ => Err(NodeError {
            code: "NOT_IMPLEMENTED".to_string(),
            message: "GeoJSON conversion not implemented for this geometry type".to_string(),
        }
        .into()),
    }
}

fn geometry_from_geojson(value: &JsonValue) -> Result<Geometry> {
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

    let coords = obj.get("coordinates").ok_or_else(|| NodeError {
        code: "INVALID_GEOJSON".to_string(),
        message: "Missing 'coordinates' field".to_string(),
    })?;

    match geom_type {
        "Point" => {
            let arr = coords.as_array().ok_or_else(|| NodeError {
                code: "INVALID_GEOJSON".to_string(),
                message: "Point coordinates must be an array".to_string(),
            })?;

            if arr.len() < 2 {
                return Err(NodeError {
                    code: "INVALID_COORDINATES".to_string(),
                    message: "Point must have at least 2 coordinates".to_string(),
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

            let coord = if arr.len() > 2 {
                let z = arr[2].as_f64().ok_or_else(|| NodeError {
                    code: "INVALID_COORDINATES".to_string(),
                    message: "Invalid z coordinate".to_string(),
                })?;
                CoreCoord::new_3d(x, y, z)
            } else {
                CoreCoord::new_2d(x, y)
            };

            Ok(Geometry::Point(Point::from_coord(coord)))
        }
        _ => Err(NodeError {
            code: "NOT_IMPLEMENTED".to_string(),
            message: format!("Geometry type '{}' not yet supported", geom_type),
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
