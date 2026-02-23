//! Operator state for stream processing.

use crate::error::{Result, StreamingError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Operator state trait.
pub trait OperatorState: Send + Sync {
    /// Snapshot the state.
    fn snapshot(&self) -> impl std::future::Future<Output = Result<Vec<u8>>> + Send;

    /// Restore from a snapshot.
    fn restore(&self, snapshot: &[u8]) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// Broadcast state (shared across all parallel instances).
pub struct BroadcastState {
    state: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl BroadcastState {
    /// Create a new broadcast state.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get a value.
    pub async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.state.read().await.get(key).cloned()
    }

    /// Put a value.
    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.state.write().await.insert(key, value);
    }

    /// Remove a value.
    pub async fn remove(&self, key: &[u8]) {
        self.state.write().await.remove(key);
    }

    /// Check if a key exists.
    pub async fn contains(&self, key: &[u8]) -> bool {
        self.state.read().await.contains_key(key)
    }

    /// Clear all state.
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }

    /// Get all keys.
    pub async fn keys(&self) -> Vec<Vec<u8>> {
        self.state.read().await.keys().cloned().collect()
    }
}

impl Default for BroadcastState {
    fn default() -> Self {
        Self::new()
    }
}

impl OperatorState for BroadcastState {
    async fn snapshot(&self) -> Result<Vec<u8>> {
        let state = self.state.read().await;
        // Use oxicode for binary serialization since JSON requires string keys
        oxicode::encode_to_vec(&*state)
            .map_err(|e| StreamingError::SerializationError(e.to_string()))
    }

    async fn restore(&self, snapshot: &[u8]) -> Result<()> {
        let (restored, _): (HashMap<Vec<u8>, Vec<u8>>, _) = oxicode::decode_from_slice(snapshot)
            .map_err(|e| StreamingError::SerializationError(e.to_string()))?;
        *self.state.write().await = restored;
        Ok(())
    }
}

/// Union list state (list that is distributed across parallel instances).
pub struct UnionListState {
    state: Arc<RwLock<Vec<Vec<u8>>>>,
}

impl UnionListState {
    /// Create a new union list state.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get all values.
    pub async fn get(&self) -> Vec<Vec<u8>> {
        self.state.read().await.clone()
    }

    /// Add a value.
    pub async fn add(&self, value: Vec<u8>) {
        self.state.write().await.push(value);
    }

    /// Add multiple values.
    pub async fn add_all(&self, values: Vec<Vec<u8>>) {
        self.state.write().await.extend(values);
    }

    /// Update with new values.
    pub async fn update(&self, values: Vec<Vec<u8>>) {
        *self.state.write().await = values;
    }

    /// Clear all values.
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }

    /// Get the number of values.
    pub async fn len(&self) -> usize {
        self.state.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.state.read().await.is_empty()
    }
}

impl Default for UnionListState {
    fn default() -> Self {
        Self::new()
    }
}

impl OperatorState for UnionListState {
    async fn snapshot(&self) -> Result<Vec<u8>> {
        let state = self.state.read().await;
        Ok(serde_json::to_vec(&*state)?)
    }

    async fn restore(&self, snapshot: &[u8]) -> Result<()> {
        let restored: Vec<Vec<u8>> = serde_json::from_slice(snapshot)?;
        *self.state.write().await = restored;
        Ok(())
    }
}

/// Trait for checkpointable list state.
pub trait ListCheckpointed {
    /// Get the current state as a list.
    fn snapshot_state(&self) -> impl std::future::Future<Output = Vec<Vec<u8>>> + Send;

    /// Restore state from a list.
    fn restore_state(
        &self,
        state: Vec<Vec<u8>>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_broadcast_state() {
        let state = BroadcastState::new();

        state.put(vec![1], vec![42]).await;
        assert_eq!(state.get(&[1]).await, Some(vec![42]));

        assert!(state.contains(&[1]).await);
        assert!(!state.contains(&[2]).await);

        state.remove(&[1]).await;
        assert_eq!(state.get(&[1]).await, None);
    }

    #[tokio::test]
    async fn test_broadcast_state_snapshot() {
        let state = BroadcastState::new();

        state.put(vec![1], vec![42]).await;
        state.put(vec![2], vec![43]).await;

        let snapshot = state
            .snapshot()
            .await
            .expect("Failed to create snapshot of broadcast state");

        let state2 = BroadcastState::new();
        state2
            .restore(&snapshot)
            .await
            .expect("Failed to restore broadcast state from snapshot");

        assert_eq!(state2.get(&[1]).await, Some(vec![42]));
        assert_eq!(state2.get(&[2]).await, Some(vec![43]));
    }

    #[tokio::test]
    async fn test_union_list_state() {
        let state = UnionListState::new();

        state.add(vec![1]).await;
        state.add(vec![2]).await;
        state.add(vec![3]).await;

        let values = state.get().await;
        assert_eq!(values, vec![vec![1], vec![2], vec![3]]);

        assert_eq!(state.len().await, 3);
        assert!(!state.is_empty().await);
    }

    #[tokio::test]
    async fn test_union_list_state_snapshot() {
        let state = UnionListState::new();

        state.add(vec![1]).await;
        state.add(vec![2]).await;

        let snapshot = state
            .snapshot()
            .await
            .expect("Failed to create snapshot of union list state");

        let state2 = UnionListState::new();
        state2
            .restore(&snapshot)
            .await
            .expect("Failed to restore union list state from snapshot");

        assert_eq!(state2.get().await, vec![vec![1], vec![2]]);
    }
}
