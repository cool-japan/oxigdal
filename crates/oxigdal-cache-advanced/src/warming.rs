//! Cache warming strategies
//!
//! Provides intelligent cache warming:
//! - Preload strategies (sequential, random, pattern-based)
//! - Background warming (low priority)
//! - Warm-up on cluster restart
//! - Critical data prioritization
//! - Warming progress tracking

use crate::error::{CacheError, Result};
use crate::multi_tier::{CacheKey, CacheValue, MultiTierCache};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cache warming strategy trait
#[async_trait]
pub trait WarmingStrategy: Send + Sync {
    /// Get next batch of keys to warm
    async fn next_batch(&mut self, batch_size: usize) -> Result<Vec<CacheKey>>;

    /// Check if warming is complete
    async fn is_complete(&self) -> bool;

    /// Get progress (0.0 - 1.0)
    async fn progress(&self) -> f64;

    /// Reset strategy
    async fn reset(&mut self);
}

/// Sequential warming strategy
/// Warms cache by loading keys in sequential order
pub struct SequentialWarming {
    /// All keys to warm
    keys: Vec<CacheKey>,
    /// Current position
    position: usize,
}

impl SequentialWarming {
    /// Create new sequential warming strategy
    pub fn new(keys: Vec<CacheKey>) -> Self {
        Self { keys, position: 0 }
    }
}

#[async_trait]
impl WarmingStrategy for SequentialWarming {
    async fn next_batch(&mut self, batch_size: usize) -> Result<Vec<CacheKey>> {
        let end = (self.position + batch_size).min(self.keys.len());
        let batch = self.keys[self.position..end].to_vec();
        self.position = end;
        Ok(batch)
    }

    async fn is_complete(&self) -> bool {
        self.position >= self.keys.len()
    }

    async fn progress(&self) -> f64 {
        if self.keys.is_empty() {
            1.0
        } else {
            self.position as f64 / self.keys.len() as f64
        }
    }

    async fn reset(&mut self) {
        self.position = 0;
    }
}

/// Random warming strategy
/// Warms cache by loading random keys
pub struct RandomWarming {
    /// All keys to warm
    keys: Vec<CacheKey>,
    /// Warmed keys count
    warmed_count: usize,
}

impl RandomWarming {
    /// Create new random warming strategy
    pub fn new(keys: Vec<CacheKey>) -> Self {
        Self {
            keys,
            warmed_count: 0,
        }
    }
}

#[async_trait]
impl WarmingStrategy for RandomWarming {
    async fn next_batch(&mut self, batch_size: usize) -> Result<Vec<CacheKey>> {
        // Seed fastrand for reproducibility
        fastrand::seed(42);
        let remaining = self.keys.len().saturating_sub(self.warmed_count);
        let batch_size = batch_size.min(remaining);

        let mut batch = Vec::with_capacity(batch_size);
        let mut indices: Vec<usize> = (0..self.keys.len()).collect();

        // Fisher-Yates shuffle
        for i in (1..indices.len()).rev() {
            let j = fastrand::usize(0..=i);
            indices.swap(i, j);
        }

        for i in 0..batch_size {
            if let Some(&idx) = indices.get(i) {
                if let Some(key) = self.keys.get(idx) {
                    batch.push(key.clone());
                }
            }
        }

        self.warmed_count += batch.len();
        Ok(batch)
    }

    async fn is_complete(&self) -> bool {
        self.warmed_count >= self.keys.len()
    }

    async fn progress(&self) -> f64 {
        if self.keys.is_empty() {
            1.0
        } else {
            self.warmed_count as f64 / self.keys.len() as f64
        }
    }

    async fn reset(&mut self) {
        self.warmed_count = 0;
    }
}

/// Priority-based warming strategy
/// Warms critical keys first based on priority scores
pub struct PriorityWarming {
    /// Keys with priorities (key, priority)
    keys_with_priority: Vec<(CacheKey, f64)>,
    /// Current position
    position: usize,
}

impl PriorityWarming {
    /// Create new priority-based warming strategy
    pub fn new(mut keys_with_priority: Vec<(CacheKey, f64)>) -> Self {
        // Sort by priority (descending)
        keys_with_priority
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Self {
            keys_with_priority,
            position: 0,
        }
    }
}

#[async_trait]
impl WarmingStrategy for PriorityWarming {
    async fn next_batch(&mut self, batch_size: usize) -> Result<Vec<CacheKey>> {
        let end = (self.position + batch_size).min(self.keys_with_priority.len());
        let batch = self.keys_with_priority[self.position..end]
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        self.position = end;
        Ok(batch)
    }

    async fn is_complete(&self) -> bool {
        self.position >= self.keys_with_priority.len()
    }

    async fn progress(&self) -> f64 {
        if self.keys_with_priority.is_empty() {
            1.0
        } else {
            self.position as f64 / self.keys_with_priority.len() as f64
        }
    }

    async fn reset(&mut self) {
        self.position = 0;
    }
}

/// Warming progress information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WarmingProgress {
    /// Total keys to warm
    pub total_keys: usize,
    /// Keys warmed so far
    pub warmed_keys: usize,
    /// Progress percentage (0.0 - 100.0)
    pub progress_percent: f64,
    /// Estimated time remaining (seconds)
    pub estimated_time_remaining: Option<i64>,
    /// Current warming rate (keys/sec)
    pub warming_rate: f64,
    /// Start time
    pub start_time: chrono::DateTime<chrono::Utc>,
    /// Elapsed time (seconds)
    pub elapsed_seconds: i64,
}

impl WarmingProgress {
    /// Create new warming progress
    pub fn new(total_keys: usize) -> Self {
        Self {
            total_keys,
            warmed_keys: 0,
            progress_percent: 0.0,
            estimated_time_remaining: None,
            warming_rate: 0.0,
            start_time: chrono::Utc::now(),
            elapsed_seconds: 0,
        }
    }

    /// Update progress
    pub fn update(&mut self, warmed_keys: usize) {
        self.warmed_keys = warmed_keys;
        self.progress_percent = if self.total_keys > 0 {
            (warmed_keys as f64 / self.total_keys as f64) * 100.0
        } else {
            100.0
        };

        let now = chrono::Utc::now();
        self.elapsed_seconds = (now - self.start_time).num_seconds();

        if self.elapsed_seconds > 0 {
            self.warming_rate = warmed_keys as f64 / self.elapsed_seconds as f64;

            let remaining_keys = self.total_keys.saturating_sub(warmed_keys);
            if self.warming_rate > 0.0 {
                self.estimated_time_remaining =
                    Some((remaining_keys as f64 / self.warming_rate) as i64);
            }
        }
    }

    /// Check if complete
    pub fn is_complete(&self) -> bool {
        self.warmed_keys >= self.total_keys
    }
}

/// Data source for cache warming
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Load data for a key
    async fn load(&self, key: &CacheKey) -> Result<CacheValue>;

    /// Check if key exists
    async fn exists(&self, key: &CacheKey) -> Result<bool>;

    /// Get all available keys
    async fn keys(&self) -> Result<Vec<CacheKey>>;
}

/// Cache warmer
pub struct CacheWarmer {
    /// Cache to warm
    cache: Arc<MultiTierCache>,
    /// Data source
    data_source: Arc<dyn DataSource>,
    /// Warming strategy
    strategy: Arc<RwLock<Box<dyn WarmingStrategy>>>,
    /// Progress tracking
    progress: Arc<RwLock<WarmingProgress>>,
    /// Batch size for parallel loading
    batch_size: usize,
    /// Is warming active
    is_active: Arc<RwLock<bool>>,
}

impl CacheWarmer {
    /// Create new cache warmer
    pub fn new(
        cache: Arc<MultiTierCache>,
        data_source: Arc<dyn DataSource>,
        strategy: Box<dyn WarmingStrategy>,
        total_keys: usize,
    ) -> Self {
        Self {
            cache,
            data_source,
            strategy: Arc::new(RwLock::new(strategy)),
            progress: Arc::new(RwLock::new(WarmingProgress::new(total_keys))),
            batch_size: 10,
            is_active: Arc::new(RwLock::new(false)),
        }
    }

    /// Set batch size
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Start warming process
    pub async fn start(&self) -> Result<()> {
        let mut is_active = self.is_active.write().await;
        if *is_active {
            return Err(CacheError::Other("Warming already in progress".to_string()));
        }
        *is_active = true;
        drop(is_active);

        let mut warmed_count = 0;

        loop {
            // Check if complete
            let is_complete = {
                let strategy = self.strategy.read().await;
                strategy.is_complete().await
            };

            if is_complete {
                break;
            }

            // Get next batch
            let batch = {
                let mut strategy = self.strategy.write().await;
                strategy.next_batch(self.batch_size).await?
            };

            if batch.is_empty() {
                break;
            }

            // Load batch in parallel
            let mut tasks: Vec<tokio::task::JoinHandle<std::result::Result<usize, CacheError>>> =
                Vec::new();

            for key in batch {
                let data_source = Arc::clone(&self.data_source);
                let cache = Arc::clone(&self.cache);

                let task = tokio::spawn(async move {
                    if let Ok(value) = data_source.load(&key).await {
                        let _ = cache.put(key, value).await;
                        Ok::<usize, CacheError>(1)
                    } else {
                        Ok::<usize, CacheError>(0)
                    }
                });

                tasks.push(task);
            }

            // Wait for batch to complete
            for task in tasks {
                if let Ok(Ok(count)) = task.await {
                    warmed_count += count;
                }
            }

            // Update progress
            let mut progress = self.progress.write().await;
            progress.update(warmed_count);
        }

        let mut is_active = self.is_active.write().await;
        *is_active = false;

        Ok(())
    }

    /// Start warming in background
    pub fn start_background(self: Arc<Self>) -> tokio::task::JoinHandle<Result<()>> {
        tokio::spawn(async move { self.start().await })
    }

    /// Stop warming process
    pub async fn stop(&self) -> Result<()> {
        let mut is_active = self.is_active.write().await;
        *is_active = false;
        Ok(())
    }

    /// Get current progress
    pub async fn progress(&self) -> WarmingProgress {
        self.progress.read().await.clone()
    }

    /// Check if warming is active
    pub async fn is_active(&self) -> bool {
        *self.is_active.read().await
    }

    /// Reset warming process
    pub async fn reset(&self) -> Result<()> {
        let mut strategy = self.strategy.write().await;
        strategy.reset().await;

        let mut progress = self.progress.write().await;
        *progress = WarmingProgress::new(progress.total_keys);

        Ok(())
    }
}

/// Simple in-memory data source for testing
pub struct InMemoryDataSource {
    /// Data storage
    data: Arc<RwLock<HashMap<CacheKey, CacheValue>>>,
}

impl InMemoryDataSource {
    /// Create new in-memory data source
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add data
    pub async fn add(&self, key: CacheKey, value: CacheValue) {
        let mut data = self.data.write().await;
        data.insert(key, value);
    }
}

impl Default for InMemoryDataSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for InMemoryDataSource {
    async fn load(&self, key: &CacheKey) -> Result<CacheValue> {
        let data = self.data.read().await;
        data.get(key)
            .cloned()
            .ok_or_else(|| CacheError::KeyNotFound(key.clone()))
    }

    async fn exists(&self, key: &CacheKey) -> Result<bool> {
        let data = self.data.read().await;
        Ok(data.contains_key(key))
    }

    async fn keys(&self) -> Result<Vec<CacheKey>> {
        let data = self.data.read().await;
        Ok(data.keys().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CacheConfig;
    use crate::compression::DataType;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_sequential_warming() {
        let keys: Vec<_> = (0..100).map(|i| format!("key{}", i)).collect();
        let mut strategy = SequentialWarming::new(keys.clone());

        let batch = strategy.next_batch(10).await.expect("next_batch failed");
        assert_eq!(batch.len(), 10);
        assert_eq!(batch[0], "key0");

        let progress = strategy.progress().await;
        approx::assert_relative_eq!(progress, 0.1, epsilon = 0.01);
    }

    #[tokio::test]
    async fn test_random_warming() {
        let keys: Vec<_> = (0..100).map(|i| format!("key{}", i)).collect();
        let mut strategy = RandomWarming::new(keys.clone());

        let batch = strategy.next_batch(10).await.expect("next_batch failed");
        assert_eq!(batch.len(), 10);

        let progress = strategy.progress().await;
        approx::assert_relative_eq!(progress, 0.1, epsilon = 0.01);
    }

    #[tokio::test]
    async fn test_priority_warming() {
        let mut keys_with_priority = Vec::new();
        for i in 0..100 {
            keys_with_priority.push((format!("key{}", i), i as f64));
        }

        let mut strategy = PriorityWarming::new(keys_with_priority);

        let batch = strategy.next_batch(10).await.expect("next_batch failed");
        assert_eq!(batch.len(), 10);

        // Should get highest priority keys first
        assert_eq!(batch[0], "key99");
    }

    #[tokio::test]
    async fn test_cache_warmer() {
        let temp_dir = std::env::temp_dir().join("oxigdal_warmer_test");
        let config = CacheConfig {
            l1_size: 1024 * 1024,
            l2_size: 0,
            l3_size: 0,
            enable_compression: false,
            enable_prefetch: false,
            enable_distributed: false,
            cache_dir: Some(temp_dir.clone()),
        };

        let cache = Arc::new(
            MultiTierCache::new(config)
                .await
                .expect("cache creation failed"),
        );

        // Create data source
        let data_source = Arc::new(InMemoryDataSource::new());

        for i in 0..10 {
            let key = format!("key{}", i);
            let value = CacheValue::new(Bytes::from(format!("value{}", i)), DataType::Binary);
            data_source.add(key.clone(), value).await;
        }

        // Create warmer
        let keys: Vec<_> = (0..10).map(|i| format!("key{}", i)).collect();
        let strategy = Box::new(SequentialWarming::new(keys.clone()));

        let warmer = Arc::new(CacheWarmer::new(
            Arc::clone(&cache),
            data_source,
            strategy,
            10,
        ));

        // Start warming
        warmer.start().await.expect("warming failed");

        // Check progress
        let progress = warmer.progress().await;
        assert!(progress.is_complete());

        // Clean up
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[test]
    fn test_warming_progress() {
        let mut progress = WarmingProgress::new(100);

        progress.update(50);
        approx::assert_relative_eq!(progress.progress_percent, 50.0, epsilon = 0.01);
        assert!(!progress.is_complete());

        progress.update(100);
        approx::assert_relative_eq!(progress.progress_percent, 100.0, epsilon = 0.01);
        assert!(progress.is_complete());
    }
}
