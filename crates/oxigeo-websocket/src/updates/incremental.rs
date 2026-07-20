//! Incremental update delivery and delta encoding

use crate::error::Result;
use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Update delta representation
#[derive(Debug, Clone)]
pub struct UpdateDelta {
    /// Base version
    pub base_version: u64,
    /// Target version
    pub target_version: u64,
    /// Delta data
    pub delta: Bytes,
    /// Delta encoding type
    pub encoding: DeltaEncoding,
}

/// Delta encoding type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaEncoding {
    /// Binary diff
    BinaryDiff,
    /// JSON patch (RFC 6902)
    JsonPatch,
    /// Custom encoding
    Custom,
}

impl UpdateDelta {
    /// Create a new update delta
    pub fn new(
        base_version: u64,
        target_version: u64,
        delta: Bytes,
        encoding: DeltaEncoding,
    ) -> Self {
        Self {
            base_version,
            target_version,
            delta,
            encoding,
        }
    }

    /// Get delta size
    pub fn size(&self) -> usize {
        self.delta.len()
    }
}

/// Incremental update
pub struct IncrementalUpdate {
    /// Entity ID
    pub entity_id: String,
    /// Current version
    pub version: u64,
    /// Full data
    pub full_data: Option<Bytes>,
    /// Available deltas (indexed by target version)
    pub deltas: HashMap<u64, UpdateDelta>,
}

impl IncrementalUpdate {
    /// Create a new incremental update
    pub fn new(entity_id: String, version: u64, full_data: Option<Bytes>) -> Self {
        Self {
            entity_id,
            version,
            full_data,
            deltas: HashMap::new(),
        }
    }

    /// Add a delta
    pub fn add_delta(&mut self, delta: UpdateDelta) {
        self.deltas.insert(delta.target_version, delta);
    }

    /// Get delta to specific version
    pub fn get_delta(&self, target_version: u64) -> Option<&UpdateDelta> {
        self.deltas.get(&target_version)
    }

    /// Get delta chain from base to target version
    pub fn get_delta_chain(&self, from_version: u64, to_version: u64) -> Option<Vec<&UpdateDelta>> {
        let mut chain = Vec::new();
        let mut current_version = from_version;

        while current_version < to_version {
            let next_version = current_version + 1;
            if let Some(delta) = self.deltas.get(&next_version) {
                if delta.base_version == current_version {
                    chain.push(delta);
                    current_version = next_version;
                } else {
                    return None; // Chain broken
                }
            } else {
                return None; // Missing delta
            }
        }

        if current_version == to_version {
            Some(chain)
        } else {
            None
        }
    }

    /// Check if full data is available
    pub fn has_full_data(&self) -> bool {
        self.full_data.is_some()
    }

    /// Get full data size
    pub fn full_data_size(&self) -> usize {
        self.full_data.as_ref().map_or(0, |d| d.len())
    }

    /// Get total delta size
    pub fn total_delta_size(&self) -> usize {
        self.deltas.values().map(|d| d.size()).sum()
    }
}

/// Incremental update manager
pub struct IncrementalUpdateManager {
    updates: Arc<RwLock<HashMap<String, IncrementalUpdate>>>,
}

impl IncrementalUpdateManager {
    /// Create a new incremental update manager
    pub fn new() -> Self {
        Self {
            updates: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an entity for incremental updates
    pub fn register(
        &self,
        entity_id: String,
        version: u64,
        full_data: Option<Bytes>,
    ) -> Result<()> {
        let mut updates = self.updates.write();
        updates.insert(
            entity_id.clone(),
            IncrementalUpdate::new(entity_id, version, full_data),
        );
        Ok(())
    }

    /// Add a delta for an entity
    pub fn add_delta(&self, entity_id: &str, delta: UpdateDelta) -> Result<()> {
        let mut updates = self.updates.write();

        if let Some(update) = updates.get_mut(entity_id) {
            let target_version = delta.target_version;
            update.add_delta(delta);
            update.version = target_version;
            Ok(())
        } else {
            Err(crate::error::Error::InvalidState(format!(
                "Entity {} not registered",
                entity_id
            )))
        }
    }

    /// Get full data or delta for an entity
    pub fn get_update(&self, entity_id: &str, client_version: u64) -> Option<UpdateResponse> {
        let updates = self.updates.read();

        if let Some(update) = updates.get(entity_id) {
            // If client is up to date, no update needed
            if client_version >= update.version {
                return Some(UpdateResponse::NoUpdate);
            }

            // Try to get delta chain
            if let Some(chain) = update.get_delta_chain(client_version, update.version) {
                let total_delta_size: usize = chain.iter().map(|d| d.size()).sum();

                // If delta chain is available and smaller than full data, use it
                if let Some(full_data) = &update.full_data {
                    if total_delta_size < full_data.len() {
                        return Some(UpdateResponse::DeltaChain(
                            chain.into_iter().cloned().collect(),
                        ));
                    }
                } else {
                    return Some(UpdateResponse::DeltaChain(
                        chain.into_iter().cloned().collect(),
                    ));
                }
            }

            // Fall back to full data if available
            if let Some(full_data) = &update.full_data {
                Some(UpdateResponse::FullData(full_data.clone(), update.version))
            } else {
                Some(UpdateResponse::NoData)
            }
        } else {
            None
        }
    }

    /// Remove an entity
    pub fn remove(&self, entity_id: &str) -> Option<IncrementalUpdate> {
        let mut updates = self.updates.write();
        updates.remove(entity_id)
    }

    /// Get entity count
    pub fn entity_count(&self) -> usize {
        self.updates.read().len()
    }

    /// Get total delta count
    pub fn total_delta_count(&self) -> usize {
        let updates = self.updates.read();
        updates.values().map(|u| u.deltas.len()).sum()
    }

    /// Get statistics
    pub fn stats(&self) -> IncrementalUpdateStats {
        let updates = self.updates.read();
        let mut total_full_size = 0;
        let mut total_delta_size = 0;
        let mut total_deltas = 0;

        for update in updates.values() {
            total_full_size += update.full_data_size();
            total_delta_size += update.total_delta_size();
            total_deltas += update.deltas.len();
        }

        IncrementalUpdateStats {
            entity_count: updates.len(),
            total_deltas,
            total_full_size,
            total_delta_size,
        }
    }
}

impl Default for IncrementalUpdateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Update response
#[derive(Debug, Clone)]
pub enum UpdateResponse {
    /// No update needed (client is up to date)
    NoUpdate,
    /// Full data response
    FullData(Bytes, u64),
    /// Delta chain response
    DeltaChain(Vec<UpdateDelta>),
    /// No data available
    NoData,
}

/// Incremental update statistics
#[derive(Debug, Clone)]
pub struct IncrementalUpdateStats {
    /// Number of entities
    pub entity_count: usize,
    /// Total deltas
    pub total_deltas: usize,
    /// Total full data size
    pub total_full_size: usize,
    /// Total delta size
    pub total_delta_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_delta() {
        let delta = UpdateDelta::new(1, 2, Bytes::from(vec![1, 2, 3]), DeltaEncoding::BinaryDiff);

        assert_eq!(delta.base_version, 1);
        assert_eq!(delta.target_version, 2);
        assert_eq!(delta.size(), 3);
    }

    #[test]
    fn test_incremental_update() {
        let update = IncrementalUpdate::new("entity1".to_string(), 1, None);

        assert_eq!(update.entity_id, "entity1");
        assert_eq!(update.version, 1);
        assert!(!update.has_full_data());
    }

    #[test]
    fn test_incremental_update_add_delta() {
        let mut update = IncrementalUpdate::new("entity1".to_string(), 1, None);

        let delta = UpdateDelta::new(1, 2, Bytes::from(vec![1, 2, 3]), DeltaEncoding::BinaryDiff);
        update.add_delta(delta);

        assert_eq!(update.deltas.len(), 1);
        assert!(update.get_delta(2).is_some());
    }

    #[test]
    fn test_incremental_update_delta_chain() {
        let mut update = IncrementalUpdate::new("entity1".to_string(), 1, None);

        let delta1 = UpdateDelta::new(1, 2, Bytes::from(vec![1]), DeltaEncoding::BinaryDiff);
        let delta2 = UpdateDelta::new(2, 3, Bytes::from(vec![2]), DeltaEncoding::BinaryDiff);
        let delta3 = UpdateDelta::new(3, 4, Bytes::from(vec![3]), DeltaEncoding::BinaryDiff);

        update.add_delta(delta1);
        update.add_delta(delta2);
        update.add_delta(delta3);

        let chain = update.get_delta_chain(1, 4);
        assert!(chain.is_some());
        assert_eq!(chain.as_ref().map(|c| c.len()), Some(3));
    }

    #[test]
    fn test_incremental_update_manager() -> Result<()> {
        let manager = IncrementalUpdateManager::new();

        manager.register("entity1".to_string(), 1, Some(Bytes::from(vec![1, 2, 3])))?;

        assert_eq!(manager.entity_count(), 1);
        Ok(())
    }

    #[test]
    fn test_incremental_update_manager_delta() -> Result<()> {
        let manager = IncrementalUpdateManager::new();

        manager.register("entity1".to_string(), 1, Some(Bytes::from(vec![1, 2, 3])))?;

        let delta = UpdateDelta::new(1, 2, Bytes::from(vec![4, 5]), DeltaEncoding::BinaryDiff);
        manager.add_delta("entity1", delta)?;

        assert_eq!(manager.total_delta_count(), 1);
        Ok(())
    }

    #[test]
    fn test_incremental_update_manager_get_update() -> Result<()> {
        let manager = IncrementalUpdateManager::new();

        manager.register("entity1".to_string(), 2, Some(Bytes::from(vec![1, 2, 3])))?;

        let delta = UpdateDelta::new(1, 2, Bytes::from(vec![4, 5]), DeltaEncoding::BinaryDiff);
        manager.add_delta("entity1", delta)?;

        // Client at version 1 should get delta
        let response = manager.get_update("entity1", 1);
        assert!(matches!(response, Some(UpdateResponse::DeltaChain(_))));

        // Client at version 2 should get no update
        let response = manager.get_update("entity1", 2);
        assert!(matches!(response, Some(UpdateResponse::NoUpdate)));

        Ok(())
    }
}
