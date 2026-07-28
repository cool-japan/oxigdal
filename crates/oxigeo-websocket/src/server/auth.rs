//! Connection authentication for the low-level WebSocket server.
//!
//! Auth is enforced during the HTTP upgrade handshake (see
//! [`crate::server::Server`]). In open mode (the default) every handshake is
//! accepted and granted an anonymous administrator principal; in authenticated
//! mode a valid bearer token — supplied via the `Authorization: Bearer <token>`
//! header or a `?token=<token>` query parameter — is required, and unknown or
//! missing tokens cause the handshake to be rejected with `401 Unauthorized`.

use crate::error::{Error, Result};
use std::collections::HashMap;

/// Access level granted to an authenticated principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// May receive data only.
    ReadOnly,
    /// May create and remove its own subscriptions.
    Subscriber,
    /// Full access, including administrative operations.
    Admin,
}

impl Role {
    /// Stable lowercase name used when storing the role in connection metadata.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::ReadOnly => "readonly",
            Role::Subscriber => "subscriber",
            Role::Admin => "admin",
        }
    }

    /// Whether this role satisfies (is at least) the `required` role.
    #[must_use]
    pub fn allows(self, required: Role) -> bool {
        self >= required
    }
}

/// An authenticated principal: an identity plus its granted role.
#[derive(Debug, Clone)]
pub struct AuthPrincipal {
    /// Stable identifier for the principal.
    pub subject: String,
    /// The access level granted to this principal.
    pub role: Role,
}

impl AuthPrincipal {
    /// Create a new principal.
    #[must_use]
    pub fn new(subject: impl Into<String>, role: Role) -> Self {
        Self {
            subject: subject.into(),
            role,
        }
    }

    /// The implicit principal used in open (no-auth) mode.
    #[must_use]
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".to_string(),
            role: Role::Admin,
        }
    }

    /// Ensure this principal holds at least `required` role.
    pub fn authorize(&self, required: Role) -> Result<()> {
        if self.role.allows(required) {
            Ok(())
        } else {
            Err(Error::Authorization(format!(
                "principal '{}' with role {} is not permitted this operation",
                self.subject,
                self.role.as_str()
            )))
        }
    }
}

/// Authentication configuration for the WebSocket server.
#[derive(Debug, Clone, Default)]
pub struct AuthConfig {
    /// When `true`, handshakes must present a valid token.
    pub require_auth: bool,
    /// Map of accepted bearer tokens to their principals.
    tokens: HashMap<String, AuthPrincipal>,
}

impl AuthConfig {
    /// Create an open (no-auth) configuration.
    #[must_use]
    pub fn open() -> Self {
        Self::default()
    }

    /// Register an accepted token mapped to a principal, enabling auth.
    #[must_use]
    pub fn with_token(
        mut self,
        token: impl Into<String>,
        subject: impl Into<String>,
        role: Role,
    ) -> Self {
        self.add_token(token, subject, role);
        self
    }

    /// Register an accepted token (mutable form).
    pub fn add_token(&mut self, token: impl Into<String>, subject: impl Into<String>, role: Role) {
        self.require_auth = true;
        self.tokens
            .insert(token.into(), AuthPrincipal::new(subject, role));
    }

    /// Number of configured tokens.
    #[must_use]
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    /// Authenticate a handshake given an optional token.
    pub fn authenticate(&self, token: Option<&str>) -> Result<AuthPrincipal> {
        if !self.require_auth {
            return Ok(AuthPrincipal::anonymous());
        }

        let token = token.ok_or_else(|| {
            Error::Authentication(
                "missing credentials: provide an 'Authorization: Bearer <token>' header \
                 or a '?token=<token>' query parameter"
                    .to_string(),
            )
        })?;

        self.tokens
            .get(token)
            .cloned()
            .ok_or_else(|| Error::Authentication("invalid or unknown token".to_string()))
    }
}

/// Extract a bearer token from an `Authorization` header value.
#[must_use]
pub fn token_from_authorization(header: &str) -> Option<&str> {
    let header = header.trim();
    let (scheme, value) = header.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") {
        let value = value.trim();
        if value.is_empty() { None } else { Some(value) }
    } else {
        None
    }
}

/// Extract a `token` parameter from a raw URL query string.
#[must_use]
pub fn token_from_query(query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=')
            && key == "token"
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_mode_grants_admin() {
        let auth = AuthConfig::open();
        assert!(!auth.require_auth);
        let p = auth.authenticate(None).expect("open mode allows all");
        assert_eq!(p.role, Role::Admin);
    }

    #[test]
    fn test_token_validation() {
        let auth = AuthConfig::open().with_token("k", "svc", Role::Subscriber);
        assert!(auth.require_auth);
        assert!(matches!(
            auth.authenticate(None),
            Err(Error::Authentication(_))
        ));
        assert!(matches!(
            auth.authenticate(Some("nope")),
            Err(Error::Authentication(_))
        ));
        let p = auth.authenticate(Some("k")).expect("valid token");
        assert_eq!(p.subject, "svc");
        assert_eq!(p.role, Role::Subscriber);
    }

    #[test]
    fn test_authorize() {
        let p = AuthPrincipal::new("x", Role::ReadOnly);
        assert!(p.authorize(Role::ReadOnly).is_ok());
        assert!(matches!(
            p.authorize(Role::Admin),
            Err(Error::Authorization(_))
        ));
    }

    #[test]
    fn test_token_extractors() {
        assert_eq!(token_from_authorization("Bearer tok"), Some("tok"));
        assert_eq!(token_from_authorization("Basic tok"), None);
        assert_eq!(token_from_query("a=1&token=zzz"), Some("zzz".to_string()));
        assert_eq!(token_from_query("a=1"), None);
    }
}
