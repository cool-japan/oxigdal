//! 3D bounding box type for volumetric and point-cloud spatial indexing.
//!
//! Parallel to [`crate::bbox::Bbox2D`] but extended to a third (Z) axis.
//! All coordinates are in the range `(-∞, +∞)`.  Validity requires
//! `min_x <= max_x`, `min_y <= max_y`, and `min_z <= max_z`.

/// A 3D axis-aligned bounding box.
///
/// Used as the fundamental unit of the [`crate::rtree3d::RTree3D`] spatial
/// index.  A degenerate bbox (zero volume in one or more dimensions) is valid
/// and represents points, line segments, or flat rectangles embedded in 3D
/// space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox3D {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Bbox3D {
    /// Create a new `Bbox3D`.
    ///
    /// All min/max pairs must be non-inverted; the constructor returns `None`
    /// if any pair violates `min <= max`.
    #[inline]
    pub fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Option<Self> {
        if min_x <= max_x && min_y <= max_y && min_z <= max_z {
            Some(Self {
                min_x,
                min_y,
                min_z,
                max_x,
                max_y,
                max_z,
            })
        } else {
            None
        }
    }

    /// Create a zero-volume bbox at a single point `(x, y, z)`.
    #[inline]
    pub fn point(x: f64, y: f64, z: f64) -> Self {
        Self {
            min_x: x,
            min_y: y,
            min_z: z,
            max_x: x,
            max_y: y,
            max_z: z,
        }
    }

    /// Construct the bounding box from a slice of `[x, y, z]` triples.
    ///
    /// Returns `None` if `points` is empty.
    pub fn from_points(points: &[[f64; 3]]) -> Option<Self> {
        let mut iter = points.iter();
        let first = iter.next()?;
        let mut min_x = first[0];
        let mut min_y = first[1];
        let mut min_z = first[2];
        let mut max_x = first[0];
        let mut max_y = first[1];
        let mut max_z = first[2];
        for pt in iter {
            let (x, y, z) = (pt[0], pt[1], pt[2]);
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if z < min_z {
                min_z = z;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
            if z > max_z {
                max_z = z;
            }
        }
        Some(Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        })
    }

    /// Extent along the X axis (`max_x - min_x`).
    #[inline]
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Extent along the Y axis (`max_y - min_y`).
    #[inline]
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Extent along the Z axis (`max_z - min_z`).
    #[inline]
    pub fn depth(&self) -> f64 {
        self.max_z - self.min_z
    }

    /// Volume of the bounding box (`width × height × depth`).
    #[inline]
    pub fn volume(&self) -> f64 {
        self.width() * self.height() * self.depth()
    }

    /// Surface area of the bounding box.
    ///
    /// Used instead of perimeter in 3D R*-tree margin computations:
    /// `2 * (width * height + width * depth + height * depth)`.
    #[inline]
    pub fn surface_area(&self) -> f64 {
        let w = self.width();
        let h = self.height();
        let d = self.depth();
        2.0 * (w * h + w * d + h * d)
    }

    /// Centre of the bounding box.
    #[inline]
    pub fn center(&self) -> (f64, f64, f64) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
            (self.min_z + self.max_z) * 0.5,
        )
    }

    /// Whether point `(x, y, z)` lies strictly within or on the boundary of
    /// this bbox.
    #[inline]
    pub fn contains_point(&self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
            && z >= self.min_z
            && z <= self.max_z
    }

    /// Whether `other` is fully contained within (or touching) this bbox.
    #[inline]
    pub fn contains_bbox(&self, other: &Bbox3D) -> bool {
        other.min_x >= self.min_x
            && other.max_x <= self.max_x
            && other.min_y >= self.min_y
            && other.max_y <= self.max_y
            && other.min_z >= self.min_z
            && other.max_z <= self.max_z
    }

    /// Whether `self` and `other` overlap (including touching faces, edges,
    /// or corners).
    #[inline]
    pub fn intersects(&self, other: &Bbox3D) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }

    /// Smallest bbox that covers both `self` and `other`.
    #[inline]
    pub fn union(&self, other: &Bbox3D) -> Bbox3D {
        Bbox3D {
            min_x: f64::min(self.min_x, other.min_x),
            min_y: f64::min(self.min_y, other.min_y),
            min_z: f64::min(self.min_z, other.min_z),
            max_x: f64::max(self.max_x, other.max_x),
            max_y: f64::max(self.max_y, other.max_y),
            max_z: f64::max(self.max_z, other.max_z),
        }
    }

    /// Intersection of `self` and `other`, or `None` when they are disjoint.
    #[inline]
    pub fn intersection(&self, other: &Bbox3D) -> Option<Bbox3D> {
        let min_x = f64::max(self.min_x, other.min_x);
        let min_y = f64::max(self.min_y, other.min_y);
        let min_z = f64::max(self.min_z, other.min_z);
        let max_x = f64::min(self.max_x, other.max_x);
        let max_y = f64::min(self.max_y, other.max_y);
        let max_z = f64::min(self.max_z, other.max_z);
        if min_x <= max_x && min_y <= max_y && min_z <= max_z {
            Some(Bbox3D {
                min_x,
                min_y,
                min_z,
                max_x,
                max_y,
                max_z,
            })
        } else {
            None
        }
    }

    /// Return a bbox expanded on all six faces by `delta`.
    ///
    /// `delta` may be negative (shrinking), but the result is clamped so that
    /// `min <= max` is always preserved on every axis.
    #[inline]
    pub fn expand_by(&self, delta: f64) -> Bbox3D {
        let min_x = self.min_x - delta;
        let min_y = self.min_y - delta;
        let min_z = self.min_z - delta;
        let max_x = self.max_x + delta;
        let max_y = self.max_y + delta;
        let max_z = self.max_z + delta;
        Bbox3D {
            min_x: f64::min(min_x, max_x),
            min_y: f64::min(min_y, max_y),
            min_z: f64::min(min_z, max_z),
            max_x: f64::max(min_x, max_x),
            max_y: f64::max(min_y, max_y),
            max_z: f64::max(min_z, max_z),
        }
    }

    /// Whether the bbox has zero volume (any dimension collapsed to a point).
    #[inline]
    pub fn is_degenerate(&self) -> bool {
        self.volume() == 0.0
    }

    /// How much `self` would need to grow (in volume) to include `other`.
    ///
    /// Returns `0.0` when `other` is already contained.
    #[inline]
    pub fn enlargement_to_include(&self, other: &Bbox3D) -> f64 {
        let enlarged = self.union(other);
        (enlarged.volume() - self.volume()).max(0.0)
    }

    /// Minimum Euclidean distance from point `(x, y, z)` to this bbox.
    ///
    /// Returns `0.0` when the point lies inside or on the boundary.
    /// This is the 3D MINDIST metric used in nearest-neighbour R-tree queries.
    #[inline]
    pub fn min_distance_to_point(&self, x: f64, y: f64, z: f64) -> f64 {
        self.min_distance_sq_to_point(x, y, z).sqrt()
    }

    /// Minimum **squared** Euclidean distance from point `(x, y, z)` to this
    /// bbox.  Returns `0.0` when the point lies inside or on the boundary.
    ///
    /// Avoids `sqrt` for priority-queue k-NN traversal.
    #[inline]
    pub fn min_distance_sq_to_point(&self, x: f64, y: f64, z: f64) -> f64 {
        let dx = if x < self.min_x {
            self.min_x - x
        } else if x > self.max_x {
            x - self.max_x
        } else {
            0.0
        };
        let dy = if y < self.min_y {
            self.min_y - y
        } else if y > self.max_y {
            y - self.max_y
        } else {
            0.0
        };
        let dz = if z < self.min_z {
            self.min_z - z
        } else if z > self.max_z {
            z - self.max_z
        } else {
            0.0
        };
        dx * dx + dy * dy + dz * dz
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn unit_bbox3d_volume() {
        let b = Bbox3D::new(0.0, 0.0, 0.0, 2.0, 3.0, 4.0).expect("valid");
        assert_eq!(b.volume(), 24.0);
    }

    #[test]
    fn point_bbox3d_is_degenerate() {
        let b = Bbox3D::point(1.0, 2.0, 3.0);
        assert!(b.is_degenerate());
        assert_eq!(b.volume(), 0.0);
    }

    #[test]
    fn invalid_bbox3d_returns_none() {
        assert!(Bbox3D::new(1.0, 0.0, 0.0, 0.0, 1.0, 1.0).is_none());
        assert!(Bbox3D::new(0.0, 1.0, 0.0, 1.0, 0.0, 1.0).is_none());
        assert!(Bbox3D::new(0.0, 0.0, 1.0, 1.0, 1.0, 0.0).is_none());
    }

    #[test]
    fn bbox3d_surface_area_unit_cube() {
        // Unit cube: 6 faces of area 1 each → surface area = 6.
        let b = Bbox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).expect("valid");
        assert_eq!(b.surface_area(), 6.0);
    }

    #[test]
    fn bbox3d_union_covers_both() {
        let a = Bbox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).unwrap();
        let b = Bbox3D::new(2.0, 2.0, 2.0, 3.0, 3.0, 3.0).unwrap();
        let u = a.union(&b);
        assert_eq!(u.min_x, 0.0);
        assert_eq!(u.max_z, 3.0);
    }

    #[test]
    fn bbox3d_intersects_and_disjoint() {
        let a = Bbox3D::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0).unwrap();
        let b = Bbox3D::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0).unwrap();
        let c = Bbox3D::new(5.0, 5.0, 5.0, 6.0, 6.0, 6.0).unwrap();
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }
}
