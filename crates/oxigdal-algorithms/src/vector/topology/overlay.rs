//! Advanced overlay operations for geometric analysis
//!
//! This module implements sophisticated overlay operations that combine multiple
//! geometries using different overlay types (intersection, union, difference, etc.).

use crate::error::Result;
use oxigdal_core::vector::{Coordinate, LineString, MultiPolygon, Point, Polygon};

/// Type of overlay operation to perform
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayType {
    /// Intersection - returns the common area
    Intersection,
    /// Union - returns the combined area
    Union,
    /// Difference - returns area in first but not in second
    Difference,
    /// Symmetric difference - returns area in either but not both
    SymmetricDifference,
    /// Identity - preserves first geometry's attributes
    Identity,
    /// Update - updates first geometry with second
    Update,
}

/// Options for overlay operations
#[derive(Debug, Clone)]
pub struct OverlayOptions {
    /// Tolerance for coordinate comparison
    pub tolerance: f64,
    /// Whether to preserve topology
    pub preserve_topology: bool,
    /// Whether to snap vertices to grid
    pub snap_to_grid: bool,
    /// Grid size for snapping (if enabled)
    pub grid_size: f64,
    /// Whether to simplify result
    pub simplify_result: bool,
    /// Simplification tolerance (if enabled)
    pub simplify_tolerance: f64,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-10,
            preserve_topology: true,
            snap_to_grid: false,
            grid_size: 1e-6,
            simplify_result: false,
            simplify_tolerance: 1e-8,
        }
    }
}

/// Represents an edge in the overlay graph
#[derive(Debug, Clone)]
struct OverlayEdge {
    start: Coordinate,
    end: Coordinate,
    left_label: EdgeLabel,
    right_label: EdgeLabel,
}

/// Label for an edge indicating which input geometries it belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgeLabel {
    /// Edge is in geometry A
    in_a: bool,
    /// Edge is in geometry B
    in_b: bool,
    /// Edge is on boundary of A
    on_boundary_a: bool,
    /// Edge is on boundary of B
    on_boundary_b: bool,
}

impl EdgeLabel {
    fn new() -> Self {
        Self {
            in_a: false,
            in_b: false,
            on_boundary_a: false,
            on_boundary_b: false,
        }
    }

    fn should_include(&self, overlay_type: OverlayType) -> bool {
        match overlay_type {
            OverlayType::Intersection => self.in_a && self.in_b,
            OverlayType::Union => self.in_a || self.in_b,
            OverlayType::Difference => self.in_a && !self.in_b,
            OverlayType::SymmetricDifference => self.in_a ^ self.in_b,
            OverlayType::Identity => self.in_a,
            OverlayType::Update => {
                if self.in_b {
                    true
                } else {
                    self.in_a && !self.on_boundary_a
                }
            }
        }
    }
}

/// Overlay graph for computing overlay operations
struct OverlayGraph {
    edges: Vec<OverlayEdge>,
    vertices: Vec<Coordinate>,
    tolerance: f64,
}

impl OverlayGraph {
    fn new(tolerance: f64) -> Self {
        Self {
            edges: Vec::new(),
            vertices: Vec::new(),
            tolerance,
        }
    }

    fn add_polygon(&mut self, polygon: &Polygon, is_a: bool) -> Result<()> {
        // Add exterior ring edges
        let coords = &polygon.exterior.coords;
        for i in 0..coords.len().saturating_sub(1) {
            let start = coords[i];
            let end = coords[i + 1];

            let mut label = EdgeLabel::new();
            if is_a {
                label.in_a = true;
                label.on_boundary_a = true;
            } else {
                label.in_b = true;
                label.on_boundary_b = true;
            }

            self.add_edge(start, end, label)?;
        }

        // Add interior ring edges (holes)
        for interior in &polygon.interiors {
            let coords = &interior.coords;
            for i in 0..coords.len().saturating_sub(1) {
                let start = coords[i];
                let end = coords[i + 1];

                let mut label = EdgeLabel::new();
                if is_a {
                    label.in_a = true;
                    label.on_boundary_a = true;
                } else {
                    label.in_b = true;
                    label.on_boundary_b = true;
                }

                self.add_edge(start, end, label)?;
            }
        }

        Ok(())
    }

    fn add_edge(&mut self, start: Coordinate, end: Coordinate, label: EdgeLabel) -> Result<()> {
        // Check for degenerate edge
        if Self::coords_equal(&start, &end, self.tolerance) {
            return Ok(());
        }

        // Find or add vertices
        let start_idx = self.find_or_add_vertex(start);
        let end_idx = self.find_or_add_vertex(end);

        if start_idx == end_idx {
            return Ok(());
        }

        // Create edge
        let edge = OverlayEdge {
            start: self.vertices[start_idx],
            end: self.vertices[end_idx],
            left_label: label,
            right_label: label,
        };

        self.edges.push(edge);
        Ok(())
    }

    fn find_or_add_vertex(&mut self, coord: Coordinate) -> usize {
        // Look for existing vertex within tolerance
        for (idx, vertex) in self.vertices.iter().enumerate() {
            if Self::coords_equal(vertex, &coord, self.tolerance) {
                return idx;
            }
        }

        // Add new vertex
        let idx = self.vertices.len();
        self.vertices.push(coord);
        idx
    }

    fn coords_equal(a: &Coordinate, b: &Coordinate, tolerance: f64) -> bool {
        (a.x - b.x).abs() < tolerance && (a.y - b.y).abs() < tolerance
    }

    fn compute_intersections(&mut self) -> Result<()> {
        // Use sweep line algorithm to find all edge intersections
        let n = self.edges.len();

        // First pass: Collect all intersections without modifying the edges vector.
        let mut intersections = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                if let Some(intersection) = self.compute_edge_intersection(i, j)? {
                    intersections.push((i, j, intersection));
                }
            }
        }

        // Group intersections by edge index to handle multiple splits correctly
        use std::collections::HashMap;
        let mut edge_splits: HashMap<usize, Vec<Coordinate>> = HashMap::new();

        for (i, j, intersection) in intersections {
            edge_splits.entry(i).or_default().push(intersection);
            edge_splits.entry(j).or_default().push(intersection);
        }

        // Split each edge at all its intersection points
        // Process in reverse order to avoid index invalidation
        let mut split_edges = Vec::new();
        for edge_idx in (0..n).rev() {
            if let Some(split_points) = edge_splits.get(&edge_idx) {
                let edge = self.edges[edge_idx].clone();

                // Filter out endpoints
                let mut points: Vec<Coordinate> = split_points
                    .iter()
                    .filter(|p| {
                        !Self::coords_equal(p, &edge.start, self.tolerance)
                            && !Self::coords_equal(p, &edge.end, self.tolerance)
                    })
                    .copied()
                    .collect();

                if points.is_empty() {
                    continue;
                }

                // Sort points along the edge
                points.sort_by(|a, b| {
                    let t_a = if (edge.end.x - edge.start.x).abs() > self.tolerance {
                        (a.x - edge.start.x) / (edge.end.x - edge.start.x)
                    } else {
                        (a.y - edge.start.y) / (edge.end.y - edge.start.y)
                    };
                    let t_b = if (edge.end.x - edge.start.x).abs() > self.tolerance {
                        (b.x - edge.start.x) / (edge.end.x - edge.start.x)
                    } else {
                        (b.y - edge.start.y) / (edge.end.y - edge.start.y)
                    };
                    t_a.partial_cmp(&t_b).unwrap_or(std::cmp::Ordering::Equal)
                });

                // Create split edges
                let mut segments = Vec::new();
                let mut current_start = edge.start;
                for point in points {
                    segments.push(OverlayEdge {
                        start: current_start,
                        end: point,
                        left_label: edge.left_label,
                        right_label: edge.right_label,
                    });
                    current_start = point;
                }
                // Final segment
                segments.push(OverlayEdge {
                    start: current_start,
                    end: edge.end,
                    left_label: edge.left_label,
                    right_label: edge.right_label,
                });

                // Store for later
                split_edges.push((edge_idx, segments));
            }
        }

        // Apply splits (in reverse order to maintain indices)
        for (edge_idx, segments) in split_edges {
            self.edges[edge_idx] = segments[0].clone();
            for segment in segments.into_iter().skip(1) {
                self.edges.push(segment);
            }
        }

        Ok(())
    }

    fn compute_edge_intersection(&self, i: usize, j: usize) -> Result<Option<Coordinate>> {
        let e1 = &self.edges[i];
        let e2 = &self.edges[j];

        let p1 = &e1.start;
        let p2 = &e1.end;
        let p3 = &e2.start;
        let p4 = &e2.end;

        let d = (p1.x - p2.x) * (p3.y - p4.y) - (p1.y - p2.y) * (p3.x - p4.x);

        if d.abs() < self.tolerance {
            // Parallel or coincident
            return Ok(None);
        }

        let t = ((p1.x - p3.x) * (p3.y - p4.y) - (p1.y - p3.y) * (p3.x - p4.x)) / d;
        let u = -((p1.x - p2.x) * (p1.y - p3.y) - (p1.y - p2.y) * (p1.x - p3.x)) / d;

        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            let x = p1.x + t * (p2.x - p1.x);
            let y = p1.y + t * (p2.y - p1.y);
            Ok(Some(Coordinate::new_2d(x, y)))
        } else {
            Ok(None)
        }
    }

    fn split_edge_at_point(&mut self, edge_idx: usize, point: Coordinate) -> Result<()> {
        let edge = self.edges[edge_idx].clone();

        // Check if point is actually on the edge (not at endpoints)
        if Self::coords_equal(&edge.start, &point, self.tolerance)
            || Self::coords_equal(&edge.end, &point, self.tolerance)
        {
            return Ok(());
        }

        // Create two new edges
        let edge1 = OverlayEdge {
            start: edge.start,
            end: point,
            left_label: edge.left_label,
            right_label: edge.right_label,
        };

        let edge2 = OverlayEdge {
            start: point,
            end: edge.end,
            left_label: edge.left_label,
            right_label: edge.right_label,
        };

        // Replace original edge with split edges
        self.edges[edge_idx] = edge1;
        self.edges.push(edge2);

        Ok(())
    }

    fn label_edges(&mut self, poly_a: &Polygon, poly_b: &Polygon) -> Result<()> {
        // Label each edge based on whether it's inside/outside each polygon
        for edge in &mut self.edges {
            // Compute midpoint of edge
            let mid_x = (edge.start.x + edge.end.x) / 2.0;
            let mid_y = (edge.start.y + edge.end.y) / 2.0;
            let midpoint = Point::new(mid_x, mid_y);

            // For edges NOT on boundary A, check if midpoint is in polygon A
            if !edge.left_label.on_boundary_a {
                edge.left_label.in_a = crate::vector::point_in_polygon(&midpoint.coord, poly_a)?;
            }
            // For edges ON boundary A, in_a is already set to true in add_polygon
            // but we still need to check if it's in B

            // For edges NOT on boundary B, check if midpoint is in polygon B
            if !edge.left_label.on_boundary_b {
                edge.left_label.in_b = crate::vector::point_in_polygon(&midpoint.coord, poly_b)?;
            }
            // For edges ON boundary B, in_b is already set to true in add_polygon
            // but we still need to check if it's in A
        }

        Ok(())
    }

    fn extract_result(&self, overlay_type: OverlayType) -> Result<Vec<Polygon>> {
        // Filter edges based on overlay type
        let mut result_edges: Vec<&OverlayEdge> = self
            .edges
            .iter()
            .filter(|e| e.left_label.should_include(overlay_type))
            .collect();

        // Remove duplicate edges (edges that are reverses of each other)
        // This can happen when edges are split at intersection points
        let mut to_remove = Vec::new();
        for i in 0..result_edges.len() {
            if to_remove.contains(&i) {
                continue;
            }
            for j in (i + 1)..result_edges.len() {
                if to_remove.contains(&j) {
                    continue;
                }
                // Check if edge j is the reverse of edge i
                if Self::coords_equal(&result_edges[i].start, &result_edges[j].end, self.tolerance)
                    && Self::coords_equal(
                        &result_edges[i].end,
                        &result_edges[j].start,
                        self.tolerance,
                    )
                {
                    // Keep the edge with better orientation (arbitrary: keep first one)
                    to_remove.push(j);
                }
            }
        }

        // Remove marked edges
        for &idx in to_remove.iter().rev() {
            result_edges.remove(idx);
        }

        if result_edges.is_empty() {
            return Ok(Vec::new());
        }

        // Build polygons from edges
        let mut polygons = Vec::new();
        let mut used = vec![false; result_edges.len()];

        for start_idx in 0..result_edges.len() {
            if used[start_idx] {
                continue;
            }

            let mut coords = Vec::new();
            let mut current_idx = start_idx;
            let start_coord = result_edges[start_idx].start;

            loop {
                if used[current_idx] {
                    break;
                }

                used[current_idx] = true;
                let edge = result_edges[current_idx];
                coords.push(edge.start);

                // Check if we've closed the loop (check before looking for next unused edge)
                let next_start = edge.end;
                if Self::coords_equal(&next_start, &start_coord, self.tolerance) {
                    coords.push(start_coord); // Close the ring
                    break;
                }

                // Find next edge
                let mut found_next = false;

                for (idx, e) in result_edges.iter().enumerate() {
                    if !used[idx] && Self::coords_equal(&e.start, &next_start, self.tolerance) {
                        current_idx = idx;
                        found_next = true;
                        break;
                    }
                }

                if !found_next {
                    break;
                }
            }

            // Create polygon if we have enough coordinates
            if coords.len() >= 4 {
                if let Ok(exterior) = LineString::new(coords) {
                    if let Ok(polygon) = Polygon::new(exterior, vec![]) {
                        polygons.push(polygon);
                    }
                }
            }
        }

        Ok(polygons)
    }
}

/// Perform overlay operation on two polygons
///
/// # Arguments
///
/// * `poly_a` - First polygon
/// * `poly_b` - Second polygon
/// * `overlay_type` - Type of overlay to perform
/// * `options` - Overlay options
///
/// # Returns
///
/// Result containing the overlay result as a vector of polygons
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use oxigdal_algorithms::vector::topology::{overlay_polygon, OverlayType, OverlayOptions};
/// use oxigdal_algorithms::{Coordinate, LineString, Polygon};
///
/// let coords1 = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let exterior1 = LineString::new(coords1)?;
/// let poly1 = Polygon::new(exterior1, vec![])?;
///
/// let coords2 = vec![
///     Coordinate::new_2d(5.0, 5.0),
///     Coordinate::new_2d(15.0, 5.0),
///     Coordinate::new_2d(15.0, 15.0),
///     Coordinate::new_2d(5.0, 15.0),
///     Coordinate::new_2d(5.0, 5.0),
/// ];
/// let exterior2 = LineString::new(coords2)?;
/// let poly2 = Polygon::new(exterior2, vec![])?;
///
/// let result = overlay_polygon(&poly1, &poly2, OverlayType::Intersection, &OverlayOptions::default())?;
/// assert!(!result.is_empty());
/// # Ok(())
/// # }
/// ```
pub fn overlay_polygon(
    poly_a: &Polygon,
    poly_b: &Polygon,
    overlay_type: OverlayType,
    options: &OverlayOptions,
) -> Result<Vec<Polygon>> {
    // Create overlay graph
    let mut graph = OverlayGraph::new(options.tolerance);

    // Add both polygons to graph
    graph.add_polygon(poly_a, true)?;
    graph.add_polygon(poly_b, false)?;

    // Compute intersections
    graph.compute_intersections()?;

    // Label edges
    graph.label_edges(poly_a, poly_b)?;

    // Extract result
    let mut result = graph.extract_result(overlay_type)?;

    // Apply post-processing
    if options.simplify_result {
        result = result
            .into_iter()
            .filter_map(|poly| simplify_polygon_result(&poly, options.simplify_tolerance))
            .collect();
    }

    Ok(result)
}

fn simplify_polygon_result(polygon: &Polygon, tolerance: f64) -> Option<Polygon> {
    use crate::vector::simplify::{SimplifyMethod, simplify_linestring};

    let simplified_exterior =
        simplify_linestring(&polygon.exterior, tolerance, SimplifyMethod::DouglasPeucker).ok()?;

    let simplified_interiors: Result<Vec<LineString>> = polygon
        .interiors
        .iter()
        .map(|interior| simplify_linestring(interior, tolerance, SimplifyMethod::DouglasPeucker))
        .collect();

    let simplified_interiors = simplified_interiors.ok()?;

    Polygon::new(simplified_exterior, simplified_interiors).ok()
}

/// Perform overlay operation on multipolygons
pub fn overlay_multipolygon(
    multi_a: &MultiPolygon,
    multi_b: &MultiPolygon,
    overlay_type: OverlayType,
    options: &OverlayOptions,
) -> Result<MultiPolygon> {
    let mut result_polygons = Vec::new();

    for poly_a in &multi_a.polygons {
        for poly_b in &multi_b.polygons {
            let overlay_result = overlay_polygon(poly_a, poly_b, overlay_type, options)?;
            result_polygons.extend(overlay_result);
        }
    }

    Ok(MultiPolygon::new(result_polygons))
}

/// Perform overlay on generic geometries (handles various geometry types)
pub fn overlay_geometries(
    geom_a: &Polygon,
    geom_b: &Polygon,
    overlay_type: OverlayType,
    options: &OverlayOptions,
) -> Result<Vec<Polygon>> {
    overlay_polygon(geom_a, geom_b, overlay_type, options)
}

/// Batch overlay operation for multiple polygon pairs
pub fn overlay_polygon_batch(
    pairs: &[(Polygon, Polygon)],
    overlay_type: OverlayType,
    options: &OverlayOptions,
) -> Result<Vec<Vec<Polygon>>> {
    pairs
        .iter()
        .map(|(a, b)| overlay_polygon(a, b, overlay_type, options))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_square(x: f64, y: f64, size: f64) -> Polygon {
        let coords = vec![
            Coordinate::new_2d(x, y),
            Coordinate::new_2d(x + size, y),
            Coordinate::new_2d(x + size, y + size),
            Coordinate::new_2d(x, y + size),
            Coordinate::new_2d(x, y),
        ];
        let exterior = LineString::new(coords).expect("Failed to create linestring");
        Polygon::new(exterior, vec![]).expect("Failed to create polygon")
    }

    #[test]
    fn test_overlay_intersection() {
        let poly1 = create_square(0.0, 0.0, 10.0);
        let poly2 = create_square(5.0, 5.0, 10.0);

        let result = overlay_polygon(
            &poly1,
            &poly2,
            OverlayType::Intersection,
            &OverlayOptions::default(),
        );

        assert!(result.is_ok());
        let polygons = result.expect("Overlay failed");
        assert!(!polygons.is_empty());
    }

    #[test]
    fn test_overlay_union() {
        let poly1 = create_square(0.0, 0.0, 10.0);
        let poly2 = create_square(5.0, 5.0, 10.0);

        let result = overlay_polygon(
            &poly1,
            &poly2,
            OverlayType::Union,
            &OverlayOptions::default(),
        );

        assert!(result.is_ok());
        let polygons = result.expect("Overlay failed");
        assert!(!polygons.is_empty());
    }

    #[test]
    fn test_overlay_difference() {
        let poly1 = create_square(0.0, 0.0, 10.0);
        let poly2 = create_square(5.0, 5.0, 10.0);

        let result = overlay_polygon(
            &poly1,
            &poly2,
            OverlayType::Difference,
            &OverlayOptions::default(),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_overlay_non_overlapping() {
        let poly1 = create_square(0.0, 0.0, 5.0);
        let poly2 = create_square(10.0, 10.0, 5.0);

        let result = overlay_polygon(
            &poly1,
            &poly2,
            OverlayType::Intersection,
            &OverlayOptions::default(),
        );

        assert!(result.is_ok());
        let polygons = result.expect("Overlay failed");
        assert!(polygons.is_empty()); // No intersection
    }

    #[test]
    fn test_edge_label_logic() {
        let label = EdgeLabel::new();
        assert!(!label.should_include(OverlayType::Intersection));

        let mut label = EdgeLabel::new();
        label.in_a = true;
        label.in_b = true;
        assert!(label.should_include(OverlayType::Intersection));
        assert!(label.should_include(OverlayType::Union));
        assert!(!label.should_include(OverlayType::Difference));
    }

    #[test]
    fn test_overlay_batch() {
        let poly1 = create_square(0.0, 0.0, 10.0);
        let poly2 = create_square(5.0, 5.0, 10.0);
        let poly3 = create_square(10.0, 0.0, 10.0);

        let pairs = vec![(poly1.clone(), poly2), (poly1, poly3)];

        let result = overlay_polygon_batch(
            &pairs,
            OverlayType::Intersection,
            &OverlayOptions::default(),
        );

        assert!(result.is_ok());
        let results = result.expect("Batch overlay failed");
        assert_eq!(results.len(), 2);
    }
}
