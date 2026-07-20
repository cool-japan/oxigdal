//! Permission definitions.

use crate::access_control::{Action, ResourceType};
use serde::{Deserialize, Serialize};

/// Permission definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Permission {
    /// Permission ID.
    pub id: String,
    /// Permission name.
    pub name: String,
    /// Action allowed.
    pub action: Action,
    /// Resource type.
    pub resource_type: ResourceType,
    /// Resource ID pattern (glob pattern, None means all).
    pub resource_pattern: Option<String>,
    /// Description.
    pub description: Option<String>,
}

impl Permission {
    /// Create a new permission.
    pub fn new(id: String, name: String, action: Action, resource_type: ResourceType) -> Self {
        Self {
            id,
            name,
            action,
            resource_type,
            resource_pattern: None,
            description: None,
        }
    }

    /// Set resource pattern.
    pub fn with_pattern(mut self, pattern: String) -> Self {
        self.resource_pattern = Some(pattern);
        self
    }

    /// Set description.
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Check if permission matches a resource ID.
    pub fn matches_resource(&self, resource_id: &str) -> bool {
        if let Some(ref pattern) = self.resource_pattern {
            glob_match(pattern, resource_id)
        } else {
            true // No pattern means all resources
        }
    }
}

/// Simple glob pattern matching.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('*').collect();

    if pattern_parts.len() == 1 {
        // No wildcards - exact match
        return pattern == text;
    }

    let mut text_idx = 0;
    for (i, part) in pattern_parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if i == 0 {
            // First part - must match start
            if !text.starts_with(part) {
                return false;
            }
            text_idx = part.len();
        } else if i == pattern_parts.len() - 1 {
            // Last part - must match end
            if !text.ends_with(part) {
                return false;
            }
        } else {
            // Middle part - find in remaining text
            if let Some(pos) = text[text_idx..].find(part) {
                text_idx += pos + part.len();
            } else {
                return false;
            }
        }
    }

    true
}

/// Permission set builder.
pub struct PermissionSet {
    permissions: Vec<Permission>,
}

impl PermissionSet {
    /// Create a new permission set.
    pub fn new() -> Self {
        Self {
            permissions: Vec::new(),
        }
    }

    /// Add a permission.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }

    /// Build the permission set.
    pub fn build(self) -> Vec<Permission> {
        self.permissions
    }

    /// Create read-only permissions for all resource types.
    pub fn read_only() -> Self {
        let mut set = Self::new();

        for resource_type in &[
            ResourceType::Dataset,
            ResourceType::Layer,
            ResourceType::Feature,
            ResourceType::Raster,
            ResourceType::File,
            ResourceType::Directory,
        ] {
            set = set.add(Permission::new(
                format!("read-{:?}", resource_type).to_lowercase(),
                format!("Read {:?}", resource_type),
                Action::Read,
                *resource_type,
            ));
        }

        set
    }

    /// Create read-write permissions for all resource types.
    pub fn read_write() -> Self {
        let mut set = Self::read_only();

        for resource_type in &[
            ResourceType::Dataset,
            ResourceType::Layer,
            ResourceType::Feature,
            ResourceType::Raster,
            ResourceType::File,
            ResourceType::Directory,
        ] {
            for action in &[Action::Write, Action::Create, Action::Update] {
                set = set.add(Permission::new(
                    format!("{:?}-{:?}", action, resource_type).to_lowercase(),
                    format!("{:?} {:?}", action, resource_type),
                    *action,
                    *resource_type,
                ));
            }
        }

        set
    }

    /// Create admin permissions (all actions on all resource types).
    pub fn admin() -> Self {
        let mut set = Self::read_write();

        for resource_type in &[
            ResourceType::Dataset,
            ResourceType::Layer,
            ResourceType::Feature,
            ResourceType::Raster,
            ResourceType::File,
            ResourceType::Directory,
            ResourceType::Service,
            ResourceType::Tenant,
        ] {
            for action in &[Action::Delete, Action::Admin, Action::Execute, Action::List] {
                set = set.add(Permission::new(
                    format!("{:?}-{:?}", action, resource_type).to_lowercase(),
                    format!("{:?} {:?}", action, resource_type),
                    *action,
                    *resource_type,
                ));
            }
        }

        set
    }
}

impl Default for PermissionSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_creation() {
        let permission = Permission::new(
            "read-dataset".to_string(),
            "Read Dataset".to_string(),
            Action::Read,
            ResourceType::Dataset,
        );

        assert_eq!(permission.id, "read-dataset");
        assert_eq!(permission.action, Action::Read);
        assert_eq!(permission.resource_type, ResourceType::Dataset);
    }

    #[test]
    fn test_permission_with_pattern() {
        let permission = Permission::new(
            "read-dataset".to_string(),
            "Read Dataset".to_string(),
            Action::Read,
            ResourceType::Dataset,
        )
        .with_pattern("dataset-*".to_string());

        assert_eq!(permission.resource_pattern, Some("dataset-*".to_string()));
        assert!(permission.matches_resource("dataset-123"));
        assert!(permission.matches_resource("dataset-abc"));
        assert!(!permission.matches_resource("other-123"));
    }

    #[test]
    fn test_glob_matching() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("test-*", "test-123"));
        assert!(glob_match("*-test", "123-test"));
        assert!(glob_match("*test*", "123test456"));
        assert!(glob_match("exact", "exact"));

        assert!(!glob_match("test-*", "other-123"));
        assert!(!glob_match("exact", "not-exact"));
    }

    #[test]
    fn test_permission_set_read_only() {
        let permissions = PermissionSet::read_only().build();
        assert!(!permissions.is_empty());
        assert!(permissions.iter().all(|p| p.action == Action::Read));
    }

    #[test]
    fn test_permission_set_read_write() {
        let permissions = PermissionSet::read_write().build();
        assert!(!permissions.is_empty());

        let has_read = permissions.iter().any(|p| p.action == Action::Read);
        let has_write = permissions.iter().any(|p| p.action == Action::Write);
        assert!(has_read);
        assert!(has_write);
    }

    #[test]
    fn test_permission_set_admin() {
        let permissions = PermissionSet::admin().build();
        assert!(!permissions.is_empty());

        let has_admin = permissions.iter().any(|p| p.action == Action::Admin);
        let has_delete = permissions.iter().any(|p| p.action == Action::Delete);
        assert!(has_admin);
        assert!(has_delete);
    }
}
