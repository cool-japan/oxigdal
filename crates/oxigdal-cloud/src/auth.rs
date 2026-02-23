//! Authentication strategies for cloud storage backends
//!
//! This module provides various authentication methods for cloud providers,
//! including OAuth 2.0, service accounts, API keys, SAS tokens, and IAM roles.

use std::collections::HashMap;
use std::path::Path;

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
}
