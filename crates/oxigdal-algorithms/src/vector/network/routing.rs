//! Advanced routing with constraints, turn restrictions, and multi-criteria optimization
//!
//! This module provides:
//!
//! - **Multi-criteria routing**: Optimize for distance, time, cost, or custom weights
//! - **Alternative routes**: Generate multiple distinct route options
//! - **Route optimization**: Optimize waypoint ordering (TSP approximation)
//! - **Turn penalties**: Apply costs for turns at intersections
//! - **Via-point routing**: Route through mandatory intermediate points

use crate::error::{AlgorithmError, Result};
use crate::vector::network::{
    EdgeId, EdgeWeight, Graph, NodeId, ShortestPath, ShortestPathOptions, TurnPenalties,
    astar_search, dijkstra_search, dijkstra_turn_restricted,
};
use oxigdal_core::vector::Coordinate;
use std::collections::{HashMap, HashSet};

/// Routing algorithm variant
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingAlgorithm {
    /// Fastest route (minimum travel time)
    Fastest,
    /// Shortest route (minimum distance)
    Shortest,
    /// Most economical (minimum cost)
    Economical,
    /// Multi-criteria (weighted combination)
    MultiCriteria,
}

/// Turn restriction type
#[derive(Debug, Clone)]
pub struct TurnRestriction {
    /// Node where turn restriction applies
    pub node: NodeId,
    /// Incoming edge
    pub from_edge: EdgeId,
    /// Outgoing edge
    pub to_edge: EdgeId,
    /// Whether this turn is prohibited
    pub prohibited: bool,
}

/// Multi-criteria weight specification
#[derive(Debug, Clone)]
pub struct RoutingCriteria {
    /// Weight for distance (0.0 to 1.0)
    pub distance_weight: f64,
    /// Weight for travel time (0.0 to 1.0)
    pub time_weight: f64,
    /// Weight for monetary cost (0.0 to 1.0)
    pub monetary_weight: f64,
    /// Custom weights
    pub custom_weights: HashMap<String, f64>,
}

impl Default for RoutingCriteria {
    fn default() -> Self {
        Self {
            distance_weight: 1.0,
            time_weight: 0.0,
            monetary_weight: 0.0,
            custom_weights: HashMap::new(),
        }
    }
}

impl RoutingCriteria {
    /// Create criteria for shortest distance
    pub fn shortest() -> Self {
        Self {
            distance_weight: 1.0,
            time_weight: 0.0,
            monetary_weight: 0.0,
            custom_weights: HashMap::new(),
        }
    }

    /// Create criteria for fastest time
    pub fn fastest() -> Self {
        Self {
            distance_weight: 0.0,
            time_weight: 1.0,
            monetary_weight: 0.0,
            custom_weights: HashMap::new(),
        }
    }

    /// Create criteria for cheapest route
    pub fn cheapest() -> Self {
        Self {
            distance_weight: 0.0,
            time_weight: 0.0,
            monetary_weight: 1.0,
            custom_weights: HashMap::new(),
        }
    }

    /// Create balanced criteria
    pub fn balanced() -> Self {
        Self {
            distance_weight: 0.4,
            time_weight: 0.4,
            monetary_weight: 0.2,
            custom_weights: HashMap::new(),
        }
    }

    /// Convert to weight criteria map for ShortestPathOptions
    pub fn to_weight_criteria(&self) -> HashMap<String, f64> {
        let mut map = HashMap::new();
        if self.distance_weight > 0.0 {
            map.insert("distance".to_string(), self.distance_weight);
        }
        if self.time_weight > 0.0 {
            map.insert("time".to_string(), self.time_weight);
        }
        if self.monetary_weight > 0.0 {
            map.insert("monetary".to_string(), self.monetary_weight);
        }
        for (k, v) in &self.custom_weights {
            if *v > 0.0 {
                map.insert(k.clone(), *v);
            }
        }
        map
    }
}

/// Routing options
#[derive(Debug, Clone)]
pub struct RouteOptions {
    /// Routing algorithm
    pub algorithm: RoutingAlgorithm,
    /// Turn restrictions
    pub turn_restrictions: Vec<TurnRestriction>,
    /// Avoid toll roads
    pub avoid_tolls: bool,
    /// Avoid highways
    pub avoid_highways: bool,
    /// Maximum detour factor (e.g., 1.2 = 20% longer than straight line)
    pub max_detour_factor: Option<f64>,
    /// Multi-criteria routing weights
    pub criteria: RoutingCriteria,
    /// Turn penalties
    pub turn_penalties: Option<TurnPenalties>,
    /// Number of alternative routes to generate
    pub alternatives: usize,
    /// Alternative route overlap threshold (0.0 to 1.0, lower = more distinct)
    pub alternative_overlap_threshold: f64,
    /// Via points (mandatory intermediate stops, in order)
    pub via_points: Vec<NodeId>,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            algorithm: RoutingAlgorithm::Shortest,
            turn_restrictions: Vec::new(),
            avoid_tolls: false,
            avoid_highways: false,
            max_detour_factor: None,
            criteria: RoutingCriteria::default(),
            turn_penalties: None,
            alternatives: 0,
            alternative_overlap_threshold: 0.6,
            via_points: Vec::new(),
        }
    }
}

/// A route segment
#[derive(Debug, Clone)]
pub struct RouteSegment {
    /// Edge ID
    pub edge: EdgeId,
    /// Distance in this segment
    pub distance: f64,
    /// Travel time in this segment (seconds)
    pub travel_time: f64,
    /// Monetary cost for this segment
    pub monetary_cost: f64,
    /// Instructions (e.g., "Turn left on Main St")
    pub instruction: Option<String>,
    /// Turn angle at the end of this segment (degrees, 0=straight, positive=left, negative=right)
    pub turn_angle: Option<f64>,
}

/// A complete route
#[derive(Debug, Clone)]
pub struct Route {
    /// Sequence of segments
    pub segments: Vec<RouteSegment>,
    /// Total distance (meters)
    pub total_distance: f64,
    /// Total travel time (seconds)
    pub total_time: f64,
    /// Total monetary cost
    pub total_monetary_cost: f64,
    /// Sequence of node IDs
    pub node_sequence: Vec<NodeId>,
    /// Sequence of coordinates
    pub coordinates: Vec<Coordinate>,
    /// Whether this is the primary route
    pub is_primary: bool,
    /// Route quality score (lower is better for the given criteria)
    pub quality_score: f64,
}

impl Route {
    /// Create a new empty route
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            total_distance: 0.0,
            total_time: 0.0,
            total_monetary_cost: 0.0,
            node_sequence: Vec::new(),
            coordinates: Vec::new(),
            is_primary: true,
            quality_score: 0.0,
        }
    }

    /// Get the edge set for overlap comparison
    fn edge_set(&self) -> HashSet<EdgeId> {
        self.segments.iter().map(|s| s.edge).collect()
    }

    /// Calculate overlap ratio with another route
    pub fn overlap_ratio(&self, other: &Route) -> f64 {
        let self_edges = self.edge_set();
        let other_edges = other.edge_set();

        if self_edges.is_empty() || other_edges.is_empty() {
            return 0.0;
        }

        let intersection = self_edges.intersection(&other_edges).count();
        let min_size = self_edges.len().min(other_edges.len());

        if min_size == 0 {
            return 0.0;
        }

        intersection as f64 / min_size as f64
    }
}

impl Default for Route {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of multi-route calculation
#[derive(Debug, Clone)]
pub struct RouteResult {
    /// Primary (best) route
    pub primary: Route,
    /// Alternative routes (if requested)
    pub alternatives: Vec<Route>,
}

/// Calculate a route between two points
pub fn calculate_route(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &RouteOptions,
) -> Result<Route> {
    // Build path options from route options
    let path_options = build_path_options(options);

    // Handle via-points routing
    if !options.via_points.is_empty() {
        return calculate_via_route(graph, start, end, options);
    }

    // Find shortest path based on algorithm selection
    let shortest = if options.turn_penalties.is_some() || !options.turn_restrictions.is_empty() {
        let mut tp = options.turn_penalties.clone().unwrap_or_default();
        // Add turn restrictions to penalties
        for restriction in &options.turn_restrictions {
            if restriction.prohibited {
                tp.add_prohibition(restriction.node, restriction.from_edge, restriction.to_edge);
            }
        }
        let mut opts = path_options.clone();
        opts.turn_penalties = Some(tp);
        dijkstra_turn_restricted(graph, start, end, &opts)?
    } else {
        match options.algorithm {
            RoutingAlgorithm::Fastest | RoutingAlgorithm::MultiCriteria => {
                astar_search(graph, start, end, &path_options)?
            }
            _ => dijkstra_search(graph, start, end, &path_options)?,
        }
    };

    if !shortest.found {
        return Err(AlgorithmError::PathNotFound(format!(
            "No route found from {:?} to {:?}",
            start, end
        )));
    }

    build_route_from_path(graph, &shortest, true)
}

/// Calculate route through via-points
fn calculate_via_route(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &RouteOptions,
) -> Result<Route> {
    let mut full_route = Route::new();
    let path_options = build_path_options(options);

    // Build sequence: start -> via1 -> via2 -> ... -> end
    let mut waypoints = vec![start];
    waypoints.extend_from_slice(&options.via_points);
    waypoints.push(end);

    for window in waypoints.windows(2) {
        let from = window[0];
        let to = window[1];

        let shortest = dijkstra_search(graph, from, to, &path_options)?;

        if !shortest.found {
            return Err(AlgorithmError::PathNotFound(format!(
                "No route found from {:?} to {:?} (via-point segment)",
                from, to
            )));
        }

        let segment_route = build_route_from_path(graph, &shortest, false)?;

        // Merge into full route (avoid duplicating the connecting node)
        if full_route.node_sequence.is_empty() {
            full_route
                .node_sequence
                .extend(&segment_route.node_sequence);
        } else {
            // Skip first node of segment (it's the last of the previous)
            full_route
                .node_sequence
                .extend(&segment_route.node_sequence[1..]);
        }

        full_route.segments.extend(segment_route.segments);
        full_route.total_distance += segment_route.total_distance;
        full_route.total_time += segment_route.total_time;
        full_route.total_monetary_cost += segment_route.total_monetary_cost;
        full_route.coordinates.extend(segment_route.coordinates);
    }

    full_route.is_primary = true;
    full_route.quality_score = full_route.total_distance;

    Ok(full_route)
}

/// Build ShortestPathOptions from RouteOptions
fn build_path_options(options: &RouteOptions) -> ShortestPathOptions {
    let weight_criteria = match options.algorithm {
        RoutingAlgorithm::Fastest => Some(RoutingCriteria::fastest().to_weight_criteria()),
        RoutingAlgorithm::Shortest => Some(RoutingCriteria::shortest().to_weight_criteria()),
        RoutingAlgorithm::Economical => Some(RoutingCriteria::cheapest().to_weight_criteria()),
        RoutingAlgorithm::MultiCriteria => Some(options.criteria.to_weight_criteria()),
    };

    ShortestPathOptions {
        include_geometry: true,
        weight_criteria,
        turn_penalties: options.turn_penalties.clone(),
        ..Default::default()
    }
}

/// Build a Route from a ShortestPath result
fn build_route_from_path(graph: &Graph, path: &ShortestPath, is_primary: bool) -> Result<Route> {
    let mut route = Route::new();
    let mut coordinates = Vec::new();

    for (i, &edge_id) in path.edges.iter().enumerate() {
        if let Some(edge) = graph.get_edge(edge_id) {
            let distance = edge.multi_weight.distance;
            let time_cost = edge.multi_weight.time;
            let monetary = edge.multi_weight.monetary;

            let travel_time = if time_cost > 0.0 {
                time_cost
            } else {
                edge.travel_time()
                    .map(|t| t * 3600.0) // hours to seconds
                    .unwrap_or(distance / 13.89) // Default ~50 km/h
            };

            // Calculate turn angle between consecutive edges
            let turn_angle = if i > 0 {
                calculate_turn_angle(graph, path.edges[i - 1], edge_id)
            } else {
                None
            };

            let instruction = turn_angle.map(|angle| generate_turn_instruction(angle));

            let segment = RouteSegment {
                edge: edge_id,
                distance,
                travel_time,
                monetary_cost: monetary,
                instruction,
                turn_angle,
            };

            route.segments.push(segment);
            route.total_distance += distance;
            route.total_time += travel_time;
            route.total_monetary_cost += monetary;

            // Add geometry if available
            if let Some(geom) = &edge.geometry {
                coordinates.extend(geom.coords.iter().copied());
            }
        }
    }

    route.node_sequence = path.nodes.clone();
    route.coordinates = coordinates;
    route.is_primary = is_primary;
    route.quality_score = path.cost;

    Ok(route)
}

/// Calculate the turn angle between two consecutive edges
fn calculate_turn_angle(graph: &Graph, edge1_id: EdgeId, edge2_id: EdgeId) -> Option<f64> {
    let edge1 = graph.get_edge(edge1_id)?;
    let edge2 = graph.get_edge(edge2_id)?;

    let src1 = graph.get_node(edge1.source)?;
    let via = graph.get_node(edge1.target)?;
    let dst2 = graph.get_node(edge2.target)?;

    // Vector from src1 to via
    let v1x = via.coordinate.x - src1.coordinate.x;
    let v1y = via.coordinate.y - src1.coordinate.y;

    // Vector from via to dst2
    let v2x = dst2.coordinate.x - via.coordinate.x;
    let v2y = dst2.coordinate.y - via.coordinate.y;

    // Cross product (sin of angle)
    let cross = v1x * v2y - v1y * v2x;
    // Dot product (cos of angle)
    let dot = v1x * v2x + v1y * v2y;

    // Angle in degrees
    let angle = cross.atan2(dot).to_degrees();

    Some(angle)
}

/// Generate a human-readable turn instruction from angle
fn generate_turn_instruction(angle: f64) -> String {
    if angle.abs() < 15.0 {
        "Continue straight".to_string()
    } else if (15.0..60.0).contains(&angle) {
        "Bear left".to_string()
    } else if (60.0..120.0).contains(&angle) {
        "Turn left".to_string()
    } else if angle >= 120.0 {
        "Sharp left".to_string()
    } else if angle <= -15.0 && angle > -60.0 {
        "Bear right".to_string()
    } else if angle <= -60.0 && angle > -120.0 {
        "Turn right".to_string()
    } else {
        "Sharp right".to_string()
    }
}

/// Calculate route with alternative routes
pub fn calculate_route_with_alternatives(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &RouteOptions,
) -> Result<RouteResult> {
    // Calculate primary route
    let primary = calculate_route(graph, start, end, options)?;

    let mut alternatives = Vec::new();
    let num_alternatives = options.alternatives;

    if num_alternatives == 0 {
        return Ok(RouteResult {
            primary,
            alternatives,
        });
    }

    // Generate alternatives using penalty method
    // For each alternative, penalize edges used in previous routes
    let mut penalized_edges: HashMap<EdgeId, f64> = HashMap::new();

    // Penalize edges from the primary route
    for segment in &primary.segments {
        let current = penalized_edges.entry(segment.edge).or_insert(1.0);
        *current *= 2.0; // Double the effective cost
    }

    let path_options = build_path_options(options);

    for alt_idx in 0..num_alternatives {
        // Create a modified graph with penalized edge weights
        let mut alt_graph = graph.clone();

        for (&edge_id, &penalty_multiplier) in &penalized_edges {
            if let Some(edge) = alt_graph.get_edge_mut(edge_id) {
                edge.weight *= penalty_multiplier;
                edge.multi_weight.distance *= penalty_multiplier;
                edge.multi_weight.time *= penalty_multiplier;
            }
        }

        let shortest = dijkstra_search(&alt_graph, start, end, &path_options)?;

        if !shortest.found {
            continue;
        }

        // Build route using original graph weights for accurate costs
        let alt_route_result = build_alternative_route(graph, &shortest);
        let alt_route = match alt_route_result {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Check overlap with existing routes
        let overlap_with_primary = alt_route.overlap_ratio(&primary);
        let overlap_with_others = alternatives
            .iter()
            .map(|other: &Route| alt_route.overlap_ratio(other))
            .fold(0.0f64, |a, b| a.max(b));

        let max_overlap = overlap_with_primary.max(overlap_with_others);

        if max_overlap < options.alternative_overlap_threshold {
            // Add penalties for this route's edges too
            for segment in &alt_route.segments {
                let current = penalized_edges.entry(segment.edge).or_insert(1.0);
                *current *= 1.5;
            }

            alternatives.push(alt_route);
        } else {
            // Increase penalties more aggressively
            for segment in &alt_route.segments {
                let current = penalized_edges.entry(segment.edge).or_insert(1.0);
                *current *= 3.0;
            }
        }

        if alternatives.len() >= num_alternatives {
            break;
        }

        // Safety: prevent infinite attempts
        if alt_idx > num_alternatives * 5 {
            break;
        }
    }

    // Sort alternatives by quality score
    alternatives.sort_by(|a, b| {
        a.quality_score
            .partial_cmp(&b.quality_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(RouteResult {
        primary,
        alternatives,
    })
}

/// Build an alternative route with accurate costs from the original graph
fn build_alternative_route(graph: &Graph, path: &ShortestPath) -> Result<Route> {
    let mut route = Route::new();
    let mut coordinates = Vec::new();
    let mut actual_cost = 0.0;

    for (i, &edge_id) in path.edges.iter().enumerate() {
        if let Some(edge) = graph.get_edge(edge_id) {
            let distance = edge.multi_weight.distance;
            let time_cost = edge.multi_weight.time;
            let monetary = edge.multi_weight.monetary;

            let travel_time = if time_cost > 0.0 {
                time_cost
            } else {
                edge.travel_time()
                    .map(|t| t * 3600.0)
                    .unwrap_or(distance / 13.89)
            };

            actual_cost += edge.weight; // Use original weight

            let turn_angle = if i > 0 {
                calculate_turn_angle(graph, path.edges[i - 1], edge_id)
            } else {
                None
            };

            let instruction = turn_angle.map(|angle| generate_turn_instruction(angle));

            let segment = RouteSegment {
                edge: edge_id,
                distance,
                travel_time,
                monetary_cost: monetary,
                instruction,
                turn_angle,
            };

            route.segments.push(segment);
            route.total_distance += distance;
            route.total_time += travel_time;
            route.total_monetary_cost += monetary;

            if let Some(geom) = &edge.geometry {
                coordinates.extend(geom.coords.iter().copied());
            }
        }
    }

    route.node_sequence = path.nodes.clone();
    route.coordinates = coordinates;
    route.is_primary = false;
    route.quality_score = actual_cost;

    Ok(route)
}

/// Optimize the order of waypoints to minimize total route cost
/// Uses nearest-neighbor heuristic followed by 2-opt improvement
///
/// # Arguments
///
/// * `graph` - The network graph
/// * `waypoints` - List of waypoints to visit (order will be optimized)
/// * `start` - Fixed starting point
/// * `end` - Fixed ending point (can be same as start for round-trip)
/// * `options` - Route options
///
/// # Returns
///
/// Optimized ordering of waypoints and the total route
pub fn optimize_waypoint_order(
    graph: &Graph,
    waypoints: &[NodeId],
    start: NodeId,
    end: Option<NodeId>,
    options: &RouteOptions,
) -> Result<WaypointOptimizationResult> {
    if waypoints.is_empty() {
        return Ok(WaypointOptimizationResult {
            optimized_order: Vec::new(),
            total_cost: 0.0,
            route: Route::new(),
        });
    }

    if waypoints.len() == 1 {
        let mut opts = options.clone();
        opts.via_points = waypoints.to_vec();
        let end_node = end.unwrap_or(start);
        let route = calculate_route(graph, start, end_node, &opts)?;
        return Ok(WaypointOptimizationResult {
            optimized_order: waypoints.to_vec(),
            total_cost: route.total_distance,
            route,
        });
    }

    let path_options = build_path_options(options);

    // Pre-compute cost matrix between all relevant nodes
    let mut all_points = vec![start];
    all_points.extend_from_slice(waypoints);
    if let Some(e) = end {
        if !all_points.contains(&e) {
            all_points.push(e);
        }
    }

    let mut cost_matrix: HashMap<(NodeId, NodeId), f64> = HashMap::new();

    for &from in &all_points {
        for &to in &all_points {
            if from == to {
                cost_matrix.insert((from, to), 0.0);
                continue;
            }

            let result = dijkstra_search(graph, from, to, &path_options);
            let cost = match result {
                Ok(path) if path.found => path.cost,
                _ => f64::MAX / 2.0, // Very high cost for unreachable pairs
            };
            cost_matrix.insert((from, to), cost);
        }
    }

    // Nearest-neighbor heuristic
    let mut order = Vec::new();
    let mut remaining: HashSet<NodeId> = waypoints.iter().copied().collect();
    let mut current = start;

    while !remaining.is_empty() {
        let nearest = remaining
            .iter()
            .min_by(|&&a, &&b| {
                let cost_a = cost_matrix.get(&(current, a)).copied().unwrap_or(f64::MAX);
                let cost_b = cost_matrix.get(&(current, b)).copied().unwrap_or(f64::MAX);
                cost_a
                    .partial_cmp(&cost_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied();

        if let Some(next) = nearest {
            order.push(next);
            remaining.remove(&next);
            current = next;
        } else {
            break;
        }
    }

    // 2-opt improvement
    let improved_order = two_opt_improve(&order, start, end, &cost_matrix);

    // Calculate total cost of optimized route
    let total_cost = calculate_order_cost(&improved_order, start, end, &cost_matrix);

    // Build the actual route with optimized order
    let mut opts = options.clone();
    opts.via_points = improved_order.clone();
    let end_node = end.unwrap_or(start);
    let route = calculate_route(graph, start, end_node, &opts)?;

    Ok(WaypointOptimizationResult {
        optimized_order: improved_order,
        total_cost,
        route,
    })
}

/// 2-opt improvement for waypoint ordering
fn two_opt_improve(
    order: &[NodeId],
    start: NodeId,
    end: Option<NodeId>,
    cost_matrix: &HashMap<(NodeId, NodeId), f64>,
) -> Vec<NodeId> {
    let mut best_order = order.to_vec();
    let mut best_cost = calculate_order_cost(&best_order, start, end, cost_matrix);
    let mut improved = true;

    while improved {
        improved = false;

        for i in 0..best_order.len().saturating_sub(1) {
            for j in (i + 1)..best_order.len() {
                // Reverse the segment between i and j
                let mut new_order = best_order.clone();
                new_order[i..=j].reverse();

                let new_cost = calculate_order_cost(&new_order, start, end, cost_matrix);

                if new_cost < best_cost - 1e-10 {
                    best_order = new_order;
                    best_cost = new_cost;
                    improved = true;
                }
            }
        }
    }

    best_order
}

/// Calculate total cost for a given waypoint order
fn calculate_order_cost(
    order: &[NodeId],
    start: NodeId,
    end: Option<NodeId>,
    cost_matrix: &HashMap<(NodeId, NodeId), f64>,
) -> f64 {
    let mut total = 0.0;
    let mut current = start;

    for &next in order {
        total += cost_matrix
            .get(&(current, next))
            .copied()
            .unwrap_or(f64::MAX / 2.0);
        current = next;
    }

    if let Some(end_node) = end {
        total += cost_matrix
            .get(&(current, end_node))
            .copied()
            .unwrap_or(f64::MAX / 2.0);
    }

    total
}

/// Result of waypoint order optimization
#[derive(Debug, Clone)]
pub struct WaypointOptimizationResult {
    /// Optimized ordering of waypoints
    pub optimized_order: Vec<NodeId>,
    /// Total route cost
    pub total_cost: f64,
    /// The complete route following the optimized order
    pub route: Route,
}

/// Calculate multiple routes in batch
pub fn calculate_routes_batch(
    graph: &Graph,
    pairs: &[(NodeId, NodeId)],
    options: &RouteOptions,
) -> Result<Vec<Route>> {
    pairs
        .iter()
        .map(|(start, end)| calculate_route(graph, *start, *end, options))
        .collect()
}

/// Origin-destination matrix computation
///
/// Computes shortest path costs between all origin-destination pairs
pub fn od_matrix(
    graph: &Graph,
    origins: &[NodeId],
    destinations: &[NodeId],
    options: &RouteOptions,
) -> Result<Vec<Vec<f64>>> {
    let path_options = build_path_options(options);

    let mut matrix = Vec::with_capacity(origins.len());

    for &origin in origins {
        let mut row = Vec::with_capacity(destinations.len());

        // Use single-source Dijkstra for efficiency
        let sssp = crate::vector::network::dijkstra_single_source(graph, origin, &path_options)?;

        for &dest in destinations {
            let cost = sssp.get(&dest).map(|(c, _)| *c).unwrap_or(f64::INFINITY);
            row.push(cost);
        }

        matrix.push(row);
    }

    Ok(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_routing_graph() -> Graph {
        let mut graph = Graph::new();

        // Simple road network:
        //  n0 --1-- n1 --1-- n2
        //   |               / |
        //   2             1   1
        //   |           /     |
        //  n3 --3-- n4 --1-- n5
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(0.0, 1.0));
        let n4 = graph.add_node(Coordinate::new_2d(1.0, 1.0));
        let n5 = graph.add_node(Coordinate::new_2d(2.0, 1.0));

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n0, n3, 2.0);
        let _ = graph.add_edge(n3, n4, 3.0);
        let _ = graph.add_edge(n4, n2, 1.0);
        let _ = graph.add_edge(n4, n5, 1.0);
        let _ = graph.add_edge(n2, n5, 1.0);

        graph
    }

    #[test]
    fn test_route_creation() {
        let route = Route::new();
        assert_eq!(route.segments.len(), 0);
        assert_eq!(route.total_distance, 0.0);
    }

    #[test]
    fn test_route_options() {
        let options = RouteOptions::default();
        assert_eq!(options.algorithm, RoutingAlgorithm::Shortest);
        assert!(!options.avoid_tolls);
    }

    #[test]
    fn test_calculate_route() {
        let graph = create_routing_graph();
        let nodes = graph.sorted_node_ids();

        let route = calculate_route(&graph, nodes[0], nodes[5], &RouteOptions::default());
        assert!(route.is_ok());

        let r = route.expect("route");
        assert!(!r.segments.is_empty());
        assert!(r.total_distance > 0.0);
    }

    #[test]
    fn test_routing_criteria() {
        let fastest = RoutingCriteria::fastest();
        let criteria_map = fastest.to_weight_criteria();
        assert!(criteria_map.contains_key("time"));
        assert!(!criteria_map.contains_key("distance"));
    }

    #[test]
    fn test_via_point_routing() {
        let graph = create_routing_graph();
        let nodes = graph.sorted_node_ids();

        let mut options = RouteOptions::default();
        options.via_points = vec![nodes[1]]; // Route through node 1

        let route = calculate_route(&graph, nodes[0], nodes[5], &options);
        assert!(route.is_ok());

        let r = route.expect("route");
        assert!(r.node_sequence.contains(&nodes[1]));
    }

    #[test]
    fn test_route_overlap() {
        let mut route1 = Route::new();
        route1.segments.push(RouteSegment {
            edge: EdgeId(0),
            distance: 1.0,
            travel_time: 1.0,
            monetary_cost: 0.0,
            instruction: None,
            turn_angle: None,
        });
        route1.segments.push(RouteSegment {
            edge: EdgeId(1),
            distance: 1.0,
            travel_time: 1.0,
            monetary_cost: 0.0,
            instruction: None,
            turn_angle: None,
        });

        let mut route2 = Route::new();
        route2.segments.push(RouteSegment {
            edge: EdgeId(0),
            distance: 1.0,
            travel_time: 1.0,
            monetary_cost: 0.0,
            instruction: None,
            turn_angle: None,
        });
        route2.segments.push(RouteSegment {
            edge: EdgeId(2),
            distance: 1.0,
            travel_time: 1.0,
            monetary_cost: 0.0,
            instruction: None,
            turn_angle: None,
        });

        let overlap = route1.overlap_ratio(&route2);
        assert!((overlap - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_turn_instruction() {
        assert_eq!(generate_turn_instruction(0.0), "Continue straight");
        assert_eq!(generate_turn_instruction(90.0), "Turn left");
        assert_eq!(generate_turn_instruction(-90.0), "Turn right");
        assert_eq!(generate_turn_instruction(30.0), "Bear left");
        assert_eq!(generate_turn_instruction(-30.0), "Bear right");
        assert_eq!(generate_turn_instruction(150.0), "Sharp left");
        assert_eq!(generate_turn_instruction(-150.0), "Sharp right");
    }

    #[test]
    fn test_batch_routes() {
        let graph = create_routing_graph();
        let nodes = graph.sorted_node_ids();

        let pairs = vec![(nodes[0], nodes[5]), (nodes[0], nodes[2])];
        let result = calculate_routes_batch(&graph, &pairs, &RouteOptions::default());
        assert!(result.is_ok());

        let routes = result.expect("batch routes");
        assert_eq!(routes.len(), 2);
    }

    #[test]
    fn test_od_matrix() {
        let graph = create_routing_graph();
        let nodes = graph.sorted_node_ids();

        let origins = vec![nodes[0], nodes[3]];
        let destinations = vec![nodes[2], nodes[5]];

        let result = od_matrix(&graph, &origins, &destinations, &RouteOptions::default());
        assert!(result.is_ok());

        let matrix = result.expect("od matrix");
        assert_eq!(matrix.len(), 2);
        assert_eq!(matrix[0].len(), 2);
        // From n0 to n2 should be 2.0 (n0->n1->n2)
        assert!((matrix[0][0] - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_waypoint_optimization() {
        let mut graph = Graph::new();

        // Create a line graph: n0 -- n1 -- n2 -- n3 -- n4
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(3.0, 0.0));
        let n4 = graph.add_node(Coordinate::new_2d(4.0, 0.0));

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n2, n3, 1.0);
        let _ = graph.add_edge(n3, n4, 1.0);
        // Also add reverse edges for flexibility
        let _ = graph.add_edge(n1, n0, 1.0);
        let _ = graph.add_edge(n2, n1, 1.0);
        let _ = graph.add_edge(n3, n2, 1.0);
        let _ = graph.add_edge(n4, n3, 1.0);

        // Optimize visiting n3, n1 from n0 to n4
        // Natural order would be n0 -> n1 -> n3 -> n4 (cost 4)
        // Reverse n3, n1 would be n0 -> n3 -> n1 -> n4 (cost 3+2+3=8, worse)
        let result =
            optimize_waypoint_order(&graph, &[n3, n1], n0, Some(n4), &RouteOptions::default());
        assert!(result.is_ok());

        let opt = result.expect("optimization");
        // Should optimize to [n1, n3] order
        assert_eq!(opt.optimized_order, vec![n1, n3]);
    }
}
