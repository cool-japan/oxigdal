//! Graph operation types and auxiliary structures
//!
//! This module contains validation, topology cleaning results, time-dependent
//! weight modeling, turn penalty management, and rich network node/edge types.

use super::graph::{Edge, EdgeId, Graph, GraphType, Node, NodeId};
use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::Coordinate;
use std::collections::{HashMap, HashSet, VecDeque};

/// Result of graph validation containing all detected issues
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the graph is valid (no critical issues)
    pub is_valid: bool,
    /// List of validation issues
    pub issues: Vec<ValidationIssue>,
}

/// A single validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Severity of the issue
    pub severity: ValidationSeverity,
    /// Description of the issue
    pub description: String,
}

/// Severity level for validation issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Error: graph is structurally invalid
    Error,
    /// Warning: graph may have problems
    Warning,
    /// Info: informational note
    Info,
}

/// Connected component information
#[derive(Debug, Clone)]
pub struct ConnectedComponent {
    /// Nodes in this component
    pub nodes: Vec<NodeId>,
    /// Number of edges in this component
    pub edge_count: usize,
}

/// Result of topology cleaning
#[derive(Debug, Clone)]
pub struct TopologyCleanResult {
    /// Number of nodes that were snapped together
    pub nodes_snapped: usize,
    /// Number of self-loops removed
    pub self_loops_removed: usize,
    /// Number of parallel edges removed
    pub parallel_edges_removed: usize,
    /// Number of isolated nodes removed
    pub isolated_nodes_removed: usize,
    /// Number of degree-2 nodes contracted
    pub degree2_nodes_contracted: usize,
}

/// A node in a network (with richer semantics)
#[derive(Debug, Clone)]
pub struct NetworkNode {
    /// Base node
    pub node: Node,
    /// Node type (e.g., "intersection", "dead_end")
    pub node_type: String,
    /// Elevation (optional)
    pub elevation: Option<f64>,
}

/// An edge in a network (with richer semantics)
#[derive(Debug, Clone)]
pub struct NetworkEdge {
    /// Base edge
    pub edge: Edge,
    /// Road type (e.g., "highway", "residential")
    pub road_type: String,
    /// One-way restriction
    pub one_way: bool,
    /// Surface type (e.g., "paved", "gravel")
    pub surface: Option<String>,
}

/// Time-dependent weight function
///
/// Maps a time-of-day (in seconds since midnight, 0..86400)
/// to an edge weight multiplier.
#[derive(Debug, Clone)]
pub struct TimeDependentWeight {
    /// Time intervals (start_time, multiplier) sorted by start_time
    /// The multiplier applies from start_time until the next interval
    pub intervals: Vec<(f64, f64)>,
}

impl TimeDependentWeight {
    /// Create a constant (time-independent) weight
    pub fn constant() -> Self {
        Self {
            intervals: vec![(0.0, 1.0)],
        }
    }

    /// Create from interval data
    pub fn new(mut intervals: Vec<(f64, f64)>) -> Self {
        intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if intervals.is_empty() {
            intervals.push((0.0, 1.0));
        }
        Self { intervals }
    }

    /// Get the weight multiplier at a given time (seconds since midnight)
    pub fn multiplier_at(&self, time: f64) -> f64 {
        // Normalize to 0..86400
        let time = time % 86400.0;

        // Binary search for the interval containing this time
        let mut result = self.intervals[0].1;
        for &(start, mult) in &self.intervals {
            if time >= start {
                result = mult;
            } else {
                break;
            }
        }
        result
    }

    /// Create a typical rush-hour pattern
    pub fn rush_hour() -> Self {
        Self::new(vec![
            (0.0, 0.8),     // Night: low traffic
            (25200.0, 1.5), // 7:00 AM: morning rush
            (32400.0, 1.0), // 9:00 AM: normal
            (61200.0, 1.5), // 5:00 PM: evening rush
            (68400.0, 1.0), // 7:00 PM: normal
            (79200.0, 0.8), // 10:00 PM: low traffic
        ])
    }
}

/// Turn penalty specification between two edges meeting at a node
#[derive(Debug, Clone)]
pub struct TurnPenalty {
    /// The node where the turn occurs
    pub via_node: NodeId,
    /// The incoming edge
    pub from_edge: EdgeId,
    /// The outgoing edge
    pub to_edge: EdgeId,
    /// Additional cost for making this turn
    pub penalty: f64,
}

/// Collection of turn penalties for a graph
#[derive(Debug, Clone, Default)]
pub struct TurnPenalties {
    /// Map from (via_node, from_edge, to_edge) to penalty
    penalties: HashMap<(NodeId, EdgeId, EdgeId), f64>,
    /// Set of prohibited turns (infinite penalty)
    prohibited: HashSet<(NodeId, EdgeId, EdgeId)>,
}

impl TurnPenalties {
    /// Create empty turn penalties
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a turn penalty
    pub fn add_penalty(
        &mut self,
        via_node: NodeId,
        from_edge: EdgeId,
        to_edge: EdgeId,
        penalty: f64,
    ) {
        self.penalties
            .insert((via_node, from_edge, to_edge), penalty);
    }

    /// Add a prohibited turn (infinite penalty)
    pub fn add_prohibition(&mut self, via_node: NodeId, from_edge: EdgeId, to_edge: EdgeId) {
        self.prohibited.insert((via_node, from_edge, to_edge));
    }

    /// Get the penalty for a turn, or 0.0 if no penalty
    pub fn get_penalty(&self, via_node: NodeId, from_edge: EdgeId, to_edge: EdgeId) -> f64 {
        if self.prohibited.contains(&(via_node, from_edge, to_edge)) {
            return f64::INFINITY;
        }
        self.penalties
            .get(&(via_node, from_edge, to_edge))
            .copied()
            .unwrap_or(0.0)
    }

    /// Check if a turn is prohibited
    pub fn is_prohibited(&self, via_node: NodeId, from_edge: EdgeId, to_edge: EdgeId) -> bool {
        self.prohibited.contains(&(via_node, from_edge, to_edge))
    }

    /// Number of penalties registered
    pub fn len(&self) -> usize {
        self.penalties.len() + self.prohibited.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.penalties.is_empty() && self.prohibited.is_empty()
    }
}

/// Haversine distance between two geographic coordinates (in meters)
pub fn haversine_distance(a: &Coordinate, b: &Coordinate) -> f64 {
    const EARTH_RADIUS: f64 = 6_371_000.0; // meters

    let lat1 = a.y.to_radians();
    let lat2 = b.y.to_radians();
    let dlat = (b.y - a.y).to_radians();
    let dlon = (b.x - a.x).to_radians();

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);

    2.0 * EARTH_RADIUS * h.sqrt().asin()
}

// ---- Graph validation and analysis methods ----

impl Graph {
    /// Comprehensive graph validation
    pub fn validate(&self) -> Result<()> {
        let result = self.validate_detailed();
        if result.is_valid {
            Ok(())
        } else {
            let errors: Vec<String> = result
                .issues
                .iter()
                .filter(|i| i.severity == ValidationSeverity::Error)
                .map(|i| i.description.clone())
                .collect();
            Err(AlgorithmError::InvalidGeometry(errors.join("; ")))
        }
    }

    /// Detailed graph validation returning all issues
    pub fn validate_detailed(&self) -> ValidationResult {
        let mut issues = Vec::new();

        // Check that all edges reference valid nodes
        for (edge_id, edge) in self.edges_iter() {
            if !self.has_node(edge.source) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    description: format!(
                        "Edge {:?} references non-existent source node {:?}",
                        edge_id, edge.source
                    ),
                });
            }

            if !self.has_node(edge.target) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    description: format!(
                        "Edge {:?} references non-existent target node {:?}",
                        edge_id, edge.target
                    ),
                });
            }

            if edge.is_self_loop() {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    description: format!(
                        "Edge {:?} is a self-loop at node {:?}",
                        edge_id, edge.source
                    ),
                });
            }

            if edge.weight < 0.0 {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    description: format!("Edge {:?} has negative weight {}", edge_id, edge.weight),
                });
            }

            if edge.weight.is_nan() || edge.weight.is_infinite() {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    description: format!("Edge {:?} has invalid weight (NaN or infinite)", edge_id),
                });
            }
        }

        // Check for isolated nodes
        let isolated_count = self
            .node_ids()
            .iter()
            .filter(|node_id| {
                self.outgoing_edges(**node_id).is_empty()
                    && self.incoming_edges(**node_id).is_empty()
            })
            .count();

        if isolated_count > 0 {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Info,
                description: format!("Graph contains {} isolated node(s)", isolated_count),
            });
        }

        // Check for parallel edges
        let mut edge_pairs: HashMap<(NodeId, NodeId), Vec<EdgeId>> = HashMap::new();
        for (&edge_id, edge) in self.edges_iter() {
            let key = if self.graph_type() == GraphType::Undirected {
                if edge.source.0 <= edge.target.0 {
                    (edge.source, edge.target)
                } else {
                    (edge.target, edge.source)
                }
            } else {
                (edge.source, edge.target)
            };
            edge_pairs.entry(key).or_default().push(edge_id);
        }
        for ((src, tgt), ids) in &edge_pairs {
            if ids.len() > 1 {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    description: format!(
                        "Parallel edges detected between {:?} and {:?}: {:?}",
                        src, tgt, ids
                    ),
                });
            }
        }

        let is_valid = !issues
            .iter()
            .any(|i| i.severity == ValidationSeverity::Error);

        ValidationResult { is_valid, issues }
    }

    /// Check if the graph is connected (weakly for directed graphs)
    pub fn is_connected(&self) -> bool {
        if self.num_nodes() == 0 {
            return true;
        }

        let components = self.connected_components();
        components.len() <= 1
    }

    /// Find all connected components (weakly connected for directed graphs)
    pub fn connected_components(&self) -> Vec<ConnectedComponent> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut components = Vec::new();

        for &node_id in &self.node_ids() {
            if visited.contains(&node_id) {
                continue;
            }

            let mut component_nodes = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(node_id);
            visited.insert(node_id);

            while let Some(current) = queue.pop_front() {
                component_nodes.push(current);

                for &edge_id in self.outgoing_edges(current) {
                    if let Some(edge) = self.get_edge(edge_id) {
                        let neighbor = if self.graph_type() == GraphType::Undirected {
                            edge.other_node(current)
                        } else {
                            Some(edge.target)
                        };
                        if let Some(n) = neighbor {
                            if !visited.contains(&n) {
                                visited.insert(n);
                                queue.push_back(n);
                            }
                        }
                    }
                }

                if self.graph_type() == GraphType::Directed {
                    for &edge_id in self.incoming_edges(current) {
                        if let Some(edge) = self.get_edge(edge_id) {
                            if !visited.contains(&edge.source) {
                                visited.insert(edge.source);
                                queue.push_back(edge.source);
                            }
                        }
                    }
                }
            }

            let component_node_set: HashSet<NodeId> = component_nodes.iter().copied().collect();
            let edge_count = self
                .edges_iter()
                .filter(|(_, e)| {
                    component_node_set.contains(&e.source) && component_node_set.contains(&e.target)
                })
                .count();

            components.push(ConnectedComponent {
                nodes: component_nodes,
                edge_count,
            });
        }

        components
    }

    /// Find strongly connected components using Tarjan's algorithm (directed graphs only)
    pub fn strongly_connected_components(&self) -> Vec<Vec<NodeId>> {
        if self.graph_type() == GraphType::Undirected {
            return self
                .connected_components()
                .into_iter()
                .map(|c| c.nodes)
                .collect();
        }

        let mut index_counter: usize = 0;
        let mut stack: Vec<NodeId> = Vec::new();
        let mut on_stack: HashSet<NodeId> = HashSet::new();
        let mut index_map: HashMap<NodeId, usize> = HashMap::new();
        let mut lowlink: HashMap<NodeId, usize> = HashMap::new();
        let mut sccs: Vec<Vec<NodeId>> = Vec::new();

        let node_ids: Vec<NodeId> = self.node_ids();

        for node in &node_ids {
            if !index_map.contains_key(node) {
                tarjan_dfs(
                    self,
                    *node,
                    &mut index_counter,
                    &mut stack,
                    &mut on_stack,
                    &mut index_map,
                    &mut lowlink,
                    &mut sccs,
                );
            }
        }

        sccs
    }

    // ---- Topology cleaning operations ----

    /// Remove all isolated nodes (nodes with no edges)
    pub fn remove_isolated_nodes(&mut self) -> Vec<NodeId> {
        let isolated: Vec<NodeId> = self
            .node_ids()
            .into_iter()
            .filter(|node_id| {
                self.outgoing_edges(*node_id).is_empty() && self.incoming_edges(*node_id).is_empty()
            })
            .collect();

        for node_id in &isolated {
            self.remove_node_unchecked(*node_id);
        }

        isolated
    }

    /// Remove self-loops
    pub fn remove_self_loops(&mut self) -> Vec<EdgeId> {
        let self_loops: Vec<EdgeId> = self
            .edges_iter()
            .filter(|(_, e)| e.is_self_loop())
            .map(|(&id, _)| id)
            .collect();

        let mut removed = Vec::new();
        for edge_id in self_loops {
            if self.remove_edge(edge_id).is_ok() {
                removed.push(edge_id);
            }
        }

        removed
    }

    /// Remove duplicate (parallel) edges, keeping the one with minimum weight
    pub fn remove_parallel_edges(&mut self) -> Vec<EdgeId> {
        let mut edge_groups: HashMap<(NodeId, NodeId), Vec<(EdgeId, f64)>> = HashMap::new();

        for (&edge_id, edge) in self.edges_iter() {
            let key = if self.graph_type() == GraphType::Undirected {
                if edge.source.0 <= edge.target.0 {
                    (edge.source, edge.target)
                } else {
                    (edge.target, edge.source)
                }
            } else {
                (edge.source, edge.target)
            };
            edge_groups
                .entry(key)
                .or_default()
                .push((edge_id, edge.weight));
        }

        let mut removed = Vec::new();
        for (_key, mut group) in edge_groups {
            if group.len() <= 1 {
                continue;
            }
            group.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            for &(edge_id, _) in &group[1..] {
                if self.remove_edge(edge_id).is_ok() {
                    removed.push(edge_id);
                }
            }
        }

        removed
    }

    /// Contract degree-2 nodes (nodes with exactly one incoming and one outgoing edge)
    pub fn contract_degree2_nodes(&mut self) -> usize {
        let mut contracted = 0;

        loop {
            let candidate = self.node_ids().into_iter().find(|&node_id| {
                let out_edges = self.outgoing_edges(node_id);
                let in_edges = self.incoming_edges(node_id);

                if self.graph_type() == GraphType::Undirected {
                    out_edges.len() == 2
                } else {
                    out_edges.len() == 1
                        && in_edges.len() == 1
                        && self
                            .get_edge(out_edges[0])
                            .map(|e| e.target)
                            .unwrap_or(node_id)
                            != self
                                .get_edge(in_edges[0])
                                .map(|e| e.source)
                                .unwrap_or(node_id)
                }
            });

            let node_id = match candidate {
                Some(n) => n,
                None => break,
            };

            if self.graph_type() == GraphType::Directed {
                let in_edges = self.incoming_edges(node_id).to_vec();
                let out_edges = self.outgoing_edges(node_id).to_vec();

                if in_edges.len() != 1 || out_edges.len() != 1 {
                    break;
                }

                let in_edge = match self.get_edge(in_edges[0]) {
                    Some(e) => e.clone(),
                    None => break,
                };
                let out_edge = match self.get_edge(out_edges[0]) {
                    Some(e) => e.clone(),
                    None => break,
                };

                let new_weight = in_edge.weight + out_edge.weight;
                let source = in_edge.source;
                let target = out_edge.target;

                let _ = self.remove_edge(in_edges[0]);
                let _ = self.remove_edge(out_edges[0]);
                let _ = self.remove_node(node_id);
                let _ = self.add_edge(source, target, new_weight);
                contracted += 1;
            } else {
                let adj_edges = self.outgoing_edges(node_id).to_vec();
                if adj_edges.len() != 2 {
                    break;
                }

                let e0 = match self.get_edge(adj_edges[0]) {
                    Some(e) => e.clone(),
                    None => break,
                };
                let e1 = match self.get_edge(adj_edges[1]) {
                    Some(e) => e.clone(),
                    None => break,
                };

                let n0 = e0.other_node(node_id).unwrap_or(node_id);
                let n1 = e1.other_node(node_id).unwrap_or(node_id);
                let new_weight = e0.weight + e1.weight;

                let _ = self.remove_edge(adj_edges[0]);
                let _ = self.remove_edge(adj_edges[1]);
                let _ = self.remove_node(node_id);
                let _ = self.add_edge(n0, n1, new_weight);
                contracted += 1;
            }
        }

        contracted
    }

    /// Snap close nodes together within a tolerance
    pub fn snap_nodes(&mut self, tolerance: f64) -> usize {
        let node_ids: Vec<NodeId> = self.node_ids();
        let mut merge_map: HashMap<NodeId, NodeId> = HashMap::new();
        let mut merged_count = 0;

        for i in 0..node_ids.len() {
            if merge_map.contains_key(&node_ids[i]) {
                continue;
            }

            let coord_i = match self.get_node(node_ids[i]) {
                Some(n) => n.coordinate,
                None => continue,
            };

            for j in (i + 1)..node_ids.len() {
                if merge_map.contains_key(&node_ids[j]) {
                    continue;
                }

                let coord_j = match self.get_node(node_ids[j]) {
                    Some(n) => n.coordinate,
                    None => continue,
                };

                let dx = coord_i.x - coord_j.x;
                let dy = coord_i.y - coord_j.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < tolerance {
                    merge_map.insert(node_ids[j], node_ids[i]);
                    merged_count += 1;
                }
            }
        }

        // Remap edges
        let edge_ids: Vec<EdgeId> = self.edge_ids();
        for edge_id in edge_ids {
            let edge = match self.get_edge(edge_id) {
                Some(e) => e.clone(),
                None => continue,
            };

            let new_source = merge_map.get(&edge.source).copied().unwrap_or(edge.source);
            let new_target = merge_map.get(&edge.target).copied().unwrap_or(edge.target);

            if new_source != edge.source || new_target != edge.target {
                let _ = self.remove_edge(edge_id);
                if new_source != new_target {
                    let _ = self.add_edge(new_source, new_target, edge.weight);
                }
            }
        }

        // Remove merged nodes
        for old_id in merge_map.keys() {
            self.remove_node_unchecked(*old_id);
        }

        merged_count
    }

    /// Clean topology by performing multiple operations
    pub fn clean_topology(&mut self, tolerance: f64) -> TopologyCleanResult {
        let snapped = self.snap_nodes(tolerance);
        let self_loops = self.remove_self_loops();
        let parallel = self.remove_parallel_edges();
        let isolated = self.remove_isolated_nodes();
        let contracted = self.contract_degree2_nodes();

        TopologyCleanResult {
            nodes_snapped: snapped,
            self_loops_removed: self_loops.len(),
            parallel_edges_removed: parallel.len(),
            isolated_nodes_removed: isolated.len(),
            degree2_nodes_contracted: contracted,
        }
    }
}

/// Tarjan DFS helper for strongly connected components
fn tarjan_dfs(
    graph: &Graph,
    v: NodeId,
    index_counter: &mut usize,
    stack: &mut Vec<NodeId>,
    on_stack: &mut HashSet<NodeId>,
    index_map: &mut HashMap<NodeId, usize>,
    lowlink: &mut HashMap<NodeId, usize>,
    sccs: &mut Vec<Vec<NodeId>>,
) {
    index_map.insert(v, *index_counter);
    lowlink.insert(v, *index_counter);
    *index_counter += 1;
    stack.push(v);
    on_stack.insert(v);

    for &edge_id in graph.outgoing_edges(v) {
        if let Some(edge) = graph.get_edge(edge_id) {
            let w = edge.target;
            if !index_map.contains_key(&w) {
                tarjan_dfs(
                    graph,
                    w,
                    index_counter,
                    stack,
                    on_stack,
                    index_map,
                    lowlink,
                    sccs,
                );
                let w_low = lowlink.get(&w).copied().unwrap_or(0);
                let v_low = lowlink.get(&v).copied().unwrap_or(0);
                lowlink.insert(v, v_low.min(w_low));
            } else if on_stack.contains(&w) {
                let w_idx = index_map.get(&w).copied().unwrap_or(0);
                let v_low = lowlink.get(&v).copied().unwrap_or(0);
                lowlink.insert(v, v_low.min(w_idx));
            }
        }
    }

    let v_low = lowlink.get(&v).copied().unwrap_or(0);
    let v_idx = index_map.get(&v).copied().unwrap_or(0);
    if v_low == v_idx {
        let mut scc = Vec::new();
        while let Some(w) = stack.pop() {
            on_stack.remove(&w);
            scc.push(w);
            if w == v {
                break;
            }
        }
        sccs.push(scc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::network::{EdgeWeight, GraphBuilder, GraphType};
    use oxigdal_core::vector::LineString;

    #[test]
    fn test_validate_graph() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn test_validate_detailed() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_node(Coordinate::new_2d(5.0, 5.0)); // Isolated
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n1, n1, 0.5); // Self-loop

        let result = graph.validate_detailed();
        assert!(result.is_valid);
        assert!(!result.issues.is_empty());
    }

    #[test]
    fn test_connected_components() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(5.0, 5.0));
        let n4 = graph.add_node(Coordinate::new_2d(6.0, 5.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n3, n4, 1.0);
        let components = graph.connected_components();
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_strongly_connected_components() {
        let mut graph = Graph::with_type(GraphType::Directed);
        let n0 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n1 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let _ = graph.add_edge(n0, n1, 1.0);
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n2, n0, 1.0);
        let sccs = graph.strongly_connected_components();
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), 3);
    }

    #[test]
    fn test_remove_self_loops() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_edge(n1, n1, 0.5);
        let _ = graph.add_edge(n1, n2, 1.0);
        assert_eq!(graph.num_edges(), 2);
        let removed = graph.remove_self_loops();
        assert_eq!(removed.len(), 1);
        assert_eq!(graph.num_edges(), 1);
    }

    #[test]
    fn test_remove_isolated_nodes() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_node(Coordinate::new_2d(5.0, 5.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        assert_eq!(graph.num_nodes(), 3);
        let removed = graph.remove_isolated_nodes();
        assert_eq!(removed.len(), 1);
        assert_eq!(graph.num_nodes(), 2);
    }

    #[test]
    fn test_clean_topology() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_node(Coordinate::new_2d(100.0, 100.0));
        let _ = graph.add_edge(n1, n1, 0.5);
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n1, n2, 2.0);
        let result = graph.clean_topology(0.0);
        assert_eq!(result.self_loops_removed, 1);
        assert_eq!(result.parallel_edges_removed, 1);
        assert_eq!(result.isolated_nodes_removed, 1);
    }

    #[test]
    fn test_time_dependent_weight() {
        let tdw = TimeDependentWeight::rush_hour();
        assert!((tdw.multiplier_at(3600.0) - 0.8).abs() < 1e-10);
        assert!((tdw.multiplier_at(28800.0) - 1.5).abs() < 1e-10);
        assert!((tdw.multiplier_at(43200.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_turn_penalties() {
        let mut tp = TurnPenalties::new();
        let node = NodeId(1);
        let from = EdgeId(0);
        let to = EdgeId(2);
        tp.add_penalty(node, from, to, 5.0);
        assert!((tp.get_penalty(node, from, to) - 5.0).abs() < 1e-10);
        assert!((tp.get_penalty(node, from, EdgeId(3)) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_turn_prohibition() {
        let mut tp = TurnPenalties::new();
        let node = NodeId(1);
        let from = EdgeId(0);
        let to = EdgeId(2);
        tp.add_prohibition(node, from, to);
        assert!(tp.is_prohibited(node, from, to));
        assert!(tp.get_penalty(node, from, to).is_infinite());
    }

    #[test]
    fn test_haversine_distance() {
        let london = Coordinate::new_2d(-0.1278, 51.5074);
        let paris = Coordinate::new_2d(2.3522, 48.8566);
        let dist = haversine_distance(&london, &paris);
        assert!(dist > 300_000.0 && dist < 400_000.0);
    }
}
