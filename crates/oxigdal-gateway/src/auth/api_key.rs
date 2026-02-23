//! API key authentication implementation.

use super::{AuthContext, AuthMethod, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::sync::Arc;

/// API key authenticator.
pub struct ApiKeyAuthenticator {
    keys: Arc<DashMap<String, ApiKeyInfo>>,
}

/// API key information.
#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    /// User ID associated with the key
    pub user_id: String,
    /// Key name/description
    pub name: String,
    /// Key scopes/permissions
    pub scopes: Vec<String>,
    /// Key creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Key expiration timestamp
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Key is active
    pub active: bool,
}

impl ApiKeyAuthenticator {
    /// Creates a new API key authenticator.
    pub fn new() -> Self {
        Self {
            keys: Arc::new(DashMap::new()),
        }
    }

    /// Generates a new API key.
    pub fn generate_key(
        &self,
        user_id: String,
        name: String,
        scopes: Vec<String>,
    ) -> Result<String> {
        use blake3::Hasher;

        let random_bytes = generate_random_bytes(32)?;
        let mut hasher = Hasher::new();
        hasher.update(&random_bytes);
        hasher.update(user_id.as_bytes());
        hasher.update(name.as_bytes());

        let key = format!(
            "oxigdal_{}",
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                hasher.finalize().as_bytes()
            )
        );

        let info = ApiKeyInfo {
            user_id,
            name,
            scopes,
            created_at: chrono::Utc::now(),
            expires_at: None,
            active: true,
        };

        self.keys.insert(key.clone(), info);

        Ok(key)
    }

    /// Revokes an API key.
    pub fn revoke_key(&self, key: &str) -> Result<()> {
        self.keys
            .get_mut(key)
            .map(|mut info| info.active = false)
            .ok_or(GatewayError::InvalidApiKey)?;

        Ok(())
    }

    /// Lists all API keys for a user.
    pub fn list_keys(&self, user_id: &str) -> Vec<ApiKeyInfo> {
        self.keys
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Deletes an API key.
    pub fn delete_key(&self, key: &str) -> Result<()> {
        self.keys.remove(key).ok_or(GatewayError::InvalidApiKey)?;

        Ok(())
    }
}

impl Default for ApiKeyAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Authenticator for ApiKeyAuthenticator {
    async fn authenticate(&self, token: &str) -> Result<AuthContext> {
        let info = self.keys.get(token).ok_or(GatewayError::InvalidApiKey)?;

        if !info.active {
            return Err(GatewayError::InvalidApiKey);
        }

        // Check expiration
        if let Some(expires_at) = info.expires_at {
            if chrono::Utc::now() > expires_at {
                return Err(GatewayError::TokenExpired);
            }
        }

        let mut identity = Identity::new(info.user_id.clone());
        for scope in &info.scopes {
            identity.permissions.insert(scope.clone());
        }

        Ok(AuthContext {
            identity,
            method: AuthMethod::ApiKey,
            token: Some(token.to_string()),
            mfa_verified: false,
        })
    }

    async fn validate(&self, context: &AuthContext) -> Result<bool> {
        if context.method != AuthMethod::ApiKey {
            return Ok(false);
        }

        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        let info = match self.keys.get(token) {
            Some(info) => info,
            None => return Ok(false),
        };

        if !info.active {
            return Ok(false);
        }

        // Check expiration
        if let Some(expires_at) = info.expires_at {
            if chrono::Utc::now() > expires_at {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn refresh(&self, _context: &AuthContext) -> Result<String> {
        // API keys don't support refresh
        Err(GatewayError::InvalidRequest(
            "API keys cannot be refreshed".to_string(),
        ))
    }

    async fn revoke(&self, token: &str) -> Result<()> {
        self.revoke_key(token)
    }
}

fn generate_random_bytes(len: usize) -> Result<Vec<u8>> {
    use getrandom::getrandom;

    let mut bytes = vec![0u8; len];
    getrandom(&mut bytes).map_err(|e| {
        GatewayError::InternalError(format!("Failed to generate random bytes: {}", e))
    })?;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_key() {
        let auth = ApiKeyAuthenticator::new();
        let key = auth
            .generate_key(
                "user123".to_string(),
                "test-key".to_string(),
                vec!["read".to_string(), "write".to_string()],
            )
            .ok();

        assert!(key.is_some());
        let key = key.unwrap_or_default();
        assert!(key.starts_with("oxigdal_"));
    }

    #[tokio::test]
    async fn test_authenticate_valid_key() {
        let auth = ApiKeyAuthenticator::new();
        let key = auth
            .generate_key(
                "user123".to_string(),
                "test-key".to_string(),
                vec!["read".to_string()],
            )
            .ok();

        assert!(key.is_some());
        let key = key.unwrap_or_default();

        let result = auth.authenticate(&key).await;
        assert!(result.is_ok());

        let context = result.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::ApiKey,
        ));
        assert_eq!(context.identity.user_id, "user123");
        assert!(context.identity.has_permission("read"));
    }

    #[tokio::test]
    async fn test_authenticate_invalid_key() {
        let auth = ApiKeyAuthenticator::new();
        let result = auth.authenticate("invalid_key").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_revoke_key() {
        let auth = ApiKeyAuthenticator::new();
        let key = auth
            .generate_key(
                "user123".to_string(),
                "test-key".to_string(),
                vec!["read".to_string()],
            )
            .ok();

        assert!(key.is_some());
        let key = key.unwrap_or_default();

        // Key should work before revocation
        assert!(auth.authenticate(&key).await.is_ok());

        // Revoke the key
        assert!(auth.revoke_key(&key).is_ok());

        // Key should not work after revocation
        assert!(auth.authenticate(&key).await.is_err());
    }

    #[test]
    fn test_list_keys() {
        let auth = ApiKeyAuthenticator::new();
        let _key1 = auth.generate_key(
            "user123".to_string(),
            "key1".to_string(),
            vec!["read".to_string()],
        );
        let _key2 = auth.generate_key(
            "user123".to_string(),
            "key2".to_string(),
            vec!["write".to_string()],
        );

        let keys = auth.list_keys("user123");
        assert_eq!(keys.len(), 2);
    }
}
