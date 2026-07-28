//! Connection authentication and authorization for the WebSocket server.
//!
//! The server can operate in two modes:
//!
//! - **Open mode** (`AuthConfig::require_auth == false`, the default): every
//!   connection is accepted and granted the [`Role::Admin`] principal. This
//!   preserves the historical behaviour for trusted/local deployments.
//! - **Authenticated mode** (`require_auth == true`): a connection must present a
//!   valid bearer token (via the `Authorization: Bearer <token>` header or a
//!   `?token=<token>` query parameter) that maps to a configured
//!   [`AuthPrincipal`]. Unknown or missing tokens are rejected before the
//!   WebSocket upgrade completes.
//!
//! Once connected, individual operations are gated by the principal's
//! [`Role`] via [`AuthPrincipal::authorize`], which yields
//! [`Error::Authorization`] when the role is insufficient.

use crate::error::{Error, Result};
use std::collections::HashMap;

/// Access level granted to an authenticated principal.
///
/// Ordered from least to most privileged; a higher role satisfies any
/// requirement for a lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// May receive data but not create subscriptions or mutate server state.
    ReadOnly,
    /// May create and remove its own subscriptions.
    Subscriber,
    /// Full access, including administrative operations.
    Admin,
}

impl Role {
    /// Parse a role from its lowercase string name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "readonly" | "read-only" | "read_only" => Some(Self::ReadOnly),
            "subscriber" | "user" => Some(Self::Subscriber),
            "admin" | "administrator" => Some(Self::Admin),
            _ => None,
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
    /// Stable identifier for the principal (e.g. an account or API-key id).
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

    /// The implicit principal used in open (no-auth) mode: a locally trusted
    /// administrator.
    #[must_use]
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".to_string(),
            role: Role::Admin,
        }
    }

    /// Ensure this principal holds at least `required` role.
    ///
    /// Returns [`Error::Authorization`] when the role is insufficient.
    pub fn authorize(&self, required: Role) -> Result<()> {
        if self.role.allows(required) {
            Ok(())
        } else {
            Err(Error::Authorization(format!(
                "principal '{}' with role {:?} is not permitted this operation (requires {:?})",
                self.subject, self.role, required
            )))
        }
    }
}

/// Authentication configuration for the WebSocket server.
#[derive(Debug, Clone, Default)]
pub struct AuthConfig {
    /// When `true`, connections must present a valid token.
    pub require_auth: bool,
    /// Map of accepted bearer tokens to their principals.
    tokens: HashMap<String, AuthPrincipal>,
}

impl AuthConfig {
    /// Create an open (no-auth) configuration.
    #[must_use]
    pub fn open() -> Self {
        Self {
            require_auth: false,
            tokens: HashMap::new(),
        }
    }

    /// Register an accepted token mapped to a principal, enabling auth.
    #[must_use]
    pub fn with_token(
        mut self,
        token: impl Into<String>,
        subject: impl Into<String>,
        role: Role,
    ) -> Self {
        self.require_auth = true;
        self.tokens
            .insert(token.into(), AuthPrincipal::new(subject, role));
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

    /// Authenticate a connection request.
    ///
    /// In open mode this always succeeds with [`AuthPrincipal::anonymous`]. In
    /// authenticated mode it validates the supplied token, returning
    /// [`Error::Authentication`] when the token is missing or unknown.
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
///
/// Accepts `Bearer <token>` (case-insensitive scheme).
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

/// Extract a `token` parameter from a raw URL query string (e.g.
/// `foo=1&token=abc`). Returns the first `token` value found.
#[must_use]
pub fn token_from_query(query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=')
            && key == "token"
            && !value.is_empty()
        {
            return Some(decode_query_component(value));
        }
    }
    None
}

/// Minimal percent-decoding for query components (enough for tokens).
fn decode_query_component(input: &str) -> String {
    let bytes = input.replace('+', " ");
    let bytes = bytes.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_ordering() {
        assert!(Role::Admin.allows(Role::ReadOnly));
        assert!(Role::Admin.allows(Role::Subscriber));
        assert!(Role::Subscriber.allows(Role::ReadOnly));
        assert!(!Role::ReadOnly.allows(Role::Subscriber));
        assert!(!Role::Subscriber.allows(Role::Admin));
    }

    #[test]
    fn test_open_mode_grants_admin() {
        let auth = AuthConfig::open();
        assert!(!auth.require_auth);
        let principal = auth.authenticate(None).expect("open mode allows all");
        assert_eq!(principal.role, Role::Admin);
    }

    #[test]
    fn test_authenticated_mode_requires_valid_token() {
        let auth = AuthConfig::open().with_token("secret", "alice", Role::Subscriber);
        assert!(auth.require_auth);

        // Missing token -> authentication error.
        assert!(matches!(
            auth.authenticate(None),
            Err(Error::Authentication(_))
        ));
        // Unknown token -> authentication error.
        assert!(matches!(
            auth.authenticate(Some("wrong")),
            Err(Error::Authentication(_))
        ));
        // Valid token -> principal with the configured role.
        let principal = auth.authenticate(Some("secret")).expect("valid token");
        assert_eq!(principal.subject, "alice");
        assert_eq!(principal.role, Role::Subscriber);
    }

    #[test]
    fn test_authorize_produces_authorization_error() {
        let principal = AuthPrincipal::new("bob", Role::ReadOnly);
        assert!(principal.authorize(Role::ReadOnly).is_ok());
        assert!(matches!(
            principal.authorize(Role::Admin),
            Err(Error::Authorization(_))
        ));
    }

    #[test]
    fn test_token_from_authorization() {
        assert_eq!(token_from_authorization("Bearer abc123"), Some("abc123"));
        assert_eq!(token_from_authorization("bearer  xyz "), Some("xyz"));
        assert_eq!(token_from_authorization("Basic abc"), None);
        assert_eq!(token_from_authorization("Bearer "), None);
    }

    #[test]
    fn test_token_from_query() {
        assert_eq!(
            token_from_query("a=1&token=deadbeef&b=2"),
            Some("deadbeef".to_string())
        );
        assert_eq!(
            token_from_query("token=abc%20def"),
            Some("abc def".to_string())
        );
        assert_eq!(token_from_query("foo=bar"), None);
    }
}
