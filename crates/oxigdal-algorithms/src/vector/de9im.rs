//! DE-9IM (Dimensionally Extended 9-Intersection Model) spatial relationships
//!
//! Implements the OGC DE-9IM model for computing topological relationships
//! between geometry pairs. The DE-9IM matrix encodes relationships between
//! the Interior (I), Boundary (B), and Exterior (E) of two geometries.
//!
//! # Matrix Layout
//!
//! ```text
//!        Interior(b)  Boundary(b)  Exterior(b)
//! I(a)   [0]=II       [1]=IB       [2]=IE
//! B(a)   [3]=BI       [4]=BB       [5]=BE
//! E(a)   [6]=EI       [7]=EB       [8]=EE
//! ```
//!
//! # Named Predicates (OGC SF)
//!
//! - **Equals**: `T*F**FFF*`
//! - **Disjoint**: `FF*FF****`
//! - **Intersects**: NOT Disjoint
//! - **Touches**: `FT*******` OR `F**T*****` OR `F***T****`
//! - **Crosses**: dimension-dependent patterns
//! - **Within**: `T*F**F***`
//! - **Contains**: `T*****FF*`
//! - **Overlaps**: dimension-dependent patterns
//! - **Covers**: `T*****FF*` OR `*T****FF*` OR `***T**FF*` OR `****T*FF*`
//! - **CoveredBy**: `T*F**F***` OR `*TF**F***` OR `**FT*F***` OR `**F*TF***`
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::de9im::{De9im, Dimension};
//!
//! // Build a matrix for overlapping polygons: "212101212"
//! let matrix = De9im::new([
//!     Dimension::Area,   // II
//!     Dimension::Line,   // IB
//!     Dimension::Area,   // IE
//!     Dimension::Line,   // BI
//!     Dimension::Point,  // BB
//!     Dimension::Line,   // BE
//!     Dimension::Area,   // EI
//!     Dimension::Line,   // EB
//!     Dimension::Area,   // EE
//! ]);
//! assert!(matrix.is_overlaps(2, 2));
//! assert!(!matrix.is_disjoint());
//! assert!(matrix.matches("2*2***2*2"));
//! ```

use crate::error::{AlgorithmError, Result};
use crate::vector::contains::{
    point_in_polygon_or_boundary, point_on_polygon_boundary, point_strictly_inside_polygon,
};
use crate::vector::intersection::SegmentIntersection;
use crate::vector::intersection::intersect_segment_segment;
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

use core::fmt;

// ---------------------------------------------------------------------------
// Dimension enum
// ---------------------------------------------------------------------------

/// Dimension of a geometric intersection component in the DE-9IM model.
///
/// Each cell in the 3x3 matrix records the maximum dimension of the
/// intersection between parts (Interior, Boundary, Exterior) of two geometries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    /// The intersection is empty (F in the matrix string, value -1)
    Empty,
    /// The intersection is a point or set of points (0 in the matrix string)
    Point,
    /// The intersection is a line or set of lines (1 in the matrix string)
    Line,
    /// The intersection is an area / surface (2 in the matrix string)
    Area,
    /// Wildcard used for pattern matching only -- never stored in a real matrix
    DontCare,
}

impl Dimension {
    /// Returns the character used in the standard DE-9IM string representation.
    #[must_use]
    pub fn to_char(self) -> char {
        match self {
            Self::Empty => 'F',
            Self::Point => '0',
            Self::Line => '1',
            Self::Area => '2',
            Self::DontCare => '*',
        }
    }

    /// Parse a single character from a DE-9IM string.
    pub fn from_char(c: char) -> Result<Self> {
        match c {
            'F' | 'f' => Ok(Self::Empty),
            '0' => Ok(Self::Point),
            '1' => Ok(Self::Line),
            '2' => Ok(Self::Area),
            '*' => Ok(Self::DontCare),
            'T' | 't' => Ok(Self::DontCare), // 'T' is a pattern char meaning "non-Empty"
            _ => Err(AlgorithmError::InvalidInput(format!(
                "invalid DE-9IM character: '{c}'"
            ))),
        }
    }

    /// Returns `true` when `self` matches pattern dimension `pat`.
    ///
    /// | Pattern | Matches |
    /// |---------|---------|
    /// | `*`     | any     |
    /// | `T`     | Point, Line, Area (non-Empty) |
    /// | `F`     | Empty   |
    /// | `0`     | Point   |
    /// | `1`     | Line    |
    /// | `2`     | Area    |
    fn matches_pattern(self, pat: char) -> bool {
        match pat {
            '*' => true,
            'T' | 't' => self != Self::Empty,
            'F' | 'f' => self == Self::Empty,
            '0' => self == Self::Point,
            '1' => self == Self::Line,
            '2' => self == Self::Area,
            _ => false,
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

// ---------------------------------------------------------------------------
// De9im struct
// ---------------------------------------------------------------------------

/// A DE-9IM (Dimensionally Extended 9-Intersection Model) matrix.
///
/// The nine cells are stored in row-major order:
///
/// ```text
/// [II, IB, IE,  BI, BB, BE,  EI, EB, EE]
/// ```
///
/// where `I` = Interior, `B` = Boundary, `E` = Exterior,
/// and the first letter refers to geometry **a**, the second to geometry **b**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct De9im {
    cells: [Dimension; 9],
}

impl De9im {
    // -- cell index constants --
    /// Interior-Interior
    pub const II: usize = 0;
    /// Interior-Boundary
    pub const IB: usize = 1;
    /// Interior-Exterior
    pub const IE: usize = 2;
    /// Boundary-Interior
    pub const BI: usize = 3;
    /// Boundary-Boundary
    pub const BB: usize = 4;
    /// Boundary-Exterior
    pub const BE: usize = 5;
    /// Exterior-Interior
    pub const EI: usize = 6;
    /// Exterior-Boundary
    pub const EB: usize = 7;
    /// Exterior-Exterior
    pub const EE: usize = 8;

    /// Create a matrix from an explicit 9-element array.
    #[must_use]
    pub const fn new(cells: [Dimension; 9]) -> Self {
        Self { cells }
    }

    /// Parse a 9-character DE-9IM string such as `"212101212"`.
    pub fn from_str(s: &str) -> Result<Self> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() != 9 {
            return Err(AlgorithmError::InvalidInput(format!(
                "DE-9IM string must be exactly 9 characters, got {}",
                chars.len()
            )));
        }
        let mut cells = [Dimension::Empty; 9];
        for (i, &ch) in chars.iter().enumerate() {
            cells[i] = Dimension::from_char(ch)?;
        }
        Ok(Self { cells })
    }

    /// Access a single cell by index (0..9).
    #[must_use]
    pub fn get(&self, index: usize) -> Dimension {
        if index < 9 {
            self.cells[index]
        } else {
            Dimension::Empty
        }
    }

    /// Set a single cell by index (0..9).
    pub fn set(&mut self, index: usize, dim: Dimension) {
        if index < 9 {
            self.cells[index] = dim;
        }
    }

    /// Return the 9-character string form (e.g. `"212101212"`).
    #[must_use]
    pub fn to_string_repr(&self) -> String {
        self.cells.iter().map(|d| d.to_char()).collect()
    }

    /// Return the transposed matrix (swap a <-> b).
    #[must_use]
    pub fn transpose(&self) -> Self {
        Self::new([
            self.cells[Self::II],
            self.cells[Self::BI],
            self.cells[Self::EI],
            self.cells[Self::IB],
            self.cells[Self::BB],
            self.cells[Self::EB],
            self.cells[Self::IE],
            self.cells[Self::BE],
            self.cells[Self::EE],
        ])
    }

    // -----------------------------------------------------------------------
    // Pattern matching
    // -----------------------------------------------------------------------

    /// Test whether this matrix matches a 9-character pattern string.
    ///
    /// Pattern characters: `T` (non-empty), `F` (empty), `*` (any),
    /// `0` (Point), `1` (Line), `2` (Area).
    #[must_use]
    pub fn matches(&self, pattern: &str) -> bool {
        let chars: Vec<char> = pattern.chars().collect();
        if chars.len() != 9 {
            return false;
        }
        self.cells
            .iter()
            .zip(chars.iter())
            .all(|(dim, &pat)| dim.matches_pattern(pat))
    }

    // -----------------------------------------------------------------------
    // Named OGC predicates
    // -----------------------------------------------------------------------

    /// OGC **Equals**: `T*F**FFF*`
    ///
    /// Two geometries are topologically equal when their interiors intersect
    /// and no part of either geometry lies in the exterior of the other.
    #[must_use]
    pub fn is_equals(&self) -> bool {
        self.matches("T*F**FFF*")
    }

    /// OGC **Disjoint**: `FF*FF****`
    ///
    /// Two geometries are disjoint when they share no points in common.
    #[must_use]
    pub fn is_disjoint(&self) -> bool {
        self.matches("FF*FF****")
    }

    /// OGC **Intersects**: NOT Disjoint.
    #[must_use]
    pub fn is_intersects(&self) -> bool {
        !self.is_disjoint()
    }

    /// OGC **Touches**: the geometries share boundary but not interior.
    ///
    /// Matches `FT*******` OR `F**T*****` OR `F***T****`.
    #[must_use]
    pub fn is_touches(&self) -> bool {
        self.matches("FT*******") || self.matches("F**T*****") || self.matches("F***T****")
    }

    /// OGC **Crosses** (requires the topological dimensions of the two
    /// geometries).
    ///
    /// - Point/Line, Point/Area, Line/Area (dim_a < dim_b): pattern `T*T******`
    /// - Line/Line (dim_a == dim_b == 1): pattern `0********`
    /// - Polygon/Polygon (dim_a == dim_b == 2): always `false` (undefined)
    #[must_use]
    pub fn is_crosses(&self, dim_a: u8, dim_b: u8) -> bool {
        if dim_a < dim_b {
            // lower-dim crosses higher-dim
            self.matches("T*T******")
        } else if dim_a > dim_b {
            // reverse: same test on transposed matrix
            self.transpose().matches("T*T******")
        } else if dim_a == 1 && dim_b == 1 {
            // Line/Line
            self.matches("0********")
        } else {
            // Polygon/Polygon (same dim 2) -- crosses is undefined, return false
            false
        }
    }

    /// OGC **Within**: `T*F**F***`
    ///
    /// Geometry a is within geometry b when every point of a lies inside
    /// (interior or boundary of) b and the interiors intersect.
    #[must_use]
    pub fn is_within(&self) -> bool {
        self.matches("T*F**F***")
    }

    /// OGC **Contains**: `T*****FF*`
    ///
    /// Geometry a contains geometry b when within(b, a) is true.
    #[must_use]
    pub fn is_contains(&self) -> bool {
        self.matches("T*****FF*")
    }

    /// OGC **Overlaps** (requires the topological dimensions of the two
    /// geometries).
    ///
    /// - Point/Point or Area/Area (dim_a == dim_b, both != 1): `T*T***T**`
    /// - Line/Line (dim_a == dim_b == 1): `1*T***T**`
    /// - Different dimensions: always `false`
    #[must_use]
    pub fn is_overlaps(&self, dim_a: u8, dim_b: u8) -> bool {
        if dim_a != dim_b {
            return false;
        }
        if dim_a == 1 {
            self.matches("1*T***T**")
        } else {
            self.matches("T*T***T**")
        }
    }

    /// OGC **Covers**: `T*****FF*` OR `*T****FF*` OR `***T**FF*` OR `****T*FF*`
    ///
    /// Geometry a covers geometry b when every point of b is a point of a.
    #[must_use]
    pub fn is_covers(&self) -> bool {
        self.matches("T*****FF*")
            || self.matches("*T****FF*")
            || self.matches("***T**FF*")
            || self.matches("****T*FF*")
    }

    /// OGC **CoveredBy**: `T*F**F***` OR `*TF**F***` OR `**FT*F***` OR `**F*TF***`
    ///
    /// Geometry a is covered by geometry b when every point of a is a point of b.
    #[must_use]
    pub fn is_covered_by(&self) -> bool {
        self.matches("T*F**F***")
            || self.matches("*TF**F***")
            || self.matches("**FT*F***")
            || self.matches("**F*TF***")
    }
}

impl fmt::Display for De9im {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_repr())
    }
}

// ---------------------------------------------------------------------------
// Relate: compute the DE-9IM matrix for geometry pairs
// ---------------------------------------------------------------------------

/// Compute the DE-9IM matrix for two polygons.
///
/// Classifies every topological pair (Interior, Boundary, Exterior) of a and b
/// and records the maximum dimension of each intersection component.
///
/// # Errors
///
/// Returns an error if either polygon has fewer than 4 exterior coordinates.
pub fn relate_polygons(a: &Polygon, b: &Polygon) -> Result<De9im> {
    validate_polygon(a, "relate_polygons", "polygon a")?;
    validate_polygon(b, "relate_polygons", "polygon b")?;

    let mut matrix = [Dimension::Empty; 9];

    // -- EE is always Area for polygons (infinite exterior meets infinite exterior)
    matrix[De9im::EE] = Dimension::Area;

    // -- Phase 1: Classify boundary vertices and edge midpoints --
    let mut a_bdry_in_b_interior = false;
    let mut a_bdry_on_b_boundary = false;
    let mut a_bdry_in_b_exterior = false;

    classify_boundary_against_polygon(
        a,
        b,
        &mut a_bdry_in_b_interior,
        &mut a_bdry_on_b_boundary,
        &mut a_bdry_in_b_exterior,
    );

    let mut b_bdry_in_a_interior = false;
    let mut b_bdry_on_a_boundary = false;
    let mut b_bdry_in_a_exterior = false;

    classify_boundary_against_polygon(
        b,
        a,
        &mut b_bdry_in_a_interior,
        &mut b_bdry_on_a_boundary,
        &mut b_bdry_in_a_exterior,
    );

    // -- Phase 2: Interior classification --
    // Sample points strictly inside each polygon and classify against the other.
    // Also generate intersection-region sample points for robustness.
    let mut a_int_in_b_interior = false;
    let mut a_int_on_b_boundary = false;
    let mut a_int_in_b_exterior = false;
    let mut b_int_in_a_interior = false;
    let mut b_int_on_a_boundary = false;
    let mut b_int_in_a_exterior = false;

    // Sample interior of A against B
    let a_samples = interior_sample_points(a);
    for pt in &a_samples {
        classify_point_against_polygon(
            pt,
            b,
            &mut a_int_in_b_interior,
            &mut a_int_on_b_boundary,
            &mut a_int_in_b_exterior,
        );
    }

    // Sample interior of B against A
    let b_samples = interior_sample_points(b);
    for pt in &b_samples {
        classify_point_against_polygon(
            pt,
            a,
            &mut b_int_in_a_interior,
            &mut b_int_on_a_boundary,
            &mut b_int_in_a_exterior,
        );
    }

    // Phase 2b: Generate additional samples near boundary intersection points.
    // When boundary edges cross, sample points slightly offset from the crossing
    // to detect the interior-interior overlap that vertex sampling might miss.
    let cross_samples = generate_crossing_interior_samples(a, b);
    for pt in &cross_samples {
        // These points are designed to be strictly inside both polygons
        if point_strictly_inside_polygon(pt, a) && point_strictly_inside_polygon(pt, b) {
            a_int_in_b_interior = true;
            b_int_in_a_interior = true;
        }
    }

    // -- Phase 3: Boundary-Boundary intersection dimension --
    let bb_dim = compute_boundary_boundary_dim(a, b);

    // -- Phase 4: Fill matrix from classification flags --

    // II: Interior(a) intersects Interior(b)
    if a_int_in_b_interior || b_int_in_a_interior {
        matrix[De9im::II] = Dimension::Area;
    }

    // IB: Interior(a) intersects Boundary(b)
    // If boundary of B passes through interior of A, this is at least Line.
    if b_bdry_in_a_interior || a_int_on_b_boundary {
        matrix[De9im::IB] = Dimension::Line;
    }

    // IE: Interior(a) intersects Exterior(b)
    if a_int_in_b_exterior {
        matrix[De9im::IE] = Dimension::Area;
    }

    // BI: Boundary(a) intersects Interior(b)
    if a_bdry_in_b_interior {
        matrix[De9im::BI] = Dimension::Line;
    }

    // BB: Boundary(a) intersects Boundary(b)
    if a_bdry_on_b_boundary || b_bdry_on_a_boundary || bb_dim != Dimension::Empty {
        matrix[De9im::BB] = bb_dim;
    }

    // BE: Boundary(a) intersects Exterior(b)
    if a_bdry_in_b_exterior {
        matrix[De9im::BE] = Dimension::Line;
    }

    // EI: Exterior(a) intersects Interior(b)
    if b_int_in_a_exterior {
        matrix[De9im::EI] = Dimension::Area;
    }

    // EB: Exterior(a) intersects Boundary(b)
    if b_bdry_in_a_exterior {
        matrix[De9im::EB] = Dimension::Line;
    }

    Ok(De9im::new(matrix))
}

/// Compute the DE-9IM matrix for a point and a polygon.
///
/// # Errors
///
/// Returns an error if the polygon has fewer than 4 exterior coordinates.
pub fn relate_point_polygon(pt: &Point, poly: &Polygon) -> Result<De9im> {
    validate_polygon(poly, "relate_point_polygon", "polygon")?;

    let coord = &pt.coord;
    let mut matrix = [Dimension::Empty; 9];

    // EE is always Area (exterior is 2D plane minus bounded geometry)
    matrix[De9im::EE] = Dimension::Area;
    // IE: Point interior is 0-dim; it is always in exterior of polygon or not
    // EI: polygon interior is 2-dim area; exterior of point is everything else

    // The point has no boundary (dimension 0 -> boundary is empty)
    // so IB, BI, BB, BE, EB all depend only on whether the point falls in
    // the polygon's interior, boundary, or exterior.

    if point_on_polygon_boundary(coord, poly) {
        // Point is on the polygon boundary
        matrix[De9im::II] = Dimension::Empty; // point interior does not meet poly interior
        matrix[De9im::IB] = Dimension::Point; // point interior meets poly boundary
        matrix[De9im::IE] = Dimension::Empty; // point interior does not meet poly exterior
        // Boundary of point is empty (dim 0 has no boundary), so BI=BB=BE=F
        matrix[De9im::EI] = Dimension::Area; // exterior of point covers poly interior
        matrix[De9im::EB] = Dimension::Line; // exterior of point covers rest of poly boundary
    } else if point_strictly_inside_polygon(coord, poly) {
        // Point is strictly inside the polygon
        matrix[De9im::II] = Dimension::Point; // point interior meets poly interior
        matrix[De9im::IB] = Dimension::Empty;
        matrix[De9im::IE] = Dimension::Empty;
        matrix[De9im::EI] = Dimension::Area;
        matrix[De9im::EB] = Dimension::Line;
    } else {
        // Point is exterior to the polygon
        matrix[De9im::II] = Dimension::Empty;
        matrix[De9im::IB] = Dimension::Empty;
        matrix[De9im::IE] = Dimension::Point;
        matrix[De9im::EI] = Dimension::Area;
        matrix[De9im::EB] = Dimension::Line;
    }

    Ok(De9im::new(matrix))
}

/// Compute the DE-9IM matrix for a line string and a polygon.
///
/// Walks each segment of the line and classifies it against the polygon's
/// interior, boundary, and exterior.
///
/// # Errors
///
/// Returns an error if the polygon has fewer than 4 exterior coordinates or
/// the line has fewer than 2 coordinates.
pub fn relate_line_polygon(line: &LineString, poly: &Polygon) -> Result<De9im> {
    validate_polygon(poly, "relate_line_polygon", "polygon")?;
    if line.coords.len() < 2 {
        return Err(AlgorithmError::InsufficientData {
            operation: "relate_line_polygon",
            message: "line must have at least 2 coordinates".to_string(),
        });
    }

    let mut matrix = [Dimension::Empty; 9];
    matrix[De9im::EE] = Dimension::Area;

    // Classify each vertex and segment midpoint of the line against the polygon
    let mut line_has_interior_in_poly_interior = false;
    let mut line_has_interior_on_poly_boundary = false;
    let mut line_has_interior_in_poly_exterior = false;

    // Classify line vertices
    for coord in &line.coords {
        if point_on_polygon_boundary(coord, poly) {
            line_has_interior_on_poly_boundary = true;
        } else if point_strictly_inside_polygon(coord, poly) {
            line_has_interior_in_poly_interior = true;
        } else {
            line_has_interior_in_poly_exterior = true;
        }
    }

    // Classify segment midpoints for finer resolution
    for i in 0..line.coords.len().saturating_sub(1) {
        let mid = midpoint(&line.coords[i], &line.coords[i + 1]);
        if point_on_polygon_boundary(&mid, poly) {
            line_has_interior_on_poly_boundary = true;
        } else if point_strictly_inside_polygon(&mid, poly) {
            line_has_interior_in_poly_interior = true;
        } else {
            line_has_interior_in_poly_exterior = true;
        }
    }

    // Line endpoints are the boundary of a LineString (if not closed)
    let line_is_closed = line.coords.len() >= 3
        && coords_equal(&line.coords[0], &line.coords[line.coords.len() - 1]);

    let mut line_boundary_in_poly_interior = false;
    let mut line_boundary_on_poly_boundary = false;
    let mut line_boundary_in_poly_exterior = false;

    if !line_is_closed && line.coords.len() >= 2 {
        let endpoints = [&line.coords[0], &line.coords[line.coords.len() - 1]];
        for ep in &endpoints {
            if point_on_polygon_boundary(ep, poly) {
                line_boundary_on_poly_boundary = true;
            } else if point_strictly_inside_polygon(ep, poly) {
                line_boundary_in_poly_interior = true;
            } else {
                line_boundary_in_poly_exterior = true;
            }
        }
    }

    // Check if the polygon boundary intersects the line
    let boundary_intersections = count_boundary_line_intersections(line, poly);

    // II: line interior meets polygon interior
    if line_has_interior_in_poly_interior {
        matrix[De9im::II] = Dimension::Line;
    }

    // IB: line interior meets polygon boundary
    if line_has_interior_on_poly_boundary || boundary_intersections > 0 {
        if has_segment_on_polygon_boundary(line, poly) {
            matrix[De9im::IB] = Dimension::Line;
        } else {
            matrix[De9im::IB] = Dimension::Point;
        }
    }

    // IE: line interior in polygon exterior
    if line_has_interior_in_poly_exterior {
        matrix[De9im::IE] = Dimension::Line;
    }

    // BI: line boundary (endpoints) meets polygon interior
    if line_boundary_in_poly_interior {
        matrix[De9im::BI] = Dimension::Point;
    }

    // BB: line boundary meets polygon boundary
    if line_boundary_on_poly_boundary {
        matrix[De9im::BB] = Dimension::Point;
    }

    // BE: line boundary in polygon exterior
    if line_boundary_in_poly_exterior {
        matrix[De9im::BE] = Dimension::Point;
    }

    // EI: polygon interior meets line exterior -- always Area if polygon exists
    matrix[De9im::EI] = Dimension::Area;

    // EB: polygon boundary meets line exterior -- always Line
    matrix[De9im::EB] = Dimension::Line;

    Ok(De9im::new(matrix))
}

/// High-level relate dispatcher for Polygon-Polygon, Point-Polygon, Line-Polygon.
///
/// # Errors
///
/// Returns an error if geometries are invalid or the combination is unsupported.
pub fn relate(a: &Polygon, b: &Polygon) -> Result<De9im> {
    relate_polygons(a, b)
}

// ---------------------------------------------------------------------------
// New predicate traits (Equals, Covers, CoveredBy)
// ---------------------------------------------------------------------------

/// Trait for geometries that support the **Equals** predicate.
pub trait EqualsPredicate {
    /// Tests if this geometry is topologically equal to another.
    fn equals_topo(&self, other: &Self) -> Result<bool>;
}

/// Trait for geometries that support the **Covers** predicate.
pub trait CoversPredicate {
    /// Tests if this geometry covers another (every point of other is a point of self).
    fn covers(&self, other: &Self) -> Result<bool>;
}

/// Trait for geometries that support the **CoveredBy** predicate.
pub trait CoveredByPredicate {
    /// Tests if this geometry is covered by another.
    fn covered_by(&self, other: &Self) -> Result<bool>;
}

// Polygon implementations delegating to DE-9IM

impl EqualsPredicate for Polygon {
    fn equals_topo(&self, other: &Self) -> Result<bool> {
        let m = relate_polygons(self, other)?;
        Ok(m.is_equals())
    }
}

impl CoversPredicate for Polygon {
    fn covers(&self, other: &Self) -> Result<bool> {
        let m = relate_polygons(self, other)?;
        Ok(m.is_covers())
    }
}

impl CoveredByPredicate for Polygon {
    fn covered_by(&self, other: &Self) -> Result<bool> {
        let m = relate_polygons(self, other)?;
        Ok(m.is_covered_by())
    }
}

// Point implementations

impl EqualsPredicate for Point {
    fn equals_topo(&self, other: &Self) -> Result<bool> {
        Ok(coords_equal(&self.coord, &other.coord))
    }
}

impl CoversPredicate for Point {
    fn covers(&self, other: &Self) -> Result<bool> {
        // A point covers another point iff they are equal
        Ok(coords_equal(&self.coord, &other.coord))
    }
}

impl CoveredByPredicate for Point {
    fn covered_by(&self, other: &Self) -> Result<bool> {
        Ok(coords_equal(&self.coord, &other.coord))
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Validate a polygon has sufficient coordinates.
fn validate_polygon(poly: &Polygon, operation: &'static str, name: &str) -> Result<()> {
    if poly.exterior.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation,
            message: format!("{name} exterior must have at least 4 coordinates"),
        });
    }
    Ok(())
}

/// Test coordinate equality within epsilon.
fn coords_equal(a: &Coordinate, b: &Coordinate) -> bool {
    (a.x - b.x).abs() < f64::EPSILON && (a.y - b.y).abs() < f64::EPSILON
}

/// Compute the midpoint of two coordinates.
fn midpoint(a: &Coordinate, b: &Coordinate) -> Coordinate {
    Coordinate::new_2d((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

/// Classify a point against a polygon as interior/boundary/exterior and set flags.
fn classify_point_against_polygon(
    pt: &Coordinate,
    poly: &Polygon,
    in_interior: &mut bool,
    on_boundary: &mut bool,
    in_exterior: &mut bool,
) {
    if point_on_polygon_boundary(pt, poly) {
        *on_boundary = true;
    } else if point_strictly_inside_polygon(pt, poly) {
        *in_interior = true;
    } else {
        *in_exterior = true;
    }
}

/// Classify boundary vertices and edge midpoints of `source` against `target`.
fn classify_boundary_against_polygon(
    source: &Polygon,
    target: &Polygon,
    in_interior: &mut bool,
    on_boundary: &mut bool,
    in_exterior: &mut bool,
) {
    // Classify each vertex
    for coord in &source.exterior.coords {
        classify_point_against_polygon(coord, target, in_interior, on_boundary, in_exterior);
    }
    // Classify edge midpoints for finer resolution
    for i in 0..source.exterior.coords.len().saturating_sub(1) {
        let mid = midpoint(&source.exterior.coords[i], &source.exterior.coords[i + 1]);
        classify_point_against_polygon(&mid, target, in_interior, on_boundary, in_exterior);
    }
}

/// Generate sample points near boundary crossings that are likely strictly
/// inside both polygons.
///
/// When two polygon boundaries cross at a point, the four quadrants around
/// that crossing alternate between "inside both", "inside A only",
/// "inside B only", and "outside both". We nudge slightly along the bisector
/// of the crossing edge normals to find the "inside both" quadrant.
fn generate_crossing_interior_samples(a: &Polygon, b: &Polygon) -> Vec<Coordinate> {
    let mut samples = Vec::new();
    let a_coords = &a.exterior.coords;
    let b_coords = &b.exterior.coords;
    let eps = 1e-6;

    for i in 0..a_coords.len().saturating_sub(1) {
        for j in 0..b_coords.len().saturating_sub(1) {
            if let SegmentIntersection::Point(pt) = intersect_segment_segment(
                &a_coords[i],
                &a_coords[i + 1],
                &b_coords[j],
                &b_coords[j + 1],
            ) {
                // Compute inward-pointing normals of each edge
                let a_dx = a_coords[i + 1].x - a_coords[i].x;
                let a_dy = a_coords[i + 1].y - a_coords[i].y;
                let b_dx = b_coords[j + 1].x - b_coords[j].x;
                let b_dy = b_coords[j + 1].y - b_coords[j].y;

                // Inward normal candidates (both orientations)
                let normals = [(a_dy, -a_dx), (-a_dy, a_dx), (b_dy, -b_dx), (-b_dy, b_dx)];

                // Try combinations of normal directions to find inside-both
                for &(nx_a, ny_a) in &normals[..2] {
                    for &(nx_b, ny_b) in &normals[2..] {
                        let nx = nx_a + nx_b;
                        let ny = ny_a + ny_b;
                        let len = (nx * nx + ny * ny).sqrt();
                        if len < f64::EPSILON {
                            continue;
                        }
                        let candidate =
                            Coordinate::new_2d(pt.x + eps * nx / len, pt.y + eps * ny / len);
                        if point_strictly_inside_polygon(&candidate, a)
                            && point_strictly_inside_polygon(&candidate, b)
                        {
                            samples.push(candidate);
                        }
                    }
                }
            }
        }
        if samples.len() >= 4 {
            break;
        }
    }
    samples
}

/// Generate sample points known to be in the interior of a polygon.
///
/// Uses edge midpoints projected slightly inward, and the centroid as a
/// fallback. This is not bulletproof for extremely non-convex shapes but
/// covers the vast majority of practical geometries.
fn interior_sample_points(poly: &Polygon) -> Vec<Coordinate> {
    let mut samples = Vec::new();
    let n = poly.exterior.coords.len();
    if n < 4 {
        return samples;
    }

    // Centroid of the exterior ring (arithmetic mean of vertices)
    let (mut cx, mut cy) = (0.0_f64, 0.0_f64);
    // Exclude the closing vertex (last == first)
    let vertex_count = n - 1;
    for coord in &poly.exterior.coords[..vertex_count] {
        cx += coord.x;
        cy += coord.y;
    }
    if vertex_count > 0 {
        cx /= vertex_count as f64;
        cy /= vertex_count as f64;
    }
    let centroid = Coordinate::new_2d(cx, cy);

    if point_strictly_inside_polygon(&centroid, poly) {
        samples.push(centroid);
    }

    // Edge midpoints pulled slightly toward centroid
    for i in 0..vertex_count {
        let mid = midpoint(&poly.exterior.coords[i], &poly.exterior.coords[i + 1]);
        // Pull 10% toward centroid
        let pulled = Coordinate::new_2d(mid.x + (cx - mid.x) * 0.1, mid.y + (cy - mid.y) * 0.1);
        if point_strictly_inside_polygon(&pulled, poly) {
            samples.push(pulled);
            if samples.len() >= 5 {
                break;
            }
        }
    }

    // Fallback: if no sample found, try a point grid inside the bounding box
    if samples.is_empty() {
        if let Some((min_x, min_y, max_x, max_y)) = poly.bounds() {
            let step_x = (max_x - min_x) / 5.0;
            let step_y = (max_y - min_y) / 5.0;
            'outer: for ix in 1..5 {
                for iy in 1..5 {
                    let pt = Coordinate::new_2d(
                        min_x + step_x * (ix as f64),
                        min_y + step_y * (iy as f64),
                    );
                    if point_strictly_inside_polygon(&pt, poly) {
                        samples.push(pt);
                        if samples.len() >= 3 {
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    samples
}

/// Compute the dimension of the Boundary-Boundary intersection between two polygons.
///
/// - No intersection -> Empty
/// - Finite intersection points only -> Point
/// - Collinear edge overlaps -> Line
fn compute_boundary_boundary_dim(a: &Polygon, b: &Polygon) -> Dimension {
    let a_coords = &a.exterior.coords;
    let b_coords = &b.exterior.coords;

    let mut has_point_intersection = false;
    let mut has_line_intersection = false;

    for i in 0..a_coords.len().saturating_sub(1) {
        for j in 0..b_coords.len().saturating_sub(1) {
            match intersect_segment_segment(
                &a_coords[i],
                &a_coords[i + 1],
                &b_coords[j],
                &b_coords[j + 1],
            ) {
                SegmentIntersection::Point(_) => {
                    has_point_intersection = true;
                }
                SegmentIntersection::Overlap(_, _) => {
                    has_line_intersection = true;
                }
                SegmentIntersection::None => {}
            }
        }
    }

    if has_line_intersection {
        Dimension::Line
    } else if has_point_intersection {
        Dimension::Point
    } else {
        Dimension::Empty
    }
}

/// Count the number of intersection points between a line and the polygon boundary.
fn count_boundary_line_intersections(line: &LineString, poly: &Polygon) -> usize {
    let mut count = 0;
    let ring = &poly.exterior.coords;
    for i in 0..line.coords.len().saturating_sub(1) {
        for j in 0..ring.len().saturating_sub(1) {
            match intersect_segment_segment(
                &line.coords[i],
                &line.coords[i + 1],
                &ring[j],
                &ring[j + 1],
            ) {
                SegmentIntersection::Point(_) | SegmentIntersection::Overlap(_, _) => {
                    count += 1;
                }
                SegmentIntersection::None => {}
            }
        }
    }
    count
}

/// Check if any segment of the line lies entirely on the polygon boundary.
fn has_segment_on_polygon_boundary(line: &LineString, poly: &Polygon) -> bool {
    let ring = &poly.exterior.coords;
    for i in 0..line.coords.len().saturating_sub(1) {
        for j in 0..ring.len().saturating_sub(1) {
            if let SegmentIntersection::Overlap(_, _) = intersect_segment_segment(
                &line.coords[i],
                &line.coords[i + 1],
                &ring[j],
                &ring[j + 1],
            ) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AlgorithmError;

    type TestResult = core::result::Result<(), Box<dyn std::error::Error>>;

    /// Helper: create a square polygon from (x0,y0) to (x1,y1).
    fn make_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(x0, y0),
            Coordinate::new_2d(x1, y0),
            Coordinate::new_2d(x1, y1),
            Coordinate::new_2d(x0, y1),
            Coordinate::new_2d(x0, y0),
        ];
        let ext = LineString::new(coords).map_err(AlgorithmError::Core)?;
        Polygon::new(ext, vec![]).map_err(AlgorithmError::Core)
    }

    // =======================================================================
    // Dimension / De9im unit tests
    // =======================================================================

    #[test]
    fn test_dimension_to_char() {
        assert_eq!(Dimension::Empty.to_char(), 'F');
        assert_eq!(Dimension::Point.to_char(), '0');
        assert_eq!(Dimension::Line.to_char(), '1');
        assert_eq!(Dimension::Area.to_char(), '2');
        assert_eq!(Dimension::DontCare.to_char(), '*');
    }

    #[test]
    fn test_dimension_from_char() -> TestResult {
        assert_eq!(Dimension::from_char('F')?, Dimension::Empty);
        assert_eq!(Dimension::from_char('0')?, Dimension::Point);
        assert_eq!(Dimension::from_char('1')?, Dimension::Line);
        assert_eq!(Dimension::from_char('2')?, Dimension::Area);
        assert_eq!(Dimension::from_char('*')?, Dimension::DontCare);
        assert!(Dimension::from_char('X').is_err());
        Ok(())
    }

    #[test]
    fn test_de9im_from_str() -> TestResult {
        let m = De9im::from_str("212101212")?;
        assert_eq!(m.get(De9im::II), Dimension::Area);
        assert_eq!(m.get(De9im::IB), Dimension::Line);
        assert_eq!(m.get(De9im::IE), Dimension::Area);
        assert_eq!(m.get(De9im::BI), Dimension::Line);
        assert_eq!(m.get(De9im::BB), Dimension::Point);
        assert_eq!(m.get(De9im::BE), Dimension::Line);
        assert_eq!(m.get(De9im::EI), Dimension::Area);
        assert_eq!(m.get(De9im::EB), Dimension::Line);
        assert_eq!(m.get(De9im::EE), Dimension::Area);
        Ok(())
    }

    #[test]
    fn test_de9im_display() -> TestResult {
        let m = De9im::from_str("212101212")?;
        assert_eq!(format!("{m}"), "212101212");
        Ok(())
    }

    #[test]
    fn test_de9im_matches_basic() -> TestResult {
        let m = De9im::from_str("212101212")?;
        assert!(m.matches("2*2***2*2")); // wildcard
        assert!(m.matches("T*T***T**")); // T matches non-empty
        assert!(!m.matches("FF*FF****")); // disjoint pattern - should not match
        Ok(())
    }

    #[test]
    fn test_de9im_transpose() -> TestResult {
        let m = De9im::from_str("212101212")?;
        let t = m.transpose();
        // Transpose swaps rows and columns:
        // II stays, IB<->BI, IE<->EI, BB stays, BE<->EB, EE stays
        assert_eq!(t.get(De9im::II), Dimension::Area); // same
        assert_eq!(t.get(De9im::IB), Dimension::Line); // was BI=1
        assert_eq!(t.get(De9im::IE), Dimension::Area); // was EI=2
        assert_eq!(t.get(De9im::BI), Dimension::Line); // was IB=1
        assert_eq!(t.get(De9im::BB), Dimension::Point); // same
        assert_eq!(t.get(De9im::BE), Dimension::Line); // was EB=1
        assert_eq!(t.get(De9im::EI), Dimension::Area); // was IE=2
        assert_eq!(t.get(De9im::EB), Dimension::Line); // was BE=1
        assert_eq!(t.get(De9im::EE), Dimension::Area); // same
        Ok(())
    }

    #[test]
    fn test_de9im_from_str_invalid_length() {
        assert!(De9im::from_str("212").is_err());
        assert!(De9im::from_str("2121012121").is_err());
    }

    #[test]
    fn test_de9im_get_out_of_bounds() {
        let m = De9im::new([Dimension::Empty; 9]);
        assert_eq!(m.get(99), Dimension::Empty);
    }

    // =======================================================================
    // Named predicate tests on synthetic matrices
    // =======================================================================

    #[test]
    fn test_is_equals_synthetic() -> TestResult {
        // T*F**FFF* -> equals
        let m = De9im::from_str("2FFF1FFF2")?;
        assert!(m.is_equals());
        // Overlapping polygons should not be equals
        let m2 = De9im::from_str("212101212")?;
        assert!(!m2.is_equals());
        Ok(())
    }

    #[test]
    fn test_is_disjoint_synthetic() -> TestResult {
        let m = De9im::from_str("FF2FF1212")?;
        assert!(m.is_disjoint());
        assert!(!m.is_intersects());

        let m2 = De9im::from_str("212101212")?;
        assert!(!m2.is_disjoint());
        assert!(m2.is_intersects());
        Ok(())
    }

    #[test]
    fn test_is_touches_synthetic() -> TestResult {
        // FT******* pattern (boundary contact, no interior contact)
        let m = De9im::from_str("F11FF0212")?;
        assert!(m.is_touches());

        let m2 = De9im::from_str("212101212")?;
        assert!(!m2.is_touches());
        Ok(())
    }

    #[test]
    fn test_is_crosses_synthetic() -> TestResult {
        // Line/Polygon crossing: T*T****** with dim_a=1, dim_b=2
        let m = De9im::from_str("1020F1102")?;
        assert!(m.is_crosses(1, 2));
        // Polygon/Polygon: always false
        assert!(!m.is_crosses(2, 2));

        // Line/Line crossing: 0********
        let m2 = De9im::from_str("0FFFFFFFF")?;
        assert!(m2.is_crosses(1, 1));
        Ok(())
    }

    #[test]
    fn test_is_within_synthetic() -> TestResult {
        // T*F**F***
        let m = De9im::from_str("2FF1FF212")?;
        assert!(m.is_within());
        assert!(!m.is_contains()); // within != contains
        Ok(())
    }

    #[test]
    fn test_is_contains_synthetic() -> TestResult {
        // T*****FF*
        let m = De9im::from_str("212101FF2")?;
        assert!(m.is_contains());
        Ok(())
    }

    #[test]
    fn test_is_overlaps_synthetic() -> TestResult {
        // Area/Area: T*T***T**
        let m = De9im::from_str("212101212")?;
        assert!(m.is_overlaps(2, 2));
        // Different dimensions: false
        assert!(!m.is_overlaps(1, 2));

        // Line/Line: 1*T***T**
        let m2 = De9im::from_str("1FT1FFT1F")?;
        assert!(m2.is_overlaps(1, 1));
        Ok(())
    }

    #[test]
    fn test_is_covers_synthetic() -> TestResult {
        // T*****FF*
        let m = De9im::from_str("2FF1FFFF2")?;
        assert!(m.is_covers());
        Ok(())
    }

    #[test]
    fn test_is_covered_by_synthetic() -> TestResult {
        // T*F**F***
        let m = De9im::from_str("2FF0FF212")?;
        assert!(m.is_covered_by());
        Ok(())
    }

    // =======================================================================
    // relate_polygons: geometric tests
    // =======================================================================

    #[test]
    fn test_relate_disjoint_squares() -> TestResult {
        let a = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let b = make_rect(10.0, 10.0, 14.0, 14.0)?;
        let m = relate_polygons(&a, &b)?;
        assert!(m.is_disjoint(), "disjoint squares: matrix = {m}");
        assert!(!m.is_intersects());
        // Exact matrix should be FF2FF1212
        assert_eq!(m.get(De9im::II), Dimension::Empty);
        assert_eq!(m.get(De9im::EE), Dimension::Area);
        Ok(())
    }

    #[test]
    fn test_relate_overlapping_squares() -> TestResult {
        let a = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let b = make_rect(2.0, 2.0, 6.0, 6.0)?;
        let m = relate_polygons(&a, &b)?;
        assert!(m.is_intersects(), "overlapping squares: matrix = {m}");
        assert!(!m.is_disjoint());
        assert!(m.is_overlaps(2, 2), "overlapping squares: matrix = {m}");
        assert!(!m.is_contains());
        assert!(!m.is_within());
        // II should be Area (shared interior region)
        assert_eq!(
            m.get(De9im::II),
            Dimension::Area,
            "II = {}",
            m.get(De9im::II)
        );
        // IE should be Area (part of a's interior outside b)
        assert_eq!(
            m.get(De9im::IE),
            Dimension::Area,
            "IE = {}",
            m.get(De9im::IE)
        );
        // EI should be Area (part of b's interior outside a)
        assert_eq!(
            m.get(De9im::EI),
            Dimension::Area,
            "EI = {}",
            m.get(De9im::EI)
        );
        Ok(())
    }

    #[test]
    fn test_relate_contained_square() -> TestResult {
        // b is strictly inside a
        let a = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let b = make_rect(2.0, 2.0, 8.0, 8.0)?;
        let m = relate_polygons(&a, &b)?;
        assert!(m.is_contains(), "a contains b: matrix = {m}");
        // Transpose should give within
        let mt = m.transpose();
        assert!(mt.is_within(), "b within a: transposed matrix = {mt}");
        Ok(())
    }

    #[test]
    fn test_relate_touching_squares() -> TestResult {
        // Two squares sharing an edge at x=4
        let a = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let b = make_rect(4.0, 0.0, 8.0, 4.0)?;
        let m = relate_polygons(&a, &b)?;
        assert!(m.is_touches(), "touching squares: matrix = {m}");
        assert!(!m.is_disjoint());
        assert!(!m.is_overlaps(2, 2));
        Ok(())
    }

    #[test]
    fn test_relate_identical_polygons() -> TestResult {
        let a = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let b = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let m = relate_polygons(&a, &b)?;
        assert!(m.is_equals(), "identical polygons: matrix = {m}");
        assert!(!m.is_disjoint());
        Ok(())
    }

    // =======================================================================
    // relate_point_polygon tests
    // =======================================================================

    #[test]
    fn test_relate_point_inside_polygon() -> TestResult {
        let poly = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let pt = Point::new(5.0, 5.0);
        let m = relate_point_polygon(&pt, &poly)?;
        assert!(m.is_within(), "point inside polygon: matrix = {m}");
        Ok(())
    }

    #[test]
    fn test_relate_point_on_boundary() -> TestResult {
        let poly = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let pt = Point::new(0.0, 5.0);
        let m = relate_point_polygon(&pt, &poly)?;
        // Point on boundary should give touches: F0FFFF102 or similar
        assert!(m.is_touches(), "point on boundary: matrix = {m}");
        Ok(())
    }

    #[test]
    fn test_relate_point_outside_polygon() -> TestResult {
        let poly = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let pt = Point::new(20.0, 20.0);
        let m = relate_point_polygon(&pt, &poly)?;
        assert!(m.is_disjoint(), "point outside polygon: matrix = {m}");
        Ok(())
    }

    // =======================================================================
    // relate_line_polygon tests
    // =======================================================================

    #[test]
    fn test_relate_line_crossing_polygon() -> TestResult {
        // Line passes through polygon interior
        let poly = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let line_coords = vec![Coordinate::new_2d(-5.0, 5.0), Coordinate::new_2d(15.0, 5.0)];
        let line = LineString::new(line_coords).map_err(AlgorithmError::Core)?;
        let m = relate_line_polygon(&line, &poly)?;
        // Line crosses polygon: dim_a=1 (line), dim_b=2 (polygon)
        assert!(m.is_crosses(1, 2), "line crossing polygon: matrix = {m}");
        Ok(())
    }

    #[test]
    fn test_relate_line_inside_polygon() -> TestResult {
        let poly = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let line_coords = vec![Coordinate::new_2d(2.0, 5.0), Coordinate::new_2d(8.0, 5.0)];
        let line = LineString::new(line_coords).map_err(AlgorithmError::Core)?;
        let m = relate_line_polygon(&line, &poly)?;
        assert!(m.is_within(), "line inside polygon: matrix = {m}");
        Ok(())
    }

    #[test]
    fn test_relate_line_outside_polygon() -> TestResult {
        let poly = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let line_coords = vec![
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(30.0, 30.0),
        ];
        let line = LineString::new(line_coords).map_err(AlgorithmError::Core)?;
        let m = relate_line_polygon(&line, &poly)?;
        assert!(m.is_disjoint(), "line outside polygon: matrix = {m}");
        Ok(())
    }

    // =======================================================================
    // Predicate trait tests
    // =======================================================================

    #[test]
    fn test_equals_predicate_polygon() -> TestResult {
        let a = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let b = make_rect(0.0, 0.0, 4.0, 4.0)?;
        assert!(a.equals_topo(&b)?);
        let c = make_rect(1.0, 1.0, 5.0, 5.0)?;
        assert!(!a.equals_topo(&c)?);
        Ok(())
    }

    #[test]
    fn test_covers_predicate_polygon() -> TestResult {
        let a = make_rect(0.0, 0.0, 10.0, 10.0)?;
        let b = make_rect(2.0, 2.0, 8.0, 8.0)?;
        assert!(a.covers(&b)?);
        assert!(!b.covers(&a)?);
        Ok(())
    }

    #[test]
    fn test_covered_by_predicate_polygon() -> TestResult {
        let a = make_rect(2.0, 2.0, 8.0, 8.0)?;
        let b = make_rect(0.0, 0.0, 10.0, 10.0)?;
        assert!(a.covered_by(&b)?);
        assert!(!b.covered_by(&a)?);
        Ok(())
    }

    #[test]
    fn test_equals_predicate_point() -> TestResult {
        let a = Point::new(1.0, 2.0);
        let b = Point::new(1.0, 2.0);
        let c = Point::new(3.0, 4.0);
        assert!(a.equals_topo(&b)?);
        assert!(!a.equals_topo(&c)?);
        Ok(())
    }

    // =======================================================================
    // Pattern matching round-trip
    // =======================================================================

    #[test]
    fn test_pattern_matching_roundtrip() -> TestResult {
        let patterns = [
            "T*F**FFF*", // equals
            "FF*FF****", // disjoint
            "FT*******", // touches variant
            "T*T******", // crosses (line/polygon)
            "T*F**F***", // within
            "T*****FF*", // contains
            "T*T***T**", // overlaps (area/area)
        ];
        for pat in &patterns {
            let m = De9im::from_str(pat)?;
            assert!(
                m.matches(pat),
                "pattern {pat} should match itself, matrix = {m}"
            );
        }
        Ok(())
    }

    // =======================================================================
    // Crosses fix: Polygon/Polygon
    // =======================================================================

    #[test]
    fn test_polygon_polygon_crosses_always_false_via_de9im() -> TestResult {
        // Two overlapping polygons -- crosses is undefined for same-dim(2) geometries
        let a = make_rect(0.0, 0.0, 4.0, 4.0)?;
        let b = make_rect(2.0, 2.0, 6.0, 6.0)?;
        let m = relate_polygons(&a, &b)?;
        assert!(
            !m.is_crosses(2, 2),
            "polygon/polygon crosses must always be false per OGC"
        );
        Ok(())
    }

    // =======================================================================
    // Edge cases
    // =======================================================================

    #[test]
    fn test_relate_invalid_polygon() {
        // Polygon with too few coordinates
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let ext = LineString::new(coords);
        // LineString::new may succeed with 3 coords, but Polygon::new should fail
        if let Ok(e) = ext {
            if let Ok(bad_poly) = Polygon::new(e, vec![]) {
                let good = make_rect(0.0, 0.0, 10.0, 10.0);
                if let Ok(g) = good {
                    let result = relate_polygons(&bad_poly, &g);
                    assert!(result.is_err());
                }
            }
        }
    }

    #[test]
    fn test_relate_line_polygon_invalid_line() {
        let poly_result = make_rect(0.0, 0.0, 10.0, 10.0);
        if let Ok(poly) = poly_result {
            // Cannot create a LineString with 1 coord, so we test with the error path
            let coords = vec![Coordinate::new_2d(5.0, 5.0)];
            let line_result = LineString::new(coords);
            // LineString::new should fail with < 2 coords
            assert!(line_result.is_err());
        }
    }

    #[test]
    fn test_dimension_display() {
        assert_eq!(format!("{}", Dimension::Empty), "F");
        assert_eq!(format!("{}", Dimension::Point), "0");
        assert_eq!(format!("{}", Dimension::Line), "1");
        assert_eq!(format!("{}", Dimension::Area), "2");
    }
}
