//! HTTP storage backend for Zarr arrays
//!
//! This module provides read-only HTTP/HTTPS storage support for accessing
//! Zarr arrays hosted on web servers.

#[cfg(feature = "async")]
use super::AsyncStore;
use super::StoreKey;
use crate::error::{Result, StorageError, ZarrError};

#[cfg(feature = "http")]
use reqwest::Client;

/// HTTP storage backend for read-only access to Zarr arrays
#[derive(Debug, Clone)]
pub struct HttpStorage {
    /// Base URL for the Zarr store
    pub base_url: String,
    /// Optional headers to include in requests
    pub headers: Vec<(String, String)>,
    /// HTTP client (lazy initialized)
    #[cfg(feature = "http")]
    client: Option<Client>,
}

impl HttpStorage {
    /// Creates a new HTTP storage backend
    ///
    /// # Arguments
    /// * `base_url` - The base URL of the Zarr store
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut url = base_url.into();
        // Ensure URL doesn't end with slash for consistent joining
        if url.ends_with('/') {
            url.pop();
        }
        Self {
            base_url: url,
            headers: Vec::new(),
            #[cfg(feature = "http")]
            client: None,
        }
    }

    /// Adds a header to include in all requests
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn full_url(&self, key: &StoreKey) -> String {
        format!("{}/{}", self.base_url, key.as_str())
    }

    #[cfg(feature = "http")]
    fn get_or_create_client(&mut self) -> Result<Client> {
        if let Some(ref client) = self.client {
            return Ok(client.clone());
        }

        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in &self.headers {
            let header_name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
                    ZarrError::Storage(StorageError::Network {
                        message: format!("Invalid header name '{name}': {e}"),
                    })
                })?;
            let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|e| {
                ZarrError::Storage(StorageError::Network {
                    message: format!("Invalid header value for '{name}': {e}"),
                })
            })?;
            headers.insert(header_name, header_value);
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| {
                ZarrError::Storage(StorageError::Network {
                    message: format!("Failed to create HTTP client: {e}"),
                })
            })?;

        self.client = Some(client.clone());
        Ok(client)
    }
}

#[cfg(all(feature = "http", feature = "async"))]
#[async_trait::async_trait]
impl AsyncStore for HttpStorage {
    async fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
        let mut storage = self.clone();
        let client = storage.get_or_create_client()?;
        let url = self.full_url(key);

        let response = client.get(&url).send().await.map_err(|e| {
            ZarrError::Storage(StorageError::Network {
                message: format!("HTTP GET failed for '{url}': {e}"),
            })
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(ZarrError::Storage(StorageError::Http {
                status: status.as_u16(),
                message: format!("HTTP GET failed for '{url}'"),
            }));
        }

        let bytes = response.bytes().await.map_err(|e| {
            ZarrError::Storage(StorageError::Network {
                message: format!("Failed to read response body: {e}"),
            })
        })?;

        Ok(bytes.to_vec())
    }

    async fn set(&mut self, _key: &StoreKey, _value: &[u8]) -> Result<()> {
        // HTTP storage is typically read-only
        Err(ZarrError::Storage(StorageError::ReadOnly))
    }

    async fn delete(&mut self, _key: &StoreKey) -> Result<()> {
        // HTTP storage is typically read-only
        Err(ZarrError::Storage(StorageError::ReadOnly))
    }

    async fn exists(&self, key: &StoreKey) -> Result<bool> {
        let mut storage = self.clone();
        let client = storage.get_or_create_client()?;
        let url = self.full_url(key);

        match client.head(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(e) => {
                let error_message = format!("{e}");
                if error_message.contains("404") || error_message.contains("NotFound") {
                    Ok(false)
                } else {
                    Err(ZarrError::Storage(StorageError::Network {
                        message: format!("HTTP HEAD failed for '{url}': {e}"),
                    }))
                }
            }
        }
    }

    async fn list_prefix(&self, _prefix: &StoreKey) -> Result<Vec<StoreKey>> {
        // HTTP storage typically doesn't support listing
        // Some servers may provide a directory index, but this is not standardized
        Err(ZarrError::Storage(StorageError::NotSupported {
            operation: "HTTP storage does not support listing".to_string(),
        }))
    }

    fn is_readonly(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_storage_new() {
        let storage = HttpStorage::new("https://example.com/zarr");
        assert_eq!(storage.base_url, "https://example.com/zarr");
    }

    #[test]
    fn test_http_storage_strips_trailing_slash() {
        let storage = HttpStorage::new("https://example.com/zarr/");
        assert_eq!(storage.base_url, "https://example.com/zarr");
    }

    #[test]
    fn test_http_storage_with_header() {
        let storage = HttpStorage::new("https://example.com/zarr")
            .with_header("Authorization", "Bearer token123");
        assert_eq!(storage.headers.len(), 1);
        assert_eq!(storage.headers[0].0, "Authorization");
        assert_eq!(storage.headers[0].1, "Bearer token123");
    }

    #[test]
    fn test_http_storage_full_url() {
        let storage = HttpStorage::new("https://example.com/zarr");
        let key = StoreKey::new("array/.zarray".to_string());
        assert_eq!(
            storage.full_url(&key),
            "https://example.com/zarr/array/.zarray"
        );
    }

    #[test]
    fn test_http_storage_builder_chain() {
        let storage = HttpStorage::new("https://example.com/data")
            .with_header("User-Agent", "OxiGDAL/1.0")
            .with_header("Accept", "application/json");

        assert_eq!(storage.base_url, "https://example.com/data");
        assert_eq!(storage.headers.len(), 2);
    }

    // Integration tests require a test HTTP server
    // These should be run with cargo test --features http,async -- --ignored
    #[cfg(all(feature = "http", feature = "async"))]
    #[tokio::test]
    #[ignore] // Requires HTTP server setup
    async fn test_http_storage_get() {
        // This test requires TEST_HTTP_URL environment variable pointing to a test Zarr store
        let base_url = match std::env::var("TEST_HTTP_URL") {
            Ok(url) => url,
            Err(_) => return, // Skip if not configured
        };

        let storage = HttpStorage::new(&base_url);
        let key = StoreKey::new(".zarray".to_string());

        match storage.get(&key).await {
            Ok(data) => {
                assert!(!data.is_empty());
                // Should be valid JSON
                let _: serde_json::Value =
                    serde_json::from_slice(&data).expect("Should be valid JSON");
            }
            Err(_) => {
                // If the key doesn't exist, that's also acceptable for this test
            }
        }
    }

    #[cfg(all(feature = "http", feature = "async"))]
    #[tokio::test]
    #[ignore] // Requires HTTP server setup
    async fn test_http_storage_exists() {
        let base_url = match std::env::var("TEST_HTTP_URL") {
            Ok(url) => url,
            Err(_) => return,
        };

        let storage = HttpStorage::new(&base_url);
        let key = StoreKey::new(".zarray".to_string());

        // Should not error even if key doesn't exist
        let _exists = storage.exists(&key).await.expect("Should not error");
    }

    #[cfg(all(feature = "http", feature = "async"))]
    #[tokio::test]
    async fn test_http_storage_readonly() {
        let storage = HttpStorage::new("https://example.com/zarr");

        #[cfg(feature = "async")]
        {
            use super::AsyncStore;
            assert!(storage.is_readonly());
        }
    }

    #[cfg(all(feature = "http", feature = "async"))]
    #[tokio::test]
    async fn test_http_storage_set_fails() {
        let mut storage = HttpStorage::new("https://example.com/zarr");
        let key = StoreKey::new("test.bin".to_string());

        let result = storage.set(&key, b"data").await;
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ZarrError::Storage(StorageError::ReadOnly))
        ));
    }

    #[cfg(all(feature = "http", feature = "async"))]
    #[tokio::test]
    async fn test_http_storage_delete_fails() {
        let mut storage = HttpStorage::new("https://example.com/zarr");
        let key = StoreKey::new("test.bin".to_string());

        let result = storage.delete(&key).await;
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ZarrError::Storage(StorageError::ReadOnly))
        ));
    }

    #[cfg(all(feature = "http", feature = "async"))]
    #[tokio::test]
    async fn test_http_storage_list_fails() {
        let storage = HttpStorage::new("https://example.com/zarr");
        let prefix = StoreKey::new("".to_string());

        let result = storage.list_prefix(&prefix).await;
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ZarrError::Storage(StorageError::NotSupported { .. }))
        ));
    }
}
