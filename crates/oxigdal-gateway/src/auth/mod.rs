//! Authentication and authorization module.
//!
//! Provides comprehensive authentication mechanisms including API keys, JWT tokens,
//! OAuth2/OIDC integration, session management, and multi-factor authentication.

pub mod api_key;
pub mod jwt;
pub mod mfa;
pub mod oauth2;
pub mod session;

use crate::error::{GatewayError, Result};
use std::collections::HashSet;
use std::sync::Arc;

/// Authentication configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Enable API key authentication
    pub enable_api_key: bool,
    /// Enable JWT authentication
    pub enable_jwt: bool,
    /// Enable OAuth2 authentication
    pub enable_oauth2: bool,
    /// Enable session-based authentication
    pub enable_session: bool,
    /// Require MFA for sensitive operations
    pub require_mfa: bool,
    /// JWT secret key
    pub jwt_secret: Option<String>,
    /// JWT token expiration in seconds
    pub jwt_expiration: u64,
    /// Session timeout in seconds
    pub session_timeout: u64,
    /// OAuth2 client ID
    pub oauth2_client_id: Option<String>,
    /// OAuth2 client secret
    pub oauth2_client_secret: Option<String>,
    /// OAuth2 authorization endpoint
    pub oauth2_auth_url: Option<String>,
    /// OAuth2 token endpoint
    pub oauth2_token_url: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enable_api_key: true,
            enable_jwt: true,
            enable_oauth2: false,
            enable_session: true,
            require_mfa: false,
            jwt_secret: None,
            jwt_expiration: 3600,  // 1 hour
            session_timeout: 1800, // 30 minutes
            oauth2_client_id: None,
            oauth2_client_secret: None,
            oauth2_auth_url: None,
            oauth2_token_url: None,
        }
    }
}

/// User identity information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// User ID
    pub user_id: String,
    /// User email
    pub email: Option<String>,
    /// User roles
    pub roles: HashSet<String>,
    /// User permissions
    pub permissions: HashSet<String>,
    /// User metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl Identity {
    /// Creates a new identity.
    pub fn new(user_id: String) -> Self {
        Self {
            user_id,
            email: None,
            roles: HashSet::new(),
            permissions: HashSet::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Checks if the identity has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }

    /// Checks if the identity has a specific permission.
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    /// Checks if the identity has any of the given roles.
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|r| self.has_role(r))
    }

    /// Checks if the identity has all of the given permissions.
    pub fn has_all_permissions(&self, permissions: &[&str]) -> bool {
        permissions.iter().all(|p| self.has_permission(p))
    }
}

/// Authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// API key authentication
    ApiKey,
    /// JWT token authentication
    Jwt,
    /// OAuth2 authentication
    OAuth2,
    /// Session-based authentication
    Session,
}

/// Authentication context.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User identity
    pub identity: Identity,
    /// Authentication method used
    pub method: AuthMethod,
    /// Token or session ID
    pub token: Option<String>,
    /// MFA verified
    pub mfa_verified: bool,
}

impl AuthContext {
    /// Creates a new authentication context.
    pub fn new(identity: Identity, method: AuthMethod) -> Self {
        Self {
            identity,
            method,
            token: None,
            mfa_verified: false,
        }
    }

    /// Checks if the user is authorized for a specific action.
    pub fn is_authorized(&self, required_permission: &str) -> bool {
        self.identity.has_permission(required_permission)
    }

    /// Checks if the user has any of the required roles.
    pub fn has_required_role(&self, required_roles: &[&str]) -> bool {
        self.identity.has_any_role(required_roles)
    }
}

/// Authenticator trait for different authentication methods.
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    /// Authenticates a request and returns an authentication context.
    async fn authenticate(&self, token: &str) -> Result<AuthContext>;

    /// Validates if the authentication context is still valid.
    async fn validate(&self, context: &AuthContext) -> Result<bool>;

    /// Refreshes the authentication token.
    async fn refresh(&self, context: &AuthContext) -> Result<String>;

    /// Revokes the authentication token.
    async fn revoke(&self, token: &str) -> Result<()>;
}

/// Multi-authenticator that supports multiple authentication methods.
pub struct MultiAuthenticator {
    api_key: Option<Arc<api_key::ApiKeyAuthenticator>>,
    jwt: Option<Arc<jwt::JwtAuthenticator>>,
    oauth2: Option<Arc<oauth2::OAuth2Authenticator>>,
    session: Option<Arc<session::SessionAuthenticator>>,
}

impl MultiAuthenticator {
    /// Creates a new multi-authenticator from configuration.
    pub fn from_config(config: &AuthConfig) -> Result<Self> {
        let api_key = if config.enable_api_key {
            Some(Arc::new(api_key::ApiKeyAuthenticator::new()))
        } else {
            None
        };

        let jwt = if config.enable_jwt {
            let secret = config.jwt_secret.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("JWT secret not configured".to_string())
            })?;
            Some(Arc::new(jwt::JwtAuthenticator::new(
                secret.as_bytes(),
                config.jwt_expiration,
            )))
        } else {
            None
        };

        let oauth2 = if config.enable_oauth2 {
            let client_id = config.oauth2_client_id.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("OAuth2 client ID not configured".to_string())
            })?;
            let client_secret = config.oauth2_client_secret.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("OAuth2 client secret not configured".to_string())
            })?;
            let auth_url = config.oauth2_auth_url.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("OAuth2 auth URL not configured".to_string())
            })?;
            let token_url = config.oauth2_token_url.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("OAuth2 token URL not configured".to_string())
            })?;

            Some(Arc::new(oauth2::OAuth2Authenticator::new(
                client_id,
                client_secret,
                auth_url,
                token_url,
            )?))
        } else {
            None
        };

        let session = if config.enable_session {
            Some(Arc::new(session::SessionAuthenticator::new(
                config.session_timeout,
            )))
        } else {
            None
        };

        Ok(Self {
            api_key,
            jwt,
            oauth2,
            session,
        })
    }

    /// Attempts to authenticate using any available method.
    pub async fn authenticate(&self, auth_header: &str) -> Result<AuthContext> {
        // Try different authentication methods based on the header format
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            // Try JWT first
            if let Some(jwt) = &self.jwt {
                if let Ok(context) = jwt.authenticate(token).await {
                    return Ok(context);
                }
            }

            // Try OAuth2
            if let Some(oauth2) = &self.oauth2 {
                if let Ok(context) = oauth2.authenticate(token).await {
                    return Ok(context);
                }
            }
        } else if let Some(key) = auth_header.strip_prefix("ApiKey ") {
            // Try API key
            if let Some(api_key) = &self.api_key {
                if let Ok(context) = api_key.authenticate(key).await {
                    return Ok(context);
                }
            }
        } else if let Some(session_id) = auth_header.strip_prefix("Session ") {
            // Try session
            if let Some(session) = &self.session {
                if let Ok(context) = session.authenticate(session_id).await {
                    return Ok(context);
                }
            }
        }

        Err(GatewayError::AuthenticationFailed(
            "Invalid authentication credentials".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_creation() {
        let identity = Identity::new("user123".to_string());
        assert_eq!(identity.user_id, "user123");
        assert!(identity.roles.is_empty());
        assert!(identity.permissions.is_empty());
    }

    #[test]
    fn test_identity_roles() {
        let mut identity = Identity::new("user123".to_string());
        identity.roles.insert("admin".to_string());
        identity.roles.insert("editor".to_string());

        assert!(identity.has_role("admin"));
        assert!(identity.has_role("editor"));
        assert!(!identity.has_role("viewer"));
        assert!(identity.has_any_role(&["admin", "viewer"]));
    }

    #[test]
    fn test_identity_permissions() {
        let mut identity = Identity::new("user123".to_string());
        identity.permissions.insert("read".to_string());
        identity.permissions.insert("write".to_string());

        assert!(identity.has_permission("read"));
        assert!(identity.has_permission("write"));
        assert!(!identity.has_permission("delete"));
        assert!(identity.has_all_permissions(&["read", "write"]));
        assert!(!identity.has_all_permissions(&["read", "delete"]));
    }

    #[test]
    fn test_auth_context() {
        let identity = Identity::new("user123".to_string());
        let context = AuthContext::new(identity, AuthMethod::Jwt);

        assert_eq!(context.method, AuthMethod::Jwt);
        assert!(!context.mfa_verified);
        assert!(context.token.is_none());
    }

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert!(config.enable_api_key);
        assert!(config.enable_jwt);
        assert!(!config.enable_oauth2);
        assert!(config.enable_session);
        assert!(!config.require_mfa);
    }
}
