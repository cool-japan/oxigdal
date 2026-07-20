//! Session-based authentication implementation.

use super::{AuthContext, AuthMethod, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::sync::Arc;

/// Session authenticator.
pub struct SessionAuthenticator {
    sessions: Arc<DashMap<String, SessionInfo>>,
    timeout: i64,
}

/// Session information.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session ID
    pub session_id: String,
    /// User identity
    pub identity: Identity,
    /// Session creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last access timestamp
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    /// Session metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl SessionAuthenticator {
    /// Creates a new session authenticator.
    pub fn new(timeout: u64) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            timeout: timeout as i64,
        }
    }

    /// Creates a new session for the given identity.
    pub fn create_session(&self, identity: Identity) -> Result<String> {
        let session_id = format!("session_{}", uuid::Uuid::new_v4());

        let now = chrono::Utc::now();
        let session = SessionInfo {
            session_id: session_id.clone(),
            identity,
            created_at: now,
            last_accessed: now,
            metadata: std::collections::HashMap::new(),
        };

        self.sessions.insert(session_id.clone(), session);

        Ok(session_id)
    }

    /// Gets session information.
    pub fn get_session(&self, session_id: &str) -> Result<SessionInfo> {
        let mut session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| GatewayError::InvalidToken("Session not found".to_string()))?;

        // Check if session has expired
        let now = chrono::Utc::now();
        let elapsed = (now - session.last_accessed).num_seconds();

        if elapsed > self.timeout {
            drop(session);
            self.sessions.remove(session_id);
            return Err(GatewayError::TokenExpired);
        }

        // Update last accessed time
        session.last_accessed = now;

        Ok(session.clone())
    }

    /// Updates session metadata.
    pub fn update_session_metadata(
        &self,
        session_id: &str,
        key: String,
        value: String,
    ) -> Result<()> {
        let mut session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| GatewayError::InvalidToken("Session not found".to_string()))?;

        session.metadata.insert(key, value);

        Ok(())
    }

    /// Destroys a session.
    pub fn destroy_session(&self, session_id: &str) -> Result<()> {
        self.sessions
            .remove(session_id)
            .ok_or_else(|| GatewayError::InvalidToken("Session not found".to_string()))?;

        Ok(())
    }

    /// Lists all active sessions for a user.
    pub fn list_user_sessions(&self, user_id: &str) -> Vec<SessionInfo> {
        let now = chrono::Utc::now();

        self.sessions
            .iter()
            .filter(|entry| {
                let session = entry.value();
                let elapsed = (now - session.last_accessed).num_seconds();
                session.identity.user_id == user_id && elapsed <= self.timeout
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Cleans up expired sessions.
    pub fn cleanup_expired_sessions(&self) -> usize {
        let now = chrono::Utc::now();
        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter(|entry| {
                let session = entry.value();
                let elapsed = (now - session.last_accessed).num_seconds();
                elapsed > self.timeout
            })
            .map(|entry| entry.key().clone())
            .collect();

        let count = expired.len();
        for session_id in expired {
            self.sessions.remove(&session_id);
        }

        count
    }
}

#[async_trait::async_trait]
impl Authenticator for SessionAuthenticator {
    async fn authenticate(&self, token: &str) -> Result<AuthContext> {
        let session = self.get_session(token)?;

        Ok(AuthContext {
            identity: session.identity.clone(),
            method: AuthMethod::Session,
            token: Some(token.to_string()),
            mfa_verified: false,
        })
    }

    async fn validate(&self, context: &AuthContext) -> Result<bool> {
        if context.method != AuthMethod::Session {
            return Ok(false);
        }

        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        match self.get_session(token) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn refresh(&self, context: &AuthContext) -> Result<String> {
        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        // Get the session and create a new one
        let session = self.get_session(token)?;
        let new_session_id = self.create_session(session.identity.clone())?;

        // Copy metadata to new session
        for (key, value) in &session.metadata {
            let _ = self.update_session_metadata(&new_session_id, key.clone(), value.clone());
        }

        // Destroy old session
        let _ = self.destroy_session(token);

        Ok(new_session_id)
    }

    async fn revoke(&self, token: &str) -> Result<()> {
        self.destroy_session(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_session() {
        let auth = SessionAuthenticator::new(1800);
        let identity = Identity::new("user123".to_string());

        let session_id = auth.create_session(identity);
        assert!(session_id.is_ok());

        let session_id = session_id.ok();
        assert!(session_id.is_some());
        let session_id = session_id.unwrap_or_default();
        assert!(session_id.starts_with("session_"));
    }

    #[test]
    fn test_get_session() {
        let auth = SessionAuthenticator::new(1800);
        let identity = Identity::new("user123".to_string());

        let session_id = auth.create_session(identity).ok();
        assert!(session_id.is_some());

        let session_id = session_id.unwrap_or_default();
        let session = auth.get_session(&session_id);
        assert!(session.is_ok());

        let session = session.ok();
        assert!(session.is_some());
        let session = session.unwrap_or(SessionInfo {
            session_id: String::new(),
            identity: Identity::new(String::new()),
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        });
        assert_eq!(session.identity.user_id, "user123");
    }

    #[tokio::test]
    async fn test_authenticate() {
        let auth = SessionAuthenticator::new(1800);
        let identity = Identity::new("user123".to_string());

        let session_id = auth.create_session(identity).ok();
        assert!(session_id.is_some());

        let session_id = session_id.unwrap_or_default();
        let context = auth.authenticate(&session_id).await;
        assert!(context.is_ok());

        let context = context.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::Session,
        ));
        assert_eq!(context.identity.user_id, "user123");
        assert_eq!(context.method, AuthMethod::Session);
    }

    #[test]
    fn test_session_metadata() {
        let auth = SessionAuthenticator::new(1800);
        let identity = Identity::new("user123".to_string());

        let session_id = auth.create_session(identity).ok();
        assert!(session_id.is_some());

        let session_id = session_id.unwrap_or_default();

        // Add metadata
        assert!(
            auth.update_session_metadata(&session_id, "ip".to_string(), "192.168.1.1".to_string())
                .is_ok()
        );

        // Verify metadata
        let session = auth.get_session(&session_id).ok();
        assert!(session.is_some());
        let session = session.unwrap_or(SessionInfo {
            session_id: String::new(),
            identity: Identity::new(String::new()),
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        });
        assert_eq!(session.metadata.get("ip"), Some(&"192.168.1.1".to_string()));
    }

    #[tokio::test]
    async fn test_destroy_session() {
        let auth = SessionAuthenticator::new(1800);
        let identity = Identity::new("user123".to_string());

        let session_id = auth.create_session(identity).ok();
        assert!(session_id.is_some());

        let session_id = session_id.unwrap_or_default();

        // Session should exist
        assert!(auth.authenticate(&session_id).await.is_ok());

        // Destroy session
        assert!(auth.destroy_session(&session_id).is_ok());

        // Session should not exist
        assert!(auth.authenticate(&session_id).await.is_err());
    }

    #[test]
    fn test_list_user_sessions() {
        let auth = SessionAuthenticator::new(1800);

        let identity1 = Identity::new("user123".to_string());
        let identity2 = Identity::new("user123".to_string());

        let _session1 = auth.create_session(identity1);
        let _session2 = auth.create_session(identity2);

        let sessions = auth.list_user_sessions("user123");
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_refresh_session() {
        let auth = SessionAuthenticator::new(1800);
        let identity = Identity::new("user123".to_string());

        let session_id = auth.create_session(identity).ok();
        assert!(session_id.is_some());

        let session_id = session_id.unwrap_or_default();
        let context = auth.authenticate(&session_id).await.ok();
        assert!(context.is_some());

        let context = context.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::Session,
        ));

        let new_session_id = auth.refresh(&context).await;
        assert!(new_session_id.is_ok());

        let new_session_id = new_session_id.ok();
        assert!(new_session_id.is_some());
        let new_session_id = new_session_id.unwrap_or_default();
        assert_ne!(session_id, new_session_id);

        // Old session should be invalid
        assert!(auth.authenticate(&session_id).await.is_err());

        // New session should be valid
        assert!(auth.authenticate(&new_session_id).await.is_ok());
    }
}
