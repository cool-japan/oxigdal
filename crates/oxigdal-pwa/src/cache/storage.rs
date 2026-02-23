//! Cache storage management and quota handling.

use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::StorageEstimate;

/// Storage estimate information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    /// Current usage in bytes
    pub usage: Option<u64>,

    /// Total quota in bytes
    pub quota: Option<u64>,

    /// Percentage used (0.0 to 1.0)
    pub usage_percentage: Option<f64>,
}

impl StorageInfo {
    /// Create storage info from web_sys::StorageEstimate.
    pub fn from_estimate(estimate: &StorageEstimate) -> Self {
        // Use js_sys::Reflect to get values from StorageEstimate
        let obj: &js_sys::Object = estimate.as_ref();

        let usage = js_sys::Reflect::get(obj, &"usage".into())
            .ok()
            .and_then(|v| v.as_f64())
            .map(|v| v as u64);

        let quota = js_sys::Reflect::get(obj, &"quota".into())
            .ok()
            .and_then(|v| v.as_f64())
            .map(|v| v as u64);

        let usage_percentage = if let (Some(u), Some(q)) = (usage, quota) {
            if q > 0 {
                Some(u as f64 / q as f64)
            } else {
                None
            }
        } else {
            None
        };

        Self {
            usage,
            quota,
            usage_percentage,
        }
    }

    /// Get available space in bytes.
    pub fn available(&self) -> Option<u64> {
        if let (Some(quota), Some(usage)) = (self.quota, self.usage) {
            Some(quota.saturating_sub(usage))
        } else {
            None
        }
    }

    /// Check if storage is nearly full (>90%).
    pub fn is_nearly_full(&self) -> bool {
        self.usage_percentage.map(|p| p > 0.9).unwrap_or(false)
    }

    /// Check if storage has enough space for a given size.
    pub fn has_space_for(&self, size: u64) -> bool {
        self.available().map(|a| a >= size).unwrap_or(false)
    }
}

/// Cache storage manager for managing storage quota and cleanup.
pub struct CacheStorageManager;

impl CacheStorageManager {
    /// Estimate storage usage and quota.
    pub async fn estimate() -> Result<StorageInfo> {
        let navigator = Self::get_navigator()?;
        let storage = navigator.storage();

        let promise = storage.estimate().map_err(|e| {
            PwaError::StorageEstimateFailed(format!("Estimate call failed: {:?}", e))
        })?;

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::StorageEstimateFailed(format!("Estimate failed: {:?}", e)))?;

        let estimate = result
            .dyn_into::<StorageEstimate>()
            .map_err(|_| PwaError::StorageEstimateFailed("Invalid estimate object".to_string()))?;

        Ok(StorageInfo::from_estimate(&estimate))
    }

    /// Check if persistent storage is available.
    pub async fn is_persistent() -> Result<bool> {
        let navigator = Self::get_navigator()?;
        let storage = navigator.storage();

        let promise = storage.persisted().map_err(|e| {
            PwaError::StorageEstimateFailed(format!("Persisted call failed: {:?}", e))
        })?;
        let result = JsFuture::from(promise).await.map_err(|e| {
            PwaError::StorageEstimateFailed(format!("Persisted check failed: {:?}", e))
        })?;

        result
            .as_bool()
            .ok_or_else(|| PwaError::StorageEstimateFailed("Invalid persisted result".to_string()))
    }

    /// Request persistent storage.
    pub async fn request_persistent() -> Result<bool> {
        let navigator = Self::get_navigator()?;
        let storage = navigator.storage();

        let promise = storage.persist().map_err(|e| {
            PwaError::StorageEstimateFailed(format!("Persist call failed: {:?}", e))
        })?;

        let result = JsFuture::from(promise).await.map_err(|e| {
            PwaError::StorageEstimateFailed(format!("Persist request failed: {:?}", e))
        })?;

        result
            .as_bool()
            .ok_or_else(|| PwaError::StorageEstimateFailed("Invalid persist result".to_string()))
    }

    /// Clean up old caches to free space.
    pub async fn cleanup_old_caches(keep_names: &[String]) -> Result<u64> {
        let cache_names = super::get_cache_names().await?;
        let mut freed_bytes = 0u64;

        for name in cache_names {
            if !keep_names.contains(&name) {
                // Estimate cache size before deleting
                if let Ok(estimate) = Self::estimate_cache_size(&name).await {
                    freed_bytes += estimate;
                }

                super::delete_cache(&name).await?;
            }
        }

        Ok(freed_bytes)
    }

    /// Estimate the size of a specific cache.
    pub async fn estimate_cache_size(cache_name: &str) -> Result<u64> {
        let cache = super::open_cache(cache_name).await?;
        let keys = cache.keys();

        let requests = JsFuture::from(keys)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Keys promise failed: {:?}", e)))?;

        let array = js_sys::Array::from(&requests);
        let mut total_size = 0u64;

        for i in 0..array.length() {
            if let Ok(request) = array.get(i).dyn_into::<web_sys::Request>() {
                // Match the request to get response
                if let Ok(Some(response)) = Self::match_request_in_cache(&cache, &request).await {
                    if let Ok(Some(size)) = Self::estimate_response_size(&response).await {
                        total_size += size;
                    }
                }
            }
        }

        Ok(total_size)
    }

    /// Match a request in a cache.
    async fn match_request_in_cache(
        cache: &web_sys::Cache,
        request: &web_sys::Request,
    ) -> Result<Option<web_sys::Response>> {
        let promise = cache.match_with_request(request);

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Match promise failed: {:?}", e)))?;

        if result.is_undefined() || result.is_null() {
            Ok(None)
        } else {
            let response = result
                .dyn_into::<web_sys::Response>()
                .map_err(|_| PwaError::CacheOperation("Invalid response".to_string()))?;
            Ok(Some(response))
        }
    }

    /// Estimate response size from Content-Length header or by reading the body.
    async fn estimate_response_size(response: &web_sys::Response) -> Result<Option<u64>> {
        // Try to get Content-Length header
        let headers = response.headers();
        if let Ok(Some(length)) = headers.get("content-length") {
            if let Ok(size) = length.parse::<u64>() {
                return Ok(Some(size));
            }
        }

        // Clone and read body to estimate size

        // Note: Reading the body to estimate size is not implemented
        // This would require ReadableStream support which may not be available
        // in all web-sys versions. For now, we return None if Content-Length
        // header is not present.

        Ok(None)
    }

    /// Get the navigator object.
    fn get_navigator() -> Result<web_sys::Navigator> {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;
        Ok(window.navigator())
    }
}

/// Cache eviction policy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Least Recently Used
    Lru,

    /// First In First Out
    Fifo,

    /// Least Frequently Used
    Lfu,

    /// Expire oldest entries first
    OldestFirst,
}

/// Cache cleanup configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// Maximum cache size in bytes
    pub max_size: Option<u64>,

    /// Maximum number of entries
    pub max_entries: Option<usize>,

    /// Eviction policy to use
    pub eviction_policy: EvictionPolicy,

    /// Clean up when storage usage exceeds this percentage
    pub cleanup_threshold: f64,

    /// Target usage percentage after cleanup
    pub target_usage: f64,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            max_size: Some(100 * 1024 * 1024), // 100 MB
            max_entries: Some(100),
            eviction_policy: EvictionPolicy::Lru,
            cleanup_threshold: 0.9,
            target_usage: 0.7,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_info() {
        let info = StorageInfo {
            usage: Some(50_000_000),
            quota: Some(100_000_000),
            usage_percentage: Some(0.5),
        };

        assert_eq!(info.available(), Some(50_000_000));
        assert!(!info.is_nearly_full());
        assert!(info.has_space_for(10_000_000));
        assert!(!info.has_space_for(60_000_000));
    }

    #[test]
    fn test_storage_info_nearly_full() {
        let info = StorageInfo {
            usage: Some(95_000_000),
            quota: Some(100_000_000),
            usage_percentage: Some(0.95),
        };

        assert!(info.is_nearly_full());
    }

    #[test]
    fn test_cleanup_config_default() {
        let config = CleanupConfig::default();
        assert_eq!(config.max_size, Some(100 * 1024 * 1024));
        assert_eq!(config.max_entries, Some(100));
        assert_eq!(config.cleanup_threshold, 0.9);
    }
}
