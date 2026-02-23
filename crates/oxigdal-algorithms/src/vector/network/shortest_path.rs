//! Shortest path algorithms for network analysis
//!
//! This module implements various shortest path finding algorithms optimized
//! for geospatial networks, including:
//!
//! - **Dijkstra**: Classic single-source shortest path
//! - **A***: Heuristic-guided search for faster point-to-point queries
//! - **Bidirectional Dijkstra**: Searches from both ends simultaneously
//! - **Turn-restricted routing**: Edge-based state expansion with turn penalties
//! - **Time-dependent routing**: Edge costs vary by departure time
//! - **Floyd-Warshall**: All-pairs shortest paths for small graphs

use crate::error::{AlgorithmError, Result};
use crate::vector::network::{EdgeId, Graph, NodeId, TimeDependentWeight, TurnPenalties};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Type alias for edge-based state in turn-restricted routing
/// Maps (NodeId, arriving EdgeId) to (predecessor NodeId, predecessor arriving EdgeId, used EdgeId)
type EdgePredecessorMap = HashMap<(NodeId, Option<EdgeId>), (NodeId, Option<EdgeId>, EdgeId)>;

/// Algorithm to use for pathfinding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathFindingAlgorithm {
    /// Dijkstra's algorithm (guarantees shortest path)
    Dijkstra,
    /// A* algorithm (heuristic-guided, faster but needs good heuristic)
    AStar,
    /// Bidirectional search (searches from both ends)
    Bidirectional,
    /// Dijkstra with turn restrictions (edge-based expansion)
    DijkstraTurnRestricted,
    /// A* with turn restrictions
    AStarTurnRestricted,
    /// Time-dependent Dijkstra
    TimeDependentDijkstra,
}

/// Options for shortest path computation
#[derive(Debug, Clone)]
pub struct ShortestPathOptions {
    /// Algorithm to use
    pub algorithm: PathFindingAlgorithm,
    /// Maximum path length (in weight units)
    pub max_length: Option<f64>,
    /// Whether to return the full path geometry
    pub include_geometry: bool,
    /// Heuristic weight for A* (1.0 = optimal, >1.0 = faster but suboptimal)
    pub heuristic_weight: f64,
    /// Turn penalties (optional)
    pub turn_penalties: Option<TurnPenalties>,
    /// Time-dependent weights per edge (optional)
    pub time_dependent_weights: Option<HashMap<EdgeId, TimeDependentWeight>>,
    /// Departure time (seconds since midnight) for time-dependent routing
    pub departure_time: f64,
    /// Weight criteria for multi-criteria routing (maps weight name to coefficient)
    pub weight_criteria: Option<HashMap<String, f64>>,
}

impl Default for ShortestPathOptions {
    fn default() -> Self {
        Self {
            algorithm: PathFindingAlgorithm::Dijkstra,
            max_length: None,
            include_geometry: true,
            heuristic_weight: 1.0,
            turn_penalties: None,
            time_dependent_weights: None,
            departure_time: 0.0,
            weight_criteria: None,
        }
    }
}

/// Result of a shortest path computation
#[derive(Debug, Clone)]
pub struct ShortestPath {
    /// Sequence of nodes in the path
    pub nodes: Vec<NodeId>,
    /// Sequence of edges in the path
    pub edges: Vec<EdgeId>,
    /// Total cost of the path
    pub cost: f64,
    /// Whether a path was found
    pub found: bool,
    /// Number of nodes visited during search
    pub nodes_visited: usize,
}

impl ShortestPath {
    /// Create a new empty path (not found)
    pub fn not_found() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            cost: f64::INFINITY,
            found: false,
            nodes_visited: 0,
        }
    }

    /// Create a path from node and edge sequences
    pub fn new(nodes: Vec<NodeId>, edges: Vec<EdgeId>, cost: f64) -> Self {
        Self {
            nodes,
            edges,
            cost,
            found: true,
            nodes_visited: 0,
        }
    }

    /// Set the number of nodes visited
    pub fn with_visited(mut self, count: usize) -> Self {
        self.nodes_visited = count;
        self
    }
}

/// State in the priority queue for Dijkstra's algorithm
#[derive(Debug, Clone)]
struct DijkstraState {
    cost: f64,
    node: NodeId,
}

impl PartialEq for DijkstraState {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.node == other.node
    }
}

impl Eq for DijkstraState {}

impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

/// Edge-based state for turn-restricted pathfinding
#[derive(Debug, Clone)]
struct EdgeState {
    cost: f64,
    node: NodeId,
    /// The edge used to arrive at this node (None for start node)
    arriving_edge: Option<EdgeId>,
}

impl PartialEq for EdgeState {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.node == other.node
    }
}

impl Eq for EdgeState {}

impl PartialOrd for EdgeState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EdgeState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

/// Find shortest path using Dijkstra's algorithm
///
/// # Arguments
///
/// * `graph` - The network graph
/// * `start` - Starting node
/// * `end` - Target node
/// * `options` - Path finding options
///
/// # Returns
///
/// The shortest path from start to end
pub fn dijkstra_search(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &ShortestPathOptions,
) -> Result<ShortestPath> {
    if graph.get_node(start).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Start node {:?} not found",
            start
        )));
    }

    if graph.get_node(end).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "End node {:?} not found",
            end
        )));
    }

    let mut distances: HashMap<NodeId, f64> = HashMap::new();
    let mut predecessors: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut visited_count: usize = 0;

    distances.insert(start, 0.0);
    heap.push(DijkstraState {
        cost: 0.0,
        node: start,
    });

    while let Some(DijkstraState { cost, node }) = heap.pop() {
        visited_count += 1;

        // Skip if we've already found a better path
        if let Some(&best_cost) = distances.get(&node) {
            if cost > best_cost {
                continue;
            }
        }

        // Check max length constraint
        if let Some(max_len) = options.max_length {
            if cost > max_len {
                continue;
            }
        }

        // Found target
        if node == end {
            let path = reconstruct_path(start, end, &predecessors, cost);
            return Ok(path.with_visited(visited_count));
        }

        // Explore neighbors
        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;
                let edge_cost = get_edge_cost(edge, options);
                let next_cost = cost + edge_cost;

                let is_better = distances
                    .get(&next_node)
                    .is_none_or(|&current| next_cost < current);

                if is_better {
                    distances.insert(next_node, next_cost);
                    predecessors.insert(next_node, (node, edge_id));
                    heap.push(DijkstraState {
                        cost: next_cost,
                        node: next_node,
                    });
                }
            }
        }
    }

    Ok(ShortestPath::not_found())
}

/// Find shortest path using Dijkstra with turn restrictions
///
/// Uses edge-based state expansion to support turn penalties and restrictions.
pub fn dijkstra_turn_restricted(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &ShortestPathOptions,
) -> Result<ShortestPath> {
    if graph.get_node(start).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Start node {:?} not found",
            start
        )));
    }
    if graph.get_node(end).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "End node {:?} not found",
            end
        )));
    }

    let turn_penalties = options.turn_penalties.as_ref().cloned().unwrap_or_default();

    // State: (node, arriving_edge) -> best cost
    let mut distances: HashMap<(NodeId, Option<EdgeId>), f64> = HashMap::new();
    let mut predecessors: EdgePredecessorMap = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut visited_count: usize = 0;

    distances.insert((start, None), 0.0);
    heap.push(EdgeState {
        cost: 0.0,
        node: start,
        arriving_edge: None,
    });

    let mut best_end_cost = f64::INFINITY;
    let mut best_end_arriving: Option<EdgeId> = None;

    while let Some(EdgeState {
        cost,
        node,
        arriving_edge,
    }) = heap.pop()
    {
        visited_count += 1;

        if node == end && cost < best_end_cost {
            best_end_cost = cost;
            best_end_arriving = arriving_edge;
        }

        if cost > best_end_cost {
            break; // All remaining states are worse
        }

        let state_key = (node, arriving_edge);
        if let Some(&best) = distances.get(&state_key) {
            if cost > best {
                continue;
            }
        }

        if let Some(max_len) = options.max_length {
            if cost > max_len {
                continue;
            }
        }

        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;

                // Check turn restriction
                let turn_cost = if let Some(from_edge) = arriving_edge {
                    let penalty = turn_penalties.get_penalty(node, from_edge, edge_id);
                    if penalty.is_infinite() {
                        continue; // Turn is prohibited
                    }
                    penalty
                } else {
                    0.0
                };

                let edge_cost = get_edge_cost(edge, options);
                let next_cost = cost + edge_cost + turn_cost;

                let next_key = (next_node, Some(edge_id));
                let is_better = distances
                    .get(&next_key)
                    .is_none_or(|&current| next_cost < current);

                if is_better {
                    distances.insert(next_key, next_cost);
                    predecessors.insert(next_key, (node, arriving_edge, edge_id));
                    heap.push(EdgeState {
                        cost: next_cost,
                        node: next_node,
                        arriving_edge: Some(edge_id),
                    });
                }
            }
        }
    }

    if best_end_cost < f64::INFINITY {
        // Reconstruct path from edge-based predecessors
        let path = reconstruct_edge_based_path(
            start,
            end,
            best_end_arriving,
            &predecessors,
            best_end_cost,
        );
        Ok(path.with_visited(visited_count))
    } else {
        Ok(ShortestPath::not_found())
    }
}

/// Reconstruct path from edge-based predecessors
fn reconstruct_edge_based_path(
    start: NodeId,
    end: NodeId,
    end_arriving: Option<EdgeId>,
    predecessors: &EdgePredecessorMap,
    cost: f64,
) -> ShortestPath {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let mut current_node = end;
    let mut current_arriving = end_arriving;

    nodes.push(current_node);

    while current_node != start || current_arriving.is_some() {
        let key = (current_node, current_arriving);
        if let Some(&(prev_node, prev_arriving, edge_id)) = predecessors.get(&key) {
            edges.push(edge_id);
            nodes.push(prev_node);
            current_node = prev_node;
            current_arriving = prev_arriving;
        } else {
            if current_node == start {
                break;
            }
            return ShortestPath::not_found();
        }
    }

    nodes.reverse();
    edges.reverse();

    ShortestPath::new(nodes, edges, cost)
}

/// Get the effective cost of traversing an edge given the options
fn get_edge_cost(edge: &crate::vector::network::Edge, options: &ShortestPathOptions) -> f64 {
    if let Some(ref criteria) = options.weight_criteria {
        edge.multi_weight.weighted_cost(criteria)
    } else {
        edge.weight
    }
}

/// Reconstruct path from predecessors map
fn reconstruct_path(
    start: NodeId,
    end: NodeId,
    predecessors: &HashMap<NodeId, (NodeId, EdgeId)>,
    cost: f64,
) -> ShortestPath {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut current = end;

    nodes.push(current);

    while current != start {
        if let Some(&(prev_node, edge_id)) = predecessors.get(&current) {
            edges.push(edge_id);
            nodes.push(prev_node);
            current = prev_node;
        } else {
            // Path reconstruction failed
            return ShortestPath::not_found();
        }
    }

    nodes.reverse();
    edges.reverse();

    ShortestPath::new(nodes, edges, cost)
}

/// Find shortest path using A* algorithm
///
/// Uses Euclidean distance as heuristic.
pub fn astar_search(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &ShortestPathOptions,
) -> Result<ShortestPath> {
    if graph.get_node(start).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Start node {:?} not found",
            start
        )));
    }

    let end_node = graph
        .get_node(end)
        .ok_or_else(|| AlgorithmError::InvalidGeometry(format!("End node {:?} not found", end)))?;

    let end_coord = &end_node.coordinate;

    let mut g_scores: HashMap<NodeId, f64> = HashMap::new();
    let mut predecessors: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut visited_count: usize = 0;

    g_scores.insert(start, 0.0);
    let h_start = heuristic(graph, start, end_coord) * options.heuristic_weight;

    heap.push(DijkstraState {
        cost: h_start,
        node: start,
    });

    while let Some(DijkstraState { cost: _f, node }) = heap.pop() {
        visited_count += 1;

        let current_g = g_scores.get(&node).copied().unwrap_or(f64::INFINITY);

        // Check max length constraint
        if let Some(max_len) = options.max_length {
            if current_g > max_len {
                continue;
            }
        }

        if node == end {
            let cost = g_scores.get(&end).copied().unwrap_or(f64::INFINITY);
            let path = reconstruct_path(start, end, &predecessors, cost);
            return Ok(path.with_visited(visited_count));
        }

        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;
                let edge_cost = get_edge_cost(edge, options);
                let tentative_g = current_g + edge_cost;

                let is_better = g_scores
                    .get(&next_node)
                    .is_none_or(|&current| tentative_g < current);

                if is_better {
                    g_scores.insert(next_node, tentative_g);
                    predecessors.insert(next_node, (node, edge_id));

                    let h = heuristic(graph, next_node, end_coord) * options.heuristic_weight;
                    let f = tentative_g + h;

                    heap.push(DijkstraState {
                        cost: f,
                        node: next_node,
                    });
                }
            }
        }
    }

    Ok(ShortestPath::not_found())
}

/// Find shortest path using A* with turn restrictions
pub fn astar_turn_restricted(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &ShortestPathOptions,
) -> Result<ShortestPath> {
    if graph.get_node(start).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Start node {:?} not found",
            start
        )));
    }

    let end_node = graph
        .get_node(end)
        .ok_or_else(|| AlgorithmError::InvalidGeometry(format!("End node {:?} not found", end)))?;
    let end_coord = end_node.coordinate;

    let turn_penalties = options.turn_penalties.as_ref().cloned().unwrap_or_default();

    // State: (node, arriving_edge) -> best g-score
    let mut g_scores: HashMap<(NodeId, Option<EdgeId>), f64> = HashMap::new();
    let mut predecessors: EdgePredecessorMap = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut visited_count: usize = 0;

    g_scores.insert((start, None), 0.0);
    let h_start = heuristic(graph, start, &end_coord) * options.heuristic_weight;
    heap.push(EdgeState {
        cost: h_start,
        node: start,
        arriving_edge: None,
    });

    let mut best_end_cost = f64::INFINITY;
    let mut best_end_arriving: Option<EdgeId> = None;

    while let Some(EdgeState {
        cost: _f,
        node,
        arriving_edge,
    }) = heap.pop()
    {
        visited_count += 1;

        let state_key = (node, arriving_edge);
        let current_g = g_scores.get(&state_key).copied().unwrap_or(f64::INFINITY);

        if node == end && current_g < best_end_cost {
            best_end_cost = current_g;
            best_end_arriving = arriving_edge;
        }

        if current_g > best_end_cost {
            break;
        }

        if let Some(max_len) = options.max_length {
            if current_g > max_len {
                continue;
            }
        }

        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;

                let turn_cost = if let Some(from_edge) = arriving_edge {
                    let penalty = turn_penalties.get_penalty(node, from_edge, edge_id);
                    if penalty.is_infinite() {
                        continue;
                    }
                    penalty
                } else {
                    0.0
                };

                let edge_cost = get_edge_cost(edge, options);
                let tentative_g = current_g + edge_cost + turn_cost;

                let next_key = (next_node, Some(edge_id));
                let is_better = g_scores
                    .get(&next_key)
                    .is_none_or(|&current| tentative_g < current);

                if is_better {
                    g_scores.insert(next_key, tentative_g);
                    predecessors.insert(next_key, (node, arriving_edge, edge_id));

                    let h = heuristic(graph, next_node, &end_coord) * options.heuristic_weight;
                    let f = tentative_g + h;

                    heap.push(EdgeState {
                        cost: f,
                        node: next_node,
                        arriving_edge: Some(edge_id),
                    });
                }
            }
        }
    }

    if best_end_cost < f64::INFINITY {
        let path = reconstruct_edge_based_path(
            start,
            end,
            best_end_arriving,
            &predecessors,
            best_end_cost,
        );
        Ok(path.with_visited(visited_count))
    } else {
        Ok(ShortestPath::not_found())
    }
}

/// Heuristic function for A* (Euclidean distance)
fn heuristic(graph: &Graph, node: NodeId, target_coord: &oxigdal_core::vector::Coordinate) -> f64 {
    if let Some(current_node) = graph.get_node(node) {
        let dx = current_node.coordinate.x - target_coord.x;
        let dy = current_node.coordinate.y - target_coord.y;
        (dx * dx + dy * dy).sqrt()
    } else {
        0.0
    }
}

/// Find shortest path using bidirectional Dijkstra
///
/// Searches from both start and end simultaneously, meeting in the middle.
/// Typically 2x faster than unidirectional Dijkstra for point-to-point queries.
pub fn bidirectional_search(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &ShortestPathOptions,
) -> Result<ShortestPath> {
    if graph.get_node(start).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Start node {:?} not found",
            start
        )));
    }

    if graph.get_node(end).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "End node {:?} not found",
            end
        )));
    }

    // Special case: start == end
    if start == end {
        return Ok(ShortestPath::new(vec![start], vec![], 0.0));
    }

    // Forward search from start
    let mut forward_distances: HashMap<NodeId, f64> = HashMap::new();
    let mut forward_predecessors: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();
    let mut forward_heap = BinaryHeap::new();
    let mut forward_settled: HashMap<NodeId, f64> = HashMap::new();

    // Backward search from end
    let mut backward_distances: HashMap<NodeId, f64> = HashMap::new();
    let mut backward_predecessors: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();
    let mut backward_heap = BinaryHeap::new();
    let mut backward_settled: HashMap<NodeId, f64> = HashMap::new();

    let mut visited_count: usize = 0;

    forward_distances.insert(start, 0.0);
    forward_heap.push(DijkstraState {
        cost: 0.0,
        node: start,
    });

    backward_distances.insert(end, 0.0);
    backward_heap.push(DijkstraState {
        cost: 0.0,
        node: end,
    });

    let mut best_cost = f64::INFINITY;
    let mut meeting_node: Option<NodeId> = None;

    // Alternating forward/backward expansion
    loop {
        let forward_min = forward_heap.peek().map(|s| s.cost).unwrap_or(f64::INFINITY);
        let backward_min = backward_heap
            .peek()
            .map(|s| s.cost)
            .unwrap_or(f64::INFINITY);

        // Stopping criterion: when the sum of minimum costs exceeds best_cost
        if forward_min + backward_min >= best_cost {
            break;
        }

        if forward_heap.is_empty() && backward_heap.is_empty() {
            break;
        }

        // Forward step
        if forward_min <= backward_min {
            if let Some(DijkstraState { cost, node }) = forward_heap.pop() {
                visited_count += 1;

                if let Some(&best) = forward_distances.get(&node) {
                    if cost > best {
                        continue;
                    }
                }

                forward_settled.insert(node, cost);

                // Check if this node was reached by backward search
                if let Some(&backward_cost) = backward_settled.get(&node) {
                    let total = cost + backward_cost;
                    if total < best_cost {
                        best_cost = total;
                        meeting_node = Some(node);
                    }
                }

                if let Some(max_len) = options.max_length {
                    if cost > max_len {
                        continue;
                    }
                }

                for &edge_id in graph.outgoing_edges(node) {
                    if let Some(edge) = graph.get_edge(edge_id) {
                        let next_node = edge.target;
                        let edge_cost = get_edge_cost(edge, options);
                        let next_cost = cost + edge_cost;

                        let is_better = forward_distances
                            .get(&next_node)
                            .is_none_or(|&current| next_cost < current);

                        if is_better {
                            forward_distances.insert(next_node, next_cost);
                            forward_predecessors.insert(next_node, (node, edge_id));
                            forward_heap.push(DijkstraState {
                                cost: next_cost,
                                node: next_node,
                            });

                            // Check meeting
                            if let Some(&bw_cost) = backward_settled.get(&next_node) {
                                let total = next_cost + bw_cost;
                                if total < best_cost {
                                    best_cost = total;
                                    meeting_node = Some(next_node);
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Backward step
            if let Some(DijkstraState { cost, node }) = backward_heap.pop() {
                visited_count += 1;

                if let Some(&best) = backward_distances.get(&node) {
                    if cost > best {
                        continue;
                    }
                }

                backward_settled.insert(node, cost);

                // Check meeting
                if let Some(&forward_cost) = forward_settled.get(&node) {
                    let total = forward_cost + cost;
                    if total < best_cost {
                        best_cost = total;
                        meeting_node = Some(node);
                    }
                }

                if let Some(max_len) = options.max_length {
                    if cost > max_len {
                        continue;
                    }
                }

                for &edge_id in graph.incoming_edges(node) {
                    if let Some(edge) = graph.get_edge(edge_id) {
                        let prev_node = edge.source;
                        let edge_cost = get_edge_cost(edge, options);
                        let prev_cost = cost + edge_cost;

                        let is_better = backward_distances
                            .get(&prev_node)
                            .is_none_or(|&current| prev_cost < current);

                        if is_better {
                            backward_distances.insert(prev_node, prev_cost);
                            backward_predecessors.insert(prev_node, (node, edge_id));
                            backward_heap.push(DijkstraState {
                                cost: prev_cost,
                                node: prev_node,
                            });

                            // Check meeting
                            if let Some(&fw_cost) = forward_settled.get(&prev_node) {
                                let total = fw_cost + prev_cost;
                                if total < best_cost {
                                    best_cost = total;
                                    meeting_node = Some(prev_node);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(meeting) = meeting_node {
        let path = reconstruct_bidirectional_path(
            start,
            end,
            meeting,
            &forward_predecessors,
            &backward_predecessors,
            best_cost,
        );
        Ok(path.with_visited(visited_count))
    } else {
        Ok(ShortestPath::not_found())
    }
}

/// Reconstruct path from bidirectional search
fn reconstruct_bidirectional_path(
    start: NodeId,
    end: NodeId,
    meeting: NodeId,
    forward_preds: &HashMap<NodeId, (NodeId, EdgeId)>,
    backward_preds: &HashMap<NodeId, (NodeId, EdgeId)>,
    cost: f64,
) -> ShortestPath {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Forward path (start to meeting)
    let mut current = meeting;
    let mut forward_nodes = vec![current];
    let mut forward_edges = Vec::new();

    while current != start {
        if let Some(&(prev_node, edge_id)) = forward_preds.get(&current) {
            forward_edges.push(edge_id);
            forward_nodes.push(prev_node);
            current = prev_node;
        } else {
            break;
        }
    }

    forward_nodes.reverse();
    forward_edges.reverse();

    nodes.extend(forward_nodes);
    edges.extend(forward_edges);

    // Backward path (meeting to end)
    current = meeting;
    while current != end {
        if let Some(&(next_node, edge_id)) = backward_preds.get(&current) {
            edges.push(edge_id);
            nodes.push(next_node);
            current = next_node;
        } else {
            break;
        }
    }

    ShortestPath::new(nodes, edges, cost)
}

/// Time-dependent Dijkstra search
///
/// Edge costs vary based on departure time. This models real-world scenarios
/// like rush-hour traffic, time-varying tolls, etc.
///
/// FIFO property must hold: departing later means arriving later on any edge.
pub fn time_dependent_dijkstra(
    graph: &Graph,
    start: NodeId,
    end: NodeId,
    options: &ShortestPathOptions,
) -> Result<ShortestPath> {
    if graph.get_node(start).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Start node {:?} not found",
            start
        )));
    }
    if graph.get_node(end).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "End node {:?} not found",
            end
        )));
    }

    let td_weights = options
        .time_dependent_weights
        .as_ref()
        .cloned()
        .unwrap_or_default();

    let departure_time = options.departure_time;

    // arrival_time[node] = earliest arrival time
    let mut arrival_times: HashMap<NodeId, f64> = HashMap::new();
    let mut predecessors: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let mut visited_count: usize = 0;

    arrival_times.insert(start, departure_time);
    heap.push(DijkstraState {
        cost: departure_time,
        node: start,
    });

    while let Some(DijkstraState {
        cost: current_time,
        node,
    }) = heap.pop()
    {
        visited_count += 1;

        if let Some(&best_time) = arrival_times.get(&node) {
            if current_time > best_time {
                continue;
            }
        }

        if let Some(max_len) = options.max_length {
            if current_time - departure_time > max_len {
                continue;
            }
        }

        if node == end {
            let total_cost = current_time - departure_time;
            let path = reconstruct_path(start, end, &predecessors, total_cost);
            return Ok(path.with_visited(visited_count));
        }

        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;

                // Get time-dependent multiplier
                let multiplier = td_weights
                    .get(&edge_id)
                    .map(|tdw| tdw.multiplier_at(current_time))
                    .unwrap_or(1.0);

                let travel_cost = edge.weight * multiplier;
                let arrival_time = current_time + travel_cost;

                let is_better = arrival_times
                    .get(&next_node)
                    .is_none_or(|&current| arrival_time < current);

                if is_better {
                    arrival_times.insert(next_node, arrival_time);
                    predecessors.insert(next_node, (node, edge_id));
                    heap.push(DijkstraState {
                        cost: arrival_time,
                        node: next_node,
                    });
                }
            }
        }
    }

    Ok(ShortestPath::not_found())
}

/// All-pairs shortest paths result
#[derive(Debug, Clone)]
pub struct AllPairsResult {
    /// Distance matrix: dist\[i\]\[j\] = shortest distance from node i to node j
    pub distances: HashMap<NodeId, HashMap<NodeId, f64>>,
    /// Next-hop matrix for path reconstruction: next\[i\]\[j\] = next node on shortest path from i to j
    pub next_hop: HashMap<NodeId, HashMap<NodeId, Option<NodeId>>>,
    /// Node ordering for matrix access
    pub node_order: Vec<NodeId>,
}

impl AllPairsResult {
    /// Get the shortest distance between two nodes
    pub fn distance(&self, from: NodeId, to: NodeId) -> f64 {
        self.distances
            .get(&from)
            .and_then(|row| row.get(&to))
            .copied()
            .unwrap_or(f64::INFINITY)
    }

    /// Reconstruct the shortest path between two nodes
    pub fn path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        if self.distance(from, to).is_infinite() {
            return None;
        }

        let mut path = vec![from];
        let mut current = from;

        while current != to {
            let next = self
                .next_hop
                .get(&current)
                .and_then(|row| row.get(&to))
                .copied()
                .flatten();

            match next {
                Some(n) => {
                    if path.contains(&n) {
                        return None; // Cycle detected
                    }
                    path.push(n);
                    current = n;
                }
                None => return None,
            }
        }

        Some(path)
    }
}

/// Floyd-Warshall all-pairs shortest paths
///
/// Computes shortest paths between all pairs of nodes.
/// Time complexity: O(V^3), so only suitable for small to medium graphs.
///
/// # Arguments
///
/// * `graph` - The network graph
/// * `max_nodes` - Maximum number of nodes (returns error if exceeded)
///
/// # Returns
///
/// All-pairs shortest path result with distance matrix and path reconstruction
pub fn floyd_warshall(graph: &Graph, max_nodes: usize) -> Result<AllPairsResult> {
    let n = graph.num_nodes();
    if n > max_nodes {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "max_nodes",
            message: format!(
                "Graph has {} nodes, exceeds limit of {} for Floyd-Warshall",
                n, max_nodes
            ),
        });
    }

    let node_order: Vec<NodeId> = graph.sorted_node_ids();
    let node_index: HashMap<NodeId, usize> = node_order
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    // Initialize distance and next-hop matrices
    let mut dist: Vec<Vec<f64>> = vec![vec![f64::INFINITY; n]; n];
    let mut next: Vec<Vec<Option<usize>>> = vec![vec![None; n]; n];

    // Self distances are 0
    for i in 0..n {
        dist[i][i] = 0.0;
        next[i][i] = Some(i);
    }

    // Initialize from edges
    for edge in graph.edges_iter().map(|(_, e)| e) {
        let src_idx = node_index.get(&edge.source).copied();
        let tgt_idx = node_index.get(&edge.target).copied();

        if let (Some(i), Some(j)) = (src_idx, tgt_idx) {
            if edge.weight < dist[i][j] {
                dist[i][j] = edge.weight;
                next[i][j] = Some(j);
            }
        }
    }

    // Floyd-Warshall main loop
    for k in 0..n {
        for i in 0..n {
            if dist[i][k] == f64::INFINITY {
                continue;
            }
            for j in 0..n {
                if dist[k][j] == f64::INFINITY {
                    continue;
                }
                let new_dist = dist[i][k] + dist[k][j];
                if new_dist < dist[i][j] {
                    dist[i][j] = new_dist;
                    next[i][j] = next[i][k];
                }
            }
        }
    }

    // Convert to HashMap-based result
    let mut distances: HashMap<NodeId, HashMap<NodeId, f64>> = HashMap::new();
    let mut next_hop: HashMap<NodeId, HashMap<NodeId, Option<NodeId>>> = HashMap::new();

    for i in 0..n {
        let from_id = node_order[i];
        let mut dist_row = HashMap::new();
        let mut next_row = HashMap::new();

        for j in 0..n {
            let to_id = node_order[j];
            dist_row.insert(to_id, dist[i][j]);
            next_row.insert(to_id, next[i][j].map(|idx| node_order[idx]));
        }

        distances.insert(from_id, dist_row);
        next_hop.insert(from_id, next_row);
    }

    Ok(AllPairsResult {
        distances,
        next_hop,
        node_order,
    })
}

/// Single-source shortest paths (Dijkstra from one source to all reachable nodes)
pub fn dijkstra_single_source(
    graph: &Graph,
    source: NodeId,
    options: &ShortestPathOptions,
) -> Result<HashMap<NodeId, (f64, Vec<NodeId>)>> {
    if graph.get_node(source).is_none() {
        return Err(AlgorithmError::InvalidGeometry(format!(
            "Source node {:?} not found",
            source
        )));
    }

    let mut distances: HashMap<NodeId, f64> = HashMap::new();
    let mut predecessors: HashMap<NodeId, NodeId> = HashMap::new();
    let mut heap = BinaryHeap::new();

    distances.insert(source, 0.0);
    heap.push(DijkstraState {
        cost: 0.0,
        node: source,
    });

    while let Some(DijkstraState { cost, node }) = heap.pop() {
        if let Some(&best) = distances.get(&node) {
            if cost > best {
                continue;
            }
        }

        if let Some(max_len) = options.max_length {
            if cost > max_len {
                continue;
            }
        }

        for &edge_id in graph.outgoing_edges(node) {
            if let Some(edge) = graph.get_edge(edge_id) {
                let next_node = edge.target;
                let edge_cost = get_edge_cost(edge, options);
                let next_cost = cost + edge_cost;

                let is_better = distances
                    .get(&next_node)
                    .is_none_or(|&current| next_cost < current);

                if is_better {
                    distances.insert(next_node, next_cost);
                    predecessors.insert(next_node, node);
                    heap.push(DijkstraState {
                        cost: next_cost,
                        node: next_node,
                    });
                }
            }
        }
    }

    // Build result with paths
    let mut result = HashMap::new();
    for (&node, &dist) in &distances {
        let mut path = vec![node];
        let mut current = node;
        while current != source {
            if let Some(&prev) = predecessors.get(&current) {
                path.push(prev);
                current = prev;
            } else {
                break;
            }
        }
        path.reverse();
        result.insert(node, (dist, path));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::network::TurnPenalties;
    use oxigdal_core::vector::Coordinate;

    fn create_test_graph() -> Graph {
        let mut graph = Graph::new();

        // Create a simple graph:
        //   0 -- 1 -- 2
        //   |         |
        //   3 ------- 4
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(0.0, 1.0));
        let n4 = graph.add_node(Coordinate::new_2d(2.0, 1.0));

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n0, n3, 1.0);
        let _ = graph.add_edge(n3, n4, 3.0);
        let _ = graph.add_edge(n2, n4, 1.0);

        graph
    }

    #[test]
    fn test_dijkstra_simple_path() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let start = nodes[0];
        let end = nodes[2];

        let path = dijkstra_search(&graph, start, end, &ShortestPathOptions::default());
        assert!(path.is_ok());

        let shortest = path.expect("Failed to find path");
        assert!(shortest.found);
        assert_eq!(shortest.cost, 2.0); // 0 -> 1 -> 2
    }

    #[test]
    fn test_dijkstra_no_path() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        // No edge between n1 and n2

        let path = dijkstra_search(&graph, n1, n2, &ShortestPathOptions::default());
        assert!(path.is_ok());

        let shortest = path.expect("Failed to search");
        assert!(!shortest.found);
    }

    #[test]
    fn test_astar_search() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let start = nodes[0];
        let end = nodes[4];

        let path = astar_search(&graph, start, end, &ShortestPathOptions::default());
        assert!(path.is_ok());

        let shortest = path.expect("Failed to find path");
        assert!(shortest.found);
    }

    #[test]
    fn test_bidirectional_search() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let start = nodes[0];
        let end = nodes[4];

        let path = bidirectional_search(&graph, start, end, &ShortestPathOptions::default());
        assert!(path.is_ok());

        let shortest = path.expect("Failed to find path");
        assert!(shortest.found);
    }

    #[test]
    fn test_max_length_constraint() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let start = nodes[0];
        let end = nodes[4];

        let options = ShortestPathOptions {
            max_length: Some(2.0), // Too short to reach end
            ..Default::default()
        };

        let path = dijkstra_search(&graph, start, end, &options);
        assert!(path.is_ok());

        let shortest = path.expect("Failed to search");
        assert!(!shortest.found);
    }

    #[test]
    fn test_turn_restricted_dijkstra() {
        let mut graph = Graph::new();

        // Create a graph where turn restriction forces a longer path
        //  n0 --e0--> n1 --e1--> n2
        //              |
        //             e2
        //              v
        //             n3

        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(1.0, 1.0));

        let e0 = graph.add_edge(n0, n1, 1.0).expect("edge");
        let e1 = graph.add_edge(n1, n2, 1.0).expect("edge");
        let e2 = graph.add_edge(n1, n3, 1.0).expect("edge");
        let _ = graph.add_edge(n3, n2, 2.0).expect("edge");

        // Without turn restriction: n0 -> n1 -> n2 (cost 2)
        let path = dijkstra_search(&graph, n0, n2, &ShortestPathOptions::default());
        assert!(path.is_ok());
        let shortest = path.expect("path");
        assert_eq!(shortest.cost, 2.0);

        // Prohibit turn from e0 to e1 at n1 (no left turn)
        let mut tp = TurnPenalties::new();
        tp.add_prohibition(n1, e0, e1);

        let options = ShortestPathOptions {
            turn_penalties: Some(tp),
            ..Default::default()
        };

        // With turn restriction: n0 -> n1 -> n3 -> n2 (cost 4)
        let path = dijkstra_turn_restricted(&graph, n0, n2, &options);
        assert!(path.is_ok());
        let shortest = path.expect("path");
        assert!(shortest.found);
        assert_eq!(shortest.cost, 4.0);
    }

    #[test]
    fn test_turn_penalty() {
        let mut graph = Graph::new();

        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));

        let e0 = graph.add_edge(n0, n1, 1.0).expect("edge");
        let e1 = graph.add_edge(n1, n2, 1.0).expect("edge");

        let mut tp = TurnPenalties::new();
        tp.add_penalty(n1, e0, e1, 5.0);

        let options = ShortestPathOptions {
            turn_penalties: Some(tp),
            ..Default::default()
        };

        let path = dijkstra_turn_restricted(&graph, n0, n2, &options);
        assert!(path.is_ok());
        let shortest = path.expect("path");
        assert!(shortest.found);
        // Cost: edge(0->1) + turn_penalty + edge(1->2) = 1 + 5 + 1 = 7
        assert!((shortest.cost - 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_time_dependent_dijkstra() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));

        let e0 = graph.add_edge(n0, n1, 100.0).expect("edge"); // base cost 100

        // Rush hour: multiplier 2.0 during 7-9 AM
        let mut td_weights = HashMap::new();
        td_weights.insert(
            e0,
            TimeDependentWeight::new(vec![
                (0.0, 1.0),
                (25200.0, 2.0), // 7:00 AM
                (32400.0, 1.0), // 9:00 AM
            ]),
        );

        // Departure at 8:00 AM (28800s) -> rush hour multiplier 2.0
        let options = ShortestPathOptions {
            time_dependent_weights: Some(td_weights.clone()),
            departure_time: 28800.0,
            ..Default::default()
        };

        let path = time_dependent_dijkstra(&graph, n0, n1, &options);
        assert!(path.is_ok());
        let shortest = path.expect("path");
        assert!(shortest.found);
        assert!((shortest.cost - 200.0).abs() < 1e-10); // 100 * 2.0

        // Departure at noon -> normal multiplier 1.0
        let options2 = ShortestPathOptions {
            time_dependent_weights: Some(td_weights),
            departure_time: 43200.0,
            ..Default::default()
        };

        let path2 = time_dependent_dijkstra(&graph, n0, n1, &options2);
        assert!(path2.is_ok());
        let shortest2 = path2.expect("path");
        assert!((shortest2.cost - 100.0).abs() < 1e-10); // 100 * 1.0
    }

    #[test]
    fn test_floyd_warshall() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 2.0);
        let _ = graph.add_edge(n0, n2, 5.0);

        let result = floyd_warshall(&graph, 100);
        assert!(result.is_ok());

        let apsp = result.expect("Floyd-Warshall failed");
        assert!((apsp.distance(n0, n2) - 3.0).abs() < 1e-10); // via n1: 1+2=3 < 5
        assert!((apsp.distance(n0, n1) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_floyd_warshall_path_reconstruction() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));

        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 2.0);
        let _ = graph.add_edge(n0, n2, 5.0);

        let apsp = floyd_warshall(&graph, 100).expect("Floyd-Warshall failed");

        let path = apsp.path(n0, n2);
        assert!(path.is_some());
        let path = path.expect("path");
        assert_eq!(path, vec![n0, n1, n2]);
    }

    #[test]
    fn test_floyd_warshall_too_large() {
        let mut graph = Graph::new();
        for i in 0..20 {
            graph.add_node(Coordinate::new_2d(i as f64, 0.0));
        }

        let result = floyd_warshall(&graph, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_single_source_dijkstra() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let start = nodes[0];

        let result = dijkstra_single_source(&graph, start, &ShortestPathOptions::default());
        assert!(result.is_ok());

        let distances = result.expect("Failed single-source Dijkstra");
        // Start node has distance 0
        assert!((distances.get(&start).expect("start").0 - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_astar_turn_restricted() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(1.0, 1.0));

        let e0 = graph.add_edge(n0, n1, 1.0).expect("edge");
        let e1 = graph.add_edge(n1, n2, 1.0).expect("edge");
        let _ = graph.add_edge(n1, n3, 1.0).expect("edge");
        let _ = graph.add_edge(n3, n2, 2.0).expect("edge");

        // Prohibit e0 -> e1 at n1
        let mut tp = TurnPenalties::new();
        tp.add_prohibition(n1, e0, e1);

        let options = ShortestPathOptions {
            turn_penalties: Some(tp),
            ..Default::default()
        };

        let path = astar_turn_restricted(&graph, n0, n2, &options);
        assert!(path.is_ok());
        let shortest = path.expect("path");
        assert!(shortest.found);
        // Must go n0->n1->n3->n2 = 1+1+2 = 4
        assert!((shortest.cost - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_nodes_visited_count() {
        let graph = create_test_graph();
        let nodes = graph.sorted_node_ids();
        let start = nodes[0];
        let end = nodes[4];

        let path = dijkstra_search(&graph, start, end, &ShortestPathOptions::default());
        let shortest = path.expect("path");
        assert!(shortest.nodes_visited > 0);
    }

    #[test]
    fn test_bidirectional_same_node() {
        let mut graph = Graph::new();
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));

        let path = bidirectional_search(&graph, n0, n0, &ShortestPathOptions::default());
        assert!(path.is_ok());
        let shortest = path.expect("path");
        assert!(shortest.found);
        assert_eq!(shortest.cost, 0.0);
    }
}
