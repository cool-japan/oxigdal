//! Role-Based Access Control (RBAC).

use crate::access_control::{
    AccessControlEvaluator, AccessDecision, AccessRequest, Action, ResourceType,
    permissions::Permission, roles::Role,
};
use crate::error::{Result, SecurityError};
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;

/// RBAC policy engine.
pub struct RbacEngine {
    /// Role assignments (subject_id -> role_ids).
    role_assignments: Arc<DashMap<String, HashSet<String>>>,
    /// Roles (role_id -> Role).
    roles: Arc<DashMap<String, Role>>,
    /// Permissions (permission_id -> Permission).
    permissions: Arc<DashMap<String, Permission>>,
    /// Role inheritance (child_role_id -> parent_role_ids).
    role_inheritance: Arc<DashMap<String, HashSet<String>>>,
}

impl RbacEngine {
    /// Create a new RBAC engine.
    pub fn new() -> Self {
        Self {
            role_assignments: Arc::new(DashMap::new()),
            roles: Arc::new(DashMap::new()),
            permissions: Arc::new(DashMap::new()),
            role_inheritance: Arc::new(DashMap::new()),
        }
    }

    /// Add a role.
    pub fn add_role(&self, role: Role) -> Result<()> {
        self.roles.insert(role.id.clone(), role);
        Ok(())
    }

    /// Get a role by ID.
    pub fn get_role(&self, role_id: &str) -> Option<Role> {
        self.roles.get(role_id).map(|r| r.clone())
    }

    /// Remove a role.
    pub fn remove_role(&self, role_id: &str) -> Result<()> {
        self.roles.remove(role_id);
        self.role_inheritance.remove(role_id);

        // Remove role assignments
        for mut assignment in self.role_assignments.iter_mut() {
            assignment.value_mut().remove(role_id);
        }

        Ok(())
    }

    /// List all roles.
    pub fn list_roles(&self) -> Vec<Role> {
        self.roles.iter().map(|r| r.value().clone()).collect()
    }

    /// Add a permission.
    pub fn add_permission(&self, permission: Permission) -> Result<()> {
        self.permissions.insert(permission.id.clone(), permission);
        Ok(())
    }

    /// Get a permission by ID.
    pub fn get_permission(&self, permission_id: &str) -> Option<Permission> {
        self.permissions.get(permission_id).map(|p| p.clone())
    }

    /// Assign a role to a subject.
    pub fn assign_role(&self, subject_id: &str, role_id: &str) -> Result<()> {
        // Verify role exists
        if !self.roles.contains_key(role_id) {
            return Err(SecurityError::role_not_found(role_id));
        }

        self.role_assignments
            .entry(subject_id.to_string())
            .or_default()
            .insert(role_id.to_string());

        Ok(())
    }

    /// Revoke a role from a subject.
    pub fn revoke_role(&self, subject_id: &str, role_id: &str) -> Result<()> {
        if let Some(mut roles) = self.role_assignments.get_mut(subject_id) {
            roles.remove(role_id);
        }
        Ok(())
    }

    /// Get roles assigned to a subject.
    pub fn get_subject_roles(&self, subject_id: &str) -> Vec<String> {
        self.role_assignments
            .get(subject_id)
            .map(|roles| roles.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Set role inheritance (child inherits from parent).
    pub fn set_role_inheritance(&self, child_role_id: &str, parent_role_id: &str) -> Result<()> {
        // Verify both roles exist
        if !self.roles.contains_key(child_role_id) {
            return Err(SecurityError::role_not_found(child_role_id));
        }
        if !self.roles.contains_key(parent_role_id) {
            return Err(SecurityError::role_not_found(parent_role_id));
        }

        // Check for circular inheritance
        if self.would_create_cycle(child_role_id, parent_role_id) {
            return Err(SecurityError::policy_evaluation(
                "Circular role inheritance detected",
            ));
        }

        self.role_inheritance
            .entry(child_role_id.to_string())
            .or_default()
            .insert(parent_role_id.to_string());

        Ok(())
    }

    /// Get all roles for a subject including inherited roles.
    pub fn get_effective_roles(&self, subject_id: &str) -> HashSet<String> {
        let mut effective_roles = HashSet::new();
        let direct_roles = self.get_subject_roles(subject_id);

        for role_id in direct_roles {
            self.collect_inherited_roles(&role_id, &mut effective_roles);
        }

        effective_roles
    }

    /// Collect all inherited roles recursively.
    fn collect_inherited_roles(&self, role_id: &str, collected: &mut HashSet<String>) {
        if collected.contains(role_id) {
            return;
        }

        collected.insert(role_id.to_string());

        if let Some(parents) = self.role_inheritance.get(role_id) {
            for parent_id in parents.iter() {
                self.collect_inherited_roles(parent_id, collected);
            }
        }
    }

    /// Check if adding inheritance would create a cycle.
    fn would_create_cycle(&self, child_id: &str, parent_id: &str) -> bool {
        let mut visited = HashSet::new();
        self.has_cycle(parent_id, child_id, &mut visited)
    }

    /// Check for cycles in role inheritance.
    fn has_cycle(&self, current: &str, target: &str, visited: &mut HashSet<String>) -> bool {
        if current == target {
            return true;
        }

        if visited.contains(current) {
            return false;
        }

        visited.insert(current.to_string());

        if let Some(parents) = self.role_inheritance.get(current) {
            for parent in parents.iter() {
                if self.has_cycle(parent, target, visited) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if a subject has a specific permission.
    pub fn has_permission(
        &self,
        subject_id: &str,
        action: Action,
        resource_type: ResourceType,
    ) -> bool {
        let effective_roles = self.get_effective_roles(subject_id);

        for role_id in effective_roles {
            if let Some(role) = self.roles.get(&role_id) {
                for permission_id in &role.permissions {
                    if let Some(permission) = self.permissions.get(permission_id) {
                        if permission.action == action && permission.resource_type == resource_type
                        {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Get all permissions for a subject.
    pub fn get_subject_permissions(&self, subject_id: &str) -> Vec<Permission> {
        let effective_roles = self.get_effective_roles(subject_id);
        let mut permissions = Vec::new();
        let mut seen = HashSet::new();

        for role_id in effective_roles {
            if let Some(role) = self.roles.get(&role_id) {
                for permission_id in &role.permissions {
                    if !seen.contains(permission_id) {
                        if let Some(permission) = self.permissions.get(permission_id) {
                            permissions.push(permission.clone());
                            seen.insert(permission_id.clone());
                        }
                    }
                }
            }
        }

        permissions
    }

    /// Clear all role assignments.
    pub fn clear_assignments(&self) {
        self.role_assignments.clear();
    }

    /// Clear all roles.
    pub fn clear_roles(&self) {
        self.roles.clear();
        self.role_inheritance.clear();
    }

    /// Clear all permissions.
    pub fn clear_permissions(&self) {
        self.permissions.clear();
    }
}

impl Default for RbacEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessControlEvaluator for RbacEngine {
    fn evaluate(&self, request: &AccessRequest) -> Result<AccessDecision> {
        let has_permission = self.has_permission(
            &request.subject.id,
            request.action,
            request.resource.resource_type,
        );

        if has_permission {
            Ok(AccessDecision::Allow)
        } else {
            Ok(AccessDecision::Deny)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access_control::permissions::Permission;
    use crate::access_control::roles::Role;

    #[test]
    fn test_role_assignment() {
        let engine = RbacEngine::new();
        let role = Role::new("admin".to_string(), "Administrator".to_string());

        engine.add_role(role).expect("Failed to add role");
        engine
            .assign_role("user-123", "admin")
            .expect("Failed to assign role");

        let roles = engine.get_subject_roles("user-123");
        assert_eq!(roles.len(), 1);
        assert!(roles.contains(&"admin".to_string()));
    }

    #[test]
    fn test_role_revocation() {
        let engine = RbacEngine::new();
        let role = Role::new("admin".to_string(), "Administrator".to_string());

        engine.add_role(role).expect("Failed to add role");
        engine
            .assign_role("user-123", "admin")
            .expect("Failed to assign role");
        engine
            .revoke_role("user-123", "admin")
            .expect("Failed to revoke role");

        let roles = engine.get_subject_roles("user-123");
        assert_eq!(roles.len(), 0);
    }

    #[test]
    fn test_role_inheritance() {
        let engine = RbacEngine::new();

        let admin_role = Role::new("admin".to_string(), "Administrator".to_string());
        let user_role = Role::new("user".to_string(), "User".to_string());

        engine
            .add_role(admin_role)
            .expect("Failed to add admin role");
        engine.add_role(user_role).expect("Failed to add user role");

        engine
            .set_role_inheritance("admin", "user")
            .expect("Failed to set inheritance");

        engine
            .assign_role("user-123", "admin")
            .expect("Failed to assign role");

        let effective_roles = engine.get_effective_roles("user-123");
        assert_eq!(effective_roles.len(), 2);
        assert!(effective_roles.contains("admin"));
        assert!(effective_roles.contains("user"));
    }

    #[test]
    fn test_circular_inheritance_prevention() {
        let engine = RbacEngine::new();

        let role_a = Role::new("role-a".to_string(), "Role A".to_string());
        let role_b = Role::new("role-b".to_string(), "Role B".to_string());

        engine.add_role(role_a).expect("Failed to add role A");
        engine.add_role(role_b).expect("Failed to add role B");

        engine
            .set_role_inheritance("role-a", "role-b")
            .expect("Failed to set inheritance");

        // This should fail due to circular dependency
        let result = engine.set_role_inheritance("role-b", "role-a");
        assert!(result.is_err());
    }

    #[test]
    fn test_permission_check() {
        let engine = RbacEngine::new();

        let permission = Permission::new(
            "read-dataset".to_string(),
            "Read Dataset".to_string(),
            Action::Read,
            ResourceType::Dataset,
        );

        let mut role = Role::new("viewer".to_string(), "Viewer".to_string());
        role.add_permission("read-dataset".to_string());

        engine
            .add_permission(permission)
            .expect("Failed to add permission");
        engine.add_role(role).expect("Failed to add role");
        engine
            .assign_role("user-123", "viewer")
            .expect("Failed to assign role");

        assert!(engine.has_permission("user-123", Action::Read, ResourceType::Dataset));
        assert!(!engine.has_permission("user-123", Action::Write, ResourceType::Dataset));
    }
}
