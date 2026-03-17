//! 2D bounding box type for spatial indexing.

/// A 2D axis-aligned bounding box.
///
/// All coordinates are in the range `(-∞, +∞)`. Validity requires
/// `min_x <= max_x` and `min_y <= max_y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox2D {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bbox2D {
    /// Create a new `Bbox2D`, returning `None` if coordinates are inverted.
    #[inline]
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Option<Self> {
        if min_x <= max_x && min_y <= max_y {
            Some(Self {
                min_x,
                min_y,
                max_x,
                max_y,
            })
        } else {
            None
        }
    }

    /// Create a zero-size bbox at a single point.
    #[inline]
    pub fn point(x: f64, y: f64) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    /// Construct the bounding box from a slice of `(x, y)` points.
    ///
    /// Returns `None` if `points` is empty.
    pub fn from_points(points: &[(f64, f64)]) -> Option<Self> {
        let mut iter = points.iter();
        let &(x0, y0) = iter.next()?;
        let mut min_x = x0;
        let mut min_y = y0;
        let mut max_x = x0;
        let mut max_y = y0;
        for &(x, y) in iter {
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
        }
        Some(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Width of the bounding box (`max_x - min_x`).
    #[inline]
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Height of the bounding box (`max_y - min_y`).
    #[inline]
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Area of the bounding box (`width * height`).
    #[inline]
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Perimeter of the bounding box (`2 * (width + height)`).
    #[inline]
    pub fn perimeter(&self) -> f64 {
        2.0 * (self.width() + self.height())
    }

    /// Centre of the bounding box.
    #[inline]
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        )
    }

    /// Whether a point `(x, y)` lies strictly within or on the boundary of
    /// this bbox.
    #[inline]
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Whether `other` is fully contained within (or touching) this bbox.
    #[inline]
    pub fn contains_bbox(&self, other: &Bbox2D) -> bool {
        other.min_x >= self.min_x
            && other.max_x <= self.max_x
            && other.min_y >= self.min_y
            && other.max_y <= self.max_y
    }

    /// Whether `self` and `other` overlap (including touching edges).
    #[inline]
    pub fn intersects(&self, other: &Bbox2D) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Smallest bbox that covers both `self` and `other`.
    #[inline]
    pub fn union(&self, other: &Bbox2D) -> Bbox2D {
        Bbox2D {
            min_x: f64::min(self.min_x, other.min_x),
            min_y: f64::min(self.min_y, other.min_y),
            max_x: f64::max(self.max_x, other.max_x),
            max_y: f64::max(self.max_y, other.max_y),
        }
    }

    /// Intersection of `self` and `other`, or `None` when they are disjoint.
    #[inline]
    pub fn intersection(&self, other: &Bbox2D) -> Option<Bbox2D> {
        let min_x = f64::max(self.min_x, other.min_x);
        let min_y = f64::max(self.min_y, other.min_y);
        let max_x = f64::min(self.max_x, other.max_x);
        let max_y = f64::min(self.max_y, other.max_y);
        if min_x <= max_x && min_y <= max_y {
            Some(Bbox2D {
                min_x,
                min_y,
                max_x,
                max_y,
            })
        } else {
            None
        }
    }

    /// Return a bbox expanded on all sides by `delta`.
    ///
    /// `delta` may be negative (shrinking), but the result is clamped so that
    /// `min <= max` is always preserved.
    #[inline]
    pub fn expand_by(&self, delta: f64) -> Bbox2D {
        let min_x = self.min_x - delta;
        let min_y = self.min_y - delta;
        let max_x = self.max_x + delta;
        let max_y = self.max_y + delta;
        Bbox2D {
            min_x: f64::min(min_x, max_x),
            min_y: f64::min(min_y, max_y),
            max_x: f64::max(min_x, max_x),
            max_y: f64::max(min_y, max_y),
        }
    }

    /// Whether the bbox has zero area (either dimension is collapsed to a
    /// point).
    #[inline]
    pub fn is_degenerate(&self) -> bool {
        self.area() == 0.0
    }

    /// How much `self` would need to grow (in area) to include `other`.
    ///
    /// Returns `0.0` when `other` is already contained.
    #[inline]
    pub fn enlargement_to_include(&self, other: &Bbox2D) -> f64 {
        let enlarged = self.union(other);
        (enlarged.area() - self.area()).max(0.0)
    }

    /// Minimum squared Euclidean distance from a point `(x, y)` to the
    /// **boundary** of this bbox.  Returns `0.0` when the point is inside.
    ///
    /// This is the classic "MINDIST" metric used in nearest-neighbour R-tree
    /// queries.
    #[inline]
    pub fn min_distance_to_point(&self, x: f64, y: f64) -> f64 {
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
        (dx * dx + dy * dy).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_bbox_area() {
        let b = Bbox2D::new(0.0, 0.0, 1.0, 1.0).expect("valid unit bbox");
        assert_eq!(b.area(), 1.0);
    }

    #[test]
    fn point_bbox_is_degenerate() {
        let b = Bbox2D::point(3.0, 4.0);
        assert!(b.is_degenerate());
        assert_eq!(b.area(), 0.0);
    }

    #[test]
    fn invalid_bbox_returns_none() {
        assert!(Bbox2D::new(1.0, 0.0, 0.0, 1.0).is_none());
        assert!(Bbox2D::new(0.0, 1.0, 1.0, 0.0).is_none());
    }
}
