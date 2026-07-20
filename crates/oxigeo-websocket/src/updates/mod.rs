//! Live updates system for real-time data changes
//!
//! This module provides:
//! - Tile update notifications
//! - Feature change tracking
//! - Change stream processing
//! - Incremental update delivery

pub mod change_stream;
pub mod feature_updates;
pub mod incremental;
pub mod tile_updates;

pub use change_stream::{ChangeEvent, ChangeStream, ChangeStreamConfig};
pub use feature_updates::{FeatureUpdate, FeatureUpdateManager, FeatureUpdateType};
pub use incremental::{IncrementalUpdate, IncrementalUpdateManager, UpdateDelta};
pub use tile_updates::{TileUpdate, TileUpdateManager, TileUpdateType};

use std::sync::Arc;
use tokio::sync::RwLock;

/// Update configuration
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    /// Enable tile updates
    pub enable_tile_updates: bool,
    /// Enable feature updates
    pub enable_feature_updates: bool,
    /// Enable change streams
    pub enable_change_streams: bool,
    /// Maximum update queue size
    pub max_queue_size: usize,
    /// Update batch size
    pub batch_size: usize,
    /// Update interval in milliseconds
    pub update_interval_ms: u64,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enable_tile_updates: true,
            enable_feature_updates: true,
            enable_change_streams: true,
            max_queue_size: 10_000,
            batch_size: 100,
            update_interval_ms: 100,
        }
    }
}

/// Update system manager
pub struct UpdateSystem {
    #[allow(dead_code)]
    config: UpdateConfig,
    tile_manager: Arc<TileUpdateManager>,
    feature_manager: Arc<FeatureUpdateManager>,
    incremental_manager: Arc<IncrementalUpdateManager>,
    change_streams: Arc<RwLock<std::collections::HashMap<String, Arc<ChangeStream>>>>,
}

impl UpdateSystem {
    /// Create a new update system
    pub fn new(config: UpdateConfig) -> Self {
        Self {
            config: config.clone(),
            tile_manager: Arc::new(TileUpdateManager::new(config.max_queue_size)),
            feature_manager: Arc::new(FeatureUpdateManager::new(config.max_queue_size)),
            incremental_manager: Arc::new(IncrementalUpdateManager::new()),
            change_streams: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get tile update manager
    pub fn tile_manager(&self) -> &Arc<TileUpdateManager> {
        &self.tile_manager
    }

    /// Get feature update manager
    pub fn feature_manager(&self) -> &Arc<FeatureUpdateManager> {
        &self.feature_manager
    }

    /// Get incremental update manager
    pub fn incremental_manager(&self) -> &Arc<IncrementalUpdateManager> {
        &self.incremental_manager
    }

    /// Create or get a change stream
    pub async fn get_or_create_stream(&self, name: &str) -> Arc<ChangeStream> {
        let mut streams = self.change_streams.write().await;

        streams
            .entry(name.to_string())
            .or_insert_with(|| {
                Arc::new(ChangeStream::new(
                    name.to_string(),
                    ChangeStreamConfig::default(),
                ))
            })
            .clone()
    }

    /// Remove a change stream
    pub async fn remove_stream(&self, name: &str) -> Option<Arc<ChangeStream>> {
        let mut streams = self.change_streams.write().await;
        streams.remove(name)
    }

    /// Get update statistics
    pub async fn stats(&self) -> UpdateStats {
        let tile_stats = self.tile_manager.stats().await;
        let feature_stats = self.feature_manager.stats().await;

        UpdateStats {
            tile_updates: tile_stats.total_updates,
            feature_updates: feature_stats.total_updates,
            change_streams: self.change_streams.read().await.len(),
        }
    }
}

/// Update statistics
#[derive(Debug, Clone)]
pub struct UpdateStats {
    /// Total tile updates
    pub tile_updates: u64,
    /// Total feature updates
    pub feature_updates: u64,
    /// Number of change streams
    pub change_streams: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.enable_tile_updates);
        assert!(config.enable_feature_updates);
        assert!(config.enable_change_streams);
    }

    #[tokio::test]
    async fn test_update_system() {
        let config = UpdateConfig::default();
        let system = UpdateSystem::new(config);

        let stats = system.stats().await;
        assert_eq!(stats.tile_updates, 0);
        assert_eq!(stats.feature_updates, 0);
        assert_eq!(stats.change_streams, 0);
    }
}
