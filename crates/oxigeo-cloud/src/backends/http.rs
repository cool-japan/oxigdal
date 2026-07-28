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
use oxigeo_core::io::ByteRange;
use std::sync::Arc;

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
    /// OAuth 2.0 bearer token that is checked (and, if a
    /// [`CredentialProvider`](crate::auth::CredentialProvider) is attached,
    /// automatically refreshed) before every request via
    /// [`RefreshingCredentials::ensure_fresh`](crate::auth::RefreshingCredentials::ensure_fresh).
    ///
    /// Unlike [`HttpAuth::Bearer`], which sends a fixed token forever, this
    /// variant re-checks expiry on every request so a long-running process
    /// doesn't silently start failing once a time-limited token expires.
    OAuth2 {
        /// Self-refreshing OAuth2 credentials
        credentials: Arc<crate::auth::RefreshingCredentials>,
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
    async fn create_client(&self) -> Result<Client> {
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
            HttpAuth::OAuth2 { credentials } => {
                // This is the actual wiring point: check (and, if needed and
                // a provider is attached, refresh) the token on every client
                // build, rather than sending a fixed token forever.
                let fresh = credentials.ensure_fresh().await?;
                let access_token = match fresh {
                    Credentials::OAuth2 { access_token, .. } => access_token,
                    other => {
                        return Err(CloudError::Http(HttpError::InvalidHeader {
                            name: "Authorization".to_string(),
                            message: format!(
                                "HttpAuth::OAuth2 requires Credentials::OAuth2, got '{}'",
                                other.variant_name()
                            ),
                        }));
                    }
                };
                let header_value = format!("Bearer {access_token}");

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
                let client = self.create_client().await?;
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

    async fn get_range(&self, key: &str, range: ByteRange) -> Result<Bytes> {
        if range.is_empty() {
            return Ok(Bytes::new());
        }

        let mut executor = RetryExecutor::new(self.retry_config.clone());
        // HTTP `Range` is inclusive on both ends: "bytes=start-end".
        let last_byte = range.end.saturating_sub(1);
        let range_value = format!("bytes={}-{}", range.start, last_byte);

        executor
            .execute(|| async {
                let client = self.create_client().await?;
                let url = self.full_url(key);

                let response = client
                    .get(&url)
                    .header(reqwest::header::RANGE, range_value.clone())
                    .send()
                    .await
                    .map_err(|e| {
                        CloudError::Http(HttpError::Network {
                            message: format!(
                                "HTTP GET (range {range_value}) failed for '{url}': {e}"
                            ),
                        })
                    })?;

                let status = response.status();
                match status {
                    // Server honored the Range request.
                    reqwest::StatusCode::PARTIAL_CONTENT => response.bytes().await.map_err(|e| {
                        CloudError::Http(HttpError::ResponseParse {
                            message: format!("Failed to read ranged response body: {e}"),
                        })
                    }),
                    // Server ignored the Range header and sent the whole
                    // object; slice out the requested range ourselves so the
                    // caller still gets exactly what it asked for (at the
                    // cost of the bandwidth savings a real 206 would give).
                    reqwest::StatusCode::OK => {
                        let full = response.bytes().await.map_err(|e| {
                            CloudError::Http(HttpError::ResponseParse {
                                message: format!("Failed to read response body: {e}"),
                            })
                        })?;
                        let len = full.len() as u64;
                        let start = range.start.min(len);
                        let end = range.end.min(len).max(start);
                        Ok(full.slice(start as usize..end as usize))
                    }
                    other => Err(CloudError::Http(HttpError::Status {
                        status: other.as_u16(),
                        message: format!("HTTP ranged GET failed for '{url}'"),
                    })),
                }
            })
            .await
    }

    fn supports_native_range_reads(&self) -> bool {
        true
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
        let client = self.create_client().await?;
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
            .with_header("User-Agent", "OxiGeo/1.0")
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

    /// Minimal single-shot HTTP/1.1 test server: accepts one connection,
    /// reads the request headers, and replies according to `responder`
    /// (which receives the parsed `Range:` header value, if any, and
    /// returns the status line plus body bytes to send back).
    async fn serve_one(
        listener: tokio::net::TcpListener,
        full_body: Vec<u8>,
        responder: impl FnOnce(Option<String>, &[u8]) -> (&'static str, Vec<u8>, Vec<(String, String)>)
        + Send
        + 'static,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut socket, _) = listener.accept().await.expect("accept failed");
        let mut buf = vec![0u8; 8192];
        let mut received = Vec::new();
        loop {
            let n = socket.read(&mut buf).await.expect("read failed");
            received.extend_from_slice(&buf[..n]);
            if received.windows(4).any(|w| w == b"\r\n\r\n") || n == 0 {
                break;
            }
        }
        let request = String::from_utf8_lossy(&received);
        let range_header = request
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("range:"))
            .map(|l| {
                l.split_once(':')
                    .map(|x| x.1)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            });

        let (status_line, body, extra_headers) = responder(range_header, &full_body);

        let mut response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in extra_headers {
            response.push_str(&format!("{name}: {value}\r\n"));
        }
        response.push_str("\r\n");

        socket
            .write_all(response.as_bytes())
            .await
            .expect("write header failed");
        socket.write_all(&body).await.expect("write body failed");
        socket.flush().await.expect("flush failed");
    }

    #[tokio::test]
    async fn test_get_range_with_native_206_partial_content() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr failed");

        let full_body = b"0123456789ABCDEFGHIJ".to_vec();
        let server = tokio::spawn(serve_one(listener, full_body, |range, full| {
            // Expect "bytes=5-9"
            let range = range.expect("expected a Range header");
            let spec = range.trim_start_matches("bytes=");
            let mut parts = spec.splitn(2, '-');
            let start: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
            let end: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
            let slice = full[start..=end].to_vec();
            let content_range = format!("bytes {start}-{end}/{}", full.len());
            (
                "206 Partial Content",
                slice,
                vec![("Content-Range".to_string(), content_range)],
            )
        }));

        let backend = HttpBackend::new(format!("http://{addr}"));
        let data = backend
            .get_range("obj", ByteRange::new(5, 10))
            .await
            .expect("get_range failed");
        assert_eq!(&data[..], b"56789");
        assert!(backend.supports_native_range_reads());

        server.await.expect("server task panicked");
    }

    #[tokio::test]
    async fn test_get_range_falls_back_to_client_side_slice_when_server_ignores_range() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr failed");

        let full_body = b"0123456789ABCDEFGHIJ".to_vec();
        let server = tokio::spawn(serve_one(listener, full_body, |_range, full| {
            // Server does not support ranges; always returns the whole body
            // with a plain 200, exactly as some static file hosts do.
            ("200 OK", full.to_vec(), vec![])
        }));

        let backend = HttpBackend::new(format!("http://{addr}"));
        let data = backend
            .get_range("obj", ByteRange::new(5, 10))
            .await
            .expect("get_range failed");
        assert_eq!(&data[..], b"56789");

        server.await.expect("server task panicked");
    }

    #[tokio::test]
    async fn test_get_range_empty_range_returns_empty_without_network_call() {
        // No listener bound at all -- if this made a network call it would
        // fail to connect and return an Err, not Ok(empty).
        let backend = HttpBackend::new("http://127.0.0.1:1");
        let data = backend
            .get_range("obj", ByteRange::new(10, 10))
            .await
            .expect("empty range should short-circuit without any I/O");
        assert!(data.is_empty());
    }

    /// A fake `CredentialProvider` that always refreshes to a fixed token,
    /// used to prove `HttpAuth::OAuth2` really calls through to
    /// `RefreshingCredentials::ensure_fresh` on every request.
    struct FixedRefreshProvider {
        token: String,
    }

    #[async_trait::async_trait]
    impl crate::auth::CredentialProvider for FixedRefreshProvider {
        async fn load(&self) -> Result<Credentials> {
            Err(CloudError::Http(HttpError::InvalidHeader {
                name: "n/a".to_string(),
                message: "load() unused in this test".to_string(),
            }))
        }

        async fn refresh(&self, _credentials: &Credentials) -> Result<Credentials> {
            Ok(Credentials::OAuth2 {
                access_token: self.token.clone(),
                refresh_token: Some("rt".to_string()),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            })
        }
    }

    #[tokio::test]
    async fn test_oauth2_auth_refreshes_expiring_token_and_sends_it_as_bearer() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr failed");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept failed");
            let mut buf = vec![0u8; 8192];
            let mut received = Vec::new();
            loop {
                let n = socket.read(&mut buf).await.expect("read failed");
                received.extend_from_slice(&buf[..n]);
                if received.windows(4).any(|w| w == b"\r\n\r\n") || n == 0 {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&received).to_string();

            let body = b"payload";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write header failed");
            socket.write_all(body).await.expect("write body failed");
            socket.flush().await.expect("flush failed");

            request
        });

        // Start with an already-expired credential and no cached token: the
        // very first request must trigger a refresh via the attached
        // provider before the GET is sent.
        let expired = Credentials::OAuth2 {
            access_token: "stale".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
        };
        let provider = Arc::new(FixedRefreshProvider {
            token: "freshly-refreshed-token".to_string(),
        });
        let credentials = Arc::new(crate::auth::RefreshingCredentials::new(
            expired,
            Some(provider),
        ));

        let backend =
            HttpBackend::new(format!("http://{addr}")).with_auth(HttpAuth::OAuth2 { credentials });

        let data = backend.get("obj").await.expect("get should succeed");
        assert_eq!(&data[..], b"payload");

        let request = server.await.expect("server task panicked");
        let auth_header = request
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
            .expect("request must carry an Authorization header");
        assert!(
            auth_header.contains("freshly-refreshed-token"),
            "expected the refreshed token in the Authorization header, got: {auth_header}"
        );
    }
}
