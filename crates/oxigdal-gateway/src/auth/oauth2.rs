//! OAuth2/OIDC authentication implementation.

use super::{AuthContext, AuthMethod, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::sync::Arc;

/// OAuth2 authenticator.
pub struct OAuth2Authenticator {
    client_id: String,
    client_secret: String,
    auth_url: String,
    _token_url: String,
    tokens: Arc<DashMap<String, OAuth2Token>>,
}

/// OAuth2 token information.
#[derive(Debug, Clone)]
pub struct OAuth2Token {
    /// Access token
    pub access_token: String,
    /// Token type (usually "Bearer")
    pub token_type: String,
    /// Expiration timestamp
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Refresh token
    pub refresh_token: Option<String>,
    /// User identity
    pub identity: Identity,
}

impl OAuth2Authenticator {
    /// Creates a new OAuth2 authenticator.
    pub fn new(
        client_id: &str,
        client_secret: &str,
        auth_url: &str,
        token_url: &str,
    ) -> Result<Self> {
        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url: auth_url.to_string(),
            _token_url: token_url.to_string(),
            tokens: Arc::new(DashMap::new()),
        })
    }

    /// Gets the authorization URL for OAuth2 flow.
    pub fn get_authorization_url(&self, redirect_uri: &str, state: &str) -> String {
        format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&state={}",
            self.auth_url,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(state)
        )
    }

    /// Exchanges authorization code for access token.
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<OAuth2Token> {
        // In a real implementation, this would make an HTTP request to the token endpoint
        // For now, we'll return a mock token for testing
        let _params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        // Mock implementation
        let access_token = format!("oauth2_token_{}", uuid::Uuid::new_v4());
        let identity = Identity::new("oauth2_user".to_string());

        let token = OAuth2Token {
            access_token: access_token.clone(),
            token_type: "Bearer".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            refresh_token: Some(format!("refresh_{}", uuid::Uuid::new_v4())),
            identity,
        };

        self.tokens.insert(access_token, token.clone());

        Ok(token)
    }

    /// Refreshes an OAuth2 token using the refresh token.
    pub async fn refresh_token_with_refresh(&self, refresh_token: &str) -> Result<OAuth2Token> {
        // Find the old token by refresh token
        let old_token = self
            .tokens
            .iter()
            .find(|entry| {
                entry
                    .value()
                    .refresh_token
                    .as_ref()
                    .map(|rt| rt == refresh_token)
                    .unwrap_or(false)
            })
            .ok_or_else(|| GatewayError::InvalidToken("Invalid refresh token".to_string()))?;

        let identity = old_token.value().identity.clone();

        // Remove old token
        let old_access_token = old_token.value().access_token.clone();
        drop(old_token);
        self.tokens.remove(&old_access_token);

        // Create new token
        let access_token = format!("oauth2_token_{}", uuid::Uuid::new_v4());
        let new_token = OAuth2Token {
            access_token: access_token.clone(),
            token_type: "Bearer".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            refresh_token: Some(format!("refresh_{}", uuid::Uuid::new_v4())),
            identity,
        };

        self.tokens.insert(access_token, new_token.clone());

        Ok(new_token)
    }

    /// Revokes an OAuth2 token.
    pub fn revoke_token(&self, access_token: &str) -> Result<()> {
        self.tokens
            .remove(access_token)
            .ok_or_else(|| GatewayError::InvalidToken("Token not found".to_string()))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl Authenticator for OAuth2Authenticator {
    async fn authenticate(&self, token: &str) -> Result<AuthContext> {
        let oauth_token = self
            .tokens
            .get(token)
            .ok_or_else(|| GatewayError::InvalidToken("Invalid OAuth2 token".to_string()))?;

        // Check expiration
        if chrono::Utc::now() > oauth_token.expires_at {
            return Err(GatewayError::TokenExpired);
        }

        Ok(AuthContext {
            identity: oauth_token.identity.clone(),
            method: AuthMethod::OAuth2,
            token: Some(token.to_string()),
            mfa_verified: false,
        })
    }

    async fn validate(&self, context: &AuthContext) -> Result<bool> {
        if context.method != AuthMethod::OAuth2 {
            return Ok(false);
        }

        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        let oauth_token = match self.tokens.get(token) {
            Some(t) => t,
            None => return Ok(false),
        };

        // Check expiration
        if chrono::Utc::now() > oauth_token.expires_at {
            return Ok(false);
        }

        Ok(true)
    }

    async fn refresh(&self, context: &AuthContext) -> Result<String> {
        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        let oauth_token = self
            .tokens
            .get(token)
            .ok_or_else(|| GatewayError::InvalidToken("Invalid token".to_string()))?;

        let refresh_token = oauth_token
            .refresh_token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("No refresh token available".to_string()))?
            .clone();

        drop(oauth_token);

        let new_token = self.refresh_token_with_refresh(&refresh_token).await?;

        Ok(new_token.access_token)
    }

    async fn revoke(&self, token: &str) -> Result<()> {
        self.revoke_token(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_authenticator() -> OAuth2Authenticator {
        OAuth2Authenticator::new(
            "test_client_id",
            "test_client_secret",
            "https://auth.example.com/oauth/authorize",
            "https://auth.example.com/oauth/token",
        )
        .ok()
        .unwrap_or_else(|| OAuth2Authenticator {
            client_id: String::new(),
            client_secret: String::new(),
            auth_url: String::new(),
            _token_url: String::new(),
            tokens: Arc::new(DashMap::new()),
        })
    }

    #[test]
    fn test_authorization_url() {
        let auth = create_test_authenticator();
        let url = auth.get_authorization_url("https://example.com/callback", "random_state");

        assert!(url.contains("client_id=test_client_id"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("state=random_state"));
    }

    #[tokio::test]
    async fn test_exchange_code() {
        let auth = create_test_authenticator();
        let token = auth
            .exchange_code("test_code", "https://example.com/callback")
            .await;

        assert!(token.is_ok());
        let token = token.ok();
        assert!(token.is_some());
        let token = token.unwrap_or(OAuth2Token {
            access_token: String::new(),
            token_type: String::new(),
            expires_at: chrono::Utc::now(),
            refresh_token: None,
            identity: Identity::new(String::new()),
        });
        assert!(!token.access_token.is_empty());
        assert!(token.refresh_token.is_some());
    }

    #[tokio::test]
    async fn test_authenticate() {
        let auth = create_test_authenticator();
        let token = auth
            .exchange_code("test_code", "https://example.com/callback")
            .await
            .ok();

        assert!(token.is_some());
        let token = token.unwrap_or(OAuth2Token {
            access_token: String::new(),
            token_type: String::new(),
            expires_at: chrono::Utc::now(),
            refresh_token: None,
            identity: Identity::new(String::new()),
        });

        let context = auth.authenticate(&token.access_token).await;
        assert!(context.is_ok());

        let context = context.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::OAuth2,
        ));
        assert_eq!(context.method, AuthMethod::OAuth2);
    }

    #[tokio::test]
    async fn test_refresh_token() {
        let auth = create_test_authenticator();
        let token = auth
            .exchange_code("test_code", "https://example.com/callback")
            .await
            .ok();

        assert!(token.is_some());
        let token = token.unwrap_or(OAuth2Token {
            access_token: String::new(),
            token_type: String::new(),
            expires_at: chrono::Utc::now(),
            refresh_token: None,
            identity: Identity::new(String::new()),
        });

        let context = auth.authenticate(&token.access_token).await.ok();
        assert!(context.is_some());

        let context = context.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::OAuth2,
        ));

        let new_token = auth.refresh(&context).await;
        assert!(new_token.is_ok());

        let new_token = new_token.ok();
        assert!(new_token.is_some());
        let new_token = new_token.unwrap_or_default();
        assert_ne!(token.access_token, new_token);
    }

    #[tokio::test]
    async fn test_revoke_token() {
        let auth = create_test_authenticator();
        let token = auth
            .exchange_code("test_code", "https://example.com/callback")
            .await
            .ok();

        assert!(token.is_some());
        let token = token.unwrap_or(OAuth2Token {
            access_token: String::new(),
            token_type: String::new(),
            expires_at: chrono::Utc::now(),
            refresh_token: None,
            identity: Identity::new(String::new()),
        });

        // Token should work before revocation
        assert!(auth.authenticate(&token.access_token).await.is_ok());

        // Revoke the token
        assert!(auth.revoke(&token.access_token).await.is_ok());

        // Token should not work after revocation
        assert!(auth.authenticate(&token.access_token).await.is_err());
    }
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        super::urlencoding_encode(s)
    }
}
