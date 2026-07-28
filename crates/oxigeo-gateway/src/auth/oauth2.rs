//! OAuth2/OIDC authentication implementation.
//!
//! Performs a real Authorization Code Grant exchange against the identity provider's
//! token endpoint (RFC 6749 section 4.1.3) using the `oauth2` crate over rustls, and
//! derives the authenticated [`Identity`] from the provider's userinfo endpoint rather
//! than trusting anything supplied by the caller.

use super::{AuthContext, AuthMethod, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, TokenResponse, TokenUrl,
};
use std::borrow::Cow;
use std::sync::Arc;

/// A `BasicClient` with the authorization and token endpoints configured, but no
/// device-authorization, introspection, or revocation endpoints (we don't use them).
type ConfiguredOAuth2Client =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// OAuth2 authenticator.
pub struct OAuth2Authenticator {
    client_id: String,
    auth_url: String,
    /// Userinfo endpoint used to derive a verified [`Identity`] from an access token.
    ///
    /// Required to establish identity: without it, `exchange_code` succeeds at obtaining a
    /// token from the IdP but refuses to fabricate an identity and returns an error instead.
    userinfo_url: Option<String>,
    oauth_client: ConfiguredOAuth2Client,
    http_client: oauth2::reqwest::Client,
    tokens: Arc<DashMap<String, OAuth2Token>>,
    /// PKCE (RFC 7636) code verifiers awaiting their authorization-code exchange, keyed by
    /// the `state` parameter. Populated by [`OAuth2Authenticator::get_authorization_url`] and
    /// consumed by [`OAuth2Authenticator::exchange_code`].
    pkce_verifiers: Arc<DashMap<String, String>>,
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
        let auth_uri = AuthUrl::new(auth_url.to_string()).map_err(|error| {
            GatewayError::ConfigError(format!("invalid OAuth2 auth_url: {error}"))
        })?;
        let token_uri = TokenUrl::new(token_url.to_string()).map_err(|error| {
            GatewayError::ConfigError(format!("invalid OAuth2 token_url: {error}"))
        })?;

        let oauth_client = BasicClient::new(ClientId::new(client_id.to_string()))
            .set_client_secret(ClientSecret::new(client_secret.to_string()))
            .set_auth_uri(auth_uri)
            .set_token_uri(token_uri);

        // Following redirects on the token/userinfo HTTP calls would open an SSRF vector, so
        // redirects are disabled per the oauth2 crate's own security guidance.
        let http_client = oauth2::reqwest::ClientBuilder::new()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                GatewayError::ConfigError(format!("failed to build OAuth2 HTTP client: {error}"))
            })?;

        Ok(Self {
            client_id: client_id.to_string(),
            auth_url: auth_url.to_string(),
            userinfo_url: None,
            oauth_client,
            http_client,
            tokens: Arc::new(DashMap::new()),
            pkce_verifiers: Arc::new(DashMap::new()),
        })
    }

    /// Configures the userinfo endpoint used to derive a verified [`Identity`] after a
    /// successful token exchange. Without this, `exchange_code`/`refresh_token_with_refresh`
    /// will succeed in obtaining a token from the provider but fail with a clear error rather
    /// than fabricating an identity.
    #[must_use]
    pub fn with_userinfo_url(mut self, userinfo_url: impl Into<String>) -> Self {
        self.userinfo_url = Some(userinfo_url.into());
        self
    }

    /// Gets the authorization URL for the OAuth2 flow, with PKCE (RFC 7636).
    ///
    /// Generates a fresh `code_verifier`/`code_challenge` (S256) pair, remembers the verifier
    /// keyed by `state`, and includes `code_challenge`/`code_challenge_method=S256` in the
    /// returned URL. The matching verifier is sent automatically by [`Self::exchange_code`],
    /// protecting public/native/SPA clients against authorization-code interception.
    pub fn get_authorization_url(&self, redirect_uri: &str, state: &str) -> String {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        self.pkce_verifiers
            .insert(state.to_string(), verifier.secret().clone());

        format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
            self.auth_url,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(state),
            urlencoding::encode(challenge.as_str()),
        )
    }

    /// Exchanges authorization code for access token.
    ///
    /// Performs a real POST to the identity provider's token endpoint (RFC 6749 section
    /// 4.1.3). The IdP validates `code`/`redirect_uri`/client credentials; on any failure
    /// (invalid code, mismatched redirect_uri, network error, malformed response) this
    /// returns an error instead of minting a token.
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        state: &str,
    ) -> Result<OAuth2Token> {
        let redirect = RedirectUrl::new(redirect_uri.to_string()).map_err(|error| {
            GatewayError::InvalidRequest(format!("invalid OAuth2 redirect_uri: {error}"))
        })?;

        // Attach the PKCE verifier remembered for this `state` (if get_authorization_url was
        // used). Removing it makes the verifier single-use.
        let pkce_verifier = self
            .pkce_verifiers
            .remove(state)
            .map(|(_, secret)| PkceCodeVerifier::new(secret));

        let mut request = self
            .oauth_client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_redirect_uri(Cow::Owned(redirect));
        if let Some(verifier) = pkce_verifier {
            request = request.set_pkce_verifier(verifier);
        }

        let token_response = request
            .request_async(&self.http_client)
            .await
            .map_err(|error| {
                GatewayError::OAuth2Error(format!(
                    "token exchange with identity provider failed: {error}"
                ))
            })?;

        let access_token = token_response.access_token().secret().clone();
        let refresh_token = token_response
            .refresh_token()
            .map(|token| token.secret().clone());
        let expires_at = Self::compute_expiry(token_response.expires_in());

        let identity = self.fetch_identity(&access_token).await?;

        let token = OAuth2Token {
            access_token: access_token.clone(),
            token_type: "Bearer".to_string(),
            expires_at,
            refresh_token,
            identity,
        };

        self.tokens.insert(access_token, token.clone());

        Ok(token)
    }

    /// Refreshes an OAuth2 token using the refresh token.
    ///
    /// Performs a real POST to the identity provider's token endpoint with the
    /// `refresh_token` grant (RFC 6749 section 6) rather than fabricating a new local token.
    pub async fn refresh_token_with_refresh(&self, refresh_token: &str) -> Result<OAuth2Token> {
        // Find the old token by refresh token
        let old_token = self
            .tokens
            .iter()
            .find(|entry| entry.value().refresh_token.as_deref() == Some(refresh_token))
            .ok_or_else(|| GatewayError::InvalidToken("Invalid refresh token".to_string()))?;

        let identity = old_token.value().identity.clone();
        let old_access_token = old_token.value().access_token.clone();
        drop(old_token);

        let token_response = self
            .oauth_client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(|error| {
                GatewayError::OAuth2Error(format!(
                    "token refresh with identity provider failed: {error}"
                ))
            })?;

        let access_token = token_response.access_token().secret().clone();
        // Some providers omit `refresh_token` on refresh responses, meaning the original
        // refresh token remains valid (no rotation) -- preserve it in that case.
        let new_refresh_token = token_response
            .refresh_token()
            .map(|token| token.secret().clone())
            .or_else(|| Some(refresh_token.to_string()));
        let expires_at = Self::compute_expiry(token_response.expires_in());

        let new_token = OAuth2Token {
            access_token: access_token.clone(),
            token_type: "Bearer".to_string(),
            expires_at,
            refresh_token: new_refresh_token,
            identity,
        };

        self.tokens.remove(&old_access_token);
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

    /// Calls the configured userinfo endpoint with the given access token and derives a
    /// verified [`Identity`] from the response. Returns an error (never a fabricated
    /// identity) if no userinfo endpoint is configured, the request fails, or the response
    /// is missing a subject claim.
    async fn fetch_identity(&self, access_token: &str) -> Result<Identity> {
        let userinfo_url = self.userinfo_url.as_ref().ok_or_else(|| {
            GatewayError::ConfigError(
                "OAuth2 userinfo_url not configured; cannot derive a verified identity".to_string(),
            )
        })?;

        let response = self
            .http_client
            .get(userinfo_url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| {
                GatewayError::OAuth2Error(format!("userinfo request failed: {error}"))
            })?;

        if !response.status().is_success() {
            return Err(GatewayError::OAuth2Error(format!(
                "userinfo endpoint returned status {}",
                response.status()
            )));
        }

        let body = response.bytes().await.map_err(|error| {
            GatewayError::OAuth2Error(format!("failed to read userinfo response: {error}"))
        })?;
        let payload: serde_json::Value = serde_json::from_slice(&body).map_err(|error| {
            GatewayError::OAuth2Error(format!("invalid userinfo response: {error}"))
        })?;

        let user_id = payload
            .get("sub")
            .or_else(|| payload.get("id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                GatewayError::OAuth2Error("userinfo response missing 'sub'/'id' claim".to_string())
            })?
            .to_string();

        let mut identity = Identity::new(user_id);
        identity.email = payload
            .get("email")
            .and_then(|value| value.as_str())
            .map(|s| s.to_string());

        if let Some(roles) = payload.get("roles").and_then(|value| value.as_array()) {
            identity.roles = roles
                .iter()
                .filter_map(|value| value.as_str())
                .map(|s| s.to_string())
                .collect();
        }

        if let Some(permissions) = payload
            .get("permissions")
            .and_then(|value| value.as_array())
        {
            identity.permissions = permissions
                .iter()
                .filter_map(|value| value.as_str())
                .map(|s| s.to_string())
                .collect();
        }

        Ok(identity)
    }

    /// Computes the expiry timestamp from the provider's `expires_in`, defaulting to one
    /// hour when the provider omits it (per RFC 6749 section 5.1, `expires_in` is optional).
    fn compute_expiry(expires_in: Option<std::time::Duration>) -> chrono::DateTime<chrono::Utc> {
        let ttl = expires_in
            .and_then(|duration| chrono::Duration::from_std(duration).ok())
            .unwrap_or_else(|| chrono::Duration::hours(1));

        chrono::Utc::now() + ttl
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

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    /// A minimal single-connection-per-request HTTP/1.1 test server used to stand in for an
    /// identity provider's token/userinfo endpoints, without pulling in a new test-only mock
    /// HTTP crate (the workspace already depends on tokio, which is enough to hand-roll this).
    struct MockIdp {
        addr: SocketAddr,
        handle: JoinHandle<()>,
    }

    impl MockIdp {
        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        fn token_url(&self) -> String {
            format!("{}/token", self.base_url())
        }

        fn userinfo_url(&self) -> String {
            format!("{}/userinfo", self.base_url())
        }
    }

    impl Drop for MockIdp {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    /// Spawns a background HTTP server that routes `POST /token` and `GET /userinfo` to
    /// canned JSON responses, looping until aborted. Successive `/token` calls cycle through
    /// `token_bodies` (so an initial exchange and a subsequent refresh can be told apart),
    /// sticking on the last entry once exhausted.
    async fn spawn_mock_idp(
        token_bodies: &'static [&'static str],
        userinfo_body: &'static str,
    ) -> MockIdp {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock IdP listener");
        let addr = listener.local_addr().expect("local addr");
        let token_call = std::sync::atomic::AtomicUsize::new(0);

        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };

                let mut buf = vec![0u8; 8192];
                let n = match stream.read(&mut buf).await {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();

                let body: &str = if path.starts_with("/token") {
                    let call = token_call.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    token_bodies[call.min(token_bodies.len().saturating_sub(1))]
                } else if path.starts_with("/userinfo") {
                    userinfo_body
                } else {
                    "{}"
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );

                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        MockIdp { addr, handle }
    }

    async fn create_test_authenticator(idp: &MockIdp) -> OAuth2Authenticator {
        OAuth2Authenticator::new(
            "test_client_id",
            "test_client_secret",
            "https://auth.example.com/oauth/authorize",
            &idp.token_url(),
        )
        .expect("build authenticator")
        .with_userinfo_url(idp.userinfo_url())
    }

    #[test]
    fn test_authorization_url() {
        let auth = OAuth2Authenticator::new(
            "test_client_id",
            "test_client_secret",
            "https://auth.example.com/oauth/authorize",
            "https://auth.example.com/oauth/token",
        )
        .expect("build authenticator");
        let url = auth.get_authorization_url("https://example.com/callback", "random_state");

        assert!(url.contains("client_id=test_client_id"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("state=random_state"));
        // PKCE (RFC 7636) must be present for public-client protection.
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn test_pkce_verifier_is_stored_per_state() {
        let auth = OAuth2Authenticator::new(
            "test_client_id",
            "test_client_secret",
            "https://auth.example.com/oauth/authorize",
            "https://auth.example.com/oauth/token",
        )
        .expect("build authenticator");

        let _ = auth.get_authorization_url("https://example.com/callback", "state-xyz");
        // A verifier must be remembered for the state so exchange_code can present it.
        assert!(auth.pkce_verifiers.contains_key("state-xyz"));
    }

    #[tokio::test]
    async fn test_exchange_code_invalid_provider_response_fails() {
        // A provider that returns garbage for the token exchange must produce an error, not
        // a minted token -- this is the core regression test for the auth-bypass this file
        // used to contain (any `code` string used to always succeed with a mock identity).
        let idp = spawn_mock_idp(&["not json"], "{}").await;
        let auth = create_test_authenticator(&idp).await;

        let result = auth
            .exchange_code(
                "whatever-code",
                "https://example.com/callback",
                "test-state",
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exchange_code_without_userinfo_url_fails_closed() {
        // Even if the token endpoint succeeds, without a configured userinfo endpoint we must
        // refuse to fabricate an identity.
        let idp = spawn_mock_idp(
            &[r#"{"access_token":"real_token_abc","token_type":"Bearer","expires_in":3600,"refresh_token":"real_refresh_abc"}"#],
            r#"{"sub":"provider_user_1"}"#,
        )
        .await;

        let auth = OAuth2Authenticator::new(
            "test_client_id",
            "test_client_secret",
            "https://auth.example.com/oauth/authorize",
            &idp.token_url(),
        )
        .expect("build authenticator");

        let result = auth
            .exchange_code("test_code", "https://example.com/callback", "test-state")
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exchange_code_succeeds_with_real_provider_identity() {
        let idp = spawn_mock_idp(
            &[r#"{"access_token":"real_token_abc","token_type":"Bearer","expires_in":3600,"refresh_token":"real_refresh_abc"}"#],
            r#"{"sub":"provider_user_1","email":"user@example.com","roles":["admin"]}"#,
        )
        .await;
        let auth = create_test_authenticator(&idp).await;

        let token = auth
            .exchange_code("test_code", "https://example.com/callback", "test-state")
            .await
            .expect("exchange should succeed against a well-formed mock IdP");

        assert_eq!(token.access_token, "real_token_abc");
        assert_eq!(token.refresh_token.as_deref(), Some("real_refresh_abc"));
        assert_eq!(token.identity.user_id, "provider_user_1");
        assert_eq!(token.identity.email.as_deref(), Some("user@example.com"));
        assert!(token.identity.has_role("admin"));
    }

    #[tokio::test]
    async fn test_authenticate() {
        let idp = spawn_mock_idp(
            &[r#"{"access_token":"real_token_abc","token_type":"Bearer","expires_in":3600,"refresh_token":"real_refresh_abc"}"#],
            r#"{"sub":"provider_user_1"}"#,
        )
        .await;
        let auth = create_test_authenticator(&idp).await;

        let token = auth
            .exchange_code("test_code", "https://example.com/callback", "test-state")
            .await
            .expect("exchange should succeed");

        let context = auth
            .authenticate(&token.access_token)
            .await
            .expect("authenticate should succeed for a freshly minted token");
        assert_eq!(context.method, AuthMethod::OAuth2);
        assert_eq!(context.identity.user_id, "provider_user_1");
    }

    #[tokio::test]
    async fn test_refresh_token() {
        let idp = spawn_mock_idp(
            &[
                r#"{"access_token":"real_token_abc","token_type":"Bearer","expires_in":3600,"refresh_token":"real_refresh_abc"}"#,
                r#"{"access_token":"refreshed_token_xyz","token_type":"Bearer","expires_in":3600,"refresh_token":"refreshed_refresh_xyz"}"#,
            ],
            r#"{"sub":"provider_user_1"}"#,
        )
        .await;
        let auth = create_test_authenticator(&idp).await;

        let token = auth
            .exchange_code("test_code", "https://example.com/callback", "test-state")
            .await
            .expect("exchange should succeed");

        let context = auth
            .authenticate(&token.access_token)
            .await
            .expect("authenticate should succeed");

        let new_access_token = auth
            .refresh(&context)
            .await
            .expect("refresh should succeed");
        assert_ne!(token.access_token, new_access_token);
        assert_eq!(new_access_token, "refreshed_token_xyz");

        // The refreshed token should authenticate successfully with the same identity, and
        // the old (pre-refresh) access token must no longer work.
        let refreshed_context = auth
            .authenticate(&new_access_token)
            .await
            .expect("refreshed token should authenticate");
        assert_eq!(refreshed_context.identity.user_id, "provider_user_1");
        assert!(auth.authenticate(&token.access_token).await.is_err());
    }

    #[tokio::test]
    async fn test_refresh_token_invalid_fails() {
        let idp = spawn_mock_idp(&["{}"], "{}").await;
        let auth = create_test_authenticator(&idp).await;

        let result = auth
            .refresh_token_with_refresh("not-a-real-refresh-token")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_revoke_token() {
        let idp = spawn_mock_idp(
            &[r#"{"access_token":"real_token_abc","token_type":"Bearer","expires_in":3600,"refresh_token":"real_refresh_abc"}"#],
            r#"{"sub":"provider_user_1"}"#,
        )
        .await;
        let auth = create_test_authenticator(&idp).await;

        let token = auth
            .exchange_code("test_code", "https://example.com/callback", "test-state")
            .await
            .expect("exchange should succeed");

        // Token should work before revocation
        assert!(auth.authenticate(&token.access_token).await.is_ok());

        // Revoke the token
        assert!(auth.revoke(&token.access_token).await.is_ok());

        // Token should not work after revocation
        assert!(auth.authenticate(&token.access_token).await.is_err());
    }
}
