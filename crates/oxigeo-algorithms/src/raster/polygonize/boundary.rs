//! Boundary tracing and ring classification for raster polygonization
//!
//! Implements:
//! - Moore-Neighbor contour tracing (boundary following on a label grid)
//! - Signed-area ring classification (exterior vs. hole)
//! - Hole-to-exterior assignment via point-in-polygon tests
//!
//! The algorithm operates on a **label grid** produced by connected component
//! labeling. Each unique non-zero label represents one connected region. The
//! tracer walks around the boundary of each region, producing closed coordinate
//! rings in pixel space.

use std::collections::HashMap;

use oxigeo_core::vector::Coordinate;

use crate::error::{AlgorithmError, Result};

// ---------------------------------------------------------------------------
// Connectivity mode
// ---------------------------------------------------------------------------

/// Pixel connectivity mode for boundary tracing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectivity {
    /// 4-connected: only orthogonal neighbors (N, E, S, W)
    Four,
    /// 8-connected: orthogonal + diagonal neighbors
    Eight,
}

impl Default for Connectivity {
    fn default() -> Self {
        Self::Eight
    }
}

// ---------------------------------------------------------------------------
// Direction tables
// ---------------------------------------------------------------------------

/// 8-direction offsets: (dx, dy) in clockwise order starting from E.
///
/// Index mapping:
///   0=E, 1=SE, 2=S, 3=SW, 4=W, 5=NW, 6=N, 7=NE
const DIR8_DX: [i32; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
const DIR8_DY: [i32; 8] = [0, 1, 1, 1, 0, -1, -1, -1];

/// 4-direction offsets: (dx, dy) in clockwise order starting from E.
///
/// Index mapping:
///   0=E, 1=S, 2=W, 3=N
const DIR4_DX: [i32; 4] = [1, 0, -1, 0];
const DIR4_DY: [i32; 4] = [0, 1, 0, -1];

// ---------------------------------------------------------------------------
// Ring types
// ---------------------------------------------------------------------------

/// A closed ring of pixel-space coordinates extracted from boundary tracing.
#[derive(Debug, Clone)]
pub(crate) struct PixelRing {
    /// The label of the connected component this ring belongs to.
    pub label: u32,
    /// Coordinates in pixel space (closed: first == last).
    pub coords: Vec<(f64, f64)>,
    /// Signed area (positive = CCW = exterior, negative = CW = hole).
    pub signed_area: f64,
}

impl PixelRing {
    /// Returns `true` if this ring is an exterior ring (CCW, positive signed area).
    pub fn is_exterior(&self) -> bool {
        self.signed_area >= 0.0
    }
}

/// A classified polygon: one exterior ring plus zero or more holes.
#[derive(Debug, Clone)]
pub(crate) struct ClassifiedPolygon {
    pub label: u32,
    pub exterior: Vec<(f64, f64)>,
    pub holes: Vec<Vec<(f64, f64)>>,
}

// ---------------------------------------------------------------------------
// Boundary tracing
// ---------------------------------------------------------------------------

/// Trace all boundary rings for every non-zero label in the grid.
///
/// Uses Moore-Neighbor contour tracing. Each connected component can produce
/// one exterior ring and zero or more interior (hole) rings.
///
/// # Arguments
/// * `labels` - Flat label grid (row-major, width x height). Label 0 = background.
/// * `width` - Grid width in pixels.
/// * `height` - Grid height in pixels.
/// * `connectivity` - Pixel connectivity mode.
///
/// # Returns
/// A list of classified polygons, one per connected component.
pub(crate) fn trace_boundaries(
    labels: &[u32],
    width: usize,
    height: usize,
    connectivity: Connectivity,
) -> Result<Vec<ClassifiedPolygon>> {
    if labels.len() != width * height {
        return Err(AlgorithmError::InvalidInput(
            "label grid size does not match width*height".to_string(),
        ));
    }

    // Collect unique non-zero labels
    let mut label_set: Vec<u32> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        for &lbl in labels {
            if lbl != 0 && seen.insert(lbl) {
                label_set.push(lbl);
            }
        }
    }
    label_set.sort_unstable();

    // For each label, trace all boundary rings
    let mut all_rings: Vec<PixelRing> = Vec::new();

    // We use a visited grid per label to avoid re-tracing the same boundary.
    // To save memory, we reuse a single visited buffer.
    let mut visited = vec![false; width * height];

    for &target_label in &label_set {
        // Reset visited
        for v in visited.iter_mut() {
            *v = false;
        }

        // Find all boundary start pixels for this label and trace rings
        let rings = trace_label_boundaries(
            labels,
            width,
            height,
            target_label,
            connectivity,
            &mut visited,
        );
        all_rings.extend(rings);
    }

    // Classify rings into exteriors and holes, then assign holes to exteriors
    classify_and_assemble(all_rings)
}

/// Trace all boundary rings for a single label.
fn trace_label_boundaries(
    labels: &[u32],
    width: usize,
    height: usize,
    target: u32,
    connectivity: Connectivity,
    visited: &mut [bool],
) -> Vec<PixelRing> {
    let mut rings = Vec::new();

    // Scan for boundary pixels of this label.
    // A boundary pixel is one that belongs to `target` and has at least one
    // non-target neighbor (including out-of-bounds as non-target).
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if labels[idx] != target || visited[idx] {
                continue;
            }

            // Check if this is a boundary pixel
            if !is_boundary_pixel(labels, width, height, x, y, target) {
                continue;
            }

            // Trace the contour starting from this pixel
            if let Some(ring) =
                trace_single_ring(labels, width, height, x, y, target, connectivity, visited)
            {
                if ring.coords.len() >= 4 {
                    rings.push(ring);
                }
            }
        }
    }

    rings
}

/// Check whether pixel (x, y) is on the boundary of `target`.
fn is_boundary_pixel(
    labels: &[u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    target: u32,
) -> bool {
    // A pixel is on the boundary if any of its 4-connected neighbors is not `target`
    let neighbors: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    for (dx, dy) in neighbors {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
            return true; // Edge of grid counts as non-target
        }
        if labels[ny as usize * width + nx as usize] != target {
            return true;
        }
    }
    false
}

/// Trace a single contour ring using Moore-Neighbor tracing.
///
/// The algorithm walks around the boundary of a connected region, following
/// the Moore neighborhood (8 or 4 directions). It produces a closed ring
/// of pixel-center coordinates.
fn trace_single_ring(
    labels: &[u32],
    width: usize,
    height: usize,
    start_x: usize,
    start_y: usize,
    target: u32,
    connectivity: Connectivity,
    visited: &mut [bool],
) -> Option<PixelRing> {
    let num_dirs = match connectivity {
        Connectivity::Eight => 8usize,
        Connectivity::Four => 4usize,
    };

    let dx_table: &[i32] = match connectivity {
        Connectivity::Eight => &DIR8_DX,
        Connectivity::Four => &DIR4_DX,
    };
    let dy_table: &[i32] = match connectivity {
        Connectivity::Eight => &DIR8_DY,
        Connectivity::Four => &DIR4_DY,
    };

    // Pixel-center coordinates for the ring
    let mut coords: Vec<(f64, f64)> = Vec::new();
    coords.push((start_x as f64 + 0.5, start_y as f64 + 0.5));
    visited[start_y * width + start_x] = true;

    let mut cx = start_x;
    let mut cy = start_y;

    // Find initial search direction: start by looking for the first non-target
    // neighbor going clockwise, then the next target neighbor after that.
    let mut dir = 0usize; // start searching from direction 0 (East)

    // Find the first non-target neighbor to establish entry direction
    for d in 0..num_dirs {
        let nx = cx as i32 + dx_table[d];
        let ny = cy as i32 + dy_table[d];
        if !in_bounds_and_target(labels, width, height, nx, ny, target) {
            // The backtrack direction is (d + num_dirs/2) mod num_dirs
            dir = (d + num_dirs / 2) % num_dirs;
            break;
        }
    }

    // Safety: limit iterations to prevent infinite loops on degenerate grids
    let max_iter = width * height * 4;
    let mut iter_count = 0;

    loop {
        iter_count += 1;
        if iter_count > max_iter {
            break;
        }

        // Search for the next boundary pixel in the Moore neighborhood
        let mut found = false;
        let search_start = (dir + 1) % num_dirs;

        for step in 0..num_dirs {
            let d = (search_start + step) % num_dirs;
            let nx = cx as i32 + dx_table[d];
            let ny = cy as i32 + dy_table[d];

            if in_bounds_and_target(labels, width, height, nx, ny, target) {
                let nxu = nx as usize;
                let nyu = ny as usize;

                // Check if we've returned to start
                if nxu == start_x && nyu == start_y && coords.len() > 2 {
                    // Close the ring
                    coords.push((start_x as f64 + 0.5, start_y as f64 + 0.5));
                    let signed_area = compute_signed_area(&coords);
                    return Some(PixelRing {
                        label: target,
                        coords,
                        signed_area,
                    });
                }

                // Move to this neighbor
                cx = nxu;
                cy = nyu;
                visited[cy * width + cx] = true;
                coords.push((cx as f64 + 0.5, cy as f64 + 0.5));

                // Set backtrack direction: opposite of the direction we came from
                dir = (d + num_dirs / 2) % num_dirs;
                found = true;
                break;
            }
        }

        if !found {
            // Isolated single pixel
            break;
        }
    }

    // If we didn't close the ring via the start pixel, try to close it anyway
    if coords.len() >= 3 {
        let first = coords[0];
        let last = coords[coords.len() - 1];
        if (first.0 - last.0).abs() > f64::EPSILON || (first.1 - last.1).abs() > f64::EPSILON {
            coords.push(first);
        }
        let signed_area = compute_signed_area(&coords);
        return Some(PixelRing {
            label: target,
            coords,
            signed_area,
        });
    }

    // Single pixel or degenerate: create a unit square ring
    if coords.len() <= 2 {
        let px = start_x as f64;
        let py = start_y as f64;
        let square = vec![
            (px, py),
            (px + 1.0, py),
            (px + 1.0, py + 1.0),
            (px, py + 1.0),
            (px, py),
        ];
        let signed_area = compute_signed_area(&square);
        return Some(PixelRing {
            label: target,
            coords: square,
            signed_area,
        });
    }

    None
}

/// Check if (nx, ny) is within bounds and belongs to `target`.
#[inline]
fn in_bounds_and_target(
    labels: &[u32],
    width: usize,
    height: usize,
    nx: i32,
    ny: i32,
    target: u32,
) -> bool {
    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
        return false;
    }
    labels[ny as usize * width + nx as usize] == target
}

// ---------------------------------------------------------------------------
// Signed area and ring classification
// ---------------------------------------------------------------------------

/// Compute the signed area of a closed ring using the shoelace formula.
///
/// Positive area = counter-clockwise (exterior ring).
/// Negative area = clockwise (hole ring).
pub(crate) fn compute_signed_area(coords: &[(f64, f64)]) -> f64 {
    if coords.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0_f64;
    let n = coords.len();
    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].0 * coords[j].1;
        area -= coords[j].0 * coords[i].1;
    }
    area * 0.5
}

/// Classify rings and assemble polygons (exterior + holes).
///
/// For each label, the largest (by absolute area) CCW ring is the exterior.
/// CW rings are holes. Each hole is assigned to the smallest containing
/// exterior ring of the same label using point-in-polygon tests.
fn classify_and_assemble(rings: Vec<PixelRing>) -> Result<Vec<ClassifiedPolygon>> {
    // Group rings by label
    let mut by_label: HashMap<u32, Vec<PixelRing>> = HashMap::new();
    for ring in rings {
        by_label.entry(ring.label).or_default().push(ring);
    }

    let mut polygons = Vec::new();

    for (label, mut label_rings) in by_label {
        // Separate exteriors (positive signed area) and holes (negative)
        let mut exteriors: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut holes: Vec<Vec<(f64, f64)>> = Vec::new();

        // Sort by absolute area descending so largest exterior comes first
        label_rings.sort_by(|a, b| {
            b.signed_area
                .abs()
                .partial_cmp(&a.signed_area.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for ring in &label_rings {
            if ring.is_exterior() {
                // Ensure CCW orientation (positive signed area)
                let mut coords = ring.coords.clone();
                if compute_signed_area(&coords) < 0.0 {
                    coords.reverse();
                }
                exteriors.push(coords);
            } else {
                // Ensure CW orientation (negative signed area) for holes
                let mut coords = ring.coords.clone();
                if compute_signed_area(&coords) > 0.0 {
                    coords.reverse();
                }
                holes.push(coords);
            }
        }

        // If no exteriors found, treat the largest ring as exterior
        if exteriors.is_empty() && !holes.is_empty() {
            let mut largest = holes.remove(0);
            largest.reverse(); // Flip to CCW
            exteriors.push(largest);
        }

        // Simple case: one exterior, assign all holes to it
        if exteriors.len() == 1 {
            polygons.push(ClassifiedPolygon {
                label,
                exterior: exteriors.into_iter().next().unwrap_or_default(),
                holes,
            });
        } else {
            // Multiple exteriors: assign each hole to the smallest containing exterior
            let mut poly_holes: Vec<Vec<Vec<(f64, f64)>>> = vec![Vec::new(); exteriors.len()];

            for hole in holes {
                // Use a point from the hole to test containment
                if let Some(test_point) = hole.first() {
                    let mut best_idx = 0usize;
                    let mut best_area = f64::MAX;
                    for (i, ext) in exteriors.iter().enumerate() {
                        if point_in_ring(test_point.0, test_point.1, ext) {
                            let area = compute_signed_area(ext).abs();
                            if area < best_area {
                                best_area = area;
                                best_idx = i;
                            }
                        }
                    }
                    poly_holes[best_idx].push(hole);
                }
            }

            for (i, ext) in exteriors.into_iter().enumerate() {
                let h = if i < poly_holes.len() {
                    std::mem::take(&mut poly_holes[i])
                } else {
                    Vec::new()
                };
                polygons.push(ClassifiedPolygon {
                    label,
                    exterior: ext,
                    holes: h,
                });
            }
        }
    }

    // Sort output by label for deterministic ordering
    polygons.sort_by_key(|p| p.label);

    Ok(polygons)
}

/// Point-in-polygon test using ray casting.
///
/// Tests whether the point (px, py) is inside the closed ring.
fn point_in_ring(px: f64, py: f64, ring: &[(f64, f64)]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let n = ring.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Pixel-edge boundary extraction (alternative approach)
// ---------------------------------------------------------------------------

/// Extract polygon boundaries as pixel-edge paths.
///
/// Instead of tracing pixel centers, this produces boundaries along the edges
/// between pixels, giving exact rectilinear polygon boundaries. This is the
/// approach used by GDAL's GDALPolygonize.
///
/// For each labeled region, the exterior boundary runs along edges where the
/// label meets a different label (or the grid boundary). The result is a set
/// of closed coordinate rings in pixel-corner space.
pub(crate) fn extract_pixel_edge_boundaries(
    labels: &[u32],
    width: usize,
    height: usize,
) -> Result<Vec<ClassifiedPolygon>> {
    if labels.len() != width * height {
        return Err(AlgorithmError::InvalidInput(
            "label grid size does not match width*height".to_string(),
        ));
    }

    // Collect unique non-zero labels
    let mut label_set: Vec<u32> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        for &lbl in labels {
            if lbl != 0 && seen.insert(lbl) {
                label_set.push(lbl);
            }
        }
    }
    label_set.sort_unstable();

    let mut all_polygons = Vec::new();

    for &target in &label_set {
        // Build a binary mask for this label
        let mask: Vec<bool> = labels.iter().map(|&l| l == target).collect();

        // Extract boundary edges
        let edges = extract_boundary_edges(&mask, width, height);

        // Assemble edges into closed rings
        let rings = assemble_edge_rings(&edges);

        // Classify rings
        let mut exteriors: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut holes: Vec<Vec<(f64, f64)>> = Vec::new();

        for ring in &rings {
            let area = compute_signed_area(ring);
            if area >= 0.0 {
                exteriors.push(ring.clone());
            } else {
                holes.push(ring.clone());
            }
        }

        // If no exteriors, promote the largest hole
        if exteriors.is_empty() && !holes.is_empty() {
            // Sort by absolute area descending
            holes.sort_by(|a, b| {
                compute_signed_area(b)
                    .abs()
                    .partial_cmp(&compute_signed_area(a).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut largest = holes.remove(0);
            largest.reverse();
            exteriors.push(largest);
        }

        if exteriors.len() == 1 {
            all_polygons.push(ClassifiedPolygon {
                label: target,
                exterior: exteriors.into_iter().next().unwrap_or_default(),
                holes,
            });
        } else {
            // Multiple exteriors: assign holes
            let mut poly_holes: Vec<Vec<Vec<(f64, f64)>>> = vec![Vec::new(); exteriors.len()];

            for hole in holes {
                if let Some(test_point) = hole.first() {
                    let mut best_idx = 0usize;
                    let mut best_area = f64::MAX;
                    for (i, ext) in exteriors.iter().enumerate() {
                        if point_in_ring(test_point.0, test_point.1, ext) {
                            let area = compute_signed_area(ext).abs();
                            if area < best_area {
                                best_area = area;
                                best_idx = i;
                            }
                        }
                    }
                    poly_holes[best_idx].push(hole);
                }
            }

            for (i, ext) in exteriors.into_iter().enumerate() {
                let h = if i < poly_holes.len() {
                    std::mem::take(&mut poly_holes[i])
                } else {
                    Vec::new()
                };
                all_polygons.push(ClassifiedPolygon {
                    label: target,
                    exterior: ext,
                    holes: h,
                });
            }
        }
    }

    all_polygons.sort_by_key(|p| p.label);
    Ok(all_polygons)
}

/// An edge segment between two pixel corners.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct EdgeSegment {
    /// Start corner (col, row) in pixel-corner coordinates
    x0: i32,
    y0: i32,
    /// End corner (col, row)
    x1: i32,
    y1: i32,
}

/// Extract all boundary edge segments for a binary mask.
///
/// An edge exists where a `true` pixel meets a `false` pixel (or the grid
/// boundary). Edge segments run along pixel corners, oriented so the `true`
/// region is to the left (producing CCW exterior rings).
fn extract_boundary_edges(mask: &[bool], width: usize, height: usize) -> Vec<EdgeSegment> {
    let mut edges = Vec::new();

    for y in 0..height {
        for x in 0..width {
            if !mask[y * width + x] {
                continue;
            }

            // Top edge: between row y and row y-1
            if y == 0 || !mask[(y - 1) * width + x] {
                // Edge from (x, y) to (x+1, y), left-to-right means interior is below
                edges.push(EdgeSegment {
                    x0: x as i32,
                    y0: y as i32,
                    x1: x as i32 + 1,
                    y1: y as i32,
                });
            }

            // Bottom edge: between row y and row y+1
            if y == height - 1 || !mask[(y + 1) * width + x] {
                // Edge from (x+1, y+1) to (x, y+1), right-to-left means interior is above
                edges.push(EdgeSegment {
                    x0: x as i32 + 1,
                    y0: y as i32 + 1,
                    x1: x as i32,
                    y1: y as i32 + 1,
                });
            }

            // Left edge: between col x and col x-1
            if x == 0 || !mask[y * width + x - 1] {
                // Edge from (x, y+1) to (x, y), bottom-to-top means interior is right
                edges.push(EdgeSegment {
                    x0: x as i32,
                    y0: y as i32 + 1,
                    x1: x as i32,
                    y1: y as i32,
                });
            }

            // Right edge: between col x and col x+1
            if x == width - 1 || !mask[y * width + x + 1] {
                // Edge from (x+1, y) to (x+1, y+1), top-to-bottom means interior is left
                edges.push(EdgeSegment {
                    x0: x as i32 + 1,
                    y0: y as i32,
                    x1: x as i32 + 1,
                    y1: y as i32 + 1,
                });
            }
        }
    }

    edges
}

/// Assemble edge segments into closed rings by chaining endpoint matches.
fn assemble_edge_rings(edges: &[EdgeSegment]) -> Vec<Vec<(f64, f64)>> {
    if edges.is_empty() {
        return Vec::new();
    }

    // Build adjacency: start_point -> list of (edge_index)
    let mut adjacency: HashMap<(i32, i32), Vec<usize>> = HashMap::with_capacity(edges.len());
    for (i, edge) in edges.iter().enumerate() {
        adjacency.entry((edge.x0, edge.y0)).or_default().push(i);
    }

    let mut used = vec![false; edges.len()];
    let mut rings: Vec<Vec<(f64, f64)>> = Vec::new();

    for start_idx in 0..edges.len() {
        if used[start_idx] {
            continue;
        }

        let mut ring = Vec::new();
        let mut current_idx = start_idx;

        loop {
            if used[current_idx] {
                break;
            }
            used[current_idx] = true;

            let edge = &edges[current_idx];
            ring.push((edge.x0 as f64, edge.y0 as f64));

            // Find next edge that starts where this one ends
            let end_point = (edge.x1, edge.y1);
            let mut found_next = false;

            if let Some(candidates) = adjacency.get(&end_point) {
                for &cand_idx in candidates {
                    if !used[cand_idx] {
                        current_idx = cand_idx;
                        found_next = true;
                        break;
                    }
                }
            }

            if !found_next {
                break;
            }
        }

        // Close the ring
        if ring.len() >= 3 {
            let first = ring[0];
            ring.push(first);
            rings.push(ring);
        }
    }

    rings
}

// ---------------------------------------------------------------------------
// Coordinate conversion
// ---------------------------------------------------------------------------

/// Convert pixel-space coordinates to world coordinates using a GeoTransform.
pub(crate) fn transform_coords(
    coords: &[(f64, f64)],
    gt: &oxigeo_core::types::GeoTransform,
) -> Vec<Coordinate> {
    coords
        .iter()
        .map(|&(px, py)| {
            let (wx, wy) = gt.pixel_to_world(px, py);
            Coordinate::new_2d(wx, wy)
        })
        .collect()
}

/// Convert pixel-space coordinates to Coordinate without transformation.
pub(crate) fn pixel_coords_to_coordinates(coords: &[(f64, f64)]) -> Vec<Coordinate> {
    coords
        .iter()
        .map(|&(px, py)| Coordinate::new_2d(px, py))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_area_ccw_square() {
        // CCW square: (0,0) -> (1,0) -> (1,1) -> (0,1) -> (0,0)
        let coords = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)];
        let area = compute_signed_area(&coords);
        assert!(
            area > 0.0,
            "CCW square should have positive area, got {area}"
        );
        assert!((area - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_signed_area_cw_square() {
        // CW square: (0,0) -> (0,1) -> (1,1) -> (1,0) -> (0,0)
        let coords = vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)];
        let area = compute_signed_area(&coords);
        assert!(
            area < 0.0,
            "CW square should have negative area, got {area}"
        );
        assert!((area + 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_signed_area_degenerate() {
        let coords = vec![(0.0, 0.0), (1.0, 0.0)];
        assert!((compute_signed_area(&coords)).abs() < 1e-10);
    }

    #[test]
    fn test_signed_area_empty() {
        assert!((compute_signed_area(&[])).abs() < 1e-10);
    }

    #[test]
    fn test_point_in_ring_inside() {
        let ring = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ];
        assert!(point_in_ring(5.0, 5.0, &ring));
    }

    #[test]
    fn test_point_in_ring_outside() {
        let ring = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ];
        assert!(!point_in_ring(15.0, 5.0, &ring));
    }

    #[test]
    fn test_point_in_ring_degenerate() {
        assert!(!point_in_ring(0.0, 0.0, &[]));
        assert!(!point_in_ring(0.0, 0.0, &[(0.0, 0.0), (1.0, 0.0)]));
    }

    #[test]
    fn test_is_boundary_pixel_center() {
        // 3x3 grid, center pixel is NOT boundary (surrounded by same label)
        let labels = vec![1, 1, 1, 1, 1, 1, 1, 1, 1];
        assert!(!is_boundary_pixel(&labels, 3, 3, 1, 1, 1));
    }

    #[test]
    fn test_is_boundary_pixel_edge() {
        // Corner pixel is always boundary
        let labels = vec![1, 1, 1, 1, 1, 1, 1, 1, 1];
        assert!(is_boundary_pixel(&labels, 3, 3, 0, 0, 1));
    }

    #[test]
    fn test_is_boundary_pixel_adjacent_to_different() {
        let labels = vec![1, 2, 1, 1, 1, 1, 1, 1, 1];
        // Pixel (0,0) is adjacent to (1,0)=2, so it's a boundary
        assert!(is_boundary_pixel(&labels, 3, 3, 0, 0, 1));
    }

    #[test]
    fn test_extract_edges_single_pixel() {
        let mask = vec![true];
        let edges = extract_boundary_edges(&mask, 1, 1);
        // A single pixel should have 4 boundary edges
        assert_eq!(edges.len(), 4);
    }

    #[test]
    fn test_extract_edges_2x2_solid() {
        // All 4 pixels are true
        let mask = vec![true, true, true, true];
        let edges = extract_boundary_edges(&mask, 2, 2);
        // Perimeter edges only: 2+2+2+2 = 8
        assert_eq!(edges.len(), 8);
    }

    #[test]
    fn test_assemble_edge_rings_square() {
        // Manually create 4 edges forming a unit square
        let edges = vec![
            EdgeSegment {
                x0: 0,
                y0: 0,
                x1: 1,
                y1: 0,
            },
            EdgeSegment {
                x0: 1,
                y0: 0,
                x1: 1,
                y1: 1,
            },
            EdgeSegment {
                x0: 1,
                y0: 1,
                x1: 0,
                y1: 1,
            },
            EdgeSegment {
                x0: 0,
                y0: 1,
                x1: 0,
                y1: 0,
            },
        ];
        let rings = assemble_edge_rings(&edges);
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].len(), 5); // 4 corners + close
    }

    #[test]
    fn test_pixel_edge_single_pixel() {
        let labels = vec![1u32];
        let result = extract_pixel_edge_boundaries(&labels, 1, 1);
        assert!(result.is_ok());
        let polys = result.ok();
        assert!(polys.is_some());
        let polys = polys.as_ref();
        assert!(polys.is_some_and(|p| p.len() == 1));
        if let Some(polys) = polys {
            assert_eq!(polys[0].label, 1);
            assert!(polys[0].exterior.len() >= 4);
        }
    }

    #[test]
    fn test_pixel_edge_two_regions() {
        // 2x1 grid: [1, 2]
        let labels = vec![1u32, 2];
        let result = extract_pixel_edge_boundaries(&labels, 2, 1);
        assert!(result.is_ok());
        let polys = result.ok();
        assert!(polys.is_some());
        let polys = polys.as_ref();
        assert!(polys.is_some_and(|p| p.len() == 2));
    }

    #[test]
    fn test_pixel_edge_with_hole() {
        // 5x5 grid with a hole:
        // 1 1 1 1 1
        // 1 0 0 0 1
        // 1 0 0 0 1
        // 1 0 0 0 1
        // 1 1 1 1 1
        #[rustfmt::skip]
        let labels = vec![
            1, 1, 1, 1, 1,
            1, 0, 0, 0, 1,
            1, 0, 0, 0, 1,
            1, 0, 0, 0, 1,
            1, 1, 1, 1, 1,
        ];
        let result = extract_pixel_edge_boundaries(&labels, 5, 5);
        assert!(result.is_ok());
        let polys = result.ok();
        assert!(polys.is_some());
        if let Some(polys) = polys {
            assert_eq!(polys.len(), 1);
            assert_eq!(polys[0].label, 1);
            // Should have one exterior and one hole
            assert!(!polys[0].exterior.is_empty());
            assert_eq!(polys[0].holes.len(), 1);
        }
    }

    #[test]
    fn test_pixel_edge_empty_grid() {
        let labels = vec![0u32; 9];
        let result = extract_pixel_edge_boundaries(&labels, 3, 3);
        assert!(result.is_ok());
        let polys = result.ok();
        assert!(polys.is_some_and(|p| p.is_empty()));
    }

    #[test]
    fn test_pixel_edge_size_mismatch() {
        let labels = vec![1u32, 2, 3]; // 3 elements
        let result = extract_pixel_edge_boundaries(&labels, 2, 2); // expects 4
        assert!(result.is_err());
    }

    #[test]
    fn test_transform_coords() {
        let gt = oxigeo_core::types::GeoTransform::north_up(100.0, 200.0, 10.0, -10.0);
        let pixel_coords = vec![(0.0, 0.0), (1.0, 1.0)];
        let world_coords = transform_coords(&pixel_coords, &gt);
        assert_eq!(world_coords.len(), 2);
        assert!((world_coords[0].x - 100.0).abs() < 1e-10);
        assert!((world_coords[0].y - 200.0).abs() < 1e-10);
        assert!((world_coords[1].x - 110.0).abs() < 1e-10);
        assert!((world_coords[1].y - 190.0).abs() < 1e-10);
    }

    #[test]
    fn test_pixel_coords_to_coordinates() {
        let coords = vec![(1.5, 2.5), (3.0, 4.0)];
        let result = pixel_coords_to_coordinates(&coords);
        assert_eq!(result.len(), 2);
        assert!((result[0].x - 1.5).abs() < 1e-10);
        assert!((result[0].y - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_connectivity_default() {
        assert_eq!(Connectivity::default(), Connectivity::Eight);
    }

    #[test]
    fn test_trace_boundaries_single_region() {
        // 3x3 grid, all label=1
        let labels = vec![1u32; 9];
        let result = trace_boundaries(&labels, 3, 3, Connectivity::Eight);
        assert!(result.is_ok());
        let polys = result.ok();
        assert!(polys.is_some());
        if let Some(polys) = polys {
            assert!(!polys.is_empty());
            assert_eq!(polys[0].label, 1);
        }
    }

    #[test]
    fn test_trace_boundaries_two_separate_regions() {
        // 5x1 grid: [1, 1, 0, 2, 2]
        let labels = vec![1u32, 1, 0, 2, 2];
        let result = trace_boundaries(&labels, 5, 1, Connectivity::Four);
        assert!(result.is_ok());
        let polys = result.ok();
        assert!(polys.is_some());
        if let Some(polys) = polys {
            assert_eq!(polys.len(), 2);
        }
    }

    #[test]
    fn test_trace_boundaries_size_mismatch() {
        let labels = vec![1u32; 5];
        let result = trace_boundaries(&labels, 3, 3, Connectivity::Eight);
        assert!(result.is_err());
    }

    #[test]
    fn test_pixel_ring_is_exterior() {
        let ring = PixelRing {
            label: 1,
            coords: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)],
            signed_area: 1.0,
        };
        assert!(ring.is_exterior());

        let hole = PixelRing {
            label: 1,
            coords: vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)],
            signed_area: -1.0,
        };
        assert!(!hole.is_exterior());
    }
}
