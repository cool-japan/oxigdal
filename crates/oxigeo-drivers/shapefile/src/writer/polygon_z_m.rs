//! Writer helpers for Polygon / MultiPolygon shapes (2D, Z, and M variants).
//!
//! Converts OxiGeo polygon geometries into the appropriate `Shape` variant
//! based on the presence of Z/M coordinates on the coordinates.

use crate::error::{Result, ShapefileError};
use crate::polygon_rings::{normalize_multipolygon_rings, normalize_polygon_rings};
use crate::shp::Shape;
use crate::shp::shapes::{MultiPartShape, MultiPartShapeM, MultiPartShapeZ, Point};
use oxigeo_core::vector::{Coordinate, MultiPolygon, Polygon};

/// Flattens winding-normalized rings (exterior clockwise, holes counter-clockwise
/// per the ESRI spec) into the appropriate multi-part `Shape` variant. Z and M
/// values are collected in exactly the same, possibly reversed, ring order so
/// they stay aligned with their vertices.
fn rings_to_shape(rings: &[Vec<Coordinate>], has_z: bool, has_m: bool) -> Result<Shape> {
    let mut all_points: Vec<Point> = Vec::new();
    let mut parts: Vec<i32> = Vec::new();

    for ring in rings {
        parts.push(all_points.len() as i32);
        for coord in ring {
            all_points.push(Point::new(coord.x, coord.y));
        }
    }

    if all_points.is_empty() {
        return Err(ShapefileError::invalid_geometry(
            "Polygon must have at least one point",
        ));
    }

    if has_z {
        let z_values: Vec<f64> = rings.iter().flatten().map(|c| c.z.unwrap_or(0.0)).collect();
        let m_values_opt: Option<Vec<f64>> = if has_m {
            Some(rings.iter().flatten().map(|c| c.m.unwrap_or(0.0)).collect())
        } else {
            None
        };
        let shape_z = MultiPartShapeZ::new(parts, all_points, z_values, m_values_opt)?;
        Ok(Shape::PolygonZ(shape_z))
    } else if has_m {
        let m_values: Vec<f64> = rings.iter().flatten().map(|c| c.m.unwrap_or(0.0)).collect();
        let shape_m = MultiPartShapeM::new(parts, all_points, m_values)?;
        Ok(Shape::PolygonM(shape_m))
    } else {
        let shape = MultiPartShape::new(parts, all_points)?;
        Ok(Shape::Polygon(shape))
    }
}

/// Converts a core `Polygon` geometry to the correct `Shape` variant.
///
/// The polygon's rings are winding-normalized (exterior clockwise, holes
/// counter-clockwise) so the resulting record is ESRI-spec conformant and reads
/// back correctly with the winding-aware reader.
///
/// - Has Z  → `Shape::PolygonZ`
/// - Has M only → `Shape::PolygonM`
/// - 2D → `Shape::Polygon`
pub fn geometry_polygon_to_shape(polygon: &Polygon, has_z: bool, has_m: bool) -> Result<Shape> {
    let rings = normalize_polygon_rings(polygon);
    rings_to_shape(&rings, has_z, has_m)
}

/// Converts a core `MultiPolygon` geometry to the correct `Shape` variant.
///
/// All polygons in a MultiPolygon are flattened into a single multi-part shape
/// (the Shapefile spec uses one multi-ring `Polygon` record and distinguishes the
/// rings by winding direction). Each polygon contributes its clockwise exterior
/// ring followed by its counter-clockwise holes.
///
/// - Has Z  → `Shape::PolygonZ`
/// - Has M only → `Shape::PolygonM`
/// - 2D → `Shape::Polygon`
pub fn geometry_multipolygon_to_shape(
    multipolygon: &MultiPolygon,
    has_z: bool,
    has_m: bool,
) -> Result<Shape> {
    let rings = normalize_multipolygon_rings(multipolygon);
    rings_to_shape(&rings, has_z, has_m)
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

    // Clockwise square (ESRI exterior winding) at the given origin.
    fn cw_square(ox: f64, oy: f64, s: f64) -> LineString {
        LineString::new(vec![
            Coordinate::new_2d(ox, oy),
            Coordinate::new_2d(ox, oy + s),
            Coordinate::new_2d(ox + s, oy + s),
            Coordinate::new_2d(ox + s, oy),
            Coordinate::new_2d(ox, oy),
        ])
        .expect("valid ring")
    }

    #[test]
    fn test_multipolygon_two_islands_round_trip() {
        use crate::reader::ShapefileReader;
        use oxigeo_core::vector::Geometry;

        // Two disjoint "islands" (e.g. a country with an offshore island) — each
        // its own clockwise exterior. This must NOT collapse into one polygon
        // whose second island is a bogus hole.
        let island_a = Polygon::new(cw_square(0.0, 0.0, 10.0), vec![]).expect("island A");
        let island_b = Polygon::new(cw_square(100.0, 0.0, 10.0), vec![]).expect("island B");
        let original = MultiPolygon::new(vec![island_a, island_b]);

        let shape = geometry_multipolygon_to_shape(&original, false, false).expect("to shape");
        // The writer flattens both islands into ONE multi-ring Polygon record.
        assert!(matches!(shape, Shape::Polygon(_)));

        let geometry = ShapefileReader::shape_to_geometry_pub(&shape)
            .expect("read back")
            .expect("some geometry");

        match geometry {
            Geometry::MultiPolygon(mp) => {
                assert_eq!(mp.polygons.len(), 2, "both islands must survive");
                for poly in &mp.polygons {
                    assert!(
                        poly.interiors.is_empty(),
                        "no island should gain a spurious hole"
                    );
                }
            }
            other => panic!("expected MultiPolygon round-trip, got {other:?}"),
        }
    }

    #[test]
    fn test_polygon_with_hole_round_trip() {
        use crate::reader::ShapefileReader;
        use oxigeo_core::vector::Geometry;

        // One exterior with a real hole (counter-clockwise inner ring).
        let hole = LineString::new(vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(4.0, 2.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(2.0, 4.0),
            Coordinate::new_2d(2.0, 2.0),
        ])
        .expect("hole ring");
        let original = Polygon::new(cw_square(0.0, 0.0, 10.0), vec![hole]).expect("polygon");

        let shape = geometry_polygon_to_shape(&original, false, false).expect("to shape");
        let geometry = ShapefileReader::shape_to_geometry_pub(&shape)
            .expect("read back")
            .expect("some geometry");

        match geometry {
            Geometry::Polygon(p) => {
                assert_eq!(p.interiors.len(), 1, "the hole must be preserved");
            }
            other => panic!("expected single Polygon with a hole, got {other:?}"),
        }
    }
}
