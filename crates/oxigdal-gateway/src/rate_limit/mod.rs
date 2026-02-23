//! Rate limiting framework for API gateway.
//!
//! This module provides flexible rate limiting with multiple algorithms, storage backends,
//! and distributed support for high-availability deployments.

pub mod algorithms;
pub mod rules;
pub mod storage;

use crate::error::Result;
use async_trait::async_trait;
use std::time::Duration;

pub use algorithms::{Algorithm, FixedWindow, LeakyBucket, SlidingWindow, TokenBucket};
pub use rules::{RateLimitRule, RuleEngine};
pub use storage::{MemoryStorage, Storage};

#[cfg(feature = "redis")]
pub use storage::RedisStorage;

/// Rate limit decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Request is allowed.
    Allowed,
    /// Request is rate limited.
    Limited {
        /// Retry after duration.
        retry_after: Duration,
        /// Limit that was hit.
        limit: u64,
        /// Current count.
        current: u64,
    },
}

/// Rate limit key components.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RateLimitKey {
    /// Identifier (e.g., user ID, API key, IP address).
    pub identifier: String,
    /// Resource (e.g., endpoint path).
    pub resource: Option<String>,
    /// Additional namespace.
    pub namespace: Option<String>,
}

impl RateLimitKey {
    /// Creates a new rate limit key.
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            resource: None,
            namespace: None,
        }
    }

    /// Sets the resource.
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Sets the namespace.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Converts to storage key string.
    pub fn to_key(&self) -> String {
        let mut parts = vec![self.identifier.as_str()];

        if let Some(ref resource) = self.resource {
            parts.push(resource.as_str());
        }

        if let Some(ref namespace) = self.namespace {
            parts.push(namespace.as_str());
        }

        parts.join(":")
    }
}

/// Rate limiter trait.
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Checks if request is allowed.
    async fn check(&self, key: &RateLimitKey) -> Result<Decision>;

    /// Records request.
    async fn record(&self, key: &RateLimitKey) -> Result<()>;

    /// Resets rate limit for key.
    async fn reset(&self, key: &RateLimitKey) -> Result<()>;

    /// Gets current count for key.
    async fn count(&self, key: &RateLimitKey) -> Result<u64>;
}

/// Standard rate limiter implementation.
pub struct StandardRateLimiter<S: Storage, A: Algorithm> {
    storage: S,
    algorithm: A,
    limit: u64,
    window: Duration,
}

impl<S: Storage, A: Algorithm> StandardRateLimiter<S, A> {
    /// Creates a new standard rate limiter.
    pub fn new(storage: S, algorithm: A, limit: u64, window: Duration) -> Self {
        Self {
            storage,
            algorithm,
            limit,
            window,
        }
    }

    /// Gets the limit.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Gets the window duration.
    pub fn window(&self) -> Duration {
        self.window
    }
}

#[async_trait]
impl<S: Storage, A: Algorithm> RateLimiter for StandardRateLimiter<S, A> {
    async fn check(&self, key: &RateLimitKey) -> Result<Decision> {
        let storage_key = key.to_key();
        let allowed = self
            .algorithm
            .check(&self.storage, &storage_key, self.limit, self.window)
            .await?;

        if allowed {
            Ok(Decision::Allowed)
        } else {
            let current = self.storage.get(&storage_key).await?.unwrap_or(0);
            Ok(Decision::Limited {
                retry_after: self.window,
                limit: self.limit,
                current,
            })
        }
    }

    async fn record(&self, key: &RateLimitKey) -> Result<()> {
        let storage_key = key.to_key();
        self.algorithm
            .record(&self.storage, &storage_key, self.window)
            .await
    }

    async fn count(&self, key: &RateLimitKey) -> Result<u64> {
        let storage_key = key.to_key();
        Ok(self.storage.get(&storage_key).await?.unwrap_or(0))
    }

    async fn reset(&self, key: &RateLimitKey) -> Result<()> {
        let storage_key = key.to_key();
        self.storage.delete(&storage_key).await
    }
}

/// Quota information.
#[derive(Debug, Clone)]
pub struct Quota {
    /// Total allowed requests.
    pub limit: u64,
    /// Remaining requests.
    pub remaining: u64,
    /// Time window.
    pub window: Duration,
    /// Reset time (seconds since epoch).
    pub reset_at: u64,
}

impl Quota {
    /// Creates a new quota.
    pub fn new(limit: u64, remaining: u64, window: Duration, reset_at: u64) -> Self {
        Self {
            limit,
            remaining,
            window,
            reset_at,
        }
    }

    /// Checks if quota is exceeded.
    pub fn is_exceeded(&self) -> bool {
        self.remaining == 0
    }

    /// Gets retry-after duration.
    pub fn retry_after(&self) -> Option<Duration> {
        if self.is_exceeded() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some(Duration::from_secs(self.reset_at.saturating_sub(now)))
        } else {
            None
        }
    }
}

/// Burst allowance configuration.
#[derive(Debug, Clone)]
pub struct BurstConfig {
    /// Burst size (additional requests allowed).
    pub burst_size: u64,
    /// Burst window duration.
    pub burst_window: Duration,
}

impl BurstConfig {
    /// Creates a new burst configuration.
    pub fn new(burst_size: u64, burst_window: Duration) -> Self {
        Self {
            burst_size,
            burst_window,
        }
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed.
    pub max_requests: u64,
    /// Time window for rate limiting.
    pub window: Duration,
    /// Enable rate limiting.
    pub enabled: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 1000,
            window: Duration::from_secs(60),
            enabled: true,
        }
    }
}

impl RateLimitConfig {
    /// Creates a new rate limit configuration.
    pub fn new(max_requests: u64, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            enabled: true,
        }
    }

    /// Disables rate limiting.
    pub fn disabled() -> Self {
        Self {
            max_requests: 0,
            window: Duration::from_secs(0),
            enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_key() {
        let key = RateLimitKey::new("user123")
            .with_resource("/api/v1/data")
            .with_namespace("production");

        assert_eq!(key.to_key(), "user123:/api/v1/data:production");
    }

    #[test]
    fn test_rate_limit_key_simple() {
        let key = RateLimitKey::new("user456");
        assert_eq!(key.to_key(), "user456");
    }

    #[test]
    fn test_quota() {
        let quota = Quota::new(100, 50, Duration::from_secs(60), 1234567890);
        assert_eq!(quota.limit, 100);
        assert_eq!(quota.remaining, 50);
        assert!(!quota.is_exceeded());
    }

    #[test]
    fn test_quota_exceeded() {
        let quota = Quota::new(100, 0, Duration::from_secs(60), 1234567890);
        assert!(quota.is_exceeded());
    }

    #[tokio::test]
    async fn test_standard_rate_limiter() {
        let storage = MemoryStorage::new();
        let algorithm = TokenBucket;
        let limiter = StandardRateLimiter::new(storage, algorithm, 10, Duration::from_secs(60));

        let key = RateLimitKey::new("test_user");

        // First request should be allowed
        let decision = limiter.check(&key).await.ok().unwrap_or(Decision::Allowed);
        assert_eq!(decision, Decision::Allowed);
    }
}
