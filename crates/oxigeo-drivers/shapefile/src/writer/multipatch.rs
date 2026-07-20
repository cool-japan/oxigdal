//! Writer helpers for MultiPatch shapes (shape type 31).
//!
//! MultiPatch represents 3D surfaces using triangulated patches.  Each part
//! in a MultiPatch has an associated `PartType` that describes how to
//! interpret the vertices (triangle strip, triangle fan, outer ring, etc.).
//!
//! This module re-exports `PartType` from `shp::shapes` and provides
//! a convenience constructor for building `MultiPatchShape` records that
//! can be written via `Shape::MultiPatch`.

use crate::error::Result;
use crate::shp::shapes::Point;
pub use crate::shp::shapes::{MultiPatchShape, PartType};

/// Builds a `MultiPatchShape` from parallel arrays.
///
/// # Arguments
///
/// * `parts`      — start vertex index for each part
/// * `part_types` — one `PartType` per entry in `parts`
/// * `points`     — XY coordinates as `(x, y)` tuples
/// * `z_values`   — Z coordinate for each point (same length as `points`)
/// * `m_values`   — optional M (measure) value for each point
///
/// # Errors
///
/// Returns an error if lengths are inconsistent or geometry is invalid.
pub fn build_multipatch(
    parts: Vec<i32>,
    part_types: Vec<PartType>,
    points: Vec<(f64, f64)>,
    z_values: Vec<f64>,
    m_values: Option<Vec<f64>>,
) -> Result<MultiPatchShape> {
    let shp_points: Vec<Point> = points.iter().map(|(x, y)| Point::new(*x, *y)).collect();
    MultiPatchShape::new(parts, part_types, shp_points, z_values, m_values)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::shp::{Shape, ShapeRecord};

    /// A minimal triangle strip: 3 vertices, 1 part of type TriangleStrip.
    fn triangle_strip() -> MultiPatchShape {
        let parts = vec![0i32];
        let part_types = vec![PartType::TriangleStrip];
        let points = vec![(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)];
        let z_values = vec![0.0, 0.0, 5.0];
        build_multipatch(parts, part_types, points, z_values, None).expect("valid triangle strip")
    }

    #[test]
    fn test_multipatch_shape_type_31() {
        let mp = triangle_strip();
        let shape = Shape::MultiPatch(mp);
        let mut buf = Vec::new();
        let record = ShapeRecord::new(1, shape);
        record.write(&mut buf).expect("write MultiPatch");

        // Offset 8: shape type (4 bytes LE)
        let shape_type = i32::from_le_bytes(buf[8..12].try_into().expect("4 bytes"));
        assert_eq!(shape_type, 31, "MultiPatch shape type must be 31");
    }

    #[test]
    fn test_multipatch_parts_array() {
        // 3 parts
        let parts = vec![0i32, 3, 6];
        let part_types = vec![PartType::OuterRing, PartType::InnerRing, PartType::Ring];
        let pts: Vec<(f64, f64)> = (0..9).map(|i| (i as f64, i as f64)).collect();
        let z: Vec<f64> = (0..9).map(|i| i as f64 * 2.0).collect();
        let mp = build_multipatch(parts, part_types, pts, z, None).expect("3-part multipatch");

        assert_eq!(mp.base.num_parts, 3);
        assert_eq!(mp.base.num_points, 9);
        assert_eq!(mp.part_types.len(), 3);
        assert_eq!(mp.part_types[0], PartType::OuterRing);
        assert_eq!(mp.part_types[1], PartType::InnerRing);
        assert_eq!(mp.part_types[2], PartType::Ring);
    }

    #[test]
    fn test_multipatch_roundtrip() {
        use crate::shp::shapes::MultiPatchShape as MPS;
        use crate::shp::shapes::Point as ShpPoint;
        use std::io::Cursor;

        let pts = vec![
            ShpPoint::new(0.0, 0.0),
            ShpPoint::new(1.0, 0.0),
            ShpPoint::new(0.5, 1.0),
            ShpPoint::new(2.0, 0.0),
            ShpPoint::new(3.0, 0.0),
            ShpPoint::new(2.5, 1.0),
        ];
        let parts = vec![0i32, 3];
        let part_types = vec![PartType::TriangleStrip, PartType::TriangleFan];
        let z_values = vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let m_values = Some(vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0]);

        let original =
            MPS::new(parts, part_types, pts, z_values, m_values).expect("valid multipatch");

        // Write to buffer
        let shape = Shape::MultiPatch(original.clone());
        let record = ShapeRecord::new(1, shape);
        let mut buf = Vec::new();
        record.write(&mut buf).expect("write");

        // Read back
        let mut cursor = Cursor::new(&buf);
        let read_record = ShapeRecord::read(&mut cursor).expect("read");

        if let Shape::MultiPatch(read_mp) = read_record.shape {
            assert_eq!(read_mp.base.num_parts, 2);
            assert_eq!(read_mp.base.num_points, 6);
            assert_eq!(read_mp.part_types.len(), 2);
            assert_eq!(read_mp.part_types[0], PartType::TriangleStrip);
            assert_eq!(read_mp.part_types[1], PartType::TriangleFan);
            assert!((read_mp.z_values[2] - 2.0).abs() < f64::EPSILON);
            assert!((read_mp.z_values[5] - 5.0).abs() < f64::EPSILON);
            let mv = read_mp.m_values.as_ref().expect("m_values present");
            assert!((mv[0] - 10.0).abs() < f64::EPSILON);
            assert!((mv[5] - 15.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected MultiPatch");
        }
    }

    #[test]
    fn test_part_type_codes() {
        assert_eq!(PartType::TriangleStrip.to_code(), 0);
        assert_eq!(PartType::TriangleFan.to_code(), 1);
        assert_eq!(PartType::OuterRing.to_code(), 2);
        assert_eq!(PartType::InnerRing.to_code(), 3);
        assert_eq!(PartType::FirstRing.to_code(), 4);
        assert_eq!(PartType::Ring.to_code(), 5);

        assert_eq!(PartType::from_code(2).expect("valid"), PartType::OuterRing);
        assert!(PartType::from_code(99).is_err());
    }
}
