//! Conversion utilities for GeoJSON types
//!
//! This module provides utilities for converting between different GeoJSON
//! types and coordinate systems.

use crate::error::Result;
use crate::types::*;

/// Converts a Geometry to its bounding box
pub fn geometry_to_bbox(geometry: &Geometry) -> Option<BBox> {
    geometry.compute_bbox()
}

/// Converts a FeatureCollection to a bounding box that encompasses all features
pub fn feature_collection_to_bbox(fc: &FeatureCollection) -> Option<BBox> {
    if fc.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for feature in &fc.features {
        if let Some(ref geometry) = feature.geometry {
            if let Some(bbox) = geometry.compute_bbox() {
                if bbox.len() >= 4 {
                    min_x = min_x.min(bbox[0]);
                    min_y = min_y.min(bbox[1]);
                    max_x = max_x.max(bbox[2]);
                    max_y = max_y.max(bbox[3]);
                }
            }
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some(vec![min_x, min_y, max_x, max_y])
    } else {
        None
    }
}

/// Converts a Point to a Position
pub fn point_to_position(point: &Point) -> Position {
    point.coordinates.clone()
}

/// Converts a Position to a Point
pub fn position_to_point(pos: Position) -> Result<Point> {
    Point::new(pos)
}

/// Extracts all coordinates from a Geometry
pub fn extract_coordinates(geometry: &Geometry) -> Vec<Position> {
    match geometry {
        Geometry::Point(p) => vec![p.coordinates.clone()],
        Geometry::LineString(ls) => ls.coordinates.clone(),
        Geometry::Polygon(p) => p.coordinates.iter().flatten().cloned().collect(),
        Geometry::MultiPoint(mp) => mp.coordinates.clone(),
        Geometry::MultiLineString(mls) => mls.coordinates.iter().flatten().cloned().collect(),
        Geometry::MultiPolygon(mp) => mp.coordinates.iter().flatten().flatten().cloned().collect(),
        Geometry::GeometryCollection(gc) => {
            gc.geometries.iter().flat_map(extract_coordinates).collect()
        }
    }
}

/// Counts the total number of coordinates in a Geometry
pub fn count_coordinates(geometry: &Geometry) -> usize {
    match geometry {
        Geometry::Point(_) => 1,
        Geometry::LineString(ls) => ls.coordinates.len(),
        Geometry::Polygon(p) => p.coordinates.iter().map(|ring| ring.len()).sum(),
        Geometry::MultiPoint(mp) => mp.coordinates.len(),
        Geometry::MultiLineString(mls) => mls.coordinates.iter().map(|line| line.len()).sum(),
        Geometry::MultiPolygon(mp) => mp
            .coordinates
            .iter()
            .map(|poly| poly.iter().map(|ring| ring.len()).sum::<usize>())
            .sum(),
        Geometry::GeometryCollection(gc) => gc.geometries.iter().map(count_coordinates).sum(),
    }
}

/// Converts a simple geometry to a multi-geometry
pub fn to_multi(geometry: Geometry) -> Geometry {
    match geometry {
        Geometry::Point(p) => {
            Geometry::MultiPoint(MultiPoint::new(vec![p.coordinates]).expect("valid multipoint"))
        }
        Geometry::LineString(ls) => Geometry::MultiLineString(
            MultiLineString::new(vec![ls.coordinates]).expect("valid multilinestring"),
        ),
        Geometry::Polygon(p) => Geometry::MultiPolygon(
            MultiPolygon::new(vec![p.coordinates]).expect("valid multipolygon"),
        ),
        other => other, // Already multi or GeometryCollection
    }
}

/// Attempts to convert a multi-geometry to a simple geometry
/// Returns None if the multi-geometry contains more than one element
pub fn to_single(geometry: Geometry) -> Option<Geometry> {
    match geometry {
        Geometry::MultiPoint(mp) if mp.coordinates.len() == 1 => {
            Point::new(mp.coordinates[0].clone())
                .ok()
                .map(Geometry::Point)
        }
        Geometry::MultiLineString(mls) if mls.coordinates.len() == 1 => {
            LineString::new(mls.coordinates[0].clone())
                .ok()
                .map(Geometry::LineString)
        }
        Geometry::MultiPolygon(mp) if mp.coordinates.len() == 1 => {
            Polygon::new(mp.coordinates[0].clone())
                .ok()
                .map(Geometry::Polygon)
        }
        other => Some(other),
    }
}

/// Flattens a GeometryCollection into individual geometries
pub fn flatten_collection(geometry: Geometry) -> Vec<Geometry> {
    match geometry {
        Geometry::GeometryCollection(gc) => {
            let mut result = Vec::new();
            for geom in gc.geometries {
                result.extend(flatten_collection(geom));
            }
            result
        }
        other => vec![other],
    }
}

/// Merges multiple FeatureCollections into one
pub fn merge_feature_collections(collections: Vec<FeatureCollection>) -> FeatureCollection {
    let mut result = FeatureCollection::empty();

    for fc in collections {
        result.add_features(fc.features);
    }

    result
}

/// Splits a FeatureCollection into multiple collections by a property value
pub fn split_by_property(
    fc: &FeatureCollection,
    property_name: &str,
) -> Vec<(serde_json::Value, FeatureCollection)> {
    use std::collections::HashMap;

    let mut groups: HashMap<String, Vec<Feature>> = HashMap::new();

    for feature in &fc.features {
        if let Some(value) = feature.get_property(property_name) {
            let key = value.to_string();
            groups.entry(key.clone()).or_default().push(feature.clone());
        }
    }

    groups
        .into_iter()
        .map(|(key, features)| {
            let value: serde_json::Value =
                serde_json::from_str(&key).unwrap_or(serde_json::Value::String(key));
            (value, FeatureCollection::new(features))
        })
        .collect()
}

/// Creates a Feature from a Geometry with no properties
pub fn geometry_to_feature(geometry: Geometry) -> Feature {
    Feature::new(Some(geometry), None)
}

/// Extracts all geometries from a FeatureCollection
pub fn extract_geometries(fc: &FeatureCollection) -> Vec<Geometry> {
    fc.features
        .iter()
        .filter_map(|f| f.geometry.clone())
        .collect()
}

/// Creates a FeatureCollection from a list of geometries
pub fn geometries_to_feature_collection(geometries: Vec<Geometry>) -> FeatureCollection {
    let features = geometries
        .into_iter()
        .map(|g| Feature::new(Some(g), None))
        .collect();
    FeatureCollection::new(features)
}

/// Filters features by geometry type
pub fn filter_by_geometry_type(
    fc: &FeatureCollection,
    geometry_type: GeometryType,
) -> FeatureCollection {
    let features = fc
        .features
        .iter()
        .filter(|f| {
            f.geometry
                .as_ref()
                .is_some_and(|g| g.geometry_type() == geometry_type)
        })
        .cloned()
        .collect();
    FeatureCollection::new(features)
}

/// Computes statistics for a numeric property across all features
pub fn property_statistics(fc: &FeatureCollection, property_name: &str) -> PropertyStats {
    let mut values = Vec::new();

    for feature in &fc.features {
        if let Some(value) = feature.get_property(property_name) {
            if let Some(num) = value.as_f64() {
                values.push(num);
            } else if let Some(num) = value.as_i64() {
                values.push(num as f64);
            }
        }
    }

    if values.is_empty() {
        return PropertyStats::default();
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let count = values.len();
    let min = values[0];
    let max = values[count - 1];
    let sum: f64 = values.iter().sum();
    let mean = sum / count as f64;

    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
    let std_dev = variance.sqrt();

    let median = if count % 2 == 0 {
        (values[count / 2 - 1] + values[count / 2]) / 2.0
    } else {
        values[count / 2]
    };

    PropertyStats {
        count,
        min,
        max,
        mean,
        median,
        std_dev,
        sum,
    }
}

/// Statistics for a numeric property
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropertyStats {
    /// Number of values
    pub count: usize,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Mean (average) value
    pub mean: f64,
    /// Median value
    pub median: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Sum of all values
    pub sum: f64,
}

impl Default for PropertyStats {
    fn default() -> Self {
        Self {
            count: 0,
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            median: 0.0,
            std_dev: 0.0,
            sum: 0.0,
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_point_conversion() {
        let pos = vec![100.0, 0.0];
        let point = position_to_point(pos.clone()).expect("valid point");
        let extracted = point_to_position(&point);
        assert_eq!(extracted, pos);
    }

    #[test]
    fn test_to_multi() {
        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        let geom = Geometry::Point(point);
        let multi = to_multi(geom);

        assert!(matches!(multi, Geometry::MultiPoint(_)));
    }

    #[test]
    fn test_to_single() {
        let mp = MultiPoint::new(vec![vec![0.0, 0.0]]).expect("valid multipoint");
        let geom = Geometry::MultiPoint(mp);
        let single = to_single(geom);

        assert!(single.is_some());
        if let Some(Geometry::Point(_)) = single {
            // Success
        } else {
            panic!("Expected Point");
        }
    }

    #[test]
    fn test_count_coordinates() {
        let ls = LineString::new(vec![vec![0.0, 0.0], vec![1.0, 1.0], vec![2.0, 2.0]])
            .expect("valid linestring");
        let geom = Geometry::LineString(ls);

        assert_eq!(count_coordinates(&geom), 3);
    }

    #[test]
    fn test_extract_coordinates() {
        let ls = LineString::new(vec![vec![0.0, 0.0], vec![1.0, 1.0]]).expect("valid linestring");
        let geom = Geometry::LineString(ls);

        let coords = extract_coordinates(&geom);
        assert_eq!(coords.len(), 2);
    }

    #[test]
    fn test_flatten_collection() {
        let point = Geometry::Point(Point::new_2d(0.0, 0.0).expect("valid point"));
        let line = Geometry::LineString(
            LineString::new(vec![vec![1.0, 1.0], vec![2.0, 2.0]]).expect("valid linestring"),
        );

        let collection = Geometry::GeometryCollection(
            GeometryCollection::new(vec![point, line]).expect("valid collection"),
        );

        let flattened = flatten_collection(collection);
        assert_eq!(flattened.len(), 2);
    }

    #[test]
    fn test_merge_feature_collections() {
        let fc1 = FeatureCollection::new(vec![Feature::default()]);
        let fc2 = FeatureCollection::new(vec![Feature::default(), Feature::default()]);

        let merged = merge_feature_collections(vec![fc1, fc2]);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn test_filter_by_geometry_type() {
        let mut fc = FeatureCollection::empty();

        fc.add_feature(Feature::new(
            Some(Geometry::Point(
                Point::new_2d(0.0, 0.0).expect("valid point"),
            )),
            None,
        ));
        fc.add_feature(Feature::new(
            Some(Geometry::LineString(
                LineString::new(vec![vec![0.0, 0.0], vec![1.0, 1.0]]).expect("valid linestring"),
            )),
            None,
        ));

        let points = filter_by_geometry_type(&fc, GeometryType::Point);
        assert_eq!(points.len(), 1);
    }

    #[test]
    fn test_property_statistics() {
        let mut fc = FeatureCollection::empty();

        for i in 1..=10 {
            let point = Point::new_2d(0.0, 0.0).expect("valid point");
            let mut feature = Feature::new(Some(Geometry::Point(point)), None);
            feature.add_property("value", i);
            fc.add_feature(feature);
        }

        let stats = property_statistics(&fc, "value");
        assert_eq!(stats.count, 10);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 10.0);
        assert_eq!(stats.mean, 5.5);
    }
}
