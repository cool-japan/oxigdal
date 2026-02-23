//! iOS-specific vector operations.
//!
//! Provides MapKit and CoreLocation integration for vector data.

#![cfg(feature = "ios")]

use crate::ffi::types::*;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::Mutex;

// ============================================================================
// Internal Data Structures
// ============================================================================

/// Internal representation of a geometry for iOS.
#[derive(Debug, Clone)]
pub struct IosGeometry {
    /// Geometry type (Point, LineString, Polygon, etc.)
    pub geometry_type: GeometryType,
    /// Coordinates (lon, lat pairs for 2D)
    pub coordinates: Vec<(f64, f64)>,
    /// Rings for polygons (indices into coordinates)
    pub rings: Vec<usize>,
}

/// Geometry type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum GeometryType {
    /// Unknown type
    Unknown = 0,
    /// Point geometry
    Point = 1,
    /// LineString geometry
    LineString = 2,
    /// Polygon geometry
    Polygon = 3,
    /// MultiPoint geometry
    MultiPoint = 4,
    /// MultiLineString geometry
    MultiLineString = 5,
    /// MultiPolygon geometry
    MultiPolygon = 6,
}

/// Internal feature representation with properties.
#[derive(Debug, Clone)]
pub struct IosFeature {
    /// Feature ID
    pub fid: i64,
    /// Geometry
    pub geometry: Option<IosGeometry>,
    /// Properties as string key-value pairs
    pub properties: HashMap<String, String>,
    /// Bounding box (min_x, min_y, max_x, max_y)
    pub bbox: Option<(f64, f64, f64, f64)>,
}

/// Internal layer representation with features and spatial index.
#[derive(Debug)]
pub struct IosVectorLayer {
    /// Layer name
    pub name: String,
    /// Features in the layer
    pub features: Vec<IosFeature>,
    /// Spatial index (R-tree nodes)
    pub spatial_index: Option<RTreeIndex>,
    /// Layer bounding box
    pub bbox: Option<OxiGdalBbox>,
}

// ============================================================================
// MapKit Overlay Representations
// ============================================================================

/// MapKit overlay data for iOS rendering.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct MapKitOverlayData {
    /// Overlay type
    pub overlay_type: MapKitOverlayType,
    /// Number of coordinates
    pub coord_count: c_int,
    /// Pointer to coordinates (lon, lat pairs)
    pub coordinates: *mut c_double,
    /// Number of rings (for polygons)
    pub ring_count: c_int,
    /// Ring start indices (for polygons)
    pub ring_starts: *mut c_int,
}

/// MapKit overlay types.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapKitOverlayType {
    /// MKPointAnnotation
    Annotation = 0,
    /// MKPolyline
    Polyline = 1,
    /// MKPolygon
    Polygon = 2,
    /// MKCircle
    Circle = 3,
}

// ============================================================================
// R-Tree Spatial Index Implementation
// ============================================================================

/// Rectangle for R-tree bounding boxes.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Rect {
    /// Creates a new rectangle.
    fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Checks if two rectangles intersect.
    fn intersects(&self, other: &Rect) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Computes the area of the rectangle.
    fn area(&self) -> f64 {
        (self.max_x - self.min_x) * (self.max_y - self.min_y)
    }

    /// Returns the union of two rectangles.
    fn union(&self, other: &Rect) -> Rect {
        Rect {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    /// Computes the enlargement needed to include another rectangle.
    fn enlargement(&self, other: &Rect) -> f64 {
        self.union(other).area() - self.area()
    }
}

/// R-tree node.
#[derive(Debug, Clone)]
pub struct RTreeNode {
    /// Bounding box of this node
    bbox: Rect,
    /// Child indices (for internal nodes) or feature indices (for leaf nodes)
    children: Vec<usize>,
    /// Is this a leaf node?
    is_leaf: bool,
}

/// R-tree spatial index.
#[derive(Debug)]
pub struct RTreeIndex {
    /// All nodes in the tree
    nodes: Vec<RTreeNode>,
    /// Root node index
    root: usize,
    /// Maximum entries per node
    max_entries: usize,
    /// Minimum entries per node
    _min_entries: usize,
    /// Feature bounding boxes (indexed by feature index)
    feature_bboxes: Vec<Rect>,
}

impl RTreeIndex {
    /// Creates a new R-tree index.
    fn new() -> Self {
        let root_node = RTreeNode {
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            is_leaf: true,
        };

        Self {
            nodes: vec![root_node],
            root: 0,
            max_entries: 10,
            _min_entries: 4,
            feature_bboxes: Vec::new(),
        }
    }

    /// Inserts a feature with its bounding box.
    fn insert(&mut self, feature_idx: usize, bbox: Rect) {
        self.feature_bboxes.push(bbox);
        let feature_bbox_idx = self.feature_bboxes.len() - 1;

        if self.nodes[self.root].children.is_empty() {
            // First insertion - update root bbox
            self.nodes[self.root].bbox = bbox;
            self.nodes[self.root].children.push(feature_idx);
        } else {
            // Find the best leaf node and insert
            let leaf_idx = self.choose_leaf(self.root, &bbox);
            self.nodes[leaf_idx].children.push(feature_idx);
            self.nodes[leaf_idx].bbox = self.nodes[leaf_idx].bbox.union(&bbox);

            // Handle node overflow
            if self.nodes[leaf_idx].children.len() > self.max_entries {
                self.split_node(leaf_idx);
            }

            // Update root bounding box
            self.update_bbox(self.root);
        }
    }

    /// Chooses the best leaf node for insertion.
    fn choose_leaf(&self, node_idx: usize, bbox: &Rect) -> usize {
        let node = &self.nodes[node_idx];

        if node.is_leaf {
            return node_idx;
        }

        // Find child with minimum enlargement
        let mut best_child = 0;
        let mut min_enlargement = f64::INFINITY;
        let mut min_area = f64::INFINITY;

        for &child_idx in &node.children {
            if child_idx >= self.nodes.len() {
                continue;
            }
            let child_bbox = &self.nodes[child_idx].bbox;
            let enlargement = child_bbox.enlargement(bbox);

            if enlargement < min_enlargement
                || (enlargement == min_enlargement && child_bbox.area() < min_area)
            {
                min_enlargement = enlargement;
                min_area = child_bbox.area();
                best_child = child_idx;
            }
        }

        self.choose_leaf(best_child, bbox)
    }

    /// Splits an overflowed node.
    fn split_node(&mut self, node_idx: usize) {
        // Simple split: divide children into two groups
        let children = std::mem::take(&mut self.nodes[node_idx].children);
        let mid = children.len() / 2;

        let (left_children, right_children) = children.split_at(mid);
        self.nodes[node_idx].children = left_children.to_vec();

        // Create new node with right half
        let new_node = RTreeNode {
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: right_children.to_vec(),
            is_leaf: self.nodes[node_idx].is_leaf,
        };
        let new_node_idx = self.nodes.len();
        self.nodes.push(new_node);

        // Update bounding boxes
        self.update_bbox(node_idx);
        self.update_bbox(new_node_idx);

        // If we split the root, create a new root
        if node_idx == self.root {
            let new_root = RTreeNode {
                bbox: self.nodes[node_idx]
                    .bbox
                    .union(&self.nodes[new_node_idx].bbox),
                children: vec![node_idx, new_node_idx],
                is_leaf: false,
            };
            self.root = self.nodes.len();
            self.nodes.push(new_root);
        }
    }

    /// Updates the bounding box of a node based on its children.
    fn update_bbox(&mut self, node_idx: usize) {
        let node = &self.nodes[node_idx];
        if node.children.is_empty() {
            return;
        }

        let mut bbox = if node.is_leaf {
            // For leaf nodes, children are feature indices
            if let Some(&first_child) = node.children.first() {
                if first_child < self.feature_bboxes.len() {
                    self.feature_bboxes[first_child]
                } else {
                    return;
                }
            } else {
                return;
            }
        } else {
            // For internal nodes, children are node indices
            if let Some(&first_child) = node.children.first() {
                if first_child < self.nodes.len() {
                    self.nodes[first_child].bbox
                } else {
                    return;
                }
            } else {
                return;
            }
        };

        for &child_idx in node.children.iter().skip(1) {
            if node.is_leaf {
                if child_idx < self.feature_bboxes.len() {
                    bbox = bbox.union(&self.feature_bboxes[child_idx]);
                }
            } else if child_idx < self.nodes.len() {
                bbox = bbox.union(&self.nodes[child_idx].bbox);
            }
        }

        self.nodes[node_idx].bbox = bbox;
    }

    /// Searches for features intersecting the given bounding box.
    fn search(&self, bbox: &Rect) -> Vec<usize> {
        let mut results = Vec::new();
        self.search_node(self.root, bbox, &mut results);
        results
    }

    /// Recursively searches a node.
    fn search_node(&self, node_idx: usize, bbox: &Rect, results: &mut Vec<usize>) {
        if node_idx >= self.nodes.len() {
            return;
        }

        let node = &self.nodes[node_idx];
        if !node.bbox.intersects(bbox) {
            return;
        }

        if node.is_leaf {
            for &child_idx in &node.children {
                if child_idx < self.feature_bboxes.len()
                    && self.feature_bboxes[child_idx].intersects(bbox)
                {
                    results.push(child_idx);
                }
            }
        } else {
            for &child_idx in &node.children {
                self.search_node(child_idx, bbox, results);
            }
        }
    }
}

// ============================================================================
// Douglas-Peucker Line Simplification
// ============================================================================

/// Computes the perpendicular distance from a point to a line segment.
fn perpendicular_distance(point: (f64, f64), line_start: (f64, f64), line_end: (f64, f64)) -> f64 {
    let dx = line_end.0 - line_start.0;
    let dy = line_end.1 - line_start.1;

    let line_length_sq = dx * dx + dy * dy;

    if line_length_sq < f64::EPSILON {
        // Line segment is essentially a point
        let pdx = point.0 - line_start.0;
        let pdy = point.1 - line_start.1;
        return (pdx * pdx + pdy * pdy).sqrt();
    }

    // Compute the perpendicular distance
    let t = ((point.0 - line_start.0) * dx + (point.1 - line_start.1) * dy) / line_length_sq;
    let t_clamped = t.clamp(0.0, 1.0);

    let closest_x = line_start.0 + t_clamped * dx;
    let closest_y = line_start.1 + t_clamped * dy;

    let dist_x = point.0 - closest_x;
    let dist_y = point.1 - closest_y;

    (dist_x * dist_x + dist_y * dist_y).sqrt()
}

/// Douglas-Peucker line simplification algorithm.
fn douglas_peucker(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut result = Vec::new();
    douglas_peucker_recursive(points, epsilon, &mut result);

    // Always include the last point
    if let Some(&last) = points.last() {
        result.push(last);
    }

    result
}

/// Recursive helper for Douglas-Peucker algorithm.
fn douglas_peucker_recursive(points: &[(f64, f64)], epsilon: f64, result: &mut Vec<(f64, f64)>) {
    if points.len() < 2 {
        if let Some(&first) = points.first() {
            result.push(first);
        }
        return;
    }

    let first = points[0];
    let last = points[points.len() - 1];

    // Find the point with maximum distance
    let mut max_dist = 0.0;
    let mut max_idx = 0;

    for (i, &point) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let dist = perpendicular_distance(point, first, last);
        if dist > max_dist {
            max_dist = dist;
            max_idx = i;
        }
    }

    if max_dist > epsilon {
        // Recursively simplify both halves
        douglas_peucker_recursive(&points[..=max_idx], epsilon, result);
        douglas_peucker_recursive(&points[max_idx..], epsilon, result);
    } else {
        // Keep only the first point
        result.push(first);
    }
}

/// Calculates simplification tolerance based on zoom level.
fn tolerance_for_zoom(zoom: c_int) -> f64 {
    // At zoom 0, use large tolerance (~1 degree)
    // At zoom 20, use small tolerance (~0.00001 degrees)
    // Tolerance halves with each zoom level
    let base_tolerance = 1.0;
    base_tolerance / (1 << zoom.clamp(0, 20)) as f64
}

/// Simplifies a geometry for display at a given zoom level.
fn simplify_geometry(geometry: &IosGeometry, zoom: c_int) -> IosGeometry {
    let tolerance = tolerance_for_zoom(zoom);

    let simplified_coords = match geometry.geometry_type {
        GeometryType::Point => geometry.coordinates.clone(),
        GeometryType::LineString => douglas_peucker(&geometry.coordinates, tolerance),
        GeometryType::Polygon => {
            // Simplify each ring separately
            let mut result = Vec::new();
            let mut prev_ring_start = 0;
            let mut new_rings = Vec::new();

            for (ring_idx, &ring_end) in geometry.rings.iter().enumerate() {
                let ring_coords = &geometry.coordinates[prev_ring_start..ring_end];
                let simplified_ring = douglas_peucker(ring_coords, tolerance);

                // Ensure ring has at least 4 points (closed polygon)
                if simplified_ring.len() >= 3 {
                    result.extend(simplified_ring);
                    // Close the ring if not already closed
                    if let (Some(&first), Some(&last)) = (result.first(), result.last()) {
                        if (first.0 - last.0).abs() > f64::EPSILON
                            || (first.1 - last.1).abs() > f64::EPSILON
                        {
                            result.push(first);
                        }
                    }
                    new_rings.push(result.len());
                }

                prev_ring_start = ring_end;
            }

            result
        }
        _ => geometry.coordinates.clone(),
    };

    IosGeometry {
        geometry_type: geometry.geometry_type,
        coordinates: simplified_coords,
        rings: geometry.rings.clone(),
    }
}

// ============================================================================
// GeoJSON Parsing
// ============================================================================

/// Parses a GeoJSON file and returns features.
fn parse_geojson(json_str: &str) -> Result<Vec<IosFeature>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("JSON parse error: {}", e))?;

    let mut features = Vec::new();

    match value.get("type").and_then(|v| v.as_str()) {
        Some("FeatureCollection") => {
            if let Some(feature_array) = value.get("features").and_then(|v| v.as_array()) {
                for (idx, feature_value) in feature_array.iter().enumerate() {
                    if let Some(feature) = parse_geojson_feature(feature_value, idx as i64) {
                        features.push(feature);
                    }
                }
            }
        }
        Some("Feature") => {
            if let Some(feature) = parse_geojson_feature(&value, 0) {
                features.push(feature);
            }
        }
        Some(geom_type) => {
            // Single geometry
            if let Some(geometry) = parse_geojson_geometry(&value) {
                features.push(IosFeature {
                    fid: 0,
                    geometry: Some(geometry),
                    properties: HashMap::new(),
                    bbox: None,
                });
            }
        }
        None => return Err("Missing 'type' field in GeoJSON".to_string()),
    }

    // Calculate bounding boxes for all features
    for feature in &mut features {
        if let Some(ref geom) = feature.geometry {
            feature.bbox = compute_bbox(&geom.coordinates);
        }
    }

    Ok(features)
}

/// Parses a single GeoJSON feature.
fn parse_geojson_feature(value: &serde_json::Value, default_fid: i64) -> Option<IosFeature> {
    let geometry = value.get("geometry").and_then(parse_geojson_geometry);

    let mut properties = HashMap::new();
    if let Some(props) = value.get("properties").and_then(|v| v.as_object()) {
        for (key, val) in props {
            let string_val = match val {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => "null".to_string(),
                _ => val.to_string(),
            };
            properties.insert(key.clone(), string_val);
        }
    }

    let fid = value
        .get("id")
        .and_then(|v| v.as_i64())
        .unwrap_or(default_fid);

    Some(IosFeature {
        fid,
        geometry,
        properties,
        bbox: None,
    })
}

/// Parses a GeoJSON geometry.
fn parse_geojson_geometry(value: &serde_json::Value) -> Option<IosGeometry> {
    let geom_type = value.get("type").and_then(|v| v.as_str())?;
    let coords = value.get("coordinates")?;

    match geom_type {
        "Point" => {
            let coord = parse_coordinate(coords)?;
            Some(IosGeometry {
                geometry_type: GeometryType::Point,
                coordinates: vec![coord],
                rings: vec![],
            })
        }
        "LineString" => {
            let coordinates = parse_coordinate_array(coords)?;
            Some(IosGeometry {
                geometry_type: GeometryType::LineString,
                coordinates,
                rings: vec![],
            })
        }
        "Polygon" => {
            let rings_array = coords.as_array()?;
            let mut coordinates = Vec::new();
            let mut rings = Vec::new();

            for ring in rings_array {
                let ring_coords = parse_coordinate_array(ring)?;
                coordinates.extend(ring_coords);
                rings.push(coordinates.len());
            }

            Some(IosGeometry {
                geometry_type: GeometryType::Polygon,
                coordinates,
                rings,
            })
        }
        "MultiPoint" => {
            let coordinates = parse_coordinate_array(coords)?;
            Some(IosGeometry {
                geometry_type: GeometryType::MultiPoint,
                coordinates,
                rings: vec![],
            })
        }
        "MultiLineString" => {
            let lines_array = coords.as_array()?;
            let mut coordinates = Vec::new();
            let mut rings = Vec::new();

            for line in lines_array {
                let line_coords = parse_coordinate_array(line)?;
                coordinates.extend(line_coords);
                rings.push(coordinates.len());
            }

            Some(IosGeometry {
                geometry_type: GeometryType::MultiLineString,
                coordinates,
                rings,
            })
        }
        "MultiPolygon" => {
            let polygons_array = coords.as_array()?;
            let mut coordinates = Vec::new();
            let mut rings = Vec::new();

            for polygon in polygons_array {
                let rings_array = polygon.as_array()?;
                for ring in rings_array {
                    let ring_coords = parse_coordinate_array(ring)?;
                    coordinates.extend(ring_coords);
                    rings.push(coordinates.len());
                }
            }

            Some(IosGeometry {
                geometry_type: GeometryType::MultiPolygon,
                coordinates,
                rings,
            })
        }
        _ => None,
    }
}

/// Parses a single coordinate.
fn parse_coordinate(value: &serde_json::Value) -> Option<(f64, f64)> {
    let arr = value.as_array()?;
    if arr.len() < 2 {
        return None;
    }
    let x = arr.get(0)?.as_f64()?;
    let y = arr.get(1)?.as_f64()?;
    Some((x, y))
}

/// Parses an array of coordinates.
fn parse_coordinate_array(value: &serde_json::Value) -> Option<Vec<(f64, f64)>> {
    let arr = value.as_array()?;
    let mut coordinates = Vec::with_capacity(arr.len());

    for coord in arr {
        coordinates.push(parse_coordinate(coord)?);
    }

    Some(coordinates)
}

/// Computes the bounding box of a set of coordinates.
fn compute_bbox(coords: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    if coords.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for &(x, y) in coords {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    Some((min_x, min_y, max_x, max_y))
}

// ============================================================================
// Global Layer Storage (for FFI)
// ============================================================================

static LAYERS: Mutex<Vec<IosVectorLayer>> = Mutex::new(Vec::new());

/// Stores a layer and returns its handle.
fn store_layer(layer: IosVectorLayer) -> Option<*mut OxiGdalLayer> {
    let mut layers = LAYERS.lock().ok()?;
    let idx = layers.len();
    layers.push(layer);
    // Return index as a pointer (will be cast back)
    Some((idx + 1) as *mut OxiGdalLayer)
}

/// Retrieves a layer by handle.
#[allow(dead_code)]
fn get_layer(
    handle: *const OxiGdalLayer,
) -> Option<std::sync::MutexGuard<'static, Vec<IosVectorLayer>>> {
    let layers = LAYERS.lock().ok()?;
    let idx = (handle as usize).checked_sub(1)?;
    if idx < layers.len() {
        Some(layers)
    } else {
        None
    }
}

/// Gets the layer index from a handle.
fn handle_to_index(handle: *const OxiGdalLayer) -> Option<usize> {
    (handle as usize).checked_sub(1)
}

// ============================================================================
// FFI Functions
// ============================================================================

/// Converts vector layer to iOS MapKit annotations.
///
/// # Parameters
/// - `layer`: Vector layer handle
/// - `out_coords`: Array to receive coordinate pairs (lon, lat)
/// - `max_coords`: Maximum number of coordinates to return
///
/// # Returns
/// Number of coordinates written, or -1 on error
///
/// # Safety
/// - layer must be valid
/// - out_coords must be pre-allocated with size >= max_coords * 2
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_layer_to_annotations(
    layer: *const OxiGdalLayer,
    out_coords: *mut c_double,
    max_coords: c_int,
) -> c_int {
    if layer.is_null() || out_coords.is_null() || max_coords <= 0 {
        crate::ffi::error::set_last_error("Invalid parameters".to_string());
        return -1;
    }

    let idx = match handle_to_index(layer) {
        Some(i) => i,
        None => {
            crate::ffi::error::set_last_error("Invalid layer handle".to_string());
            return -1;
        }
    };

    let layers = match LAYERS.lock() {
        Ok(l) => l,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to acquire lock".to_string());
            return -1;
        }
    };

    if idx >= layers.len() {
        crate::ffi::error::set_last_error("Layer not found".to_string());
        return -1;
    }

    let layer_data = &layers[idx];
    let mut written = 0;
    let max_coords = max_coords as usize;

    for feature in &layer_data.features {
        if let Some(ref geom) = feature.geometry {
            for &(lon, lat) in &geom.coordinates {
                if written >= max_coords {
                    break;
                }
                unsafe {
                    *out_coords.add(written * 2) = lon;
                    *out_coords.add(written * 2 + 1) = lat;
                }
                written += 1;
            }
        }
        if written >= max_coords {
            break;
        }
    }

    written as c_int
}

/// Checks if a point is within iOS MapKit visible region.
///
/// # Parameters
/// - `point`: Point to check
/// - `bbox`: Visible bounding box
///
/// # Returns
/// - 1 if point is visible
/// - 0 if not visible or on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_point_in_visible_region(
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

/// Converts GeoJSON to iOS-compatible format.
///
/// # Parameters
/// - `geojson_path`: Path to GeoJSON file
/// - `out_layer`: Output layer handle
///
/// # Safety
/// - geojson_path must be valid null-terminated string
/// - out_layer must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_load_geojson(
    geojson_path: *const c_char,
    out_layer: *mut *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    if geojson_path.is_null() || out_layer.is_null() {
        crate::ffi::error::set_last_error("Null pointer provided".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    // Convert path to Rust string
    let path = match unsafe { CStr::from_ptr(geojson_path) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid UTF-8 in path".to_string());
            return OxiGdalErrorCode::InvalidUtf8;
        }
    };

    // Read the file
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to read file: {}", e));
            return OxiGdalErrorCode::FileNotFound;
        }
    };

    // Parse GeoJSON
    let features = match parse_geojson(&contents) {
        Ok(f) => f,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to parse GeoJSON: {}", e));
            return OxiGdalErrorCode::UnsupportedFormat;
        }
    };

    // Compute layer bounding box
    let layer_bbox = if features.is_empty() {
        None
    } else {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for feature in &features {
            if let Some((fx_min, fy_min, fx_max, fy_max)) = feature.bbox {
                min_x = min_x.min(fx_min);
                min_y = min_y.min(fy_min);
                max_x = max_x.max(fx_max);
                max_y = max_y.max(fy_max);
            }
        }

        if min_x.is_finite() {
            Some(OxiGdalBbox {
                min_x,
                min_y,
                max_x,
                max_y,
            })
        } else {
            None
        }
    };

    // Create layer
    let layer = IosVectorLayer {
        name: path.to_string(),
        features,
        spatial_index: None,
        bbox: layer_bbox,
    };

    // Store layer and return handle
    match store_layer(layer) {
        Some(handle) => {
            unsafe { *out_layer = handle };
            OxiGdalErrorCode::Success
        }
        None => {
            crate::ffi::error::set_last_error("Failed to store layer".to_string());
            OxiGdalErrorCode::AllocationFailed
        }
    }
}

/// Creates iOS MapKit overlay from vector layer.
///
/// Returns a pointer to MapKitOverlayData that must be freed by the caller.
///
/// # Safety
/// - layer must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_create_map_overlay(
    layer: *const OxiGdalLayer,
) -> *mut std::os::raw::c_void {
    if layer.is_null() {
        crate::ffi::error::set_last_error("Null layer pointer".to_string());
        return ptr::null_mut();
    }

    let idx = match handle_to_index(layer) {
        Some(i) => i,
        None => {
            crate::ffi::error::set_last_error("Invalid layer handle".to_string());
            return ptr::null_mut();
        }
    };

    let layers = match LAYERS.lock() {
        Ok(l) => l,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to acquire lock".to_string());
            return ptr::null_mut();
        }
    };

    if idx >= layers.len() {
        crate::ffi::error::set_last_error("Layer not found".to_string());
        return ptr::null_mut();
    }

    let layer_data = &layers[idx];

    // Collect all coordinates and determine overlay type
    let mut all_coords: Vec<c_double> = Vec::new();
    let mut ring_starts: Vec<c_int> = Vec::new();
    let mut overlay_type = MapKitOverlayType::Annotation;

    for feature in &layer_data.features {
        if let Some(ref geom) = feature.geometry {
            match geom.geometry_type {
                GeometryType::Point | GeometryType::MultiPoint => {
                    overlay_type = MapKitOverlayType::Annotation;
                }
                GeometryType::LineString | GeometryType::MultiLineString => {
                    overlay_type = MapKitOverlayType::Polyline;
                }
                GeometryType::Polygon | GeometryType::MultiPolygon => {
                    overlay_type = MapKitOverlayType::Polygon;
                }
                _ => {}
            }

            for &(lon, lat) in &geom.coordinates {
                all_coords.push(lon);
                all_coords.push(lat);
            }

            for &ring_end in &geom.rings {
                ring_starts.push(ring_end as c_int);
            }
        }
    }

    if all_coords.is_empty() {
        crate::ffi::error::set_last_error("No coordinates in layer".to_string());
        return ptr::null_mut();
    }

    // Allocate coordinate array
    let coord_count = all_coords.len() / 2;
    let coords_ptr = match std::alloc::Layout::array::<c_double>(all_coords.len()) {
        Ok(layout) => unsafe { std::alloc::alloc(layout) as *mut c_double },
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to allocate coordinates".to_string());
            return ptr::null_mut();
        }
    };

    if coords_ptr.is_null() {
        crate::ffi::error::set_last_error("Failed to allocate coordinates".to_string());
        return ptr::null_mut();
    }

    // Copy coordinates
    unsafe {
        ptr::copy_nonoverlapping(all_coords.as_ptr(), coords_ptr, all_coords.len());
    }

    // Allocate ring starts if needed
    let rings_ptr = if !ring_starts.is_empty() {
        match std::alloc::Layout::array::<c_int>(ring_starts.len()) {
            Ok(layout) => {
                let ptr = unsafe { std::alloc::alloc(layout) as *mut c_int };
                if !ptr.is_null() {
                    unsafe {
                        ptr::copy_nonoverlapping(ring_starts.as_ptr(), ptr, ring_starts.len());
                    }
                }
                ptr
            }
            Err(_) => ptr::null_mut(),
        }
    } else {
        ptr::null_mut()
    };

    // Create overlay data
    let overlay_data = Box::new(MapKitOverlayData {
        overlay_type,
        coord_count: coord_count as c_int,
        coordinates: coords_ptr,
        ring_count: ring_starts.len() as c_int,
        ring_starts: rings_ptr,
    });

    Box::into_raw(overlay_data) as *mut std::os::raw::c_void
}

/// Frees a MapKit overlay data structure.
///
/// # Safety
/// - overlay must be a valid pointer returned by oxigdal_ios_create_map_overlay
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_free_map_overlay(overlay: *mut std::os::raw::c_void) {
    if overlay.is_null() {
        return;
    }

    let overlay_data = unsafe { Box::from_raw(overlay as *mut MapKitOverlayData) };

    // Free coordinates
    if !overlay_data.coordinates.is_null() {
        let coord_count = overlay_data.coord_count as usize * 2;
        if let Ok(layout) = std::alloc::Layout::array::<c_double>(coord_count) {
            unsafe {
                std::alloc::dealloc(overlay_data.coordinates as *mut u8, layout);
            }
        }
    }

    // Free ring starts
    if !overlay_data.ring_starts.is_null() {
        let ring_count = overlay_data.ring_count as usize;
        if let Ok(layout) = std::alloc::Layout::array::<c_int>(ring_count) {
            unsafe {
                std::alloc::dealloc(overlay_data.ring_starts as *mut u8, layout);
            }
        }
    }

    // overlay_data is dropped here
}

/// Simplifies geometry for iOS display at given zoom level.
///
/// Uses Douglas-Peucker algorithm with tolerance based on zoom.
///
/// # Parameters
/// - `layer`: Input layer
/// - `zoom`: Map zoom level (0-20)
/// - `out_layer`: Simplified output layer
///
/// # Safety
/// - layer and out_layer must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_simplify_for_zoom(
    layer: *const OxiGdalLayer,
    zoom: c_int,
    out_layer: *mut *mut OxiGdalLayer,
) -> OxiGdalErrorCode {
    if layer.is_null() || out_layer.is_null() {
        crate::ffi::error::set_last_error("Null pointer provided".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    if !(0..=22).contains(&zoom) {
        crate::ffi::error::set_last_error("Invalid zoom level (must be 0-22)".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let idx = match handle_to_index(layer) {
        Some(i) => i,
        None => {
            crate::ffi::error::set_last_error("Invalid layer handle".to_string());
            return OxiGdalErrorCode::InvalidArgument;
        }
    };

    let layers = match LAYERS.lock() {
        Ok(l) => l,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to acquire lock".to_string());
            return OxiGdalErrorCode::Unknown;
        }
    };

    if idx >= layers.len() {
        crate::ffi::error::set_last_error("Layer not found".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let source_layer = &layers[idx];

    // Clone needed data before releasing the lock
    let source_name = source_layer.name.clone();
    let source_bbox = source_layer.bbox;

    // Simplify all features
    let simplified_features: Vec<IosFeature> = source_layer
        .features
        .iter()
        .map(|f| {
            let simplified_geom = f.geometry.as_ref().map(|g| simplify_geometry(g, zoom));
            IosFeature {
                fid: f.fid,
                geometry: simplified_geom,
                properties: f.properties.clone(),
                bbox: f.bbox,
            }
        })
        .collect();

    drop(layers); // Release lock before storing

    // Create new layer with simplified geometries
    let new_layer = IosVectorLayer {
        name: format!("{}_simplified_z{}", source_name, zoom),
        features: simplified_features,
        spatial_index: None,
        bbox: source_bbox,
    };

    match store_layer(new_layer) {
        Some(handle) => {
            unsafe { *out_layer = handle };
            OxiGdalErrorCode::Success
        }
        None => {
            crate::ffi::error::set_last_error("Failed to store simplified layer".to_string());
            OxiGdalErrorCode::AllocationFailed
        }
    }
}

/// Converts vector features to iOS search index format.
///
/// Builds an R-tree spatial index for efficient spatial queries.
///
/// # Parameters
/// - `layer`: Vector layer
/// - `attribute_name`: Attribute to index (for search by attribute)
///
/// # Returns
/// - 0 on success
/// - non-zero on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_index_features(
    layer: *const OxiGdalLayer,
    attribute_name: *const c_char,
) -> c_int {
    if layer.is_null() {
        crate::ffi::error::set_last_error("Null layer pointer".to_string());
        return -1;
    }

    let idx = match handle_to_index(layer) {
        Some(i) => i,
        None => {
            crate::ffi::error::set_last_error("Invalid layer handle".to_string());
            return -1;
        }
    };

    // Parse attribute name if provided
    let _attr_name: Option<String> = if !attribute_name.is_null() {
        match unsafe { CStr::from_ptr(attribute_name) }.to_str() {
            Ok(s) => Some(s.to_string()),
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid UTF-8 in attribute name".to_string());
                return -1;
            }
        }
    } else {
        None
    };

    let mut layers = match LAYERS.lock() {
        Ok(l) => l,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to acquire lock".to_string());
            return -1;
        }
    };

    if idx >= layers.len() {
        crate::ffi::error::set_last_error("Layer not found".to_string());
        return -1;
    }

    // Build R-tree index
    let mut rtree = RTreeIndex::new();

    for (feature_idx, feature) in layers[idx].features.iter().enumerate() {
        if let Some((min_x, min_y, max_x, max_y)) = feature.bbox {
            let rect = Rect::new(min_x, min_y, max_x, max_y);
            rtree.insert(feature_idx, rect);
        }
    }

    layers[idx].spatial_index = Some(rtree);

    0
}

/// Queries features within a bounding box using the spatial index.
///
/// # Parameters
/// - `layer`: Vector layer (must have been indexed)
/// - `bbox`: Query bounding box
/// - `out_fids`: Output array for feature IDs
/// - `max_fids`: Maximum number of FIDs to return
///
/// # Returns
/// Number of features found, or -1 on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_query_features(
    layer: *const OxiGdalLayer,
    bbox: *const OxiGdalBbox,
    out_fids: *mut i64,
    max_fids: c_int,
) -> c_int {
    if layer.is_null() || bbox.is_null() || out_fids.is_null() || max_fids <= 0 {
        crate::ffi::error::set_last_error("Invalid parameters".to_string());
        return -1;
    }

    let idx = match handle_to_index(layer) {
        Some(i) => i,
        None => {
            crate::ffi::error::set_last_error("Invalid layer handle".to_string());
            return -1;
        }
    };

    let layers = match LAYERS.lock() {
        Ok(l) => l,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to acquire lock".to_string());
            return -1;
        }
    };

    if idx >= layers.len() {
        crate::ffi::error::set_last_error("Layer not found".to_string());
        return -1;
    }

    let layer_data = &layers[idx];

    let bb = unsafe { &*bbox };
    let query_rect = Rect::new(bb.min_x, bb.min_y, bb.max_x, bb.max_y);

    let results = match &layer_data.spatial_index {
        Some(rtree) => rtree.search(&query_rect),
        None => {
            // Fallback: linear search if no index
            layer_data
                .features
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    if let Some((fx_min, fy_min, fx_max, fy_max)) = f.bbox {
                        let feature_rect = Rect::new(fx_min, fy_min, fx_max, fy_max);
                        feature_rect.intersects(&query_rect)
                    } else {
                        false
                    }
                })
                .map(|(i, _)| i)
                .collect()
        }
    };

    let max_fids = max_fids as usize;
    let mut written = 0;

    for feature_idx in results {
        if written >= max_fids {
            break;
        }
        if feature_idx < layer_data.features.len() {
            unsafe {
                *out_fids.add(written) = layer_data.features[feature_idx].fid;
            }
            written += 1;
        }
    }

    written as c_int
}

/// Closes a vector layer and frees resources.
///
/// # Safety
/// - layer must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_layer_close(layer: *mut OxiGdalLayer) -> OxiGdalErrorCode {
    if layer.is_null() {
        return OxiGdalErrorCode::NullPointer;
    }

    // Note: In a full implementation, we would mark the layer as freed
    // For now, layers remain in the vector (memory management simplified)
    OxiGdalErrorCode::Success
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_in_region() {
        let point = OxiGdalPoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };

        let bbox = OxiGdalBbox {
            min_x: -10.0,
            min_y: -10.0,
            max_x: 10.0,
            max_y: 10.0,
        };

        let result = unsafe { oxigdal_ios_point_in_visible_region(&point, &bbox) };
        assert_eq!(result, 1);

        let outside_point = OxiGdalPoint {
            x: 20.0,
            y: 20.0,
            z: 0.0,
        };

        let result = unsafe { oxigdal_ios_point_in_visible_region(&outside_point, &bbox) };
        assert_eq!(result, 0);
    }

    #[test]
    fn test_perpendicular_distance() {
        // Point on the line
        let dist = perpendicular_distance((0.5, 0.0), (0.0, 0.0), (1.0, 0.0));
        assert!(dist < f64::EPSILON);

        // Point perpendicular to line
        let dist = perpendicular_distance((0.5, 1.0), (0.0, 0.0), (1.0, 0.0));
        assert!((dist - 1.0).abs() < f64::EPSILON);

        // Point at end of line
        let dist = perpendicular_distance((0.0, 0.0), (0.0, 0.0), (1.0, 0.0));
        assert!(dist < f64::EPSILON);
    }

    #[test]
    fn test_douglas_peucker_simple() {
        let points = vec![(0.0, 0.0), (0.5, 0.0), (1.0, 0.0)];
        let simplified = douglas_peucker(&points, 0.1);
        // Middle point should be removed (collinear)
        assert_eq!(simplified.len(), 2);
        assert_eq!(simplified[0], (0.0, 0.0));
        assert_eq!(simplified[1], (1.0, 0.0));
    }

    #[test]
    fn test_douglas_peucker_preserve() {
        let points = vec![(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)];
        let simplified = douglas_peucker(&points, 0.1);
        // Point is not collinear, should be preserved
        assert_eq!(simplified.len(), 3);
    }

    #[test]
    fn test_tolerance_for_zoom() {
        let t0 = tolerance_for_zoom(0);
        let t10 = tolerance_for_zoom(10);
        let t20 = tolerance_for_zoom(20);

        assert!(t0 > t10);
        assert!(t10 > t20);
        assert!(t20 > 0.0);
    }

    #[test]
    fn test_rect_intersects() {
        let r1 = Rect::new(0.0, 0.0, 10.0, 10.0);
        let r2 = Rect::new(5.0, 5.0, 15.0, 15.0);
        let r3 = Rect::new(20.0, 20.0, 30.0, 30.0);

        assert!(r1.intersects(&r2));
        assert!(r2.intersects(&r1));
        assert!(!r1.intersects(&r3));
        assert!(!r3.intersects(&r1));
    }

    #[test]
    fn test_rect_union() {
        let r1 = Rect::new(0.0, 0.0, 10.0, 10.0);
        let r2 = Rect::new(5.0, 5.0, 15.0, 15.0);
        let union = r1.union(&r2);

        assert_eq!(union.min_x, 0.0);
        assert_eq!(union.min_y, 0.0);
        assert_eq!(union.max_x, 15.0);
        assert_eq!(union.max_y, 15.0);
    }

    #[test]
    fn test_rtree_insert_and_search() {
        let mut rtree = RTreeIndex::new();

        // Insert some features
        rtree.insert(0, Rect::new(0.0, 0.0, 1.0, 1.0));
        rtree.insert(1, Rect::new(5.0, 5.0, 6.0, 6.0));
        rtree.insert(2, Rect::new(10.0, 10.0, 11.0, 11.0));

        // Search for features in a region
        let results = rtree.search(&Rect::new(4.0, 4.0, 7.0, 7.0));
        assert_eq!(results.len(), 1);
        assert!(results.contains(&1));

        // Search for features that don't exist
        let results = rtree.search(&Rect::new(100.0, 100.0, 101.0, 101.0));
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_geojson_point() {
        let geojson = r#"{
            "type": "Feature",
            "geometry": {
                "type": "Point",
                "coordinates": [100.0, 0.0]
            },
            "properties": {
                "name": "Test Point"
            }
        }"#;

        let features = parse_geojson(geojson);
        assert!(features.is_ok());
        let features = features.expect("parse should succeed");
        assert_eq!(features.len(), 1);
        assert!(features[0].geometry.is_some());

        let geom = features[0].geometry.as_ref().expect("geometry exists");
        assert_eq!(geom.geometry_type, GeometryType::Point);
        assert_eq!(geom.coordinates.len(), 1);
        assert_eq!(geom.coordinates[0], (100.0, 0.0));
    }

    #[test]
    fn test_parse_geojson_linestring() {
        let geojson = r#"{
            "type": "Feature",
            "geometry": {
                "type": "LineString",
                "coordinates": [[0, 0], [1, 1], [2, 0]]
            },
            "properties": {}
        }"#;

        let features = parse_geojson(geojson);
        assert!(features.is_ok());
        let features = features.expect("parse should succeed");
        assert_eq!(features.len(), 1);

        let geom = features[0].geometry.as_ref().expect("geometry exists");
        assert_eq!(geom.geometry_type, GeometryType::LineString);
        assert_eq!(geom.coordinates.len(), 3);
    }

    #[test]
    fn test_parse_geojson_polygon() {
        let geojson = r#"{
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]
            },
            "properties": {}
        }"#;

        let features = parse_geojson(geojson);
        assert!(features.is_ok());
        let features = features.expect("parse should succeed");
        assert_eq!(features.len(), 1);

        let geom = features[0].geometry.as_ref().expect("geometry exists");
        assert_eq!(geom.geometry_type, GeometryType::Polygon);
        assert_eq!(geom.coordinates.len(), 5);
        assert_eq!(geom.rings.len(), 1);
    }

    #[test]
    fn test_parse_geojson_feature_collection() {
        let geojson = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [0, 0]},
                    "properties": {"id": 1}
                },
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [1, 1]},
                    "properties": {"id": 2}
                }
            ]
        }"#;

        let features = parse_geojson(geojson);
        assert!(features.is_ok());
        let features = features.expect("parse should succeed");
        assert_eq!(features.len(), 2);
    }

    #[test]
    fn test_compute_bbox() {
        let coords = vec![(0.0, 0.0), (10.0, 5.0), (5.0, 10.0)];
        let bbox = compute_bbox(&coords);
        assert!(bbox.is_some());

        let (min_x, min_y, max_x, max_y) = bbox.expect("bbox exists");
        assert_eq!(min_x, 0.0);
        assert_eq!(min_y, 0.0);
        assert_eq!(max_x, 10.0);
        assert_eq!(max_y, 10.0);
    }

    #[test]
    fn test_compute_bbox_empty() {
        let coords: Vec<(f64, f64)> = vec![];
        let bbox = compute_bbox(&coords);
        assert!(bbox.is_none());
    }

    #[test]
    fn test_geometry_type_values() {
        assert_eq!(GeometryType::Unknown as i32, 0);
        assert_eq!(GeometryType::Point as i32, 1);
        assert_eq!(GeometryType::LineString as i32, 2);
        assert_eq!(GeometryType::Polygon as i32, 3);
    }

    #[test]
    fn test_null_pointer_handling() {
        let result = unsafe { oxigdal_ios_point_in_visible_region(ptr::null(), ptr::null()) };
        assert_eq!(result, 0);

        let result = unsafe { oxigdal_ios_layer_to_annotations(ptr::null(), ptr::null_mut(), 10) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_simplify_geometry_point() {
        let geom = IosGeometry {
            geometry_type: GeometryType::Point,
            coordinates: vec![(0.0, 0.0)],
            rings: vec![],
        };

        let simplified = simplify_geometry(&geom, 10);
        assert_eq!(simplified.coordinates.len(), 1);
    }

    #[test]
    fn test_simplify_geometry_linestring() {
        // Create a line with many collinear points
        let geom = IosGeometry {
            geometry_type: GeometryType::LineString,
            coordinates: vec![(0.0, 0.0), (0.25, 0.0), (0.5, 0.0), (0.75, 0.0), (1.0, 0.0)],
            rings: vec![],
        };

        let simplified = simplify_geometry(&geom, 0); // High tolerance at zoom 0
        assert!(simplified.coordinates.len() < geom.coordinates.len());
    }
}
