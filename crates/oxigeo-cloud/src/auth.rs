//! Authentication strategies for cloud storage backends
//!
//! This module provides various authentication methods for cloud providers,
//! including OAuth 2.0, service accounts, API keys, SAS tokens, and IAM roles.
//!
//! # Credential refresh
//!
//! [`Credentials::is_expired`]/[`Credentials::needs_refresh`] and
//! [`CredentialProvider::refresh`] describe *when* a credential should be
//! refreshed, but on their own they are never invoked automatically.
//! [`RefreshingCredentials`] is the piece that actually calls them: wrap a
//! [`Credentials`] value and (optionally) a [`CredentialProvider`] in one,
//! then call [`RefreshingCredentials::ensure_fresh`] immediately before each
//! request a backend makes. `HttpBackend`'s `HttpAuth::OAuth2` variant
//! (gated behind the `http` feature) is a real, wired-up consumer.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::error::{AuthError, CloudError, Result};

/// Authentication credentials
#[derive(Debug, Clone)]
pub enum Credentials {
    /// No authentication
    None,

    /// API key authentication
    ApiKey {
        /// API key
        key: String,
    },

    /// Access key and secret key (AWS-style)
    AccessKey {
        /// Access key ID
        access_key: String,
        /// Secret access key
        secret_key: String,
        /// Optional session token
        session_token: Option<String>,
    },

    /// OAuth 2.0 token
    OAuth2 {
        /// Access token
        access_token: String,
        /// Optional refresh token
        refresh_token: Option<String>,
        /// Token expiration time
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    },

    /// Service account key (GCP-style JSON)
    ServiceAccount {
        /// Service account key JSON
        key_json: String,
        /// Project ID
        project_id: Option<String>,
    },

    /// Shared Access Signature token (Azure-style)
    SasToken {
        /// SAS token
        token: String,
        /// Token expiration time
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    },

    /// IAM role credentials
    IamRole {
        /// Role ARN
        role_arn: String,
        /// Session name
        session_name: String,
    },

    /// Custom credentials with arbitrary key-value pairs
    Custom {
        /// Credential data
        data: HashMap<String, String>,
    },
}

impl Credentials {
    /// Creates API key credentials
    #[must_use]
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::ApiKey { key: key.into() }
    }

    /// Creates access key credentials
    #[must_use]
    pub fn access_key(access_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self::AccessKey {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: None,
        }
    }

    /// Creates access key credentials with session token
    #[must_use]
    pub fn access_key_with_session(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        session_token: impl Into<String>,
    ) -> Self {
        Self::AccessKey {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: Some(session_token.into()),
        }
    }

    /// Creates OAuth 2.0 credentials
    #[must_use]
    pub fn oauth2(access_token: impl Into<String>) -> Self {
        Self::OAuth2 {
            access_token: access_token.into(),
            refresh_token: None,
            expires_at: None,
        }
    }

    /// Creates OAuth 2.0 credentials with refresh token
    #[must_use]
    pub fn oauth2_with_refresh(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
    ) -> Self {
        Self::OAuth2 {
            access_token: access_token.into(),
            refresh_token: Some(refresh_token.into()),
            expires_at: None,
        }
    }

    /// Creates service account credentials from JSON
    pub fn service_account_from_json(json: impl Into<String>) -> Result<Self> {
        let json_str = json.into();

        // Try to parse JSON to validate
        let parsed: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            CloudError::Auth(AuthError::ServiceAccountKey {
                message: format!("Invalid JSON: {e}"),
            })
        })?;

        // Extract project ID if available
        let project_id = parsed
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(Self::ServiceAccount {
            key_json: json_str,
            project_id,
        })
    }

    /// Creates service account credentials from file
    pub fn service_account_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            CloudError::Auth(AuthError::ServiceAccountKey {
                message: format!("Failed to read service account key file: {e}"),
            })
        })?;

        Self::service_account_from_json(content)
    }

    /// Creates SAS token credentials
    #[must_use]
    pub fn sas_token(token: impl Into<String>) -> Self {
        Self::SasToken {
            token: token.into(),
            expires_at: None,
        }
    }

    /// Creates IAM role credentials
    #[must_use]
    pub fn iam_role(role_arn: impl Into<String>, session_name: impl Into<String>) -> Self {
        Self::IamRole {
            role_arn: role_arn.into(),
            session_name: session_name.into(),
        }
    }

    /// Checks if credentials are expired
    #[must_use]
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now();

        match self {
            Self::OAuth2 {
                expires_at: Some(expiry),
                ..
            } => *expiry <= now,
            Self::SasToken {
                expires_at: Some(expiry),
                ..
            } => *expiry <= now,
            _ => false,
        }
    }

    /// Returns true if credentials need refresh
    #[must_use]
    pub fn needs_refresh(&self) -> bool {
        let now = chrono::Utc::now();
        let buffer = chrono::Duration::minutes(5); // Refresh 5 minutes before expiry

        match self {
            Self::OAuth2 {
                expires_at: Some(expiry),
                ..
            } => *expiry <= now + buffer,
            Self::SasToken {
                expires_at: Some(expiry),
                ..
            } => *expiry <= now + buffer,
            _ => false,
        }
    }

    /// Returns a short, secret-free name for the credentials variant.
    ///
    /// Intended for diagnostics/error messages where the full `Debug`
    /// representation would risk leaking secret material (access keys,
    /// tokens, service account JSON, etc.).
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::ApiKey { .. } => "ApiKey",
            Self::AccessKey { .. } => "AccessKey",
            Self::OAuth2 { .. } => "OAuth2",
            Self::ServiceAccount { .. } => "ServiceAccount",
            Self::SasToken { .. } => "SasToken",
            Self::IamRole { .. } => "IamRole",
            Self::Custom { .. } => "Custom",
        }
    }
}

/// Credential provider trait for dynamic credential loading
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait CredentialProvider: Send + Sync {
    /// Loads credentials
    async fn load(&self) -> Result<Credentials>;

    /// Refreshes credentials if needed
    async fn refresh(&self, _credentials: &Credentials) -> Result<Credentials> {
        // Default implementation: just reload
        self.load().await
    }
}

/// A [`Credentials`] value that keeps itself fresh.
///
/// This is the piece that actually *uses*
/// [`Credentials::needs_refresh`]/[`is_expired`](Credentials::is_expired) and
/// [`CredentialProvider::refresh`]: without it, those methods exist but
/// nothing ever calls them, and a long-running process configured with a
/// time-limited OAuth2/SAS credential silently starts failing every request
/// once the token expires.
///
/// Call [`ensure_fresh`](Self::ensure_fresh) immediately before every
/// request a backend makes (e.g. when building the per-request client /
/// headers). If the current credentials are still valid, this is a cheap
/// read-lock check; if they're expiring and a [`CredentialProvider`] is
/// attached, it performs a real refresh (e.g. an OAuth2 refresh-token grant
/// via [`OAuth2RefreshProvider`]) and atomically swaps in the result so
/// concurrent callers observe the new credentials.
#[cfg(feature = "async")]
pub struct RefreshingCredentials {
    current: tokio::sync::RwLock<Credentials>,
    provider: Option<Arc<dyn CredentialProvider>>,
}

// `CredentialProvider` trait objects aren't `Debug`, so this is implemented
// by hand; it deliberately shows only the credential variant name (never
// the secret material inside it).
#[cfg(feature = "async")]
impl std::fmt::Debug for RefreshingCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshingCredentials")
            .field("has_provider", &self.provider.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "async")]
impl RefreshingCredentials {
    /// Creates a self-refreshing credential wrapper.
    ///
    /// `provider` is optional: without one, expiring credentials are still
    /// detected (and a warning logged) but cannot actually be refreshed --
    /// callers relying on that path should treat `ensure_fresh`'s return
    /// value as best-effort in that configuration.
    #[must_use]
    pub fn new(initial: Credentials, provider: Option<Arc<dyn CredentialProvider>>) -> Self {
        Self {
            current: tokio::sync::RwLock::new(initial),
            provider,
        }
    }

    /// Wraps a static, never-expiring credential (no provider attached).
    /// `ensure_fresh` on this is just a cheap clone of `credentials`.
    #[must_use]
    pub fn static_credentials(credentials: Credentials) -> Self {
        Self::new(credentials, None)
    }

    /// Returns credentials that are not (per [`Credentials::needs_refresh`])
    /// about to expire, refreshing them first via the attached
    /// [`CredentialProvider`] if necessary.
    ///
    /// # Errors
    /// Returns an error if a refresh was needed, a provider was attached,
    /// and the provider's refresh call itself failed (e.g. the refresh
    /// token was rejected by the token endpoint).
    pub async fn ensure_fresh(&self) -> Result<Credentials> {
        // Fast path: no refresh needed, just clone under a read lock.
        {
            let guard = self.current.read().await;
            if !guard.needs_refresh() {
                return Ok(guard.clone());
            }
        }

        let Some(provider) = &self.provider else {
            let guard = self.current.read().await;
            tracing::warn!(
                "Credentials ({}) are expiring or already expired and no CredentialProvider \
                 is configured to refresh them; continuing with the current (possibly stale) \
                 credentials. Configure a CredentialProvider (e.g. OAuth2RefreshProvider) via \
                 RefreshingCredentials::new to enable automatic refresh.",
                guard.variant_name(),
            );
            return Ok(guard.clone());
        };

        // Take the write lock and re-check: another concurrent caller may
        // have already refreshed while we were waiting, in which case we
        // should not hit the token endpoint again.
        let mut guard = self.current.write().await;
        if !guard.needs_refresh() {
            return Ok(guard.clone());
        }

        let refreshed = provider.refresh(&guard).await?;
        *guard = refreshed.clone();
        Ok(refreshed)
    }

    /// Returns the current credentials without checking or forcing a
    /// refresh. Prefer [`ensure_fresh`](Self::ensure_fresh) on any actual
    /// request path.
    pub async fn current(&self) -> Credentials {
        self.current.read().await.clone()
    }
}

/// A [`CredentialProvider`] that performs real OAuth 2.0 refresh-token
/// grants ([RFC 6749 §6](https://www.rfc-editor.org/rfc/rfc6749#section-6))
/// against a token endpoint over HTTP.
///
/// This is a genuine network call (`grant_type=refresh_token` form-encoded
/// POST, parsing the JSON `access_token`/`refresh_token`/`expires_in`
/// response), not a re-load of the same static token: it's what turns
/// [`Credentials::OAuth2`]'s `refresh_token` field from write-only data into
/// something that's actually used.
///
/// `load()` deliberately errors: this provider only knows how to *refresh*
/// an existing OAuth2 credential that already carries a `refresh_token`. The
/// first access/refresh token pair should come from wherever your
/// application's own authorization-code (or similar) flow issued it.
#[cfg(all(feature = "http", feature = "async"))]
pub struct OAuth2RefreshProvider {
    /// OAuth2 token endpoint, e.g. `https://oauth2.googleapis.com/token`.
    token_url: String,
    client_id: String,
    client_secret: Option<String>,
    http_client: reqwest::Client,
}

#[cfg(all(feature = "http", feature = "async"))]
impl OAuth2RefreshProvider {
    /// Creates a refresh provider for a public (secret-less) OAuth2 client.
    #[must_use]
    pub fn new(token_url: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self {
            token_url: token_url.into(),
            client_id: client_id.into(),
            client_secret: None,
            http_client: reqwest::Client::new(),
        }
    }

    /// Attaches a confidential client secret to the refresh grant.
    #[must_use]
    pub fn with_client_secret(mut self, client_secret: impl Into<String>) -> Self {
        self.client_secret = Some(client_secret.into());
        self
    }
}

/// Minimal RFC 6749 §5.1 token endpoint success response.
#[derive(Debug, serde::Deserialize)]
struct OAuth2TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[cfg(all(feature = "http", feature = "async"))]
#[async_trait::async_trait]
impl CredentialProvider for OAuth2RefreshProvider {
    async fn load(&self) -> Result<Credentials> {
        Err(CloudError::Auth(AuthError::OAuth2 {
            message: "OAuth2RefreshProvider only supports refresh() of an existing \
                      refresh_token, not an initial load(); obtain the first access/refresh \
                      token pair from your application's own authorization flow and \
                      construct `Credentials::OAuth2` directly."
                .to_string(),
        }))
    }

    async fn refresh(&self, credentials: &Credentials) -> Result<Credentials> {
        let refresh_token = match credentials {
            Credentials::OAuth2 {
                refresh_token: Some(rt),
                ..
            } => rt.clone(),
            Credentials::OAuth2 {
                refresh_token: None,
                ..
            } => {
                return Err(CloudError::Auth(AuthError::OAuth2 {
                    message: "Cannot refresh an OAuth2 credential that has no refresh_token"
                        .to_string(),
                }));
            }
            other => {
                return Err(CloudError::Auth(AuthError::OAuth2 {
                    message: format!(
                        "OAuth2RefreshProvider can only refresh OAuth2 credentials, got '{}'",
                        other.variant_name()
                    ),
                }));
            }
        };

        let mut form: Vec<(&str, String)> = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.clone()),
            ("client_id", self.client_id.clone()),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.clone()));
        }

        let response = self
            .http_client
            .post(&self.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                CloudError::Auth(AuthError::OAuth2 {
                    message: format!("Token refresh request to '{}' failed: {e}", self.token_url),
                })
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(CloudError::Auth(AuthError::OAuth2 {
                message: format!(
                    "Token endpoint '{}' rejected the refresh grant (HTTP {status}): {body}",
                    self.token_url
                ),
            }));
        }

        let parsed: OAuth2TokenResponse = response.json().await.map_err(|e| {
            CloudError::Auth(AuthError::OAuth2 {
                message: format!("Failed to parse token endpoint response: {e}"),
            })
        })?;

        let expires_at = parsed
            .expires_in
            .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));

        Ok(Credentials::OAuth2 {
            access_token: parsed.access_token,
            // Some token endpoints omit `refresh_token` on a refresh
            // response, meaning "unchanged" (RFC 6749 §6); keep the old one
            // in that case rather than discarding it.
            refresh_token: parsed.refresh_token.or(Some(refresh_token)),
            expires_at,
        })
    }
}

/// Environment variable credential provider
pub struct EnvCredentialProvider {
    /// Credential type
    credential_type: CredentialType,
}

/// Supported credential types for environment variable provider
#[derive(Debug, Clone, Copy)]
pub enum CredentialType {
    /// AWS access key credentials
    Aws,
    /// Azure storage credentials
    Azure,
    /// GCP service account credentials
    Gcp,
    /// Generic API key
    ApiKey,
}

impl EnvCredentialProvider {
    /// Creates a new environment variable credential provider
    #[must_use]
    pub const fn new(credential_type: CredentialType) -> Self {
        Self { credential_type }
    }

    /// Loads AWS credentials from environment variables
    fn load_aws() -> Result<Credentials> {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID").map_err(|_| {
            CloudError::Auth(AuthError::CredentialsNotFound {
                message: "AWS_ACCESS_KEY_ID not found".to_string(),
            })
        })?;

        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").map_err(|_| {
            CloudError::Auth(AuthError::CredentialsNotFound {
                message: "AWS_SECRET_ACCESS_KEY not found".to_string(),
            })
        })?;

        let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

        Ok(Credentials::AccessKey {
            access_key,
            secret_key,
            session_token,
        })
    }

    /// Loads Azure credentials from environment variables
    fn load_azure() -> Result<Credentials> {
        let account_name = std::env::var("AZURE_STORAGE_ACCOUNT").map_err(|_| {
            CloudError::Auth(AuthError::CredentialsNotFound {
                message: "AZURE_STORAGE_ACCOUNT not found".to_string(),
            })
        })?;

        // Try account key first, then SAS token
        if let Ok(account_key) = std::env::var("AZURE_STORAGE_KEY") {
            let mut data = HashMap::new();
            data.insert("account_name".to_string(), account_name);
            data.insert("account_key".to_string(), account_key);

            Ok(Credentials::Custom { data })
        } else if let Ok(sas_token) = std::env::var("AZURE_STORAGE_SAS_TOKEN") {
            Ok(Credentials::SasToken {
                token: sas_token,
                expires_at: None,
            })
        } else {
            Err(CloudError::Auth(AuthError::CredentialsNotFound {
                message: "Neither AZURE_STORAGE_KEY nor AZURE_STORAGE_SAS_TOKEN found".to_string(),
            }))
        }
    }

    /// Loads GCP credentials from environment variables
    fn load_gcp() -> Result<Credentials> {
        let key_file = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").map_err(|_| {
            CloudError::Auth(AuthError::CredentialsNotFound {
                message: "GOOGLE_APPLICATION_CREDENTIALS not found".to_string(),
            })
        })?;

        Credentials::service_account_from_file(&key_file)
    }

    /// Loads API key from environment variables
    fn load_api_key() -> Result<Credentials> {
        let key = std::env::var("API_KEY")
            .or_else(|_| std::env::var("APIKEY"))
            .map_err(|_| {
                CloudError::Auth(AuthError::CredentialsNotFound {
                    message: "API_KEY or APIKEY not found".to_string(),
                })
            })?;

        Ok(Credentials::ApiKey { key })
    }
}

#[cfg(feature = "async")]
#[async_trait::async_trait]
impl CredentialProvider for EnvCredentialProvider {
    async fn load(&self) -> Result<Credentials> {
        match self.credential_type {
            CredentialType::Aws => Self::load_aws(),
            CredentialType::Azure => Self::load_azure(),
            CredentialType::Gcp => Self::load_gcp(),
            CredentialType::ApiKey => Self::load_api_key(),
        }
    }
}

/// File-based credential provider
pub struct FileCredentialProvider {
    /// Path to credentials file
    path: std::path::PathBuf,
}

impl FileCredentialProvider {
    /// Creates a new file credential provider
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

#[cfg(feature = "async")]
#[async_trait::async_trait]
impl CredentialProvider for FileCredentialProvider {
    async fn load(&self) -> Result<Credentials> {
        Credentials::service_account_from_file(&self.path)
    }
}

/// Chain credential provider that tries multiple providers in order
pub struct ChainCredentialProvider {
    /// List of credential providers
    providers: Vec<Box<dyn CredentialProvider>>,
}

impl ChainCredentialProvider {
    /// Creates a new chain credential provider
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Adds a credential provider to the chain
    #[must_use]
    pub fn with_provider(mut self, provider: Box<dyn CredentialProvider>) -> Self {
        self.providers.push(provider);
        self
    }
}

impl Default for ChainCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "async")]
#[async_trait::async_trait]
impl CredentialProvider for ChainCredentialProvider {
    async fn load(&self) -> Result<Credentials> {
        for provider in &self.providers {
            if let Ok(credentials) = provider.load().await {
                return Ok(credentials);
            }
        }

        Err(CloudError::Auth(AuthError::CredentialsNotFound {
            message: "No credential provider succeeded".to_string(),
        }))
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_api_key() {
        let creds = Credentials::api_key("test-key");
        match creds {
            Credentials::ApiKey { key } => assert_eq!(key, "test-key"),
            _ => panic!("Expected ApiKey credentials"),
        }
    }

    #[test]
    fn test_credentials_access_key() {
        let creds = Credentials::access_key("access", "secret");
        match creds {
            Credentials::AccessKey {
                access_key,
                secret_key,
                session_token,
            } => {
                assert_eq!(access_key, "access");
                assert_eq!(secret_key, "secret");
                assert!(session_token.is_none());
            }
            _ => panic!("Expected AccessKey credentials"),
        }
    }

    #[test]
    fn test_credentials_oauth2() {
        let creds = Credentials::oauth2("token");
        match creds {
            Credentials::OAuth2 { access_token, .. } => assert_eq!(access_token, "token"),
            _ => panic!("Expected OAuth2 credentials"),
        }
    }

    #[test]
    fn test_credentials_sas_token() {
        let creds = Credentials::sas_token("token");
        match creds {
            Credentials::SasToken { token, .. } => assert_eq!(token, "token"),
            _ => panic!("Expected SasToken credentials"),
        }
    }

    #[test]
    fn test_credentials_iam_role() {
        let creds = Credentials::iam_role("arn:aws:iam::123:role/test", "session");
        match creds {
            Credentials::IamRole {
                role_arn,
                session_name,
            } => {
                assert_eq!(role_arn, "arn:aws:iam::123:role/test");
                assert_eq!(session_name, "session");
            }
            _ => panic!("Expected IamRole credentials"),
        }
    }

    #[test]
    fn test_credentials_service_account_from_json() {
        let json = r#"{"type":"service_account","project_id":"test-project"}"#;
        let creds = Credentials::service_account_from_json(json);
        assert!(creds.is_ok());

        match creds.ok() {
            Some(Credentials::ServiceAccount {
                project_id: Some(project_id),
                ..
            }) => {
                assert_eq!(project_id, "test-project");
            }
            _ => panic!("Expected ServiceAccount credentials with project_id"),
        }
    }

    #[test]
    fn test_credentials_is_expired() {
        let now = chrono::Utc::now();
        let past = now - chrono::Duration::hours(1);
        let future = now + chrono::Duration::hours(1);

        let expired = Credentials::OAuth2 {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: Some(past),
        };
        assert!(expired.is_expired());

        let valid = Credentials::OAuth2 {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: Some(future),
        };
        assert!(!valid.is_expired());
    }

    #[test]
    fn test_credentials_needs_refresh() {
        let now = chrono::Utc::now();
        let soon = now + chrono::Duration::minutes(3); // Within 5-minute buffer
        let later = now + chrono::Duration::hours(1);

        let needs_refresh = Credentials::OAuth2 {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: Some(soon),
        };
        assert!(needs_refresh.needs_refresh());

        let valid = Credentials::OAuth2 {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: Some(later),
        };
        assert!(!valid.needs_refresh());
    }

    // -- RefreshingCredentials -------------------------------------------

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_refreshing_credentials_no_refresh_needed_returns_current() {
        let creds = Credentials::api_key("static-key");
        let refreshing = RefreshingCredentials::static_credentials(creds);

        let result = refreshing
            .ensure_fresh()
            .await
            .expect("ensure_fresh should succeed for non-expiring credentials");
        match result {
            Credentials::ApiKey { key } => assert_eq!(key, "static-key"),
            other => panic!("expected ApiKey credentials, got {other:?}"),
        }
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_refreshing_credentials_without_provider_warns_and_returns_stale() {
        let expired = Credentials::OAuth2 {
            access_token: "old-token".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
        };
        let refreshing = RefreshingCredentials::new(expired, None);

        // No provider attached: we can't manufacture a fresh token, but this
        // must not error or panic -- it should honestly hand back the stale
        // credentials (the warning is logged via `tracing::warn!`).
        let result = refreshing
            .ensure_fresh()
            .await
            .expect("ensure_fresh without a provider should still succeed");
        match result {
            Credentials::OAuth2 { access_token, .. } => assert_eq!(access_token, "old-token"),
            other => panic!("expected OAuth2 credentials, got {other:?}"),
        }
    }

    #[cfg(feature = "async")]
    struct CountingProvider {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[cfg(feature = "async")]
    #[async_trait::async_trait]
    impl CredentialProvider for CountingProvider {
        async fn load(&self) -> Result<Credentials> {
            Err(CloudError::Auth(AuthError::CredentialsNotFound {
                message: "load() not supported by CountingProvider".to_string(),
            }))
        }

        async fn refresh(&self, _credentials: &Credentials) -> Result<Credentials> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Credentials::OAuth2 {
                access_token: "refreshed-token".to_string(),
                refresh_token: Some("rt".to_string()),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            })
        }
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_refreshing_credentials_calls_provider_and_caches_result() {
        let expired = Credentials::OAuth2 {
            access_token: "old-token".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
        };
        let provider = Arc::new(CountingProvider {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let refreshing = RefreshingCredentials::new(expired, Some(provider.clone()));

        let first = refreshing
            .ensure_fresh()
            .await
            .expect("first ensure_fresh should refresh");
        match first {
            Credentials::OAuth2 { access_token, .. } => {
                assert_eq!(access_token, "refreshed-token")
            }
            other => panic!("expected OAuth2 credentials, got {other:?}"),
        }
        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Second call: the refreshed credentials are not expiring, so the
        // provider must not be called again.
        let second = refreshing
            .ensure_fresh()
            .await
            .expect("second ensure_fresh should succeed without refreshing");
        match second {
            Credentials::OAuth2 { access_token, .. } => {
                assert_eq!(access_token, "refreshed-token")
            }
            other => panic!("expected OAuth2 credentials, got {other:?}"),
        }
        assert_eq!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "provider must not be called again once credentials are fresh"
        );
    }

    // -- OAuth2RefreshProvider ---------------------------------------------

    #[cfg(all(feature = "http", feature = "async"))]
    mod oauth2_refresh_provider_tests {
        use super::*;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        /// Minimal single-shot HTTP/1.1 server that captures the request
        /// body it received and replies with a fixed JSON body.
        async fn serve_one_capturing(
            listener: tokio::net::TcpListener,
            status_line: &'static str,
            response_body: String,
        ) -> String {
            let (mut socket, _) = listener.accept().await.expect("accept failed");
            let mut buf = vec![0u8; 8192];
            let mut received = Vec::new();
            // Read headers first.
            loop {
                let n = socket.read(&mut buf).await.expect("read failed");
                received.extend_from_slice(&buf[..n]);
                if received.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if n == 0 {
                    break;
                }
            }
            let header_text = String::from_utf8_lossy(&received).to_string();
            let content_length: usize = header_text
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                .and_then(|l| l.split_once(':').map(|x| x.1))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);

            let header_end = received
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| p + 4)
                .unwrap_or(received.len());
            let mut body = received[header_end..].to_vec();
            while body.len() < content_length {
                let n = socket.read(&mut buf).await.expect("read failed");
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..n]);
            }

            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response failed");
            socket.flush().await.expect("flush failed");

            String::from_utf8_lossy(&body).to_string()
        }

        #[tokio::test]
        async fn test_refresh_performs_real_http_grant_and_parses_response() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind failed");
            let addr = listener.local_addr().expect("local_addr failed");
            let token_url = format!("http://{addr}/token");

            let response_body =
                r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600}"#
                    .to_string();
            let server = tokio::spawn(serve_one_capturing(
                listener,
                "200 OK",
                response_body.clone(),
            ));

            let provider = OAuth2RefreshProvider::new(token_url, "client-123")
                .with_client_secret("shh-secret");
            let old = Credentials::OAuth2 {
                access_token: "old-access".to_string(),
                refresh_token: Some("old-refresh".to_string()),
                expires_at: None,
            };

            let refreshed = provider
                .refresh(&old)
                .await
                .expect("refresh should succeed");

            match refreshed {
                Credentials::OAuth2 {
                    access_token,
                    refresh_token,
                    expires_at,
                } => {
                    assert_eq!(access_token, "new-access");
                    assert_eq!(refresh_token.as_deref(), Some("new-refresh"));
                    let expires_at = expires_at.expect("expires_at should be set");
                    let expected = chrono::Utc::now() + chrono::Duration::seconds(3600);
                    let delta = (expires_at - expected).num_seconds().abs();
                    assert!(
                        delta < 5,
                        "expires_at should be ~3600s from now, delta={delta}"
                    );
                }
                other => panic!("expected OAuth2 credentials, got {other:?}"),
            }

            let captured_body = server.await.expect("server task panicked");
            assert!(captured_body.contains("grant_type=refresh_token"));
            assert!(captured_body.contains("refresh_token=old-refresh"));
            assert!(captured_body.contains("client_id=client-123"));
            assert!(captured_body.contains("client_secret=shh-secret"));
        }

        #[tokio::test]
        async fn test_refresh_keeps_old_refresh_token_when_endpoint_omits_it() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind failed");
            let addr = listener.local_addr().expect("local_addr failed");
            let token_url = format!("http://{addr}/token");

            let response_body = r#"{"access_token":"new-access","expires_in":60}"#.to_string();
            let server = tokio::spawn(serve_one_capturing(listener, "200 OK", response_body));

            let provider = OAuth2RefreshProvider::new(token_url, "client-123");
            let old = Credentials::OAuth2 {
                access_token: "old-access".to_string(),
                refresh_token: Some("old-refresh".to_string()),
                expires_at: None,
            };

            let refreshed = provider
                .refresh(&old)
                .await
                .expect("refresh should succeed");
            match refreshed {
                Credentials::OAuth2 { refresh_token, .. } => {
                    assert_eq!(refresh_token.as_deref(), Some("old-refresh"));
                }
                other => panic!("expected OAuth2 credentials, got {other:?}"),
            }

            server.await.expect("server task panicked");
        }

        #[tokio::test]
        async fn test_refresh_rejects_non_oauth2_credentials() {
            let provider = OAuth2RefreshProvider::new("http://127.0.0.1:1/token", "client-123");
            let err = provider
                .refresh(&Credentials::api_key("nope"))
                .await
                .expect_err("expected an error for non-OAuth2 credentials");
            assert!(matches!(err, CloudError::Auth(AuthError::OAuth2 { .. })));
        }

        #[tokio::test]
        async fn test_refresh_rejects_missing_refresh_token() {
            let provider = OAuth2RefreshProvider::new("http://127.0.0.1:1/token", "client-123");
            let no_rt = Credentials::OAuth2 {
                access_token: "tok".to_string(),
                refresh_token: None,
                expires_at: None,
            };
            let err = provider
                .refresh(&no_rt)
                .await
                .expect_err("expected an error for missing refresh_token");
            assert!(matches!(err, CloudError::Auth(AuthError::OAuth2 { .. })));
        }

        #[tokio::test]
        async fn test_load_is_unsupported() {
            let provider = OAuth2RefreshProvider::new("http://127.0.0.1:1/token", "client-123");
            let err = provider
                .load()
                .await
                .expect_err("load() must be unsupported");
            assert!(matches!(err, CloudError::Auth(AuthError::OAuth2 { .. })));
        }

        #[tokio::test]
        async fn test_refresh_surfaces_error_response_from_token_endpoint() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind failed");
            let addr = listener.local_addr().expect("local_addr failed");
            let token_url = format!("http://{addr}/token");

            let response_body = r#"{"error":"invalid_grant"}"#.to_string();
            let server = tokio::spawn(serve_one_capturing(
                listener,
                "400 Bad Request",
                response_body,
            ));

            let provider = OAuth2RefreshProvider::new(token_url, "client-123");
            let old = Credentials::OAuth2 {
                access_token: "old-access".to_string(),
                refresh_token: Some("old-refresh".to_string()),
                expires_at: None,
            };

            let err = provider
                .refresh(&old)
                .await
                .expect_err("a 400 response should surface as an error");
            assert!(matches!(err, CloudError::Auth(AuthError::OAuth2 { .. })));

            server.await.expect("server task panicked");
        }
    }
}
