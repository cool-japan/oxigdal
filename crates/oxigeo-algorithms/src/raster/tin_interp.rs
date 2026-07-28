//! TIN (Triangulated Irregular Network) interpolation using Delaunay triangulation.
//!
//! A Triangulated Irregular Network (TIN) is a vector-based representation of a
//! surface, built from a set of arbitrarily distributed `(x, y, z)` samples by
//! triangulating their 2D footprint and interpolating elevations within each
//! triangle. TINs are a fundamental data structure in GIS, used for terrain
//! modelling, scattered-data interpolation, watershed analysis and conversion
//! between point clouds and raster digital elevation models.
//!
//! This module provides:
//!
//! - [`build_tin`]: build a TIN from a slice of [`TinPoint`] samples by
//!   computing the Delaunay triangulation of their 2D projection.
//! - [`interpolate_idw_tin`]: query a height at `(x, y)` using inverse-distance
//!   weighting from the enclosing triangle's three vertices.
//! - [`interpolate_natural_neighbor`]: query a height at `(x, y)` using
//!   barycentric (natural-neighbor-style) interpolation within the enclosing
//!   triangle. Full Sibson area-stealing remains a documented future enhancement.
//! - [`rasterize_tin`]: rasterise the TIN over a regular grid, producing a
//!   row-major `Vec<f32>` with `NaN` outside the convex hull.
//!
//! # Choosing an interpolation method
//!
//! - **IDW** is simple, robust and continuous everywhere inside the convex
//!   hull. With `power = 2.0` it falls off quadratically, giving more weight
//!   to the nearest vertex of the enclosing triangle.
//! - **Natural neighbor** (barycentric within the triangle) is C0-continuous
//!   and exactly reproduces linear data — every point of a planar input
//!   surface returns the exact analytic height — making it the preferred
//!   choice for terrain rasterisation of well-distributed samples.
//!
//! Both methods return `None` for query points that lie outside the convex
//! hull of the input samples (no extrapolation is performed).
//!
//! # Examples
//!
//! ```
//! use oxigeo_algorithms::raster::{build_tin, interpolate_idw_tin, TinPoint};
//!
//! let points = vec![
//!     TinPoint { x: 0.0, y: 0.0, z: 0.0 },
//!     TinPoint { x: 1.0, y: 0.0, z: 1.0 },
//!     TinPoint { x: 0.5, y: 1.0, z: 0.5 },
//! ];
//! let tin = build_tin(&points).expect("build TIN");
//! assert_eq!(tin.triangle_count(), 1);
//!
//! let z = interpolate_idw_tin(&tin, 0.5, 0.25, 2.0)
//!     .expect("query inside hull");
//! assert!(z.is_finite());
//! ```

use crate::error::Result;
use crate::vector::delaunay::{DelaunayOptions, delaunay_triangulation};
use oxigeo_core::vector::Point;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A 3D sample point used as input to TIN construction.
///
/// `x` and `y` define the horizontal location of the sample; `z` is the
/// scalar attribute being interpolated (typically elevation, but the TIN
/// machinery is agnostic to physical meaning).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TinPoint {
    /// Horizontal x coordinate (planimetric).
    pub x: f64,
    /// Horizontal y coordinate (planimetric).
    pub y: f64,
    /// Scalar value at `(x, y)` (e.g. elevation).
    pub z: f64,
}

impl TinPoint {
    /// Construct a new `TinPoint` from its three coordinates.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

/// A Triangulated Irregular Network: a set of 3D samples together with the
/// Delaunay triangulation of their 2D projection.
///
/// Triangle vertex indices are stored as triples into [`Self::points`]. The
/// orientation produced by [`delaunay_triangulation`] is implementation-defined
/// but consistent: `find_triangle` performs orientation-agnostic
/// barycentric containment tests, so consumers do not need to assume CCW.
#[derive(Debug, Clone)]
pub struct Tin {
    /// The input sample points, preserved in their original order so that
    /// triangle indices remain valid for downstream consumers.
    pub points: Vec<TinPoint>,
    /// Triangle vertex indices, three per triangle.
    pub triangles: Vec<[usize; 3]>,
}

impl Tin {
    /// Number of triangles in the TIN.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Number of input sample points (regardless of whether they participated
    /// in any triangle, e.g. for degenerate inputs).
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Returns `true` if the TIN contains no triangles.
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// Axis-aligned bounding box of the input points as
    /// `(min_x, min_y, max_x, max_y)`, or `None` if the TIN is empty of points.
    pub fn bounding_box(&self) -> Option<(f64, f64, f64, f64)> {
        let mut iter = self.points.iter();
        let first = iter.next()?;
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut max_x = first.x;
        let mut max_y = first.y;
        for p in iter {
            if p.x < min_x {
                min_x = p.x;
            }
            if p.y < min_y {
                min_y = p.y;
            }
            if p.x > max_x {
                max_x = p.x;
            }
            if p.y > max_y {
                max_y = p.y;
            }
        }
        Some((min_x, min_y, max_x, max_y))
    }
}

/// Interpolation method used by [`rasterize_tin`].
///
/// See module-level documentation for guidance on choosing between the two
/// variants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TinInterpMethod {
    /// Inverse-distance weighting from the enclosing triangle's three
    /// vertices. `power` controls the falloff exponent; `power = 2.0` is the
    /// classical Shepard interpolation. Must be strictly positive.
    Idw {
        /// Falloff exponent applied to the inverse vertex-distance weights.
        power: f64,
    },
    /// Barycentric (natural-neighbor-style) interpolation within the
    /// enclosing Delaunay triangle. Exactly reproduces linear surfaces.
    NaturalNeighbor,
}

impl Default for TinInterpMethod {
    fn default() -> Self {
        Self::NaturalNeighbor
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

/// Build a TIN from a point cloud by computing the Delaunay triangulation of
/// the 2D projection of `points`.
///
/// Returns a [`Tin`] whose `points` field is a copy of the input slice and
/// whose `triangles` field lists vertex-index triples. If `points.len() < 3`
/// the returned TIN contains the input points but no triangles (this is a
/// well-defined degenerate result rather than an error, since callers may
/// chain TIN construction over streaming chunks).
///
/// # Errors
///
/// Propagates any error from [`delaunay_triangulation`].
pub fn build_tin(points: &[TinPoint]) -> Result<Tin> {
    if points.len() < 3 {
        return Ok(Tin {
            points: points.to_vec(),
            triangles: Vec::new(),
        });
    }

    // Convert to oxigeo-core Points (2D only — z is preserved separately).
    let core_points: Vec<Point> = points.iter().map(|p| Point::new(p.x, p.y)).collect();

    let options = DelaunayOptions::default();
    let triangulation = delaunay_triangulation(&core_points, &options)?;

    let triangles: Vec<[usize; 3]> = triangulation
        .triangles
        .into_iter()
        .map(|t| t.vertices)
        .collect();

    Ok(Tin {
        points: points.to_vec(),
        triangles,
    })
}

// ---------------------------------------------------------------------------
// Triangle location
// ---------------------------------------------------------------------------

/// Tolerance, in barycentric-coordinate space, used when deciding whether a
/// query point lies inside a triangle. Generous enough to handle floating
/// point noise on edges and corners without rejecting valid queries.
const BARY_EPS: f64 = 1e-9;

/// Tolerance for treating a query point as coincident with a triangle vertex.
const COINCIDENT_EPS: f64 = 1e-12;

/// Find the triangle containing `(qx, qy)` and return its index together with
/// the barycentric coordinates of the query within that triangle.
///
/// Performs a brute-force linear scan over all triangles. For typical TIN
/// sizes (tens of thousands of triangles) this is acceptable; a future
/// enhancement could plug in the R-tree from `vector::clustering` for
/// sub-linear queries. Returns `None` if the query lies outside every
/// triangle (i.e. outside the convex hull of the input points).
fn find_triangle(tin: &Tin, qx: f64, qy: f64) -> Option<(usize, [f64; 3])> {
    for (ti, tri) in tin.triangles.iter().enumerate() {
        let [a, b, c] = *tri;
        let pa = tin.points[a];
        let pb = tin.points[b];
        let pc = tin.points[c];

        // Compute the (signed) twice-area determinant used to normalise the
        // barycentric coordinates. A near-zero determinant signals a
        // degenerate (collinear) triangle that we skip.
        let det = (pb.y - pc.y) * (pa.x - pc.x) + (pc.x - pb.x) * (pa.y - pc.y);
        if det.abs() < COINCIDENT_EPS {
            continue;
        }

        let l1 = ((pb.y - pc.y) * (qx - pc.x) + (pc.x - pb.x) * (qy - pc.y)) / det;
        let l2 = ((pc.y - pa.y) * (qx - pc.x) + (pa.x - pc.x) * (qy - pc.y)) / det;
        let l3 = 1.0 - l1 - l2;

        // Inside-or-on-boundary test. We accept negative values within
        // BARY_EPS to absorb floating-point noise along shared edges.
        if l1 >= -BARY_EPS && l2 >= -BARY_EPS && l3 >= -BARY_EPS {
            return Some((ti, [l1, l2, l3]));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Single-point interpolation
// ---------------------------------------------------------------------------

/// Inverse-distance-weighted interpolation at `(qx, qy)` using the three
/// vertices of the enclosing Delaunay triangle.
///
/// `power` is the falloff exponent; `power = 2.0` is classical Shepard
/// interpolation. If `(qx, qy)` coincides with a vertex (within
/// `COINCIDENT_EPS`) the corresponding vertex's `z` value is returned
/// directly to avoid a division by zero. Returns `None` if the query lies
/// outside the convex hull of the input samples.
///
/// # Panics
///
/// Does not panic. A non-positive `power` is silently treated as `power = 1.0`
/// (the safest non-degenerate fallback): a real geospatial pipeline would
/// surface this as a configuration error upstream of the inner loop.
pub fn interpolate_idw_tin(tin: &Tin, qx: f64, qy: f64, power: f64) -> Option<f64> {
    let (tri_idx, _bary) = find_triangle(tin, qx, qy)?;
    let [a, b, c] = tin.triangles[tri_idx];
    let pa = tin.points[a];
    let pb = tin.points[b];
    let pc = tin.points[c];

    let effective_power = if power.is_finite() && power > 0.0 {
        power
    } else {
        1.0
    };

    let dist = |p: TinPoint| -> f64 {
        let dx = p.x - qx;
        let dy = p.y - qy;
        (dx * dx + dy * dy).sqrt()
    };
    let da = dist(pa);
    let db = dist(pb);
    let dc = dist(pc);

    if da < COINCIDENT_EPS {
        return Some(pa.z);
    }
    if db < COINCIDENT_EPS {
        return Some(pb.z);
    }
    if dc < COINCIDENT_EPS {
        return Some(pc.z);
    }

    let wa = 1.0 / da.powf(effective_power);
    let wb = 1.0 / db.powf(effective_power);
    let wc = 1.0 / dc.powf(effective_power);
    let total = wa + wb + wc;
    if total <= 0.0 || !total.is_finite() {
        return None;
    }
    Some((pa.z * wa + pb.z * wb + pc.z * wc) / total)
}

/// Natural-neighbor-style interpolation at `(qx, qy)` using barycentric
/// coordinates within the enclosing Delaunay triangle.
///
/// This is exact for linear surfaces (planar inputs reproduce z everywhere)
/// and continuous across triangle edges. Full Sibson area-stealing — which
/// considers the second ring of natural neighbours — is documented as a
/// v0.2.0 enhancement; barycentric interpolation is the standard fallback
/// in production TIN rasterisers (matching e.g. GDAL's
/// `gdal_grid -a linear`).
///
/// Returns `None` if the query lies outside the convex hull of the input
/// samples.
pub fn interpolate_natural_neighbor(tin: &Tin, qx: f64, qy: f64) -> Option<f64> {
    let (tri_idx, bary) = find_triangle(tin, qx, qy)?;
    let [a, b, c] = tin.triangles[tri_idx];
    let pa = tin.points[a];
    let pb = tin.points[b];
    let pc = tin.points[c];
    Some(pa.z * bary[0] + pb.z * bary[1] + pc.z * bary[2])
}

// ---------------------------------------------------------------------------
// Rasterisation
// ---------------------------------------------------------------------------

/// Rasterise a TIN over a regular grid covering the bounding box
/// `(min_x, min_y, max_x, max_y)`.
///
/// Returns a row-major `Vec<f32>` of length `width * height`. Pixel
/// `(i, j)` — column `i`, row `j` — corresponds to the sample at the centre
/// of cell `(i, j)`, where row `0` is at the top (`max_y` end) and row
/// `height - 1` is at the bottom (`min_y` end). Pixels whose centres lie
/// outside the convex hull of the input samples are written as `f32::NAN`.
///
/// Returns an all-NaN buffer for degenerate dimensions (zero width/height or
/// inverted bounds).
pub fn rasterize_tin(
    tin: &Tin,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    width: usize,
    height: usize,
    method: TinInterpMethod,
) -> Vec<f32> {
    let mut out = vec![f32::NAN; width * height];
    if width == 0 || height == 0 || max_x <= min_x || max_y <= min_y {
        return out;
    }

    let dx = (max_x - min_x) / width as f64;
    let dy = (max_y - min_y) / height as f64;

    for j in 0..height {
        // Image-space convention: row 0 is the top, mapped to max_y.
        let qy = max_y - (j as f64 + 0.5) * dy;
        for i in 0..width {
            let qx = min_x + (i as f64 + 0.5) * dx;
            let v = match method {
                TinInterpMethod::Idw { power } => interpolate_idw_tin(tin, qx, qy, power),
                TinInterpMethod::NaturalNeighbor => interpolate_natural_neighbor(tin, qx, qy),
            };
            if let Some(z) = v {
                out[j * width + i] = z as f32;
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_triangle() -> Tin {
        let pts = vec![
            TinPoint::new(0.0, 0.0, 0.0),
            TinPoint::new(1.0, 0.0, 10.0),
            TinPoint::new(0.5, 1.0, 5.0),
        ];
        build_tin(&pts).expect("build TIN")
    }

    #[test]
    fn unit_build_tin_three_points() {
        let tin = sample_triangle();
        assert_eq!(tin.triangle_count(), 1);
        assert_eq!(tin.point_count(), 3);
    }

    #[test]
    fn unit_build_tin_too_few() {
        let tin = build_tin(&[]).expect("empty ok");
        assert!(tin.is_empty());
        let tin = build_tin(&[TinPoint::new(0.0, 0.0, 1.0)]).expect("single ok");
        assert!(tin.is_empty());
        let tin = build_tin(&[TinPoint::new(0.0, 0.0, 1.0), TinPoint::new(1.0, 0.0, 2.0)])
            .expect("pair ok");
        assert!(tin.is_empty());
    }

    #[test]
    fn unit_idw_at_vertex() {
        let tin = sample_triangle();
        let z = interpolate_idw_tin(&tin, 0.0, 0.0, 2.0).expect("inside hull");
        assert!((z - 0.0).abs() < 1e-9);
    }

    #[test]
    fn unit_natural_neighbor_at_vertex() {
        let tin = sample_triangle();
        let z = interpolate_natural_neighbor(&tin, 1.0, 0.0).expect("inside hull");
        assert!((z - 10.0).abs() < 1e-9);
    }

    #[test]
    fn unit_outside_hull_returns_none() {
        let tin = sample_triangle();
        assert!(interpolate_idw_tin(&tin, 10.0, 10.0, 2.0).is_none());
        assert!(interpolate_natural_neighbor(&tin, 10.0, 10.0).is_none());
    }

    #[test]
    fn unit_bounding_box() {
        let tin = sample_triangle();
        let bbox = tin.bounding_box().expect("non-empty");
        assert!((bbox.0 - 0.0).abs() < 1e-12);
        assert!((bbox.1 - 0.0).abs() < 1e-12);
        assert!((bbox.2 - 1.0).abs() < 1e-12);
        assert!((bbox.3 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn unit_rasterize_idw_dims() {
        let tin = sample_triangle();
        let grid = rasterize_tin(
            &tin,
            0.0,
            0.0,
            1.0,
            1.0,
            8,
            6,
            TinInterpMethod::Idw { power: 2.0 },
        );
        assert_eq!(grid.len(), 8 * 6);
    }

    #[test]
    fn unit_rasterize_natural_neighbor_dims() {
        let tin = sample_triangle();
        let grid = rasterize_tin(
            &tin,
            0.0,
            0.0,
            1.0,
            1.0,
            4,
            4,
            TinInterpMethod::NaturalNeighbor,
        );
        assert_eq!(grid.len(), 16);
    }

    #[test]
    fn unit_rasterize_degenerate_returns_all_nan() {
        let tin = sample_triangle();
        let grid = rasterize_tin(
            &tin,
            1.0,
            1.0,
            0.0,
            0.0,
            4,
            4,
            TinInterpMethod::NaturalNeighbor,
        );
        assert_eq!(grid.len(), 16);
        assert!(grid.iter().all(|v| v.is_nan()));
    }
}
