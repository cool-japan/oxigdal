//! Postprocessing operations for ML results
//!
//! This module provides tile merging, confidence thresholding, polygon conversion,
//! and GeoJSON export capabilities.

use geo::{Simplify, unary_union};
use geo_types::{Coord, LineString, MultiPolygon, Polygon};
use geojson::{Feature, FeatureCollection, Geometry, GeometryValue};
use oxigeo_core::buffer::RasterBuffer;
use serde_json::{Map, Value as JsonValue};
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::debug;

use crate::detection::GeoDetection;
use crate::error::{PostprocessingError, Result};
use crate::segmentation::SegmentationMask;

/// Applies confidence thresholding to a probability map
///
/// # Errors
/// Returns an error if thresholding fails
pub fn apply_threshold(probabilities: &RasterBuffer, threshold: f32) -> Result<RasterBuffer> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err(PostprocessingError::InvalidThreshold { value: threshold }.into());
    }

    let mut result = probabilities.clone();

    for y in 0..probabilities.height() {
        for x in 0..probabilities.width() {
            let prob =
                probabilities
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::ExportFailed {
                        reason: format!("Failed to get probability: {}", e),
                    })?;

            let value = if prob >= threshold as f64 { 1.0 } else { 0.0 };

            result
                .set_pixel(x, y, value)
                .map_err(|e| PostprocessingError::ExportFailed {
                    reason: format!("Failed to set value: {}", e),
                })?;
        }
    }

    Ok(result)
}

/// Converts a binary mask to polygons by tracing each connected component's
/// true outline.
///
/// Each foreground connected component (4-connected, value `> 0.0`) is traced
/// with a crack-following boundary walk on the pixel-corner lattice, yielding a
/// rectilinear polygon that exactly follows the component's outline (not merely
/// its bounding box). Components whose polygon area is below `min_area` are
/// discarded.
///
/// Coordinates are in pixel space: a pixel `(x, y)` occupies the unit square
/// `[x, x+1] x [y, y+1]`, so a component's polygon vertices are integer lattice
/// points.
///
/// # Errors
/// Returns an error if conversion fails
pub fn mask_to_polygons(mask: &RasterBuffer, min_area: f64) -> Result<Vec<Polygon>> {
    debug!(
        "Converting {}x{} mask to polygons",
        mask.width(),
        mask.height()
    );

    let mut polygons = Vec::new();

    let width = mask.width();
    let height = mask.height();

    let mut visited = vec![vec![false; width as usize]; height as usize];

    for y in 0..height {
        for x in 0..width {
            if visited[y as usize][x as usize] {
                continue;
            }

            let value =
                mask.get_pixel(x, y)
                    .map_err(|e| PostprocessingError::PolygonConversionFailed {
                        reason: format!("Failed to get pixel: {}", e),
                    })?;

            if value > 0.0 {
                let polygon = trace_contour(mask, x, y, &mut visited)?;
                let area = calculate_polygon_area(&polygon);

                if area >= min_area {
                    polygons.push(polygon);
                }
            }
        }
    }

    debug!("Extracted {} polygons", polygons.len());

    Ok(polygons)
}

/// Traces the outline of the connected component containing `(start_x, start_y)`.
///
/// The component is first flood-filled (4-connected) to collect its foreground
/// pixels and mark them `visited`. The outer boundary is then extracted with a
/// crack-following walk on the pixel-corner lattice (a "right-hand rule" wall
/// follower that keeps the foreground region to the right of the travel
/// direction), producing a closed rectilinear ring that exactly follows the
/// component's shape.
///
/// `(start_x, start_y)` is the first pixel of the component encountered by the
/// row-major scan in [`mask_to_polygons`], i.e. the topmost-then-leftmost pixel,
/// so its top-left lattice corner is a valid, unambiguous starting boundary
/// vertex.
///
/// Note: holes (background pockets fully enclosed by the component) are not
/// emitted as interior rings; only the outer boundary is traced.
fn trace_contour(
    mask: &RasterBuffer,
    start_x: u64,
    start_y: u64,
    visited: &mut [Vec<bool>],
) -> Result<Polygon> {
    // 1. Flood-fill the connected component, collecting its foreground pixels.
    let mut component: HashSet<(i64, i64)> = HashSet::new();
    let mut stack = vec![(start_x, start_y)];

    while let Some((x, y)) = stack.pop() {
        if x >= mask.width() || y >= mask.height() {
            continue;
        }
        if visited[y as usize][x as usize] {
            continue;
        }

        let value =
            mask.get_pixel(x, y)
                .map_err(|e| PostprocessingError::PolygonConversionFailed {
                    reason: format!("Failed to get pixel: {}", e),
                })?;

        if value > 0.0 {
            visited[y as usize][x as usize] = true;
            component.insert((x as i64, y as i64));

            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < mask.width() {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < mask.height() {
                stack.push((x, y + 1));
            }
        }
    }

    // 2. Trace the outer boundary via crack following.
    let coords = trace_component_boundary(&component, start_x as i64, start_y as i64);

    Ok(Polygon::new(LineString::from(coords), vec![]))
}

/// Directions on the pixel-corner lattice, indexed 0=Right, 1=Down, 2=Left, 3=Up.
/// A "right turn" (clockwise, in image coordinates where `y` increases downward)
/// is `(dir + 1) % 4`.
const DIR_OFFSETS: [(i64, i64); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// Traces the outer boundary of a connected component as a closed ring of
/// pixel-corner coordinates using a crack-following ("wall follower") walk.
///
/// The region is kept to the right of the travel direction, so the walk hugs the
/// component's outline. Only corner vertices (points where the direction changes)
/// are emitted, and the ring is explicitly closed.
fn trace_component_boundary(
    component: &HashSet<(i64, i64)>,
    start_x: i64,
    start_y: i64,
) -> Vec<Coord> {
    // Foreground test for a pixel (background outside the set / out of bounds).
    let fg = |px: i64, py: i64| component.contains(&(px, py));

    // For an edge leaving corner `(x, y)` in direction `dir`, returns the pixel
    // on the right of travel and the pixel on the left of travel. The edge is a
    // valid boundary edge (foreground on the right) iff `right` is foreground and
    // `left` is background.
    let right_left = |x: i64, y: i64, dir: usize| -> ((i64, i64), (i64, i64)) {
        match dir {
            0 => ((x, y), (x, y - 1)),         // Right
            1 => ((x - 1, y), (x, y)),         // Down
            2 => ((x - 1, y - 1), (x - 1, y)), // Left
            _ => ((x, y - 1), (x - 1, y - 1)), // Up
        }
    };

    let valid_edge = |x: i64, y: i64, dir: usize| -> bool {
        let (r, l) = right_left(x, y, dir);
        fg(r.0, r.1) && !fg(l.0, l.1)
    };

    // Right-hand rule: prefer turning right, then straight, then left, then back.
    let choose = |x: i64, y: i64, incoming: usize| -> Option<usize> {
        [
            (incoming + 1) % 4, // right
            incoming,           // straight
            (incoming + 3) % 4, // left
            (incoming + 2) % 4, // back
        ]
        .into_iter()
        .find(|&turn| valid_edge(x, y, turn))
    };

    let initial_dir = 0usize; // start heading Right along the top edge
    let mut coords: Vec<Coord> = Vec::new();
    let mut x = start_x;
    let mut y = start_y;
    let mut dir = initial_dir;
    let mut first = true;

    // Safety cap: a boundary cannot be longer than 4 corners per pixel.
    let max_steps = component.len().saturating_mul(4).saturating_add(8);

    for _ in 0..max_steps {
        let next_dir = match choose(x, y, dir) {
            Some(d) => d,
            None => break, // isolated corner (should not happen for a real component)
        };

        // Stopping criterion: back at the start corner about to repeat the
        // initial heading.
        if !first && x == start_x && y == start_y && next_dir == initial_dir {
            break;
        }

        // Emit a vertex whenever the direction changes (or at the very start).
        if first || next_dir != dir {
            coords.push(Coord {
                x: x as f64,
                y: y as f64,
            });
        }

        let (dx, dy) = DIR_OFFSETS[next_dir];
        x += dx;
        y += dy;
        dir = next_dir;
        first = false;
    }

    // Close the ring.
    if let Some(first_coord) = coords.first().copied() {
        if coords.last() != Some(&first_coord) {
            coords.push(first_coord);
        }
    }

    coords
}

/// Calculates the area of a polygon
fn calculate_polygon_area(polygon: &Polygon) -> f64 {
    let coords = polygon.exterior().coords().collect::<Vec<_>>();
    if coords.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    for i in 0..coords.len() - 1 {
        area += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }

    (area / 2.0).abs()
}

/// Exports detections to GeoJSON format
///
/// # Errors
/// Returns an error if export fails
pub fn export_detections_geojson<P: AsRef<Path>>(
    detections: &[GeoDetection],
    output_path: P,
) -> Result<()> {
    debug!("Exporting {} detections to GeoJSON", detections.len());

    let features: Vec<Feature> = detections.iter().map(detection_to_feature).collect();

    let collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let json = serde_json::to_string_pretty(&collection).map_err(|e| {
        PostprocessingError::ExportFailed {
            reason: format!("Failed to serialize GeoJSON: {}", e),
        }
    })?;

    let mut file =
        File::create(output_path.as_ref()).map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to create output file: {}", e),
        })?;

    file.write_all(json.as_bytes())
        .map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to write GeoJSON: {}", e),
        })?;

    debug!("Successfully exported detections");

    Ok(())
}

/// Converts a detection to a GeoJSON feature
fn detection_to_feature(det: &GeoDetection) -> Feature {
    let polygon = det.geo_bbox.to_polygon();

    let mut properties = Map::new();
    properties.insert(
        "class_id".to_string(),
        JsonValue::Number(det.detection.class_id.into()),
    );
    properties.insert(
        "confidence".to_string(),
        JsonValue::Number(
            serde_json::Number::from_f64(det.detection.confidence as f64)
                .unwrap_or_else(|| serde_json::Number::from(0)),
        ),
    );

    if let Some(ref label) = det.detection.class_label {
        properties.insert("class_label".to_string(), JsonValue::String(label.clone()));
    }

    for (key, value) in &det.detection.attributes {
        properties.insert(key.clone(), JsonValue::String(value.clone()));
    }

    Feature {
        bbox: None,
        geometry: Some(Geometry::new(GeometryValue::from(&polygon))),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    }
}

/// Exports a segmentation mask to GeoJSON
///
/// # Errors
/// Returns an error if export fails
pub fn export_segmentation_geojson<P: AsRef<Path>>(
    mask: &SegmentationMask,
    output_path: P,
    min_area: f64,
) -> Result<()> {
    debug!("Exporting segmentation mask to GeoJSON");

    let polygons = mask_to_polygons(&mask.mask, min_area)?;

    let features: Vec<Feature> = polygons
        .iter()
        .enumerate()
        .map(|(i, poly)| {
            let mut properties = Map::new();
            properties.insert("id".to_string(), JsonValue::Number(i.into()));

            Feature {
                bbox: None,
                geometry: Some(Geometry::new(GeometryValue::from(poly))),
                id: None,
                properties: Some(properties),
                foreign_members: None,
            }
        })
        .collect();

    let collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let json = serde_json::to_string_pretty(&collection).map_err(|e| {
        PostprocessingError::ExportFailed {
            reason: format!("Failed to serialize GeoJSON: {}", e),
        }
    })?;

    let mut file =
        File::create(output_path.as_ref()).map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to create output file: {}", e),
        })?;

    file.write_all(json.as_bytes())
        .map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to write GeoJSON: {}", e),
        })?;

    debug!("Successfully exported segmentation");

    Ok(())
}

/// Simplifies polygons using the Ramer-Douglas-Peucker algorithm.
///
/// Each polygon's exterior and interior rings are simplified with the given
/// `tolerance` (the maximum distance a removed vertex may lie from the retained
/// line). A larger tolerance removes more vertices.
///
/// # Errors
/// Returns an error if `tolerance` is negative.
pub fn simplify_polygons(polygons: &[Polygon], tolerance: f64) -> Result<Vec<Polygon>> {
    if tolerance < 0.0 {
        return Err(PostprocessingError::ExportFailed {
            reason: "Tolerance must be non-negative".to_string(),
        }
        .into());
    }

    // `geo::Simplify` for `Polygon` applies Douglas-Peucker to the exterior ring
    // and every interior ring.
    Ok(polygons.iter().map(|p| p.simplify(tolerance)).collect())
}

/// Merges overlapping (or adjacent) polygons into their geometric union.
///
/// Uses `geo`'s boolean-operations `unary_union`, which computes the true union
/// of the input polygons: overlapping regions are dissolved into single faces
/// and disjoint regions remain separate parts of the returned [`MultiPolygon`].
///
/// # Errors
/// This operation does not currently fail, but returns [`Result`] for API
/// stability.
pub fn merge_polygons(polygons: &[Polygon]) -> Result<MultiPolygon> {
    if polygons.is_empty() {
        return Ok(MultiPolygon::new(Vec::new()));
    }

    Ok(unary_union(polygons.iter()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_core::types::RasterDataType;
    use std::collections::HashMap;

    #[test]
    fn test_apply_threshold() {
        let probs = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let result = apply_threshold(&probs, 0.5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mask_to_polygons() {
        let mut mask = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let _ = mask.set_pixel(5, 5, 1.0);
        let polygons = mask_to_polygons(&mask, 0.0);
        assert!(polygons.is_ok());
    }

    #[test]
    fn test_mask_to_polygons_square_outline() {
        // A solid 3x3 square must trace to a 4-corner ring with area 9.
        let mut mask = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 2..5 {
            for x in 2..5 {
                let _ = mask.set_pixel(x, y, 1.0);
            }
        }
        let polygons = mask_to_polygons(&mask, 0.0).expect("trace square");
        assert_eq!(polygons.len(), 1);
        let poly = polygons.first().expect("one polygon");
        // Exterior ring has 4 distinct corners plus the closing vertex.
        assert_eq!(poly.exterior().0.len(), 5);
        assert!((calculate_polygon_area(poly) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn test_mask_to_polygons_l_shape_not_bounding_box() {
        // An L-shaped region must NOT collapse to its bounding box: its true
        // outline has more than 4 corners and a smaller area than the 3x3 bbox.
        //
        //   X . .
        //   X . .
        //   X X X
        let mut mask = RasterBuffer::zeros(8, 8, RasterDataType::Float32);
        let _ = mask.set_pixel(1, 1, 1.0);
        let _ = mask.set_pixel(1, 2, 1.0);
        let _ = mask.set_pixel(1, 3, 1.0);
        let _ = mask.set_pixel(2, 3, 1.0);
        let _ = mask.set_pixel(3, 3, 1.0);

        let polygons = mask_to_polygons(&mask, 0.0).expect("trace L");
        assert_eq!(polygons.len(), 1);
        let poly = polygons.first().expect("one polygon");

        // Bounding box would be 3x3 = 9; the true L area is 5 pixels.
        let area = calculate_polygon_area(poly);
        assert!((area - 5.0).abs() < 1e-9, "expected L area 5, got {}", area);
        // An L outline has 6 distinct corners (+ closing vertex), never 4.
        assert!(
            poly.exterior().0.len() > 5,
            "L outline collapsed to a rectangle: {} coords",
            poly.exterior().0.len()
        );
    }

    #[test]
    fn test_simplify_polygons_reduces_vertices() {
        // A square with a redundant collinear midpoint on one edge should lose
        // that vertex after Douglas-Peucker simplification.
        let poly = Polygon::new(
            LineString::from(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 5.0, y: 0.0 }, // redundant collinear point
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        );
        let before = poly.exterior().0.len();
        let simplified = simplify_polygons(&[poly], 0.5).expect("simplify");
        let after = simplified.first().expect("one polygon").exterior().0.len();
        assert!(
            after < before,
            "expected fewer vertices after simplify: {} -> {}",
            before,
            after
        );

        // Negative tolerance is rejected.
        assert!(simplify_polygons(&[], -1.0).is_err());
    }

    #[test]
    fn test_merge_polygons_unions_overlap() {
        // Two overlapping unit squares must dissolve into a single face.
        let a = Polygon::new(
            LineString::from(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 2.0, y: 0.0 },
                Coord { x: 2.0, y: 2.0 },
                Coord { x: 0.0, y: 2.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        );
        let b = Polygon::new(
            LineString::from(vec![
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 3.0, y: 1.0 },
                Coord { x: 3.0, y: 3.0 },
                Coord { x: 1.0, y: 3.0 },
                Coord { x: 1.0, y: 1.0 },
            ]),
            vec![],
        );
        let merged = merge_polygons(&[a, b]).expect("merge");
        // Overlapping squares union into exactly one polygon.
        assert_eq!(merged.0.len(), 1);

        // Two disjoint squares remain two parts.
        let c = Polygon::new(
            LineString::from(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        );
        let d = Polygon::new(
            LineString::from(vec![
                Coord { x: 5.0, y: 5.0 },
                Coord { x: 6.0, y: 5.0 },
                Coord { x: 6.0, y: 6.0 },
                Coord { x: 5.0, y: 6.0 },
                Coord { x: 5.0, y: 5.0 },
            ]),
            vec![],
        );
        let disjoint = merge_polygons(&[c, d]).expect("merge disjoint");
        assert_eq!(disjoint.0.len(), 2);
    }

    #[test]
    fn test_calculate_polygon_area() {
        let polygon = Polygon::new(
            LineString::from(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        );

        let area = calculate_polygon_area(&polygon);
        assert!((area - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_export_detections_geojson() {
        use crate::detection::{BoundingBox, Detection, GeoBoundingBox};
        use std::env;

        let temp_dir = env::temp_dir();
        let output_path = temp_dir.join("test_detections.geojson");

        let detections = vec![GeoDetection {
            detection: Detection {
                bbox: BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                class_id: 0,
                class_label: Some("test".to_string()),
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            geo_bbox: GeoBoundingBox {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 10.0,
                max_y: 10.0,
            },
        }];

        let result = export_detections_geojson(&detections, &output_path);
        assert!(result.is_ok());

        // Clean up
        let _ = std::fs::remove_file(output_path);
    }
}
