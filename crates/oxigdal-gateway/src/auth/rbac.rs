//! Role-Based Access Control (RBAC) implementation.
//!
//! This module provides comprehensive RBAC functionality including:
//! - Role definitions and hierarchies
//! - Permission management
//! - Role assignment and inheritance
//! - Policy-based access decisions

use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Role identifier type.
pub type RoleId = String;

/// Permission identifier type.
pub type PermissionId = String;

/// A permission represents an action that can be performed on a resource type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Permission {
    /// Permission identifier
    pub id: PermissionId,
    /// Human-readable name
    pub name: String,
    /// Description of what this permission allows
    pub description: String,
    /// Resource type this permission applies to (e.g., "dataset", "layer", "feature")
    pub resource_type: String,
    /// Action type (e.g., "read", "write", "delete", "admin")
    pub action: PermissionAction,
}

impl Permission {
    /// Creates a new permission.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        resource_type: impl Into<String>,
        action: PermissionAction,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            resource_type: resource_type.into(),
            action,
        }
    }

    /// Creates a standard read permission for a resource type.
    pub fn read(resource_type: impl Into<String>) -> Self {
        let rt = resource_type.into();
        Self {
            id: format!("{}.read", rt),
            name: format!("Read {}", rt),
            description: format!("Permission to read {} resources", rt),
            resource_type: rt,
            action: PermissionAction::Read,
        }
    }

    /// Creates a standard write permission for a resource type.
    pub fn write(resource_type: impl Into<String>) -> Self {
        let rt = resource_type.into();
        Self {
            id: format!("{}.write", rt),
            name: format!("Write {}", rt),
            description: format!("Permission to write {} resources", rt),
            resource_type: rt,
            action: PermissionAction::Write,
        }
    }

    /// Creates a standard delete permission for a resource type.
    pub fn delete(resource_type: impl Into<String>) -> Self {
        let rt = resource_type.into();
        Self {
            id: format!("{}.delete", rt),
            name: format!("Delete {}", rt),
            description: format!("Permission to delete {} resources", rt),
            resource_type: rt,
            action: PermissionAction::Delete,
        }
    }

    /// Creates a standard admin permission for a resource type.
    pub fn admin(resource_type: impl Into<String>) -> Self {
        let rt = resource_type.into();
        Self {
            id: format!("{}.admin", rt),
            name: format!("Admin {}", rt),
            description: format!("Full administrative access to {} resources", rt),
            resource_type: rt,
            action: PermissionAction::Admin,
        }
    }
}

/// Permission action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionAction {
    /// Read/view access
    Read,
    /// Create/update access
    Write,
    /// Delete access
    Delete,
    /// Execute/invoke access
    Execute,
    /// Full administrative access
    Admin,
    /// Custom action
    Custom,
}

impl PermissionAction {
    /// Returns the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Execute => "execute",
            Self::Admin => "admin",
            Self::Custom => "custom",
        }
    }

    /// Checks if this action implies another action.
    pub fn implies(&self, other: &PermissionAction) -> bool {
        match self {
            Self::Admin => true, // Admin implies all
            Self::Write => matches!(other, Self::Read | Self::Write),
            Self::Delete => matches!(other, Self::Read | Self::Delete),
            _ => self == other,
        }
    }
}

/// A role represents a named set of permissions.
#[derive(Debug, Clone)]
pub struct Role {
    /// Role identifier
    pub id: RoleId,
    /// Human-readable name
    pub name: String,
    /// Role description
    pub description: String,
    /// Direct permissions assigned to this role
    pub permissions: HashSet<PermissionId>,
    /// Parent roles (for inheritance)
    pub parent_roles: HashSet<RoleId>,
    /// Role priority (higher = more privileged)
    pub priority: u32,
    /// Whether this role is a system role
    pub is_system: bool,
    /// Role metadata
    pub metadata: HashMap<String, String>,
}

impl Role {
    /// Creates a new role.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            permissions: HashSet::new(),
            parent_roles: HashSet::new(),
            priority: 0,
            is_system: false,
            metadata: HashMap::new(),
        }
    }

    /// Creates a system role.
    pub fn system(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        priority: u32,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            permissions: HashSet::new(),
            parent_roles: HashSet::new(),
            priority,
            is_system: true,
            metadata: HashMap::new(),
        }
    }

    /// Adds a permission to this role.
    pub fn with_permission(mut self, permission_id: impl Into<String>) -> Self {
        self.permissions.insert(permission_id.into());
        self
    }

    /// Adds multiple permissions to this role.
    pub fn with_permissions(mut self, permission_ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for perm in permission_ids {
            self.permissions.insert(perm.into());
        }
        self
    }

    /// Adds a parent role for inheritance.
    pub fn with_parent(mut self, parent_role_id: impl Into<String>) -> Self {
        self.parent_roles.insert(parent_role_id.into());
        self
    }

    /// Sets the role priority.
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Adds metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// RBAC manager for role and permission management.
pub struct RbacManager {
    /// All defined roles
    roles: Arc<DashMap<RoleId, Role>>,
    /// All defined permissions
    permissions: Arc<DashMap<PermissionId, Permission>>,
    /// User role assignments (user_id -> set of role_ids)
    user_roles: Arc<DashMap<String, HashSet<RoleId>>>,
    /// Computed effective permissions cache (user_id -> set of permission_ids)
    effective_permissions_cache: Arc<DashMap<String, HashSet<PermissionId>>>,
}

impl RbacManager {
    /// Creates a new RBAC manager.
    pub fn new() -> Self {
        let manager = Self {
            roles: Arc::new(DashMap::new()),
            permissions: Arc::new(DashMap::new()),
            user_roles: Arc::new(DashMap::new()),
            effective_permissions_cache: Arc::new(DashMap::new()),
        };

        // Initialize with default system roles
        manager.init_default_roles();
        manager.init_default_permissions();

        manager
    }

    /// Initializes default system roles.
    fn init_default_roles(&self) {
        // Super admin role
        let super_admin = Role::system("super_admin", "Super Administrator", "Full system access", 1000)
            .with_permission("*");
        self.roles.insert(super_admin.id.clone(), super_admin);

        // Admin role
        let admin = Role::system("admin", "Administrator", "Administrative access", 900)
            .with_permissions(vec![
                "dataset.admin",
                "layer.admin",
                "user.admin",
                "system.read",
            ]);
        self.roles.insert(admin.id.clone(), admin);

        // Editor role
        let editor = Role::system("editor", "Editor", "Can create and edit content", 500)
            .with_parent("viewer")
            .with_permissions(vec![
                "dataset.read",
                "dataset.write",
                "layer.read",
                "layer.write",
                "feature.read",
                "feature.write",
            ]);
        self.roles.insert(editor.id.clone(), editor);

        // Viewer role
        let viewer = Role::system("viewer", "Viewer", "Read-only access", 100)
            .with_permissions(vec![
                "dataset.read",
                "layer.read",
                "feature.read",
            ]);
        self.roles.insert(viewer.id.clone(), viewer);

        // Anonymous/guest role
        let anonymous = Role::system("anonymous", "Anonymous", "Public access", 0)
            .with_permission("public.read");
        self.roles.insert(anonymous.id.clone(), anonymous);
    }

    /// Initializes default permissions.
    fn init_default_permissions(&self) {
        // Wildcard permission
        let wildcard = Permission::new(
            "*",
            "All Permissions",
            "Full access to all resources",
            "*",
            PermissionAction::Admin,
        );
        self.permissions.insert(wildcard.id.clone(), wildcard);

        // Dataset permissions
        for perm in [
            Permission::read("dataset"),
            Permission::write("dataset"),
            Permission::delete("dataset"),
            Permission::admin("dataset"),
        ] {
            self.permissions.insert(perm.id.clone(), perm);
        }

        // Layer permissions
        for perm in [
            Permission::read("layer"),
            Permission::write("layer"),
            Permission::delete("layer"),
            Permission::admin("layer"),
        ] {
            self.permissions.insert(perm.id.clone(), perm);
        }

        // Feature permissions
        for perm in [
            Permission::read("feature"),
            Permission::write("feature"),
            Permission::delete("feature"),
            Permission::admin("feature"),
        ] {
            self.permissions.insert(perm.id.clone(), perm);
        }

        // User management permissions
        for perm in [
            Permission::read("user"),
            Permission::write("user"),
            Permission::delete("user"),
            Permission::admin("user"),
        ] {
            self.permissions.insert(perm.id.clone(), perm);
        }

        // System permissions
        let system_read = Permission::read("system");
        self.permissions.insert(system_read.id.clone(), system_read);

        let system_admin = Permission::admin("system");
        self.permissions.insert(system_admin.id.clone(), system_admin);

        // Public read permission
        let public_read = Permission::read("public");
        self.permissions.insert(public_read.id.clone(), public_read);
    }

    /// Registers a new role.
    pub fn register_role(&self, role: Role) -> Result<()> {
        if self.roles.contains_key(&role.id) {
            return Err(GatewayError::InvalidRequest(format!(
                "Role '{}' already exists",
                role.id
            )));
        }

        // Validate parent roles exist
        for parent_id in &role.parent_roles {
            if !self.roles.contains_key(parent_id) {
                return Err(GatewayError::InvalidRequest(format!(
                    "Parent role '{}' does not exist",
                    parent_id
                )));
            }
        }

        self.roles.insert(role.id.clone(), role);
        self.invalidate_permissions_cache();
        Ok(())
    }

    /// Updates an existing role.
    pub fn update_role(&self, role: Role) -> Result<()> {
        if !self.roles.contains_key(&role.id) {
            return Err(GatewayError::InvalidRequest(format!(
                "Role '{}' does not exist",
                role.id
            )));
        }

        // Check if trying to update system role
        if let Some(existing) = self.roles.get(&role.id) {
            if existing.is_system {
                return Err(GatewayError::AuthorizationFailed(
                    "Cannot modify system roles".to_string(),
                ));
            }
        }

        self.roles.insert(role.id.clone(), role);
        self.invalidate_permissions_cache();
        Ok(())
    }

    /// Deletes a role.
    pub fn delete_role(&self, role_id: &str) -> Result<()> {
        if let Some(existing) = self.roles.get(role_id) {
            if existing.is_system {
                return Err(GatewayError::AuthorizationFailed(
                    "Cannot delete system roles".to_string(),
                ));
            }
        }

        self.roles
            .remove(role_id)
            .ok_or_else(|| GatewayError::InvalidRequest(format!("Role '{}' not found", role_id)))?;

        self.invalidate_permissions_cache();
        Ok(())
    }

    /// Gets a role by ID.
    pub fn get_role(&self, role_id: &str) -> Option<Role> {
        self.roles.get(role_id).map(|r| r.clone())
    }

    /// Lists all roles.
    pub fn list_roles(&self) -> Vec<Role> {
        self.roles.iter().map(|r| r.value().clone()).collect()
    }

    /// Registers a new permission.
    pub fn register_permission(&self, permission: Permission) -> Result<()> {
        if self.permissions.contains_key(&permission.id) {
            return Err(GatewayError::InvalidRequest(format!(
                "Permission '{}' already exists",
                permission.id
            )));
        }

        self.permissions.insert(permission.id.clone(), permission);
        Ok(())
    }

    /// Gets a permission by ID.
    pub fn get_permission(&self, permission_id: &str) -> Option<Permission> {
        self.permissions.get(permission_id).map(|p| p.clone())
    }

    /// Lists all permissions.
    pub fn list_permissions(&self) -> Vec<Permission> {
        self.permissions.iter().map(|p| p.value().clone()).collect()
    }

    /// Assigns a role to a user.
    pub fn assign_role(&self, user_id: &str, role_id: &str) -> Result<()> {
        if !self.roles.contains_key(role_id) {
            return Err(GatewayError::InvalidRequest(format!(
                "Role '{}' does not exist",
                role_id
            )));
        }

        self.user_roles
            .entry(user_id.to_string())
            .or_insert_with(HashSet::new)
            .insert(role_id.to_string());

        // Invalidate user's effective permissions cache
        self.effective_permissions_cache.remove(user_id);

        Ok(())
    }

    /// Revokes a role from a user.
    pub fn revoke_role(&self, user_id: &str, role_id: &str) -> Result<()> {
        if let Some(mut roles) = self.user_roles.get_mut(user_id) {
            roles.remove(role_id);
            // Invalidate user's effective permissions cache
            self.effective_permissions_cache.remove(user_id);
            Ok(())
        } else {
            Err(GatewayError::InvalidRequest(format!(
                "User '{}' has no role assignments",
                user_id
            )))
        }
    }

    /// Gets roles assigned to a user.
    pub fn get_user_roles(&self, user_id: &str) -> HashSet<RoleId> {
        self.user_roles
            .get(user_id)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Gets all effective permissions for a user (including inherited).
    pub fn get_effective_permissions(&self, user_id: &str) -> HashSet<PermissionId> {
        // Check cache first
        if let Some(cached) = self.effective_permissions_cache.get(user_id) {
            return cached.clone();
        }

        let user_roles = self.get_user_roles(user_id);
        let mut effective_perms = HashSet::new();
        let mut visited_roles = HashSet::new();

        for role_id in &user_roles {
            self.collect_permissions_recursive(role_id, &mut effective_perms, &mut visited_roles);
        }

        // Cache the result
        self.effective_permissions_cache
            .insert(user_id.to_string(), effective_perms.clone());

        effective_perms
    }

    /// Recursively collects permissions from a role and its parents.
    fn collect_permissions_recursive(
        &self,
        role_id: &str,
        permissions: &mut HashSet<PermissionId>,
        visited: &mut HashSet<RoleId>,
    ) {
        if visited.contains(role_id) {
            return; // Avoid cycles
        }
        visited.insert(role_id.to_string());

        if let Some(role) = self.roles.get(role_id) {
            // Add direct permissions
            for perm in &role.permissions {
                permissions.insert(perm.clone());
            }

            // Recursively add parent permissions
            for parent_id in &role.parent_roles {
                self.collect_permissions_recursive(parent_id, permissions, visited);
            }
        }
    }

    /// Checks if a user has a specific permission.
    pub fn has_permission(&self, user_id: &str, permission_id: &str) -> bool {
        let effective_perms = self.get_effective_permissions(user_id);

        // Check for wildcard permission
        if effective_perms.contains("*") {
            return true;
        }

        // Check for exact match
        if effective_perms.contains(permission_id) {
            return true;
        }

        // Check for resource-type wildcard (e.g., "dataset.*" matches "dataset.read")
        if let Some(dot_pos) = permission_id.find('.') {
            let resource_wildcard = format!("{}.*", &permission_id[..dot_pos]);
            if effective_perms.contains(&resource_wildcard) {
                return true;
            }
        }

        false
    }

    /// Checks if a user has any of the specified permissions.
    pub fn has_any_permission(&self, user_id: &str, permission_ids: &[&str]) -> bool {
        permission_ids
            .iter()
            .any(|p| self.has_permission(user_id, p))
    }

    /// Checks if a user has all of the specified permissions.
    pub fn has_all_permissions(&self, user_id: &str, permission_ids: &[&str]) -> bool {
        permission_ids
            .iter()
            .all(|p| self.has_permission(user_id, p))
    }

    /// Checks if a user has a specific role.
    pub fn has_role(&self, user_id: &str, role_id: &str) -> bool {
        self.get_user_roles(user_id).contains(role_id)
    }

    /// Gets the highest priority role for a user.
    pub fn get_highest_priority_role(&self, user_id: &str) -> Option<Role> {
        let user_roles = self.get_user_roles(user_id);

        user_roles
            .iter()
            .filter_map(|role_id| self.roles.get(role_id).map(|r| r.clone()))
            .max_by_key(|role| role.priority)
    }

    /// Invalidates the entire permissions cache.
    fn invalidate_permissions_cache(&self) {
        self.effective_permissions_cache.clear();
    }
}

impl Default for RbacManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Access decision result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access is allowed
    Allow,
    /// Access is denied with reason
    Deny(String),
    /// Access decision is deferred to another authority
    Defer,
}

impl AccessDecision {
    /// Returns true if access is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Returns true if access is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Deny(_))
    }
}

/// Policy evaluation context.
#[derive(Debug, Clone)]
pub struct PolicyContext {
    /// User ID making the request
    pub user_id: String,
    /// Resource type being accessed
    pub resource_type: String,
    /// Resource ID being accessed
    pub resource_id: Option<String>,
    /// Action being performed
    pub action: PermissionAction,
    /// Additional context attributes
    pub attributes: HashMap<String, String>,
}

impl PolicyContext {
    /// Creates a new policy context.
    pub fn new(
        user_id: impl Into<String>,
        resource_type: impl Into<String>,
        action: PermissionAction,
    ) -> Self {
        Self {
            user_id: user_id.into(),
            resource_type: resource_type.into(),
            resource_id: None,
            action,
            attributes: HashMap::new(),
        }
    }

    /// Sets the resource ID.
    pub fn with_resource_id(mut self, resource_id: impl Into<String>) -> Self {
        self.resource_id = Some(resource_id.into());
        self
    }

    /// Adds an attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Policy trait for custom access control logic.
pub trait AccessPolicy: Send + Sync {
    /// Evaluates the policy for the given context.
    fn evaluate(&self, context: &PolicyContext, rbac: &RbacManager) -> AccessDecision;

    /// Returns the policy name.
    fn name(&self) -> &str;

    /// Returns the policy priority (higher = evaluated first).
    fn priority(&self) -> i32 {
        0
    }
}

/// Default RBAC policy that uses permission checks.
pub struct DefaultRbacPolicy;

impl AccessPolicy for DefaultRbacPolicy {
    fn evaluate(&self, context: &PolicyContext, rbac: &RbacManager) -> AccessDecision {
        let permission_id = format!("{}.{}", context.resource_type, context.action.as_str());

        if rbac.has_permission(&context.user_id, &permission_id) {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny(format!(
                "User '{}' lacks permission '{}'",
                context.user_id, permission_id
            ))
        }
    }

    fn name(&self) -> &str {
        "default_rbac"
    }

    fn priority(&self) -> i32 {
        -1000 // Low priority, evaluated last
    }
}

/// Time-based access policy.
pub struct TimeBasedPolicy {
    /// Allowed start hour (0-23)
    pub allowed_start_hour: u32,
    /// Allowed end hour (0-23)
    pub allowed_end_hour: u32,
    /// Allowed days of week (0=Sunday, 6=Saturday)
    pub allowed_days: HashSet<u32>,
}

impl TimeBasedPolicy {
    /// Creates a new time-based policy for business hours (Mon-Fri, 9-17).
    pub fn business_hours() -> Self {
        Self {
            allowed_start_hour: 9,
            allowed_end_hour: 17,
            allowed_days: [1, 2, 3, 4, 5].into_iter().collect(), // Mon-Fri
        }
    }

    /// Creates a policy allowing 24/7 access.
    pub fn always_allowed() -> Self {
        Self {
            allowed_start_hour: 0,
            allowed_end_hour: 24,
            allowed_days: (0..7).collect(),
        }
    }
}

impl AccessPolicy for TimeBasedPolicy {
    fn evaluate(&self, _context: &PolicyContext, _rbac: &RbacManager) -> AccessDecision {
        let now = chrono::Utc::now();
        let hour = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
        let day = now.format("%w").to_string().parse::<u32>().unwrap_or(0);

        if !self.allowed_days.contains(&day) {
            return AccessDecision::Deny("Access not allowed on this day".to_string());
        }

        if hour < self.allowed_start_hour || hour >= self.allowed_end_hour {
            return AccessDecision::Deny("Access not allowed at this time".to_string());
        }

        AccessDecision::Defer
    }

    fn name(&self) -> &str {
        "time_based"
    }

    fn priority(&self) -> i32 {
        100 // Evaluated early
    }
}

/// IP-based access policy.
pub struct IpBasedPolicy {
    /// Allowed IP prefixes
    pub allowed_prefixes: Vec<String>,
    /// Blocked IP prefixes
    pub blocked_prefixes: Vec<String>,
}

impl IpBasedPolicy {
    /// Creates a new IP-based policy.
    pub fn new() -> Self {
        Self {
            allowed_prefixes: Vec::new(),
            blocked_prefixes: Vec::new(),
        }
    }

    /// Adds an allowed IP prefix.
    pub fn allow_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.allowed_prefixes.push(prefix.into());
        self
    }

    /// Adds a blocked IP prefix.
    pub fn block_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.blocked_prefixes.push(prefix.into());
        self
    }
}

impl Default for IpBasedPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessPolicy for IpBasedPolicy {
    fn evaluate(&self, context: &PolicyContext, _rbac: &RbacManager) -> AccessDecision {
        if let Some(ip) = context.attributes.get("client_ip") {
            // Check blocked first
            for blocked in &self.blocked_prefixes {
                if ip.starts_with(blocked) {
                    return AccessDecision::Deny(format!("IP '{}' is blocked", ip));
                }
            }

            // If allowed list is not empty, check it
            if !self.allowed_prefixes.is_empty() {
                let is_allowed = self.allowed_prefixes.iter().any(|prefix| ip.starts_with(prefix));
                if !is_allowed {
                    return AccessDecision::Deny(format!("IP '{}' is not in allowed list", ip));
                }
            }
        }

        AccessDecision::Defer
    }

    fn name(&self) -> &str {
        "ip_based"
    }

    fn priority(&self) -> i32 {
        200 // High priority
    }
}

/// Policy engine that evaluates multiple policies.
pub struct PolicyEngine {
    /// Registered policies
    policies: Vec<Arc<dyn AccessPolicy>>,
    /// RBAC manager reference
    rbac: Arc<RbacManager>,
}

impl PolicyEngine {
    /// Creates a new policy engine.
    pub fn new(rbac: Arc<RbacManager>) -> Self {
        let mut engine = Self {
            policies: Vec::new(),
            rbac,
        };

        // Add default RBAC policy
        engine.add_policy(Arc::new(DefaultRbacPolicy));

        engine
    }

    /// Adds a policy to the engine.
    pub fn add_policy(&mut self, policy: Arc<dyn AccessPolicy>) {
        self.policies.push(policy);
        // Sort by priority (highest first)
        self.policies.sort_by_key(|p| std::cmp::Reverse(p.priority()));
    }

    /// Evaluates all policies for the given context.
    pub fn evaluate(&self, context: &PolicyContext) -> AccessDecision {
        for policy in &self.policies {
            match policy.evaluate(context, &self.rbac) {
                AccessDecision::Allow => return AccessDecision::Allow,
                AccessDecision::Deny(reason) => return AccessDecision::Deny(reason),
                AccessDecision::Defer => continue,
            }
        }

        // If all policies defer, deny by default
        AccessDecision::Deny("No policy allowed access".to_string())
    }

    /// Checks if access is allowed.
    pub fn is_allowed(&self, context: &PolicyContext) -> bool {
        self.evaluate(context).is_allowed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_creation() {
        let perm = Permission::read("dataset");
        assert_eq!(perm.id, "dataset.read");
        assert_eq!(perm.action, PermissionAction::Read);
        assert_eq!(perm.resource_type, "dataset");
    }

    #[test]
    fn test_permission_action_implies() {
        assert!(PermissionAction::Admin.implies(&PermissionAction::Read));
        assert!(PermissionAction::Admin.implies(&PermissionAction::Write));
        assert!(PermissionAction::Write.implies(&PermissionAction::Read));
        assert!(!PermissionAction::Read.implies(&PermissionAction::Write));
    }

    #[test]
    fn test_role_creation() {
        let role = Role::new("test_role", "Test Role", "A test role")
            .with_permission("dataset.read")
            .with_permission("layer.read")
            .with_priority(50);

        assert_eq!(role.id, "test_role");
        assert!(role.permissions.contains("dataset.read"));
        assert!(role.permissions.contains("layer.read"));
        assert_eq!(role.priority, 50);
    }

    #[test]
    fn test_rbac_manager_default_roles() {
        let rbac = RbacManager::new();

        assert!(rbac.get_role("super_admin").is_some());
        assert!(rbac.get_role("admin").is_some());
        assert!(rbac.get_role("editor").is_some());
        assert!(rbac.get_role("viewer").is_some());
        assert!(rbac.get_role("anonymous").is_some());
    }

    #[test]
    fn test_role_assignment() {
        let rbac = RbacManager::new();

        assert!(rbac.assign_role("user1", "viewer").is_ok());
        assert!(rbac.has_role("user1", "viewer"));
        assert!(!rbac.has_role("user1", "admin"));
    }

    #[test]
    fn test_permission_check() {
        let rbac = RbacManager::new();

        assert!(rbac.assign_role("user1", "viewer").is_ok());
        assert!(rbac.has_permission("user1", "dataset.read"));
        assert!(!rbac.has_permission("user1", "dataset.write"));
    }

    #[test]
    fn test_super_admin_wildcard() {
        let rbac = RbacManager::new();

        assert!(rbac.assign_role("admin_user", "super_admin").is_ok());
        assert!(rbac.has_permission("admin_user", "anything.read"));
        assert!(rbac.has_permission("admin_user", "anything.admin"));
    }

    #[test]
    fn test_role_inheritance() {
        let rbac = RbacManager::new();

        // Editor inherits from viewer
        assert!(rbac.assign_role("editor_user", "editor").is_ok());

        // Should have viewer's permissions
        assert!(rbac.has_permission("editor_user", "dataset.read"));
        // And editor's own permissions
        assert!(rbac.has_permission("editor_user", "dataset.write"));
    }

    #[test]
    fn test_custom_role_registration() {
        let rbac = RbacManager::new();

        let custom_role = Role::new("data_scientist", "Data Scientist", "Can analyze data")
            .with_parent("viewer")
            .with_permission("analysis.execute");

        assert!(rbac.register_permission(Permission::new(
            "analysis.execute",
            "Execute Analysis",
            "Can run data analysis",
            "analysis",
            PermissionAction::Execute,
        )).is_ok());

        assert!(rbac.register_role(custom_role).is_ok());
        assert!(rbac.assign_role("scientist1", "data_scientist").is_ok());

        assert!(rbac.has_permission("scientist1", "analysis.execute"));
        assert!(rbac.has_permission("scientist1", "dataset.read")); // Inherited
    }

    #[test]
    fn test_policy_engine() {
        let rbac = Arc::new(RbacManager::new());
        let engine = PolicyEngine::new(Arc::clone(&rbac));

        assert!(rbac.assign_role("user1", "viewer").is_ok());

        let context = PolicyContext::new("user1", "dataset", PermissionAction::Read);
        assert!(engine.is_allowed(&context));

        let context = PolicyContext::new("user1", "dataset", PermissionAction::Write);
        assert!(!engine.is_allowed(&context));
    }

    #[test]
    fn test_access_decision() {
        let allow = AccessDecision::Allow;
        assert!(allow.is_allowed());
        assert!(!allow.is_denied());

        let deny = AccessDecision::Deny("reason".to_string());
        assert!(!deny.is_allowed());
        assert!(deny.is_denied());
    }

    #[test]
    fn test_highest_priority_role() {
        let rbac = RbacManager::new();

        assert!(rbac.assign_role("multi_role", "viewer").is_ok());
        assert!(rbac.assign_role("multi_role", "editor").is_ok());

        let highest = rbac.get_highest_priority_role("multi_role");
        assert!(highest.is_some());
        assert_eq!(highest.map(|r| r.id), Some("editor".to_string()));
    }

    #[test]
    fn test_revoke_role() {
        let rbac = RbacManager::new();

        assert!(rbac.assign_role("user1", "viewer").is_ok());
        assert!(rbac.has_role("user1", "viewer"));

        assert!(rbac.revoke_role("user1", "viewer").is_ok());
        assert!(!rbac.has_role("user1", "viewer"));
    }

    #[test]
    fn test_cannot_delete_system_role() {
        let rbac = RbacManager::new();

        let result = rbac.delete_role("admin");
        assert!(result.is_err());
    }

    #[test]
    fn test_ip_based_policy() {
        let policy = IpBasedPolicy::new()
            .allow_prefix("192.168.")
            .block_prefix("192.168.1.100");

        let rbac = RbacManager::new();

        let context = PolicyContext::new("user1", "resource", PermissionAction::Read)
            .with_attribute("client_ip", "192.168.1.50");
        assert!(matches!(policy.evaluate(&context, &rbac), AccessDecision::Defer));

        let context = PolicyContext::new("user1", "resource", PermissionAction::Read)
            .with_attribute("client_ip", "192.168.1.100");
        assert!(matches!(policy.evaluate(&context, &rbac), AccessDecision::Deny(_)));

        let context = PolicyContext::new("user1", "resource", PermissionAction::Read)
            .with_attribute("client_ip", "10.0.0.1");
        assert!(matches!(policy.evaluate(&context, &rbac), AccessDecision::Deny(_)));
    }
}
