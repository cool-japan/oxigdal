//! Adaptive greedy VIP TIN (Triangulated Irregular Network) generation from DEM data.
//!
//! Implements the Very Important Points (VIP) algorithm for adaptive DEM to TIN
//! conversion. Starting with the four corner points, the algorithm iteratively
//! inserts the grid cell with the highest vertical error relative to the current
//! TIN surface until a stopping criterion is reached.
//!
//! # Algorithm
//!
//! 1. Seed the TIN with the 4 corners of the DEM.
//! 2. Triangulate with Bowyer–Watson Delaunay triangulation.
//! 3. Find the DEM cell with maximum vertical error (|interpolated − actual|).
//! 4. Insert that cell and re-triangulate.
//! 5. Repeat until `max_error` or `max_points` stopping criterion is met.
//!
//! # References
//!
//! Chen, Z.-T., & Tobler, W. R. (1986). *Quadtree representation of digital terrain*.
//! Lee, J. (1991). *Comparison of existing methods for building triangular irregular
//! network models of terrains from grid digital elevation models*.

use num_traits::Float;
use oxigdal_index::{Coord, triangulate};
use scirs2_core::prelude::Array2;

use crate::error::{Result, TerrainError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A terrain mesh generated from a DEM using adaptive greedy VIP point selection.
///
/// Each vertex stores `[x, y, z]` in map coordinates and triangles reference
/// vertices by 0-based index.
#[derive(Debug, Clone)]
pub struct TerrainTin {
    /// 3D vertices: each element is `[x, y, z]` in map coordinates.
    pub vertices: Vec<[f64; 3]>,
    /// Triangles as triples of 0-based indices into [`vertices`][TerrainTin::vertices].
    pub triangles: Vec<[usize; 3]>,
}

impl TerrainTin {
    /// Interpolate elevation at `(x, y)` using barycentric coordinates.
    ///
    /// Iterates all triangles in the TIN, finds the first one whose 2-D projection
    /// contains `(x, y)` (using a signed-area / barycentric test with a small
    /// epsilon tolerance for numerical stability), and returns the linearly
    /// interpolated elevation.
    ///
    /// Returns `None` if `(x, y)` lies outside the TIN convex hull or if the
    /// TIN has no triangles.
    pub fn interpolate_elevation(&self, x: f64, y: f64) -> Option<f64> {
        for tri in &self.triangles {
            let [ia, ib, ic] = *tri;
            let va = Vert2d::from_arr(&self.vertices[ia]);
            let vb = Vert2d::from_arr(&self.vertices[ib]);
            let vc = Vert2d::from_arr(&self.vertices[ic]);
            if let Some(z) = barycentric_interp(va, vb, vc, x, y) {
                return Some(z);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Public function
// ---------------------------------------------------------------------------

/// Generate an adaptive TIN from a DEM using greedy VIP refinement.
///
/// Starts with the 4 DEM corner points, triangulates them, then iteratively
/// inserts the DEM cell with the largest vertical error versus the current TIN
/// surface until the stopping criterion is met.
///
/// # Arguments
///
/// * `dem` - 2-D elevation raster (`rows × cols`).
/// * `origin_x` - Map X coordinate of the top-left corner of `dem[[0, 0]]`.
/// * `origin_y` - Map Y coordinate of the top-left corner of `dem[[0, 0]]`.
/// * `cell_size` - Cell size in map units (must be positive).
/// * `max_error` - Stop when the maximum vertical error ≤ `max_error`.
/// * `max_points` - Stop when the TIN has ≥ `max_points` vertices.
/// * `nodata` - Optional nodata sentinel; nodata cells are never inserted.
///
/// # Errors
///
/// Returns [`TerrainError::InvalidDimensions`] if `dem` has fewer than 2 rows or
/// columns, [`TerrainError::InvalidCellSize`] if `cell_size ≤ 0`, and
/// [`TerrainError::InvalidThreshold`] if `max_error < 0`.
pub fn tin_from_dem<T>(
    dem: &Array2<T>,
    origin_x: f64,
    origin_y: f64,
    cell_size: f64,
    max_error: f64,
    max_points: usize,
    nodata: Option<T>,
) -> Result<TerrainTin>
where
    T: Float + Into<f64> + Copy,
{
    // --- Validation ---
    let rows = dem.nrows();
    let cols = dem.ncols();
    if rows < 2 || cols < 2 {
        return Err(TerrainError::InvalidDimensions {
            width: cols,
            height: rows,
        });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    if max_error < 0.0 {
        return Err(TerrainError::InvalidThreshold {
            threshold: max_error,
            message: "max_error must be >= 0".to_string(),
        });
    }

    // --- Helpers ---
    let dem_xyz = |row: usize, col: usize| -> [f64; 3] {
        let x = origin_x + col as f64 * cell_size;
        let y = origin_y - row as f64 * cell_size; // north-up
        let z: f64 = dem[[row, col]].into();
        [x, y, z]
    };

    let is_nodata = |row: usize, col: usize| -> bool {
        if let Some(nd) = nodata {
            let v = dem[[row, col]];
            // NaN-safe comparison
            if nd.is_nan() { v.is_nan() } else { v == nd }
        } else {
            false
        }
    };

    // --- Seed vertices: 4 corners ---
    let corner_indices: [(usize, usize); 4] =
        [(0, 0), (0, cols - 1), (rows - 1, 0), (rows - 1, cols - 1)];

    let mut vertices: Vec<[f64; 3]> = corner_indices.iter().map(|&(r, c)| dem_xyz(r, c)).collect();

    // Track which cells are already inserted (by flat index = row*cols + col)
    let mut inserted: std::collections::HashSet<usize> =
        corner_indices.iter().map(|&(r, c)| r * cols + c).collect();

    // --- Initial triangulation ---
    let mut tin = build_tin_from_vertices(&vertices)?;

    // --- Greedy refinement ---
    loop {
        if vertices.len() >= max_points {
            break;
        }

        // Find the cell with maximum vertical error
        let mut best_error = max_error; // threshold — only beat this to continue
        let mut best_flat: Option<usize> = None;

        for row in 0..rows {
            for col in 0..cols {
                let flat = row * cols + col;
                if inserted.contains(&flat) {
                    continue;
                }
                if is_nodata(row, col) {
                    continue;
                }
                let [px, py, pz] = dem_xyz(row, col);
                let interpolated = tin.interpolate_elevation(px, py).unwrap_or(f64::NAN);
                if interpolated.is_nan() {
                    continue;
                }
                let err = (pz - interpolated).abs();
                if err > best_error {
                    best_error = err;
                    best_flat = Some(flat);
                }
            }
        }

        // Stop if no cell beats the threshold
        let flat = match best_flat {
            Some(f) => f,
            None => break,
        };

        let row = flat / cols;
        let col = flat % cols;

        vertices.push(dem_xyz(row, col));
        inserted.insert(flat);

        // Re-triangulate with updated vertex set
        tin = build_tin_from_vertices(&vertices)?;
    }

    Ok(tin)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Rebuild a [`TerrainTin`] from a set of 3-D vertices.
///
/// Extracts `(x, y)` as 2-D Delaunay input, then maps triangle indices back to
/// the original vertex slice.
fn build_tin_from_vertices(vertices: &[[f64; 3]]) -> Result<TerrainTin> {
    let coords: Vec<Coord> = vertices.iter().map(|&[x, y, _]| Coord::new(x, y)).collect();

    let tri = triangulate(&coords).map_err(|e| TerrainError::ComputationError {
        message: format!("Delaunay triangulation failed: {e}"),
    })?;

    // tri.points may be deduplicated / reordered — build a mapping from
    // Coord back to original vertex index so triangle indices refer to `vertices`.
    // Bowyer–Watson may remove duplicate points; for our use-case all inserted
    // vertices should be distinct, but we handle it gracefully.
    let triangles: Vec<[usize; 3]> = tri
        .triangles
        .iter()
        .map(|&[ia, ib, ic]| {
            // tri.points[i] has the same (x, y) as some vertex in `vertices`.
            // Map each Delaunay point index back to the original vertex index.
            let a = find_vertex_index(vertices, &tri.points[ia]);
            let b = find_vertex_index(vertices, &tri.points[ib]);
            let c = find_vertex_index(vertices, &tri.points[ic]);
            [a, b, c]
        })
        .collect();

    Ok(TerrainTin {
        vertices: vertices.to_vec(),
        triangles,
    })
}

/// Find the index in `vertices` whose `(x, y)` matches `coord`.
///
/// Falls back to 0 if no match is found (should not occur with well-formed input).
fn find_vertex_index(vertices: &[[f64; 3]], coord: &Coord) -> usize {
    vertices
        .iter()
        .position(|&[x, y, _]| (x - coord.x).abs() < 1e-10 && (y - coord.y).abs() < 1e-10)
        .unwrap_or(0)
}

/// A 2-D point with an associated elevation, used for barycentric interpolation.
#[derive(Debug, Clone, Copy)]
struct Vert2d {
    x: f64,
    y: f64,
    z: f64,
}

impl Vert2d {
    #[inline]
    fn from_arr(v: &[f64; 3]) -> Self {
        Self {
            x: v[0],
            y: v[1],
            z: v[2],
        }
    }
}

/// Compute barycentric elevation interpolation for a query point `(px, py)`.
///
/// Returns `Some(z)` when `(px, py)` lies inside (or on the boundary of) the
/// triangle `(a, b, c)` projected to 2-D, `None` otherwise.
fn barycentric_interp(a: Vert2d, b: Vert2d, c: Vert2d, px: f64, py: f64) -> Option<f64> {
    let denom = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if denom.abs() < 1e-12 {
        return None;
    }
    let w_a = ((b.y - c.y) * (px - c.x) + (c.x - b.x) * (py - c.y)) / denom;
    let w_b = ((c.y - a.y) * (px - c.x) + (a.x - c.x) * (py - c.y)) / denom;
    let w_c = 1.0 - w_a - w_b;
    const EPS: f64 = 1e-9;
    if w_a >= -EPS && w_b >= -EPS && w_c >= -EPS {
        Some(w_a * a.z + w_b * b.z + w_c * c.z)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::prelude::Array2;

    // Build a flat DEM filled with a constant value.
    fn flat_dem(rows: usize, cols: usize, elev: f64) -> Array2<f64> {
        Array2::from_elem((rows, cols), elev)
    }

    // Build a pyramid DEM: center is high, edges are low.
    fn pyramid_dem(size: usize) -> Array2<f64> {
        let mut dem = Array2::<f64>::zeros((size, size));
        let center = (size / 2) as f64;
        for r in 0..size {
            for c in 0..size {
                let dist = ((r as f64 - center).abs()).max((c as f64 - center).abs());
                dem[[r, c]] = (center - dist).max(0.0);
            }
        }
        dem
    }

    /// A flat DEM should require only the 4 corner vertices (error = 0 everywhere
    /// after the first triangulation).
    #[test]
    fn test_tin_planar_dem() {
        let dem = flat_dem(4, 4, 100.0_f64);
        let tin = tin_from_dem(&dem, 0.0, 30.0, 10.0, 0.01, 100, None)
            .expect("planar TIN should succeed");
        // All cells should have error ≤ tolerance, so only 4 corners inserted.
        assert_eq!(tin.vertices.len(), 4, "planar DEM needs only 4 vertices");
        assert!(
            !tin.triangles.is_empty(),
            "should have at least 2 triangles"
        );
    }

    /// A pyramid DEM should trigger insertion of the centre peak.
    #[test]
    fn test_tin_pyramid_dem() {
        let dem = pyramid_dem(5);
        let tin =
            tin_from_dem(&dem, 0.0, 40.0, 10.0, 0.1, 50, None).expect("pyramid TIN should succeed");
        // Center (row=2, col=2) has the highest error and must be inserted.
        assert!(
            tin.vertices.len() > 4,
            "pyramid DEM should insert more than 4 vertices"
        );
        // Check the centre vertex is present (z = 2.0 for 5×5 pyramid)
        let centre_z = 2.0_f64;
        let has_centre = tin
            .vertices
            .iter()
            .any(|&[_, _, z]| (z - centre_z).abs() < 1e-9);
        assert!(has_centre, "centre peak vertex should be inserted");
    }

    /// Max error should strictly decrease as more points are added (curved surface).
    #[test]
    fn test_tin_error_decreases() {
        // Build a parabolic DEM: z = r^2 + c^2
        let size = 6_usize;
        let dem: Array2<f64> = Array2::from_shape_fn((size, size), |(r, c)| {
            (r as f64).powi(2) + (c as f64).powi(2)
        });

        // We run with decreasing max_points budgets and check that error decreases.
        let mut prev_max_err = f64::INFINITY;
        for pts in [5_usize, 8, 12, 20] {
            let tin = tin_from_dem(&dem, 0.0, 50.0, 10.0, 0.0, pts, None)
                .expect("parabolic TIN should succeed");
            // Compute actual max error over all non-corner cells.
            let cols = dem.ncols();
            let rows = dem.nrows();
            let mut max_err = 0.0_f64;
            for r in 0..rows {
                for c in 0..cols {
                    let x = c as f64 * 10.0;
                    let y = 50.0 - r as f64 * 10.0;
                    let actual_z = dem[[r, c]];
                    if let Some(iz) = tin.interpolate_elevation(x, y) {
                        let e = (actual_z - iz).abs();
                        if e > max_err {
                            max_err = e;
                        }
                    }
                }
            }
            assert!(
                max_err <= prev_max_err,
                "error should not increase with more points: pts={pts}, err={max_err}, prev={prev_max_err}"
            );
            prev_max_err = max_err;
        }
    }

    /// Setting `max_points = 5` should produce a TIN with at most 5 vertices.
    #[test]
    fn test_tin_max_points_stops() {
        let dem: Array2<f64> = Array2::from_shape_fn((8, 8), |(r, c)| {
            (r as f64 * 1.3 + c as f64 * 0.7).sin() * 50.0
        });
        let tin = tin_from_dem(&dem, 0.0, 70.0, 5.0, 0.0, 5, None)
            .expect("max_points TIN should succeed");
        assert!(
            tin.vertices.len() <= 5,
            "expected ≤5 vertices, got {}",
            tin.vertices.len()
        );
    }

    /// Interpolating at an interior point of a planar TIN should match the
    /// known constant elevation.
    #[test]
    fn test_tin_interpolate_corner() {
        let elev = 42.0_f64;
        let dem = flat_dem(4, 4, elev);
        // Grid: origin (0, 30), cell_size=10 → cols 0..3, rows 0..3
        // Last row/col → y=0, x=30
        let tin =
            tin_from_dem(&dem, 0.0, 30.0, 10.0, 0.01, 100, None).expect("flat TIN should succeed");

        // Interior point at (15.0, 15.0) should interpolate to elev
        let interp = tin
            .interpolate_elevation(15.0, 15.0)
            .expect("interior point should interpolate");
        let diff = (interp - elev).abs();
        assert!(diff < 1e-9, "expected {elev}, got {interp} (diff={diff})");
    }

    /// `cell_size = 0` must return an error.
    #[test]
    fn test_tin_invalid_cellsize() {
        let dem = flat_dem(4, 4, 0.0_f64);
        let result = tin_from_dem(&dem, 0.0, 30.0, 0.0, 0.01, 100, None::<f64>);
        assert!(result.is_err(), "cell_size=0 should return Err");
        assert!(matches!(result, Err(TerrainError::InvalidCellSize { .. })));
    }
}
