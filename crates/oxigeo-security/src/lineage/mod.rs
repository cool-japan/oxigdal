//! Data lineage tracking system.

pub mod graph;
pub mod metadata;
pub mod query;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Lineage node representing a data entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineageNode {
    /// Node ID.
    pub id: String,
    /// Node type.
    pub node_type: NodeType,
    /// Entity URI or identifier.
    pub entity_id: String,
    /// Node metadata.
    pub metadata: HashMap<String, String>,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
}

impl LineageNode {
    /// Create a new lineage node.
    pub fn new(node_type: NodeType, entity_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_type,
            entity_id,
            metadata: HashMap::new(),
            created_at: Utc::now(),
        }
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Node type in lineage graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    /// Dataset.
    Dataset,
    /// Processing operation.
    Operation,
    /// User/agent.
    Agent,
    /// Model.
    Model,
    /// Parameter set.
    Parameters,
}

/// Lineage edge representing a relationship.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineageEdge {
    /// Edge ID.
    pub id: String,
    /// Source node ID.
    pub source_id: String,
    /// Target node ID.
    pub target_id: String,
    /// Edge type.
    pub edge_type: EdgeType,
    /// Edge metadata.
    pub metadata: HashMap<String, String>,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
}

impl LineageEdge {
    /// Create a new lineage edge.
    pub fn new(source_id: String, target_id: String, edge_type: EdgeType) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source_id,
            target_id,
            edge_type,
            metadata: HashMap::new(),
            created_at: Utc::now(),
        }
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Edge type in lineage graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    /// Data derivation (wasDerivedFrom in PROV-O).
    DerivedFrom,
    /// Usage (used in PROV-O).
    Used,
    /// Generation (wasGeneratedBy in PROV-O).
    GeneratedBy,
    /// Attribution (wasAttributedTo in PROV-O).
    AttributedTo,
    /// Association (wasAssociatedWith in PROV-O).
    AssociatedWith,
}

/// Lineage event for tracking operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageEvent {
    /// Event ID.
    pub id: String,
    /// Event type.
    pub event_type: String,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Input nodes.
    pub inputs: Vec<String>,
    /// Output nodes.
    pub outputs: Vec<String>,
    /// Operation node.
    pub operation: Option<String>,
    /// Agent (user/service).
    pub agent: Option<String>,
    /// Event metadata.
    pub metadata: HashMap<String, String>,
}

impl LineageEvent {
    /// Create a new lineage event.
    pub fn new(event_type: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event_type,
            timestamp: Utc::now(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            operation: None,
            agent: None,
            metadata: HashMap::new(),
        }
    }

    /// Add input.
    pub fn with_input(mut self, input_id: String) -> Self {
        self.inputs.push(input_id);
        self
    }

    /// Add output.
    pub fn with_output(mut self, output_id: String) -> Self {
        self.outputs.push(output_id);
        self
    }

    /// Set operation.
    pub fn with_operation(mut self, operation_id: String) -> Self {
        self.operation = Some(operation_id);
        self
    }

    /// Set agent.
    pub fn with_agent(mut self, agent_id: String) -> Self {
        self.agent = Some(agent_id);
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lineage_node_creation() {
        let node = LineageNode::new(NodeType::Dataset, "dataset-123".to_string())
            .with_metadata("format".to_string(), "GeoTIFF".to_string());

        assert_eq!(node.node_type, NodeType::Dataset);
        assert_eq!(node.entity_id, "dataset-123");
        assert_eq!(node.metadata.get("format"), Some(&"GeoTIFF".to_string()));
    }

    #[test]
    fn test_lineage_edge_creation() {
        let edge = LineageEdge::new(
            "node-1".to_string(),
            "node-2".to_string(),
            EdgeType::DerivedFrom,
        )
        .with_metadata("operation".to_string(), "reproject".to_string());

        assert_eq!(edge.source_id, "node-1");
        assert_eq!(edge.target_id, "node-2");
        assert_eq!(edge.edge_type, EdgeType::DerivedFrom);
    }

    #[test]
    fn test_lineage_event() {
        let event = LineageEvent::new("transform".to_string())
            .with_input("input-1".to_string())
            .with_output("output-1".to_string())
            .with_operation("op-1".to_string())
            .with_agent("user-123".to_string());

        assert_eq!(event.event_type, "transform");
        assert_eq!(event.inputs.len(), 1);
        assert_eq!(event.outputs.len(), 1);
        assert_eq!(event.operation, Some("op-1".to_string()));
        assert_eq!(event.agent, Some("user-123".to_string()));
    }
}
