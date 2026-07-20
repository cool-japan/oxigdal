//! Coordinate transformation utilities
//!
//! This module provides utilities for transforming coordinates between
//! different coordinate reference systems and applying various transformations.

use crate::error::Result;
use crate::types::*;

/// Affine transformation matrix (2D)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AffineTransform {
    /// Scale factor in X
    pub a: f64,
    /// Rotation/skew in X
    pub b: f64,
    /// Translation in X
    pub c: f64,
    /// Rotation/skew in Y
    pub d: f64,
    /// Scale factor in Y
    pub e: f64,
    /// Translation in Y
    pub f: f64,
}

impl AffineTransform {
    /// Creates a new affine transform
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// Creates an identity transform (no change)
    pub const fn identity() -> Self {
        Self::new(1.0, 0.0, 0.0, 0.0, 1.0, 0.0)
    }

    /// Creates a translation transform
    pub const fn translate(dx: f64, dy: f64) -> Self {
        Self::new(1.0, 0.0, dx, 0.0, 1.0, dy)
    }

    /// Creates a scale transform
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, 0.0, sy, 0.0)
    }

    /// Creates a rotation transform (angle in radians)
    pub fn rotate(angle: f64) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self::new(cos, -sin, 0.0, sin, cos, 0.0)
    }

    /// Applies the transform to a position
    pub fn apply(&self, pos: &Position) -> Position {
        if pos.len() < 2 {
            return pos.clone();
        }

        let x = pos[0];
        let y = pos[1];

        let new_x = self.a * x + self.b * y + self.c;
        let new_y = self.d * x + self.e * y + self.f;

        let mut result = vec![new_x, new_y];
        if pos.len() > 2 {
            result.extend_from_slice(&pos[2..]);
        }

        result
    }

    /// Composes two transforms
    pub fn compose(&self, other: &Self) -> Self {
        Self::new(
            self.a * other.a + self.b * other.d,
            self.a * other.b + self.b * other.e,
            self.a * other.c + self.b * other.f + self.c,
            self.d * other.a + self.e * other.d,
            self.d * other.b + self.e * other.e,
            self.d * other.c + self.e * other.f + self.f,
        )
    }

    /// Computes the inverse transform
    pub fn inverse(&self) -> Option<Self> {
        let det = self.a * self.e - self.b * self.d;
        if det.abs() < 1e-10 {
            return None; // Singular matrix
        }

        let inv_det = 1.0 / det;

        Some(Self::new(
            self.e * inv_det,
            -self.b * inv_det,
            (self.b * self.f - self.c * self.e) * inv_det,
            -self.d * inv_det,
            self.a * inv_det,
            (self.c * self.d - self.a * self.f) * inv_det,
        ))
    }
}

/// Applies an affine transform to a coordinate sequence
pub fn transform_coordinates(coords: &[Position], transform: &AffineTransform) -> Vec<Position> {
    coords.iter().map(|pos| transform.apply(pos)).collect()
}

/// Transforms a Point
pub fn transform_point(point: &Point, transform: &AffineTransform) -> Result<Point> {
    Point::new(transform.apply(&point.coordinates))
}

/// Transforms a LineString
pub fn transform_linestring(
    linestring: &LineString,
    transform: &AffineTransform,
) -> Result<LineString> {
    let transformed = transform_coordinates(&linestring.coordinates, transform);
    LineString::new(transformed)
}

/// Transforms a Polygon
pub fn transform_polygon(polygon: &Polygon, transform: &AffineTransform) -> Result<Polygon> {
    let transformed_rings: Vec<_> = polygon
        .coordinates
        .iter()
        .map(|ring| transform_coordinates(ring, transform))
        .collect();

    Polygon::new(transformed_rings)
}

/// Transforms a MultiPoint
pub fn transform_multipoint(
    multipoint: &MultiPoint,
    transform: &AffineTransform,
) -> Result<MultiPoint> {
    let transformed = transform_coordinates(&multipoint.coordinates, transform);
    MultiPoint::new(transformed)
}

/// Transforms a MultiLineString
pub fn transform_multilinestring(
    mls: &MultiLineString,
    transform: &AffineTransform,
) -> Result<MultiLineString> {
    let transformed_lines: Vec<_> = mls
        .coordinates
        .iter()
        .map(|line| transform_coordinates(line, transform))
        .collect();

    MultiLineString::new(transformed_lines)
}

/// Transforms a MultiPolygon
pub fn transform_multipolygon(
    mp: &MultiPolygon,
    transform: &AffineTransform,
) -> Result<MultiPolygon> {
    let transformed_polygons: Vec<_> = mp
        .coordinates
        .iter()
        .map(|polygon| {
            polygon
                .iter()
                .map(|ring| transform_coordinates(ring, transform))
                .collect()
        })
        .collect();

    MultiPolygon::new(transformed_polygons)
}

/// Transforms any Geometry
pub fn transform_geometry(geometry: &Geometry, transform: &AffineTransform) -> Result<Geometry> {
    match geometry {
        Geometry::Point(p) => Ok(Geometry::Point(transform_point(p, transform)?)),
        Geometry::LineString(ls) => Ok(Geometry::LineString(transform_linestring(ls, transform)?)),
        Geometry::Polygon(p) => Ok(Geometry::Polygon(transform_polygon(p, transform)?)),
        Geometry::MultiPoint(mp) => Ok(Geometry::MultiPoint(transform_multipoint(mp, transform)?)),
        Geometry::MultiLineString(mls) => Ok(Geometry::MultiLineString(transform_multilinestring(
            mls, transform,
        )?)),
        Geometry::MultiPolygon(mp) => Ok(Geometry::MultiPolygon(transform_multipolygon(
            mp, transform,
        )?)),
        Geometry::GeometryCollection(gc) => {
            let transformed_geometries: Result<Vec<_>> = gc
                .geometries
                .iter()
                .map(|g| transform_geometry(g, transform))
                .collect();
            Ok(Geometry::GeometryCollection(GeometryCollection::new(
                transformed_geometries?,
            )?))
        }
    }
}

/// Transforms a Feature
pub fn transform_feature(feature: &Feature, transform: &AffineTransform) -> Result<Feature> {
    let transformed_geometry = if let Some(ref geom) = feature.geometry {
        Some(transform_geometry(geom, transform)?)
    } else {
        None
    };

    let mut transformed_feature = Feature::new(transformed_geometry, feature.properties.clone());
    transformed_feature.id = feature.id.clone();
    transformed_feature.bbox = feature.bbox.clone();
    transformed_feature.crs = feature.crs.clone();

    Ok(transformed_feature)
}

/// Transforms a FeatureCollection
pub fn transform_feature_collection(
    fc: &FeatureCollection,
    transform: &AffineTransform,
) -> Result<FeatureCollection> {
    let transformed_features: Result<Vec<_>> = fc
        .features
        .iter()
        .map(|f| transform_feature(f, transform))
        .collect();

    let mut transformed_fc = FeatureCollection::new(transformed_features?);
    transformed_fc.bbox = fc.bbox.clone();
    transformed_fc.crs = fc.crs.clone();

    Ok(transformed_fc)
}

/// Computes the centroid of a coordinate sequence
pub fn compute_centroid(coords: &[Position]) -> Option<Position> {
    if coords.is_empty() {
        return None;
    }

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0;

    for pos in coords {
        if pos.len() >= 2 {
            sum_x += pos[0];
            sum_y += pos[1];
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }

    Some(vec![sum_x / count as f64, sum_y / count as f64])
}

/// Computes the centroid of a Geometry
pub fn geometry_centroid(geometry: &Geometry) -> Option<Position> {
    match geometry {
        Geometry::Point(p) => Some(p.coordinates.clone()),
        Geometry::LineString(ls) => compute_centroid(&ls.coordinates),
        Geometry::Polygon(p) => {
            if let Some(exterior) = p.exterior() {
                compute_centroid(exterior)
            } else {
                None
            }
        }
        Geometry::MultiPoint(mp) => compute_centroid(&mp.coordinates),
        Geometry::MultiLineString(mls) => {
            let all_coords: Vec<_> = mls.coordinates.iter().flatten().cloned().collect();
            compute_centroid(&all_coords)
        }
        Geometry::MultiPolygon(mp) => {
            let all_coords: Vec<_> = mp.coordinates.iter().flatten().flatten().cloned().collect();
            compute_centroid(&all_coords)
        }
        Geometry::GeometryCollection(gc) => {
            let all_centroids: Vec<_> =
                gc.geometries.iter().filter_map(geometry_centroid).collect();
            compute_centroid(&all_centroids)
        }
    }
}

/// Flips coordinates (swaps X and Y)
pub fn flip_coordinates(pos: &Position) -> Position {
    if pos.len() < 2 {
        return pos.clone();
    }

    let mut flipped = vec![pos[1], pos[0]];
    if pos.len() > 2 {
        flipped.extend_from_slice(&pos[2..]);
    }

    flipped
}

/// Flips all coordinates in a coordinate sequence
pub fn flip_coordinate_sequence(coords: &[Position]) -> Vec<Position> {
    coords.iter().map(flip_coordinates).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_transform() {
        let transform = AffineTransform::identity();
        let pos = vec![100.0, 50.0];
        let transformed = transform.apply(&pos);

        assert_eq!(transformed[0], 100.0);
        assert_eq!(transformed[1], 50.0);
    }

    #[test]
    fn test_translate() {
        let transform = AffineTransform::translate(10.0, 20.0);
        let pos = vec![100.0, 50.0];
        let transformed = transform.apply(&pos);

        assert_eq!(transformed[0], 110.0);
        assert_eq!(transformed[1], 70.0);
    }

    #[test]
    fn test_scale() {
        let transform = AffineTransform::scale(2.0, 3.0);
        let pos = vec![10.0, 10.0];
        let transformed = transform.apply(&pos);

        assert_eq!(transformed[0], 20.0);
        assert_eq!(transformed[1], 30.0);
    }

    #[test]
    fn test_rotate() {
        let transform = AffineTransform::rotate(std::f64::consts::PI / 2.0); // 90 degrees
        let pos = vec![1.0, 0.0];
        let transformed = transform.apply(&pos);

        assert!((transformed[0] - 0.0).abs() < 1e-10);
        assert!((transformed[1] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_compose() {
        let t1 = AffineTransform::translate(10.0, 0.0);
        let t2 = AffineTransform::scale(2.0, 2.0);
        let composed = t1.compose(&t2);

        let pos = vec![5.0, 5.0];
        let transformed = composed.apply(&pos);

        assert_eq!(transformed[0], 20.0); // (5 + 10) * 2
        assert_eq!(transformed[1], 10.0); // 5 * 2
    }

    #[test]
    fn test_inverse() {
        let transform = AffineTransform::translate(10.0, 20.0);
        let inverse = transform.inverse();

        assert!(inverse.is_some());
        let inv = inverse.expect("inverse exists");

        let pos = vec![100.0, 50.0];
        let transformed = transform.apply(&pos);
        let back = inv.apply(&transformed);

        assert!((back[0] - 100.0).abs() < 1e-10);
        assert!((back[1] - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_transform_point() {
        let point = Point::new_2d(10.0, 20.0).expect("valid point");
        let transform = AffineTransform::translate(5.0, 5.0);
        let transformed = transform_point(&point, &transform).expect("transform succeeded");

        assert_eq!(transformed.longitude(), Some(15.0));
        assert_eq!(transformed.latitude(), Some(25.0));
    }

    #[test]
    fn test_compute_centroid() {
        let coords = vec![
            vec![0.0, 0.0],
            vec![10.0, 0.0],
            vec![10.0, 10.0],
            vec![0.0, 10.0],
        ];

        let centroid = compute_centroid(&coords);
        assert!(centroid.is_some());

        let c = centroid.expect("centroid exists");
        assert_eq!(c[0], 5.0);
        assert_eq!(c[1], 5.0);
    }

    #[test]
    fn test_flip_coordinates() {
        let pos = vec![100.0, 50.0];
        let flipped = flip_coordinates(&pos);

        assert_eq!(flipped[0], 50.0);
        assert_eq!(flipped[1], 100.0);
    }

    #[test]
    fn test_geometry_centroid() {
        let point = Point::new_2d(10.0, 20.0).expect("valid point");
        let geometry = Geometry::Point(point);

        let centroid = geometry_centroid(&geometry);
        assert!(centroid.is_some());

        let c = centroid.expect("centroid exists");
        assert_eq!(c[0], 10.0);
        assert_eq!(c[1], 20.0);
    }
}
