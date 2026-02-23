//! JWT token authentication implementation.

use super::{AuthContext, AuthMethod, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// JWT claims structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Issued at timestamp
    pub iat: i64,
    /// Expiration timestamp
    pub exp: i64,
    /// User email
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// User roles
    #[serde(default)]
    pub roles: Vec<String>,
    /// User permissions
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Custom claims
    #[serde(flatten)]
    pub custom: std::collections::HashMap<String, serde_json::Value>,
}

/// JWT authenticator.
pub struct JwtAuthenticator {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    expiration: i64,
}

impl JwtAuthenticator {
    /// Creates a new JWT authenticator.
    pub fn new(secret: &[u8], expiration: u64) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            expiration: expiration as i64,
        }
    }

    /// Creates a JWT token for the given identity.
    pub fn create_token(&self, identity: &Identity) -> Result<String> {
        let now = chrono::Utc::now().timestamp();

        let claims = Claims {
            sub: identity.user_id.clone(),
            iat: now,
            exp: now + self.expiration,
            email: identity.email.clone(),
            roles: identity.roles.iter().cloned().collect(),
            permissions: identity.permissions.iter().cloned().collect(),
            custom: identity
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        };

        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)?;

        Ok(token)
    }

    /// Verifies and decodes a JWT token.
    pub fn verify_token(&self, token: &str) -> Result<Claims> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;

        Ok(token_data.claims)
    }

    /// Refreshes a JWT token.
    pub fn refresh_token(&self, old_token: &str) -> Result<String> {
        let claims = self.verify_token(old_token)?;

        let now = chrono::Utc::now().timestamp();

        // Add a nonce based on nanoseconds to ensure token uniqueness even within the same second
        let nonce = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let mut custom = claims.custom;
        custom.insert(
            "nonce".to_string(),
            serde_json::Value::Number(serde_json::Number::from(nonce)),
        );

        let new_claims = Claims {
            sub: claims.sub,
            iat: now,
            exp: now + self.expiration,
            email: claims.email,
            roles: claims.roles,
            permissions: claims.permissions,
            custom,
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &new_claims,
            &self.encoding_key,
        )?;

        Ok(token)
    }
}

#[async_trait::async_trait]
impl Authenticator for JwtAuthenticator {
    async fn authenticate(&self, token: &str) -> Result<AuthContext> {
        let claims = self.verify_token(token)?;

        let mut identity = Identity::new(claims.sub.clone());
        identity.email = claims.email;
        identity.roles = claims.roles.into_iter().collect::<HashSet<_>>();
        identity.permissions = claims.permissions.into_iter().collect::<HashSet<_>>();
        identity.metadata = claims
            .custom
            .into_iter()
            .filter_map(|(k, v)| {
                if let serde_json::Value::String(s) = v {
                    Some((k, s))
                } else {
                    None
                }
            })
            .collect();

        Ok(AuthContext {
            identity,
            method: AuthMethod::Jwt,
            token: Some(token.to_string()),
            mfa_verified: false,
        })
    }

    async fn validate(&self, context: &AuthContext) -> Result<bool> {
        if context.method != AuthMethod::Jwt {
            return Ok(false);
        }

        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        match self.verify_token(token) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn refresh(&self, context: &AuthContext) -> Result<String> {
        let token = context
            .token
            .as_ref()
            .ok_or_else(|| GatewayError::InvalidToken("Missing token".to_string()))?;

        self.refresh_token(token)
    }

    async fn revoke(&self, _token: &str) -> Result<()> {
        // JWT tokens are stateless and cannot be revoked without a blacklist
        // This would require maintaining a revocation list
        Err(GatewayError::InvalidRequest(
            "JWT revocation requires a token blacklist".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_authenticator() -> JwtAuthenticator {
        JwtAuthenticator::new(b"test_secret_key_for_jwt_authentication", 3600)
    }

    #[test]
    fn test_create_token() {
        let auth = create_test_authenticator();
        let mut identity = Identity::new("user123".to_string());
        identity.email = Some("user@example.com".to_string());
        identity.roles.insert("admin".to_string());
        identity.permissions.insert("read".to_string());

        let token = auth.create_token(&identity);
        assert!(token.is_ok());
    }

    #[test]
    fn test_verify_token() {
        let auth = create_test_authenticator();
        let mut identity = Identity::new("user123".to_string());
        identity.email = Some("user@example.com".to_string());

        let token = auth.create_token(&identity).ok();
        assert!(token.is_some());

        let token = token.unwrap_or_default();
        let claims = auth.verify_token(&token);
        assert!(claims.is_ok());

        let claims = claims.ok();
        assert!(claims.is_some());
        let claims = claims.unwrap_or(Claims {
            sub: String::new(),
            iat: 0,
            exp: 0,
            email: None,
            roles: Vec::new(),
            permissions: Vec::new(),
            custom: std::collections::HashMap::new(),
        });
        assert_eq!(claims.sub, "user123");
        assert_eq!(claims.email, Some("user@example.com".to_string()));
    }

    #[tokio::test]
    async fn test_authenticate() {
        let auth = create_test_authenticator();
        let mut identity = Identity::new("user123".to_string());
        identity.roles.insert("admin".to_string());
        identity.permissions.insert("read".to_string());

        let token = auth.create_token(&identity).ok();
        assert!(token.is_some());

        let token = token.unwrap_or_default();
        let result = auth.authenticate(&token).await;
        assert!(result.is_ok());

        let context = result.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::Jwt,
        ));
        assert_eq!(context.identity.user_id, "user123");
        assert!(context.identity.has_role("admin"));
        assert!(context.identity.has_permission("read"));
    }

    #[tokio::test]
    async fn test_invalid_token() {
        let auth = create_test_authenticator();
        let result = auth.authenticate("invalid.token.here").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_token() {
        let auth = create_test_authenticator();
        let identity = Identity::new("user123".to_string());

        let token = auth.create_token(&identity).ok();
        assert!(token.is_some());

        let token = token.unwrap_or_default();
        let new_token = auth.refresh_token(&token);
        assert!(new_token.is_ok());

        let new_token = new_token.ok();
        assert!(new_token.is_some());
        let new_token = new_token.unwrap_or_default();
        assert_ne!(token, new_token);

        // New token should be valid
        let claims = auth.verify_token(&new_token);
        assert!(claims.is_ok());
    }

    #[tokio::test]
    async fn test_validate() {
        let auth = create_test_authenticator();
        let identity = Identity::new("user123".to_string());

        let token = auth.create_token(&identity).ok();
        assert!(token.is_some());

        let token = token.unwrap_or_default();
        let context = auth.authenticate(&token).await.ok();
        assert!(context.is_some());

        let context = context.unwrap_or(AuthContext::new(
            Identity::new("".to_string()),
            AuthMethod::Jwt,
        ));
        let is_valid = auth.validate(&context).await;
        assert!(is_valid.is_ok());
        assert!(is_valid.unwrap_or(false));
    }
}
