//! Provenance metadata management.

use crate::error::{Result, SecurityError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// PROV-O compliant provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceMetadata {
    /// Entity URI.
    pub entity_uri: String,
    /// Activity that generated the entity.
    pub activity: Option<ActivityMetadata>,
    /// Entities used to derive this entity.
    pub derived_from: Vec<String>,
    /// Agent responsible.
    pub attributed_to: Option<String>,
    /// Generation time.
    pub generated_at: Option<DateTime<Utc>>,
    /// Invalidation time.
    pub invalidated_at: Option<DateTime<Utc>>,
    /// Additional attributes.
    pub attributes: HashMap<String, String>,
}

impl ProvenanceMetadata {
    /// Create new provenance metadata.
    pub fn new(entity_uri: String) -> Self {
        Self {
            entity_uri,
            activity: None,
            derived_from: Vec::new(),
            attributed_to: None,
            generated_at: Some(Utc::now()),
            invalidated_at: None,
            attributes: HashMap::new(),
        }
    }

    /// Set activity.
    pub fn with_activity(mut self, activity: ActivityMetadata) -> Self {
        self.activity = Some(activity);
        self
    }

    /// Add derived from entity.
    pub fn add_derived_from(mut self, entity_uri: String) -> Self {
        self.derived_from.push(entity_uri);
        self
    }

    /// Set attributed to.
    pub fn with_attribution(mut self, agent_uri: String) -> Self {
        self.attributed_to = Some(agent_uri);
        self
    }

    /// Add attribute.
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(SecurityError::from)
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(SecurityError::from)
    }
}

/// Activity metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityMetadata {
    /// Activity URI.
    pub uri: String,
    /// Activity type.
    pub activity_type: String,
    /// Start time.
    pub started_at: DateTime<Utc>,
    /// End time.
    pub ended_at: Option<DateTime<Utc>>,
    /// Associated agent.
    pub associated_with: Option<String>,
    /// Input entities.
    pub used: Vec<String>,
    /// Parameters.
    pub parameters: HashMap<String, String>,
}

impl ActivityMetadata {
    /// Create new activity metadata.
    pub fn new(uri: String, activity_type: String) -> Self {
        Self {
            uri,
            activity_type,
            started_at: Utc::now(),
            ended_at: None,
            associated_with: None,
            used: Vec::new(),
            parameters: HashMap::new(),
        }
    }

    /// Mark activity as ended.
    pub fn end(mut self) -> Self {
        self.ended_at = Some(Utc::now());
        self
    }

    /// Add used entity.
    pub fn add_used(mut self, entity_uri: String) -> Self {
        self.used.push(entity_uri);
        self
    }

    /// Set associated agent.
    pub fn with_agent(mut self, agent_uri: String) -> Self {
        self.associated_with = Some(agent_uri);
        self
    }

    /// Add parameter.
    pub fn with_parameter(mut self, key: String, value: String) -> Self {
        self.parameters.insert(key, value);
        self
    }
}

/// Agent metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    /// Agent URI.
    pub uri: String,
    /// Agent type.
    pub agent_type: AgentType,
    /// Agent name.
    pub name: String,
    /// On behalf of (for delegation).
    pub acted_on_behalf_of: Option<String>,
    /// Additional attributes.
    pub attributes: HashMap<String, String>,
}

impl AgentMetadata {
    /// Create new agent metadata.
    pub fn new(uri: String, agent_type: AgentType, name: String) -> Self {
        Self {
            uri,
            agent_type,
            name,
            acted_on_behalf_of: None,
            attributes: HashMap::new(),
        }
    }

    /// Set delegation.
    pub fn with_delegation(mut self, delegator_uri: String) -> Self {
        self.acted_on_behalf_of = Some(delegator_uri);
        self
    }

    /// Add attribute.
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

/// Agent type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    /// Person.
    Person,
    /// Software agent.
    SoftwareAgent,
    /// Organization.
    Organization,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provenance_metadata() {
        let prov = ProvenanceMetadata::new("dataset://123".to_string())
            .add_derived_from("dataset://source".to_string())
            .with_attribution("agent://user-1".to_string())
            .with_attribute("format".to_string(), "GeoTIFF".to_string());

        assert_eq!(prov.entity_uri, "dataset://123");
        assert_eq!(prov.derived_from.len(), 1);
        assert_eq!(prov.attributed_to, Some("agent://user-1".to_string()));
    }

    #[test]
    fn test_activity_metadata() {
        let activity = ActivityMetadata::new(
            "activity://transform-1".to_string(),
            "reproject".to_string(),
        )
        .add_used("dataset://input".to_string())
        .with_agent("agent://user-1".to_string())
        .with_parameter("target_crs".to_string(), "EPSG:4326".to_string())
        .end();

        assert_eq!(activity.activity_type, "reproject");
        assert_eq!(activity.used.len(), 1);
        assert!(activity.ended_at.is_some());
    }

    #[test]
    fn test_agent_metadata() {
        let agent = AgentMetadata::new(
            "agent://service-1".to_string(),
            AgentType::SoftwareAgent,
            "Processing Service".to_string(),
        )
        .with_delegation("agent://admin".to_string());

        assert_eq!(agent.agent_type, AgentType::SoftwareAgent);
        assert_eq!(agent.acted_on_behalf_of, Some("agent://admin".to_string()));
    }

    #[test]
    fn test_provenance_serialization() {
        let prov = ProvenanceMetadata::new("dataset://123".to_string());
        let json = prov.to_json().expect("Serialization failed");
        let deserialized = ProvenanceMetadata::from_json(&json).expect("Deserialization failed");

        assert_eq!(deserialized.entity_uri, "dataset://123");
    }
}
