//! Graph data structures for network analysis
//!
//! This module provides efficient graph representations optimized for
//! geospatial network analysis, supporting both directed and undirected graphs
//! with multi-criteria edge weights.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString};
use std::collections::HashMap;

/// Unique identifier for a node in the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

/// Unique identifier for an edge in the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(pub usize);

/// Specifies whether the graph is directed or undirected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphType {
    /// Directed graph: edges have a specific direction
    Directed,
    /// Undirected graph: edges can be traversed in both directions
    Undirected,
}

/// Multi-criteria edge weight supporting distance, time, and custom costs
#[derive(Debug, Clone)]
pub struct EdgeWeight {
    /// Distance cost (e.g., meters)
    pub distance: f64,
    /// Time cost (e.g., seconds)
    pub time: f64,
    /// Monetary cost (e.g., toll fees)
    pub monetary: f64,
    /// Custom weight dimensions (named)
    pub custom: HashMap<String, f64>,
}

impl EdgeWeight {
    /// Create a simple weight with only distance
    pub fn from_distance(distance: f64) -> Self {
        Self {
            distance,
            time: 0.0,
            monetary: 0.0,
            custom: HashMap::new(),
        }
    }

    /// Create a weight with distance and time
    pub fn from_distance_time(distance: f64, time: f64) -> Self {
        Self {
            distance,
            time,
            monetary: 0.0,
            custom: HashMap::new(),
        }
    }

    /// Create a weight with all three primary dimensions
    pub fn new(distance: f64, time: f64, monetary: f64) -> Self {
        Self {
            distance,
            time,
            monetary,
            custom: HashMap::new(),
        }
    }

    /// Add a custom weight dimension
    pub fn with_custom(mut self, name: String, value: f64) -> Self {
        self.custom.insert(name, value);
        self
    }

    /// Compute a weighted combination of all costs
    ///
    /// `criteria` maps weight names to their multiplier coefficients.
    /// Built-in names: "distance", "time", "monetary".
    pub fn weighted_cost(&self, criteria: &HashMap<String, f64>) -> f64 {
        let mut total = 0.0;
        if let Some(&w) = criteria.get("distance") {
            total += self.distance * w;
        }
        if let Some(&w) = criteria.get("time") {
            total += self.time * w;
        }
        if let Some(&w) = criteria.get("monetary") {
            total += self.monetary * w;
        }
        for (name, &value) in &self.custom {
            if let Some(&w) = criteria.get(name.as_str()) {
                total += value * w;
            }
        }
        total
    }

    /// Get the primary weight (distance by default)
    pub fn primary(&self) -> f64 {
        self.distance
    }
}

impl Default for EdgeWeight {
    fn default() -> Self {
        Self {
            distance: 0.0,
            time: 0.0,
            monetary: 0.0,
            custom: HashMap::new(),
        }
    }
}

/// A node in the network graph
#[derive(Debug, Clone)]
pub struct Node {
    /// Unique identifier
    pub id: NodeId,
    /// Geographic coordinate
    pub coordinate: Coordinate,
    /// Incident edges (incoming and outgoing)
    pub edges: Vec<EdgeId>,
    /// Custom attributes
    pub attributes: HashMap<String, String>,
}

impl Node {
    /// Create a new node
    pub fn new(id: NodeId, coordinate: Coordinate) -> Self {
        Self {
            id,
            coordinate,
            edges: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute to the node
    pub fn add_attribute(&mut self, key: String, value: String) {
        self.attributes.insert(key, value);
    }

    /// Get an attribute value
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }

    /// Get the degree of this node (number of incident edges)
    pub fn degree(&self) -> usize {
        self.edges.len()
    }
}

/// Road classification for routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoadClass {
    /// Motorway / highway
    Motorway,
    /// Trunk road
    Trunk,
    /// Primary road
    Primary,
    /// Secondary road
    Secondary,
    /// Tertiary road
    Tertiary,
    /// Residential road
    Residential,
    /// Service road
    Service,
    /// Footpath / cycleway
    Path,
    /// Unclassified
    Unclassified,
}

/// An edge in the network graph
#[derive(Debug, Clone)]
pub struct Edge {
    /// Unique identifier
    pub id: EdgeId,
    /// Source node
    pub source: NodeId,
    /// Target node
    pub target: NodeId,
    /// Edge weight (simple scalar cost -- legacy compatibility)
    pub weight: f64,
    /// Multi-criteria edge weight
    pub multi_weight: EdgeWeight,
    /// Geographic geometry (optional)
    pub geometry: Option<LineString>,
    /// Whether the edge is bidirectional
    pub bidirectional: bool,
    /// Speed limit (optional, in km/h)
    pub speed_limit: Option<f64>,
    /// Road class (for routing priority)
    pub road_class: Option<RoadClass>,
    /// Custom attributes
    pub attributes: HashMap<String, String>,
}

impl Edge {
    /// Create a new edge
    pub fn new(id: EdgeId, source: NodeId, target: NodeId, weight: f64) -> Self {
        Self {
            id,
            source,
            target,
            weight,
            multi_weight: EdgeWeight::from_distance(weight),
            geometry: None,
            bidirectional: false,
            speed_limit: None,
            road_class: None,
            attributes: HashMap::new(),
        }
    }

    /// Create a new edge with multi-criteria weight
    pub fn with_multi_weight(
        id: EdgeId,
        source: NodeId,
        target: NodeId,
        multi_weight: EdgeWeight,
    ) -> Self {
        let weight = multi_weight.primary();
        Self {
            id,
            source,
            target,
            weight,
            multi_weight,
            geometry: None,
            bidirectional: false,
            speed_limit: None,
            road_class: None,
            attributes: HashMap::new(),
        }
    }

    /// Set the geometry for this edge
    pub fn with_geometry(mut self, geometry: LineString) -> Self {
        self.geometry = Some(geometry);
        self
    }

    /// Mark the edge as bidirectional
    pub fn bidirectional(mut self) -> Self {
        self.bidirectional = true;
        self
    }

    /// Set the speed limit
    pub fn with_speed_limit(mut self, speed_limit: f64) -> Self {
        self.speed_limit = Some(speed_limit);
        self
    }

    /// Set the road class
    pub fn with_road_class(mut self, road_class: RoadClass) -> Self {
        self.road_class = Some(road_class);
        self
    }

    /// Add an attribute to the edge
    pub fn add_attribute(&mut self, key: String, value: String) {
        self.attributes.insert(key, value);
    }

    /// Get an attribute value
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }

    /// Calculate travel time based on edge length and speed limit
    pub fn travel_time(&self) -> Option<f64> {
        if let (Some(geom), Some(speed)) = (&self.geometry, self.speed_limit) {
            let length = compute_linestring_length(geom);
            Some((length / 1000.0) / speed) // Convert to hours
        } else {
            None
        }
    }

    /// Returns the other endpoint of this edge given one endpoint
    pub fn other_node(&self, node: NodeId) -> Option<NodeId> {
        if node == self.source {
            Some(self.target)
        } else if node == self.target {
            Some(self.source)
        } else {
            None
        }
    }

    /// Check if this is a self-loop
    pub fn is_self_loop(&self) -> bool {
        self.source == self.target
    }
}

/// Compute the length of a linestring in meters (Euclidean)
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

/// Graph metrics for analysis
#[derive(Debug, Clone)]
pub struct GraphMetrics {
    /// Number of nodes
    pub num_nodes: usize,
    /// Number of edges
    pub num_edges: usize,
    /// Average node degree
    pub avg_degree: f64,
    /// Maximum node degree
    pub max_degree: usize,
    /// Minimum node degree
    pub min_degree: usize,
    /// Graph density (0.0 to 1.0)
    pub density: f64,
    /// Number of connected components
    pub num_components: usize,
    /// Total weight of all edges
    pub total_weight: f64,
    /// Average edge weight
    pub avg_weight: f64,
    /// Minimum edge weight
    pub min_weight: f64,
    /// Maximum edge weight
    pub max_weight: f64,
    /// Graph type
    pub graph_type: GraphType,
}

/// A network graph for spatial analysis
#[derive(Debug, Clone)]
pub struct Graph {
    /// All nodes in the graph
    nodes: HashMap<NodeId, Node>,
    /// All edges in the graph
    edges: HashMap<EdgeId, Edge>,
    /// Adjacency list (node -> outgoing edges)
    adjacency: HashMap<NodeId, Vec<EdgeId>>,
    /// Reverse adjacency list (node -> incoming edges)
    reverse_adjacency: HashMap<NodeId, Vec<EdgeId>>,
    /// Next node ID
    next_node_id: usize,
    /// Next edge ID
    next_edge_id: usize,
    /// Graph type (directed or undirected)
    graph_type: GraphType,
}

impl Graph {
    /// Create a new empty directed graph
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            adjacency: HashMap::new(),
            reverse_adjacency: HashMap::new(),
            next_node_id: 0,
            next_edge_id: 0,
            graph_type: GraphType::Directed,
        }
    }

    /// Create a new graph with specified type
    pub fn with_type(graph_type: GraphType) -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            adjacency: HashMap::new(),
            reverse_adjacency: HashMap::new(),
            next_node_id: 0,
            next_edge_id: 0,
            graph_type,
        }
    }

    /// Get the graph type
    pub fn graph_type(&self) -> GraphType {
        self.graph_type
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, coordinate: Coordinate) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id = self.next_node_id.wrapping_add(1);

        let node = Node::new(id, coordinate);
        self.nodes.insert(id, node);
        self.adjacency.insert(id, Vec::new());
        self.reverse_adjacency.insert(id, Vec::new());

        id
    }

    /// Add a node with a specific ID (useful for graph reconstruction)
    pub fn add_node_with_id(&mut self, id: NodeId, coordinate: Coordinate) -> Result<NodeId> {
        if self.nodes.contains_key(&id) {
            return Err(AlgorithmError::InvalidGeometry(format!(
                "Node {:?} already exists",
                id
            )));
        }

        let node = Node::new(id, coordinate);
        self.nodes.insert(id, node);
        self.adjacency.insert(id, Vec::new());
        self.reverse_adjacency.insert(id, Vec::new());

        if id.0 >= self.next_node_id {
            self.next_node_id = id.0.wrapping_add(1);
        }

        Ok(id)
    }

    /// Add an edge to the graph
    pub fn add_edge(&mut self, source: NodeId, target: NodeId, weight: f64) -> Result<EdgeId> {
        if !self.nodes.contains_key(&source) {
            return Err(AlgorithmError::InvalidGeometry(format!(
                "Source node {:?} not found",
                source
            )));
        }

        if !self.nodes.contains_key(&target) {
            return Err(AlgorithmError::InvalidGeometry(format!(
                "Target node {:?} not found",
                target
            )));
        }

        let id = EdgeId(self.next_edge_id);
        self.next_edge_id = self.next_edge_id.wrapping_add(1);

        let edge = Edge::new(id, source, target, weight);
        self.edges.insert(id, edge);

        self.adjacency
            .get_mut(&source)
            .ok_or_else(|| AlgorithmError::InvalidGeometry("Source node not found".to_string()))?
            .push(id);

        self.reverse_adjacency
            .get_mut(&target)
            .ok_or_else(|| AlgorithmError::InvalidGeometry("Target node not found".to_string()))?
            .push(id);

        // For undirected graphs, also add the reverse direction
        if self.graph_type == GraphType::Undirected && source != target {
            self.adjacency
                .get_mut(&target)
                .ok_or_else(|| {
                    AlgorithmError::InvalidGeometry("Target node not found".to_string())
                })?
                .push(id);

            self.reverse_adjacency
                .get_mut(&source)
                .ok_or_else(|| {
                    AlgorithmError::InvalidGeometry("Source node not found".to_string())
                })?
                .push(id);
        }

        if let Some(node) = self.nodes.get_mut(&source) {
            node.edges.push(id);
        }

        if source != target {
            if let Some(node) = self.nodes.get_mut(&target) {
                node.edges.push(id);
            }
        }

        Ok(id)
    }

    /// Add an edge with multi-criteria weight
    pub fn add_weighted_edge(
        &mut self,
        source: NodeId,
        target: NodeId,
        multi_weight: EdgeWeight,
    ) -> Result<EdgeId> {
        if !self.nodes.contains_key(&source) {
            return Err(AlgorithmError::InvalidGeometry(format!(
                "Source node {:?} not found",
                source
            )));
        }

        if !self.nodes.contains_key(&target) {
            return Err(AlgorithmError::InvalidGeometry(format!(
                "Target node {:?} not found",
                target
            )));
        }

        let id = EdgeId(self.next_edge_id);
        self.next_edge_id = self.next_edge_id.wrapping_add(1);

        let edge = Edge::with_multi_weight(id, source, target, multi_weight);
        self.edges.insert(id, edge);

        self.adjacency
            .get_mut(&source)
            .ok_or_else(|| AlgorithmError::InvalidGeometry("Source node not found".to_string()))?
            .push(id);

        self.reverse_adjacency
            .get_mut(&target)
            .ok_or_else(|| AlgorithmError::InvalidGeometry("Target node not found".to_string()))?
            .push(id);

        if self.graph_type == GraphType::Undirected && source != target {
            self.adjacency
                .get_mut(&target)
                .ok_or_else(|| {
                    AlgorithmError::InvalidGeometry("Target node not found".to_string())
                })?
                .push(id);

            self.reverse_adjacency
                .get_mut(&source)
                .ok_or_else(|| {
                    AlgorithmError::InvalidGeometry("Source node not found".to_string())
                })?
                .push(id);
        }

        if let Some(node) = self.nodes.get_mut(&source) {
            node.edges.push(id);
        }

        if source != target {
            if let Some(node) = self.nodes.get_mut(&target) {
                node.edges.push(id);
            }
        }

        Ok(id)
    }

    /// Remove an edge from the graph
    pub fn remove_edge(&mut self, edge_id: EdgeId) -> Result<Edge> {
        let edge = self.edges.remove(&edge_id).ok_or_else(|| {
            AlgorithmError::InvalidGeometry(format!("Edge {:?} not found", edge_id))
        })?;

        if let Some(adj) = self.adjacency.get_mut(&edge.source) {
            adj.retain(|&e| e != edge_id);
        }
        if let Some(radj) = self.reverse_adjacency.get_mut(&edge.target) {
            radj.retain(|&e| e != edge_id);
        }

        if self.graph_type == GraphType::Undirected && edge.source != edge.target {
            if let Some(adj) = self.adjacency.get_mut(&edge.target) {
                adj.retain(|&e| e != edge_id);
            }
            if let Some(radj) = self.reverse_adjacency.get_mut(&edge.source) {
                radj.retain(|&e| e != edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&edge.source) {
            node.edges.retain(|&e| e != edge_id);
        }
        if let Some(node) = self.nodes.get_mut(&edge.target) {
            node.edges.retain(|&e| e != edge_id);
        }

        Ok(edge)
    }

    /// Remove a node and all its incident edges from the graph
    pub fn remove_node(&mut self, node_id: NodeId) -> Result<Node> {
        let node = self.nodes.get(&node_id).ok_or_else(|| {
            AlgorithmError::InvalidGeometry(format!("Node {:?} not found", node_id))
        })?;

        let incident_edges: Vec<EdgeId> = node.edges.clone();

        for edge_id in incident_edges {
            let _ = self.remove_edge(edge_id);
        }

        self.adjacency.remove(&node_id);
        self.reverse_adjacency.remove(&node_id);

        let node = self.nodes.remove(&node_id).ok_or_else(|| {
            AlgorithmError::InvalidGeometry(format!("Node {:?} not found", node_id))
        })?;

        Ok(node)
    }

    /// Remove a node without error checking (for internal use in topology cleaning)
    pub(crate) fn remove_node_unchecked(&mut self, node_id: NodeId) {
        self.nodes.remove(&node_id);
        self.adjacency.remove(&node_id);
        self.reverse_adjacency.remove(&node_id);
    }

    /// Get a node by ID
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Get a mutable reference to a node by ID
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// Get an edge by ID
    pub fn get_edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(&id)
    }

    /// Get a mutable reference to an edge by ID
    pub fn get_edge_mut(&mut self, id: EdgeId) -> Option<&mut Edge> {
        self.edges.get_mut(&id)
    }

    /// Get all outgoing edges from a node
    pub fn outgoing_edges(&self, node: NodeId) -> &[EdgeId] {
        self.adjacency
            .get(&node)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all incoming edges to a node
    pub fn incoming_edges(&self, node: NodeId) -> &[EdgeId] {
        self.reverse_adjacency
            .get(&node)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all neighbors of a node (targets of outgoing edges)
    pub fn neighbors(&self, node: NodeId) -> Vec<NodeId> {
        self.outgoing_edges(node)
            .iter()
            .filter_map(|edge_id| {
                self.edges.get(edge_id).and_then(|e| {
                    if self.graph_type == GraphType::Undirected {
                        e.other_node(node)
                    } else {
                        Some(e.target)
                    }
                })
            })
            .collect()
    }

    /// Get all neighbors with the connecting edge information
    pub fn neighbors_with_edges(&self, node: NodeId) -> Vec<(NodeId, EdgeId, f64)> {
        self.outgoing_edges(node)
            .iter()
            .filter_map(|&edge_id| {
                self.edges.get(&edge_id).and_then(|e| {
                    let neighbor = if self.graph_type == GraphType::Undirected {
                        e.other_node(node)
                    } else {
                        Some(e.target)
                    };
                    neighbor.map(|n| (n, edge_id, e.weight))
                })
            })
            .collect()
    }

    /// Number of nodes in the graph
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges in the graph
    pub fn num_edges(&self) -> usize {
        self.edges.len()
    }

    /// Check if the graph is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get all node IDs
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().copied().collect()
    }

    /// Get all edge IDs
    pub fn edge_ids(&self) -> Vec<EdgeId> {
        self.edges.keys().copied().collect()
    }

    /// Check if a node exists
    pub fn has_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Check if an edge exists
    pub fn has_edge(&self, id: EdgeId) -> bool {
        self.edges.contains_key(&id)
    }

    /// Find an edge between two specific nodes
    pub fn find_edge(&self, source: NodeId, target: NodeId) -> Option<EdgeId> {
        self.outgoing_edges(source).iter().find_map(|&edge_id| {
            self.edges.get(&edge_id).and_then(|e| {
                if e.target == target
                    || (self.graph_type == GraphType::Undirected && e.source == target)
                {
                    Some(edge_id)
                } else {
                    None
                }
            })
        })
    }

    /// Find all edges between two specific nodes (multi-graph support)
    pub fn find_edges(&self, source: NodeId, target: NodeId) -> Vec<EdgeId> {
        self.outgoing_edges(source)
            .iter()
            .filter_map(|&edge_id| {
                self.edges.get(&edge_id).and_then(|e| {
                    if e.target == target
                        || (self.graph_type == GraphType::Undirected && e.source == target)
                    {
                        Some(edge_id)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Find the nearest node to a given coordinate
    pub fn nearest_node(&self, coord: &Coordinate) -> Option<NodeId> {
        self.nodes
            .iter()
            .min_by(|(_, a), (_, b)| {
                let dist_a = Self::euclidean_distance(&a.coordinate, coord);
                let dist_b = Self::euclidean_distance(&b.coordinate, coord);
                dist_a
                    .partial_cmp(&dist_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(id, _)| *id)
    }

    /// Find the k nearest nodes to a given coordinate
    pub fn k_nearest_nodes(&self, coord: &Coordinate, k: usize) -> Vec<(NodeId, f64)> {
        let mut node_dists: Vec<(NodeId, f64)> = self
            .nodes
            .iter()
            .map(|(id, node)| (*id, Self::euclidean_distance(&node.coordinate, coord)))
            .collect();

        node_dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        node_dists.truncate(k);
        node_dists
    }

    /// Calculate Euclidean distance between two coordinates
    fn euclidean_distance(a: &Coordinate, b: &Coordinate) -> f64 {
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Compute the in-degree of a node
    pub fn in_degree(&self, node: NodeId) -> usize {
        self.incoming_edges(node).len()
    }

    /// Compute the out-degree of a node
    pub fn out_degree(&self, node: NodeId) -> usize {
        self.outgoing_edges(node).len()
    }

    /// Compute the total degree of a node
    pub fn degree(&self, node: NodeId) -> usize {
        if self.graph_type == GraphType::Undirected {
            self.outgoing_edges(node).len()
        } else {
            self.in_degree(node) + self.out_degree(node)
        }
    }

    /// Calculate graph metrics
    pub fn metrics(&self) -> GraphMetrics {
        let mut total_degree = 0;
        let mut max_degree = 0;
        let mut min_degree = usize::MAX;
        let mut total_weight = 0.0;
        let mut min_weight = f64::INFINITY;
        let mut max_weight = f64::NEG_INFINITY;

        for node_id in self.nodes.keys() {
            let deg = self.degree(*node_id);
            total_degree += deg;
            max_degree = max_degree.max(deg);
            min_degree = min_degree.min(deg);
        }

        for edge in self.edges.values() {
            total_weight += edge.weight;
            min_weight = min_weight.min(edge.weight);
            max_weight = max_weight.max(edge.weight);
        }

        if self.nodes.is_empty() {
            min_degree = 0;
        }
        if self.edges.is_empty() {
            min_weight = 0.0;
            max_weight = 0.0;
        }

        let avg_degree = if self.num_nodes() > 0 {
            total_degree as f64 / self.num_nodes() as f64
        } else {
            0.0
        };

        let avg_weight = if self.num_edges() > 0 {
            total_weight / self.num_edges() as f64
        } else {
            0.0
        };

        let components = self.connected_components();
        let density = if self.num_nodes() > 1 {
            let max_edges = if self.graph_type == GraphType::Directed {
                self.num_nodes() * (self.num_nodes() - 1)
            } else {
                self.num_nodes() * (self.num_nodes() - 1) / 2
            };
            if max_edges > 0 {
                self.num_edges() as f64 / max_edges as f64
            } else {
                0.0
            }
        } else {
            0.0
        };

        GraphMetrics {
            num_nodes: self.num_nodes(),
            num_edges: self.num_edges(),
            avg_degree,
            max_degree,
            min_degree,
            density,
            num_components: components.len(),
            total_weight,
            avg_weight,
            min_weight,
            max_weight,
            graph_type: self.graph_type,
        }
    }

    /// Create a subgraph containing only the specified nodes and edges between them
    pub fn subgraph(&self, node_ids: &[NodeId]) -> Result<Graph> {
        let node_set: std::collections::HashSet<NodeId> = node_ids.iter().copied().collect();
        let mut sub = Graph::with_type(self.graph_type);

        for &node_id in &node_set {
            let node = self.nodes.get(&node_id).ok_or_else(|| {
                AlgorithmError::InvalidGeometry(format!("Node {:?} not found", node_id))
            })?;
            sub.add_node_with_id(node_id, node.coordinate)?;
        }

        for edge in self.edges.values() {
            if node_set.contains(&edge.source) && node_set.contains(&edge.target) {
                let _ = sub.add_edge(edge.source, edge.target, edge.weight);
            }
        }

        Ok(sub)
    }

    /// Reverse all edge directions (only meaningful for directed graphs)
    pub fn reverse(&self) -> Graph {
        let mut reversed = Graph::with_type(self.graph_type);

        for (id, node) in &self.nodes {
            let _ = reversed.add_node_with_id(*id, node.coordinate);
        }

        for edge in self.edges.values() {
            let _ = reversed.add_edge(edge.target, edge.source, edge.weight);
        }

        reversed
    }

    /// Access the nodes map directly (for iteration)
    pub fn nodes_iter(&self) -> impl Iterator<Item = (&NodeId, &Node)> {
        self.nodes.iter()
    }

    /// Access the edges map directly (for iteration)
    pub fn edges_iter(&self) -> impl Iterator<Item = (&EdgeId, &Edge)> {
        self.edges.iter()
    }

    /// Get sorted node IDs for deterministic iteration
    pub fn sorted_node_ids(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        ids.sort();
        ids
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing graphs from geospatial data
pub struct GraphBuilder {
    graph: Graph,
    tolerance: f64,
    /// Cached node lookup for fast coordinate snapping
    node_index: Vec<(Coordinate, NodeId)>,
}

impl GraphBuilder {
    /// Create a new graph builder (directed)
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            tolerance: 1e-6,
            node_index: Vec::new(),
        }
    }

    /// Create a new graph builder with specified type
    pub fn with_graph_type(graph_type: GraphType) -> Self {
        Self {
            graph: Graph::with_type(graph_type),
            tolerance: 1e-6,
            node_index: Vec::new(),
        }
    }

    /// Set the tolerance for coordinate snapping
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Add a linestring as edges in the graph
    pub fn add_linestring(
        &mut self,
        linestring: &LineString,
        weight_fn: impl Fn(f64) -> f64,
    ) -> Result<Vec<EdgeId>> {
        let coords = &linestring.coords;
        if coords.len() < 2 {
            return Err(AlgorithmError::InvalidGeometry(
                "Linestring must have at least 2 coordinates".to_string(),
            ));
        }

        let mut edge_ids = Vec::new();
        let mut prev_node = self.find_or_create_node(coords[0]);

        for i in 1..coords.len() {
            let curr_node = self.find_or_create_node(coords[i]);
            let dx = coords[i].x - coords[i - 1].x;
            let dy = coords[i].y - coords[i - 1].y;
            let length = (dx * dx + dy * dy).sqrt();
            let weight = weight_fn(length);
            let edge_id = self.graph.add_edge(prev_node, curr_node, weight)?;
            edge_ids.push(edge_id);
            prev_node = curr_node;
        }

        Ok(edge_ids)
    }

    /// Add a linestring with multi-criteria weights
    pub fn add_linestring_weighted(
        &mut self,
        linestring: &LineString,
        weight_fn: impl Fn(f64) -> EdgeWeight,
    ) -> Result<Vec<EdgeId>> {
        let coords = &linestring.coords;
        if coords.len() < 2 {
            return Err(AlgorithmError::InvalidGeometry(
                "Linestring must have at least 2 coordinates".to_string(),
            ));
        }

        let mut edge_ids = Vec::new();
        let mut prev_node = self.find_or_create_node(coords[0]);

        for i in 1..coords.len() {
            let curr_node = self.find_or_create_node(coords[i]);
            let dx = coords[i].x - coords[i - 1].x;
            let dy = coords[i].y - coords[i - 1].y;
            let length = (dx * dx + dy * dy).sqrt();
            let multi_weight = weight_fn(length);
            let edge_id = self
                .graph
                .add_weighted_edge(prev_node, curr_node, multi_weight)?;
            edge_ids.push(edge_id);
            prev_node = curr_node;
        }

        Ok(edge_ids)
    }

    /// Find an existing node or create a new one
    fn find_or_create_node(&mut self, coord: Coordinate) -> NodeId {
        for (existing_coord, node_id) in &self.node_index {
            let dx = existing_coord.x - coord.x;
            let dy = existing_coord.y - coord.y;
            if (dx * dx + dy * dy).sqrt() < self.tolerance {
                return *node_id;
            }
        }
        let node_id = self.graph.add_node(coord);
        self.node_index.push((coord, node_id));
        node_id
    }

    /// Build and return the graph
    pub fn build(self) -> Graph {
        self.graph
    }

    /// Build and validate the graph
    pub fn build_validated(self) -> Result<Graph> {
        let graph = self.graph;
        graph.validate()?;
        Ok(graph)
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_graph() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        assert_eq!(graph.num_nodes(), 2);
        let edge = graph.add_edge(n1, n2, 1.0);
        assert!(edge.is_ok());
        assert_eq!(graph.num_edges(), 1);
    }

    #[test]
    fn test_undirected_graph() {
        let mut graph = Graph::with_type(GraphType::Undirected);
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        assert!(graph.neighbors(n1).contains(&n2));
        assert!(graph.neighbors(n2).contains(&n1));
    }

    #[test]
    fn test_directed_graph_one_way() {
        let mut graph = Graph::with_type(GraphType::Directed);
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        assert!(graph.neighbors(n1).contains(&n2));
        assert!(graph.neighbors(n2).is_empty());
    }

    #[test]
    fn test_adjacency() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0).expect("add edge");
        let _ = graph.add_edge(n1, n3, 2.0).expect("add edge");
        let neighbors = graph.neighbors(n1);
        assert_eq!(neighbors.len(), 2);
    }

    #[test]
    fn test_nearest_node() {
        let mut graph = Graph::new();
        let _n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(5.0, 0.0));
        let _n3 = graph.add_node(Coordinate::new_2d(10.0, 0.0));
        assert_eq!(graph.nearest_node(&Coordinate::new_2d(4.5, 0.0)), Some(n2));
    }

    #[test]
    fn test_k_nearest_nodes() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(5.0, 0.0));
        let _n3 = graph.add_node(Coordinate::new_2d(10.0, 0.0));
        let nearest = graph.k_nearest_nodes(&Coordinate::new_2d(1.0, 0.0), 2);
        assert_eq!(nearest.len(), 2);
        assert_eq!(nearest[0].0, n1);
        assert_eq!(nearest[1].0, n2);
    }

    #[test]
    fn test_graph_builder() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(2.0, 0.0),
        ];
        let linestring = LineString::new(coords).expect("linestring");
        let mut builder = GraphBuilder::new();
        let edges = builder.add_linestring(&linestring, |length| length);
        assert!(edges.is_ok());
        let graph = builder.build();
        assert_eq!(graph.num_nodes(), 3);
        assert_eq!(graph.num_edges(), 2);
    }

    #[test]
    fn test_graph_metrics() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n2, n3, 1.0);
        let metrics = graph.metrics();
        assert_eq!(metrics.num_nodes, 3);
        assert_eq!(metrics.num_edges, 2);
        assert_eq!(metrics.total_weight, 2.0);
    }

    #[test]
    fn test_edge_travel_time() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1000.0, 0.0),
        ];
        let linestring = LineString::new(coords).expect("linestring");
        let edge = Edge::new(EdgeId(0), NodeId(0), NodeId(1), 1.0)
            .with_geometry(linestring)
            .with_speed_limit(60.0);
        let time = edge.travel_time().expect("travel time");
        assert!((time - 1.0 / 60.0).abs() < 1e-6);
    }

    #[test]
    fn test_remove_edge() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let e1 = graph.add_edge(n1, n2, 1.0).expect("add edge");
        assert!(graph.remove_edge(e1).is_ok());
        assert_eq!(graph.num_edges(), 0);
    }

    #[test]
    fn test_remove_node() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n2, n3, 1.0);
        assert!(graph.remove_node(n2).is_ok());
        assert_eq!(graph.num_nodes(), 2);
        assert_eq!(graph.num_edges(), 0);
    }

    #[test]
    fn test_edge_weight_multi_criteria() {
        let weight = EdgeWeight::new(100.0, 60.0, 5.0).with_custom("elevation".to_string(), 10.0);
        let mut criteria = HashMap::new();
        criteria.insert("distance".to_string(), 1.0);
        criteria.insert("time".to_string(), 2.0);
        criteria.insert("monetary".to_string(), 0.5);
        criteria.insert("elevation".to_string(), 0.3);
        let cost = weight.weighted_cost(&criteria);
        assert!((cost - 225.5).abs() < 1e-10);
    }

    #[test]
    fn test_subgraph() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let n3 = graph.add_node(Coordinate::new_2d(2.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        let _ = graph.add_edge(n2, n3, 1.0);
        let sub = graph.subgraph(&[n1, n2]).expect("subgraph");
        assert_eq!(sub.num_nodes(), 2);
        assert_eq!(sub.num_edges(), 1);
    }

    #[test]
    fn test_graph_reverse() {
        let mut graph = Graph::with_type(GraphType::Directed);
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let _ = graph.add_edge(n1, n2, 1.0);
        let reversed = graph.reverse();
        assert!(reversed.neighbors(n2).contains(&n1));
        assert!(reversed.neighbors(n1).is_empty());
    }

    #[test]
    fn test_find_edge() {
        let mut graph = Graph::new();
        let n1 = graph.add_node(Coordinate::new_2d(0.0, 0.0));
        let n2 = graph.add_node(Coordinate::new_2d(1.0, 0.0));
        let e1 = graph.add_edge(n1, n2, 1.0).expect("add edge");
        assert_eq!(graph.find_edge(n1, n2), Some(e1));
        assert_eq!(graph.find_edge(n2, n1), None);
    }

    #[test]
    fn test_builder_weighted() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 0.0)];
        let linestring = LineString::new(coords).expect("linestring");
        let mut builder = GraphBuilder::new();
        let edges = builder.add_linestring_weighted(&linestring, |len| {
            EdgeWeight::from_distance_time(len, len / 50.0)
        });
        assert!(edges.is_ok());
        assert_eq!(builder.build().num_edges(), 1);
    }
}
