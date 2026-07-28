//! Sync manager for coordinating synchronization

use super::protocol::SyncProtocol;
use super::{SyncItem, SyncState, SyncStrategy};
use crate::cache::Cache;
use crate::error::{EdgeError, Result};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;

/// Sync manager
pub struct SyncManager {
    strategy: SyncStrategy,
    cache: Arc<Cache>,
    state: Arc<RwLock<SyncState>>,
    protocol: Arc<dyn SyncProtocol>,
    running: Arc<AtomicBool>,
    handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl SyncManager {
    /// Create a new sync manager backed by the given [`SyncProtocol`].
    ///
    /// Callers must supply a real, network-backed (or otherwise persistent)
    /// `SyncProtocol` implementation appropriate for their deployment (e.g.
    /// [`super::protocol::HttpSyncProtocol`] under the `http-sync` feature).
    /// This crate deliberately does **not** default to any in-memory mock
    /// protocol here: a device configured to sync to the cloud must be
    /// explicitly wired to a protocol that actually talks to that cloud, or
    /// it will not compile/construct a `SyncManager` at all -- there is no
    /// silent fallback that would appear to sync while only ever reading
    /// and writing its own local process memory.
    pub fn new(
        strategy: SyncStrategy,
        cache: Arc<Cache>,
        protocol: Arc<dyn SyncProtocol>,
    ) -> Result<Self> {
        let state = Arc::new(RwLock::new(SyncState::new()));

        Ok(Self {
            strategy,
            cache,
            state,
            protocol,
            running: Arc::new(AtomicBool::new(false)),
            handle: Arc::new(RwLock::new(None)),
        })
    }

    /// Start sync manager
    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err(EdgeError::sync("Sync manager already running"));
        }

        self.running.store(true, Ordering::Relaxed);

        match self.strategy {
            SyncStrategy::Manual => {
                // No automatic sync
                Ok(())
            }
            SyncStrategy::Periodic => self.start_periodic_sync().await,
            SyncStrategy::Incremental => self.start_incremental_sync().await,
            SyncStrategy::Batch => self.start_batch_sync().await,
            SyncStrategy::Realtime => self.start_realtime_sync().await,
        }
    }

    /// Stop sync manager
    pub async fn stop(&self) -> Result<()> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(false, Ordering::Relaxed);

        let handle = {
            let mut handle_lock = self.handle.write();
            handle_lock.take()
        };

        if let Some(handle) = handle {
            let timeout_duration = Duration::from_secs(5);
            match tokio::time::timeout(timeout_duration, handle).await {
                Ok(_) => {}
                Err(_) => {
                    tracing::warn!("Sync manager stop timed out after {:?}", timeout_duration);
                }
            }
        }

        Ok(())
    }

    /// Start periodic sync
    async fn start_periodic_sync(&self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                if protocol.is_connected().await {
                    let _ = Self::perform_sync(&protocol, &state).await;
                }

                tokio::time::sleep(Duration::from_millis(100)).await; // 100ms for tests
            }
        });

        let mut handle_lock = self.handle.write();
        *handle_lock = Some(handle);

        Ok(())
    }

    /// Start incremental sync
    async fn start_incremental_sync(&self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                if protocol.is_connected().await {
                    let has_pending = {
                        let state_read = state.read();
                        !state_read.pending_items.is_empty()
                    };
                    if has_pending {
                        let _ = Self::perform_sync(&protocol, &state).await;
                    }
                }

                tokio::time::sleep(Duration::from_millis(100)).await; // 100ms for tests
            }
        });

        let mut handle_lock = self.handle.write();
        *handle_lock = Some(handle);

        Ok(())
    }

    /// Start batch sync
    async fn start_batch_sync(&self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                if protocol.is_connected().await {
                    let should_sync = {
                        let state_read = state.read();
                        state_read.pending_items.len() >= 10
                    };
                    if should_sync {
                        // Batch size threshold
                        let _ = Self::perform_sync(&protocol, &state).await;
                    }
                }

                tokio::time::sleep(Duration::from_millis(100)).await; // 100ms for tests
            }
        });

        let mut handle_lock = self.handle.write();
        *handle_lock = Some(handle);

        Ok(())
    }

    /// Start real-time sync
    async fn start_realtime_sync(&self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                if protocol.is_connected().await {
                    let has_pending = {
                        let state_read = state.read();
                        !state_read.pending_items.is_empty()
                    };
                    if has_pending {
                        let _ = Self::perform_sync(&protocol, &state).await;
                    }
                }

                tokio::time::sleep(Duration::from_millis(100)).await; // 100ms for tests
            }
        });

        let mut handle_lock = self.handle.write();
        *handle_lock = Some(handle);

        Ok(())
    }

    /// Perform synchronization
    async fn perform_sync(
        protocol: &Arc<dyn SyncProtocol>,
        state: &Arc<RwLock<SyncState>>,
    ) -> Result<()> {
        let items: Vec<SyncItem> = {
            let state_read = state.read();
            state_read.pending_items.values().cloned().collect()
        };

        if items.is_empty() {
            return Ok(());
        }

        match protocol.sync(items).await {
            Ok(result) => {
                let mut state_write = state.write();

                // Remove synced items from pending
                for item_id in &result.pushed {
                    state_write.remove_pending(item_id);
                }

                state_write.complete_sync();
                Ok(())
            }
            Err(e) => {
                let mut state_write = state.write();
                state_write.fail_sync(e.to_string());
                Err(e)
            }
        }
    }

    /// Manually trigger sync
    pub async fn sync_now(&self) -> Result<()> {
        Self::perform_sync(&self.protocol, &self.state).await
    }

    /// Add item to pending sync queue
    pub fn add_pending(&self, item: SyncItem) {
        let mut state = self.state.write();
        state.add_pending(item);
    }

    /// Get sync state
    pub fn state(&self) -> SyncState {
        self.state.read().clone()
    }

    /// Get sync statistics
    pub fn statistics(&self) -> super::SyncStatistics {
        self.state.read().statistics()
    }

    /// Get reference to the cache
    pub fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }

    /// Check if sync is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Set sync protocol (for testing)
    pub fn set_protocol(&mut self, protocol: Arc<dyn SyncProtocol>) {
        self.protocol = protocol;
    }
}

impl Drop for SyncManager {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheConfig;
    use crate::sync::protocol::MockSyncProtocol;

    fn mock_protocol() -> Arc<dyn SyncProtocol> {
        Arc::new(MockSyncProtocol::new())
    }

    #[tokio::test]
    async fn test_sync_manager_creation() -> Result<()> {
        let cache_config = CacheConfig::minimal();
        let cache = Arc::new(Cache::new(cache_config)?);
        let manager = SyncManager::new(SyncStrategy::Manual, cache, mock_protocol())?;

        assert!(!manager.is_running());
        Ok(())
    }

    #[tokio::test]
    async fn test_sync_manager_lifecycle() -> Result<()> {
        let cache_config = CacheConfig::minimal();
        let cache = Arc::new(Cache::new(cache_config)?);
        let manager = SyncManager::new(SyncStrategy::Manual, cache, mock_protocol())?;

        manager.start().await?;
        assert!(manager.is_running());

        manager.stop().await?;
        assert!(!manager.is_running());

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_manager_add_pending() -> Result<()> {
        let cache_config = CacheConfig::minimal();
        let cache = Arc::new(Cache::new(cache_config)?);
        let manager = SyncManager::new(SyncStrategy::Manual, cache, mock_protocol())?;

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        manager.add_pending(item);

        let state = manager.state();
        assert_eq!(state.pending_count(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_manager_manual_sync() -> Result<()> {
        let cache_config = CacheConfig::minimal();
        let cache = Arc::new(Cache::new(cache_config)?);
        let manager = SyncManager::new(SyncStrategy::Manual, cache, mock_protocol())?;

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        manager.add_pending(item);
        manager.sync_now().await?;

        let state = manager.state();
        assert_eq!(state.pending_count(), 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_manager_statistics() -> Result<()> {
        let cache_config = CacheConfig::minimal();
        let cache = Arc::new(Cache::new(cache_config)?);
        let manager = SyncManager::new(SyncStrategy::Manual, cache, mock_protocol())?;

        let stats = manager.statistics();
        assert_eq!(stats.total_syncs, 0);
        assert_eq!(stats.pending_items, 0);

        Ok(())
    }

    /// Regression test: `SyncManager` must actually route pushes through
    /// whatever protocol was injected, not a hardcoded internal mock. Two
    /// managers wired to *different* protocol instances must not see each
    /// other's synced data.
    #[tokio::test]
    async fn test_sync_manager_uses_injected_protocol_not_a_hardcoded_mock() -> Result<()> {
        let cache_config = CacheConfig::minimal();

        let protocol_a = mock_protocol();
        let manager_a = SyncManager::new(
            SyncStrategy::Manual,
            Arc::new(Cache::new(cache_config.clone())?),
            Arc::clone(&protocol_a),
        )?;

        let protocol_b = mock_protocol();
        let manager_b = SyncManager::new(
            SyncStrategy::Manual,
            Arc::new(Cache::new(cache_config)?),
            Arc::clone(&protocol_b),
        )?;

        let item_a = SyncItem::new("item-a".to_string(), "key-a".to_string(), vec![9, 9, 9], 1);
        manager_a.add_pending(item_a);
        manager_a.sync_now().await?;

        let item_b = SyncItem::new("item-b".to_string(), "key-b".to_string(), vec![7, 7, 7], 1);
        manager_b.add_pending(item_b);
        manager_b.sync_now().await?;

        // manager_a's item went to protocol_a, not protocol_b, and vice
        // versa: each protocol must see only its own manager's data.
        let items_in_b = protocol_b.pull(None).await?;
        assert_eq!(items_in_b.len(), 1);
        assert_eq!(items_in_b[0].id, "item-b");

        let items_in_a = protocol_a.pull(None).await?;
        assert_eq!(items_in_a.len(), 1);
        assert_eq!(items_in_a[0].id, "item-a");

        Ok(())
    }
}
