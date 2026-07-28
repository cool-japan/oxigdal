//! Synchronization protocol implementation

#[cfg(any(test, feature = "test-utils"))]
use super::SyncStrategy;
use super::{SyncItem, SyncMetadata};
#[cfg(any(test, feature = "test-utils", feature = "http-sync"))]
use crate::error::EdgeError;
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
#[cfg(any(test, feature = "test-utils"))]
use std::collections::HashMap;

/// Sync protocol trait
#[async_trait]
pub trait SyncProtocol: Send + Sync {
    /// Push data to remote
    async fn push(&self, items: Vec<SyncItem>) -> Result<SyncMetadata>;

    /// Pull data from remote
    async fn pull(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> Result<Vec<SyncItem>>;

    /// Sync bidirectionally
    async fn sync(&self, local_items: Vec<SyncItem>) -> Result<SyncResult>;

    /// Check connectivity
    async fn is_connected(&self) -> bool;
}

/// Sync result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Items pushed to remote
    pub pushed: Vec<String>,
    /// Items pulled from remote
    pub pulled: Vec<SyncItem>,
    /// Sync metadata
    pub metadata: SyncMetadata,
    /// Conflicts detected
    pub conflicts: Vec<Conflict>,
}

impl SyncResult {
    /// Create new sync result
    pub fn new(metadata: SyncMetadata) -> Self {
        Self {
            pushed: Vec::new(),
            pulled: Vec::new(),
            metadata,
            conflicts: Vec::new(),
        }
    }

    /// Check if sync was successful
    pub fn is_successful(&self) -> bool {
        self.metadata.status.is_complete()
    }

    /// Get total items synced
    pub fn total_items(&self) -> usize {
        self.pushed.len() + self.pulled.len()
    }
}

/// Conflict between local and remote data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Item ID
    pub item_id: String,
    /// Local version
    pub local_version: u64,
    /// Remote version
    pub remote_version: u64,
    /// Conflict resolution strategy
    pub resolution: ConflictResolution,
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Use local version
    UseLocal,
    /// Use remote version
    UseRemote,
    /// Merge versions
    Merge,
    /// Manual resolution required
    Manual,
}

/// Mock sync protocol for testing only.
///
/// This is **not** wired into any production code path in this crate:
/// [`SyncManager::new`](super::manager::SyncManager::new) requires callers to
/// supply their own `Arc<dyn SyncProtocol>`, so a real deployment can never
/// silently end up talking to this in-process, non-persistent stand-in
/// instead of an actual sync backend. Only compiled for `cfg(test)` builds or
/// when the crate is built with the non-default `test-utils` feature (e.g.
/// for downstream integration tests that need a fake protocol).
#[cfg(any(test, feature = "test-utils"))]
pub struct MockSyncProtocol {
    storage: parking_lot::RwLock<HashMap<String, SyncItem>>,
    connected: parking_lot::RwLock<bool>,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for MockSyncProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl MockSyncProtocol {
    /// Create new mock protocol
    pub fn new() -> Self {
        Self {
            storage: parking_lot::RwLock::new(HashMap::new()),
            connected: parking_lot::RwLock::new(true),
        }
    }

    /// Set connection status
    pub fn set_connected(&self, connected: bool) {
        *self.connected.write() = connected;
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl SyncProtocol for MockSyncProtocol {
    async fn push(&self, items: Vec<SyncItem>) -> Result<SyncMetadata> {
        if !self.is_connected().await {
            return Err(EdgeError::network("Not connected"));
        }

        let mut storage = self.storage.write();
        let mut metadata = SyncMetadata::new(
            format!("sync-{}", chrono::Utc::now().timestamp()),
            SyncStrategy::Manual,
        );

        metadata.start();

        for item in items {
            storage.insert(item.id.clone(), item);
        }

        metadata.complete(storage.len(), 0);
        Ok(metadata)
    }

    async fn pull(&self, _since: Option<chrono::DateTime<chrono::Utc>>) -> Result<Vec<SyncItem>> {
        if !self.is_connected().await {
            return Err(EdgeError::network("Not connected"));
        }

        let storage = self.storage.read();
        Ok(storage.values().cloned().collect())
    }

    async fn sync(&self, local_items: Vec<SyncItem>) -> Result<SyncResult> {
        if !self.is_connected().await {
            return Err(EdgeError::network("Not connected"));
        }

        let mut metadata = SyncMetadata::new(
            format!("sync-{}", chrono::Utc::now().timestamp()),
            SyncStrategy::Manual,
        );

        metadata.start();

        // Push local items
        let mut storage = self.storage.write();
        let mut pushed = Vec::new();

        for item in &local_items {
            storage.insert(item.id.clone(), item.clone());
            pushed.push(item.id.clone());
        }

        // Pull remote items (simplified)
        let pulled: Vec<SyncItem> = storage
            .values()
            .filter(|item| !local_items.iter().any(|l| l.id == item.id))
            .cloned()
            .collect();

        metadata.complete(pushed.len() + pulled.len(), 0);

        Ok(SyncResult {
            pushed,
            pulled,
            metadata,
            conflicts: Vec::new(),
        })
    }

    async fn is_connected(&self) -> bool {
        *self.connected.read()
    }
}

/// Configuration for [`HttpSyncProtocol`].
#[cfg(feature = "http-sync")]
#[derive(Debug, Clone)]
pub struct HttpSyncConfig {
    /// Base URL of the remote sync backend, e.g.
    /// `https://sync.example.com/api/v1` (no trailing slash required).
    pub base_url: String,
    /// Optional bearer token sent as `Authorization: Bearer <token>` on
    /// every request.
    pub auth_token: Option<String>,
    /// Per-request timeout.
    pub timeout: std::time::Duration,
}

#[cfg(feature = "http-sync")]
impl HttpSyncConfig {
    /// Create a new configuration pointing at `base_url` with a 30-second
    /// default timeout and no authentication.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            auth_token: None,
            timeout: std::time::Duration::from_secs(30),
        }
    }

    /// Attach a bearer token to every request.
    #[must_use]
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Override the per-request timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Real HTTP/REST-backed [`SyncProtocol`] implementation for edge-to-cloud
/// synchronization.
///
/// Talks to a remote sync backend over three JSON endpoints relative to
/// [`HttpSyncConfig::base_url`]:
///
/// - `POST {base_url}/push` — body `Vec<`[`SyncItem`]`>`, response
///   [`SyncMetadata`]
/// - `GET {base_url}/pull[?since=<rfc3339>]` — response `Vec<`[`SyncItem`]`>`
/// - `POST {base_url}/sync` — body `Vec<`[`SyncItem`]`>`, response
///   [`SyncResult`]
/// - `GET {base_url}/health` — any 2xx response is treated as "connected"
///   for [`SyncProtocol::is_connected`]; any error (network failure or
///   non-2xx status) is treated as "not connected".
///
/// # Pure Rust Policy note
///
/// This type is gated behind the **non-default** `http-sync` feature.
/// `reqwest`'s `rustls` backend transitively pulls `aws-lc-rs`/`aws-lc-sys`
/// (C + assembly crypto) for its default `CryptoProvider`, so enabling this
/// feature intentionally steps outside the crate's pure-Rust default
/// closure (mirroring the same accepted trade-off already documented for
/// `oxigeo-cloud`'s `http`/`s3`/`azure-blob`/`gcs` features). Callers who
/// need a 100% pure-Rust HTTP stack must supply their own
/// [`SyncProtocol`] implementation built on an `oxitls`-backed client.
#[cfg(feature = "http-sync")]
pub struct HttpSyncProtocol {
    client: reqwest::Client,
    config: HttpSyncConfig,
}

#[cfg(feature = "http-sync")]
impl HttpSyncProtocol {
    /// Create a new HTTP sync protocol client.
    pub fn new(config: HttpSyncConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| EdgeError::network(format!("failed to build HTTP client: {e}")))?;

        Ok(Self { client, config })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.config.base_url.trim_end_matches('/'), path);
        let mut builder = self.client.request(method, url);
        if let Some(token) = &self.config.auth_token {
            builder = builder.bearer_auth(token);
        }
        builder
    }
}

#[cfg(feature = "http-sync")]
#[async_trait]
impl SyncProtocol for HttpSyncProtocol {
    async fn push(&self, items: Vec<SyncItem>) -> Result<SyncMetadata> {
        let response = self
            .request(reqwest::Method::POST, "/push")
            .json(&items)
            .send()
            .await
            .map_err(|e| EdgeError::network(format!("push request failed: {e}")))?;

        let response = response
            .error_for_status()
            .map_err(|e| EdgeError::network(format!("push request returned error status: {e}")))?;

        response
            .json::<SyncMetadata>()
            .await
            .map_err(|e| EdgeError::deserialization(format!("failed to parse push response: {e}")))
    }

    async fn pull(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> Result<Vec<SyncItem>> {
        let mut builder = self.request(reqwest::Method::GET, "/pull");
        if let Some(since) = since {
            builder = builder.query(&[("since", since.to_rfc3339())]);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| EdgeError::network(format!("pull request failed: {e}")))?;

        let response = response
            .error_for_status()
            .map_err(|e| EdgeError::network(format!("pull request returned error status: {e}")))?;

        response
            .json::<Vec<SyncItem>>()
            .await
            .map_err(|e| EdgeError::deserialization(format!("failed to parse pull response: {e}")))
    }

    async fn sync(&self, local_items: Vec<SyncItem>) -> Result<SyncResult> {
        let response = self
            .request(reqwest::Method::POST, "/sync")
            .json(&local_items)
            .send()
            .await
            .map_err(|e| EdgeError::network(format!("sync request failed: {e}")))?;

        let response = response
            .error_for_status()
            .map_err(|e| EdgeError::network(format!("sync request returned error status: {e}")))?;

        response
            .json::<SyncResult>()
            .await
            .map_err(|e| EdgeError::deserialization(format!("failed to parse sync response: {e}")))
    }

    async fn is_connected(&self) -> bool {
        self.request(reqwest::Method::GET, "/health")
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_protocol_push() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let metadata = protocol.push(vec![item]).await?;
        assert!(metadata.status.is_complete());

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_pull() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        protocol.push(vec![item.clone()]).await?;

        let items = protocol.pull(None).await?;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, item.id);

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_sync() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let result = protocol.sync(vec![item]).await?;
        assert!(result.is_successful());
        assert_eq!(result.pushed.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_connectivity() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        assert!(protocol.is_connected().await);

        protocol.set_connected(false);
        assert!(!protocol.is_connected().await);

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_offline() {
        let protocol = MockSyncProtocol::new();
        protocol.set_connected(false);

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let result = protocol.push(vec![item]).await;
        assert!(result.is_err());
    }
}

/// Tests for [`HttpSyncProtocol`] against a minimal hand-rolled HTTP/1.1
/// mock server (no external mocking crate dependency needed).
#[cfg(all(test, feature = "http-sync"))]
#[allow(clippy::expect_used)]
mod http_sync_tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawn a single-purpose HTTP/1.1 mock server that serves a fixed
    /// `(status, body)` response for each configured `path`, ignoring the
    /// method and any query string. Returns the server's base URL and the
    /// background task handle (dropping/aborting the handle stops the
    /// server).
    async fn spawn_mock_server(
        routes: StdHashMap<&'static str, (u16, String)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");

        let handle = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let routes = routes.clone();
                tokio::spawn(handle_connection(stream, routes));
            }
        });

        (base_url, handle)
    }

    async fn handle_connection(
        mut stream: tokio::net::TcpStream,
        routes: StdHashMap<&'static str, (u16, String)>,
    ) {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];

        // Read until the full header block has arrived.
        let header_end = loop {
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        };

        let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let request_line = header_text.lines().next().unwrap_or_default();
        let full_path = request_line.split_whitespace().nth(1).unwrap_or_default();
        let path = full_path.split('?').next().unwrap_or(full_path);

        let content_length: usize = header_text
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);

        // Drain any remaining request body so the client isn't left
        // waiting on a connection we're about to close.
        let mut remaining = content_length.saturating_sub(buf.len().saturating_sub(header_end));
        while remaining > 0 {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => remaining = remaining.saturating_sub(n),
            }
        }

        let (status, body) = routes
            .get(path)
            .cloned()
            .unwrap_or((404, "\"not found\"".to_string()));
        let reason = match status {
            200 => "OK",
            404 => "Not Found",
            _ => "Error",
        };

        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    }

    #[tokio::test]
    async fn test_http_sync_protocol_push_pull_sync_health() {
        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let mut completed_metadata = SyncMetadata::new("sync-1".to_string(), SyncStrategy::Manual);
        completed_metadata.complete(1, 3);

        let sync_result = SyncResult {
            pushed: vec![item.id.clone()],
            pulled: vec![],
            metadata: completed_metadata.clone(),
            conflicts: vec![],
        };

        let mut routes = StdHashMap::new();
        routes.insert(
            "/push",
            (
                200,
                serde_json::to_string(&completed_metadata).expect("serialize metadata"),
            ),
        );
        routes.insert(
            "/pull",
            (
                200,
                serde_json::to_string(&vec![item.clone()]).expect("serialize items"),
            ),
        );
        routes.insert(
            "/sync",
            (
                200,
                serde_json::to_string(&sync_result).expect("serialize result"),
            ),
        );
        routes.insert("/health", (200, "{}".to_string()));

        let (base_url, _handle) = spawn_mock_server(routes).await;

        let protocol =
            HttpSyncProtocol::new(HttpSyncConfig::new(base_url)).expect("build http client");

        assert!(protocol.is_connected().await);

        let pushed_metadata = protocol.push(vec![item.clone()]).await.expect("push");
        assert!(pushed_metadata.status.is_complete());

        let pulled = protocol.pull(None).await.expect("pull");
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0].id, item.id);

        let result = protocol.sync(vec![item.clone()]).await.expect("sync");
        assert!(result.is_successful());
        assert_eq!(result.pushed, vec![item.id.clone()]);
    }

    #[tokio::test]
    async fn test_http_sync_protocol_reports_disconnected_when_unreachable() {
        // Bind then immediately drop, to obtain a port number that nothing
        // is listening on.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let protocol = HttpSyncProtocol::new(
            HttpSyncConfig::new(format!("http://{addr}"))
                .with_timeout(std::time::Duration::from_millis(500)),
        )
        .expect("build http client");

        assert!(!protocol.is_connected().await);
    }

    #[tokio::test]
    async fn test_http_sync_protocol_push_surfaces_error_status() {
        let mut routes = StdHashMap::new();
        routes.insert("/push", (500, "\"internal error\"".to_string()));

        let (base_url, _handle) = spawn_mock_server(routes).await;
        let protocol =
            HttpSyncProtocol::new(HttpSyncConfig::new(base_url)).expect("build http client");

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);
        let result = protocol.push(vec![item]).await;

        assert!(result.is_err());
    }
}
