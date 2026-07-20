//! Resource-level permissions implementation.
//!
//! This module provides fine-grained access control at the resource level,
//! supporting per-resource permissions, ownership, and sharing.

use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Resource identifier type.
pub type ResourceId = String;

/// Resource type identifier.
pub type ResourceType = String;

/// A resource represents an entity that can be protected.
#[derive(Debug, Clone)]
pub struct Resource {
    /// Unique resource identifier
    pub id: ResourceId,
    /// Resource type (e.g., "dataset", "layer", "project")
    pub resource_type: ResourceType,
    /// Owner user ID
    pub owner_id: String,
    /// Parent resource ID (for hierarchical resources)
    pub parent_id: Option<ResourceId>,
    /// Resource creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Resource metadata
    pub metadata: HashMap<String, String>,
}

impl Resource {
    /// Creates a new resource.
    pub fn new(
        id: impl Into<String>,
        resource_type: impl Into<String>,
        owner_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            resource_type: resource_type.into(),
            owner_id: owner_id.into(),
            parent_id: None,
            created_at: chrono::Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Sets the parent resource.
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Adds metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Resource permission grant.
#[derive(Debug, Clone)]
pub struct ResourceGrant {
    /// Resource ID this grant applies to
    pub resource_id: ResourceId,
    /// Grantee (user ID or group ID)
    pub grantee: Grantee,
    /// Granted permissions
    pub permissions: HashSet<ResourcePermission>,
    /// Grant expiration (optional)
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether this grant can be further shared
    pub can_share: bool,
    /// Grant creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Who created this grant
    pub created_by: String,
}

impl ResourceGrant {
    /// Creates a new resource grant.
    pub fn new(
        resource_id: impl Into<String>,
        grantee: Grantee,
        permissions: impl IntoIterator<Item = ResourcePermission>,
        created_by: impl Into<String>,
    ) -> Self {
        Self {
            resource_id: resource_id.into(),
            grantee,
            permissions: permissions.into_iter().collect(),
            expires_at: None,
            can_share: false,
            created_at: chrono::Utc::now(),
            created_by: created_by.into(),
        }
    }

    /// Sets the expiration time.
    pub fn with_expiration(mut self, expires_at: chrono::DateTime<chrono::Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Enables sharing.
    pub fn with_sharing(mut self) -> Self {
        self.can_share = true;
        self
    }

    /// Checks if the grant has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| chrono::Utc::now() > exp)
            .unwrap_or(false)
    }

    /// Checks if the grant includes a specific permission.
    pub fn has_permission(&self, permission: ResourcePermission) -> bool {
        // Owner permission implies all
        if self.permissions.contains(&ResourcePermission::Owner) {
            return true;
        }

        // Admin permission implies read/write/delete
        if self.permissions.contains(&ResourcePermission::Admin) {
            return matches!(
                permission,
                ResourcePermission::Read
                    | ResourcePermission::Write
                    | ResourcePermission::Delete
                    | ResourcePermission::Admin
            );
        }

        // Write implies read
        if self.permissions.contains(&ResourcePermission::Write) && permission == ResourcePermission::Read {
            return true;
        }

        self.permissions.contains(&permission)
    }
}

/// Grantee types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Grantee {
    /// Individual user
    User(String),
    /// User group
    Group(String),
    /// All authenticated users
    Authenticated,
    /// Public (anyone)
    Public,
}

impl Grantee {
    /// Checks if this grantee matches the given user.
    pub fn matches_user(&self, user_id: &str, user_groups: &HashSet<String>) -> bool {
        match self {
            Self::User(id) => id == user_id,
            Self::Group(group_id) => user_groups.contains(group_id),
            Self::Authenticated => true,
            Self::Public => true,
        }
    }
}

/// Resource-level permission types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourcePermission {
    /// Read/view the resource
    Read,
    /// Modify the resource
    Write,
    /// Delete the resource
    Delete,
    /// Execute/invoke the resource
    Execute,
    /// Share the resource with others
    Share,
    /// Administrative control over the resource
    Admin,
    /// Full owner control
    Owner,
}

impl ResourcePermission {
    /// Returns the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Execute => "execute",
            Self::Share => "share",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }

    /// Parses from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "delete" => Some(Self::Delete),
            "execute" => Some(Self::Execute),
            "share" => Some(Self::Share),
            "admin" => Some(Self::Admin),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

/// Resource permission manager.
pub struct ResourcePermissionManager {
    /// Resources registry
    resources: Arc<DashMap<ResourceId, Resource>>,
    /// Permission grants (resource_id -> grants)
    grants: Arc<DashMap<ResourceId, Vec<ResourceGrant>>>,
    /// User groups (user_id -> group_ids)
    user_groups: Arc<DashMap<String, HashSet<String>>>,
    /// Resource type hierarchy (parent_type -> child_types)
    type_hierarchy: Arc<DashMap<ResourceType, HashSet<ResourceType>>>,
}

impl ResourcePermissionManager {
    /// Creates a new resource permission manager.
    pub fn new() -> Self {
        Self {
            resources: Arc::new(DashMap::new()),
            grants: Arc::new(DashMap::new()),
            user_groups: Arc::new(DashMap::new()),
            type_hierarchy: Arc::new(DashMap::new()),
        }
    }

    /// Registers a resource.
    pub fn register_resource(&self, resource: Resource) -> Result<()> {
        if self.resources.contains_key(&resource.id) {
            return Err(GatewayError::InvalidRequest(format!(
                "Resource '{}' already exists",
                resource.id
            )));
        }

        // Create owner grant automatically
        let owner_grant = ResourceGrant::new(
            &resource.id,
            Grantee::User(resource.owner_id.clone()),
            vec![ResourcePermission::Owner],
            &resource.owner_id,
        );

        self.grants
            .entry(resource.id.clone())
            .or_insert_with(Vec::new)
            .push(owner_grant);

        self.resources.insert(resource.id.clone(), resource);

        Ok(())
    }

    /// Gets a resource by ID.
    pub fn get_resource(&self, resource_id: &str) -> Option<Resource> {
        self.resources.get(resource_id).map(|r| r.clone())
    }

    /// Updates a resource.
    pub fn update_resource(&self, resource: Resource) -> Result<()> {
        if !self.resources.contains_key(&resource.id) {
            return Err(GatewayError::InvalidRequest(format!(
                "Resource '{}' does not exist",
                resource.id
            )));
        }

        self.resources.insert(resource.id.clone(), resource);
        Ok(())
    }

    /// Deletes a resource and all its grants.
    pub fn delete_resource(&self, resource_id: &str) -> Result<()> {
        self.resources
            .remove(resource_id)
            .ok_or_else(|| GatewayError::InvalidRequest(format!("Resource '{}' not found", resource_id)))?;

        self.grants.remove(resource_id);

        Ok(())
    }

    /// Lists resources by type.
    pub fn list_resources_by_type(&self, resource_type: &str) -> Vec<Resource> {
        self.resources
            .iter()
            .filter(|r| r.value().resource_type == resource_type)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Lists resources owned by a user.
    pub fn list_resources_by_owner(&self, owner_id: &str) -> Vec<Resource> {
        self.resources
            .iter()
            .filter(|r| r.value().owner_id == owner_id)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Grants permissions on a resource.
    pub fn grant_permission(
        &self,
        granter_id: &str,
        resource_id: &str,
        grantee: Grantee,
        permissions: Vec<ResourcePermission>,
    ) -> Result<()> {
        // Check granter has permission to share
        if !self.can_share(granter_id, resource_id) {
            return Err(GatewayError::AuthorizationFailed(
                "User does not have permission to share this resource".to_string(),
            ));
        }

        // Verify granter has the permissions they're granting
        let user_groups = self.get_user_groups(granter_id);
        for perm in &permissions {
            if !self.check_permission_internal(
                granter_id,
                &user_groups,
                resource_id,
                *perm,
            ) {
                return Err(GatewayError::AuthorizationFailed(format!(
                    "Cannot grant '{}' permission that granter does not have",
                    perm.as_str()
                )));
            }
        }

        let grant = ResourceGrant::new(resource_id, grantee, permissions, granter_id);

        self.grants
            .entry(resource_id.to_string())
            .or_insert_with(Vec::new)
            .push(grant);

        Ok(())
    }

    /// Revokes permissions from a grantee.
    pub fn revoke_permission(
        &self,
        revoker_id: &str,
        resource_id: &str,
        grantee: &Grantee,
    ) -> Result<()> {
        // Check revoker has admin/owner permission
        let user_groups = self.get_user_groups(revoker_id);
        if !self.check_permission_internal(revoker_id, &user_groups, resource_id, ResourcePermission::Admin) {
            return Err(GatewayError::AuthorizationFailed(
                "User does not have permission to revoke access".to_string(),
            ));
        }

        if let Some(mut grants) = self.grants.get_mut(resource_id) {
            grants.retain(|g| &g.grantee != grantee);
        }

        Ok(())
    }

    /// Checks if a user has a specific permission on a resource.
    pub fn has_permission(
        &self,
        user_id: &str,
        resource_id: &str,
        permission: ResourcePermission,
    ) -> bool {
        let user_groups = self.get_user_groups(user_id);
        self.check_permission_internal(user_id, &user_groups, resource_id, permission)
    }

    /// Internal permission check with groups.
    fn check_permission_internal(
        &self,
        user_id: &str,
        user_groups: &HashSet<String>,
        resource_id: &str,
        permission: ResourcePermission,
    ) -> bool {
        // Check direct grants on the resource
        if self.check_grants(user_id, user_groups, resource_id, permission) {
            return true;
        }

        // Check parent resource permissions (inheritance)
        if let Some(resource) = self.resources.get(resource_id) {
            if let Some(parent_id) = &resource.parent_id {
                // Parent permission implies child permission for read
                if permission == ResourcePermission::Read {
                    if self.check_permission_internal(user_id, user_groups, parent_id, permission) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Checks grants for a user on a resource.
    fn check_grants(
        &self,
        user_id: &str,
        user_groups: &HashSet<String>,
        resource_id: &str,
        permission: ResourcePermission,
    ) -> bool {
        if let Some(grants) = self.grants.get(resource_id) {
            for grant in grants.iter() {
                // Skip expired grants
                if grant.is_expired() {
                    continue;
                }

                // Check if this grant matches the user
                if grant.grantee.matches_user(user_id, user_groups) {
                    if grant.has_permission(permission) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Checks if a user can share a resource.
    fn can_share(&self, user_id: &str, resource_id: &str) -> bool {
        let user_groups = self.get_user_groups(user_id);

        // Owner can always share
        if self.check_permission_internal(user_id, &user_groups, resource_id, ResourcePermission::Owner) {
            return true;
        }

        // Admin can share
        if self.check_permission_internal(user_id, &user_groups, resource_id, ResourcePermission::Admin) {
            return true;
        }

        // Check for explicit share permission
        if let Some(grants) = self.grants.get(resource_id) {
            for grant in grants.iter() {
                if grant.is_expired() {
                    continue;
                }

                if grant.grantee.matches_user(user_id, &user_groups) {
                    // Check can_share flag or Share permission
                    if grant.can_share || grant.permissions.contains(&ResourcePermission::Share) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Gets all grants for a resource.
    pub fn get_grants(&self, resource_id: &str) -> Vec<ResourceGrant> {
        self.grants
            .get(resource_id)
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Gets all resources a user has access to.
    pub fn get_accessible_resources(
        &self,
        user_id: &str,
        permission: ResourcePermission,
    ) -> Vec<Resource> {
        let user_groups = self.get_user_groups(user_id);

        self.resources
            .iter()
            .filter(|r| {
                self.check_permission_internal(user_id, &user_groups, &r.id, permission)
            })
            .map(|r| r.value().clone())
            .collect()
    }

    /// Gets all resources a user has access to of a specific type.
    pub fn get_accessible_resources_by_type(
        &self,
        user_id: &str,
        resource_type: &str,
        permission: ResourcePermission,
    ) -> Vec<Resource> {
        let user_groups = self.get_user_groups(user_id);

        self.resources
            .iter()
            .filter(|r| {
                r.value().resource_type == resource_type
                    && self.check_permission_internal(user_id, &user_groups, &r.id, permission)
            })
            .map(|r| r.value().clone())
            .collect()
    }

    /// Adds a user to a group.
    pub fn add_user_to_group(&self, user_id: &str, group_id: &str) {
        self.user_groups
            .entry(user_id.to_string())
            .or_insert_with(HashSet::new)
            .insert(group_id.to_string());
    }

    /// Removes a user from a group.
    pub fn remove_user_from_group(&self, user_id: &str, group_id: &str) {
        if let Some(mut groups) = self.user_groups.get_mut(user_id) {
            groups.remove(group_id);
        }
    }

    /// Gets groups for a user.
    pub fn get_user_groups(&self, user_id: &str) -> HashSet<String> {
        self.user_groups
            .get(user_id)
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Registers a resource type hierarchy.
    pub fn register_type_hierarchy(&self, parent_type: &str, child_type: &str) {
        self.type_hierarchy
            .entry(parent_type.to_string())
            .or_insert_with(HashSet::new)
            .insert(child_type.to_string());
    }

    /// Transfers ownership of a resource.
    pub fn transfer_ownership(
        &self,
        current_owner_id: &str,
        resource_id: &str,
        new_owner_id: &str,
    ) -> Result<()> {
        // Verify current owner
        let mut resource = self
            .resources
            .get_mut(resource_id)
            .ok_or_else(|| GatewayError::InvalidRequest(format!("Resource '{}' not found", resource_id)))?;

        if resource.owner_id != current_owner_id {
            return Err(GatewayError::AuthorizationFailed(
                "Only the owner can transfer ownership".to_string(),
            ));
        }

        resource.owner_id = new_owner_id.to_string();

        // Update grants
        if let Some(mut grants) = self.grants.get_mut(resource_id) {
            // Remove old owner grant
            grants.retain(|g| {
                if let Grantee::User(uid) = &g.grantee {
                    !(uid == current_owner_id && g.permissions.contains(&ResourcePermission::Owner))
                } else {
                    true
                }
            });

            // Add new owner grant
            let owner_grant = ResourceGrant::new(
                resource_id,
                Grantee::User(new_owner_id.to_string()),
                vec![ResourcePermission::Owner],
                current_owner_id,
            );
            grants.push(owner_grant);
        }

        Ok(())
    }

    /// Makes a resource public (read access for anyone).
    pub fn make_public(&self, owner_id: &str, resource_id: &str) -> Result<()> {
        self.grant_permission(
            owner_id,
            resource_id,
            Grantee::Public,
            vec![ResourcePermission::Read],
        )
    }

    /// Makes a resource private (removes public access).
    pub fn make_private(&self, owner_id: &str, resource_id: &str) -> Result<()> {
        self.revoke_permission(owner_id, resource_id, &Grantee::Public)
    }

    /// Cleans up expired grants.
    pub fn cleanup_expired_grants(&self) -> usize {
        let mut count = 0;

        for mut entry in self.grants.iter_mut() {
            let initial_len = entry.value().len();
            entry.value_mut().retain(|g| !g.is_expired());
            count += initial_len - entry.value().len();
        }

        count
    }
}

impl Default for ResourcePermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Resource access request for audit logging.
#[derive(Debug, Clone)]
pub struct ResourceAccessRequest {
    /// User making the request
    pub user_id: String,
    /// Resource being accessed
    pub resource_id: String,
    /// Permission required
    pub permission: ResourcePermission,
    /// Request timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Client IP address
    pub client_ip: Option<String>,
    /// User agent
    pub user_agent: Option<String>,
    /// Additional context
    pub context: HashMap<String, String>,
}

impl ResourceAccessRequest {
    /// Creates a new access request.
    pub fn new(
        user_id: impl Into<String>,
        resource_id: impl Into<String>,
        permission: ResourcePermission,
    ) -> Self {
        Self {
            user_id: user_id.into(),
            resource_id: resource_id.into(),
            permission,
            timestamp: chrono::Utc::now(),
            client_ip: None,
            user_agent: None,
            context: HashMap::new(),
        }
    }

    /// Sets the client IP.
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// Sets the user agent.
    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }
}

/// Resource access result for audit logging.
#[derive(Debug, Clone)]
pub struct ResourceAccessResult {
    /// The request
    pub request: ResourceAccessRequest,
    /// Whether access was granted
    pub granted: bool,
    /// Denial reason (if denied)
    pub denial_reason: Option<String>,
    /// Grant that allowed access (if granted)
    pub matched_grant: Option<GrantSummary>,
}

/// Summary of a grant for audit purposes.
#[derive(Debug, Clone)]
pub struct GrantSummary {
    /// Grantee type
    pub grantee_type: String,
    /// Granted permissions
    pub permissions: Vec<String>,
    /// Whether the grant allows sharing
    pub can_share: bool,
}

/// Resource permission checker with audit support.
pub struct AuditedPermissionChecker {
    /// Permission manager
    manager: Arc<ResourcePermissionManager>,
    /// Access log buffer
    access_log: Arc<DashMap<String, Vec<ResourceAccessResult>>>,
    /// Maximum log entries per resource
    max_log_entries: usize,
}

impl AuditedPermissionChecker {
    /// Creates a new audited permission checker.
    pub fn new(manager: Arc<ResourcePermissionManager>) -> Self {
        Self {
            manager,
            access_log: Arc::new(DashMap::new()),
            max_log_entries: 1000,
        }
    }

    /// Checks permission with audit logging.
    pub fn check_permission(&self, request: ResourceAccessRequest) -> ResourceAccessResult {
        let granted = self.manager.has_permission(
            &request.user_id,
            &request.resource_id,
            request.permission,
        );

        let (denial_reason, matched_grant) = if granted {
            // Find the grant that matched
            let matched = self.find_matching_grant(&request);
            (None, matched)
        } else {
            (Some("No matching grant found".to_string()), None)
        };

        let result = ResourceAccessResult {
            request: request.clone(),
            granted,
            denial_reason,
            matched_grant,
        };

        // Log the access
        self.log_access(&request.resource_id, result.clone());

        result
    }

    /// Finds the grant that matched a request.
    fn find_matching_grant(&self, request: &ResourceAccessRequest) -> Option<GrantSummary> {
        let grants = self.manager.get_grants(&request.resource_id);
        let user_groups = self.manager.get_user_groups(&request.user_id);

        for grant in grants {
            if grant.is_expired() {
                continue;
            }

            if grant.grantee.matches_user(&request.user_id, &user_groups) {
                if grant.has_permission(request.permission) {
                    return Some(GrantSummary {
                        grantee_type: match &grant.grantee {
                            Grantee::User(_) => "user".to_string(),
                            Grantee::Group(_) => "group".to_string(),
                            Grantee::Authenticated => "authenticated".to_string(),
                            Grantee::Public => "public".to_string(),
                        },
                        permissions: grant.permissions.iter().map(|p| p.as_str().to_string()).collect(),
                        can_share: grant.can_share,
                    });
                }
            }
        }

        None
    }

    /// Logs an access result.
    fn log_access(&self, resource_id: &str, result: ResourceAccessResult) {
        let mut log = self
            .access_log
            .entry(resource_id.to_string())
            .or_insert_with(Vec::new);

        log.push(result);

        // Trim if over limit
        if log.len() > self.max_log_entries {
            let to_remove = log.len() - self.max_log_entries;
            log.drain(0..to_remove);
        }
    }

    /// Gets access log for a resource.
    pub fn get_access_log(&self, resource_id: &str) -> Vec<ResourceAccessResult> {
        self.access_log
            .get(resource_id)
            .map(|l| l.clone())
            .unwrap_or_default()
    }

    /// Gets denied access attempts for a resource.
    pub fn get_denied_attempts(&self, resource_id: &str) -> Vec<ResourceAccessResult> {
        self.access_log
            .get(resource_id)
            .map(|l| l.iter().filter(|r| !r.granted).cloned().collect())
            .unwrap_or_default()
    }

    /// Clears access log for a resource.
    pub fn clear_access_log(&self, resource_id: &str) {
        self.access_log.remove(resource_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_creation() {
        let resource = Resource::new("res1", "dataset", "user1")
            .with_metadata("key", "value");

        assert_eq!(resource.id, "res1");
        assert_eq!(resource.resource_type, "dataset");
        assert_eq!(resource.owner_id, "user1");
        assert_eq!(resource.metadata.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_resource_registration() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Duplicate should fail
        let resource2 = Resource::new("res1", "dataset", "user2");
        assert!(manager.register_resource(resource2).is_err());
    }

    #[test]
    fn test_owner_has_all_permissions() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        assert!(manager.has_permission("user1", "res1", ResourcePermission::Read));
        assert!(manager.has_permission("user1", "res1", ResourcePermission::Write));
        assert!(manager.has_permission("user1", "res1", ResourcePermission::Delete));
        assert!(manager.has_permission("user1", "res1", ResourcePermission::Admin));
    }

    #[test]
    fn test_non_owner_no_access() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        assert!(!manager.has_permission("user2", "res1", ResourcePermission::Read));
    }

    #[test]
    fn test_grant_permission() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Owner grants read to user2
        assert!(manager
            .grant_permission(
                "user1",
                "res1",
                Grantee::User("user2".to_string()),
                vec![ResourcePermission::Read],
            )
            .is_ok());

        assert!(manager.has_permission("user2", "res1", ResourcePermission::Read));
        assert!(!manager.has_permission("user2", "res1", ResourcePermission::Write));
    }

    #[test]
    fn test_cannot_grant_without_permission() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // user2 cannot grant
        assert!(manager
            .grant_permission(
                "user2",
                "res1",
                Grantee::User("user3".to_string()),
                vec![ResourcePermission::Read],
            )
            .is_err());
    }

    #[test]
    fn test_revoke_permission() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Grant then revoke
        let grantee = Grantee::User("user2".to_string());
        assert!(manager
            .grant_permission("user1", "res1", grantee.clone(), vec![ResourcePermission::Read])
            .is_ok());

        assert!(manager.has_permission("user2", "res1", ResourcePermission::Read));

        assert!(manager.revoke_permission("user1", "res1", &grantee).is_ok());

        assert!(!manager.has_permission("user2", "res1", ResourcePermission::Read));
    }

    #[test]
    fn test_group_permission() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Add user2 to editors group
        manager.add_user_to_group("user2", "editors");

        // Grant to group
        assert!(manager
            .grant_permission(
                "user1",
                "res1",
                Grantee::Group("editors".to_string()),
                vec![ResourcePermission::Read, ResourcePermission::Write],
            )
            .is_ok());

        assert!(manager.has_permission("user2", "res1", ResourcePermission::Read));
        assert!(manager.has_permission("user2", "res1", ResourcePermission::Write));

        // user3 not in group should not have access
        assert!(!manager.has_permission("user3", "res1", ResourcePermission::Read));
    }

    #[test]
    fn test_public_access() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Make public
        assert!(manager.make_public("user1", "res1").is_ok());

        // Anyone should have read access
        assert!(manager.has_permission("anyone", "res1", ResourcePermission::Read));
        // But not write
        assert!(!manager.has_permission("anyone", "res1", ResourcePermission::Write));
    }

    #[test]
    fn test_ownership_transfer() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Transfer to user2
        assert!(manager.transfer_ownership("user1", "res1", "user2").is_ok());

        // user2 is now owner
        assert!(manager.has_permission("user2", "res1", ResourcePermission::Owner));

        // user1 no longer owner
        assert!(!manager.has_permission("user1", "res1", ResourcePermission::Owner));
    }

    #[test]
    fn test_expired_grant() {
        let manager = ResourcePermissionManager::new();

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        // Create expired grant manually
        let expired_grant = ResourceGrant::new(
            "res1",
            Grantee::User("user2".to_string()),
            vec![ResourcePermission::Read],
            "user1",
        )
        .with_expiration(chrono::Utc::now() - chrono::Duration::hours(1));

        if let Some(mut grants) = manager.grants.get_mut("res1") {
            grants.push(expired_grant);
        }

        // Expired grant should not give access
        assert!(!manager.has_permission("user2", "res1", ResourcePermission::Read));
    }

    #[test]
    fn test_accessible_resources() {
        let manager = ResourcePermissionManager::new();

        let resource1 = Resource::new("res1", "dataset", "user1");
        let resource2 = Resource::new("res2", "dataset", "user2");

        assert!(manager.register_resource(resource1).is_ok());
        assert!(manager.register_resource(resource2).is_ok());

        // user1 can access res1 but not res2
        let accessible = manager.get_accessible_resources("user1", ResourcePermission::Read);
        assert_eq!(accessible.len(), 1);
        assert_eq!(accessible[0].id, "res1");
    }

    #[test]
    fn test_grantee_matches() {
        let user_groups: HashSet<String> = vec!["editors".to_string()].into_iter().collect();

        assert!(Grantee::User("user1".to_string()).matches_user("user1", &user_groups));
        assert!(!Grantee::User("user1".to_string()).matches_user("user2", &user_groups));

        assert!(Grantee::Group("editors".to_string()).matches_user("user1", &user_groups));
        assert!(!Grantee::Group("admins".to_string()).matches_user("user1", &user_groups));

        assert!(Grantee::Authenticated.matches_user("anyone", &user_groups));
        assert!(Grantee::Public.matches_user("anyone", &user_groups));
    }

    #[test]
    fn test_grant_implies_permission() {
        let grant = ResourceGrant::new(
            "res1",
            Grantee::User("user1".to_string()),
            vec![ResourcePermission::Write],
            "owner",
        );

        // Write implies read
        assert!(grant.has_permission(ResourcePermission::Read));
        assert!(grant.has_permission(ResourcePermission::Write));
        assert!(!grant.has_permission(ResourcePermission::Delete));
    }

    #[test]
    fn test_audited_permission_checker() {
        let manager = Arc::new(ResourcePermissionManager::new());

        let resource = Resource::new("res1", "dataset", "user1");
        assert!(manager.register_resource(resource).is_ok());

        let checker = AuditedPermissionChecker::new(Arc::clone(&manager));

        // Check permission with audit
        let request = ResourceAccessRequest::new("user1", "res1", ResourcePermission::Read)
            .with_client_ip("192.168.1.1");

        let result = checker.check_permission(request);
        assert!(result.granted);

        // Check access log
        let log = checker.get_access_log("res1");
        assert_eq!(log.len(), 1);
        assert!(log[0].granted);
    }

    #[test]
    fn test_resource_permission_from_str() {
        assert_eq!(ResourcePermission::from_str("read"), Some(ResourcePermission::Read));
        assert_eq!(ResourcePermission::from_str("WRITE"), Some(ResourcePermission::Write));
        assert_eq!(ResourcePermission::from_str("invalid"), None);
    }
}
