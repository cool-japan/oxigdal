//! Raster polygonization (raster-to-polygon conversion)
//!
//! Converts raster data into vector polygon geometries via:
//!
//! 1. **Connected component labeling (CCL):** Two-pass union-find algorithm with
//!    path compression and union by rank. Configurable 4- or 8-connectivity.
//!    NoData pixels receive label 0 (background).
//!
//! 2. **Boundary extraction:** Pixel-edge contour extraction produces closed
//!    rectilinear rings along pixel boundaries (like GDAL's `GDALPolygonize`).
//!    Alternatively, Moore-Neighbor tracing produces pixel-center boundaries.
//!
//! 3. **Ring classification:** Signed area via shoelace formula determines ring
//!    orientation. CCW rings are exteriors; CW rings are holes. Holes are
//!    assigned to the smallest containing exterior via point-in-polygon tests.
//!
//! 4. **GeoTransform:** Optional pixel-to-world coordinate transformation.
//!
//! # Algorithm Overview
//!
//! The connected component labeling (CCL) uses a classical two-pass algorithm:
//!
//! - **Pass 1 (forward scan):** Assign provisional labels. For each pixel, check
//!   already-labeled neighbors (above / left for 4-conn; above-left, above,
//!   above-right, left for 8-conn). If no labeled neighbor exists, assign a new
//!   label. If one or more labeled neighbors exist, take the minimum label and
//!   record equivalences in the union-find.
//!
//! - **Pass 2 (relabel):** Replace each provisional label with its canonical
//!   representative from the union-find.
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::raster::polygonize::{PolygonizeOptions, polygonize};
//!
//! // 4x4 grid with two distinct regions
//! let grid: Vec<f64> = vec![
//!     1.0, 1.0, 2.0, 2.0,
//!     1.0, 1.0, 2.0, 2.0,
//!     3.0, 3.0, 2.0, 2.0,
//!     3.0, 3.0, 3.0, 3.0,
//! ];
//!
//! let opts = PolygonizeOptions::default();
//! let result = polygonize(&grid, 4, 4, &opts);
//! assert!(result.is_ok());
//! let result = result.as_ref();
//! assert!(result.is_ok_and(|r| r.polygons.len() == 3));
//! ```

mod boundary;
mod union_find;

use std::collections::HashMap;

use oxigdal_core::types::GeoTransform;
use oxigdal_core::vector::{Coordinate, LineString, MultiPolygon, Polygon};

use crate::error::{AlgorithmError, Result};

pub use boundary::Connectivity;
use boundary::{
    ClassifiedPolygon, extract_pixel_edge_boundaries, pixel_coords_to_coordinates,
    trace_boundaries, transform_coords,
};
use union_find::UnionFind;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Options controlling raster polygonization behavior.
#[derive(Debug, Clone)]
pub struct PolygonizeOptions {
    /// Pixel connectivity for connected component labeling.
    /// Default: `Connectivity::Eight`.
    pub connectivity: Connectivity,

    /// Value treated as nodata (background). Pixels matching this value
    /// (within `nodata_tolerance`) receive label 0 and are excluded from
    /// the output polygons. Default: `None` (all values are data).
    pub nodata: Option<f64>,

    /// Tolerance for comparing pixel values against nodata.
    /// Default: `1e-10`.
    pub nodata_tolerance: f64,

    /// Optional GeoTransform for converting pixel coordinates to world
    /// coordinates. If `None`, output coordinates are in pixel space.
    pub transform: Option<GeoTransform>,

    /// If `> 0.0`, apply Douglas-Peucker simplification to output polygons
    /// with this tolerance. Default: `0.0` (disabled).
    pub simplify_tolerance: f64,

    /// Minimum polygon area (in output coordinate units). Polygons smaller
    /// than this are dropped. Default: `0.0` (keep all).
    pub min_area: f64,

    /// Boundary extraction method. Default: `BoundaryMethod::PixelEdge`.
    pub boundary_method: BoundaryMethod,
}

/// Method for extracting polygon boundaries from labeled regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryMethod {
    /// Extract boundaries along pixel edges (rectilinear). Produces exact
    /// boundaries aligned to pixel edges, matching GDAL's approach.
    PixelEdge,
    /// Moore-Neighbor contour tracing through pixel centers. Produces
    /// smoother boundaries but may not be pixel-exact.
    MooreNeighbor,
}

impl Default for BoundaryMethod {
    fn default() -> Self {
        Self::PixelEdge
    }
}

impl Default for PolygonizeOptions {
    fn default() -> Self {
        Self {
            connectivity: Connectivity::default(),
            nodata: None,
            nodata_tolerance: 1e-10,
            transform: None,
            simplify_tolerance: 0.0,
            min_area: 0.0,
            boundary_method: BoundaryMethod::default(),
        }
    }
}

/// A single polygonized feature: a value and its polygon geometry.
#[derive(Debug, Clone)]
pub struct PolygonFeature {
    /// The raster pixel value for this polygon.
    pub value: f64,
    /// The polygon geometry (exterior ring + interior holes).
    pub polygon: Polygon,
}

/// Result of raster polygonization.
#[derive(Debug, Clone)]
pub struct PolygonizeResult {
    /// Extracted polygon features, one per connected component.
    pub polygons: Vec<PolygonFeature>,
    /// Number of connected components found (excluding nodata).
    pub num_components: usize,
    /// Grid width in pixels.
    pub width: usize,
    /// Grid height in pixels.
    pub height: usize,
}

impl PolygonizeResult {
    /// Get all polygons for a specific pixel value.
    pub fn polygons_for_value(&self, value: f64, tolerance: f64) -> Vec<&PolygonFeature> {
        self.polygons
            .iter()
            .filter(|f| (f.value - value).abs() <= tolerance)
            .collect()
    }

    /// Collect all polygons into a `MultiPolygon`.
    pub fn to_multipolygon(&self) -> MultiPolygon {
        let polys: Vec<Polygon> = self.polygons.iter().map(|f| f.polygon.clone()).collect();
        MultiPolygon::new(polys)
    }

    /// Get unique pixel values present in the result.
    pub fn unique_values(&self) -> Vec<f64> {
        let mut values: Vec<f64> = Vec::new();
        for feat in &self.polygons {
            if !values
                .iter()
                .any(|v| (*v - feat.value).abs() < f64::EPSILON)
            {
                values.push(feat.value);
            }
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        values
    }
}

// ---------------------------------------------------------------------------
// Connected Component Labeling
// ---------------------------------------------------------------------------

/// Two-pass connected component labeling on a quantized grid.
///
/// Pixel values are quantized to integer keys for comparison (grouping pixels
/// with the same value). Returns a label grid where each connected region of
/// identical values has a unique label. Label 0 = nodata/background.
fn connected_component_label(
    grid: &[f64],
    width: usize,
    height: usize,
    connectivity: Connectivity,
    nodata: Option<f64>,
    nodata_tolerance: f64,
) -> Result<(Vec<u32>, u32, HashMap<u32, f64>)> {
    let n = width * height;
    if grid.len() != n {
        return Err(AlgorithmError::InvalidInput(
            "grid size does not match width*height".to_string(),
        ));
    }

    let mut labels = vec![0u32; n];
    let mut next_label = 1u32;
    let mut uf = UnionFind::with_capacity(256); // will grow as needed
    let mut label_values: HashMap<u32, f64> = HashMap::new();

    // Pass 1: forward scan, assign provisional labels
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let val = grid[idx];

            // Skip nodata pixels
            if is_nodata_value(val, nodata, nodata_tolerance) {
                continue;
            }

            // Collect labels of same-value neighbors that have already been labeled
            let neighbor_labels = get_neighbor_labels(
                grid,
                &labels,
                width,
                height,
                x,
                y,
                val,
                connectivity,
                nodata,
                nodata_tolerance,
            );

            if neighbor_labels.is_empty() {
                // New component
                uf.ensure_label(next_label);
                labels[idx] = next_label;
                label_values.insert(next_label, val);
                next_label = next_label.saturating_add(1);
            } else {
                // Take the minimum label
                let mut min_label = neighbor_labels[0];
                for &nl in &neighbor_labels[1..] {
                    if nl < min_label {
                        min_label = nl;
                    }
                }
                labels[idx] = min_label;

                // Union all neighbor labels together
                for &nl in &neighbor_labels {
                    if nl != min_label {
                        uf.ensure_label(nl.max(min_label));
                        uf.union(min_label, nl);
                    }
                }
            }
        }
    }

    // Pass 2: relabel using canonical representatives
    let mut canonical_map: HashMap<u32, u32> = HashMap::new();
    let mut remap_next = 1u32;

    for label in labels.iter_mut() {
        if *label == 0 {
            continue;
        }
        let root = uf.find(*label);
        let canonical = *canonical_map.entry(root).or_insert_with(|| {
            let c = remap_next;
            remap_next = remap_next.saturating_add(1);
            c
        });
        *label = canonical;
    }

    // Remap label_values to use canonical labels
    let mut new_label_values: HashMap<u32, f64> = HashMap::new();
    for (&orig_label, &val) in &label_values {
        let root = uf.find(orig_label);
        if let Some(&canonical) = canonical_map.get(&root) {
            new_label_values.entry(canonical).or_insert(val);
        }
    }

    let num_components = remap_next.saturating_sub(1);
    Ok((labels, num_components, new_label_values))
}

/// Check whether a value is nodata.
#[inline]
fn is_nodata_value(value: f64, nodata: Option<f64>, tolerance: f64) -> bool {
    if value.is_nan() {
        return true;
    }
    if let Some(nd) = nodata {
        (value - nd).abs() <= tolerance
    } else {
        false
    }
}

/// Check whether two pixel values are "same" for CCL purposes.
#[inline]
fn same_value(a: f64, b: f64) -> bool {
    // Use exact equality for integer-like values, tolerance for floating point
    (a - b).abs() < f64::EPSILON * 100.0 || (a == b)
}

/// Get already-labeled neighbors with the same pixel value.
fn get_neighbor_labels(
    grid: &[f64],
    labels: &[u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    value: f64,
    connectivity: Connectivity,
    nodata: Option<f64>,
    nodata_tolerance: f64,
) -> Vec<u32> {
    let mut result = Vec::with_capacity(4);

    // For forward-scan CCL, we only look at already-visited neighbors:
    // - 4-conn: above (N) and left (W)
    // - 8-conn: above-left (NW), above (N), above-right (NE), left (W)

    // Above (N): y-1, x
    if y > 0 {
        let nidx = (y - 1) * width + x;
        let nl = labels[nidx];
        let nv = grid[nidx];
        if nl != 0 && !is_nodata_value(nv, nodata, nodata_tolerance) && same_value(value, nv) {
            result.push(nl);
        }
    }

    // Left (W): y, x-1
    if x > 0 {
        let nidx = y * width + (x - 1);
        let nl = labels[nidx];
        let nv = grid[nidx];
        if nl != 0 && !is_nodata_value(nv, nodata, nodata_tolerance) && same_value(value, nv) {
            if !result.contains(&nl) {
                result.push(nl);
            }
        }
    }

    if connectivity == Connectivity::Eight {
        // Above-left (NW): y-1, x-1
        if y > 0 && x > 0 {
            let nidx = (y - 1) * width + (x - 1);
            let nl = labels[nidx];
            let nv = grid[nidx];
            if nl != 0
                && !is_nodata_value(nv, nodata, nodata_tolerance)
                && same_value(value, nv)
                && !result.contains(&nl)
            {
                result.push(nl);
            }
        }

        // Above-right (NE): y-1, x+1
        if y > 0 && x + 1 < width {
            let nidx = (y - 1) * width + (x + 1);
            let nl = labels[nidx];
            let nv = grid[nidx];
            if nl != 0
                && !is_nodata_value(nv, nodata, nodata_tolerance)
                && same_value(value, nv)
                && !result.contains(&nl)
            {
                result.push(nl);
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Simplification (Douglas-Peucker)
// ---------------------------------------------------------------------------

/// Simplify a coordinate ring using Douglas-Peucker.
fn simplify_ring(coords: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if coords.len() <= 4 || tolerance <= 0.0 {
        return coords.to_vec();
    }
    let tol_sq = tolerance * tolerance;
    let mut keep = vec![false; coords.len()];
    keep[0] = true;
    keep[coords.len() - 1] = true;
    dp_simplify(coords, 0, coords.len() - 1, tol_sq, &mut keep);

    let result: Vec<(f64, f64)> = coords
        .iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .map(|(_, c)| *c)
        .collect();

    // Ensure ring closure
    if result.len() >= 3 {
        result
    } else {
        coords.to_vec() // Don't simplify below minimum ring size
    }
}

/// Douglas-Peucker recursive helper.
fn dp_simplify(coords: &[(f64, f64)], start: usize, end: usize, tol_sq: f64, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let (sx, sy) = coords[start];
    let (ex, ey) = coords[end];
    let dx = ex - sx;
    let dy = ey - sy;
    let len_sq = dx.mul_add(dx, dy * dy);

    let mut max_dist_sq = 0.0_f64;
    let mut max_idx = start;

    for i in (start + 1)..end {
        let (px, py) = coords[i];
        let dist_sq = if len_sq < f64::EPSILON {
            let dpx = px - sx;
            let dpy = py - sy;
            dpx.mul_add(dpx, dpy * dpy)
        } else {
            let t = ((px - sx).mul_add(dx, (py - sy) * dy) / len_sq).clamp(0.0, 1.0);
            let proj_x = t.mul_add(dx, sx);
            let proj_y = t.mul_add(dy, sy);
            let dpx = px - proj_x;
            let dpy = py - proj_y;
            dpx.mul_add(dpx, dpy * dpy)
        };

        if dist_sq > max_dist_sq {
            max_dist_sq = dist_sq;
            max_idx = i;
        }
    }

    if max_dist_sq > tol_sq {
        keep[max_idx] = true;
        dp_simplify(coords, start, max_idx, tol_sq, keep);
        dp_simplify(coords, max_idx, end, tol_sq, keep);
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert a raster grid to vector polygon geometries.
///
/// This is the main entry point for raster polygonization, equivalent to
/// GDAL's `GDALPolygonize()`. Each connected region of identical pixel values
/// becomes a separate polygon feature.
///
/// # Arguments
///
/// * `grid` - Flat row-major array of pixel values.
/// * `width` - Grid width in pixels.
/// * `height` - Grid height in pixels.
/// * `options` - Polygonization options (connectivity, nodata, transform, etc.).
///
/// # Returns
///
/// A `PolygonizeResult` containing polygon features with their associated pixel
/// values, or an error if the input is invalid.
///
/// # Errors
///
/// Returns an error if:
/// - `grid.len() != width * height`
/// - `width` or `height` is zero
/// - Internal geometry construction fails
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::raster::polygonize::{PolygonizeOptions, polygonize};
///
/// let grid = vec![1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0];
/// let result = polygonize(&grid, 4, 3, &PolygonizeOptions::default());
/// assert!(result.is_ok());
/// ```
pub fn polygonize(
    grid: &[f64],
    width: usize,
    height: usize,
    options: &PolygonizeOptions,
) -> Result<PolygonizeResult> {
    // Validate inputs
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidDimensions {
            message: "grid dimensions must be positive",
            actual: width.min(height),
            expected: 1,
        });
    }
    if grid.len() != width * height {
        return Err(AlgorithmError::InvalidInput(format!(
            "grid size ({}) does not match width*height ({}*{}={})",
            grid.len(),
            width,
            height,
            width * height
        )));
    }

    // Phase 1: Connected component labeling
    let (labels, num_components, label_values) = connected_component_label(
        grid,
        width,
        height,
        options.connectivity,
        options.nodata,
        options.nodata_tolerance,
    )?;

    if num_components == 0 {
        return Ok(PolygonizeResult {
            polygons: Vec::new(),
            num_components: 0,
            width,
            height,
        });
    }

    // Phase 2: Boundary extraction
    let classified_polys = match options.boundary_method {
        BoundaryMethod::PixelEdge => extract_pixel_edge_boundaries(&labels, width, height)?,
        BoundaryMethod::MooreNeighbor => {
            trace_boundaries(&labels, width, height, options.connectivity)?
        }
    };

    // Phase 3: Convert to output Polygon features
    let mut features = Vec::with_capacity(classified_polys.len());

    for cpoly in classified_polys {
        // Look up the pixel value for this label
        let value = label_values.get(&cpoly.label).copied().unwrap_or(0.0);

        // Apply simplification if requested
        let exterior_coords = if options.simplify_tolerance > 0.0 {
            simplify_ring(&cpoly.exterior, options.simplify_tolerance)
        } else {
            cpoly.exterior.clone()
        };

        let hole_coords: Vec<Vec<(f64, f64)>> = if options.simplify_tolerance > 0.0 {
            cpoly
                .holes
                .iter()
                .map(|h| simplify_ring(h, options.simplify_tolerance))
                .collect()
        } else {
            cpoly.holes.clone()
        };

        // Compute area for min_area filtering
        let area = boundary::compute_signed_area(&exterior_coords).abs();
        if options.min_area > 0.0 && area < options.min_area {
            continue;
        }

        // Convert to world coordinates if transform is provided
        let ext_world_coords = match &options.transform {
            Some(gt) => transform_coords(&exterior_coords, gt),
            None => pixel_coords_to_coordinates(&exterior_coords),
        };

        let hole_world_coords: Vec<Vec<Coordinate>> = hole_coords
            .iter()
            .map(|h| match &options.transform {
                Some(gt) => transform_coords(h, gt),
                None => pixel_coords_to_coordinates(h),
            })
            .collect();

        // Build the Polygon geometry
        let polygon = build_polygon(ext_world_coords, hole_world_coords)?;

        if let Some(poly) = polygon {
            features.push(PolygonFeature {
                value,
                polygon: poly,
            });
        }
    }

    Ok(PolygonizeResult {
        polygons: features,
        num_components: num_components as usize,
        width,
        height,
    })
}

/// Polygonize with a `RasterBuffer` from `oxigdal-core`.
///
/// Convenience wrapper that reads pixel values from a `RasterBuffer` and
/// calls [`polygonize`].
///
/// # Errors
///
/// Returns an error if pixel access fails or polygonization fails.
pub fn polygonize_raster(
    raster: &oxigdal_core::buffer::RasterBuffer,
    options: &PolygonizeOptions,
) -> Result<PolygonizeResult> {
    let width = raster.width() as usize;
    let height = raster.height() as usize;

    let mut grid = vec![0.0_f64; width * height];
    for y in 0..height {
        for x in 0..width {
            grid[y * width + x] = raster
                .get_pixel(x as u64, y as u64)
                .map_err(AlgorithmError::Core)?;
        }
    }

    polygonize(&grid, width, height, options)
}

/// Build a `Polygon` from coordinate vectors, handling edge cases.
fn build_polygon(
    exterior: Vec<Coordinate>,
    holes: Vec<Vec<Coordinate>>,
) -> Result<Option<Polygon>> {
    if exterior.len() < 4 {
        return Ok(None);
    }

    // Ensure the exterior ring is closed
    let mut ext = exterior;
    let first = ext[0];
    let last = ext[ext.len() - 1];
    if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
        ext.push(first);
    }

    if ext.len() < 4 {
        return Ok(None);
    }

    let ext_ring = LineString::new(ext).map_err(|e| AlgorithmError::Core(e))?;

    // Build interior rings
    let mut interior_rings = Vec::new();
    for hole in holes {
        if hole.len() < 4 {
            continue;
        }
        let mut h = hole;
        let hfirst = h[0];
        let hlast = h[h.len() - 1];
        if (hfirst.x - hlast.x).abs() > f64::EPSILON || (hfirst.y - hlast.y).abs() > f64::EPSILON {
            h.push(hfirst);
        }
        if h.len() >= 4 {
            if let Ok(ring) = LineString::new(h) {
                interior_rings.push(ring);
            }
        }
    }

    match Polygon::new(ext_ring, interior_rings) {
        Ok(poly) => Ok(Some(poly)),
        Err(_) => Ok(None), // Skip invalid polygons rather than failing
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    /// Helper: create a RasterBuffer from a flat f64 array.
    fn make_raster(width: usize, height: usize, values: &[f64]) -> RasterBuffer {
        let mut buf = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float64);
        for row in 0..height {
            for col in 0..width {
                let _ = buf.set_pixel(col as u64, row as u64, values[row * width + col]);
            }
        }
        buf
    }

    // --- Basic CCL tests ---

    #[test]
    fn test_ccl_single_value() {
        let grid = vec![1.0; 9];
        let (labels, num, values) =
            connected_component_label(&grid, 3, 3, Connectivity::Eight, None, 1e-10)
                .expect("CCL should succeed");
        assert_eq!(num, 1);
        assert!(values.values().any(|&v| (v - 1.0).abs() < f64::EPSILON));
        // All labels should be the same non-zero value
        let first_label = labels[0];
        assert!(first_label > 0);
        for &l in &labels {
            assert_eq!(l, first_label);
        }
    }

    #[test]
    fn test_ccl_two_regions() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 2.0,
            1.0, 1.0, 2.0,
            2.0, 2.0, 2.0,
        ];
        let (labels, num, _) =
            connected_component_label(&grid, 3, 3, Connectivity::Four, None, 1e-10)
                .expect("CCL should succeed");
        assert_eq!(num, 2);
        // Check that the 1-region and 2-region have different labels
        assert_ne!(labels[0], labels[2]);
    }

    #[test]
    fn test_ccl_nodata() {
        let grid = vec![1.0, f64::NAN, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let (labels, _, _) =
            connected_component_label(&grid, 3, 3, Connectivity::Four, None, 1e-10)
                .expect("CCL should succeed");
        // NaN pixel should have label 0
        assert_eq!(labels[1], 0);
    }

    #[test]
    fn test_ccl_custom_nodata() {
        let grid = vec![1.0, -9999.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let (labels, _, _) =
            connected_component_label(&grid, 3, 3, Connectivity::Four, Some(-9999.0), 1e-10)
                .expect("CCL should succeed");
        assert_eq!(labels[1], 0);
    }

    #[test]
    fn test_ccl_four_vs_eight_connectivity() {
        // Diagonal region: with 8-conn, diagonals connect; with 4-conn they don't
        #[rustfmt::skip]
        let grid = vec![
            1.0, 0.0,
            0.0, 1.0,
        ];

        let (_, num_4, _) =
            connected_component_label(&grid, 2, 2, Connectivity::Four, Some(0.0), 1e-10)
                .expect("CCL should succeed");
        let (_, num_8, _) =
            connected_component_label(&grid, 2, 2, Connectivity::Eight, Some(0.0), 1e-10)
                .expect("CCL should succeed");

        // 4-conn: two separate components; 8-conn: one component
        assert_eq!(num_4, 2);
        assert_eq!(num_8, 1);
    }

    #[test]
    fn test_ccl_empty_grid() {
        let grid = vec![f64::NAN; 4];
        let (labels, num, _) =
            connected_component_label(&grid, 2, 2, Connectivity::Eight, None, 1e-10)
                .expect("CCL should succeed");
        assert_eq!(num, 0);
        for &l in &labels {
            assert_eq!(l, 0);
        }
    }

    #[test]
    fn test_ccl_size_mismatch() {
        let grid = vec![1.0; 5];
        let result = connected_component_label(&grid, 2, 2, Connectivity::Eight, None, 1e-10);
        assert!(result.is_err());
    }

    // --- Polygonize API tests ---

    #[test]
    fn test_polygonize_basic() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 2.0, 2.0,
            1.0, 1.0, 2.0, 2.0,
            3.0, 3.0, 2.0, 2.0,
            3.0, 3.0, 3.0, 3.0,
        ];
        let opts = PolygonizeOptions::default();
        let result = polygonize(&grid, 4, 4, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert_eq!(result.num_components, 3);
        assert_eq!(result.polygons.len(), 3);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
    }

    #[test]
    fn test_polygonize_single_value() {
        let grid = vec![5.0; 16];
        let opts = PolygonizeOptions::default();
        let result = polygonize(&grid, 4, 4, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert_eq!(result.num_components, 1);
        assert_eq!(result.polygons.len(), 1);
        assert!((result.polygons[0].value - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_polygonize_with_nodata() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 0.0,
            1.0, 0.0, 2.0,
            0.0, 2.0, 2.0,
        ];
        let mut opts = PolygonizeOptions::default();
        opts.nodata = Some(0.0);
        let result = polygonize(&grid, 3, 3, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // With nodata=0, should have region-1 and region-2
        assert!(result.num_components >= 2);
        // No polygon should have value 0.0
        for feat in &result.polygons {
            assert!(
                (feat.value).abs() > f64::EPSILON,
                "nodata polygons should be excluded"
            );
        }
    }

    #[test]
    fn test_polygonize_with_transform() {
        let grid = vec![1.0; 4];
        let mut opts = PolygonizeOptions::default();
        opts.transform = Some(GeoTransform::north_up(100.0, 200.0, 10.0, -10.0));
        let result = polygonize(&grid, 2, 2, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert_eq!(result.polygons.len(), 1);

        // Verify coordinates are in world space
        let poly = &result.polygons[0].polygon;
        for coord in &poly.exterior.coords {
            // World X should be >= 100 (origin)
            assert!(
                coord.x >= 99.0,
                "world x should be near origin, got {}",
                coord.x
            );
        }
    }

    #[test]
    fn test_polygonize_min_area_filter() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 1.0, 1.0,
            1.0, 1.0, 1.0, 1.0,
            1.0, 1.0, 1.0, 2.0,
            1.0, 1.0, 1.0, 1.0,
        ];
        let mut opts = PolygonizeOptions::default();
        opts.min_area = 2.0; // Filter out the single-pixel "2" region
        let result = polygonize(&grid, 4, 4, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // The small "2" region should be filtered out
        for feat in &result.polygons {
            assert!(
                (feat.value - 2.0).abs() > f64::EPSILON,
                "small region should be filtered"
            );
        }
    }

    #[test]
    fn test_polygonize_simplify() {
        let grid = vec![1.0; 100]; // 10x10
        let mut opts = PolygonizeOptions::default();
        opts.simplify_tolerance = 2.0;
        let result = polygonize(&grid, 10, 10, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert!(!result.polygons.is_empty());
    }

    #[test]
    fn test_polygonize_zero_dimensions() {
        let result = polygonize(&[], 0, 0, &PolygonizeOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_polygonize_size_mismatch() {
        let result = polygonize(&[1.0; 5], 2, 2, &PolygonizeOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_polygonize_all_nodata() {
        let grid = vec![f64::NAN; 9];
        let result = polygonize(&grid, 3, 3, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert_eq!(result.num_components, 0);
        assert!(result.polygons.is_empty());
    }

    #[test]
    fn test_polygonize_moore_neighbor_method() {
        let grid = vec![1.0; 9];
        let mut opts = PolygonizeOptions::default();
        opts.boundary_method = BoundaryMethod::MooreNeighbor;
        let result = polygonize(&grid, 3, 3, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert!(!result.polygons.is_empty());
    }

    #[test]
    fn test_polygonize_four_connectivity() {
        let grid = vec![1.0; 9];
        let mut opts = PolygonizeOptions::default();
        opts.connectivity = Connectivity::Four;
        let result = polygonize(&grid, 3, 3, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert_eq!(result.num_components, 1);
    }

    // --- PolygonizeResult methods ---

    #[test]
    fn test_polygonize_result_for_value() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 2.0,
            1.0, 1.0, 2.0,
            2.0, 2.0, 2.0,
        ];
        let result = polygonize(&grid, 3, 3, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");

        let ones = result.polygons_for_value(1.0, f64::EPSILON);
        assert_eq!(ones.len(), 1);
        let twos = result.polygons_for_value(2.0, f64::EPSILON);
        assert_eq!(twos.len(), 1);
        let threes = result.polygons_for_value(3.0, f64::EPSILON);
        assert!(threes.is_empty());
    }

    #[test]
    fn test_polygonize_result_to_multipolygon() {
        let grid = vec![1.0; 4];
        let result = polygonize(&grid, 2, 2, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        let mp = result.to_multipolygon();
        assert_eq!(mp.polygons.len(), 1);
    }

    #[test]
    fn test_polygonize_result_unique_values() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 2.0,
            3.0, 4.0,
        ];
        let result = polygonize(&grid, 2, 2, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        let values = result.unique_values();
        assert_eq!(values.len(), 4);
    }

    // --- RasterBuffer integration ---

    #[test]
    fn test_polygonize_raster_buffer() {
        let raster = make_raster(3, 3, &[1.0; 9]);
        let result = polygonize_raster(&raster, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize_raster should succeed");
        assert_eq!(result.num_components, 1);
        assert_eq!(result.polygons.len(), 1);
    }

    #[test]
    fn test_polygonize_raster_with_mixed_values() {
        #[rustfmt::skip]
        let values = vec![
            1.0, 1.0, 2.0, 2.0,
            1.0, 1.0, 2.0, 2.0,
            3.0, 3.0, 3.0, 3.0,
        ];
        let raster = make_raster(4, 3, &values);
        let result = polygonize_raster(&raster, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize_raster should succeed");
        assert_eq!(result.num_components, 3);
    }

    // --- Checkerboard pattern ---

    #[test]
    fn test_polygonize_checkerboard_4conn() {
        // Checkerboard: with 4-connectivity, each cell is isolated
        #[rustfmt::skip]
        let grid = vec![
            1.0, 2.0, 1.0,
            2.0, 1.0, 2.0,
            1.0, 2.0, 1.0,
        ];
        let mut opts = PolygonizeOptions::default();
        opts.connectivity = Connectivity::Four;
        let result = polygonize(&grid, 3, 3, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // With 4-conn, each 1 and each 2 is separate
        // 5 ones + 4 twos = 9 components
        assert_eq!(result.num_components, 9);
    }

    #[test]
    fn test_polygonize_checkerboard_8conn() {
        // Checkerboard: with 8-connectivity, diagonals connect
        #[rustfmt::skip]
        let grid = vec![
            1.0, 2.0, 1.0,
            2.0, 1.0, 2.0,
            1.0, 2.0, 1.0,
        ];
        let mut opts = PolygonizeOptions::default();
        opts.connectivity = Connectivity::Eight;
        let result = polygonize(&grid, 3, 3, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // With 8-conn, all 1s connect (5 pixels) and all 2s connect (4 pixels)
        assert_eq!(result.num_components, 2);
    }

    // --- Large grid stress test ---

    #[test]
    fn test_polygonize_large_grid() {
        // 50x50 grid with a radial pattern (multiple concentric regions)
        let width = 50;
        let height = 50;
        let mut grid = vec![0.0_f64; width * height];
        let cx = 25.0_f64;
        let cy = 25.0_f64;
        for y in 0..height {
            for x in 0..width {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                let dist = (dx.mul_add(dx, dy * dy)).sqrt();
                // Quantize distance into bands
                grid[y * width + x] = (dist / 5.0).floor();
            }
        }
        let result = polygonize(&grid, width, height, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert!(result.num_components > 1);
        assert!(!result.polygons.is_empty());
    }

    #[test]
    fn test_polygonize_stress_500x500() {
        // 500x500 stress test: checkerboard-like quantized radial + stripes to
        // exercise CCL and boundary extraction at scale. The quantization here
        // produces a bounded number of unique values, keeping component count
        // tractable while still exercising a 250,000-pixel grid.
        let width = 500;
        let height = 500;
        let mut grid = vec![0.0_f64; width * height];
        for y in 0..height {
            for x in 0..width {
                // Coarse bands + column stripes produce several components
                let band = (y / 100) as f64;
                let stripe = (x / 125) as f64;
                grid[y * width + x] = band * 4.0 + stripe;
            }
        }
        let result = polygonize(&grid, width, height, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // Expect bounded number of components: at most 5 bands * 4 stripes + slack.
        assert!(result.num_components >= 5);
        assert!(result.num_components <= 64);
        assert!(!result.polygons.is_empty());
        assert_eq!(result.width, 500);
        assert_eq!(result.height, 500);
    }

    // --- Donut / hole integration tests ---

    #[test]
    fn test_polygonize_donut_with_hole() {
        // 5x5 grid: outer ring of 1s, inner 3x3 of nodata (0.0).
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 1.0, 1.0, 1.0,
            1.0, 0.0, 0.0, 0.0, 1.0,
            1.0, 0.0, 0.0, 0.0, 1.0,
            1.0, 0.0, 0.0, 0.0, 1.0,
            1.0, 1.0, 1.0, 1.0, 1.0,
        ];
        let mut opts = PolygonizeOptions::default();
        opts.nodata = Some(0.0);
        let result = polygonize(&grid, 5, 5, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // Exactly one polygon for the donut region; it must have one hole.
        assert_eq!(result.polygons.len(), 1);
        let feat = &result.polygons[0];
        assert!((feat.value - 1.0).abs() < f64::EPSILON);
        assert_eq!(
            feat.polygon.interiors.len(),
            1,
            "donut polygon should have exactly one interior hole"
        );
    }

    #[test]
    fn test_polygonize_multiple_disjoint_components() {
        // 5x1 grid with two components of value=1 separated by nodata.
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 0.0, 1.0, 1.0,
        ];
        let mut opts = PolygonizeOptions::default();
        opts.nodata = Some(0.0);
        let result = polygonize(&grid, 5, 1, &opts);
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        // Two separate components of value 1
        assert_eq!(result.num_components, 2);
    }

    // --- Options defaults ---

    #[test]
    fn test_options_default() {
        let opts = PolygonizeOptions::default();
        assert_eq!(opts.connectivity, Connectivity::Eight);
        assert!(opts.nodata.is_none());
        assert!((opts.nodata_tolerance - 1e-10).abs() < f64::EPSILON);
        assert!(opts.transform.is_none());
        assert!((opts.simplify_tolerance).abs() < f64::EPSILON);
        assert!((opts.min_area).abs() < f64::EPSILON);
        assert_eq!(opts.boundary_method, BoundaryMethod::PixelEdge);
    }

    #[test]
    fn test_boundary_method_default() {
        assert_eq!(BoundaryMethod::default(), BoundaryMethod::PixelEdge);
    }

    // --- Edge cases ---

    #[test]
    fn test_polygonize_1x1_grid() {
        let grid = vec![1.0];
        let result = polygonize(&grid, 1, 1, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        assert_eq!(result.num_components, 1);
    }

    #[test]
    fn test_polygonize_1xn_grid() {
        let grid = vec![1.0, 2.0, 1.0, 2.0];
        let result = polygonize(&grid, 4, 1, &PolygonizeOptions::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_polygonize_nx1_grid() {
        let grid = vec![1.0, 2.0, 1.0, 2.0];
        let result = polygonize(&grid, 1, 4, &PolygonizeOptions::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_polygonize_with_negative_values() {
        let grid = vec![-1.0, -2.0, -1.0, -2.0];
        let result = polygonize(&grid, 2, 2, &PolygonizeOptions::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_same_value_function() {
        assert!(same_value(1.0, 1.0));
        assert!(same_value(0.0, 0.0));
        assert!(!same_value(1.0, 2.0));
    }

    #[test]
    fn test_is_nodata_value() {
        assert!(is_nodata_value(f64::NAN, None, 1e-10));
        assert!(is_nodata_value(-9999.0, Some(-9999.0), 1e-10));
        assert!(!is_nodata_value(1.0, Some(-9999.0), 1e-10));
        assert!(!is_nodata_value(1.0, None, 1e-10));
    }

    #[test]
    fn test_simplify_ring_passthrough() {
        let ring = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)];
        let result = simplify_ring(&ring, 0.0);
        assert_eq!(result.len(), ring.len());
    }

    #[test]
    fn test_simplify_ring_with_tolerance() {
        let ring = vec![
            (0.0, 0.0),
            (0.5, 0.001),
            (1.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
            (0.5, 1.001),
            (0.0, 1.0),
            (0.0, 0.5),
            (0.0, 0.0),
        ];
        let result = simplify_ring(&ring, 0.1);
        assert!(result.len() <= ring.len());
    }

    #[test]
    fn test_build_polygon_valid() {
        let ext = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let result = build_polygon(ext, vec![]);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").is_some());
    }

    #[test]
    fn test_build_polygon_too_few_coords() {
        let ext = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 0.0)];
        let result = build_polygon(ext, vec![]);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").is_none());
    }

    #[test]
    fn test_build_polygon_auto_close() {
        let ext = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            // Not closed
        ];
        let result = build_polygon(ext, vec![]);
        assert!(result.is_ok());
        // Should auto-close and succeed
        let poly = result.expect("should succeed");
        assert!(poly.is_some());
    }

    // --- Integration: pixel-edge boundaries produce valid polygons ---

    #[test]
    fn test_pixel_edge_produces_valid_polygons() {
        #[rustfmt::skip]
        let grid = vec![
            1.0, 1.0, 2.0,
            1.0, 2.0, 2.0,
            2.0, 2.0, 2.0,
        ];
        let result = polygonize(&grid, 3, 3, &PolygonizeOptions::default());
        assert!(result.is_ok());
        let result = result.expect("polygonize should succeed");
        for feat in &result.polygons {
            // Each polygon should have at least 4 exterior coordinates (closed ring)
            assert!(
                feat.polygon.exterior.coords.len() >= 4,
                "polygon for value {} has too few coords: {}",
                feat.value,
                feat.polygon.exterior.coords.len()
            );
        }
    }
}
