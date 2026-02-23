//! Erase operations for removing geometric features
//!
//! This module implements erase (cookie-cutter) operations that remove parts of
//! geometries based on other geometries, with options for buffering and batch processing.

use crate::error::{AlgorithmError, Result};
use crate::vector::topology::{OverlayOptions, OverlayType, overlay_polygon};
use oxigdal_core::vector::{Coordinate, LineString, MultiPolygon, Polygon};

/// Options for erase operations
#[derive(Debug, Clone)]
pub struct EraseOptions {
    /// Tolerance for coordinate comparison
    pub tolerance: f64,
    /// Whether to buffer the erase geometry before erasing
    pub buffer_erase_geom: bool,
    /// Buffer distance (if buffering enabled)
    pub buffer_distance: f64,
    /// Minimum area threshold for result polygons
    pub min_area: f64,
    /// Whether to remove slivers (very thin polygons)
    pub remove_slivers: bool,
    /// Sliver width threshold
    pub sliver_threshold: f64,
}

impl Default for EraseOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-10,
            buffer_erase_geom: false,
            buffer_distance: 0.0,
            min_area: 0.0,
            remove_slivers: false,
            sliver_threshold: 0.1,
        }
    }
}

/// Erase part of a polygon using another polygon
///
/// Removes the area of `erase_poly` from `target_poly`, similar to a cookie-cutter operation.
///
/// # Arguments
///
/// * `target_poly` - The polygon to erase from
/// * `erase_poly` - The polygon defining the area to remove
/// * `options` - Erase options
///
/// # Returns
///
/// Result containing the erased polygon(s)
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::topology::{erase_polygon, EraseOptions};
/// use oxigdal_algorithms::{Coordinate, LineString, Polygon};
/// # use oxigdal_algorithms::error::Result;
/// #
/// # fn main() -> Result<()> {
/// let coords1 = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let exterior1 = LineString::new(coords1)?;
/// let target = Polygon::new(exterior1, vec![])?;
///
/// let coords2 = vec![
///     Coordinate::new_2d(2.0, 2.0),
///     Coordinate::new_2d(5.0, 2.0),
///     Coordinate::new_2d(5.0, 5.0),
///     Coordinate::new_2d(2.0, 5.0),
///     Coordinate::new_2d(2.0, 2.0),
/// ];
/// let exterior2 = LineString::new(coords2)?;
/// let erase = Polygon::new(exterior2, vec![])?;
///
/// let result = erase_polygon(&target, &erase, &EraseOptions::default())?;
/// assert!(!result.is_empty());
/// # Ok(())
/// # }
/// ```
pub fn erase_polygon(
    target_poly: &Polygon,
    erase_poly: &Polygon,
    options: &EraseOptions,
) -> Result<Vec<Polygon>> {
    // Convert erase options to overlay options
    let overlay_options = OverlayOptions {
        tolerance: options.tolerance,
        preserve_topology: true,
        snap_to_grid: false,
        grid_size: 1e-6,
        simplify_result: false,
        simplify_tolerance: 1e-8,
    };

    // Apply buffer to erase geometry if requested
    let erase_geom = if options.buffer_erase_geom && options.buffer_distance.abs() > 1e-10 {
        buffer_erase_polygon(erase_poly, options.buffer_distance)?
    } else {
        erase_poly.clone()
    };

    // Perform difference operation (target - erase)
    let mut result = overlay_polygon(
        target_poly,
        &erase_geom,
        OverlayType::Difference,
        &overlay_options,
    )?;

    // Post-process results
    result = filter_results(result, options)?;

    Ok(result)
}

/// Buffer an erase polygon by a specified distance
fn buffer_erase_polygon(polygon: &Polygon, distance: f64) -> Result<Polygon> {
    use crate::vector::buffer::{BufferOptions, buffer_polygon};

    let buffer_options = BufferOptions::default();
    buffer_polygon(polygon, distance, &buffer_options)
}

/// Filter results based on options
fn filter_results(mut polygons: Vec<Polygon>, options: &EraseOptions) -> Result<Vec<Polygon>> {
    // Filter by minimum area
    if options.min_area > 0.0 {
        polygons.retain(|poly| {
            if let Ok(area) = compute_polygon_area(poly) {
                area >= options.min_area
            } else {
                false
            }
        });
    }

    // Remove slivers if requested
    if options.remove_slivers {
        polygons.retain(|poly| !is_sliver(poly, options.sliver_threshold));
    }

    Ok(polygons)
}

/// Compute the area of a polygon
fn compute_polygon_area(polygon: &Polygon) -> Result<f64> {
    use crate::vector::area::{AreaMethod, area_polygon};
    area_polygon(polygon, AreaMethod::Planar)
}

/// Check if a polygon is a sliver (very thin polygon)
fn is_sliver(polygon: &Polygon, threshold: f64) -> bool {
    // Compute area and perimeter
    let area = match compute_polygon_area(polygon) {
        Ok(a) => a,
        Err(_) => return false,
    };

    let perimeter = compute_polygon_perimeter(polygon);

    if perimeter < 1e-10 {
        return false;
    }

    // Compute compactness ratio
    // For a circle, this would be 1.0
    // For a very thin rectangle, this approaches 0
    let compactness = (4.0 * std::f64::consts::PI * area) / (perimeter * perimeter);

    compactness < threshold
}

/// Compute the perimeter of a polygon
fn compute_polygon_perimeter(polygon: &Polygon) -> f64 {
    let mut perimeter = compute_linestring_length(&polygon.exterior);

    for interior in &polygon.interiors {
        perimeter += compute_linestring_length(interior);
    }

    perimeter
}

/// Compute the length of a linestring
fn compute_linestring_length(linestring: &LineString) -> f64 {
    let coords = &linestring.coords;
    let mut length = 0.0;

    for i in 0..coords.len().saturating_sub(1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        length += (dx * dx + dy * dy).sqrt();
    }

    length
}

/// Erase multiple polygons from a target polygon
///
/// Removes all erase polygons from the target in sequence.
pub fn erase_geometries(
    target_poly: &Polygon,
    erase_polys: &[Polygon],
    options: &EraseOptions,
) -> Result<Vec<Polygon>> {
    let mut current_targets = vec![target_poly.clone()];

    for erase_poly in erase_polys {
        let mut new_targets = Vec::new();

        for target in &current_targets {
            let erased = erase_polygon(target, erase_poly, options)?;
            new_targets.extend(erased);
        }

        current_targets = new_targets;

        if current_targets.is_empty() {
            break;
        }
    }

    Ok(current_targets)
}

/// Erase geometries from a multipolygon
pub fn erase_multipolygon(
    target_multi: &MultiPolygon,
    erase_multi: &MultiPolygon,
    options: &EraseOptions,
) -> Result<Vec<Polygon>> {
    let mut result_polygons = Vec::new();

    for target_poly in &target_multi.polygons {
        let erase_polys: Vec<Polygon> = erase_multi.polygons.to_vec();
        let erased = erase_geometries(target_poly, &erase_polys, options)?;
        result_polygons.extend(erased);
    }

    Ok(result_polygons)
}

/// Batch erase operation for multiple polygon pairs
pub fn erase_polygon_batch(
    pairs: &[(Polygon, Polygon)],
    options: &EraseOptions,
) -> Result<Vec<Vec<Polygon>>> {
    pairs
        .iter()
        .map(|(target, erase)| erase_polygon(target, erase, options))
        .collect()
}

/// Erase with automatic buffering based on distance
///
/// This is a convenience function that automatically buffers the erase geometry.
pub fn erase_with_buffer(
    target_poly: &Polygon,
    erase_poly: &Polygon,
    buffer_distance: f64,
    tolerance: f64,
) -> Result<Vec<Polygon>> {
    let options = EraseOptions {
        tolerance,
        buffer_erase_geom: true,
        buffer_distance,
        min_area: 0.0,
        remove_slivers: false,
        sliver_threshold: 0.1,
    };

    erase_polygon(target_poly, erase_poly, &options)
}

/// Create a polygon with a hole (for testing)
fn create_polygon_with_hole(
    outer_coords: Vec<Coordinate>,
    inner_coords: Vec<Coordinate>,
) -> Result<Polygon> {
    let exterior = LineString::new(outer_coords)
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid exterior: {}", e)))?;

    let interior = LineString::new(inner_coords)
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid interior: {}", e)))?;

    Polygon::new(exterior, vec![interior])
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid polygon: {}", e)))
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
    fn test_erase_basic() {
        let target = create_square(0.0, 0.0, 10.0);
        let erase = create_square(2.0, 2.0, 3.0);

        let result = erase_polygon(&target, &erase, &EraseOptions::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_erase_no_overlap() {
        let target = create_square(0.0, 0.0, 5.0);
        let erase = create_square(10.0, 10.0, 5.0);

        let result = erase_polygon(&target, &erase, &EraseOptions::default());
        assert!(result.is_ok());

        let polygons = result.expect("Erase failed");
        assert_eq!(polygons.len(), 1); // Target unchanged
    }

    #[test]
    fn test_erase_complete_overlap() {
        let target = create_square(0.0, 0.0, 5.0);
        let erase = create_square(-1.0, -1.0, 10.0);

        let result = erase_polygon(&target, &erase, &EraseOptions::default());
        assert!(result.is_ok());

        let polygons = result.expect("Erase failed");
        assert!(
            polygons.is_empty()
                || polygons
                    .iter()
                    .all(|p| { compute_polygon_area(p).is_ok_and(|a| a < 1e-6) })
        );
    }

    #[test]
    fn test_erase_multiple() {
        let target = create_square(0.0, 0.0, 10.0);
        let erase1 = create_square(1.0, 1.0, 2.0);
        let erase2 = create_square(5.0, 5.0, 2.0);

        let erase_polys = vec![erase1, erase2];

        let result = erase_geometries(&target, &erase_polys, &EraseOptions::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_erase_with_min_area() {
        let target = create_square(0.0, 0.0, 10.0);
        let erase = create_square(2.0, 2.0, 3.0);

        let options = EraseOptions {
            min_area: 50.0, // Only keep results larger than 50 sq units
            ..Default::default()
        };

        let result = erase_polygon(&target, &erase, &options);
        assert!(result.is_ok());

        let polygons = result.expect("Erase failed");
        for poly in &polygons {
            let area = compute_polygon_area(poly).expect("Failed to compute area");
            assert!(area >= 50.0);
        }
    }

    #[test]
    fn test_is_sliver() {
        // Create a very thin rectangle (sliver)
        let sliver = create_square(0.0, 0.0, 10.0); // Actually a square for testing
        let is_sliver_result = is_sliver(&sliver, 0.5);

        // A square should not be a sliver
        assert!(!is_sliver_result);
    }

    #[test]
    fn test_compute_polygon_perimeter() {
        let square = create_square(0.0, 0.0, 10.0);
        let perimeter = compute_polygon_perimeter(&square);

        // Perimeter of a 10x10 square is 40
        assert!((perimeter - 40.0).abs() < 1e-6);
    }

    #[test]
    fn test_erase_batch() {
        let target1 = create_square(0.0, 0.0, 10.0);
        let erase1 = create_square(2.0, 2.0, 3.0);

        let target2 = create_square(20.0, 20.0, 10.0);
        let erase2 = create_square(22.0, 22.0, 3.0);

        let pairs = vec![(target1, erase1), (target2, erase2)];

        let result = erase_polygon_batch(&pairs, &EraseOptions::default());
        assert!(result.is_ok());

        let results = result.expect("Batch erase failed");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_erase_with_buffer() {
        let target = create_square(0.0, 0.0, 10.0);
        let erase = create_square(4.0, 4.0, 2.0);

        let result = erase_with_buffer(&target, &erase, 1.0, 1e-10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compute_linestring_length() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(3.0, 0.0),
            Coordinate::new_2d(3.0, 4.0),
        ];
        let linestring = LineString::new(coords).expect("Failed to create linestring");

        let length = compute_linestring_length(&linestring);

        // Length is 3 + 4 = 7
        assert!((length - 7.0).abs() < 1e-6);
    }
}
