//! Security and access control for cluster operations.
//!
//! Provides authentication, authorization (RBAC), encryption, audit logging, and secret management.

use crate::error::{ClusterError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// User identifier.
pub type UserId = String;

/// Role identifier.
pub type RoleId = String;

/// Permission string.
pub type Permission = String;

/// Authentication token.
pub type Token = String;

/// Security manager for cluster authentication and authorization.
pub struct SecurityManager {
    /// User database
    users: Arc<DashMap<UserId, User>>,
    /// Role definitions
    roles: Arc<DashMap<RoleId, Role>>,
    /// Active sessions
    sessions: Arc<DashMap<Token, Session>>,
    /// Audit log
    audit_log: Arc<RwLock<Vec<AuditEntry>>>,
    /// Secret store
    secrets: Arc<RwLock<HashMap<String, Secret>>>,
    /// Statistics
    stats: Arc<RwLock<SecurityStats>>,
}

/// User account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique user identifier
    pub id: UserId,
    /// Username for login
    pub username: String,
    /// Email address (optional)
    pub email: Option<String>,
    /// Assigned roles
    pub roles: Vec<RoleId>,
    /// When the account was created
    pub created_at: SystemTime,
    /// Last successful login time
    pub last_login: Option<SystemTime>,
    /// Whether the account is enabled
    pub enabled: bool,
}

/// Role definition with permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique role identifier
    pub id: RoleId,
    /// Human-readable role name
    pub name: String,
    /// Role description
    pub description: Option<String>,
    /// Set of permissions granted by this role
    pub permissions: HashSet<Permission>,
}

/// Active session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Authentication token
    pub token: Token,
    /// User who owns this session
    pub user_id: UserId,
    /// When the session was created
    pub created_at: SystemTime,
    /// When the session expires
    pub expires_at: SystemTime,
    /// IP address of the client (if available)
    pub ip_address: Option<String>,
}

/// Audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When the action occurred
    pub timestamp: SystemTime,
    /// User who performed the action (if applicable)
    pub user_id: Option<UserId>,
    /// Action that was performed
    pub action: String,
    /// Resource that was affected
    pub resource: String,
    /// Result of the action
    pub result: AuditResult,
    /// Additional details about the action
    pub details: Option<String>,
}

/// Audit result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditResult {
    /// Action succeeded
    Success,
    /// Action failed due to error
    Failure,
    /// Action denied due to permissions
    Denied,
}

/// Secret stored securely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    /// Secret key/name
    pub key: String,
    /// Secret value (in production, this would be encrypted)
    pub value: String,
    /// When the secret was created
    pub created_at: SystemTime,
    /// When the secret expires (if applicable)
    pub expires_at: Option<SystemTime>,
}

/// Security statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecurityStats {
    /// Total number of registered users
    pub total_users: usize,
    /// Number of currently active sessions
    pub active_sessions: usize,
    /// Total successful logins
    pub total_logins: u64,
    /// Total failed login attempts
    pub failed_logins: u64,
    /// Total audit log entries
    pub total_audit_entries: u64,
    /// Total authorization denial events
    pub authorization_denials: u64,
}

impl SecurityManager {
    /// Create a new security manager.
    pub fn new() -> Self {
        let manager = Self {
            users: Arc::new(DashMap::new()),
            roles: Arc::new(DashMap::new()),
            sessions: Arc::new(DashMap::new()),
            audit_log: Arc::new(RwLock::new(Vec::new())),
            secrets: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(SecurityStats::default())),
        };

        // Create default roles
        manager.create_default_roles();

        manager
    }

    fn create_default_roles(&self) {
        // Admin role
        let admin_permissions: HashSet<Permission> = vec![
            "cluster:*".to_string(),
            "task:*".to_string(),
            "worker:*".to_string(),
            "user:*".to_string(),
        ]
        .into_iter()
        .collect();

        self.roles.insert(
            "admin".to_string(),
            Role {
                id: "admin".to_string(),
                name: "Administrator".to_string(),
                description: Some("Full cluster access".to_string()),
                permissions: admin_permissions,
            },
        );

        // Operator role
        let operator_permissions: HashSet<Permission> = vec![
            "cluster:read".to_string(),
            "task:*".to_string(),
            "worker:read".to_string(),
        ]
        .into_iter()
        .collect();

        self.roles.insert(
            "operator".to_string(),
            Role {
                id: "operator".to_string(),
                name: "Operator".to_string(),
                description: Some("Task and cluster operations".to_string()),
                permissions: operator_permissions,
            },
        );

        // User role
        let user_permissions: HashSet<Permission> = vec![
            "cluster:read".to_string(),
            "task:create".to_string(),
            "task:read".to_string(),
            "task:cancel".to_string(),
        ]
        .into_iter()
        .collect();

        self.roles.insert(
            "user".to_string(),
            Role {
                id: "user".to_string(),
                name: "User".to_string(),
                description: Some("Basic user access".to_string()),
                permissions: user_permissions,
            },
        );
    }

    /// Create a new user.
    pub fn create_user(
        &self,
        username: String,
        email: Option<String>,
        roles: Vec<RoleId>,
    ) -> Result<UserId> {
        let user_id = uuid::Uuid::new_v4().to_string();

        let user = User {
            id: user_id.clone(),
            username,
            email,
            roles,
            created_at: SystemTime::now(),
            last_login: None,
            enabled: true,
        };

        self.users.insert(user_id.clone(), user);

        {
            let mut stats = self.stats.write();
            stats.total_users = self.users.len();
        } // Lock is dropped here

        self.audit(
            "system".to_string(),
            "user:create".to_string(),
            user_id.clone(),
            AuditResult::Success,
            None,
        );

        Ok(user_id)
    }

    /// Authenticate user and create session.
    pub fn authenticate(&self, user_id: &UserId, _credentials: &str) -> Result<Token> {
        // In production, verify credentials (password hash, etc.)

        let mut user = self
            .users
            .get_mut(user_id)
            .ok_or_else(|| ClusterError::AuthenticationFailed("User not found".to_string()))?;

        if !user.enabled {
            let mut stats = self.stats.write();
            stats.failed_logins += 1;
            return Err(ClusterError::AuthenticationFailed(
                "User disabled".to_string(),
            ));
        }

        user.last_login = Some(SystemTime::now());

        let token = uuid::Uuid::new_v4().to_string();
        let session = Session {
            token: token.clone(),
            user_id: user_id.clone(),
            created_at: SystemTime::now(),
            expires_at: SystemTime::now() + Duration::from_secs(3600), // 1 hour
            ip_address: None,
        };

        self.sessions.insert(token.clone(), session);

        {
            let mut stats = self.stats.write();
            stats.total_logins += 1;
            stats.active_sessions = self.sessions.len();
        } // Lock is dropped here

        self.audit(
            user_id.clone(),
            "auth:login".to_string(),
            user_id.clone(),
            AuditResult::Success,
            None,
        );

        Ok(token)
    }

    /// Logout and invalidate session.
    pub fn logout(&self, token: &Token) -> Result<()> {
        if let Some((_, session)) = self.sessions.remove(token) {
            self.audit(
                session.user_id.clone(),
                "auth:logout".to_string(),
                session.user_id,
                AuditResult::Success,
                None,
            );

            let mut stats = self.stats.write();
            stats.active_sessions = self.sessions.len();
        }

        Ok(())
    }

    /// Validate a session token.
    pub fn validate_session(&self, token: &Token) -> Result<UserId> {
        let session = self
            .sessions
            .get(token)
            .ok_or_else(|| ClusterError::AuthenticationFailed("Invalid session".to_string()))?;

        if SystemTime::now() > session.expires_at {
            self.sessions.remove(token);
            return Err(ClusterError::AuthenticationFailed(
                "Session expired".to_string(),
            ));
        }

        Ok(session.user_id.clone())
    }

    /// Check if user has permission.
    pub fn check_permission(&self, user_id: &UserId, permission: &Permission) -> Result<bool> {
        let user = self
            .users
            .get(user_id)
            .ok_or_else(|| ClusterError::AuthenticationFailed("User not found".to_string()))?;

        // Check each role's permissions
        for role_id in &user.roles {
            if let Some(role) = self.roles.get(role_id) {
                // Check wildcard permissions
                // Extract the prefix before the colon (e.g., "task" from "task:create")
                let prefix = permission.split(':').next().unwrap_or("");
                if role.permissions.contains(&format!("{}:*", prefix)) {
                    return Ok(true);
                }

                // Check exact permission
                if role.permissions.contains(permission) {
                    return Ok(true);
                }

                // Check global wildcard
                if role.permissions.contains("*") {
                    return Ok(true);
                }
            }
        }

        {
            let mut stats = self.stats.write();
            stats.authorization_denials += 1;
        } // Lock is dropped here

        self.audit(
            user_id.clone(),
            permission.clone(),
            user_id.clone(),
            AuditResult::Denied,
            Some(format!("Missing permission: {}", permission)),
        );

        Ok(false)
    }

    /// Require permission (throws error if not authorized).
    pub fn require_permission(&self, user_id: &UserId, permission: &Permission) -> Result<()> {
        if self.check_permission(user_id, permission)? {
            Ok(())
        } else {
            Err(ClusterError::PermissionDenied(permission.clone()))
        }
    }

    /// Create or update a role.
    pub fn create_role(
        &self,
        id: RoleId,
        name: String,
        permissions: HashSet<Permission>,
    ) -> Result<()> {
        let role = Role {
            id: id.clone(),
            name,
            description: None,
            permissions,
        };

        self.roles.insert(id, role);

        Ok(())
    }

    /// Assign role to user.
    pub fn assign_role(&self, user_id: &UserId, role_id: &RoleId) -> Result<()> {
        let mut user = self
            .users
            .get_mut(user_id)
            .ok_or_else(|| ClusterError::AuthenticationFailed("User not found".to_string()))?;

        if !user.roles.contains(role_id) {
            user.roles.push(role_id.clone());
        }

        self.audit(
            user_id.clone(),
            "user:assign_role".to_string(),
            user_id.clone(),
            AuditResult::Success,
            Some(format!("Role: {}", role_id)),
        );

        Ok(())
    }

    /// Store a secret.
    pub fn store_secret(
        &self,
        key: String,
        value: String,
        expires_at: Option<SystemTime>,
    ) -> Result<()> {
        let secret = Secret {
            key: key.clone(),
            value, // In production, encrypt this
            created_at: SystemTime::now(),
            expires_at,
        };

        self.secrets.write().insert(key, secret);

        Ok(())
    }

    /// Retrieve a secret.
    pub fn get_secret(&self, key: &str) -> Result<String> {
        let secrets = self.secrets.read();
        let secret = secrets
            .get(key)
            .ok_or_else(|| ClusterError::SecretNotFound(key.to_string()))?;

        // Check expiration
        if let Some(expires) = secret.expires_at {
            if SystemTime::now() > expires {
                return Err(ClusterError::SecretNotFound("Secret expired".to_string()));
            }
        }

        Ok(secret.value.clone())
    }

    /// Log an audit entry.
    pub fn audit(
        &self,
        user_id: UserId,
        action: String,
        resource: String,
        result: AuditResult,
        details: Option<String>,
    ) {
        let entry = AuditEntry {
            timestamp: SystemTime::now(),
            user_id: Some(user_id),
            action,
            resource,
            result,
            details,
        };

        self.audit_log.write().push(entry);

        let mut stats = self.stats.write();
        stats.total_audit_entries += 1;
    }

    /// Get audit log.
    pub fn get_audit_log(&self, limit: usize) -> Vec<AuditEntry> {
        let log = self.audit_log.read();
        log.iter().rev().take(limit).cloned().collect()
    }

    /// Get security statistics.
    pub fn get_stats(&self) -> SecurityStats {
        self.stats.read().clone()
    }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_user_creation() {
        let manager = SecurityManager::new();
        let result = manager.create_user(
            "testuser".to_string(),
            Some("test@example.com".to_string()),
            vec!["user".to_string()],
        );

        assert!(result.is_ok());
        let stats = manager.get_stats();
        assert_eq!(stats.total_users, 1);
    }

    #[test]
    fn test_authentication() {
        let manager = SecurityManager::new();
        let user_id = manager
            .create_user("testuser".to_string(), None, vec!["user".to_string()])
            .expect("user creation should succeed");

        let token = manager
            .authenticate(&user_id, "password")
            .expect("authentication should succeed");
        assert!(!token.is_empty());

        let validated_user = manager.validate_session(&token);
        assert!(validated_user.is_ok());
        assert_eq!(
            validated_user.expect("session validation should succeed"),
            user_id
        );
    }

    #[test]
    fn test_authorization() {
        let manager = SecurityManager::new();
        let user_id = manager
            .create_user("testuser".to_string(), None, vec!["user".to_string()])
            .expect("user creation should succeed");

        // User should have task:create permission
        let has_perm = manager
            .check_permission(&user_id, &"task:create".to_string())
            .expect("permission check should succeed");
        assert!(has_perm);

        // User should not have worker:delete permission
        let has_perm = manager
            .check_permission(&user_id, &"worker:delete".to_string())
            .expect("permission check should succeed");
        assert!(!has_perm);
    }

    #[test]
    fn test_secret_management() {
        let manager = SecurityManager::new();

        manager
            .store_secret("api_key".to_string(), "secret123".to_string(), None)
            .ok();

        let secret = manager.get_secret("api_key");
        assert!(secret.is_ok());
        assert_eq!(
            secret.expect("secret retrieval should succeed"),
            "secret123"
        );
    }
}
