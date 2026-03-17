//! # oxigdal-noalloc
//!
//! `no_std`, `no_alloc` fixed-size geometry primitives for OxiGDAL.
//!
//! This crate provides zero-allocation geometry types suitable for embedded
//! and RISC-V environments where heap allocation is unavailable or undesirable.
//!
//! ## Types
//!
//! - [`Point2D`] — 2D point with distance and midpoint operations
//! - [`Point3D`] — 3D point
//! - [`BBox2D`] — 2D axis-aligned bounding box
//! - [`LineSegment2D`] — 2D line segment with intersection support
//! - [`Triangle2D`] — 2D triangle with area, containment, and centroid
//! - [`FixedPolygon`] — Fixed-capacity polygon backed by an inline array
//! - [`CoordTransform`] — 2D affine transform (2×3 matrix)
//! - [`GeoHashFixed`] — Geohash encoding stored as `[u8; 12]`
//! - [`NoAllocError`] — Error enum for no-alloc operations

#![no_std]
#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod geohash;

pub use geohash::GeoHashFixed;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can occur in no-alloc geometry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoAllocError {
    /// The fixed-capacity container is full.
    CapacityExceeded,
    /// Geohash precision is outside the range 1–12.
    InvalidPrecision,
    /// Geometry contains no vertices.
    EmptyGeometry,
}

// ── Point2D ───────────────────────────────────────────────────────────────────

/// A 2-dimensional point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2D {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Point2D {
    /// Creates a new `Point2D`.
    #[must_use]
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Computes the Euclidean distance to another point.
    #[must_use]
    #[inline]
    pub fn distance_to(&self, other: &Point2D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        libm_sqrt(dx * dx + dy * dy)
    }

    /// Returns the midpoint between this point and another.
    #[must_use]
    #[inline]
    pub fn midpoint(&self, other: &Point2D) -> Point2D {
        Point2D::new((self.x + other.x) * 0.5, (self.y + other.y) * 0.5)
    }
}

// ── Point3D ───────────────────────────────────────────────────────────────────

/// A 3-dimensional point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3D {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}

impl Point3D {
    /// Creates a new `Point3D`.
    #[must_use]
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Computes the Euclidean distance to another point.
    #[must_use]
    #[inline]
    pub fn distance_to(&self, other: &Point3D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        libm_sqrt(dx * dx + dy * dy + dz * dz)
    }

    /// Projects to a 2D point by dropping the Z component.
    #[must_use]
    #[inline]
    pub const fn to_2d(&self) -> Point2D {
        Point2D::new(self.x, self.y)
    }
}

// ── BBox2D ────────────────────────────────────────────────────────────────────

/// A 2-dimensional axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox2D {
    /// Minimum X.
    pub min_x: f64,
    /// Minimum Y.
    pub min_y: f64,
    /// Maximum X.
    pub max_x: f64,
    /// Maximum Y.
    pub max_y: f64,
}

impl BBox2D {
    /// Creates a new `BBox2D`.
    #[must_use]
    #[inline]
    pub const fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Returns `true` if the bounding box is geometrically valid (min ≤ max).
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.min_x <= self.max_x && self.min_y <= self.max_y
    }

    /// Returns `true` if the point lies inside (or on the boundary of) this box.
    #[must_use]
    #[inline]
    pub fn contains_point(&self, p: Point2D) -> bool {
        p.x >= self.min_x && p.x <= self.max_x && p.y >= self.min_y && p.y <= self.max_y
    }

    /// Returns `true` if this bounding box overlaps with another.
    #[must_use]
    #[inline]
    pub fn intersects(&self, other: &BBox2D) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Computes the smallest bounding box containing both boxes.
    #[must_use]
    #[inline]
    pub fn union(&self, other: &BBox2D) -> BBox2D {
        BBox2D::new(
            f64_min(self.min_x, other.min_x),
            f64_min(self.min_y, other.min_y),
            f64_max(self.max_x, other.max_x),
            f64_max(self.max_y, other.max_y),
        )
    }

    /// Computes the area of the bounding box.
    #[must_use]
    #[inline]
    pub fn area(&self) -> f64 {
        let w = self.max_x - self.min_x;
        let h = self.max_y - self.min_y;
        if w < 0.0 || h < 0.0 { 0.0 } else { w * h }
    }
}

// ── LineSegment2D ─────────────────────────────────────────────────────────────

/// A directed 2D line segment from `start` to `end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineSegment2D {
    /// Start point.
    pub start: Point2D,
    /// End point.
    pub end: Point2D,
}

impl LineSegment2D {
    /// Creates a new `LineSegment2D`.
    #[must_use]
    #[inline]
    pub const fn new(start: Point2D, end: Point2D) -> Self {
        Self { start, end }
    }

    /// Returns the length of the segment.
    #[must_use]
    #[inline]
    pub fn length(&self) -> f64 {
        self.start.distance_to(&self.end)
    }

    /// Returns the midpoint of the segment.
    #[must_use]
    #[inline]
    pub fn midpoint(&self) -> Point2D {
        self.start.midpoint(&self.end)
    }

    /// Returns the point at parameter `t` along the segment.
    ///
    /// `t = 0.0` returns `start`, `t = 1.0` returns `end`.
    #[must_use]
    #[inline]
    pub fn point_on_segment(&self, t: f64) -> Point2D {
        Point2D::new(
            self.start.x + t * (self.end.x - self.start.x),
            self.start.y + t * (self.end.y - self.start.y),
        )
    }

    /// Computes the intersection point of two line segments, if it exists.
    ///
    /// Uses parametric form. Returns `None` for parallel/coincident segments.
    #[must_use]
    pub fn intersects(&self, other: &LineSegment2D) -> Option<Point2D> {
        let dx1 = self.end.x - self.start.x;
        let dy1 = self.end.y - self.start.y;
        let dx2 = other.end.x - other.start.x;
        let dy2 = other.end.y - other.start.y;

        let denom = dx1 * dy2 - dy1 * dx2;

        if denom.abs() < f64::EPSILON * 1e6 {
            return None; // parallel or coincident
        }

        let ox = other.start.x - self.start.x;
        let oy = other.start.y - self.start.y;

        let t = (ox * dy2 - oy * dx2) / denom;
        let u = (ox * dy1 - oy * dx1) / denom;

        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            Some(self.point_on_segment(t))
        } else {
            None
        }
    }
}

// ── Triangle2D ────────────────────────────────────────────────────────────────

/// A triangle defined by three 2D vertices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle2D {
    /// First vertex.
    pub a: Point2D,
    /// Second vertex.
    pub b: Point2D,
    /// Third vertex.
    pub c: Point2D,
}

impl Triangle2D {
    /// Creates a new `Triangle2D`.
    #[must_use]
    #[inline]
    pub const fn new(a: Point2D, b: Point2D, c: Point2D) -> Self {
        Self { a, b, c }
    }

    /// Computes the signed area using the shoelace formula.
    ///
    /// Positive for counter-clockwise, negative for clockwise.
    #[must_use]
    #[inline]
    pub fn signed_area(&self) -> f64 {
        0.5 * ((self.b.x - self.a.x) * (self.c.y - self.a.y)
            - (self.c.x - self.a.x) * (self.b.y - self.a.y))
    }

    /// Computes the absolute area.
    #[must_use]
    #[inline]
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    /// Returns `true` if the vertices are arranged clockwise.
    #[must_use]
    #[inline]
    pub fn is_clockwise(&self) -> bool {
        self.signed_area() < 0.0
    }

    /// Computes the centroid (arithmetic mean of the three vertices).
    #[must_use]
    #[inline]
    pub fn centroid(&self) -> Point2D {
        Point2D::new(
            (self.a.x + self.b.x + self.c.x) / 3.0,
            (self.a.y + self.b.y + self.c.y) / 3.0,
        )
    }

    /// Returns the perimeter (sum of side lengths).
    #[must_use]
    pub fn perimeter(&self) -> f64 {
        self.a.distance_to(&self.b) + self.b.distance_to(&self.c) + self.c.distance_to(&self.a)
    }

    /// Tests whether a point is inside or on the boundary of the triangle.
    ///
    /// Uses barycentric coordinates.
    #[must_use]
    pub fn contains_point(&self, p: Point2D) -> bool {
        // Barycentric coordinates via signed areas
        let s_abc = self.signed_area();
        if s_abc.abs() < f64::EPSILON {
            return false; // degenerate triangle
        }

        let s_pbc = Triangle2D::new(p, self.b, self.c).signed_area();
        let s_apc = Triangle2D::new(self.a, p, self.c).signed_area();
        let s_abp = Triangle2D::new(self.a, self.b, p).signed_area();

        let same_sign = |a: f64, b: f64| (a >= 0.0) == (b >= 0.0);

        same_sign(s_abc, s_pbc) && same_sign(s_abc, s_apc) && same_sign(s_abc, s_abp)
    }
}

// ── FixedPolygon ──────────────────────────────────────────────────────────────

/// A polygon with a statically-allocated vertex array of capacity `N`.
///
/// Vertices are stored inline; no heap allocation is performed.
pub struct FixedPolygon<const N: usize> {
    vertices: [Point2D; N],
    len: usize,
}

impl<const N: usize> FixedPolygon<N> {
    /// Creates an empty `FixedPolygon`.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            vertices: [Point2D::new(0.0, 0.0); N],
            len: 0,
        }
    }

    /// Attempts to push a vertex.  Returns `false` if the polygon is full.
    #[inline]
    pub fn try_push(&mut self, p: Point2D) -> bool {
        if self.len >= N {
            return false;
        }
        self.vertices[self.len] = p;
        self.len += 1;
        true
    }

    /// Returns the number of vertices.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the polygon has no vertices.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a slice of the current vertices.
    #[must_use]
    #[inline]
    pub fn vertices(&self) -> &[Point2D] {
        &self.vertices[..self.len]
    }

    /// Computes the signed shoelace area (positive for CCW).
    #[must_use]
    pub fn signed_area(&self) -> f64 {
        if self.len < 3 {
            return 0.0;
        }
        let mut sum = 0.0_f64;
        let verts = self.vertices();
        for i in 0..self.len {
            let j = (i + 1) % self.len;
            sum += verts[i].x * verts[j].y;
            sum -= verts[j].x * verts[i].y;
        }
        sum * 0.5
    }

    /// Computes the absolute area using the shoelace formula.
    #[must_use]
    #[inline]
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    /// Computes the perimeter (sum of edge lengths).
    #[must_use]
    pub fn perimeter(&self) -> f64 {
        if self.len < 2 {
            return 0.0;
        }
        let verts = self.vertices();
        let mut total = 0.0_f64;
        for i in 0..self.len {
            let j = (i + 1) % self.len;
            total += verts[i].distance_to(&verts[j]);
        }
        total
    }

    /// Returns the axis-aligned bounding box, or `None` if the polygon is empty.
    #[must_use]
    pub fn bbox(&self) -> Option<BBox2D> {
        if self.is_empty() {
            return None;
        }
        let verts = self.vertices();
        let mut min_x = verts[0].x;
        let mut min_y = verts[0].y;
        let mut max_x = verts[0].x;
        let mut max_y = verts[0].y;
        for v in verts.iter().skip(1) {
            min_x = f64_min(min_x, v.x);
            min_y = f64_min(min_y, v.y);
            max_x = f64_max(max_x, v.x);
            max_y = f64_max(max_y, v.y);
        }
        Some(BBox2D::new(min_x, min_y, max_x, max_y))
    }

    /// Computes the centroid, or `None` if the polygon is empty.
    #[must_use]
    pub fn centroid(&self) -> Option<Point2D> {
        if self.is_empty() {
            return None;
        }
        let verts = self.vertices();
        let mut cx = 0.0_f64;
        let mut cy = 0.0_f64;
        for v in verts {
            cx += v.x;
            cy += v.y;
        }
        let n = self.len as f64;
        Some(Point2D::new(cx / n, cy / n))
    }
}

impl<const N: usize> Default for FixedPolygon<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ── CoordTransform ────────────────────────────────────────────────────────────

/// A 2D affine transformation stored as a 2×3 matrix.
///
/// The transformation is applied as:
/// ```text
/// x' = a*x + b*y + c
/// y' = d*x + e*y + f
/// ```
/// where `matrix = [a, b, c, d, e, f]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoordTransform {
    matrix: [f64; 6],
}

impl CoordTransform {
    /// Creates the identity transform.
    #[must_use]
    #[inline]
    pub const fn identity() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        }
    }

    /// Creates a uniform scale transform.
    #[must_use]
    #[inline]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self {
            matrix: [sx, 0.0, 0.0, 0.0, sy, 0.0],
        }
    }

    /// Creates a translation transform.
    #[must_use]
    #[inline]
    pub const fn translate(tx: f64, ty: f64) -> Self {
        Self {
            matrix: [1.0, 0.0, tx, 0.0, 1.0, ty],
        }
    }

    /// Creates a counter-clockwise rotation transform.
    #[must_use]
    pub fn rotate(angle_radians: f64) -> Self {
        let cos_a = libm_cos(angle_radians);
        let sin_a = libm_sin(angle_radians);
        Self {
            matrix: [cos_a, -sin_a, 0.0, sin_a, cos_a, 0.0],
        }
    }

    /// Composes `self` followed by `other` into a single transform.
    ///
    /// The result applies `self` first, then `other`.
    /// Mathematically this is `other_matrix × self_matrix` because affine
    /// transforms are applied right-to-left in column-vector notation.
    #[must_use]
    pub fn compose(self, other: &CoordTransform) -> Self {
        // self  = T1: applied first
        // other = T2: applied second
        // Combined = T2 × T1
        //
        // [a2 b2 c2]   [a1 b1 c1]
        // [d2 e2 f2] × [d1 e1 f1]
        // [0  0  1 ]   [0  0  1 ]
        let [a1, b1, c1, d1, e1, f1] = self.matrix;
        let [a2, b2, c2, d2, e2, f2] = other.matrix;
        Self {
            matrix: [
                a2 * a1 + b2 * d1,
                a2 * b1 + b2 * e1,
                a2 * c1 + b2 * f1 + c2,
                d2 * a1 + e2 * d1,
                d2 * b1 + e2 * e1,
                d2 * c1 + e2 * f1 + f2,
            ],
        }
    }

    /// Applies the transform to a 2D point.
    #[must_use]
    #[inline]
    pub fn apply(&self, p: Point2D) -> Point2D {
        let [a, b, c, d, e, f] = self.matrix;
        Point2D::new(a * p.x + b * p.y + c, d * p.x + e * p.y + f)
    }

    /// Applies the 2D transform to a 3D point, passing the Z coordinate through unchanged.
    #[must_use]
    #[inline]
    pub fn apply3d(&self, p: Point3D) -> Point3D {
        let p2 = self.apply(p.to_2d());
        Point3D::new(p2.x, p2.y, p.z)
    }
}

// ── Internal float helpers ────────────────────────────────────────────────────
// In no_std, f64 transcendental functions are not available from core.
// We implement them using a simple call to our own no_std libm wrappers.
//
// The workspace has `libm = "0.2"` available as a workspace dep.
// However, this crate intentionally avoids even alloc, so we inline
// simple software implementations for sqrt, sin, cos using compiler intrinsics
// which ARE available on supported targets (aarch64, x86_64, riscv).
//
// On hosted no_std targets the compiler lowers f64::sqrt() to hardware if
// the -C target-feature includes the FPU.  On soft-float targets one must
// link a libm; that is the deployer's responsibility (standard practice).
//
// We use the `unsafe` intrinsic approach through a shim that compiles cleanly.

/// Portable f64 sqrt using Newton-Raphson iteration.
///
/// Initial guess: halve the IEEE 754 biased exponent to get an approximation
/// of 2^(e/2), which is a good starting point for sqrt.
/// Five iterations are sufficient for full f64 precision.
#[inline(always)]
fn libm_sqrt(x: f64) -> f64 {
    if x <= 0.0 {
        return if x == 0.0 { 0.0 } else { f64::NAN };
    }
    // Initial guess via IEEE-754 exponent bisection.
    // For x = m * 2^e, sqrt(x) ≈ sqrt(m) * 2^(e/2).
    // We approximate sqrt(m) ≈ 1.0 and halve the exponent.
    // bits of f64: [sign(1)] [exponent(11)] [mantissa(52)]
    // bias = 1023; to halve: new_exp = (old_exp + 1023) / 2 → add 1023, halve, subtract 1023
    let bits = x.to_bits();
    // Extract biased exponent (bits 62..52), halve it (rounding toward even), reconstruct
    let biased_exp = (bits >> 52) & 0x7FF;
    let new_biased_exp = (biased_exp + 1023) >> 1;
    // Reconstruct with zero mantissa (approximation)
    let r_bits = new_biased_exp << 52;
    let mut r = f64::from_bits(r_bits);
    // Newton-Raphson: r = (r + x/r) / 2, 6 iterations → ~18 correct digits
    r = (r + x / r) * 0.5;
    r = (r + x / r) * 0.5;
    r = (r + x / r) * 0.5;
    r = (r + x / r) * 0.5;
    r = (r + x / r) * 0.5;
    r = (r + x / r) * 0.5;
    r
}

/// Portable f64 sin using Taylor series with argument reduction to [-π/4, π/4].
///
/// Uses quadrant-based reduction for maximum accuracy across the full range.
#[inline(always)]
fn libm_sin(x: f64) -> f64 {
    let (s, c) = sin_cos_core(x);
    let _ = c;
    s
}

/// Portable f64 cos using Taylor series with argument reduction to [-π/4, π/4].
#[inline(always)]
fn libm_cos(x: f64) -> f64 {
    let (s, c) = sin_cos_core(x);
    let _ = s;
    c
}

/// Core sin/cos computation via quadrant reduction + Horner-form Taylor series.
///
/// Reduces `x` to `[-π/4, π/4]` using `k = round(x / (π/2))` and then
/// routes between sin-series and cos-series based on the quadrant parity.
#[inline(always)]
fn sin_cos_core(x: f64) -> (f64, f64) {
    let pi = core::f64::consts::PI;
    let two_pi = 2.0 * pi;
    let half_pi = pi * 0.5;
    let quarter_pi = pi * 0.25;

    // Reduce to [-pi, pi]
    let mut x = x % two_pi;
    if x > pi {
        x -= two_pi;
    }
    if x < -pi {
        x += two_pi;
    }

    // Determine quadrant: k in {0, 1, 2, 3}
    // round(x / (π/2)) gives the nearest multiple of π/2
    let ratio = x / half_pi;
    // Manual round: add 0.5 with sign matching, then truncate
    let k_f = if ratio >= 0.0 {
        (ratio + 0.5) as i64 as f64
    } else {
        (ratio - 0.5) as i64 as f64
    };
    let k = k_f as i64;
    let r = x - k_f * half_pi; // |r| ≤ π/4

    // Ignore unused warning suppression: quarter_pi is used conceptually
    let _ = quarter_pi;

    // Taylor series for sin(r) and cos(r) — both accurate on [-π/4, π/4]
    let r2 = r * r;
    // sin(r) = r * (1 - r²/6 + r⁴/120 - r⁶/5040 + r⁸/362880 - r¹⁰/39916800 + r¹²/6227020800)
    let sin_r = r
        * (1.0
            + r2 * (-1.0 / 6.0
                + r2 * (1.0 / 120.0
                    + r2 * (-1.0 / 5040.0
                        + r2 * (1.0 / 362_880.0
                            + r2 * (-1.0 / 39_916_800.0 + r2 * (1.0 / 6_227_020_800.0)))))));
    // cos(r) = 1 - r²/2 + r⁴/24 - r⁶/720 + r⁸/40320 - r¹⁰/3628800 + r¹²/479001600
    let cos_r = 1.0
        + r2 * (-1.0 / 2.0
            + r2 * (1.0 / 24.0
                + r2 * (-1.0 / 720.0
                    + r2 * (1.0 / 40_320.0
                        + r2 * (-1.0 / 3_628_800.0 + r2 * (1.0 / 479_001_600.0))))));

    // Reconstruct sin(x) and cos(x) from quadrant and reduced values
    // k mod 4 determines which quadrant:
    //   0: sin(x) =  sin(r),  cos(x) =  cos(r)
    //   1: sin(x) =  cos(r),  cos(x) = -sin(r)
    //   2: sin(x) = -sin(r),  cos(x) = -cos(r)
    //   3: sin(x) = -cos(r),  cos(x) =  sin(r)
    let kmod = ((k % 4) + 4) as u64 % 4;
    match kmod {
        0 => (sin_r, cos_r),
        1 => (cos_r, -sin_r),
        2 => (-sin_r, -cos_r),
        _ => (-cos_r, sin_r),
    }
}

/// f64 min without std.
#[inline(always)]
fn f64_min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// f64 max without std.
#[inline(always)]
fn f64_max(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}
