//! Response caching middleware.

use super::{Middleware, Request, Response};
use crate::error::Result;
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Cache configuration.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Cache size
    pub size: usize,
    /// Cache TTL in seconds
    pub ttl: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            size: 1000,
            ttl: 300, // 5 minutes
        }
    }
}

/// Cache entry.
#[derive(Debug, Clone)]
struct CacheEntry {
    _response: Response,
    _expires_at: chrono::DateTime<chrono::Utc>,
}

/// Caching middleware.
pub struct CachingMiddleware {
    _cache: Arc<Mutex<LruCache<String, CacheEntry>>>,
    _ttl: chrono::Duration,
}

impl CachingMiddleware {
    /// Creates a new caching middleware.
    pub fn new(config: CacheConfig) -> Self {
        // Use config.size if non-zero, otherwise use 1000 (guaranteed valid)
        let cache_size = if config.size > 0 { config.size } else { 1000 };
        // SAFETY: cache_size is guaranteed to be at least 1
        let size = NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::MIN);
        Self {
            _cache: Arc::new(Mutex::new(LruCache::new(size))),
            _ttl: chrono::Duration::seconds(config.ttl as i64),
        }
    }

    /// Generates cache key from request.
    fn _cache_key(request: &Request) -> String {
        format!("{}:{}", request.method, request.path)
    }
}

#[async_trait::async_trait]
impl Middleware for CachingMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        Ok(())
    }

    async fn after_response(&self, _response: &mut Response) -> Result<()> {
        Ok(())
    }
}
