//! Rate limiting algorithms.
//!
//! This module provides various rate limiting algorithms including token bucket,
//! leaky bucket, fixed window, and sliding window.

use crate::error::Result;
use crate::rate_limit::Storage;
use async_trait::async_trait;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Rate limiting algorithm trait.
#[async_trait]
pub trait Algorithm: Send + Sync {
    /// Checks if request is allowed.
    async fn check(
        &self,
        storage: &impl Storage,
        key: &str,
        limit: u64,
        window: Duration,
    ) -> Result<bool>;

    /// Records a request.
    async fn record(&self, storage: &impl Storage, key: &str, window: Duration) -> Result<()>;
}

/// Token bucket algorithm.
///
/// Tokens are added to the bucket at a steady rate. Each request consumes a token.
/// If no tokens are available, the request is rate limited.
#[derive(Debug, Clone, Copy)]
pub struct TokenBucket;

#[async_trait]
impl Algorithm for TokenBucket {
    async fn check(
        &self,
        storage: &impl Storage,
        key: &str,
        limit: u64,
        window: Duration,
    ) -> Result<bool> {
        let token_key = format!("{key}:tokens");
        let last_refill_key = format!("{key}:last_refill");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Get current tokens and last refill time
        let current_tokens = storage.get(&token_key).await?.unwrap_or(limit);
        let last_refill = storage.get(&last_refill_key).await?.unwrap_or(now);

        // Calculate tokens to add based on elapsed time
        let elapsed = now.saturating_sub(last_refill);
        let refill_rate = limit as f64 / window.as_secs() as f64;
        let tokens_to_add = (elapsed as f64 * refill_rate) as u64;

        let new_tokens = (current_tokens + tokens_to_add).min(limit);

        Ok(new_tokens > 0)
    }

    async fn record(&self, storage: &impl Storage, key: &str, window: Duration) -> Result<()> {
        let token_key = format!("{key}:tokens");
        let last_refill_key = format!("{key}:last_refill");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Get current state
        let limit = storage.get(key).await?.unwrap_or(100); // Default limit
        let current_tokens = storage.get(&token_key).await?.unwrap_or(limit);
        let last_refill = storage.get(&last_refill_key).await?.unwrap_or(now);

        // Calculate refill
        let elapsed = now.saturating_sub(last_refill);
        let refill_rate = limit as f64 / window.as_secs() as f64;
        let tokens_to_add = (elapsed as f64 * refill_rate) as u64;

        let new_tokens = (current_tokens + tokens_to_add).min(limit);

        // Consume one token
        let remaining = new_tokens.saturating_sub(1);

        // Update storage
        storage.set(&token_key, remaining, Some(window)).await?;
        storage.set(&last_refill_key, now, Some(window)).await?;

        Ok(())
    }
}

/// Leaky bucket algorithm.
///
/// Requests leak out of the bucket at a constant rate. New requests fill the bucket.
/// If the bucket overflows, requests are rate limited.
#[derive(Debug, Clone, Copy)]
pub struct LeakyBucket;

#[async_trait]
impl Algorithm for LeakyBucket {
    async fn check(
        &self,
        storage: &impl Storage,
        key: &str,
        limit: u64,
        window: Duration,
    ) -> Result<bool> {
        let bucket_key = format!("{key}:bucket");
        let last_leak_key = format!("{key}:last_leak");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Get current bucket level and last leak time
        let current_level = storage.get(&bucket_key).await?.unwrap_or(0);
        let last_leak = storage.get(&last_leak_key).await?.unwrap_or(now);

        // Calculate leak
        let elapsed = now.saturating_sub(last_leak);
        let leak_rate = limit as f64 / window.as_secs() as f64;
        let leaked = (elapsed as f64 * leak_rate) as u64;

        let new_level = current_level.saturating_sub(leaked);

        // Check if bucket has space
        Ok(new_level < limit)
    }

    async fn record(&self, storage: &impl Storage, key: &str, window: Duration) -> Result<()> {
        let bucket_key = format!("{key}:bucket");
        let last_leak_key = format!("{key}:last_leak");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let limit = storage.get(key).await?.unwrap_or(100);
        let current_level = storage.get(&bucket_key).await?.unwrap_or(0);
        let last_leak = storage.get(&last_leak_key).await?.unwrap_or(now);

        // Calculate leak
        let elapsed = now.saturating_sub(last_leak);
        let leak_rate = limit as f64 / window.as_secs() as f64;
        let leaked = (elapsed as f64 * leak_rate) as u64;

        let new_level = current_level.saturating_sub(leaked);

        // Add new request
        let final_level = (new_level + 1).min(limit);

        // Update storage
        storage.set(&bucket_key, final_level, Some(window)).await?;
        storage.set(&last_leak_key, now, Some(window)).await?;

        Ok(())
    }
}

/// Fixed window algorithm.
///
/// Counts requests within fixed time windows. Simple but can allow bursts at window boundaries.
#[derive(Debug, Clone, Copy)]
pub struct FixedWindow;

#[async_trait]
impl Algorithm for FixedWindow {
    async fn check(
        &self,
        storage: &impl Storage,
        key: &str,
        limit: u64,
        _window: Duration,
    ) -> Result<bool> {
        let count = storage.get(key).await?.unwrap_or(0);
        Ok(count < limit)
    }

    async fn record(&self, storage: &impl Storage, key: &str, window: Duration) -> Result<()> {
        let count = storage.get(key).await?.unwrap_or(0);
        storage.set(key, count + 1, Some(window)).await
    }
}

/// Sliding window algorithm.
///
/// More accurate than fixed window, uses multiple time buckets to smooth out rate limiting.
#[derive(Debug, Clone, Copy)]
pub struct SlidingWindow {
    /// Number of sub-windows.
    pub buckets: usize,
}

impl Default for SlidingWindow {
    fn default() -> Self {
        Self { buckets: 10 }
    }
}

impl SlidingWindow {
    /// Creates a new sliding window algorithm with specified buckets.
    pub fn new(buckets: usize) -> Self {
        Self { buckets }
    }

    fn get_bucket_key(&self, key: &str, bucket: usize) -> String {
        format!("{key}:bucket:{bucket}")
    }

    fn current_bucket(&self, window: Duration) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let bucket_size = window.as_secs() / self.buckets as u64;
        ((now / bucket_size) % self.buckets as u64) as usize
    }
}

#[async_trait]
impl Algorithm for SlidingWindow {
    async fn check(
        &self,
        storage: &impl Storage,
        key: &str,
        limit: u64,
        _window: Duration,
    ) -> Result<bool> {
        let mut total = 0u64;

        // Sum all buckets
        for i in 0..self.buckets {
            let bucket_key = self.get_bucket_key(key, i);
            let count = storage.get(&bucket_key).await?.unwrap_or(0);
            total += count;
        }

        Ok(total < limit)
    }

    async fn record(&self, storage: &impl Storage, key: &str, window: Duration) -> Result<()> {
        let bucket = self.current_bucket(window);
        let bucket_key = self.get_bucket_key(key, bucket);

        let count = storage.get(&bucket_key).await?.unwrap_or(0);
        let bucket_window = window / self.buckets as u32;

        storage
            .set(&bucket_key, count + 1, Some(bucket_window))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limit::MemoryStorage;

    #[tokio::test]
    async fn test_fixed_window() {
        let storage = MemoryStorage::new();
        let algorithm = FixedWindow;

        let key = "test_user";
        let limit = 5;
        let window = Duration::from_secs(60);

        // First 5 requests should be allowed
        for _ in 0..5 {
            assert!(
                algorithm
                    .check(&storage, key, limit, window)
                    .await
                    .unwrap_or(false)
            );
            algorithm.record(&storage, key, window).await.ok();
        }

        // 6th request should be denied
        assert!(
            !algorithm
                .check(&storage, key, limit, window)
                .await
                .unwrap_or(true)
        );
    }

    #[tokio::test]
    async fn test_token_bucket() {
        let storage = MemoryStorage::new();
        let algorithm = TokenBucket;

        let key = "test_user";
        let limit = 10;
        let window = Duration::from_secs(60);

        // Should allow check
        assert!(
            algorithm
                .check(&storage, key, limit, window)
                .await
                .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn test_leaky_bucket() {
        let storage = MemoryStorage::new();
        let algorithm = LeakyBucket;

        let key = "test_user";
        let limit = 10;
        let window = Duration::from_secs(60);

        // Should allow check
        assert!(
            algorithm
                .check(&storage, key, limit, window)
                .await
                .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn test_sliding_window() {
        let storage = MemoryStorage::new();
        let algorithm = SlidingWindow::new(10);

        let key = "test_user";
        let limit = 5;
        let window = Duration::from_secs(60);

        // First 5 requests should be allowed
        for _ in 0..5 {
            assert!(
                algorithm
                    .check(&storage, key, limit, window)
                    .await
                    .unwrap_or(false)
            );
            algorithm.record(&storage, key, window).await.ok();
        }
    }
}
