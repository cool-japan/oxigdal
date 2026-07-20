//! Feature update management and notifications

use crate::error::Result;
use crate::protocol::message::{ChangeType, FeaturePayload, Message, MessageType, Payload};
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Feature update type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureUpdateType {
    /// Feature created
    Created,
    /// Feature updated
    Updated,
    /// Feature deleted
    Deleted,
}

impl From<FeatureUpdateType> for ChangeType {
    fn from(update_type: FeatureUpdateType) -> Self {
        match update_type {
            FeatureUpdateType::Created => ChangeType::Created,
            FeatureUpdateType::Updated => ChangeType::Updated,
            FeatureUpdateType::Deleted => ChangeType::Deleted,
        }
    }
}

/// Feature update
pub struct FeatureUpdate {
    /// Feature ID
    pub id: String,
    /// Layer name
    pub layer: String,
    /// Update type
    pub update_type: FeatureUpdateType,
    /// GeoJSON feature
    pub feature: serde_json::Value,
    /// Timestamp
    pub timestamp: i64,
}

impl FeatureUpdate {
    /// Create a new feature update
    pub fn new(
        id: String,
        layer: String,
        update_type: FeatureUpdateType,
        feature: serde_json::Value,
    ) -> Self {
        Self {
            id,
            layer,
            update_type,
            feature,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create a created update
    pub fn created(id: String, layer: String, feature: serde_json::Value) -> Self {
        Self::new(id, layer, FeatureUpdateType::Created, feature)
    }

    /// Create an updated update
    pub fn updated(id: String, layer: String, feature: serde_json::Value) -> Self {
        Self::new(id, layer, FeatureUpdateType::Updated, feature)
    }

    /// Create a deleted update
    pub fn deleted(id: String, layer: String) -> Self {
        Self::new(
            id,
            layer,
            FeatureUpdateType::Deleted,
            serde_json::Value::Null,
        )
    }

    /// Convert to message
    pub fn to_message(&self) -> Message {
        let payload = Payload::FeatureData(FeaturePayload {
            id: self.id.clone(),
            layer: self.layer.clone(),
            feature: self.feature.clone(),
            change_type: self.update_type.into(),
        });

        Message::new(MessageType::FeatureUpdate, payload)
    }
}

/// Feature update manager
pub struct FeatureUpdateManager {
    /// Pending updates by layer
    updates: Arc<RwLock<HashMap<String, VecDeque<FeatureUpdate>>>>,
    /// Maximum queue size per layer
    max_queue_size: usize,
    /// Statistics
    stats: Arc<FeatureUpdateStats>,
}

/// Feature update statistics
struct FeatureUpdateStats {
    total_updates: AtomicU64,
    created: AtomicU64,
    updated: AtomicU64,
    deleted: AtomicU64,
    dropped_updates: AtomicU64,
}

impl FeatureUpdateManager {
    /// Create a new feature update manager
    pub fn new(max_queue_size: usize) -> Self {
        Self {
            updates: Arc::new(RwLock::new(HashMap::new())),
            max_queue_size,
            stats: Arc::new(FeatureUpdateStats {
                total_updates: AtomicU64::new(0),
                created: AtomicU64::new(0),
                updated: AtomicU64::new(0),
                deleted: AtomicU64::new(0),
                dropped_updates: AtomicU64::new(0),
            }),
        }
    }

    /// Add a feature update
    pub fn add_update(&self, update: FeatureUpdate) -> Result<()> {
        self.stats.total_updates.fetch_add(1, Ordering::Relaxed);

        match update.update_type {
            FeatureUpdateType::Created => {
                self.stats.created.fetch_add(1, Ordering::Relaxed);
            }
            FeatureUpdateType::Updated => {
                self.stats.updated.fetch_add(1, Ordering::Relaxed);
            }
            FeatureUpdateType::Deleted => {
                self.stats.deleted.fetch_add(1, Ordering::Relaxed);
            }
        }

        let mut updates = self.updates.write();
        let queue = updates.entry(update.layer.clone()).or_default();

        if queue.len() >= self.max_queue_size {
            // Drop oldest update
            queue.pop_front();
            self.stats.dropped_updates.fetch_add(1, Ordering::Relaxed);
        }

        queue.push_back(update);
        Ok(())
    }

    /// Get pending updates for a layer
    pub fn get_updates(&self, layer: &str) -> Vec<FeatureUpdate> {
        let mut updates = self.updates.write();

        if let Some(queue) = updates.get_mut(layer) {
            queue.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Get all pending updates
    pub fn get_all_updates(&self) -> HashMap<String, Vec<FeatureUpdate>> {
        let mut updates = self.updates.write();
        let mut result = HashMap::new();

        for (layer, queue) in updates.iter_mut() {
            result.insert(layer.clone(), queue.drain(..).collect());
        }

        result
    }

    /// Clear updates for a layer
    pub fn clear_layer(&self, layer: &str) {
        let mut updates = self.updates.write();
        updates.remove(layer);
    }

    /// Clear all updates
    pub fn clear_all(&self) {
        let mut updates = self.updates.write();
        updates.clear();
    }

    /// Get pending update count
    pub fn pending_count(&self) -> usize {
        let updates = self.updates.read();
        updates.values().map(|q| q.len()).sum()
    }

    /// Get layers with pending updates
    pub fn layers_with_updates(&self) -> Vec<String> {
        let updates = self.updates.read();
        updates
            .iter()
            .filter(|(_, q)| !q.is_empty())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Get statistics
    pub async fn stats(&self) -> FeatureUpdateManagerStats {
        FeatureUpdateManagerStats {
            total_updates: self.stats.total_updates.load(Ordering::Relaxed),
            created: self.stats.created.load(Ordering::Relaxed),
            updated: self.stats.updated.load(Ordering::Relaxed),
            deleted: self.stats.deleted.load(Ordering::Relaxed),
            dropped_updates: self.stats.dropped_updates.load(Ordering::Relaxed),
            pending_updates: self.pending_count(),
        }
    }
}

/// Feature update manager statistics
#[derive(Debug, Clone)]
pub struct FeatureUpdateManagerStats {
    /// Total updates
    pub total_updates: u64,
    /// Created features
    pub created: u64,
    /// Updated features
    pub updated: u64,
    /// Deleted features
    pub deleted: u64,
    /// Dropped updates
    pub dropped_updates: u64,
    /// Pending updates
    pub pending_updates: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_update() {
        let feature = serde_json::json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
            "properties": {}
        });

        let update = FeatureUpdate::created("f1".to_string(), "layer1".to_string(), feature);

        assert_eq!(update.id, "f1");
        assert_eq!(update.layer, "layer1");
        assert_eq!(update.update_type, FeatureUpdateType::Created);
    }

    #[test]
    fn test_feature_update_deleted() {
        let update = FeatureUpdate::deleted("f1".to_string(), "layer1".to_string());

        assert_eq!(update.update_type, FeatureUpdateType::Deleted);
        assert_eq!(update.feature, serde_json::Value::Null);
    }

    #[test]
    fn test_feature_update_manager() -> Result<()> {
        let manager = FeatureUpdateManager::new(10);
        let feature = serde_json::json!({"type": "Feature"});

        let update = FeatureUpdate::created("f1".to_string(), "layer1".to_string(), feature);

        manager.add_update(update)?;
        assert_eq!(manager.pending_count(), 1);

        let updates = manager.get_updates("layer1");
        assert_eq!(updates.len(), 1);
        assert_eq!(manager.pending_count(), 0);
        Ok(())
    }

    #[test]
    fn test_feature_update_layers() -> Result<()> {
        let manager = FeatureUpdateManager::new(10);
        let feature = serde_json::json!({"type": "Feature"});

        let update1 =
            FeatureUpdate::created("f1".to_string(), "layer1".to_string(), feature.clone());
        let update2 = FeatureUpdate::created("f2".to_string(), "layer2".to_string(), feature);

        manager.add_update(update1)?;
        manager.add_update(update2)?;

        let layers = manager.layers_with_updates();
        assert_eq!(layers.len(), 2);
        assert!(layers.contains(&"layer1".to_string()));
        assert!(layers.contains(&"layer2".to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn test_feature_update_stats() -> Result<()> {
        let manager = FeatureUpdateManager::new(10);
        let feature = serde_json::json!({"type": "Feature"});

        let created =
            FeatureUpdate::created("f1".to_string(), "layer1".to_string(), feature.clone());
        let updated = FeatureUpdate::updated("f2".to_string(), "layer1".to_string(), feature);
        let deleted = FeatureUpdate::deleted("f3".to_string(), "layer1".to_string());

        manager.add_update(created)?;
        manager.add_update(updated)?;
        manager.add_update(deleted)?;

        let stats = manager.stats().await;
        assert_eq!(stats.total_updates, 3);
        assert_eq!(stats.created, 1);
        assert_eq!(stats.updated, 1);
        assert_eq!(stats.deleted, 1);
        Ok(())
    }
}
