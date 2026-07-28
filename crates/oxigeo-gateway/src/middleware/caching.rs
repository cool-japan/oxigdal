//! Response caching middleware.
//!
//! An in-memory, LRU-bounded response cache keyed by `"METHOD:path"`. Only successful
//! `GET` responses are stored, and only when the response does not opt out via a
//! `Cache-Control: no-store` directive. Entries carry a fixed time-to-live and are
//! evicted lazily the next time their key is looked up after expiry.
//!
//! The middleware itself cannot short-circuit a request from inside the in-house
//! [`Middleware`] chain, so the serving layer performs the actual cache short-circuit by
//! calling [`CachingMiddleware::lookup`] directly; the [`Middleware::after_response`]
//! implementation only *stores* eligible responses on the way out.

use super::{Middleware, Request, Response};
use crate::error::Result;
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// A single cached response together with the instant it was stored.
struct CacheEntry {
    /// The cached in-house response.
    response: Response,
    /// Monotonic instant at which the entry was inserted, used for TTL eviction.
    stored_at: Instant,
}

/// Caching middleware.
///
/// Stores a clone of eligible responses in a shared, LRU-bounded map. All fields are
/// cheap to clone (an [`Arc`] and a [`Duration`]), so the middleware can be shared across
/// the serving layer and the in-house chain simultaneously.
pub struct CachingMiddleware {
    /// LRU-bounded map from `"METHOD:path"` to a stored response entry.
    cache: Arc<Mutex<LruCache<String, CacheEntry>>>,
    /// Time-to-live applied to every stored entry.
    ttl: Duration,
}

impl CachingMiddleware {
    /// Creates a new caching middleware.
    pub fn new(config: CacheConfig) -> Self {
        // Use config.size if non-zero, otherwise use 1000 (guaranteed valid).
        let cache_size = if config.size > 0 { config.size } else { 1000 };
        // cache_size is guaranteed to be at least 1; fall back to the minimum otherwise.
        let size = NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::MIN);
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(size))),
            ttl: Duration::from_secs(config.ttl),
        }
    }

    /// Looks up a cached response for the serving layer's short-circuit.
    ///
    /// Returns a clone of the stored [`Response`] when a fresh entry exists for
    /// `"METHOD:path"`. Expired entries (older than the configured TTL) are evicted
    /// lazily during the lookup and reported as a miss.
    pub fn lookup(&self, method: &str, path: &str) -> Option<Response> {
        let key = Self::cache_key(method, path);
        let mut cache = self.cache.lock();
        // Peek first so a miss (or an eviction decision) does not perturb LRU ordering
        // before we know whether the entry is still fresh.
        let expired = match cache.peek(&key) {
            Some(entry) => entry.stored_at.elapsed() > self.ttl,
            None => return None,
        };
        if expired {
            cache.pop(&key);
            return None;
        }
        // Fresh hit: `get` marks the entry as most-recently-used and yields the clone.
        cache.get(&key).map(|entry| entry.response.clone())
    }

    /// Builds the cache key `"METHOD:path"` for a method/path pair.
    fn cache_key(method: &str, path: &str) -> String {
        format!("{method}:{path}")
    }
}

/// Returns `true` when the response opts out of caching via a `Cache-Control: no-store`
/// directive. The header name is matched case-insensitively and the directive is matched
/// case-insensitively within the header value.
fn is_no_store(response: &Response) -> bool {
    response.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("cache-control")
            && value.to_ascii_lowercase().contains("no-store")
    })
}

#[async_trait::async_trait]
impl Middleware for CachingMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        // The in-house `Middleware` trait cannot short-circuit a request; the serving
        // layer performs the cache short-circuit via `lookup`, so nothing happens here.
        Ok(())
    }

    async fn after_response(&self, request: &Request, response: &mut Response) -> Result<()> {
        if request.method == "GET" && response.status == 200 && !is_no_store(response) {
            let key = Self::cache_key(&request.method, &request.path);
            let entry = CacheEntry {
                response: response.clone(),
                stored_at: Instant::now(),
            };
            self.cache.lock().put(key, entry);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn request(method: &str, path: &str) -> Request {
        Request {
            method: method.to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    fn response(status: u16, body: &[u8]) -> Response {
        Response {
            status,
            headers: HashMap::new(),
            body: body.to_vec(),
        }
    }

    #[test]
    fn test_cache_key_format() {
        assert_eq!(
            CachingMiddleware::cache_key("GET", "/api/test"),
            "GET:/api/test"
        );
        assert_eq!(CachingMiddleware::cache_key("POST", "/x"), "POST:/x");
    }

    #[test]
    fn test_lookup_miss_on_empty_cache() {
        let mw = CachingMiddleware::new(CacheConfig::default());
        assert!(mw.lookup("GET", "/nothing").is_none());
    }

    #[tokio::test]
    async fn test_store_and_lookup_roundtrip() {
        let mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        let req = request("GET", "/data");
        let mut resp = response(200, b"payload");
        mw.after_response(&req, &mut resp).await.unwrap();

        let hit = mw.lookup("GET", "/data").expect("expected a cache hit");
        assert_eq!(hit.status, 200);
        assert_eq!(hit.body, b"payload".to_vec());
        // A different key must not resolve to the stored entry.
        assert!(mw.lookup("GET", "/other").is_none());
    }

    #[tokio::test]
    async fn test_ttl_expiry() {
        let mut mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        // Shrink the TTL to keep the test fast; the field is private but reachable in-file.
        mw.ttl = Duration::from_millis(20);

        let req = request("GET", "/ttl");
        let mut resp = response(200, b"soon-gone");
        mw.after_response(&req, &mut resp).await.unwrap();
        assert!(mw.lookup("GET", "/ttl").is_some());

        std::thread::sleep(Duration::from_millis(40));
        // Lazy eviction: the expired entry is dropped and reported as a miss.
        assert!(mw.lookup("GET", "/ttl").is_none());
        // A second lookup still misses (the entry is gone, not merely stale).
        assert!(mw.lookup("GET", "/ttl").is_none());
    }

    #[tokio::test]
    async fn test_no_store_not_cached() {
        let mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        let req = request("GET", "/private");
        let mut resp = response(200, b"secret");
        resp.headers
            .insert("Cache-Control".to_string(), "no-store".to_string());
        mw.after_response(&req, &mut resp).await.unwrap();
        assert!(mw.lookup("GET", "/private").is_none());
    }

    #[tokio::test]
    async fn test_no_store_matched_case_insensitively() {
        let mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        let req = request("GET", "/private2");
        let mut resp = response(200, b"secret");
        // Lower-case header name, mixed-case directive amongst other directives.
        resp.headers.insert(
            "cache-control".to_string(),
            "max-age=60, No-Store".to_string(),
        );
        mw.after_response(&req, &mut resp).await.unwrap();
        assert!(mw.lookup("GET", "/private2").is_none());
    }

    #[tokio::test]
    async fn test_no_cache_directive_is_still_cacheable() {
        // `no-cache` is distinct from `no-store`; only the latter blocks storage.
        let mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        let req = request("GET", "/nc");
        let mut resp = response(200, b"body");
        resp.headers
            .insert("Cache-Control".to_string(), "no-cache".to_string());
        mw.after_response(&req, &mut resp).await.unwrap();
        assert!(mw.lookup("GET", "/nc").is_some());
    }

    #[tokio::test]
    async fn test_non_get_not_stored() {
        let mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        let req = request("POST", "/submit");
        let mut resp = response(200, b"ok");
        mw.after_response(&req, &mut resp).await.unwrap();
        // Stored under neither the POST key nor a GET key.
        assert!(mw.lookup("POST", "/submit").is_none());
        assert!(mw.lookup("GET", "/submit").is_none());
    }

    #[tokio::test]
    async fn test_non_200_not_stored() {
        let mw = CachingMiddleware::new(CacheConfig { size: 16, ttl: 300 });
        let req = request("GET", "/missing");
        let mut resp = response(404, b"not found");
        mw.after_response(&req, &mut resp).await.unwrap();
        assert!(mw.lookup("GET", "/missing").is_none());
    }

    #[tokio::test]
    async fn test_capacity_eviction() {
        // Capacity of two: inserting a third eligible response evicts the oldest.
        let mw = CachingMiddleware::new(CacheConfig { size: 2, ttl: 300 });
        for path in ["/a", "/b", "/c"] {
            let req = request("GET", path);
            let mut resp = response(200, path.as_bytes());
            mw.after_response(&req, &mut resp).await.unwrap();
        }
        // `/a` was least-recently-used and is evicted; `/b` and `/c` remain.
        assert!(mw.lookup("GET", "/a").is_none());
        assert!(mw.lookup("GET", "/b").is_some());
        assert!(mw.lookup("GET", "/c").is_some());
    }

    #[test]
    fn test_zero_size_config_falls_back() {
        // A zero size must not panic (NonZeroUsize fallback keeps the cache usable).
        let mw = CachingMiddleware::new(CacheConfig { size: 0, ttl: 300 });
        assert!(mw.lookup("GET", "/x").is_none());
    }
}
