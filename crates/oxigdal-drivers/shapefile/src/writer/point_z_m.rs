//! Writer helpers for Point, PointZ, and PointM shapes.
//!
//! Converts OxiGDAL `Point` and `MultiPoint` geometries (from oxigdal-core)
//! into the appropriate `Shape` variant based on the presence of Z/M coordinates.

use crate::error::{Result, ShapefileError};
use crate::shp::Shape;
use crate::shp::shapes::{MultiPartShape, MultiPartShapeM, MultiPartShapeZ, Point, PointM, PointZ};
use oxigdal_core::vector::{MultiPoint, Point as CorePoint};

/// Converts a core `Point` geometry to the correct `Shape` variant.
///
/// - Has Z  → `Shape::PointZ`
/// - Has M only → `Shape::PointM`
/// - 2D → `Shape::Point`
pub fn geometry_point_to_shape(p: &CorePoint, has_z: bool, has_m: bool) -> Result<Shape> {
    if has_z {
        let z = p.coord.z.unwrap_or(0.0);
        let m = p.coord.m;
        Ok(Shape::PointZ(PointZ::new_with_m_opt(
            p.coord.x, p.coord.y, z, m,
        )))
    } else if has_m {
        let m = p.coord.m.unwrap_or(0.0);
        Ok(Shape::PointM(PointM::new(p.coord.x, p.coord.y, m)))
    } else {
        Ok(Shape::Point(Point::new(p.coord.x, p.coord.y)))
    }
}

/// Converts a core `MultiPoint` geometry to the correct `Shape` variant.
///
/// - Has Z  → `Shape::MultiPointZ`
/// - Has M only → `Shape::MultiPointM`
/// - 2D → `Shape::MultiPoint`
pub fn geometry_multipoint_to_shape(mp: &MultiPoint, has_z: bool, has_m: bool) -> Result<Shape> {
    if mp.points.is_empty() {
        return Err(ShapefileError::invalid_geometry(
            "MultiPoint must have at least one point",
        ));
    }

    let shp_points: Vec<Point> = mp
        .points
        .iter()
        .map(|p| Point::new(p.coord.x, p.coord.y))
        .collect();

    // parts: one entry per point (MultiPoint part array convention)
    let parts: Vec<i32> = (0..shp_points.len() as i32).collect();

    if has_z {
        let z_values: Vec<f64> = mp.points.iter().map(|p| p.coord.z.unwrap_or(0.0)).collect();
        let m_values_opt: Option<Vec<f64>> = if has_m {
            Some(mp.points.iter().map(|p| p.coord.m.unwrap_or(0.0)).collect())
        } else {
            None
        };
        let shape_z = MultiPartShapeZ::new(parts, shp_points, z_values, m_values_opt)?;
        Ok(Shape::MultiPointZ(shape_z))
    } else if has_m {
        let m_values: Vec<f64> = mp.points.iter().map(|p| p.coord.m.unwrap_or(0.0)).collect();
        let shape_m = MultiPartShapeM::new(parts, shp_points, m_values)?;
        Ok(Shape::MultiPointM(shape_m))
    } else {
        let shape = MultiPartShape::new(parts, shp_points)?;
        Ok(Shape::MultiPoint(shape))
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigdal_core::vector::Coordinate;

    #[test]
    fn test_point_2d() {
        let p = CorePoint::new(1.0, 2.0);
        let shape = geometry_point_to_shape(&p, false, false).expect("2D point");
        assert!(matches!(shape, Shape::Point(_)));
    }

    #[test]
    fn test_point_z() {
        let p = CorePoint::from_coord(Coordinate::new_3d(1.0, 2.0, 3.0));
        let shape = geometry_point_to_shape(&p, true, false).expect("PointZ");
        if let Shape::PointZ(pz) = shape {
            assert!((pz.z - 3.0).abs() < f64::EPSILON);
            assert!(pz.m.is_none());
        } else {
            panic!("expected PointZ");
        }
    }

    #[test]
    fn test_point_m() {
        let p = CorePoint::from_coord(Coordinate::new_2dm(1.0, 2.0, 5.0));
        let shape = geometry_point_to_shape(&p, false, true).expect("PointM");
        if let Shape::PointM(pm) = shape {
            assert!((pm.m - 5.0).abs() < f64::EPSILON);
        } else {
            panic!("expected PointM");
        }
    }

    #[test]
    fn test_point_z_record_layout() {
        use crate::shp::ShapeRecord;

        let p = CorePoint::from_coord(Coordinate::new_3dm(1.0, 2.0, 3.0, 4.0));
        let shape = geometry_point_to_shape(&p, true, true).expect("PointZ with M");
        let record = ShapeRecord::new(1, shape);
        let mut buf = Vec::new();
        record.write(&mut buf).expect("write record");

        // Record header (8 bytes big endian: record_number, content_length)
        // Content: shape_type (4 LE) + x(8) + y(8) + z(8) + m(8) = 36 bytes
        // Total record = 8 + 36 = 44 bytes
        assert_eq!(buf.len(), 8 + 36, "PointZ record must be 44 bytes total");

        // Read shape type from offset 8
        let shape_type_bytes: [u8; 4] = buf[8..12].try_into().expect("4 bytes");
        let shape_type = i32::from_le_bytes(shape_type_bytes);
        assert_eq!(shape_type, 11, "PointZ shape type must be 11");
    }
}
