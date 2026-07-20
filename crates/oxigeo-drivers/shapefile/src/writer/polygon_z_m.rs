//! Writer helpers for Polygon / MultiPolygon shapes (2D, Z, and M variants).
//!
//! Converts OxiGeo polygon geometries into the appropriate `Shape` variant
//! based on the presence of Z/M coordinates on the coordinates.

use crate::error::{Result, ShapefileError};
use crate::shp::Shape;
use crate::shp::shapes::{MultiPartShape, MultiPartShapeM, MultiPartShapeZ, Point};
use oxigeo_core::vector::{MultiPolygon, Polygon};

/// Converts a core `Polygon` geometry to the correct `Shape` variant.
///
/// - Has Z  → `Shape::PolygonZ`
/// - Has M only → `Shape::PolygonM`
/// - 2D → `Shape::Polygon`
pub fn geometry_polygon_to_shape(polygon: &Polygon, has_z: bool, has_m: bool) -> Result<Shape> {
    let mut all_points: Vec<Point> = Vec::new();
    let mut parts: Vec<i32> = Vec::new();

    // Exterior ring
    parts.push(all_points.len() as i32);
    for coord in &polygon.exterior.coords {
        all_points.push(Point::new(coord.x, coord.y));
    }

    // Interior rings (holes)
    for interior in &polygon.interiors {
        parts.push(all_points.len() as i32);
        for coord in &interior.coords {
            all_points.push(Point::new(coord.x, coord.y));
        }
    }

    if all_points.is_empty() {
        return Err(ShapefileError::invalid_geometry(
            "Polygon must have at least one point",
        ));
    }

    if has_z {
        // Collect Z values in the same ring-flattened order
        let z_values: Vec<f64> = polygon
            .exterior
            .coords
            .iter()
            .chain(polygon.interiors.iter().flat_map(|r| r.coords.iter()))
            .map(|c| c.z.unwrap_or(0.0))
            .collect();

        let m_values_opt: Option<Vec<f64>> = if has_m {
            Some(
                polygon
                    .exterior
                    .coords
                    .iter()
                    .chain(polygon.interiors.iter().flat_map(|r| r.coords.iter()))
                    .map(|c| c.m.unwrap_or(0.0))
                    .collect(),
            )
        } else {
            None
        };

        let shape_z = MultiPartShapeZ::new(parts, all_points, z_values, m_values_opt)?;
        Ok(Shape::PolygonZ(shape_z))
    } else if has_m {
        let m_values: Vec<f64> = polygon
            .exterior
            .coords
            .iter()
            .chain(polygon.interiors.iter().flat_map(|r| r.coords.iter()))
            .map(|c| c.m.unwrap_or(0.0))
            .collect();

        let shape_m = MultiPartShapeM::new(parts, all_points, m_values)?;
        Ok(Shape::PolygonM(shape_m))
    } else {
        let shape = MultiPartShape::new(parts, all_points)?;
        Ok(Shape::Polygon(shape))
    }
}

/// Converts a core `MultiPolygon` geometry to the correct `Shape` variant.
///
/// All polygons in a MultiPolygon are flattened into a single multi-part
/// shape (consistent with the Shapefile spec which uses multi-ring polygons).
///
/// - Has Z  → `Shape::PolygonZ`
/// - Has M only → `Shape::PolygonM`
/// - 2D → `Shape::Polygon`
pub fn geometry_multipolygon_to_shape(
    multipolygon: &MultiPolygon,
    has_z: bool,
    has_m: bool,
) -> Result<Shape> {
    let mut all_points: Vec<Point> = Vec::new();
    let mut parts: Vec<i32> = Vec::new();

    for polygon in &multipolygon.polygons {
        // Exterior ring
        parts.push(all_points.len() as i32);
        for coord in &polygon.exterior.coords {
            all_points.push(Point::new(coord.x, coord.y));
        }

        // Interior rings
        for interior in &polygon.interiors {
            parts.push(all_points.len() as i32);
            for coord in &interior.coords {
                all_points.push(Point::new(coord.x, coord.y));
            }
        }
    }

    if all_points.is_empty() {
        return Err(ShapefileError::invalid_geometry(
            "MultiPolygon must have at least one point",
        ));
    }

    if has_z {
        let z_values: Vec<f64> = multipolygon
            .polygons
            .iter()
            .flat_map(|poly| {
                poly.exterior
                    .coords
                    .iter()
                    .chain(poly.interiors.iter().flat_map(|r| r.coords.iter()))
            })
            .map(|c| c.z.unwrap_or(0.0))
            .collect();

        let m_values_opt: Option<Vec<f64>> = if has_m {
            Some(
                multipolygon
                    .polygons
                    .iter()
                    .flat_map(|poly| {
                        poly.exterior
                            .coords
                            .iter()
                            .chain(poly.interiors.iter().flat_map(|r| r.coords.iter()))
                    })
                    .map(|c| c.m.unwrap_or(0.0))
                    .collect(),
            )
        } else {
            None
        };

        let shape_z = MultiPartShapeZ::new(parts, all_points, z_values, m_values_opt)?;
        Ok(Shape::PolygonZ(shape_z))
    } else if has_m {
        let m_values: Vec<f64> = multipolygon
            .polygons
            .iter()
            .flat_map(|poly| {
                poly.exterior
                    .coords
                    .iter()
                    .chain(poly.interiors.iter().flat_map(|r| r.coords.iter()))
            })
            .map(|c| c.m.unwrap_or(0.0))
            .collect();

        let shape_m = MultiPartShapeM::new(parts, all_points, m_values)?;
        Ok(Shape::PolygonM(shape_m))
    } else {
        let shape = MultiPartShape::new(parts, all_points)?;
        Ok(Shape::Polygon(shape))
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigeo_core::vector::{Coordinate, LineString};

    fn make_square_exterior_2d() -> LineString {
        LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ])
        .expect("valid exterior")
    }

    fn make_square_exterior_3d() -> LineString {
        LineString::new(vec![
            Coordinate::new_3d(0.0, 0.0, 1.0),
            Coordinate::new_3d(1.0, 0.0, 2.0),
            Coordinate::new_3d(1.0, 1.0, 3.0),
            Coordinate::new_3d(0.0, 1.0, 4.0),
            Coordinate::new_3d(0.0, 0.0, 1.0),
        ])
        .expect("valid 3D exterior")
    }

    #[test]
    fn test_polygon_2d() {
        let exterior = make_square_exterior_2d();
        let poly = Polygon::new(exterior, vec![]).expect("valid polygon");
        let shape = geometry_polygon_to_shape(&poly, false, false).expect("2D polygon");
        assert!(matches!(shape, Shape::Polygon(_)));
    }

    #[test]
    fn test_polygon_z_shape_type() {
        let exterior = make_square_exterior_3d();
        let poly = Polygon::new(exterior, vec![]).expect("valid 3D polygon");
        let shape = geometry_polygon_to_shape(&poly, true, false).expect("PolygonZ");

        if let Shape::PolygonZ(sz) = shape {
            assert_eq!(sz.base.num_points, 5);
            assert_eq!(sz.z_values.len(), 5);
            // Verify shape type byte in serialized form
            use crate::shp::ShapeRecord;
            let mut buf = Vec::new();
            let record = ShapeRecord::new(1, Shape::PolygonZ(sz));
            record.write(&mut buf).expect("write PolygonZ");
            let shape_type = i32::from_le_bytes(buf[8..12].try_into().expect("4 bytes"));
            assert_eq!(shape_type, 15, "PolygonZ shape type must be 15");
        } else {
            panic!("Expected PolygonZ, got {:?}", shape);
        }
    }

    #[test]
    fn test_polygon_m_shape_type() {
        let exterior = LineString::new(vec![
            Coordinate::new_2dm(0.0, 0.0, 0.5),
            Coordinate::new_2dm(1.0, 0.0, 0.5),
            Coordinate::new_2dm(1.0, 1.0, 0.5),
            Coordinate::new_2dm(0.0, 1.0, 0.5),
            Coordinate::new_2dm(0.0, 0.0, 0.5),
        ])
        .expect("valid M exterior");
        let poly = Polygon::new(exterior, vec![]).expect("valid polygon M");
        let shape = geometry_polygon_to_shape(&poly, false, true).expect("PolygonM");

        if let Shape::PolygonM(sm) = shape {
            assert_eq!(sm.base.num_points, 5);
            use crate::shp::ShapeRecord;
            let mut buf = Vec::new();
            let record = ShapeRecord::new(1, Shape::PolygonM(sm));
            record.write(&mut buf).expect("write PolygonM");
            let shape_type = i32::from_le_bytes(buf[8..12].try_into().expect("4 bytes"));
            assert_eq!(shape_type, 25, "PolygonM shape type must be 25");
        } else {
            panic!("Expected PolygonM, got {:?}", shape);
        }
    }
}
