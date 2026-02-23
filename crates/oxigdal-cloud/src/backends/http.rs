//! Enhanced HTTP storage backend with authentication and retry logic
//!
//! This module provides read-only HTTP/HTTPS storage with authentication support,
//! custom headers, and comprehensive retry logic.

use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;

#[cfg(feature = "http")]
use reqwest::Client;

use crate::auth::Credentials;
use crate::error::{CloudError, HttpError, Result};
use crate::retry::{RetryConfig, RetryExecutor};

use super::CloudStorageBackend;

/// HTTP authentication method
#[derive(Debug, Clone)]
pub enum HttpAuth {
    /// No authentication
    None,
    /// Basic authentication
    Basic {
        /// Username
        username: String,
        /// Password
        password: String,
    },
    /// Bearer token
    Bearer {
        /// Token
        token: String,
    },
    /// API key (custom header)
    ApiKey {
        /// Header name
        header_name: String,
        /// API key value
        key: String,
    },
    /// Custom headers
    Custom {
        /// Headers
        headers: HashMap<String, String>,
    },
}

/// HTTP storage backend
#[derive(Debug, Clone)]
pub struct HttpBackend {
    /// Base URL
    pub base_url: String,
    /// Authentication method
    pub auth: HttpAuth,
    /// Request timeout
    pub timeout: Duration,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Credentials
    pub credentials: Option<Credentials>,
    /// Custom headers
    pub headers: HashMap<String, String>,
    /// Follow redirects
    pub follow_redirects: bool,
    /// Maximum redirects
    pub max_redirects: usize,
}

impl HttpBackend {
    /// Creates a new HTTP backend
    ///
    /// # Arguments
    /// * `base_url` - The base URL for requests
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut url = base_url.into();
        // Ensure URL doesn't end with slash
        if url.ends_with('/') {
            url.pop();
        }

        Self {
            base_url: url,
            auth: HttpAuth::None,
            timeout: Duration::from_secs(300),
            retry_config: RetryConfig::default(),
            credentials: None,
            headers: HashMap::new(),
            follow_redirects: true,
            max_redirects: 10,
        }
    }

    /// Sets authentication method
    #[must_use]
    pub fn with_auth(mut self, auth: HttpAuth) -> Self {
        self.auth = auth;
        self
    }

    /// Sets request timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets retry configuration
    #[must_use]
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Adds a custom header
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets whether to follow redirects
    #[must_use]
    pub fn with_follow_redirects(mut self, follow: bool) -> Self {
        self.follow_redirects = follow;
        self
    }

    fn full_url(&self, key: &str) -> String {
        format!("{}/{}", self.base_url, key)
    }

    #[cfg(feature = "http")]
    fn create_client(&self) -> Result<Client> {
        let mut client_builder =
            Client::builder()
                .timeout(self.timeout)
                .redirect(if self.follow_redirects {
                    reqwest::redirect::Policy::limited(self.max_redirects)
                } else {
                    reqwest::redirect::Policy::none()
                });

        // Build default headers
        let mut headers = reqwest::header::HeaderMap::new();

        // Add authentication
        match &self.auth {
            HttpAuth::None => {}
            HttpAuth::Basic { username, password } => {
                let auth_value = format!("{}:{}", username, password);
                let encoded = base64_encode(auth_value.as_bytes());
                let header_value = format!("Basic {}", encoded);

                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&header_value).map_err(|e| {
                        CloudError::Http(HttpError::InvalidHeader {
                            name: "Authorization".to_string(),
                            message: format!("{e}"),
                        })
                    })?,
                );
            }
            HttpAuth::Bearer { token } => {
                let header_value = format!("Bearer {}", token);

                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&header_value).map_err(|e| {
                        CloudError::Http(HttpError::InvalidHeader {
                            name: "Authorization".to_string(),
                            message: format!("{e}"),
                        })
                    })?,
                );
            }
            HttpAuth::ApiKey { header_name, key } => {
                let header_name_parsed = reqwest::header::HeaderName::from_bytes(
                    header_name.as_bytes(),
                )
                .map_err(|e| {
                    CloudError::Http(HttpError::InvalidHeader {
                        name: header_name.clone(),
                        message: format!("{e}"),
                    })
                })?;

                headers.insert(
                    header_name_parsed,
                    reqwest::header::HeaderValue::from_str(key).map_err(|e| {
                        CloudError::Http(HttpError::InvalidHeader {
                            name: header_name.clone(),
                            message: format!("{e}"),
                        })
                    })?,
                );
            }
            HttpAuth::Custom {
                headers: custom_headers,
            } => {
                for (name, value) in custom_headers {
                    let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                        .map_err(|e| {
                            CloudError::Http(HttpError::InvalidHeader {
                                name: name.clone(),
                                message: format!("{e}"),
                            })
                        })?;

                    headers.insert(
                        header_name,
                        reqwest::header::HeaderValue::from_str(value).map_err(|e| {
                            CloudError::Http(HttpError::InvalidHeader {
                                name: name.clone(),
                                message: format!("{e}"),
                            })
                        })?,
                    );
                }
            }
        }

        // Add custom headers
        for (name, value) in &self.headers {
            let header_name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
                    CloudError::Http(HttpError::InvalidHeader {
                        name: name.clone(),
                        message: format!("{e}"),
                    })
                })?;

            headers.insert(
                header_name,
                reqwest::header::HeaderValue::from_str(value).map_err(|e| {
                    CloudError::Http(HttpError::InvalidHeader {
                        name: name.clone(),
                        message: format!("{e}"),
                    })
                })?,
            );
        }

        client_builder = client_builder.default_headers(headers);

        client_builder.build().map_err(|e| {
            CloudError::Http(HttpError::RequestBuild {
                message: format!("{e}"),
            })
        })
    }
}

/// Simple base64 encoding
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in input.chunks(3) {
        let b1 = chunk[0];
        let b2 = chunk.get(1).copied().unwrap_or(0);
        let b3 = chunk.get(2).copied().unwrap_or(0);

        let n = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);

        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 {
            CHARS[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            CHARS[(n & 63) as usize] as char
        } else {
            '='
        });
    }

    result
}

#[cfg(all(feature = "http", feature = "async"))]
#[async_trait::async_trait]
impl CloudStorageBackend for HttpBackend {
    async fn get(&self, key: &str) -> Result<Bytes> {
        let mut executor = RetryExecutor::new(self.retry_config.clone());

        executor
            .execute(|| async {
                let client = self.create_client()?;
                let url = self.full_url(key);

                let response = client.get(&url).send().await.map_err(|e| {
                    CloudError::Http(HttpError::Network {
                        message: format!("HTTP GET failed for '{url}': {e}"),
                    })
                })?;

                let status = response.status();
                if !status.is_success() {
                    return Err(CloudError::Http(HttpError::Status {
                        status: status.as_u16(),
                        message: format!("HTTP GET failed for '{url}'"),
                    }));
                }

                let bytes = response.bytes().await.map_err(|e| {
                    CloudError::Http(HttpError::ResponseParse {
                        message: format!("Failed to read response body: {e}"),
                    })
                })?;

                Ok(bytes)
            })
            .await
    }

    async fn put(&self, _key: &str, _data: &[u8]) -> Result<()> {
        // HTTP backend is typically read-only
        Err(CloudError::NotSupported {
            operation: "HTTP backend is read-only".to_string(),
        })
    }

    async fn delete(&self, _key: &str) -> Result<()> {
        // HTTP backend is typically read-only
        Err(CloudError::NotSupported {
            operation: "HTTP backend is read-only".to_string(),
        })
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let client = self.create_client()?;
        let url = self.full_url(key);

        match client.head(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn list_prefix(&self, _prefix: &str) -> Result<Vec<String>> {
        // HTTP doesn't support listing
        Err(CloudError::NotSupported {
            operation: "HTTP backend does not support listing".to_string(),
        })
    }

    fn is_readonly(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_backend_new() {
        let backend = HttpBackend::new("https://example.com/data");
        assert_eq!(backend.base_url, "https://example.com/data");
    }

    #[test]
    fn test_http_backend_builder() {
        let backend = HttpBackend::new("https://example.com")
            .with_auth(HttpAuth::Bearer {
                token: "token123".to_string(),
            })
            .with_header("User-Agent", "OxiGDAL/1.0")
            .with_timeout(Duration::from_secs(600))
            .with_follow_redirects(false);

        assert!(matches!(backend.auth, HttpAuth::Bearer { .. }));
        assert_eq!(backend.headers.len(), 1);
        assert_eq!(backend.timeout, Duration::from_secs(600));
        assert!(!backend.follow_redirects);
    }

    #[test]
    fn test_http_backend_full_url() {
        let backend = HttpBackend::new("https://example.com/data");
        assert_eq!(
            backend.full_url("file.txt"),
            "https://example.com/data/file.txt"
        );
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"world"), "d29ybGQ=");
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
    }
}
