//! Advanced write policies for caching
//!
//! Provides various write strategies:
//! - Write-through with buffering
//! - Write-back with dirty tracking
//! - Write-allocate vs no-write-allocate
//! - Write coalescing
//! - Write-behind with async flush
//! - Write amplification tracking

use crate::error::Result;
use crate::multi_tier::CacheKey;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Type alias for write buffer
type WriteBufferQueue = Arc<RwLock<VecDeque<(CacheKey, Vec<u8>)>>>;

/// Write policy type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePolicyType {
    /// Write-through: synchronous write to backing store
    WriteThrough,
    /// Write-back: write to cache, flush later
    WriteBack,
    /// Write-behind: asynchronous write to backing store
    WriteBehind,
    /// Write-around: bypass cache, write directly to backing store
    WriteAround,
}

/// Dirty block tracking
#[derive(Debug, Clone)]
pub struct DirtyBlock {
    /// Cache key
    pub key: CacheKey,
    /// Time when marked dirty
    pub dirty_time: Instant,
    /// Size in bytes
    pub size: usize,
    /// Number of writes
    pub write_count: u64,
}

impl DirtyBlock {
    /// Create new dirty block
    pub fn new(key: CacheKey, size: usize) -> Self {
        Self {
            key,
            dirty_time: Instant::now(),
            size,
            write_count: 1,
        }
    }

    /// Record another write
    pub fn record_write(&mut self) {
        self.write_count += 1;
    }

    /// Get age of dirty block
    pub fn age(&self) -> Duration {
        self.dirty_time.elapsed()
    }
}

/// Write-back manager with dirty tracking
pub struct WriteBackManager {
    /// Dirty blocks
    dirty_blocks: Arc<RwLock<HashMap<CacheKey, DirtyBlock>>>,
    /// Maximum dirty blocks before forced flush
    max_dirty_blocks: usize,
    /// Maximum dirty age before flush
    max_dirty_age: Duration,
    /// Write coalescing enabled
    coalescing_enabled: bool,
}

impl WriteBackManager {
    /// Create new write-back manager
    pub fn new(max_dirty_blocks: usize, max_dirty_age: Duration) -> Self {
        Self {
            dirty_blocks: Arc::new(RwLock::new(HashMap::new())),
            max_dirty_blocks,
            max_dirty_age,
            coalescing_enabled: true,
        }
    }

    /// Enable or disable write coalescing
    pub fn set_coalescing(&mut self, enabled: bool) {
        self.coalescing_enabled = enabled;
    }

    /// Mark block as dirty
    pub async fn mark_dirty(&self, key: CacheKey, size: usize) -> Result<bool> {
        let mut dirty = self.dirty_blocks.write().await;

        if let Some(block) = dirty.get_mut(&key) {
            // Coalesce writes
            if self.coalescing_enabled {
                block.record_write();
                return Ok(false); // No flush needed
            }
        } else {
            dirty.insert(key.clone(), DirtyBlock::new(key, size));
        }

        // Check if flush is needed
        let needs_flush = dirty.len() >= self.max_dirty_blocks;
        Ok(needs_flush)
    }

    /// Get blocks that need flushing
    pub async fn get_flush_candidates(&self) -> Vec<DirtyBlock> {
        let dirty = self.dirty_blocks.read().await;
        let _now = Instant::now();

        dirty
            .values()
            .filter(|block| {
                block.age() >= self.max_dirty_age || dirty.len() >= self.max_dirty_blocks
            })
            .cloned()
            .collect()
    }

    /// Mark block as clean (after flush)
    pub async fn mark_clean(&self, key: &CacheKey) {
        self.dirty_blocks.write().await.remove(key);
    }

    /// Get dirty block count
    pub async fn dirty_count(&self) -> usize {
        self.dirty_blocks.read().await.len()
    }

    /// Get total dirty bytes
    pub async fn dirty_bytes(&self) -> usize {
        self.dirty_blocks
            .read()
            .await
            .values()
            .map(|b| b.size)
            .sum()
    }

    /// Get oldest dirty block age
    pub async fn oldest_dirty_age(&self) -> Option<Duration> {
        self.dirty_blocks
            .read()
            .await
            .values()
            .map(|b| b.age())
            .max()
    }
}

/// Write buffer for buffered write-through
pub struct WriteBuffer {
    /// Buffered writes
    buffer: WriteBufferQueue,
    /// Maximum buffer size
    max_buffer_size: usize,
    /// Current buffer bytes
    current_size: Arc<RwLock<usize>>,
}

impl WriteBuffer {
    /// Create new write buffer
    pub fn new(max_buffer_size: usize) -> Self {
        Self {
            buffer: Arc::new(RwLock::new(VecDeque::new())),
            max_buffer_size,
            current_size: Arc::new(RwLock::new(0)),
        }
    }

    /// Add write to buffer
    pub async fn add_write(&self, key: CacheKey, data: Vec<u8>) -> Result<bool> {
        let data_size = data.len();
        let mut size = self.current_size.write().await;

        // Check if flush is needed (when at or exceeding capacity)
        if *size + data_size >= self.max_buffer_size {
            return Ok(true); // Need to flush
        }

        let mut buffer = self.buffer.write().await;
        buffer.push_back((key, data));
        *size += data_size;

        Ok(false)
    }

    /// Get all buffered writes
    pub async fn drain(&self) -> Vec<(CacheKey, Vec<u8>)> {
        let mut buffer = self.buffer.write().await;
        let mut size = self.current_size.write().await;

        let writes: Vec<_> = buffer.drain(..).collect();
        *size = 0;

        writes
    }

    /// Get buffer size
    pub async fn size(&self) -> usize {
        *self.current_size.read().await
    }

    /// Get buffer count
    pub async fn count(&self) -> usize {
        self.buffer.read().await.len()
    }
}

/// Write amplification tracker
pub struct WriteAmplificationTracker {
    /// Total bytes written to cache
    cache_writes: Arc<RwLock<u64>>,
    /// Total bytes written to backing store
    backing_writes: Arc<RwLock<u64>>,
}

impl WriteAmplificationTracker {
    /// Create new tracker
    pub fn new() -> Self {
        Self {
            cache_writes: Arc::new(RwLock::new(0)),
            backing_writes: Arc::new(RwLock::new(0)),
        }
    }

    /// Record cache write
    pub async fn record_cache_write(&self, bytes: u64) {
        *self.cache_writes.write().await += bytes;
    }

    /// Record backing store write
    pub async fn record_backing_write(&self, bytes: u64) {
        *self.backing_writes.write().await += bytes;
    }

    /// Calculate write amplification factor
    pub async fn amplification_factor(&self) -> f64 {
        let cache = *self.cache_writes.read().await;
        let backing = *self.backing_writes.read().await;

        if cache == 0 {
            0.0
        } else {
            backing as f64 / cache as f64
        }
    }

    /// Get cache writes
    pub async fn cache_writes(&self) -> u64 {
        *self.cache_writes.read().await
    }

    /// Get backing writes
    pub async fn backing_writes(&self) -> u64 {
        *self.backing_writes.read().await
    }

    /// Reset statistics
    pub async fn reset(&self) {
        *self.cache_writes.write().await = 0;
        *self.backing_writes.write().await = 0;
    }
}

impl Default for WriteAmplificationTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Write policy manager
pub struct WritePolicyManager {
    /// Current policy type
    policy_type: WritePolicyType,
    /// Write-back manager
    write_back: WriteBackManager,
    /// Write buffer
    write_buffer: WriteBuffer,
    /// Amplification tracker
    amplification: WriteAmplificationTracker,
}

impl WritePolicyManager {
    /// Create new write policy manager
    pub fn new(
        policy_type: WritePolicyType,
        max_dirty_blocks: usize,
        max_dirty_age: Duration,
        buffer_size: usize,
    ) -> Self {
        Self {
            policy_type,
            write_back: WriteBackManager::new(max_dirty_blocks, max_dirty_age),
            write_buffer: WriteBuffer::new(buffer_size),
            amplification: WriteAmplificationTracker::new(),
        }
    }

    /// Get policy type
    pub fn policy_type(&self) -> WritePolicyType {
        self.policy_type
    }

    /// Set policy type
    pub fn set_policy_type(&mut self, policy_type: WritePolicyType) {
        self.policy_type = policy_type;
    }

    /// Handle write operation
    pub async fn handle_write(&self, key: CacheKey, data: Vec<u8>) -> Result<WriteAction> {
        let data_size = data.len();

        match self.policy_type {
            WritePolicyType::WriteThrough => {
                // Buffer the write
                let needs_flush = self.write_buffer.add_write(key, data).await?;

                if needs_flush {
                    Ok(WriteAction::FlushBuffer)
                } else {
                    Ok(WriteAction::Buffered)
                }
            }
            WritePolicyType::WriteBack => {
                // Mark as dirty
                let needs_flush = self.write_back.mark_dirty(key, data_size).await?;

                if needs_flush {
                    Ok(WriteAction::FlushDirty)
                } else {
                    Ok(WriteAction::Deferred)
                }
            }
            WritePolicyType::WriteBehind => {
                // Asynchronous write
                Ok(WriteAction::Async)
            }
            WritePolicyType::WriteAround => {
                // Write directly to backing store
                Ok(WriteAction::Direct)
            }
        }
    }

    /// Get write-back manager
    pub fn write_back(&self) -> &WriteBackManager {
        &self.write_back
    }

    /// Get write buffer
    pub fn write_buffer(&self) -> &WriteBuffer {
        &self.write_buffer
    }

    /// Get amplification tracker
    pub fn amplification(&self) -> &WriteAmplificationTracker {
        &self.amplification
    }
}

/// Write action to take
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteAction {
    /// Write has been buffered
    Buffered,
    /// Need to flush buffer
    FlushBuffer,
    /// Write has been deferred (dirty tracking)
    Deferred,
    /// Need to flush dirty blocks
    FlushDirty,
    /// Asynchronous write
    Async,
    /// Direct write to backing store
    Direct,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirty_block() {
        let mut block = DirtyBlock::new("key1".to_string(), 1024);
        assert_eq!(block.write_count, 1);

        block.record_write();
        assert_eq!(block.write_count, 2);

        assert!(block.age().as_secs() < 1);
    }

    #[tokio::test]
    async fn test_write_back_manager() {
        let manager = WriteBackManager::new(10, Duration::from_secs(60));

        let needs_flush = manager
            .mark_dirty("key1".to_string(), 1024)
            .await
            .unwrap_or(false);
        assert!(!needs_flush);

        let count = manager.dirty_count().await;
        assert_eq!(count, 1);

        let bytes = manager.dirty_bytes().await;
        assert_eq!(bytes, 1024);
    }

    #[tokio::test]
    async fn test_write_buffer() {
        let buffer = WriteBuffer::new(1024 * 10);

        let data = vec![0u8; 1024];
        let needs_flush = buffer
            .add_write("key1".to_string(), data)
            .await
            .unwrap_or(false);
        assert!(!needs_flush);

        let size = buffer.size().await;
        assert_eq!(size, 1024);

        let writes = buffer.drain().await;
        assert_eq!(writes.len(), 1);

        let size = buffer.size().await;
        assert_eq!(size, 0);
    }

    #[tokio::test]
    async fn test_write_amplification() {
        let tracker = WriteAmplificationTracker::new();

        tracker.record_cache_write(1000).await;
        tracker.record_backing_write(2000).await;

        let amp = tracker.amplification_factor().await;
        assert!((amp - 2.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_write_policy_manager() {
        let manager = WritePolicyManager::new(
            WritePolicyType::WriteBack,
            10,
            Duration::from_secs(60),
            1024 * 10,
        );

        let data = vec![0u8; 1024];
        let action = manager
            .handle_write("key1".to_string(), data)
            .await
            .unwrap_or(WriteAction::Deferred);

        assert_eq!(action, WriteAction::Deferred);
    }
}
