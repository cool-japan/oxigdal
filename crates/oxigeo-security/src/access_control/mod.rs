//! Access control framework.

pub mod abac;
pub mod permissions;
pub mod policies;
pub mod rbac;
pub mod roles;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Subject (user or service) requesting access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Subject {
    /// Subject ID.
    pub id: String,
    /// Subject type (user, service, etc.).
    pub subject_type: SubjectType,
    /// Subject attributes.
    pub attributes: HashMap<String, String>,
}

impl Subject {
    /// Create a new subject.
    pub fn new(id: String, subject_type: SubjectType) -> Self {
        Self {
            id,
            subject_type,
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute.
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }

    /// Get an attribute value.
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }
}

/// Subject type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubjectType {
    /// Human user.
    User,
    /// Service account.
    Service,
    /// API key.
    ApiKey,
    /// System (internal).
    System,
}

/// Resource being accessed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resource {
    /// Resource ID.
    pub id: String,
    /// Resource type.
    pub resource_type: ResourceType,
    /// Resource attributes.
    pub attributes: HashMap<String, String>,
    /// Parent resource (for hierarchical resources).
    pub parent: Option<Box<Resource>>,
}

impl Resource {
    /// Create a new resource.
    pub fn new(id: String, resource_type: ResourceType) -> Self {
        Self {
            id,
            resource_type,
            attributes: HashMap::new(),
            parent: None,
        }
    }

    /// Add an attribute.
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }

    /// Set parent resource.
    pub fn with_parent(mut self, parent: Resource) -> Self {
        self.parent = Some(Box::new(parent));
        self
    }

    /// Get an attribute value.
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }
}

/// Resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    /// Dataset.
    Dataset,
    /// Layer.
    Layer,
    /// Feature.
    Feature,
    /// Raster.
    Raster,
    /// File.
    File,
    /// Directory.
    Directory,
    /// Service.
    Service,
    /// Tenant.
    Tenant,
}

/// Action to be performed on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    /// Read access.
    Read,
    /// Write access.
    Write,
    /// Delete access.
    Delete,
    /// Execute access.
    Execute,
    /// List access.
    List,
    /// Create access.
    Create,
    /// Update access.
    Update,
    /// Admin access.
    Admin,
}

impl Action {
    /// Get all actions.
    pub fn all() -> Vec<Action> {
        vec![
            Action::Read,
            Action::Write,
            Action::Delete,
            Action::Execute,
            Action::List,
            Action::Create,
            Action::Update,
            Action::Admin,
        ]
    }
}

/// Access decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessDecision {
    /// Access allowed.
    Allow,
    /// Access denied.
    Deny,
}

/// Access request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    /// Subject requesting access.
    pub subject: Subject,
    /// Resource being accessed.
    pub resource: Resource,
    /// Action to be performed.
    pub action: Action,
    /// Context information.
    pub context: AccessContext,
}

impl AccessRequest {
    /// Create a new access request.
    pub fn new(
        subject: Subject,
        resource: Resource,
        action: Action,
        context: AccessContext,
    ) -> Self {
        Self {
            subject,
            resource,
            action,
            context,
        }
    }
}

/// Access context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessContext {
    /// Request time.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Source IP address.
    pub source_ip: Option<String>,
    /// Tenant ID.
    pub tenant_id: Option<String>,
    /// Additional context attributes.
    pub attributes: HashMap<String, String>,
}

impl Default for AccessContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessContext {
    /// Create a new access context.
    pub fn new() -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            source_ip: None,
            tenant_id: None,
            attributes: HashMap::new(),
        }
    }

    /// Set source IP.
    pub fn with_source_ip(mut self, ip: String) -> Self {
        self.source_ip = Some(ip);
        self
    }

    /// Set tenant ID.
    pub fn with_tenant_id(mut self, tenant_id: String) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// Add an attribute.
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }

    /// Get an attribute value.
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }
}

/// Access control evaluator trait.
pub trait AccessControlEvaluator: Send + Sync {
    /// Evaluate an access request.
    fn evaluate(&self, request: &AccessRequest) -> Result<AccessDecision>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subject_creation() {
        let subject = Subject::new("user-123".to_string(), SubjectType::User)
            .with_attribute("department".to_string(), "engineering".to_string());

        assert_eq!(subject.id, "user-123");
        assert_eq!(subject.subject_type, SubjectType::User);
        assert_eq!(
            subject.get_attribute("department"),
            Some(&"engineering".to_string())
        );
    }

    #[test]
    fn test_resource_creation() {
        let resource = Resource::new("dataset-456".to_string(), ResourceType::Dataset)
            .with_attribute("classification".to_string(), "confidential".to_string());

        assert_eq!(resource.id, "dataset-456");
        assert_eq!(resource.resource_type, ResourceType::Dataset);
        assert_eq!(
            resource.get_attribute("classification"),
            Some(&"confidential".to_string())
        );
    }

    #[test]
    fn test_access_context() {
        let context = AccessContext::new()
            .with_source_ip("192.168.1.1".to_string())
            .with_tenant_id("tenant-001".to_string())
            .with_attribute("region".to_string(), "us-west-2".to_string());

        assert_eq!(context.source_ip, Some("192.168.1.1".to_string()));
        assert_eq!(context.tenant_id, Some("tenant-001".to_string()));
        assert_eq!(
            context.get_attribute("region"),
            Some(&"us-west-2".to_string())
        );
    }

    #[test]
    fn test_all_actions() {
        let actions = Action::all();
        assert_eq!(actions.len(), 8);
        assert!(actions.contains(&Action::Read));
        assert!(actions.contains(&Action::Write));
    }
}
