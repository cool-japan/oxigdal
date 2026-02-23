//! Cache strategies for different use cases.

use crate::error::{PwaError, Result};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, Response};

/// Cache strategy type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyType {
    /// Cache first, fall back to network
    CacheFirst,

    /// Network first, fall back to cache
    NetworkFirst,

    /// Cache only (no network)
    CacheOnly,

    /// Network only (no cache)
    NetworkOnly,

    /// Fastest response wins
    StaleWhileRevalidate,
}

impl StrategyType {
    /// Get strategy name as string.
    pub fn as_str(&self) -> &'static str {
        match self {
            StrategyType::CacheFirst => "cache-first",
            StrategyType::NetworkFirst => "network-first",
            StrategyType::CacheOnly => "cache-only",
            StrategyType::NetworkOnly => "network-only",
            StrategyType::StaleWhileRevalidate => "stale-while-revalidate",
        }
    }
}

/// Cache strategy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Strategy type
    pub strategy_type: StrategyType,

    /// Cache name to use
    pub cache_name: String,

    /// Network timeout in milliseconds
    pub network_timeout: Option<u32>,

    /// Cache expiration duration
    pub cache_expiration: Option<Duration>,

    /// Maximum cache size
    pub max_cache_size: Option<usize>,

    /// Maximum number of entries
    pub max_entries: Option<usize>,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            strategy_type: StrategyType::NetworkFirst,
            cache_name: "default-cache".to_string(),
            network_timeout: Some(5000),
            cache_expiration: Some(Duration::days(7)),
            max_cache_size: None,
            max_entries: Some(50),
        }
    }
}

/// Cache strategy implementation.
pub struct CacheStrategy {
    config: StrategyConfig,
}

impl CacheStrategy {
    /// Create a new cache strategy.
    pub fn new(config: StrategyConfig) -> Self {
        Self { config }
    }

    /// Create a cache-first strategy.
    pub fn cache_first(cache_name: impl Into<String>) -> Self {
        Self::new(StrategyConfig {
            strategy_type: StrategyType::CacheFirst,
            cache_name: cache_name.into(),
            ..Default::default()
        })
    }

    /// Create a network-first strategy.
    pub fn network_first(cache_name: impl Into<String>) -> Self {
        Self::new(StrategyConfig {
            strategy_type: StrategyType::NetworkFirst,
            cache_name: cache_name.into(),
            ..Default::default()
        })
    }

    /// Create a cache-only strategy.
    pub fn cache_only(cache_name: impl Into<String>) -> Self {
        Self::new(StrategyConfig {
            strategy_type: StrategyType::CacheOnly,
            cache_name: cache_name.into(),
            network_timeout: None,
            ..Default::default()
        })
    }

    /// Create a network-only strategy.
    pub fn network_only() -> Self {
        Self::new(StrategyConfig {
            strategy_type: StrategyType::NetworkOnly,
            cache_name: String::new(),
            network_timeout: None,
            cache_expiration: None,
            max_cache_size: None,
            max_entries: None,
        })
    }

    /// Create a stale-while-revalidate strategy.
    pub fn stale_while_revalidate(cache_name: impl Into<String>) -> Self {
        Self::new(StrategyConfig {
            strategy_type: StrategyType::StaleWhileRevalidate,
            cache_name: cache_name.into(),
            ..Default::default()
        })
    }

    /// Handle a fetch request using the configured strategy.
    pub async fn handle(&self, request: &Request) -> Result<Response> {
        match self.config.strategy_type {
            StrategyType::CacheFirst => self.cache_first_handler(request).await,
            StrategyType::NetworkFirst => self.network_first_handler(request).await,
            StrategyType::CacheOnly => self.cache_only_handler(request).await,
            StrategyType::NetworkOnly => self.network_only_handler(request).await,
            StrategyType::StaleWhileRevalidate => {
                self.stale_while_revalidate_handler(request).await
            }
        }
    }

    /// Cache-first handler: Try cache first, fall back to network.
    async fn cache_first_handler(&self, request: &Request) -> Result<Response> {
        // Try cache first
        if let Some(response) = self.match_cache(request).await? {
            if !self.is_expired(&response).await {
                return Ok(response);
            }
        }

        // Fall back to network
        let response = self.fetch_from_network(request).await?;
        self.cache_response(request, &response).await?;

        Ok(response)
    }

    /// Network-first handler: Try network first, fall back to cache.
    async fn network_first_handler(&self, request: &Request) -> Result<Response> {
        // Try network first
        match self.fetch_from_network(request).await {
            Ok(response) => {
                self.cache_response(request, &response).await?;
                Ok(response)
            }
            Err(_) => {
                // Fall back to cache
                self.match_cache(request)
                    .await?
                    .ok_or_else(|| PwaError::CacheRequestNotFound(request.url()))
            }
        }
    }

    /// Cache-only handler: Only use cache, never network.
    async fn cache_only_handler(&self, request: &Request) -> Result<Response> {
        self.match_cache(request)
            .await?
            .ok_or_else(|| PwaError::CacheRequestNotFound(request.url()))
    }

    /// Network-only handler: Only use network, never cache.
    async fn network_only_handler(&self, request: &Request) -> Result<Response> {
        self.fetch_from_network(request).await
    }

    /// Stale-while-revalidate handler: Return cache immediately, update in background.
    async fn stale_while_revalidate_handler(&self, request: &Request) -> Result<Response> {
        // Get from cache immediately if available
        let cached_response = self.match_cache(request).await?;

        // Revalidate in background (fire and forget)
        let cache_name_clone = self.config.cache_name.clone();

        let request_clone = match request.clone() {
            Ok(req) => req,
            Err(_) => return Self::fetch_with_timeout(request, self.config.network_timeout).await,
        };

        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(response) = Self::fetch_with_timeout(&request_clone, None).await {
                let _ = Self::put_in_cache(&cache_name_clone, &request_clone, &response).await;
            }
        });

        // Return cached version or fetch if not cached
        if let Some(response) = cached_response {
            Ok(response)
        } else {
            self.fetch_from_network(request).await
        }
    }

    /// Match a request in the cache.
    async fn match_cache(&self, request: &Request) -> Result<Option<Response>> {
        let cache = super::open_cache(&self.config.cache_name).await?;

        let promise = cache.match_with_request(request);

        let result = JsFuture::from(promise).await.map_err(|e| {
            PwaError::CacheOperation(format!("Cache match promise failed: {:?}", e))
        })?;

        if result.is_undefined() || result.is_null() {
            Ok(None)
        } else {
            let response = result
                .dyn_into::<Response>()
                .map_err(|_| PwaError::CacheOperation("Invalid response object".to_string()))?;
            Ok(Some(response))
        }
    }

    /// Cache a response.
    async fn cache_response(&self, request: &Request, response: &Response) -> Result<()> {
        // Only cache successful responses
        if !response.ok() {
            return Ok(());
        }

        Self::put_in_cache(&self.config.cache_name, request, response).await
    }

    /// Put a response in cache (static method for background tasks).
    async fn put_in_cache(cache_name: &str, request: &Request, response: &Response) -> Result<()> {
        let cache = super::open_cache(cache_name).await?;

        // Clone response to put in cache
        let response_clone = response
            .clone()
            .map_err(|e| PwaError::CacheOperation(format!("Failed to clone response: {:?}", e)))?;
        let promise = cache.put_with_request(request, &response_clone);

        JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::CacheOperation(format!("Cache put promise failed: {:?}", e)))?;

        Ok(())
    }

    /// Fetch from network with timeout.
    async fn fetch_from_network(&self, request: &Request) -> Result<Response> {
        Self::fetch_with_timeout(request, self.config.network_timeout).await
    }

    /// Fetch with optional timeout.
    async fn fetch_with_timeout(request: &Request, timeout_ms: Option<u32>) -> Result<Response> {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

        let promise = window.fetch_with_request(request);

        // Apply timeout if specified
        let result = if let Some(timeout) = timeout_ms {
            let timeout_promise = js_sys::Promise::new(&mut |_resolve, reject| {
                let window = match web_sys::window() {
                    Some(w) => w,
                    None => return,
                };
                let timeout_closure = wasm_bindgen::closure::Closure::once(move || {
                    reject
                        .call1(
                            &wasm_bindgen::JsValue::NULL,
                            &wasm_bindgen::JsValue::from_str("Network timeout"),
                        )
                        .ok();
                });

                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        timeout_closure.as_ref().unchecked_ref(),
                        timeout as i32,
                    )
                    .ok();

                timeout_closure.forget();
            });

            let race = js_sys::Promise::race(&js_sys::Array::of2(&promise, &timeout_promise));
            JsFuture::from(race).await
        } else {
            JsFuture::from(promise).await
        };

        let response = result
            .map_err(|e| PwaError::FetchFailed(format!("{:?}", e)))?
            .dyn_into::<Response>()
            .map_err(|_| PwaError::FetchFailed("Invalid response object".to_string()))?;

        Ok(response)
    }

    /// Check if a response is expired.
    async fn is_expired(&self, _response: &Response) -> bool {
        // Check cache-control headers and custom expiration
        if let Some(_expiration) = &self.config.cache_expiration {
            // In a real implementation, we would:
            // 1. Check Date header
            // 2. Check Cache-Control max-age
            // 3. Compare with config.cache_expiration
            // For now, return false (not expired)
            false
        } else {
            false
        }
    }

    /// Get the strategy configuration.
    pub fn config(&self) -> &StrategyConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_type_str() {
        assert_eq!(StrategyType::CacheFirst.as_str(), "cache-first");
        assert_eq!(StrategyType::NetworkFirst.as_str(), "network-first");
        assert_eq!(StrategyType::CacheOnly.as_str(), "cache-only");
        assert_eq!(StrategyType::NetworkOnly.as_str(), "network-only");
        assert_eq!(
            StrategyType::StaleWhileRevalidate.as_str(),
            "stale-while-revalidate"
        );
    }

    #[test]
    fn test_strategy_creation() {
        let strategy = CacheStrategy::cache_first("my-cache");
        assert_eq!(strategy.config.cache_name, "my-cache");
        assert_eq!(strategy.config.strategy_type, StrategyType::CacheFirst);

        let strategy = CacheStrategy::network_first("api-cache");
        assert_eq!(strategy.config.strategy_type, StrategyType::NetworkFirst);

        let strategy = CacheStrategy::network_only();
        assert_eq!(strategy.config.strategy_type, StrategyType::NetworkOnly);
    }

    #[test]
    fn test_default_config() {
        let config = StrategyConfig::default();
        assert_eq!(config.strategy_type, StrategyType::NetworkFirst);
        assert_eq!(config.cache_name, "default-cache");
        assert_eq!(config.network_timeout, Some(5000));
        assert_eq!(config.max_entries, Some(50));
    }
}
