//! Lineage graph construction and management.

use crate::error::{Result, SecurityError};
use crate::lineage::{EdgeType, LineageEdge, LineageEvent, LineageNode, NodeType};
use dashmap::DashMap;
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use std::sync::Arc;

/// Lineage graph.
pub struct LineageGraph {
    /// Graph structure.
    graph: parking_lot::RwLock<DiGraph<LineageNode, LineageEdge>>,
    /// Node ID to graph index mapping.
    node_index: Arc<DashMap<String, NodeIndex>>,
    /// Entity ID to node ID mapping.
    entity_index: Arc<DashMap<String, Vec<String>>>,
}

impl LineageGraph {
    /// Create a new lineage graph.
    pub fn new() -> Self {
        Self {
            graph: parking_lot::RwLock::new(DiGraph::new()),
            node_index: Arc::new(DashMap::new()),
            entity_index: Arc::new(DashMap::new()),
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&self, node: LineageNode) -> Result<String> {
        let node_id = node.id.clone();
        let entity_id = node.entity_id.clone();

        let mut graph = self.graph.write();
        let idx = graph.add_node(node);

        self.node_index.insert(node_id.clone(), idx);
        self.entity_index
            .entry(entity_id)
            .or_default()
            .push(node_id.clone());

        Ok(node_id)
    }

    /// Add an edge to the graph.
    pub fn add_edge(&self, edge: LineageEdge) -> Result<String> {
        let source_idx = *self
            .node_index
            .get(&edge.source_id)
            .ok_or_else(|| SecurityError::lineage_tracking("Source node not found"))?;

        let target_idx = *self
            .node_index
            .get(&edge.target_id)
            .ok_or_else(|| SecurityError::lineage_tracking("Target node not found"))?;

        let edge_id = edge.id.clone();
        let mut graph = self.graph.write();
        graph.add_edge(source_idx, target_idx, edge);

        Ok(edge_id)
    }

    /// Get a node by ID.
    pub fn get_node(&self, node_id: &str) -> Option<LineageNode> {
        let idx = self.node_index.get(node_id)?;
        let graph = self.graph.read();
        graph.node_weight(*idx).cloned()
    }

    /// Get nodes by entity ID.
    pub fn get_nodes_by_entity(&self, entity_id: &str) -> Vec<LineageNode> {
        let node_ids = match self.entity_index.get(entity_id) {
            Some(ids) => ids.clone(),
            None => return Vec::new(),
        };

        node_ids.iter().filter_map(|id| self.get_node(id)).collect()
    }

    /// Get upstream nodes (dependencies).
    pub fn get_upstream(&self, node_id: &str) -> Result<Vec<LineageNode>> {
        let idx = *self
            .node_index
            .get(node_id)
            .ok_or_else(|| SecurityError::lineage_tracking("Node not found"))?;

        let graph = self.graph.read();
        let upstream_indices: Vec<_> = graph.neighbors_directed(idx, Direction::Incoming).collect();

        Ok(upstream_indices
            .iter()
            .filter_map(|&i| graph.node_weight(i).cloned())
            .collect())
    }

    /// Get downstream nodes (dependents).
    pub fn get_downstream(&self, node_id: &str) -> Result<Vec<LineageNode>> {
        let idx = *self
            .node_index
            .get(node_id)
            .ok_or_else(|| SecurityError::lineage_tracking("Node not found"))?;

        let graph = self.graph.read();
        let downstream_indices: Vec<_> =
            graph.neighbors_directed(idx, Direction::Outgoing).collect();

        Ok(downstream_indices
            .iter()
            .filter_map(|&i| graph.node_weight(i).cloned())
            .collect())
    }

    /// Get all ancestors (recursive upstream).
    pub fn get_ancestors(&self, node_id: &str) -> Result<Vec<LineageNode>> {
        let mut ancestors = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_ancestors(node_id, &mut ancestors, &mut visited)?;
        Ok(ancestors)
    }

    fn collect_ancestors(
        &self,
        node_id: &str,
        ancestors: &mut Vec<LineageNode>,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<()> {
        if visited.contains(node_id) {
            return Ok(());
        }
        visited.insert(node_id.to_string());

        let upstream = self.get_upstream(node_id)?;
        for node in upstream {
            ancestors.push(node.clone());
            self.collect_ancestors(&node.id, ancestors, visited)?;
        }

        Ok(())
    }

    /// Get all descendants (recursive downstream).
    pub fn get_descendants(&self, node_id: &str) -> Result<Vec<LineageNode>> {
        let mut descendants = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_descendants(node_id, &mut descendants, &mut visited)?;
        Ok(descendants)
    }

    fn collect_descendants(
        &self,
        node_id: &str,
        descendants: &mut Vec<LineageNode>,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<()> {
        if visited.contains(node_id) {
            return Ok(());
        }
        visited.insert(node_id.to_string());

        let downstream = self.get_downstream(node_id)?;
        for node in downstream {
            descendants.push(node.clone());
            self.collect_descendants(&node.id, descendants, visited)?;
        }

        Ok(())
    }

    /// Record a lineage event.
    pub fn record_event(&self, event: LineageEvent) -> Result<()> {
        // Create operation node if specified
        if let Some(ref operation_id) = event.operation {
            let op_node = LineageNode::new(NodeType::Operation, operation_id.clone())
                .with_metadata("event_type".to_string(), event.event_type.clone());
            self.add_node(op_node)?;
        }

        // Create edges from inputs to operation
        if let Some(ref operation_id) = event.operation {
            if let Some(op_node_id) = self
                .entity_index
                .get(operation_id)
                .and_then(|ids| ids.first().cloned())
            {
                for input_id in &event.inputs {
                    if let Some(input_node_id) = self
                        .entity_index
                        .get(input_id)
                        .and_then(|ids| ids.last().cloned())
                    {
                        let edge =
                            LineageEdge::new(input_node_id, op_node_id.clone(), EdgeType::Used);
                        self.add_edge(edge)?;
                    }
                }

                // Create edges from operation to outputs
                for output_id in &event.outputs {
                    if let Some(output_node_id) = self
                        .entity_index
                        .get(output_id)
                        .and_then(|ids| ids.last().cloned())
                    {
                        let edge = LineageEdge::new(
                            op_node_id.clone(),
                            output_node_id,
                            EdgeType::GeneratedBy,
                        );
                        self.add_edge(edge)?;
                    }
                }
            }
        } else {
            // Direct edges from inputs to outputs
            for input_id in &event.inputs {
                if let Some(input_node_id) = self
                    .entity_index
                    .get(input_id)
                    .and_then(|ids| ids.last().cloned())
                {
                    for output_id in &event.outputs {
                        if let Some(output_node_id) = self
                            .entity_index
                            .get(output_id)
                            .and_then(|ids| ids.last().cloned())
                        {
                            let edge = LineageEdge::new(
                                input_node_id.clone(),
                                output_node_id,
                                EdgeType::DerivedFrom,
                            );
                            self.add_edge(edge)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get graph statistics.
    pub fn stats(&self) -> (usize, usize) {
        let graph = self.graph.read();
        (graph.node_count(), graph.edge_count())
    }

    /// Clear the graph.
    pub fn clear(&self) {
        let mut graph = self.graph.write();
        graph.clear();
        self.node_index.clear();
        self.entity_index.clear();
    }
}

impl Default for LineageGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node() {
        let graph = LineageGraph::new();
        let node = LineageNode::new(NodeType::Dataset, "dataset-1".to_string());
        let node_id = graph.add_node(node).expect("Failed to add node");

        assert!(graph.get_node(&node_id).is_some());
    }

    #[test]
    fn test_add_edge() {
        let graph = LineageGraph::new();

        let node1 = LineageNode::new(NodeType::Dataset, "dataset-1".to_string());
        let node1_id = graph.add_node(node1).expect("Failed to add node");

        let node2 = LineageNode::new(NodeType::Dataset, "dataset-2".to_string());
        let node2_id = graph.add_node(node2).expect("Failed to add node");

        let edge = LineageEdge::new(node1_id.clone(), node2_id.clone(), EdgeType::DerivedFrom);
        graph.add_edge(edge).expect("Failed to add edge");

        let downstream = graph
            .get_downstream(&node1_id)
            .expect("Failed to get downstream");
        assert_eq!(downstream.len(), 1);
        assert_eq!(downstream[0].id, node2_id);
    }

    #[test]
    fn test_upstream_downstream() {
        let graph = LineageGraph::new();

        let node1 = LineageNode::new(NodeType::Dataset, "dataset-1".to_string());
        let node1_id = graph.add_node(node1).expect("Failed to add node");

        let node2 = LineageNode::new(NodeType::Dataset, "dataset-2".to_string());
        let node2_id = graph.add_node(node2).expect("Failed to add node");

        let edge = LineageEdge::new(node1_id.clone(), node2_id.clone(), EdgeType::DerivedFrom);
        graph.add_edge(edge).expect("Failed to add edge");

        let upstream = graph
            .get_upstream(&node2_id)
            .expect("Failed to get upstream");
        assert_eq!(upstream.len(), 1);
        assert_eq!(upstream[0].id, node1_id);

        let downstream = graph
            .get_downstream(&node1_id)
            .expect("Failed to get downstream");
        assert_eq!(downstream.len(), 1);
        assert_eq!(downstream[0].id, node2_id);
    }

    #[test]
    fn test_record_event() {
        let graph = LineageGraph::new();

        let input_node = LineageNode::new(NodeType::Dataset, "input-1".to_string());
        graph.add_node(input_node).expect("Failed to add node");

        let output_node = LineageNode::new(NodeType::Dataset, "output-1".to_string());
        graph.add_node(output_node).expect("Failed to add node");

        let event = LineageEvent::new("transform".to_string())
            .with_input("input-1".to_string())
            .with_output("output-1".to_string())
            .with_operation("op-1".to_string());

        graph.record_event(event).expect("Failed to record event");

        let (nodes, edges) = graph.stats();
        assert!(nodes >= 2); // At least input and output
        assert!(edges >= 2); // At least input->op and op->output
    }
}
