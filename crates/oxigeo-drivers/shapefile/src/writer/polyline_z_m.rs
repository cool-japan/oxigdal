//! Writer helpers for PolyLine / MultiLineString shapes (2D, Z, and M variants).
//!
//! Converts OxiGeo `LineString` and `MultiLineString` geometries into the
//! appropriate `Shape` variant based on the presence of Z/M coordinates.

use crate::error::{Result, ShapefileError};
use crate::shp::Shape;
use crate::shp::shapes::{MultiPartShape, MultiPartShapeM, MultiPartShapeZ, Point};
use oxigeo_core::vector::{LineString, MultiLineString};

/// Converts a core `LineString` geometry to the correct `Shape` variant.
///
/// - Has Z  → `Shape::PolyLineZ`
/// - Has M only → `Shape::PolyLineM`
/// - 2D → `Shape::PolyLine`
pub fn geometry_linestring_to_shape(ls: &LineString, has_z: bool, has_m: bool) -> Result<Shape> {
    if ls.coords.is_empty() {
        return Err(ShapefileError::invalid_geometry(
            "LineString must have at least one point",
        ));
    }

    let points: Vec<Point> = ls.coords.iter().map(|c| Point::new(c.x, c.y)).collect();
    let parts = vec![0i32]; // single part

    if has_z {
        let z_values: Vec<f64> = ls.coords.iter().map(|c| c.z.unwrap_or(0.0)).collect();
        let m_values_opt: Option<Vec<f64>> = if has_m {
            Some(ls.coords.iter().map(|c| c.m.unwrap_or(0.0)).collect())
        } else {
            None
        };
        let shape_z = MultiPartShapeZ::new(parts, points, z_values, m_values_opt)?;
        Ok(Shape::PolyLineZ(shape_z))
    } else if has_m {
        let m_values: Vec<f64> = ls.coords.iter().map(|c| c.m.unwrap_or(0.0)).collect();
        let shape_m = MultiPartShapeM::new(parts, points, m_values)?;
        Ok(Shape::PolyLineM(shape_m))
    } else {
        let shape = MultiPartShape::new(parts, points)?;
        Ok(Shape::PolyLine(shape))
    }
}

/// Converts a core `MultiLineString` geometry to the correct `Shape` variant.
///
/// - Has Z  → `Shape::PolyLineZ`
/// - Has M only → `Shape::PolyLineM`
/// - 2D → `Shape::PolyLine`
pub fn geometry_multilinestring_to_shape(
    mls: &MultiLineString,
    has_z: bool,
    has_m: bool,
) -> Result<Shape> {
    let mut all_points: Vec<Point> = Vec::new();
    let mut parts: Vec<i32> = Vec::new();

    for linestring in &mls.line_strings {
        parts.push(all_points.len() as i32);
        for coord in &linestring.coords {
            all_points.push(Point::new(coord.x, coord.y));
        }
    }

    if all_points.is_empty() {
        return Err(ShapefileError::invalid_geometry(
            "MultiLineString must have at least one point",
        ));
    }

    if has_z {
        let z_values: Vec<f64> = mls
            .line_strings
            .iter()
            .flat_map(|ls| ls.coords.iter())
            .map(|c| c.z.unwrap_or(0.0))
            .collect();

        let m_values_opt: Option<Vec<f64>> = if has_m {
            Some(
                mls.line_strings
                    .iter()
                    .flat_map(|ls| ls.coords.iter())
                    .map(|c| c.m.unwrap_or(0.0))
                    .collect(),
            )
        } else {
            None
        };

        let shape_z = MultiPartShapeZ::new(parts, all_points, z_values, m_values_opt)?;
        Ok(Shape::PolyLineZ(shape_z))
    } else if has_m {
        let m_values: Vec<f64> = mls
            .line_strings
            .iter()
            .flat_map(|ls| ls.coords.iter())
            .map(|c| c.m.unwrap_or(0.0))
            .collect();

        let shape_m = MultiPartShapeM::new(parts, all_points, m_values)?;
        Ok(Shape::PolyLineM(shape_m))
    } else {
        let shape = MultiPartShape::new(parts, all_points)?;
        Ok(Shape::PolyLine(shape))
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::shp::ShapeRecord;
    use oxigeo_core::vector::Coordinate;

    #[test]
    fn test_linestring_2d() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 1.0)];
        let ls = LineString::new(coords).expect("valid linestring");
        let shape = geometry_linestring_to_shape(&ls, false, false).expect("2D polyline");
        assert!(matches!(shape, Shape::PolyLine(_)));
    }

    #[test]
    fn test_polyline_z_shape_type() {
        let coords = vec![
            Coordinate::new_3d(0.0, 0.0, 10.0),
            Coordinate::new_3d(1.0, 1.0, 20.0),
            Coordinate::new_3d(2.0, 0.0, 15.0),
        ];
        let ls = LineString::new(coords).expect("valid 3D linestring");
        let shape = geometry_linestring_to_shape(&ls, true, false).expect("PolyLineZ");

        if let Shape::PolyLineZ(sz) = shape {
            assert_eq!(sz.z_values.len(), 3);
            let mut buf = Vec::new();
            let record = ShapeRecord::new(1, Shape::PolyLineZ(sz));
            record.write(&mut buf).expect("write PolyLineZ");
            let shape_type = i32::from_le_bytes(buf[8..12].try_into().expect("4 bytes"));
            assert_eq!(shape_type, 13, "PolyLineZ shape type must be 13");
        } else {
            panic!("Expected PolyLineZ");
        }
    }

    #[test]
    fn test_polyline_m_shape_type() {
        let coords = vec![
            Coordinate::new_2dm(0.0, 0.0, 0.0),
            Coordinate::new_2dm(1.0, 1.0, 1.0),
        ];
        let ls = LineString::new(coords).expect("valid M linestring");
        let shape = geometry_linestring_to_shape(&ls, false, true).expect("PolyLineM");

        if let Shape::PolyLineM(sm) = shape {
            assert_eq!(sm.m_values.len(), 2);
            let mut buf = Vec::new();
            let record = ShapeRecord::new(1, Shape::PolyLineM(sm));
            record.write(&mut buf).expect("write PolyLineM");
            let shape_type = i32::from_le_bytes(buf[8..12].try_into().expect("4 bytes"));
            assert_eq!(shape_type, 23, "PolyLineM shape type must be 23");
        } else {
            panic!("Expected PolyLineM");
        }
    }

    #[test]
    fn test_multilinestring_z_multiple_parts() {
        use oxigeo_core::vector::MultiLineString;

        let ls1 = LineString::new(vec![
            Coordinate::new_3d(0.0, 0.0, 1.0),
            Coordinate::new_3d(1.0, 0.0, 2.0),
        ])
        .expect("ls1");
        let ls2 = LineString::new(vec![
            Coordinate::new_3d(2.0, 0.0, 3.0),
            Coordinate::new_3d(3.0, 0.0, 4.0),
        ])
        .expect("ls2");
        let mls = MultiLineString::new(vec![ls1, ls2]);

        let shape = geometry_multilinestring_to_shape(&mls, true, false).expect("MultiPolyLineZ");

        if let Shape::PolyLineZ(sz) = shape {
            assert_eq!(sz.base.num_parts, 2, "two parts");
            assert_eq!(sz.base.num_points, 4, "four points total");
            assert_eq!(sz.z_values.len(), 4);
            // Check shape type byte
            let mut buf = Vec::new();
            let record = ShapeRecord::new(1, Shape::PolyLineZ(sz));
            record.write(&mut buf).expect("write");
            let shape_type = i32::from_le_bytes(buf[8..12].try_into().expect("4b"));
            assert_eq!(shape_type, 13);
        } else {
            panic!("Expected PolyLineZ");
        }
    }
}
