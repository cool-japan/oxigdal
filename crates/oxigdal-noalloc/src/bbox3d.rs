//! 3D axis-aligned bounding box for `no_std`, `no_alloc` environments.
//!
//! [`BBox3D`] extends the 2D bounding box concept with a Z dimension,
//! suitable for volumetric spatial queries and point cloud bounds.

use crate::{BBox2D, Point3D, f64_max, f64_min};

/// A 3-dimensional axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox3D {
    /// Minimum X.
    pub min_x: f64,
    /// Minimum Y.
    pub min_y: f64,
    /// Minimum Z.
    pub min_z: f64,
    /// Maximum X.
    pub max_x: f64,
    /// Maximum Y.
    pub max_y: f64,
    /// Maximum Z.
    pub max_z: f64,
}

impl BBox3D {
    /// Creates a new `BBox3D`.
    #[must_use]
    #[inline]
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// Returns `true` if the bounding box is geometrically valid (min <= max on all axes).
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.min_x <= self.max_x && self.min_y <= self.max_y && self.min_z <= self.max_z
    }

    /// Returns `true` if the 3D point lies inside (or on the boundary of) this box.
    #[must_use]
    #[inline]
    pub fn contains_point(&self, p: &Point3D) -> bool {
        p.x >= self.min_x
            && p.x <= self.max_x
            && p.y >= self.min_y
            && p.y <= self.max_y
            && p.z >= self.min_z
            && p.z <= self.max_z
    }

    /// Returns `true` if this bounding box overlaps with another in all three dimensions.
    #[must_use]
    #[inline]
    pub fn intersects(&self, other: &BBox3D) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }

    /// Computes the smallest bounding box containing both boxes.
    #[must_use]
    #[inline]
    pub fn union(&self, other: &BBox3D) -> BBox3D {
        BBox3D::new(
            f64_min(self.min_x, other.min_x),
            f64_min(self.min_y, other.min_y),
            f64_min(self.min_z, other.min_z),
            f64_max(self.max_x, other.max_x),
            f64_max(self.max_y, other.max_y),
            f64_max(self.max_z, other.max_z),
        )
    }

    /// Computes the volume of the bounding box.
    ///
    /// Returns `0.0` if any dimension has negative extent.
    #[must_use]
    #[inline]
    pub fn volume(&self) -> f64 {
        let w = self.max_x - self.min_x;
        let h = self.max_y - self.min_y;
        let d = self.max_z - self.min_z;
        if w < 0.0 || h < 0.0 || d < 0.0 {
            0.0
        } else {
            w * h * d
        }
    }

    /// Returns the center point of the bounding box.
    #[must_use]
    #[inline]
    pub fn center(&self) -> Point3D {
        Point3D::new(
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
            (self.min_z + self.max_z) * 0.5,
        )
    }

    /// Returns an expanded bounding box — each face is pushed outward by `amount`.
    ///
    /// If `amount` is negative, the box shrinks (but is not validated).
    #[must_use]
    #[inline]
    pub fn expand(&self, amount: f64) -> BBox3D {
        BBox3D::new(
            self.min_x - amount,
            self.min_y - amount,
            self.min_z - amount,
            self.max_x + amount,
            self.max_y + amount,
            self.max_z + amount,
        )
    }

    /// Returns the surface area of the bounding box (sum of all 6 face areas).
    ///
    /// Returns `0.0` if any dimension has negative extent.
    #[must_use]
    #[inline]
    pub fn surface_area(&self) -> f64 {
        let w = self.max_x - self.min_x;
        let h = self.max_y - self.min_y;
        let d = self.max_z - self.min_z;
        if w < 0.0 || h < 0.0 || d < 0.0 {
            0.0
        } else {
            2.0 * (w * h + h * d + w * d)
        }
    }

    /// Projects the 3D bounding box to a 2D bounding box by dropping Z.
    #[must_use]
    #[inline]
    pub fn to_2d(&self) -> BBox2D {
        BBox2D::new(self.min_x, self.min_y, self.max_x, self.max_y)
    }

    /// Computes the diagonal length of the bounding box.
    #[must_use]
    pub fn diagonal(&self) -> f64 {
        let dx = self.max_x - self.min_x;
        let dy = self.max_y - self.min_y;
        let dz = self.max_z - self.min_z;
        crate::libm_sqrt(dx * dx + dy * dy + dz * dz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_and_valid() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(bbox.is_valid());
    }

    #[test]
    fn test_invalid() {
        let bbox = BBox3D::new(1.0, 0.0, 0.0, 0.0, 1.0, 1.0);
        assert!(!bbox.is_valid());
    }

    #[test]
    fn test_contains_point_inside() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
        assert!(bbox.contains_point(&Point3D::new(5.0, 5.0, 5.0)));
    }

    #[test]
    fn test_contains_point_on_boundary() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
        assert!(bbox.contains_point(&Point3D::new(0.0, 0.0, 0.0)));
        assert!(bbox.contains_point(&Point3D::new(10.0, 10.0, 10.0)));
    }

    #[test]
    fn test_contains_point_outside() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
        assert!(!bbox.contains_point(&Point3D::new(-1.0, 5.0, 5.0)));
        assert!(!bbox.contains_point(&Point3D::new(5.0, 11.0, 5.0)));
        assert!(!bbox.contains_point(&Point3D::new(5.0, 5.0, -0.1)));
    }

    #[test]
    fn test_intersects() {
        let a = BBox3D::new(0.0, 0.0, 0.0, 5.0, 5.0, 5.0);
        let b = BBox3D::new(3.0, 3.0, 3.0, 8.0, 8.0, 8.0);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn test_no_intersect() {
        let a = BBox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let b = BBox3D::new(2.0, 2.0, 2.0, 3.0, 3.0, 3.0);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn test_union() {
        let a = BBox3D::new(0.0, 0.0, 0.0, 5.0, 5.0, 5.0);
        let b = BBox3D::new(3.0, -1.0, 2.0, 8.0, 4.0, 10.0);
        let u = a.union(&b);
        assert!((u.min_x - 0.0).abs() < f64::EPSILON);
        assert!((u.min_y - (-1.0)).abs() < f64::EPSILON);
        assert!((u.min_z - 0.0).abs() < f64::EPSILON);
        assert!((u.max_x - 8.0).abs() < f64::EPSILON);
        assert!((u.max_y - 5.0).abs() < f64::EPSILON);
        assert!((u.max_z - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_volume() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 2.0, 3.0, 4.0);
        assert!((bbox.volume() - 24.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_volume_zero() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 0.0, 3.0, 4.0);
        assert!((bbox.volume() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_volume_invalid() {
        let bbox = BBox3D::new(1.0, 0.0, 0.0, 0.0, 1.0, 1.0);
        assert!((bbox.volume() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_center() {
        let bbox = BBox3D::new(0.0, 2.0, 4.0, 10.0, 8.0, 12.0);
        let c = bbox.center();
        assert!((c.x - 5.0).abs() < f64::EPSILON);
        assert!((c.y - 5.0).abs() < f64::EPSILON);
        assert!((c.z - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_expand() {
        let bbox = BBox3D::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0);
        let expanded = bbox.expand(0.5);
        assert!((expanded.min_x - 0.5).abs() < f64::EPSILON);
        assert!((expanded.max_z - 3.5).abs() < f64::EPSILON);
        assert!((expanded.volume() - 27.0).abs() < 1e-10);
    }

    #[test]
    fn test_surface_area() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 2.0, 3.0, 4.0);
        // 2*(2*3 + 3*4 + 2*4) = 2*(6+12+8) = 52
        assert!((bbox.surface_area() - 52.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_to_2d() {
        let bbox3 = BBox3D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        let bbox2 = bbox3.to_2d();
        assert!((bbox2.min_x - 1.0).abs() < f64::EPSILON);
        assert!((bbox2.min_y - 2.0).abs() < f64::EPSILON);
        assert!((bbox2.max_x - 4.0).abs() < f64::EPSILON);
        assert!((bbox2.max_y - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_diagonal() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 3.0, 4.0, 0.0);
        // sqrt(9+16+0) = 5
        assert!((bbox.diagonal() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_diagonal_cube() {
        let bbox = BBox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        // sqrt(3) ≈ 1.7320508
        assert!((bbox.diagonal() - 1.732_050_808_068_872_4).abs() < 1e-8);
    }

    #[test]
    fn test_intersects_touching() {
        let a = BBox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let b = BBox3D::new(1.0, 1.0, 1.0, 2.0, 2.0, 2.0);
        assert!(a.intersects(&b)); // touching at corner counts as intersecting
    }
}
