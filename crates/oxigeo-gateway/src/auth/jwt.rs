//! JWT token authentication implementation.

use super::{AuthContext, AuthMethod, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// JWT claims structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Issued at timestamp
    pub iat: i64,
    /// Expiration timestamp
    pub exp: i64,
    /// Not-before timestamp (token is invalid before this instant, RFC 7519 §4.1.5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    /// Audience (RFC 7519 §4.1.3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    /// JWT ID, used as the handle for revocation (RFC 7519 §4.1.7)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
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
///
/// Supports symmetric (HS256) and asymmetric (RS256/ES256) signing, audience and not-before
/// validation, and real token revocation via an in-memory blacklist keyed on the token's
/// `jti` claim.
pub struct JwtAuthenticator {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    algorithm: Algorithm,
    expiration: i64,
    /// Expected audience; when set, tokens must carry a matching `aud` claim.
    audience: Option<String>,
    /// Revoked token ids mapped to their expiry (unix seconds), so entries can be pruned
    /// once the underlying token would have expired anyway.
    revoked: Arc<DashMap<String, i64>>,
}

impl JwtAuthenticator {
    /// Creates a new HS256 (symmetric) JWT authenticator.
    pub fn new(secret: &[u8], expiration: u64) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            algorithm: Algorithm::HS256,
            expiration: expiration as i64,
            audience: None,
            revoked: Arc::new(DashMap::new()),
        }
    }

    /// Creates an RS256 (asymmetric) JWT authenticator from PEM-encoded RSA keys.
    ///
    /// Enables key rotation and multi-issuer/JWKS deployments where the signing key must not
    /// be shared with verifiers.
    pub fn new_rs256(
        private_key_pem: &[u8],
        public_key_pem: &[u8],
        expiration: u64,
    ) -> Result<Self> {
        let encoding_key = EncodingKey::from_rsa_pem(private_key_pem)
            .map_err(|e| GatewayError::ConfigError(format!("invalid RSA private key: {e}")))?;
        let decoding_key = DecodingKey::from_rsa_pem(public_key_pem)
            .map_err(|e| GatewayError::ConfigError(format!("invalid RSA public key: {e}")))?;
        Ok(Self {
            encoding_key,
            decoding_key,
            algorithm: Algorithm::RS256,
            expiration: expiration as i64,
            audience: None,
            revoked: Arc::new(DashMap::new()),
        })
    }

    /// Creates an ES256 (asymmetric) JWT authenticator from PEM-encoded EC keys.
    pub fn new_es256(
        private_key_pem: &[u8],
        public_key_pem: &[u8],
        expiration: u64,
    ) -> Result<Self> {
        let encoding_key = EncodingKey::from_ec_pem(private_key_pem)
            .map_err(|e| GatewayError::ConfigError(format!("invalid EC private key: {e}")))?;
        let decoding_key = DecodingKey::from_ec_pem(public_key_pem)
            .map_err(|e| GatewayError::ConfigError(format!("invalid EC public key: {e}")))?;
        Ok(Self {
            encoding_key,
            decoding_key,
            algorithm: Algorithm::ES256,
            expiration: expiration as i64,
            audience: None,
            revoked: Arc::new(DashMap::new()),
        })
    }

    /// Sets the expected audience. When set, [`Self::verify_token`] rejects tokens whose
    /// `aud` claim does not match, and [`Self::create_token`] stamps this audience.
    #[must_use]
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// Creates a JWT token for the given identity.
    pub fn create_token(&self, identity: &Identity) -> Result<String> {
        let now = chrono::Utc::now().timestamp();

        let claims = Claims {
            sub: identity.user_id.clone(),
            iat: now,
            exp: now + self.expiration,
            nbf: Some(now),
            aud: self.audience.clone(),
            jti: Some(uuid::Uuid::new_v4().to_string()),
            email: identity.email.clone(),
            roles: identity.roles.iter().cloned().collect(),
            permissions: identity.permissions.iter().cloned().collect(),
            custom: identity
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        };

        let token = encode(&Header::new(self.algorithm), &claims, &self.encoding_key)?;

        Ok(token)
    }

    /// Verifies and decodes a JWT token, enforcing expiry, not-before, audience (if
    /// configured) and revocation.
    pub fn verify_token(&self, token: &str) -> Result<Claims> {
        let mut validation = Validation::new(self.algorithm);
        validation.validate_exp = true;
        validation.validate_nbf = true;

        match &self.audience {
            Some(aud) => validation.set_audience(&[aud]),
            // jsonwebtoken validates `aud` by default; disable when we have no expectation so
            // tokens without an audience are still accepted.
            None => validation.validate_aud = false,
        }

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;

        // Reject revoked tokens (checked by jti). Prune expired revocation entries lazily.
        if let Some(jti) = token_data.claims.jti.as_ref() {
            self.prune_revoked();
            if self.revoked.contains_key(jti) {
                return Err(GatewayError::InvalidToken(
                    "token has been revoked".to_string(),
                ));
            }
        }

        Ok(token_data.claims)
    }

    /// Adds a token's `jti` to the revocation blacklist. Returns an error if the token has no
    /// `jti` claim (older tokens minted before revocation support) since such a token cannot
    /// be individually revoked.
    pub fn revoke_token(&self, token: &str) -> Result<()> {
        // Decode without rejecting on expiry so an about-to-expire token can still be revoked,
        // but keep signature verification so we never blacklist attacker-supplied garbage.
        let mut validation = Validation::new(self.algorithm);
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;
        let jti = token_data.claims.jti.ok_or_else(|| {
            GatewayError::InvalidToken(
                "token has no 'jti' claim and cannot be revoked individually".to_string(),
            )
        })?;

        self.revoked.insert(jti, token_data.claims.exp);
        Ok(())
    }

    /// Removes revocation entries whose token would already have expired.
    fn prune_revoked(&self) {
        let now = chrono::Utc::now().timestamp();
        self.revoked.retain(|_, exp| *exp > now);
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
            nbf: Some(now),
            aud: self.audience.clone().or(claims.aud),
            jti: Some(uuid::Uuid::new_v4().to_string()),
            email: claims.email,
            roles: claims.roles,
            permissions: claims.permissions,
            custom,
        };

        let token = encode(
            &Header::new(self.algorithm),
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

    async fn revoke(&self, token: &str) -> Result<()> {
        self.revoke_token(token)
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
            nbf: None,
            aud: None,
            jti: None,
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
    async fn test_revocation_blocks_token() {
        let auth = create_test_authenticator();
        let identity = Identity::new("user123".to_string());
        let token = auth.create_token(&identity).expect("create");

        // Valid before revocation.
        assert!(auth.verify_token(&token).is_ok());

        // Revoke succeeds (real blacklist), and the token no longer verifies.
        assert!(auth.revoke(&token).await.is_ok());
        assert!(
            auth.verify_token(&token).is_err(),
            "a revoked token must no longer verify"
        );
        assert!(auth.authenticate(&token).await.is_err());
    }

    #[test]
    fn test_audience_enforced() {
        let auth =
            JwtAuthenticator::new(b"secret_key_material_here", 3600).with_audience("api.oxigeo");
        let identity = Identity::new("user123".to_string());
        let token = auth.create_token(&identity).expect("create");

        // Same audience verifies.
        assert!(auth.verify_token(&token).is_ok());

        // A verifier expecting a different audience rejects it.
        let other =
            JwtAuthenticator::new(b"secret_key_material_here", 3600).with_audience("other.service");
        assert!(other.verify_token(&token).is_err());
    }

    #[test]
    fn test_claims_carry_nbf_and_jti() {
        let auth = create_test_authenticator();
        let identity = Identity::new("user123".to_string());
        let token = auth.create_token(&identity).expect("create");
        let claims = auth.verify_token(&token).expect("verify");
        assert!(claims.nbf.is_some(), "nbf must be stamped");
        assert!(claims.jti.is_some(), "jti must be stamped for revocation");
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
