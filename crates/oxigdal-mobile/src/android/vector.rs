//! Android-specific vector operations.
//!
//! Provides Google Maps integration and Android location services support.
//! Includes geometry simplification, GeoJSON/KML handling, and point clustering.

#![cfg(feature = "android")]

use crate::check_null;
use crate::ffi::error::set_last_error;
use crate::ffi::types::*;
use crate::ffi::vector::layer::LayerHandle;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_void};

// ============================================================================
// Internal Types and Structures
// ============================================================================

/// Internal representation of a 2D point for geometry operations.
#[derive(Debug, Clone, Copy)]
pub struct Point2D {
    x: f64,
    y: f64,
}

/// Internal representation of a geometry for processing.
#[derive(Debug, Clone)]
pub enum Geometry {
    /// A single point geometry
    Point(Point2D),
    /// A line string geometry
    LineString(Vec<Point2D>),
    /// A polygon geometry
    Polygon(Vec<Vec<Point2D>>),
    /// Multiple points
    MultiPoint(Vec<Point2D>),
    /// Multiple line strings
    MultiLineString(Vec<Vec<Point2D>>),
    /// Multiple polygons
    MultiPolygon(Vec<Vec<Vec<Point2D>>>),
}

/// Cluster of points for map marker clustering.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PointCluster {
    centroid: Point2D,
    points: Vec<Point2D>,
    count: usize,
}

/// Android Path command for Canvas drawing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum AndroidPathCommand {
    /// Move to point (x, y)
    MoveTo = 0,
    /// Line to point (x, y)
    LineTo = 1,
    /// Close the path
    Close = 2,
}

/// Android Path data structure.
#[repr(C)]
pub struct AndroidPath {
    /// Command array
    commands: *mut AndroidPathCommand,
    /// X coordinates
    x_coords: *mut c_double,
    /// Y coordinates
    y_coords: *mut c_double,
    /// Number of commands
    command_count: c_int,
    /// Capacity
    capacity: c_int,
}

/// GeoJSON layer structure for Google Maps.
#[repr(C)]
pub struct AndroidGeoJsonLayer {
    /// GeoJSON string data
    geojson_data: *mut c_char,
    /// Length of data
    data_length: usize,
    /// Number of features
    feature_count: c_int,
}

/// Internal layer extension with geometry data for Android operations.
pub struct ExtendedLayerHandle {
    /// Base layer handle
    pub base: LayerHandle,
    /// Stored geometries for the layer
    pub geometries: Vec<Geometry>,
    /// Bounding box
    pub bbox: Option<(f64, f64, f64, f64)>,
}

// ============================================================================
// Douglas-Peucker Simplification Algorithm
// ============================================================================

/// Calculates perpendicular distance from a point to a line segment.
fn perpendicular_distance(point: &Point2D, line_start: &Point2D, line_end: &Point2D) -> f64 {
    let dx = line_end.x - line_start.x;
    let dy = line_end.y - line_start.y;

    // Handle degenerate case where line is actually a point
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        let pdx = point.x - line_start.x;
        let pdy = point.y - line_start.y;
        return (pdx * pdx + pdy * pdy).sqrt();
    }

    let line_length_sq = dx * dx + dy * dy;
    let t = ((point.x - line_start.x) * dx + (point.y - line_start.y) * dy) / line_length_sq;

    // Clamp t to [0, 1] for segment distance
    let t_clamped = t.clamp(0.0, 1.0);

    let proj_x = line_start.x + t_clamped * dx;
    let proj_y = line_start.y + t_clamped * dy;

    let dist_x = point.x - proj_x;
    let dist_y = point.y - proj_y;

    (dist_x * dist_x + dist_y * dist_y).sqrt()
}

/// Douglas-Peucker line simplification algorithm.
///
/// Recursively simplifies a polyline by removing points that are within
/// the specified tolerance of the simplified line.
fn douglas_peucker(points: &[Point2D], epsilon: f64) -> Vec<Point2D> {
    if points.len() < 3 {
        return points.to_vec();
    }

    // Find the point with maximum distance
    let mut max_distance = 0.0;
    let mut max_index = 0;

    let first = &points[0];
    let last = &points[points.len() - 1];

    for (i, point) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let distance = perpendicular_distance(point, first, last);
        if distance > max_distance {
            max_distance = distance;
            max_index = i;
        }
    }

    // If max distance exceeds epsilon, recursively simplify
    if max_distance > epsilon {
        // Recursively simplify both halves
        let mut left = douglas_peucker(&points[..=max_index], epsilon);
        let right = douglas_peucker(&points[max_index..], epsilon);

        // Remove last point of left (it's duplicated as first of right)
        left.pop();
        left.extend(right);
        left
    } else {
        // All points are within tolerance, keep only endpoints
        vec![*first, *last]
    }
}

/// Simplifies a polygon using Douglas-Peucker on each ring.
fn simplify_polygon(rings: &[Vec<Point2D>], epsilon: f64) -> Vec<Vec<Point2D>> {
    rings
        .iter()
        .map(|ring| {
            let simplified = douglas_peucker(ring, epsilon);
            // Ensure ring is closed and has at least 4 points
            if simplified.len() < 4 {
                ring.clone()
            } else {
                simplified
            }
        })
        .collect()
}

/// Calculates simplification tolerance based on Google Maps zoom level.
///
/// Higher zoom = smaller tolerance (more detail preserved)
/// Lower zoom = larger tolerance (more simplification)
fn zoom_to_tolerance(zoom: c_int) -> f64 {
    // Google Maps zoom levels: 0 (whole world) to 21 (street level)
    // At zoom 0, ~156,543 meters per pixel
    // Each zoom level halves the meters per pixel
    let meters_per_pixel = 156543.0 / (1 << zoom) as f64;

    // Convert to degrees (approximately, at equator)
    // 1 degree ≈ 111,319 meters
    meters_per_pixel / 111319.0
}

// ============================================================================
// GeoJSON Parsing and Generation
// ============================================================================

/// Parses GeoJSON coordinates array into Point2D.
fn parse_geojson_point(coords: &serde_json::Value) -> Option<Point2D> {
    let arr = coords.as_array()?;
    if arr.len() >= 2 {
        Some(Point2D {
            x: arr.first()?.as_f64()?,
            y: arr.get(1)?.as_f64()?,
        })
    } else {
        None
    }
}

/// Parses GeoJSON coordinates array into line string.
fn parse_geojson_line_string(coords: &serde_json::Value) -> Option<Vec<Point2D>> {
    let arr = coords.as_array()?;
    arr.iter().map(parse_geojson_point).collect()
}

/// Parses GeoJSON coordinates array into polygon rings.
fn parse_geojson_polygon(coords: &serde_json::Value) -> Option<Vec<Vec<Point2D>>> {
    let arr = coords.as_array()?;
    arr.iter().map(parse_geojson_line_string).collect()
}

/// Parses a GeoJSON geometry object.
fn parse_geojson_geometry(geometry: &serde_json::Value) -> Option<Geometry> {
    let geom_type = geometry.get("type")?.as_str()?;
    let coords = geometry.get("coordinates")?;

    match geom_type {
        "Point" => Some(Geometry::Point(parse_geojson_point(coords)?)),
        "LineString" => Some(Geometry::LineString(parse_geojson_line_string(coords)?)),
        "Polygon" => Some(Geometry::Polygon(parse_geojson_polygon(coords)?)),
        "MultiPoint" => {
            let points: Option<Vec<_>> =
                coords.as_array()?.iter().map(parse_geojson_point).collect();
            Some(Geometry::MultiPoint(points?))
        }
        "MultiLineString" => {
            let lines: Option<Vec<_>> = coords
                .as_array()?
                .iter()
                .map(parse_geojson_line_string)
                .collect();
            Some(Geometry::MultiLineString(lines?))
        }
        "MultiPolygon" => {
            let polygons: Option<Vec<_>> = coords
                .as_array()?
                .iter()
                .map(parse_geojson_polygon)
                .collect();
            Some(Geometry::MultiPolygon(polygons?))
        }
        _ => None,
    }
}

/// Converts Point2D to GeoJSON coordinates.
fn point_to_geojson_coords(point: &Point2D) -> serde_json::Value {
    serde_json::json!([point.x, point.y])
}

/// Converts line string to GeoJSON coordinates.
fn line_string_to_geojson_coords(points: &[Point2D]) -> serde_json::Value {
    serde_json::Value::Array(points.iter().map(point_to_geojson_coords).collect())
}

/// Converts polygon to GeoJSON coordinates.
fn polygon_to_geojson_coords(rings: &[Vec<Point2D>]) -> serde_json::Value {
    serde_json::Value::Array(
        rings
            .iter()
            .map(|ring| line_string_to_geojson_coords(ring))
            .collect(),
    )
}

/// Converts a Geometry to GeoJSON.
fn geometry_to_geojson(geometry: &Geometry) -> serde_json::Value {
    match geometry {
        Geometry::Point(p) => serde_json::json!({
            "type": "Point",
            "coordinates": point_to_geojson_coords(p)
        }),
        Geometry::LineString(points) => serde_json::json!({
            "type": "LineString",
            "coordinates": line_string_to_geojson_coords(points)
        }),
        Geometry::Polygon(rings) => serde_json::json!({
            "type": "Polygon",
            "coordinates": polygon_to_geojson_coords(rings)
        }),
        Geometry::MultiPoint(points) => serde_json::json!({
            "type": "MultiPoint",
            "coordinates": serde_json::Value::Array(points.iter().map(point_to_geojson_coords).collect())
        }),
        Geometry::MultiLineString(lines) => serde_json::json!({
            "type": "MultiLineString",
            "coordinates": serde_json::Value::Array(lines.iter().map(|l| line_string_to_geojson_coords(l)).collect())
        }),
        Geometry::MultiPolygon(polygons) => serde_json::json!({
            "type": "MultiPolygon",
            "coordinates": serde_json::Value::Array(polygons.iter().map(|p| polygon_to_geojson_coords(p)).collect())
        }),
    }
}

// ============================================================================
// KML Generation
// ============================================================================

/// Escapes XML special characters.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Converts Point2D to KML coordinate string (lon,lat,0).
fn point_to_kml_coords(point: &Point2D) -> String {
    format!("{},{},0", point.x, point.y)
}

/// Converts a Geometry to KML Placemark element.
fn geometry_to_kml_placemark(geometry: &Geometry, name: &str, index: usize) -> String {
    let escaped_name = escape_xml(name);
    let geometry_kml = match geometry {
        Geometry::Point(p) => format!(
            "<Point><coordinates>{}</coordinates></Point>",
            point_to_kml_coords(p)
        ),
        Geometry::LineString(points) => {
            let coords: Vec<_> = points.iter().map(point_to_kml_coords).collect();
            format!(
                "<LineString><coordinates>{}</coordinates></LineString>",
                coords.join(" ")
            )
        }
        Geometry::Polygon(rings) => {
            let mut result = String::from("<Polygon>");
            for (i, ring) in rings.iter().enumerate() {
                let coords: Vec<_> = ring.iter().map(point_to_kml_coords).collect();
                let ring_type = if i == 0 {
                    "outerBoundaryIs"
                } else {
                    "innerBoundaryIs"
                };
                result.push_str(&format!(
                    "<{}><LinearRing><coordinates>{}</coordinates></LinearRing></{}>",
                    ring_type,
                    coords.join(" "),
                    ring_type
                ));
            }
            result.push_str("</Polygon>");
            result
        }
        Geometry::MultiPoint(points) => {
            let mut result = String::from("<MultiGeometry>");
            for point in points {
                result.push_str(&format!(
                    "<Point><coordinates>{}</coordinates></Point>",
                    point_to_kml_coords(point)
                ));
            }
            result.push_str("</MultiGeometry>");
            result
        }
        Geometry::MultiLineString(lines) => {
            let mut result = String::from("<MultiGeometry>");
            for line in lines {
                let coords: Vec<_> = line.iter().map(point_to_kml_coords).collect();
                result.push_str(&format!(
                    "<LineString><coordinates>{}</coordinates></LineString>",
                    coords.join(" ")
                ));
            }
            result.push_str("</MultiGeometry>");
            result
        }
        Geometry::MultiPolygon(polygons) => {
            let mut result = String::from("<MultiGeometry>");
            for rings in polygons {
                result.push_str("<Polygon>");
                for (i, ring) in rings.iter().enumerate() {
                    let coords: Vec<_> = ring.iter().map(point_to_kml_coords).collect();
                    let ring_type = if i == 0 {
                        "outerBoundaryIs"
                    } else {
                        "innerBoundaryIs"
                    };
                    result.push_str(&format!(
                        "<{}><LinearRing><coordinates>{}</coordinates></LinearRing></{}>",
                        ring_type,
                        coords.join(" "),
                        ring_type
                    ));
                }
                result.push_str("</Polygon>");
            }
            result.push_str("</MultiGeometry>");
            result
        }
    };

    format!(
        "<Placemark><name>{} {}</name>{}</Placemark>",
        escaped_name,
        index + 1,
        geometry_kml
    )
}

/// Generates a complete KML document from geometries.
fn geometries_to_kml(geometries: &[Geometry], layer_name: &str) -> String {
    let mut kml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    kml.push_str("<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n");
    kml.push_str("<Document>\n");
    kml.push_str(&format!("<name>{}</name>\n", escape_xml(layer_name)));

    for (i, geometry) in geometries.iter().enumerate() {
        kml.push_str(&geometry_to_kml_placemark(geometry, "Feature", i));
        kml.push('\n');
    }

    kml.push_str("</Document>\n");
    kml.push_str("</kml>");
    kml
}

// ============================================================================
// Point Clustering Algorithm
// ============================================================================

/// Grid-based point clustering algorithm.
///
/// Clusters points that fall within the same grid cell based on
/// the cluster distance in screen pixels at the given zoom level.
fn cluster_points_grid(
    points: &[Point2D],
    zoom: c_int,
    cluster_distance: c_int,
) -> Vec<PointCluster> {
    if points.is_empty() {
        return Vec::new();
    }

    // Calculate grid cell size based on zoom and cluster distance
    // At zoom 0, ~156,543 meters per pixel
    let meters_per_pixel = 156543.0 / (1 << zoom) as f64;
    let cell_size_meters = cluster_distance as f64 * meters_per_pixel;
    // Convert to degrees (approximately)
    let cell_size_degrees = cell_size_meters / 111319.0;

    // Avoid division by zero
    if cell_size_degrees < f64::EPSILON {
        return points
            .iter()
            .map(|p| PointCluster {
                centroid: *p,
                points: vec![*p],
                count: 1,
            })
            .collect();
    }

    // Create grid cells
    let mut grid: std::collections::HashMap<(i64, i64), Vec<Point2D>> =
        std::collections::HashMap::new();

    for point in points {
        let cell_x = (point.x / cell_size_degrees).floor() as i64;
        let cell_y = (point.y / cell_size_degrees).floor() as i64;
        grid.entry((cell_x, cell_y)).or_default().push(*point);
    }

    // Convert grid cells to clusters
    grid.into_values()
        .map(|cell_points| {
            let count = cell_points.len();
            let sum_x: f64 = cell_points.iter().map(|p| p.x).sum();
            let sum_y: f64 = cell_points.iter().map(|p| p.y).sum();
            PointCluster {
                centroid: Point2D {
                    x: sum_x / count as f64,
                    y: sum_y / count as f64,
                },
                points: cell_points,
                count,
            }
        })
        .collect()
}

/// Extracts all point coordinates from a geometry.
fn extract_points_from_geometry(geometry: &Geometry) -> Vec<Point2D> {
    match geometry {
        Geometry::Point(p) => vec![*p],
        Geometry::MultiPoint(points) => points.clone(),
        Geometry::LineString(points) => points.clone(),
        Geometry::Polygon(rings) => rings.iter().flatten().copied().collect(),
        Geometry::MultiLineString(lines) => lines.iter().flatten().copied().collect(),
        Geometry::MultiPolygon(polygons) => polygons.iter().flatten().flatten().copied().collect(),
    }
}

// ============================================================================
// Android Path Conversion
// ============================================================================

/// Converts a geometry to Android Path commands.
fn geometry_to_android_path(geometry: &Geometry) -> Option<AndroidPath> {
    let mut commands = Vec::new();
    let mut x_coords = Vec::new();
    let mut y_coords = Vec::new();

    match geometry {
        Geometry::Point(p) => {
            // Single point - just a move
            commands.push(AndroidPathCommand::MoveTo);
            x_coords.push(p.x);
            y_coords.push(p.y);
        }
        Geometry::LineString(points) => {
            if points.is_empty() {
                return None;
            }
            // Move to first point
            commands.push(AndroidPathCommand::MoveTo);
            x_coords.push(points[0].x);
            y_coords.push(points[0].y);
            // Line to remaining points
            for point in points.iter().skip(1) {
                commands.push(AndroidPathCommand::LineTo);
                x_coords.push(point.x);
                y_coords.push(point.y);
            }
        }
        Geometry::Polygon(rings) => {
            for ring in rings {
                if ring.is_empty() {
                    continue;
                }
                // Move to first point of ring
                commands.push(AndroidPathCommand::MoveTo);
                x_coords.push(ring[0].x);
                y_coords.push(ring[0].y);
                // Line to remaining points
                for point in ring.iter().skip(1) {
                    commands.push(AndroidPathCommand::LineTo);
                    x_coords.push(point.x);
                    y_coords.push(point.y);
                }
                // Close the ring
                commands.push(AndroidPathCommand::Close);
                x_coords.push(0.0); // Placeholder for close command
                y_coords.push(0.0);
            }
        }
        Geometry::MultiPoint(points) => {
            for p in points {
                commands.push(AndroidPathCommand::MoveTo);
                x_coords.push(p.x);
                y_coords.push(p.y);
            }
        }
        Geometry::MultiLineString(lines) => {
            for line in lines {
                if line.is_empty() {
                    continue;
                }
                commands.push(AndroidPathCommand::MoveTo);
                x_coords.push(line[0].x);
                y_coords.push(line[0].y);
                for point in line.iter().skip(1) {
                    commands.push(AndroidPathCommand::LineTo);
                    x_coords.push(point.x);
                    y_coords.push(point.y);
                }
            }
        }
        Geometry::MultiPolygon(polygons) => {
            for rings in polygons {
                for ring in rings {
                    if ring.is_empty() {
                        continue;
                    }
                    commands.push(AndroidPathCommand::MoveTo);
                    x_coords.push(ring[0].x);
                    y_coords.push(ring[0].y);
                    for point in ring.iter().skip(1) {
                        commands.push(AndroidPathCommand::LineTo);
                        x_coords.push(point.x);
                        y_coords.push(point.y);
                    }
                    commands.push(AndroidPathCommand::Close);
                    x_coords.push(0.0);
                    y_coords.push(0.0);
                }
            }
        }
    }

    if commands.is_empty() {
        return None;
    }

    let command_count = commands.len() as c_int;

    // Allocate and copy commands
    let commands_ptr = {
        let mut boxed = commands.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        std::mem::forget(boxed);
        ptr
    };

    let x_coords_ptr = {
        let mut boxed = x_coords.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        std::mem::forget(boxed);
        ptr
    };

    let y_coords_ptr = {
        let mut boxed = y_coords.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        std::mem::forget(boxed);
        ptr
    };

    Some(AndroidPath {
        commands: commands_ptr,
        x_coords: x_coords_ptr,
        y_coords: y_coords_ptr,
        command_count,
        capacity: command_count,
    })
}

// ============================================================================
// FFI Functions
// ============================================================================

/// Converts vector layer to Google Maps markers.
///
/// # Parameters
/// - `layer`: Vector layer handle
/// - `out_coords`: Array for coordinate pairs (lat, lon)
/// - `max_coords`: Maximum coordinates to return
///
/// # Returns
/// Number of coordinates written, or -1 on error
///
/// # Safety
/// - layer must be valid
/// - out_coords must be pre-allocated (size >= max_coords * 2)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_layer_to_markers(
    layer: *const OxiGdalLayer,
    out_coords: *mut c_double,
    max_coords: c_int,
) -> c_int {
    if layer.is_null() {
        set_last_error("Null layer pointer".to_string());
        return -1;
    }

    if out_coords.is_null() {
        set_last_error("Null output coordinates pointer".to_string());
        return -1;
    }

    if max_coords <= 0 {
        set_last_error("Invalid max_coords value".to_string());
        return -1;
    }

    // Try to cast to extended layer handle
    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    let mut coord_count = 0;
    let max = max_coords as usize;

    for geometry in &extended.geometries {
        let points = extract_points_from_geometry(geometry);
        for point in points {
            if coord_count >= max {
                break;
            }
            // Store as (lat, lon) for Android - note: y is lat, x is lon
            unsafe {
                *out_coords.add(coord_count * 2) = point.y; // latitude
                *out_coords.add(coord_count * 2 + 1) = point.x; // longitude
            }
            coord_count += 1;
        }
        if coord_count >= max {
            break;
        }
    }

    coord_count as c_int
}

/// Checks if point is within Android map visible bounds.
///
/// # Returns
/// - 1 if visible
/// - 0 if not visible or on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_point_in_bounds(
    point: *const OxiGdalPoint,
    bbox: *const OxiGdalBbox,
) -> c_int {
    if point.is_null() || bbox.is_null() {
        return 0;
    }

    let pt = unsafe { &*point };
    let bb = unsafe { &*bbox };

    if pt.x >= bb.min_x && pt.x <= bb.max_x && pt.y >= bb.min_y && pt.y <= bb.max_y {
        1
    } else {
        0
    }
}

/// Loads GeoJSON for Android Maps.
///
/// # Safety
/// - geojson_path must be valid
/// - out_layer must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_load_geojson(
    geojson_path: *const c_char,
    out_layer: *mut *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    check_null!(geojson_path, "geojson_path");
    check_null!(out_layer, "out_layer");

    // Parse path
    let path_str = unsafe {
        match CStr::from_ptr(geojson_path).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("Invalid UTF-8 in path".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    // Read file content
    let content = match std::fs::read_to_string(path_str) {
        Ok(c) => c,
        Err(e) => {
            set_last_error(format!("Failed to read GeoJSON file: {}", e));
            return OxiGdalErrorCode::IoError;
        }
    };

    // Parse GeoJSON
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(e) => {
            set_last_error(format!("Failed to parse GeoJSON: {}", e));
            return OxiGdalErrorCode::InvalidArgument;
        }
    };

    // Extract features
    let mut geometries = Vec::new();
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    let features = match json.get("type").and_then(|t| t.as_str()) {
        Some("FeatureCollection") => json.get("features").and_then(|f| f.as_array()),
        Some("Feature") => {
            // Single feature - wrap in array
            None
        }
        _ => None,
    };

    if let Some(features) = features {
        for feature in features {
            if let Some(geometry) = feature.get("geometry") {
                if let Some(geom) = parse_geojson_geometry(geometry) {
                    // Update bbox
                    let points = extract_points_from_geometry(&geom);
                    for p in &points {
                        min_x = min_x.min(p.x);
                        min_y = min_y.min(p.y);
                        max_x = max_x.max(p.x);
                        max_y = max_y.max(p.y);
                    }
                    geometries.push(geom);
                }
            }
        }
    } else if json.get("type").and_then(|t| t.as_str()) == Some("Feature") {
        // Handle single Feature
        if let Some(geometry) = json.get("geometry") {
            if let Some(geom) = parse_geojson_geometry(geometry) {
                let points = extract_points_from_geometry(&geom);
                for p in &points {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                geometries.push(geom);
            }
        }
    } else if let Some(geom) = parse_geojson_geometry(&json) {
        // Handle bare geometry
        let points = extract_points_from_geometry(&geom);
        for p in &points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        geometries.push(geom);
    }

    let bbox = if geometries.is_empty() {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    };

    // Create extended layer handle
    let layer_name = std::path::Path::new(path_str)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("geojson_layer")
        .to_string();

    let handle = Box::new(ExtendedLayerHandle {
        base: LayerHandle::new(layer_name, "Unknown".to_string(), 4326),
        geometries,
        bbox,
    });

    unsafe {
        *out_layer = Box::into_raw(handle) as *mut OxiGdalLayer;
    }

    OxiGdalErrorCode::Success
}

/// Creates Google Maps GeoJson layer from vector data.
///
/// # Safety
/// - layer must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_create_geojson_layer(
    layer: *const OxiGdalLayer,
) -> *mut c_void {
    if layer.is_null() {
        set_last_error("Null layer pointer".to_string());
        return std::ptr::null_mut();
    }

    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    // Build GeoJSON FeatureCollection
    let features: Vec<serde_json::Value> = extended
        .geometries
        .iter()
        .enumerate()
        .map(|(i, geom)| {
            serde_json::json!({
                "type": "Feature",
                "id": i,
                "geometry": geometry_to_geojson(geom),
                "properties": {}
            })
        })
        .collect();

    let feature_collection = serde_json::json!({
        "type": "FeatureCollection",
        "features": features
    });

    let geojson_str = match serde_json::to_string(&feature_collection) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Failed to serialize GeoJSON: {}", e));
            return std::ptr::null_mut();
        }
    };

    let data_length = geojson_str.len();
    let feature_count = features.len() as c_int;

    // Create C string
    let geojson_cstr = match std::ffi::CString::new(geojson_str) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Failed to create C string: {}", e));
            return std::ptr::null_mut();
        }
    };

    let layer_struct = Box::new(AndroidGeoJsonLayer {
        geojson_data: geojson_cstr.into_raw(),
        data_length,
        feature_count,
    });

    Box::into_raw(layer_struct) as *mut c_void
}

/// Frees an Android GeoJSON layer.
///
/// # Safety
/// - layer must be a valid AndroidGeoJsonLayer pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_free_geojson_layer(layer: *mut c_void) {
    if layer.is_null() {
        return;
    }

    unsafe {
        let layer_struct = Box::from_raw(layer as *mut AndroidGeoJsonLayer);
        if !layer_struct.geojson_data.is_null() {
            drop(std::ffi::CString::from_raw(layer_struct.geojson_data));
        }
    }
}

/// Simplifies geometry for Android display at zoom level.
///
/// Uses Douglas-Peucker algorithm with tolerance based on zoom level.
///
/// # Parameters
/// - `layer`: Input layer
/// - `zoom`: Google Maps zoom level (3-21)
/// - `out_layer`: Simplified output layer
///
/// # Safety
/// - layer and out_layer must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_simplify_for_zoom(
    layer: *const OxiGdalLayer,
    zoom: c_int,
    out_layer: *mut *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(out_layer, "out_layer");

    // Validate zoom level
    let zoom_clamped = zoom.clamp(0, 21);
    let tolerance = zoom_to_tolerance(zoom_clamped);

    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    // Simplify all geometries
    let simplified_geometries: Vec<Geometry> = extended
        .geometries
        .iter()
        .map(|geom| match geom {
            Geometry::Point(p) => Geometry::Point(*p),
            Geometry::LineString(points) => {
                Geometry::LineString(douglas_peucker(points, tolerance))
            }
            Geometry::Polygon(rings) => Geometry::Polygon(simplify_polygon(rings, tolerance)),
            Geometry::MultiPoint(points) => Geometry::MultiPoint(points.clone()),
            Geometry::MultiLineString(lines) => Geometry::MultiLineString(
                lines
                    .iter()
                    .map(|line| douglas_peucker(line, tolerance))
                    .collect(),
            ),
            Geometry::MultiPolygon(polygons) => Geometry::MultiPolygon(
                polygons
                    .iter()
                    .map(|rings| simplify_polygon(rings, tolerance))
                    .collect(),
            ),
        })
        .collect();

    // Recalculate bounding box
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for geom in &simplified_geometries {
        let points = extract_points_from_geometry(geom);
        for p in &points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    let bbox = if simplified_geometries.is_empty() {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    };

    let simplified_name = format!("{}_simplified_z{}", extended.base.name(), zoom_clamped);

    let handle = Box::new(ExtendedLayerHandle {
        base: LayerHandle::new(simplified_name, "Unknown".to_string(), 4326),
        geometries: simplified_geometries,
        bbox,
    });

    unsafe {
        *out_layer = Box::into_raw(handle) as *mut OxiGdalLayer;
    }

    OxiGdalErrorCode::Success
}

/// Converts vector to KML for Google Earth.
///
/// # Parameters
/// - `layer`: Vector layer
/// - `output_path`: Path to KML file
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_export_kml(
    layer: *const OxiGdalLayer,
    output_path: *const c_char,
) -> OxiGdalErrorCode {
    check_null!(layer, "layer");
    check_null!(output_path, "output_path");

    // Parse output path
    let path_str = unsafe {
        match CStr::from_ptr(output_path).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("Invalid UTF-8 in output path".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    // Generate KML content
    let kml_content = geometries_to_kml(&extended.geometries, extended.base.name());

    // Write to file
    match std::fs::write(path_str, &kml_content) {
        Ok(()) => OxiGdalErrorCode::Success,
        Err(e) => {
            set_last_error(format!("Failed to write KML file: {}", e));
            OxiGdalErrorCode::IoError
        }
    }
}

/// Creates clustering for Android map markers.
///
/// Uses grid-based clustering algorithm optimized for mobile performance.
///
/// # Parameters
/// - `layer`: Vector layer with point features
/// - `zoom`: Current zoom level
/// - `cluster_distance`: Clustering distance in pixels
///
/// # Returns
/// Number of clusters created, or -1 on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_cluster_points(
    layer: *const OxiGdalLayer,
    zoom: c_int,
    cluster_distance: c_int,
) -> c_int {
    if layer.is_null() {
        set_last_error("Null layer pointer".to_string());
        return -1;
    }

    if !(0..=21).contains(&zoom) {
        set_last_error("Zoom level must be between 0 and 21".to_string());
        return -1;
    }

    if cluster_distance <= 0 {
        set_last_error("Cluster distance must be positive".to_string());
        return -1;
    }

    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    // Extract all points from all geometries
    let mut all_points = Vec::new();
    for geometry in &extended.geometries {
        all_points.extend(extract_points_from_geometry(geometry));
    }

    // Perform clustering
    let clusters = cluster_points_grid(&all_points, zoom, cluster_distance);

    clusters.len() as c_int
}

/// Gets cluster centroids after clustering.
///
/// # Parameters
/// - `layer`: Vector layer (must have been clustered)
/// - `zoom`: Zoom level used for clustering
/// - `cluster_distance`: Cluster distance used
/// - `out_coords`: Output array for cluster centroids (lat, lon pairs)
/// - `max_clusters`: Maximum number of clusters to return
///
/// # Returns
/// Number of clusters written, or -1 on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_get_cluster_centroids(
    layer: *const OxiGdalLayer,
    zoom: c_int,
    cluster_distance: c_int,
    out_coords: *mut c_double,
    max_clusters: c_int,
) -> c_int {
    if layer.is_null() || out_coords.is_null() {
        set_last_error("Null pointer provided".to_string());
        return -1;
    }

    if max_clusters <= 0 {
        return 0;
    }

    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    // Extract and cluster points
    let mut all_points = Vec::new();
    for geometry in &extended.geometries {
        all_points.extend(extract_points_from_geometry(geometry));
    }

    let clusters = cluster_points_grid(&all_points, zoom, cluster_distance);

    let count = clusters.len().min(max_clusters as usize);

    for (i, cluster) in clusters.iter().take(count).enumerate() {
        unsafe {
            *out_coords.add(i * 2) = cluster.centroid.y; // latitude
            *out_coords.add(i * 2 + 1) = cluster.centroid.x; // longitude
        }
    }

    count as c_int
}

/// Converts geometry to Android Path for Canvas drawing.
///
/// # Safety
/// - feature must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_feature_to_path(
    feature: *const OxiGdalFeature,
) -> *mut c_void {
    if feature.is_null() {
        set_last_error("Null feature pointer".to_string());
        return std::ptr::null_mut();
    }

    // For now, create a simple point geometry as placeholder
    // In a full implementation, the feature would contain geometry data
    let geometry = Geometry::Point(Point2D { x: 0.0, y: 0.0 });

    match geometry_to_android_path(&geometry) {
        Some(path) => Box::into_raw(Box::new(path)) as *mut c_void,
        None => std::ptr::null_mut(),
    }
}

/// Converts a layer geometry to Android Path for Canvas drawing.
///
/// # Parameters
/// - `layer`: Layer containing geometries
/// - `feature_index`: Index of the feature/geometry to convert
///
/// # Returns
/// Pointer to AndroidPath structure, or null on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_layer_geometry_to_path(
    layer: *const OxiGdalLayer,
    feature_index: c_int,
) -> *mut c_void {
    if layer.is_null() {
        set_last_error("Null layer pointer".to_string());
        return std::ptr::null_mut();
    }

    if feature_index < 0 {
        set_last_error("Invalid feature index".to_string());
        return std::ptr::null_mut();
    }

    let extended = unsafe { &*(layer as *const ExtendedLayerHandle) };

    if (feature_index as usize) >= extended.geometries.len() {
        set_last_error("Feature index out of bounds".to_string());
        return std::ptr::null_mut();
    }

    let geometry = &extended.geometries[feature_index as usize];

    match geometry_to_android_path(geometry) {
        Some(path) => Box::into_raw(Box::new(path)) as *mut c_void,
        None => {
            set_last_error("Failed to convert geometry to path".to_string());
            std::ptr::null_mut()
        }
    }
}

/// Frees an Android Path structure.
///
/// # Safety
/// - path must be a valid AndroidPath pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_free_path(path: *mut c_void) {
    if path.is_null() {
        return;
    }

    unsafe {
        let path_struct = Box::from_raw(path as *mut AndroidPath);

        // Free the arrays
        if !path_struct.commands.is_null() {
            let count = path_struct.command_count as usize;
            let _ = Vec::from_raw_parts(path_struct.commands, count, count);
        }
        if !path_struct.x_coords.is_null() {
            let count = path_struct.command_count as usize;
            let _ = Vec::from_raw_parts(path_struct.x_coords, count, count);
        }
        if !path_struct.y_coords.is_null() {
            let count = path_struct.command_count as usize;
            let _ = Vec::from_raw_parts(path_struct.y_coords, count, count);
        }
    }
}

/// Closes an extended layer and frees all resources.
///
/// # Safety
/// - layer must be a valid extended layer handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_layer_close(layer: *mut OxiGdalLayer) -> OxiGdalErrorCode {
    if layer.is_null() {
        set_last_error("Null layer pointer".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    unsafe {
        drop(Box::from_raw(layer as *mut ExtendedLayerHandle));
    }

    OxiGdalErrorCode::Success
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_point_in_bounds() {
        let point = OxiGdalPoint {
            x: 139.6917, // Tokyo longitude
            y: 35.6895,  // Tokyo latitude
            z: 0.0,
        };

        let bbox = OxiGdalBbox {
            min_x: 139.0,
            min_y: 35.0,
            max_x: 140.0,
            max_y: 36.0,
        };

        let result = unsafe { oxigdal_android_point_in_bounds(&point, &bbox) };
        assert_eq!(result, 1);

        // Test point outside bounds
        let outside_point = OxiGdalPoint {
            x: 100.0,
            y: 50.0,
            z: 0.0,
        };

        let result = unsafe { oxigdal_android_point_in_bounds(&outside_point, &bbox) };
        assert_eq!(result, 0);
    }

    #[test]
    fn test_douglas_peucker_simple() {
        let points = vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D { x: 1.0, y: 0.1 },
            Point2D { x: 2.0, y: -0.1 },
            Point2D { x: 3.0, y: 5.0 },
            Point2D { x: 4.0, y: 6.0 },
            Point2D { x: 5.0, y: 7.0 },
            Point2D { x: 6.0, y: 8.1 },
            Point2D { x: 7.0, y: 9.0 },
            Point2D { x: 8.0, y: 9.0 },
            Point2D { x: 9.0, y: 9.0 },
        ];

        let simplified = douglas_peucker(&points, 1.0);
        assert!(simplified.len() < points.len());
        assert!(simplified.len() >= 2);
    }

    #[test]
    fn test_douglas_peucker_two_points() {
        let points = vec![Point2D { x: 0.0, y: 0.0 }, Point2D { x: 10.0, y: 10.0 }];

        let simplified = douglas_peucker(&points, 1.0);
        assert_eq!(simplified.len(), 2);
    }

    #[test]
    fn test_douglas_peucker_empty() {
        let points: Vec<Point2D> = vec![];
        let simplified = douglas_peucker(&points, 1.0);
        assert!(simplified.is_empty());
    }

    #[test]
    fn test_zoom_to_tolerance() {
        // Higher zoom = smaller tolerance
        let tol_0 = zoom_to_tolerance(0);
        let tol_10 = zoom_to_tolerance(10);
        let tol_20 = zoom_to_tolerance(20);

        assert!(tol_0 > tol_10);
        assert!(tol_10 > tol_20);
    }

    #[test]
    fn test_perpendicular_distance() {
        let line_start = Point2D { x: 0.0, y: 0.0 };
        let line_end = Point2D { x: 10.0, y: 0.0 };
        let point = Point2D { x: 5.0, y: 5.0 };

        let distance = perpendicular_distance(&point, &line_start, &line_end);
        assert!((distance - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_geojson_point_parsing() {
        let coords = serde_json::json!([139.6917, 35.6895]);
        let point = parse_geojson_point(&coords);

        assert!(point.is_some());
        let p = point.expect("point should exist");
        assert!((p.x - 139.6917).abs() < 0.0001);
        assert!((p.y - 35.6895).abs() < 0.0001);
    }

    #[test]
    fn test_geojson_geometry_parsing() {
        let geometry = serde_json::json!({
            "type": "Point",
            "coordinates": [139.6917, 35.6895]
        });

        let geom = parse_geojson_geometry(&geometry);
        assert!(geom.is_some());

        match geom {
            Some(Geometry::Point(p)) => {
                assert!((p.x - 139.6917).abs() < 0.0001);
            }
            _ => panic!("Expected Point geometry"),
        }
    }

    #[test]
    fn test_geojson_linestring_parsing() {
        let geometry = serde_json::json!({
            "type": "LineString",
            "coordinates": [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]
        });

        let geom = parse_geojson_geometry(&geometry);
        assert!(geom.is_some());

        match geom {
            Some(Geometry::LineString(points)) => {
                assert_eq!(points.len(), 3);
            }
            _ => panic!("Expected LineString geometry"),
        }
    }

    #[test]
    fn test_geometry_to_geojson() {
        let point = Geometry::Point(Point2D { x: 1.0, y: 2.0 });
        let json = geometry_to_geojson(&point);

        assert_eq!(json.get("type").and_then(|t| t.as_str()), Some("Point"));
    }

    #[test]
    fn test_kml_escape_xml() {
        assert_eq!(escape_xml("<test>"), "&lt;test&gt;");
        assert_eq!(escape_xml("test & value"), "test &amp; value");
        assert_eq!(escape_xml("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_kml_generation() {
        let geometries = vec![
            Geometry::Point(Point2D { x: 139.0, y: 35.0 }),
            Geometry::LineString(vec![Point2D { x: 0.0, y: 0.0 }, Point2D { x: 1.0, y: 1.0 }]),
        ];

        let kml = geometries_to_kml(&geometries, "TestLayer");

        assert!(kml.contains("<?xml version=\"1.0\""));
        assert!(kml.contains("<kml"));
        assert!(kml.contains("<Document>"));
        assert!(kml.contains("<name>TestLayer</name>"));
        assert!(kml.contains("<Point>"));
        assert!(kml.contains("<LineString>"));
    }

    #[test]
    fn test_point_clustering_empty() {
        let points: Vec<Point2D> = vec![];
        let clusters = cluster_points_grid(&points, 10, 50);
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_point_clustering_single() {
        let points = vec![Point2D { x: 0.0, y: 0.0 }];
        let clusters = cluster_points_grid(&points, 10, 50);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 1);
    }

    #[test]
    fn test_point_clustering_nearby() {
        let points = vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D {
                x: 0.00001,
                y: 0.00001,
            },
            Point2D {
                x: 0.00002,
                y: 0.00002,
            },
        ];

        // At high zoom with small cluster distance, points should cluster
        let clusters = cluster_points_grid(&points, 5, 100);
        assert!(clusters.len() <= 3);
    }

    #[test]
    fn test_point_clustering_distant() {
        let points = vec![Point2D { x: 0.0, y: 0.0 }, Point2D { x: 100.0, y: 100.0 }];

        // Distant points should not cluster
        let clusters = cluster_points_grid(&points, 15, 50);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_extract_points_from_geometry() {
        let polygon = Geometry::Polygon(vec![vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D { x: 1.0, y: 0.0 },
            Point2D { x: 1.0, y: 1.0 },
            Point2D { x: 0.0, y: 1.0 },
            Point2D { x: 0.0, y: 0.0 },
        ]]);

        let points = extract_points_from_geometry(&polygon);
        assert_eq!(points.len(), 5);
    }

    #[test]
    fn test_geometry_to_android_path_point() {
        let point = Geometry::Point(Point2D { x: 1.0, y: 2.0 });
        let path = geometry_to_android_path(&point);

        assert!(path.is_some());
        let p = path.expect("path should exist");
        assert_eq!(p.command_count, 1);

        // Clean up
        unsafe {
            let _ = Vec::from_raw_parts(p.commands, 1, 1);
            let _ = Vec::from_raw_parts(p.x_coords, 1, 1);
            let _ = Vec::from_raw_parts(p.y_coords, 1, 1);
        }
    }

    #[test]
    fn test_geometry_to_android_path_linestring() {
        let line = Geometry::LineString(vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D { x: 1.0, y: 1.0 },
            Point2D { x: 2.0, y: 2.0 },
        ]);
        let path = geometry_to_android_path(&line);

        assert!(path.is_some());
        let p = path.expect("path should exist");
        assert_eq!(p.command_count, 3); // MoveTo + 2 LineTo

        // Clean up
        unsafe {
            let _ = Vec::from_raw_parts(p.commands, 3, 3);
            let _ = Vec::from_raw_parts(p.x_coords, 3, 3);
            let _ = Vec::from_raw_parts(p.y_coords, 3, 3);
        }
    }

    #[test]
    fn test_geometry_to_android_path_polygon() {
        let polygon = Geometry::Polygon(vec![vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D { x: 1.0, y: 0.0 },
            Point2D { x: 1.0, y: 1.0 },
            Point2D { x: 0.0, y: 0.0 },
        ]]);
        let path = geometry_to_android_path(&polygon);

        assert!(path.is_some());
        let p = path.expect("path should exist");
        // MoveTo + 3 LineTo + Close = 5 commands
        assert_eq!(p.command_count, 5);

        // Clean up
        unsafe {
            let count = p.command_count as usize;
            let _ = Vec::from_raw_parts(p.commands, count, count);
            let _ = Vec::from_raw_parts(p.x_coords, count, count);
            let _ = Vec::from_raw_parts(p.y_coords, count, count);
        }
    }

    #[test]
    fn test_simplify_polygon() {
        let rings = vec![vec![
            Point2D { x: 0.0, y: 0.0 },
            Point2D { x: 0.5, y: 0.01 },
            Point2D { x: 1.0, y: 0.0 },
            Point2D { x: 1.0, y: 1.0 },
            Point2D { x: 0.0, y: 1.0 },
            Point2D { x: 0.0, y: 0.0 },
        ]];

        let simplified = simplify_polygon(&rings, 0.1);
        assert_eq!(simplified.len(), 1);
        // Should have removed the point at (0.5, 0.01) as it's nearly collinear
        assert!(simplified[0].len() <= rings[0].len());
    }

    #[test]
    fn test_geojson_load_nonexistent_file() {
        let path = std::ffi::CString::new("/nonexistent/path/file.geojson").expect("valid string");
        let mut out_layer: *mut OxiGdalLayer = std::ptr::null_mut();

        let result = unsafe { oxigdal_android_load_geojson(path.as_ptr(), &mut out_layer) };

        assert_eq!(result, OxiGdalErrorCode::IoError);
    }

    #[test]
    fn test_cluster_points_invalid_params() {
        let result = unsafe { oxigdal_android_cluster_points(std::ptr::null(), 10, 50) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_layer_to_markers_null_safety() {
        let mut coords = [0.0_f64; 10];
        let result =
            unsafe { oxigdal_android_layer_to_markers(std::ptr::null(), coords.as_mut_ptr(), 5) };
        assert_eq!(result, -1);
    }
}
