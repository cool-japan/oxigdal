//! Offline caching strategies for PWA.

pub mod geospatial;
pub mod storage;
pub mod strategies;

use crate::error::{PwaError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Cache, CacheStorage, Request, Response};

pub use geospatial::GeospatialCache;
pub use storage::CacheStorageManager;
pub use strategies::{CacheStrategy, StrategyType};

/// Get the cache storage API.
pub fn get_cache_storage() -> Result<CacheStorage> {
    let window = web_sys::window()
        .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

    let caches = window
        .caches()
        .map_err(|_| PwaError::CacheOperation("CacheStorage not available".to_string()))?;

    Ok(caches)
}

/// Open a cache by name.
pub async fn open_cache(name: &str) -> Result<Cache> {
    let caches = get_cache_storage()?;
    let promise = caches.open(name);

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| PwaError::CacheOperation(format!("Cache open failed: {:?}", e)))?;

    result
        .dyn_into::<Cache>()
        .map_err(|_| PwaError::CacheOperation("Invalid cache object".to_string()))
}

/// Delete a cache by name.
pub async fn delete_cache(name: &str) -> Result<bool> {
    let caches = get_cache_storage()?;
    let promise = caches.delete(name);

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| PwaError::CacheOperation(format!("Cache delete failed: {:?}", e)))?;

    result
        .as_bool()
        .ok_or_else(|| PwaError::CacheOperation("Invalid delete result".to_string()))
}

/// Check if a cache exists.
pub async fn has_cache(name: &str) -> Result<bool> {
    let caches = get_cache_storage()?;
    let promise = caches.has(name);

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| PwaError::CacheOperation(format!("Cache has failed: {:?}", e)))?;

    result
        .as_bool()
        .ok_or_else(|| PwaError::CacheOperation("Invalid has result".to_string()))
}

/// Get all cache names.
pub async fn get_cache_names() -> Result<Vec<String>> {
    let caches = get_cache_storage()?;
    let promise = caches.keys();

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| PwaError::CacheOperation(format!("Cache keys failed: {:?}", e)))?;

    let array = js_sys::Array::from(&result);
    let mut names = Vec::new();

    for i in 0..array.length() {
        if let Some(name) = array.get(i).as_string() {
            names.push(name);
        }
    }

    Ok(names)
}

/// Cache entry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntryMetadata {
    /// Cache name
    pub cache_name: String,

    /// Request URL
    pub url: String,

    /// Timestamp when cached
    pub cached_at: DateTime<Utc>,

    /// Expiration time
    pub expires_at: Option<DateTime<Utc>>,

    /// Size in bytes
    pub size: Option<usize>,

    /// Custom metadata
    pub custom: Option<serde_json::Value>,
}

/// Cache manager for managing cache operations.
pub struct CacheManager {
    cache_name: String,
}

impl CacheManager {
    /// Create a new cache manager.
    pub fn new(cache_name: impl Into<String>) -> Self {
        Self {
            cache_name: cache_name.into(),
        }
    }

    /// Open the cache.
    pub async fn open(&self) -> Result<Cache> {
        open_cache(&self.cache_name).await
    }

    /// Put a request/response pair into the cache.
    pub async fn put(&self, request: &Request, response: &Response) -> Result<()> {
        let cache = self.open().await?;
        let promise = cache.put_with_request(request, response);

        JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache put failed: {:?}", e)))?;

        Ok(())
    }

    /// Add a request to the cache (fetches and caches).
    pub async fn add(&self, request: &Request) -> Result<()> {
        let cache = self.open().await?;
        let promise = cache.add_with_request(request);

        JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache add failed: {:?}", e)))?;

        Ok(())
    }

    /// Add multiple requests to the cache.
    pub async fn add_all(&self, requests: &[Request]) -> Result<()> {
        let cache = self.open().await?;

        let array = js_sys::Array::new();
        for request in requests {
            array.push(request);
        }

        let promise = cache.add_all_with_request_sequence(&array);

        JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache add_all failed: {:?}", e)))?;

        Ok(())
    }

    /// Match a request in the cache.
    pub async fn match_request(&self, request: &Request) -> Result<Option<Response>> {
        let cache = self.open().await?;
        let promise = cache.match_with_request(request);

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache match failed: {:?}", e)))?;

        if result.is_undefined() || result.is_null() {
            Ok(None)
        } else {
            let response = result
                .dyn_into::<Response>()
                .map_err(|_| PwaError::CacheOperation("Invalid response object".to_string()))?;
            Ok(Some(response))
        }
    }

    /// Delete a request from the cache.
    pub async fn delete(&self, request: &Request) -> Result<bool> {
        let cache = self.open().await?;
        let promise = cache.delete_with_request(request);

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache delete failed: {:?}", e)))?;

        result
            .as_bool()
            .ok_or_else(|| PwaError::CacheOperation("Invalid delete result".to_string()))
    }

    /// Get all requests in the cache.
    pub async fn keys(&self) -> Result<Vec<Request>> {
        let cache = self.open().await?;
        let promise = cache.keys();

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache keys failed: {:?}", e)))?;

        let array = js_sys::Array::from(&result);
        let mut requests = Vec::new();

        for i in 0..array.length() {
            if let Ok(request) = array.get(i).dyn_into::<Request>() {
                requests.push(request);
            }
        }

        Ok(requests)
    }

    /// Clear all entries in the cache.
    pub async fn clear(&self) -> Result<()> {
        let requests = self.keys().await?;

        for request in requests {
            self.delete(&request).await?;
        }

        Ok(())
    }

    /// Delete the entire cache.
    pub async fn destroy(&self) -> Result<bool> {
        delete_cache(&self.cache_name).await
    }

    /// Get the cache name.
    pub fn name(&self) -> &str {
        &self.cache_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_manager_creation() {
        let manager = CacheManager::new("test-cache");
        assert_eq!(manager.name(), "test-cache");
    }

    #[test]
    fn test_cache_entry_metadata() {
        let metadata = CacheEntryMetadata {
            cache_name: "test".to_string(),
            url: "https://example.com".to_string(),
            cached_at: Utc::now(),
            expires_at: None,
            size: Some(1024),
            custom: None,
        };

        assert_eq!(metadata.cache_name, "test");
        assert_eq!(metadata.size, Some(1024));
    }
}
