//! Role management.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Role definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Role {
    /// Role ID.
    pub id: String,
    /// Role name.
    pub name: String,
    /// Role description.
    pub description: Option<String>,
    /// Permission IDs.
    pub permissions: HashSet<String>,
    /// Role metadata.
    pub metadata: std::collections::HashMap<String, String>,
}

impl Role {
    /// Create a new role.
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            description: None,
            permissions: HashSet::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add a permission.
    pub fn add_permission(&mut self, permission_id: String) {
        self.permissions.insert(permission_id);
    }

    /// Remove a permission.
    pub fn remove_permission(&mut self, permission_id: &str) {
        self.permissions.remove(permission_id);
    }

    /// Check if role has a permission.
    pub fn has_permission(&self, permission_id: &str) -> bool {
        self.permissions.contains(permission_id)
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get metadata value.
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

/// Predefined role builder.
pub struct RoleBuilder;

impl RoleBuilder {
    /// Create a viewer role (read-only access).
    pub fn viewer() -> Role {
        Role::new("viewer".to_string(), "Viewer".to_string())
            .with_description("Read-only access to resources".to_string())
    }

    /// Create an editor role (read-write access).
    pub fn editor() -> Role {
        Role::new("editor".to_string(), "Editor".to_string())
            .with_description("Read and write access to resources".to_string())
    }

    /// Create an admin role (full access).
    pub fn admin() -> Role {
        Role::new("admin".to_string(), "Administrator".to_string())
            .with_description("Full administrative access".to_string())
    }

    /// Create a data scientist role.
    pub fn data_scientist() -> Role {
        Role::new("data-scientist".to_string(), "Data Scientist".to_string())
            .with_description("Access for data analysis and ML".to_string())
    }

    /// Create a service account role.
    pub fn service_account() -> Role {
        Role::new("service-account".to_string(), "Service Account".to_string())
            .with_description("Role for service accounts".to_string())
    }

    /// Create a guest role (minimal access).
    pub fn guest() -> Role {
        Role::new("guest".to_string(), "Guest".to_string())
            .with_description("Minimal access for guests".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_creation() {
        let role = Role::new("admin".to_string(), "Administrator".to_string())
            .with_description("Admin role".to_string());

        assert_eq!(role.id, "admin");
        assert_eq!(role.name, "Administrator");
        assert_eq!(role.description, Some("Admin role".to_string()));
    }

    #[test]
    fn test_permission_management() {
        let mut role = Role::new("editor".to_string(), "Editor".to_string());

        role.add_permission("read-dataset".to_string());
        role.add_permission("write-dataset".to_string());

        assert_eq!(role.permissions.len(), 2);
        assert!(role.has_permission("read-dataset"));
        assert!(role.has_permission("write-dataset"));

        role.remove_permission("write-dataset");
        assert_eq!(role.permissions.len(), 1);
        assert!(!role.has_permission("write-dataset"));
    }

    #[test]
    fn test_role_metadata() {
        let role = Role::new("custom".to_string(), "Custom Role".to_string())
            .with_metadata("department".to_string(), "engineering".to_string());

        assert_eq!(
            role.get_metadata("department"),
            Some(&"engineering".to_string())
        );
    }

    #[test]
    fn test_predefined_roles() {
        let viewer = RoleBuilder::viewer();
        assert_eq!(viewer.id, "viewer");

        let editor = RoleBuilder::editor();
        assert_eq!(editor.id, "editor");

        let admin = RoleBuilder::admin();
        assert_eq!(admin.id, "admin");
    }
}
