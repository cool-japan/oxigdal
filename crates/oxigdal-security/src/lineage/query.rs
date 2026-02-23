//! Lineage query API.

use crate::error::{Result, SecurityError};
use crate::lineage::{LineageNode, NodeType, graph::LineageGraph};
use std::collections::HashSet;
use std::sync::Arc;

/// Lineage query builder.
pub struct LineageQuery {
    graph: Arc<LineageGraph>,
    filters: Vec<QueryFilter>,
    max_depth: Option<usize>,
}

impl LineageQuery {
    /// Create a new lineage query.
    pub fn new(graph: Arc<LineageGraph>) -> Self {
        Self {
            graph,
            filters: Vec::new(),
            max_depth: None,
        }
    }

    /// Add a filter.
    pub fn filter(mut self, filter: QueryFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Set maximum traversal depth.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Find all ancestors of a node.
    pub fn ancestors(&self, node_id: &str) -> Result<Vec<LineageNode>> {
        let mut ancestors = self.graph.get_ancestors(node_id)?;
        ancestors.retain(|node| self.apply_filters(node));

        if let Some(max_depth) = self.max_depth {
            ancestors.truncate(max_depth);
        }

        Ok(ancestors)
    }

    /// Find all descendants of a node.
    pub fn descendants(&self, node_id: &str) -> Result<Vec<LineageNode>> {
        let mut descendants = self.graph.get_descendants(node_id)?;
        descendants.retain(|node| self.apply_filters(node));

        if let Some(max_depth) = self.max_depth {
            descendants.truncate(max_depth);
        }

        Ok(descendants)
    }

    /// Find path between two nodes.
    pub fn path(&self, from_id: &str, to_id: &str) -> Result<Option<Vec<LineageNode>>> {
        let mut visited = HashSet::new();
        let mut path = Vec::new();

        if self.find_path(from_id, to_id, &mut visited, &mut path)? {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    fn find_path(
        &self,
        current_id: &str,
        target_id: &str,
        visited: &mut HashSet<String>,
        path: &mut Vec<LineageNode>,
    ) -> Result<bool> {
        if visited.contains(current_id) {
            return Ok(false);
        }

        visited.insert(current_id.to_string());

        let current = self
            .graph
            .get_node(current_id)
            .ok_or_else(|| SecurityError::lineage_query("Node not found"))?;

        path.push(current.clone());

        if current_id == target_id {
            return Ok(true);
        }

        let downstream = self.graph.get_downstream(current_id)?;
        for node in downstream {
            if self.find_path(&node.id, target_id, visited, path)? {
                return Ok(true);
            }
        }

        path.pop();
        Ok(false)
    }

    /// Find all nodes matching a filter.
    pub fn find_nodes(&self) -> Result<Vec<LineageNode>> {
        // Get all nodes by iterating through entity index
        let nodes = Vec::new();

        // This is a simplification - in a real implementation,
        // we would need a way to iterate all nodes efficiently
        // For now, this shows the API design

        Ok(nodes)
    }

    fn apply_filters(&self, node: &LineageNode) -> bool {
        for filter in &self.filters {
            if !filter.matches(node) {
                return false;
            }
        }
        true
    }

    /// Find common ancestors of multiple nodes.
    pub fn common_ancestors(&self, node_ids: &[String]) -> Result<Vec<LineageNode>> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut common: Option<HashSet<String>> = None;

        for node_id in node_ids {
            let ancestors = self.graph.get_ancestors(node_id)?;
            let ancestor_ids: HashSet<String> = ancestors.iter().map(|n| n.id.clone()).collect();

            common = Some(match common {
                None => ancestor_ids,
                Some(existing) => existing.intersection(&ancestor_ids).cloned().collect(),
            });
        }

        let common_ids = common.unwrap_or_default();
        let mut result = Vec::new();

        for id in common_ids {
            if let Some(node) = self.graph.get_node(&id) {
                if self.apply_filters(&node) {
                    result.push(node);
                }
            }
        }

        Ok(result)
    }
}

/// Query filter.
#[derive(Debug, Clone)]
pub enum QueryFilter {
    /// Filter by node type.
    NodeType(NodeType),
    /// Filter by entity ID pattern.
    EntityIdPattern(String),
    /// Filter by metadata key-value.
    Metadata(String, String),
    /// Filter by creation time range.
    TimeRange(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>),
}

impl QueryFilter {
    /// Check if node matches filter.
    pub fn matches(&self, node: &LineageNode) -> bool {
        match self {
            QueryFilter::NodeType(node_type) => &node.node_type == node_type,
            QueryFilter::EntityIdPattern(pattern) => {
                // Simple pattern matching
                if pattern.contains('*') {
                    let parts: Vec<&str> = pattern.split('*').collect();
                    if parts.len() == 2 {
                        node.entity_id.starts_with(parts[0]) && node.entity_id.ends_with(parts[1])
                    } else {
                        false
                    }
                } else {
                    node.entity_id == *pattern
                }
            }
            QueryFilter::Metadata(key, value) => node.metadata.get(key) == Some(value),
            QueryFilter::TimeRange(start, end) => {
                node.created_at >= *start && node.created_at <= *end
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::{EdgeType, LineageEdge, LineageNode};

    #[test]
    fn test_query_filter_node_type() {
        let node = LineageNode::new(NodeType::Dataset, "dataset-1".to_string());
        let filter = QueryFilter::NodeType(NodeType::Dataset);

        assert!(filter.matches(&node));

        let filter = QueryFilter::NodeType(NodeType::Operation);
        assert!(!filter.matches(&node));
    }

    #[test]
    fn test_query_filter_entity_pattern() {
        let node = LineageNode::new(NodeType::Dataset, "dataset-123".to_string());
        let filter = QueryFilter::EntityIdPattern("dataset-*".to_string());

        assert!(filter.matches(&node));

        let filter = QueryFilter::EntityIdPattern("other-*".to_string());
        assert!(!filter.matches(&node));
    }

    #[test]
    fn test_query_filter_metadata() {
        let node = LineageNode::new(NodeType::Dataset, "dataset-1".to_string())
            .with_metadata("format".to_string(), "GeoTIFF".to_string());

        let filter = QueryFilter::Metadata("format".to_string(), "GeoTIFF".to_string());
        assert!(filter.matches(&node));

        let filter = QueryFilter::Metadata("format".to_string(), "PNG".to_string());
        assert!(!filter.matches(&node));
    }

    #[test]
    fn test_lineage_query() {
        let graph = Arc::new(LineageGraph::new());

        let node1 = LineageNode::new(NodeType::Dataset, "dataset-1".to_string());
        let node1_id = graph.add_node(node1).expect("Failed to add node");

        let node2 = LineageNode::new(NodeType::Dataset, "dataset-2".to_string());
        let node2_id = graph.add_node(node2).expect("Failed to add node");

        let edge = LineageEdge::new(node1_id.clone(), node2_id.clone(), EdgeType::DerivedFrom);
        graph.add_edge(edge).expect("Failed to add edge");

        let query = LineageQuery::new(graph).filter(QueryFilter::NodeType(NodeType::Dataset));

        let ancestors = query
            .ancestors(&node2_id)
            .expect("Failed to query ancestors");
        assert_eq!(ancestors.len(), 1);
    }
}
