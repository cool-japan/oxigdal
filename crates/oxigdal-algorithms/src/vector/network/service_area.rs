//! Service area and isochrone calculation
//!
//! Compute reachable areas from one or more locations within specified constraints:
//!
//! - **Service areas**: Determine all reachable nodes within cost thresholds
//! - **Isochrones**: Generate contour polygons of equal travel time/distance
//! - **Multi-facility**: Compute service areas from multiple origins simultaneously
//! - **Overlapping analysis**: Identify competition zones between facilities
//! - **Break values**: Support multiple cost thresholds for tiered analysis

use crate::error::{AlgorithmError, Result};
use crate::vector::network::{EdgeId, Graph, NodeId, ShortestPathOptions};
use oxigdal_core::vector::{Coordinate, LineString, Polygon};
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Options for service area calculation
#[derive(Debug, Clone)]
pub struct ServiceAreaOptions {
    /// Maximum cost (distance or time)
    pub max_cost: f64,
    /// Cost intervals for multiple service areas (break values)
    pub intervals: Vec<f64>,
    /// Whether to include unreachable nodes
    pub include_unreachable: bool,
    /// Cost type to use
    pub cost_type: ServiceAreaCostType,
    /// Weight criteria for multi-criteria cost
    pub weight_criteria: Option<HashMap<String, f64>>,
}

/// What cost metric to use for service area computation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAreaCostType {
    /// Use edge weight (distance)
    Distance,
    /// Use travel time
    Time,
    /// Use multi-criteria weighted cost
    MultiCriteria,
}

impl Default for ServiceAreaOptions {
    fn default() -> Self {
        Self {
            max_cost: 1000.0,
            intervals: vec![250.0, 500.0, 750.0, 1000.0],
            include_unreachable: false,
            cost_type: ServiceAreaCostType::Distance,
            weight_criteria: None,
        }
    }
}

/// A service area result
#[derive(Debug, Clone)]
pub struct ServiceArea {
    /// Origin node
    pub origin: NodeId,
    /// Reachable nodes with their costs
    pub reachable_nodes: HashMap<NodeId, f64>,
    /// Service area intervals (break values)
    pub intervals: Vec<ServiceAreaInterval>,
    /// Edges reachable within the max cost
    pub reachable_edges: Vec<(EdgeId, f64, f64)>, // (edge, start_cost, end_cost)
}

/// A single service area interval (break value)
#[derive(Debug, Clone)]
pub struct ServiceAreaInterval {
    /// Maximum cost for this interval
    pub max_cost: f64,
    /// Nodes within this interval
    pub nodes: Vec<NodeId>,
    /// Number of reachable nodes
    pub count: usize,
    /// Boundary polygon (if computed)
    pub boundary: Option<Polygon>,
    /// Area of the boundary polygon (if computed)
    pub area: Option<f64>,
}

/// Options for isochrone calculation
#[derive(Debug, Clone)]
pub struct IsochroneOptions {
    /// Time intervals in seconds
    pub time_intervals: Vec<f64>,
    /// Whether to smooth the isochrone polygons
    pub smooth: bool,
    /// Smoothing factor (0.0 - 1.0)
    pub smoothing_factor: f64,
    /// Method for polygon generation
    pub polygon_method: IsochronePolygonMethod,
}

/// Method for generating isochrone polygons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsochronePolygonMethod {
    /// Convex hull (fast, less accurate)
    ConvexHull,
    /// Concave hull / alpha shape (slower, more accurate)
    ConcaveHull,
}

impl Default for IsochroneOptions {
    fn default() -> Self {
        Self {
            time_intervals: vec![300.0, 600.0, 900.0, 1200.0], // 5, 10, 15, 20 minutes
            smooth: true,
            smoothing_factor: 0.5,
            polygon_method: IsochronePolygonMethod::ConvexHull,
        }
    }
}

/// An isochrone (contour of equal travel time/distance)
#[derive(Debug, Clone)]
pub struct Isochrone {
    /// Time/distance value for this isochrone
    pub value: f64,
    /// Polygon representing the isochrone boundary
    pub polygon: Option<Polygon>,
    /// Nodes on or inside the isochrone
    pub nodes: Vec<NodeId>,
    /// Area of the isochrone polygon (if computed)
    pub area: Option<f64>,
    /// Perimeter of the isochrone polygon (if computed)
    pub perimeter: Option<f64>,
}

/// Multi-facility service area result
#[derive(Debug, Clone)]
pub struct MultiFacilityResult {
    /// Individual service areas per facility
    pub facility_areas: Vec<ServiceArea>,
    /// Assignment of each node to its nearest facility
    pub nearest_facility: HashMap<NodeId, (NodeId, f64)>, // node -> (facility, cost)
    /// Overlap zones between facilities
    pub overlap_zones: Vec<OverlapZone>,
    /// Nodes not reachable from any facility
    pub unreachable_nodes: Vec<NodeId>,
}

/// An overlap zone between two or more facilities
#[derive(Debug, Clone)]
pub struct OverlapZone {
    /// Facility IDs involved in this overlap
    pub facilities: Vec<NodeId>,
    /// Nodes in the overlap zone
    pub nodes: Vec<NodeId>,
    /// Boundary polygon of the overlap zone (if computed)
    pub boundary: Option<Polygon>,
}

/// Calculate service area from a starting node
///
/// # Arguments
///
/// * `graph` - The network graph
/// * `origin` - Starting node
/// * `options` - Service area options
///
/// # Returns
///
/// Service area showing all reachable nodes and areas
pub fn calculate_service_area(
    graph: &Graph,
    origin: NodeId,
    options: &ServiceAreaOptions,
) -> Result<ServiceArea> {
    if graph.get_node(origin).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Origin node {:?} not found",
            origin
        )));
    }

    // Use Dijkstra-like expansion to find all reachable nodes
    let mut distances: HashMap<NodeId, f64> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut reachable_edges: Vec<(EdgeId, f64, f64)> = Vec::new();

    distances.insert(origin, 0.0);
    heap.push(ServiceAreaState {
        cost: 0.0,
        node: origin,
    });

    while let Some(ServiceAreaState { cost, node }) = heap.pop() {
        // Skip if we've exceeded max cost
        if cost > options.max_cost {
            continue;
        }

        // Skip if we've already found a better path
        if let Some(&best_cost) = distances.get(&node) {
            if cost > best_cost {
                continue;
            }
        }

        // Explore neighbors
        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;
                let edge_cost = compute_edge_cost(edge, options);
                let next_cost = cost + edge_cost;

                if next_cost <= options.max_cost {
                    let is_better = distances
                        .get(&next_node)
                        .is_none_or(|&current| next_cost < current);

                    if is_better {
                        distances.insert(next_node, next_cost);
                        reachable_edges.push((edge_id, cost, next_cost));
                        heap.push(ServiceAreaState {
                            cost: next_cost,
                            node: next_node,
                        });
                    }
                }
            }
        }
    }

    // Build service area intervals with break values
    let mut intervals = Vec::new();

    for &interval_cost in &options.intervals {
        let nodes: Vec<NodeId> = distances
            .iter()
            .filter(|(_, cost)| **cost <= interval_cost)
            .map(|(node, _)| *node)
            .collect();

        // Build boundary polygon
        let boundary = build_isochrone_polygon(graph, &nodes, 0.5).ok().flatten();
        let area = boundary.as_ref().map(|p| compute_polygon_area(p));

        intervals.push(ServiceAreaInterval {
            max_cost: interval_cost,
            count: nodes.len(),
            nodes,
            boundary,
            area,
        });
    }

    Ok(ServiceArea {
        origin,
        reachable_nodes: distances,
        intervals,
        reachable_edges,
    })
}

/// Compute the cost of an edge based on service area options
fn compute_edge_cost(edge: &crate::vector::network::Edge, options: &ServiceAreaOptions) -> f64 {
    match options.cost_type {
        ServiceAreaCostType::Distance => edge.weight,
        ServiceAreaCostType::Time => {
            edge.travel_time()
                .map(|t| t * 3600.0) // hours to seconds
                .unwrap_or(edge.weight / 13.89) // default ~50 km/h
        }
        ServiceAreaCostType::MultiCriteria => {
            if let Some(ref criteria) = options.weight_criteria {
                edge.multi_weight.weighted_cost(criteria)
            } else {
                edge.weight
            }
        }
    }
}

/// Calculate multi-facility service areas
///
/// Computes service areas from multiple facilities simultaneously,
/// assigning each node to the nearest facility and identifying overlap zones.
pub fn calculate_multi_facility_service_area(
    graph: &Graph,
    facilities: &[NodeId],
    options: &ServiceAreaOptions,
) -> Result<MultiFacilityResult> {
    if facilities.is_empty() {
        return Err(AlgorithmError::InvalidInput(
            "At least one facility is required".to_string(),
        ));
    }

    // Validate all facilities exist
    for &facility in facilities {
        if graph.get_node(facility).is_none() {
            return Err(AlgorithmError::InvalidGeometry(format!(
                "Facility node {:?} not found",
                facility
            )));
        }
    }

    // Multi-source Dijkstra: expand from all facilities simultaneously
    let mut distances: HashMap<NodeId, f64> = HashMap::new();
    let mut nearest_facility: HashMap<NodeId, (NodeId, f64)> = HashMap::new();
    let mut heap = BinaryHeap::new();

    // Initialize with all facilities at cost 0
    for &facility in facilities {
        distances.insert(facility, 0.0);
        nearest_facility.insert(facility, (facility, 0.0));
        heap.push(MultiFacilityState {
            cost: 0.0,
            node: facility,
            facility,
        });
    }

    // Track all facility costs per node (for overlap analysis)
    let mut all_facility_costs: HashMap<NodeId, Vec<(NodeId, f64)>> = HashMap::new();

    while let Some(MultiFacilityState {
        cost,
        node,
        facility,
    }) = heap.pop()
    {
        if cost > options.max_cost {
            continue;
        }

        // Check if this is better than current best for this (node, facility) pair
        let entry = all_facility_costs.entry(node).or_default();
        let already_visited_by_facility = entry.iter().any(|(f, c)| *f == facility && *c <= cost);
        if already_visited_by_facility {
            continue;
        }
        entry.push((facility, cost));

        // Update nearest facility
        let update_nearest = nearest_facility
            .get(&node)
            .is_none_or(|&(_, current_cost)| cost < current_cost);

        if update_nearest {
            distances.insert(node, cost);
            nearest_facility.insert(node, (facility, cost));
        }

        // Expand
        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;
                let edge_cost = compute_edge_cost(edge, options);
                let next_cost = cost + edge_cost;

                if next_cost <= options.max_cost {
                    heap.push(MultiFacilityState {
                        cost: next_cost,
                        node: next_node,
                        facility,
                    });
                }
            }
        }
    }

    // Compute individual facility areas
    let mut facility_areas = Vec::new();
    for &facility in facilities {
        let reachable: HashMap<NodeId, f64> = all_facility_costs
            .iter()
            .filter_map(|(&node, costs)| {
                costs
                    .iter()
                    .find(|(f, _)| *f == facility)
                    .map(|(_, c)| (node, *c))
            })
            .collect();

        let mut intervals = Vec::new();
        for &interval_cost in &options.intervals {
            let nodes: Vec<NodeId> = reachable
                .iter()
                .filter(|(_, cost)| **cost <= interval_cost)
                .map(|(node, _)| *node)
                .collect();

            intervals.push(ServiceAreaInterval {
                max_cost: interval_cost,
                count: nodes.len(),
                nodes,
                boundary: None,
                area: None,
            });
        }

        facility_areas.push(ServiceArea {
            origin: facility,
            reachable_nodes: reachable,
            intervals,
            reachable_edges: Vec::new(),
        });
    }

    // Identify overlap zones
    let overlap_zones = compute_overlap_zones(&all_facility_costs, facilities, options);

    // Identify unreachable nodes
    let all_nodes: HashSet<NodeId> = graph.node_ids().into_iter().collect();
    let reachable_nodes: HashSet<NodeId> = distances.keys().copied().collect();
    let unreachable_nodes: Vec<NodeId> = all_nodes.difference(&reachable_nodes).copied().collect();

    Ok(MultiFacilityResult {
        facility_areas,
        nearest_facility,
        overlap_zones,
        unreachable_nodes,
    })
}

/// Compute overlap zones between facilities
fn compute_overlap_zones(
    all_facility_costs: &HashMap<NodeId, Vec<(NodeId, f64)>>,
    facilities: &[NodeId],
    _options: &ServiceAreaOptions,
) -> Vec<OverlapZone> {
    // Group nodes by which facilities can reach them
    let mut facility_sets: HashMap<Vec<NodeId>, Vec<NodeId>> = HashMap::new();

    for (&node, costs) in all_facility_costs {
        let mut reaching_facilities: Vec<NodeId> = costs.iter().map(|(f, _)| *f).collect();
        reaching_facilities.sort();
        reaching_facilities.dedup();

        if reaching_facilities.len() >= 2 {
            facility_sets
                .entry(reaching_facilities)
                .or_default()
                .push(node);
        }
    }

    // Build overlap zones
    let mut zones = Vec::new();
    for (fac_set, nodes) in facility_sets {
        if nodes.is_empty() {
            continue;
        }
        zones.push(OverlapZone {
            facilities: fac_set,
            nodes,
            boundary: None,
        });
    }

    zones
}

/// State for service area computation
#[derive(Debug, Clone)]
struct ServiceAreaState {
    cost: f64,
    node: NodeId,
}

impl PartialEq for ServiceAreaState {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for ServiceAreaState {}

impl PartialOrd for ServiceAreaState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ServiceAreaState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// State for multi-facility service area
#[derive(Debug, Clone)]
struct MultiFacilityState {
    cost: f64,
    node: NodeId,
    facility: NodeId,
}

impl PartialEq for MultiFacilityState {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for MultiFacilityState {}

impl PartialOrd for MultiFacilityState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MultiFacilityState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Calculate isochrones from a starting point
///
/// # Arguments
///
/// * `graph` - The network graph
/// * `origin` - Starting node
/// * `options` - Isochrone options
///
/// # Returns
///
/// Vector of isochrones for each time interval
pub fn calculate_isochrones(
    graph: &Graph,
    origin: NodeId,
    options: &IsochroneOptions,
) -> Result<Vec<Isochrone>> {
    // Find the maximum interval safely
    let max_interval = options
        .time_intervals
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    let max_cost = if max_interval.is_finite() {
        max_interval
    } else {
        1000.0
    };

    // Calculate service area
    let service_area_options = ServiceAreaOptions {
        max_cost,
        intervals: options.time_intervals.clone(),
        include_unreachable: false,
        cost_type: ServiceAreaCostType::Distance,
        weight_criteria: None,
    };

    let service_area = calculate_service_area(graph, origin, &service_area_options)?;

    // Build isochrones from service area intervals
    let mut isochrones = Vec::new();

    for interval in service_area.intervals {
        let polygon = if options.smooth {
            match options.polygon_method {
                IsochronePolygonMethod::ConvexHull => {
                    build_isochrone_polygon(graph, &interval.nodes, options.smoothing_factor)?
                }
                IsochronePolygonMethod::ConcaveHull => {
                    build_concave_hull_polygon(graph, &interval.nodes, options.smoothing_factor)?
                }
            }
        } else {
            build_isochrone_polygon(graph, &interval.nodes, 0.0)?
        };

        let area = polygon.as_ref().map(|p| compute_polygon_area(p));
        let perimeter = polygon.as_ref().map(|p| compute_polygon_perimeter(p));

        isochrones.push(Isochrone {
            value: interval.max_cost,
            polygon,
            nodes: interval.nodes,
            area,
            perimeter,
        });
    }

    Ok(isochrones)
}

/// Calculate isochrones from multiple origins (multi-facility isochrones)
pub fn calculate_multi_isochrones(
    graph: &Graph,
    origins: &[NodeId],
    options: &IsochroneOptions,
) -> Result<Vec<Vec<Isochrone>>> {
    let mut all_isochrones = Vec::new();

    for &origin in origins {
        let isochrones = calculate_isochrones(graph, origin, options)?;
        all_isochrones.push(isochrones);
    }

    Ok(all_isochrones)
}

/// Calculate drive-time polygons (isochrones using travel time)
pub fn calculate_drive_time_polygons(
    graph: &Graph,
    origin: NodeId,
    time_breaks: &[f64], // time intervals in seconds
    speed_kmh: f64,
) -> Result<Vec<Isochrone>> {
    if graph.get_node(origin).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Origin node {:?} not found",
            origin
        )));
    }

    let max_time = time_breaks.iter().copied().fold(0.0f64, f64::max);

    let speed_ms = speed_kmh / 3.6; // Convert to m/s

    // Expand using travel time
    let mut arrival_times: HashMap<NodeId, f64> = HashMap::new();
    let mut heap = BinaryHeap::new();

    arrival_times.insert(origin, 0.0);
    heap.push(ServiceAreaState {
        cost: 0.0,
        node: origin,
    });

    while let Some(ServiceAreaState { cost, node }) = heap.pop() {
        if cost > max_time {
            continue;
        }

        if let Some(&best) = arrival_times.get(&node) {
            if cost > best {
                continue;
            }
        }

        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;

                // Use edge speed limit or default
                let edge_speed = edge.speed_limit.map(|s| s / 3.6).unwrap_or(speed_ms);

                // Calculate travel time for this edge
                let edge_distance = edge.weight; // Assuming weight is distance
                let travel_time = if edge_speed > 0.0 {
                    edge_distance / edge_speed
                } else {
                    edge_distance / speed_ms
                };

                let next_time = cost + travel_time;

                if next_time <= max_time {
                    let is_better = arrival_times
                        .get(&next_node)
                        .is_none_or(|&current| next_time < current);

                    if is_better {
                        arrival_times.insert(next_node, next_time);
                        heap.push(ServiceAreaState {
                            cost: next_time,
                            node: next_node,
                        });
                    }
                }
            }
        }
    }

    // Build isochrones for each time break
    let mut isochrones = Vec::new();

    for &time_break in time_breaks {
        let nodes: Vec<NodeId> = arrival_times
            .iter()
            .filter(|(_, t)| **t <= time_break)
            .map(|(n, _)| *n)
            .collect();

        let polygon = build_isochrone_polygon(graph, &nodes, 0.5)?;
        let area = polygon.as_ref().map(|p| compute_polygon_area(p));
        let perimeter = polygon.as_ref().map(|p| compute_polygon_perimeter(p));

        isochrones.push(Isochrone {
            value: time_break,
            polygon,
            nodes,
            area,
            perimeter,
        });
    }

    Ok(isochrones)
}

/// Build a polygon from isochrone nodes using convex hull
fn build_isochrone_polygon(
    graph: &Graph,
    nodes: &[NodeId],
    _smoothing_factor: f64,
) -> Result<Option<Polygon>> {
    if nodes.len() < 3 {
        return Ok(None);
    }

    // Get coordinates of all nodes
    let mut coords: Vec<Coordinate> = nodes
        .iter()
        .filter_map(|&node_id| graph.get_node(node_id).map(|n| n.coordinate))
        .collect();

    if coords.len() < 3 {
        return Ok(None);
    }

    // Compute convex hull as a simple polygon
    coords = convex_hull(&coords);

    if coords.len() < 3 {
        return Ok(None);
    }

    // Close the ring
    if let Some(first) = coords.first().copied() {
        coords.push(first);
    }

    // Create polygon
    let exterior = LineString::new(coords)
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid linestring: {}", e)))?;

    let polygon = Polygon::new(exterior, vec![])
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid polygon: {}", e)))?;

    Ok(Some(polygon))
}

/// Build a concave hull polygon (alpha shape approximation)
fn build_concave_hull_polygon(
    graph: &Graph,
    nodes: &[NodeId],
    alpha: f64,
) -> Result<Option<Polygon>> {
    if nodes.len() < 3 {
        return Ok(None);
    }

    let coords: Vec<Coordinate> = nodes
        .iter()
        .filter_map(|&node_id| graph.get_node(node_id).map(|n| n.coordinate))
        .collect();

    if coords.len() < 3 {
        return Ok(None);
    }

    // For small point sets, fall back to convex hull
    if coords.len() < 10 {
        return build_isochrone_polygon(graph, nodes, alpha);
    }

    // Alpha shape approximation:
    // Start with convex hull, then for each edge check if we can "carve in"
    // by finding interior points that form smaller triangles
    let hull = convex_hull(&coords);

    if hull.len() < 3 {
        return Ok(None);
    }

    // For a reasonable concave hull, we use an edge-refinement approach:
    // If an edge is too long relative to alpha*average_edge_length,
    // find the nearest point to the midpoint and insert it.
    let mut refined = hull.clone();
    let avg_edge_len = compute_average_edge_length(&refined);
    let threshold = avg_edge_len * (1.0 + alpha * 2.0);

    let coord_set: HashSet<u64> = coords.iter().map(|c| hash_coordinate(c)).collect();

    // Multiple refinement passes
    for _ in 0..3 {
        let mut new_refined = Vec::new();
        let mut changed = false;

        for i in 0..refined.len() {
            new_refined.push(refined[i]);
            let j = (i + 1) % refined.len();

            let edge_len = euclidean_dist(&refined[i], &refined[j]);
            if edge_len > threshold {
                // Find the nearest interior point to the midpoint
                let mid_x = (refined[i].x + refined[j].x) / 2.0;
                let mid_y = (refined[i].y + refined[j].y) / 2.0;
                let mid = Coordinate::new_2d(mid_x, mid_y);

                let nearest = coords
                    .iter()
                    .filter(|c| {
                        let h = hash_coordinate(c);
                        coord_set.contains(&h)
                            && !is_same_coord(c, &refined[i])
                            && !is_same_coord(c, &refined[j])
                    })
                    .min_by(|a, b| {
                        let da = euclidean_dist(a, &mid);
                        let db = euclidean_dist(b, &mid);
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    });

                if let Some(nearest_pt) = nearest {
                    let dist_to_mid = euclidean_dist(nearest_pt, &mid);
                    if dist_to_mid < edge_len / 2.0 {
                        new_refined.push(*nearest_pt);
                        changed = true;
                    }
                }
            }
        }

        refined = new_refined;
        if !changed {
            break;
        }
    }

    if refined.len() < 3 {
        return Ok(None);
    }

    // Close the ring
    if let Some(first) = refined.first().copied() {
        refined.push(first);
    }

    let exterior = LineString::new(refined)
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid linestring: {}", e)))?;

    let polygon = Polygon::new(exterior, vec![])
        .map_err(|e| AlgorithmError::InvalidGeometry(format!("Invalid polygon: {}", e)))?;

    Ok(Some(polygon))
}

/// Hash a coordinate for set membership testing
fn hash_coordinate(c: &Coordinate) -> u64 {
    let x_bits = c.x.to_bits();
    let y_bits = c.y.to_bits();
    x_bits.wrapping_mul(31).wrapping_add(y_bits)
}

/// Check if two coordinates are the same
fn is_same_coord(a: &Coordinate, b: &Coordinate) -> bool {
    (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12
}

/// Euclidean distance between two coordinates
fn euclidean_dist(a: &Coordinate, b: &Coordinate) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// Compute the average edge length of a polygon boundary
fn compute_average_edge_length(coords: &[Coordinate]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }

    let mut total = 0.0;
    let n = coords.len();
    for i in 0..n {
        let j = (i + 1) % n;
        total += euclidean_dist(&coords[i], &coords[j]);
    }

    total / n as f64
}

/// Compute convex hull using Graham scan
fn convex_hull(points: &[Coordinate]) -> Vec<Coordinate> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut points = points.to_vec();

    // Find the point with lowest y-coordinate (and leftmost if tie)
    let start_idx = points
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.y.partial_cmp(&b.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
        })
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    points.swap(0, start_idx);
    let start = points[0];

    // Sort by polar angle with respect to start point
    points[1..].sort_by(|a, b| {
        let angle_a = (a.y - start.y).atan2(a.x - start.x);
        let angle_b = (b.y - start.y).atan2(b.x - start.x);
        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Remove duplicates by angle (keep the farthest)
    let mut unique_points = vec![points[0]];
    for i in 1..points.len() {
        if unique_points.len() >= 2 {
            let last = unique_points[unique_points.len() - 1];
            let angle_last = (last.y - start.y).atan2(last.x - start.x);
            let angle_curr = (points[i].y - start.y).atan2(points[i].x - start.x);

            if (angle_last - angle_curr).abs() < 1e-12 {
                // Same angle - keep the farther one
                let dist_last = euclidean_dist(&start, &last);
                let dist_curr = euclidean_dist(&start, &points[i]);
                if dist_curr > dist_last {
                    unique_points.pop();
                } else {
                    continue;
                }
            }
        }
        unique_points.push(points[i]);
    }

    // Build convex hull
    let mut hull = Vec::new();
    for point in unique_points {
        while hull.len() >= 2 && !is_left_turn(&hull[hull.len() - 2], &hull[hull.len() - 1], &point)
        {
            hull.pop();
        }
        hull.push(point);
    }

    hull
}

/// Check if three points make a left turn (counter-clockwise)
fn is_left_turn(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate) -> bool {
    let cross = (p2.x - p1.x) * (p3.y - p1.y) - (p2.y - p1.y) * (p3.x - p1.x);
    cross > 0.0
}

/// Compute the area of a polygon using the shoelace formula
fn compute_polygon_area(polygon: &Polygon) -> f64 {
    let coords = &polygon.exterior.coords;
    if coords.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    let n = coords.len();

    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].x * coords[j].y;
        area -= coords[j].x * coords[i].y;
    }

    (area / 2.0).abs()
}

/// Compute the perimeter of a polygon
fn compute_polygon_perimeter(polygon: &Polygon) -> f64 {
    let coords = &polygon.exterior.coords;
    let mut perimeter = 0.0;

    for i in 0..coords.len().saturating_sub(1) {
        perimeter += euclidean_dist(&coords[i], &coords[i + 1]);
    }

    perimeter
}

/// Calculate accessibility score for a node
/// based on how many facilities/destinations can be reached within cost thresholds
pub fn accessibility_score(
    graph: &Graph,
    node: NodeId,
    destinations: &[NodeId],
    max_cost: f64,
) -> Result<AccessibilityResult> {
    if graph.get_node(node).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Node {:?} not found",
            node
        )));
    }

    let options = ShortestPathOptions {
        max_length: Some(max_cost),
        ..Default::default()
    };

    let sssp = crate::vector::network::dijkstra_single_source(graph, node, &options)?;

    let mut reachable_count = 0;
    let mut total_cost = 0.0;
    let mut destination_costs = Vec::new();

    for &dest in destinations {
        if let Some(&(cost, _)) = sssp.get(&dest) {
            if cost <= max_cost {
                reachable_count += 1;
                total_cost += cost;
                destination_costs.push((dest, cost));
            }
        }
    }

    let score = if destinations.is_empty() {
        0.0
    } else {
        reachable_count as f64 / destinations.len() as f64
    };

    let avg_cost = if reachable_count > 0 {
        total_cost / reachable_count as f64
    } else {
        f64::INFINITY
    };

    Ok(AccessibilityResult {
        node,
        score,
        reachable_count,
        total_destinations: destinations.len(),
        average_cost: avg_cost,
        destination_costs,
    })
}

/// Result of accessibility analysis
#[derive(Debug, Clone)]
pub struct AccessibilityResult {
    /// The analyzed node
    pub node: NodeId,
    /// Accessibility score (0.0 to 1.0, fraction of destinations reachable)
    pub score: f64,
    /// Number of reachable destinations
    pub reachable_count: usize,
    /// Total number of destinations
    pub total_destinations: usize,
    /// Average cost to reachable destinations
    pub average_cost: f64,
    /// Individual destination costs
    pub destination_costs: Vec<(NodeId, f64)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_graph() -> Graph {
        let mut graph = Graph::new();

        // Star pattern: center n0 connected to n1,n2,n3,n4
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(0.0, 1.0));
        let n3 = graph.add_node(Coordinate::new_2d(-1.0, 0.0));
        let n4 = graph.add_node(Coordinate::new_2d(0.0, -1.0));

        let _ = graph.add_edge(n0, n1, 100.0);
        let _ = graph.add_edge(n0, n2, 200.0);
        let _ = graph.add_edge(n0, n3, 300.0);
        let _ = graph.add_edge(n0, n4, 400.0);
        // Add some cross-edges
        let _ = graph.add_edge(n1, n2, 150.0);

        graph
    }

    #[test]
    fn test_service_area_options() {
        let options = ServiceAreaOptions::default();
        assert_eq!(options.max_cost, 1000.0);
        assert!(!options.intervals.is_empty());
    }

    #[test]
    fn test_isochrone_options() {
        let options = IsochroneOptions::default();
        assert!(options.smooth);
        assert_eq!(options.time_intervals.len(), 4);
    }

    #[test]
    fn test_service_area_basic() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let origin = nodes[0]; // Center node

        let options = ServiceAreaOptions {
            max_cost: 500.0,
            intervals: vec![100.0, 200.0, 300.0, 500.0],
            include_unreachable: false,
            cost_type: ServiceAreaCostType::Distance,
            weight_criteria: None,
        };

        let sa = calculate_service_area(&graph, origin, &options);
        assert!(sa.is_ok());

        let service_area = sa.expect("service area");
        assert!(!service_area.reachable_nodes.is_empty());

        // Check intervals are properly ordered
        for i in 1..service_area.intervals.len() {
            assert!(service_area.intervals[i].count >= service_area.intervals[i - 1].count);
        }
    }

    #[test]
    fn test_service_area_break_values() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let origin = nodes[0];

        let options = ServiceAreaOptions {
            max_cost: 500.0,
            intervals: vec![100.0, 250.0, 500.0],
            include_unreachable: false,
            cost_type: ServiceAreaCostType::Distance,
            weight_criteria: None,
        };

        let sa = calculate_service_area(&graph, origin, &options).expect("service area");

        assert_eq!(sa.intervals.len(), 3);
        // 100.0 interval: only n0 (cost 0) and n1 (cost 100)
        assert_eq!(sa.intervals[0].max_cost, 100.0);
        // 250.0 interval: also n2 (cost 200)
        assert!(sa.intervals[1].count >= sa.intervals[0].count);
    }

    #[test]
    fn test_multi_facility() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(3.0, 0.0));

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n2, n3, 1.0);
        let _ = graph.add_edge(n3, n2, 1.0);

        let options = ServiceAreaOptions {
            max_cost: 5.0,
            intervals: vec![1.0, 2.0, 5.0],
            ..Default::default()
        };

        let result = calculate_multi_facility_service_area(&graph, &[n0, n3], &options);
        assert!(result.is_ok());

        let mf = result.expect("multi-facility");
        assert_eq!(mf.facility_areas.len(), 2);

        // n1 should be nearest to n0 (cost 1)
        if let Some(&(facility, _cost)) = mf.nearest_facility.get(&n1) {
            assert_eq!(facility, n0);
        }
    }

    #[test]
    fn test_convex_hull() {
        let points = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.5, 0.5),
            Coordinate::new_2d(0.0, 1.0),
        ];

        let hull = convex_hull(&points);
        assert!(hull.len() >= 3);
    }

    #[test]
    fn test_is_left_turn() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(1.0, 0.0);
        let p3 = Coordinate::new_2d(0.5, 1.0);

        assert!(is_left_turn(&p1, &p2, &p3));
    }

    #[test]
    fn test_polygon_area() {
        // Unit square: area should be 1.0
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords).expect("linestring");
        let polygon = Polygon::new(exterior, vec![]).expect("polygon");
        let area = compute_polygon_area(&polygon);
        assert!((area - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_isochrone_generation() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let origin = nodes[0];

        let options = IsochroneOptions {
            time_intervals: vec![150.0, 300.0, 500.0],
            smooth: false,
            smoothing_factor: 0.5,
            polygon_method: IsochronePolygonMethod::ConvexHull,
        };

        let result = calculate_isochrones(&graph, origin, &options);
        assert!(result.is_ok());

        let isochrones = result.expect("isochrones");
        assert_eq!(isochrones.len(), 3);

        // Later intervals should have more or equal nodes
        for i in 1..isochrones.len() {
            assert!(isochrones[i].nodes.len() >= isochrones[i - 1].nodes.len());
        }
    }

    #[test]
    fn test_accessibility_score() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(10.0, 0.0)); // Unreachable within cost

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 1.0);

        let result = accessibility_score(&graph, n0, &[n1, n2, n3], 5.0);
        assert!(result.is_ok());

        let acc = result.expect("accessibility");
        assert_eq!(acc.reachable_count, 2); // n1 and n2 reachable
        assert_eq!(acc.total_destinations, 3);
        assert!((acc.score - 2.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_drive_time_polygons() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let origin = nodes[0];

        let result = calculate_drive_time_polygons(
            &graph,
            origin,
            &[60.0, 120.0, 300.0], // 1, 2, 5 minutes
            50.0,                  // 50 km/h
        );
        assert!(result.is_ok());

        let polygons = result.expect("drive time polygons");
        assert_eq!(polygons.len(), 3);
    }

    #[test]
    fn test_multi_isochrones() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();

        let options = IsochroneOptions {
            time_intervals: vec![200.0, 500.0],
            smooth: false,
            ..Default::default()
        };

        let result = calculate_multi_isochrones(&graph, &[nodes[0], nodes[3]], &options);
        assert!(result.is_ok());

        let all = result.expect("multi isochrones");
        assert_eq!(all.len(), 2); // One set per origin
    }

    #[test]
    fn test_overlap_zone_detection() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));

        // Both facilities can reach n1
        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n2, n1, 1.0);

        let options = ServiceAreaOptions {
            max_cost: 5.0,
            intervals: vec![2.0],
            ..Default::default()
        };

        let result = calculate_multi_facility_service_area(&graph, &[n0, n2], &options);
        assert!(result.is_ok());

        let mf = result.expect("multi-facility");
        // n1 should be in overlap
        assert!(!mf.overlap_zones.is_empty());
    }
}
