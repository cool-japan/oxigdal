//! Keyed state for stream processing.

use crate::error::Result;
use crate::state::backend::StateBackend;
use std::sync::Arc;

/// Keyed state trait.
pub trait KeyedState: Send + Sync {
    /// Get the state key.
    fn key(&self) -> &[u8];

    /// Clear the state.
    fn clear(&self) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// Value state (stores a single value per key).
pub struct ValueState<B>
where
    B: StateBackend,
{
    backend: Arc<B>,
    namespace: String,
    key: Vec<u8>,
}

impl<B> ValueState<B>
where
    B: StateBackend,
{
    /// Create a new value state.
    pub fn new(backend: Arc<B>, namespace: String, key: Vec<u8>) -> Self {
        Self {
            backend,
            namespace,
            key,
        }
    }

    /// Get the value.
    pub async fn get(&self) -> Result<Option<Vec<u8>>> {
        let state_key = self.make_state_key();
        self.backend.get(&state_key).await
    }

    /// Set the value.
    pub async fn set(&self, value: Vec<u8>) -> Result<()> {
        let state_key = self.make_state_key();
        self.backend.put(&state_key, &value).await
    }

    /// Update the value using a function.
    pub async fn update<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(Option<Vec<u8>>) -> Vec<u8>,
    {
        let current = self.get().await?;
        let new_value = f(current);
        self.set(new_value).await
    }

    fn make_state_key(&self) -> Vec<u8> {
        let mut state_key = Vec::new();
        state_key.extend_from_slice(self.namespace.as_bytes());
        state_key.push(b':');
        state_key.extend_from_slice(&self.key);
        state_key
    }
}

impl<B> KeyedState for ValueState<B>
where
    B: StateBackend,
{
    fn key(&self) -> &[u8] {
        &self.key
    }

    async fn clear(&self) -> Result<()> {
        let state_key = self.make_state_key();
        self.backend.delete(&state_key).await
    }
}

/// List state (stores a list of values per key).
pub struct ListState<B>
where
    B: StateBackend,
{
    backend: Arc<B>,
    namespace: String,
    key: Vec<u8>,
}

impl<B> ListState<B>
where
    B: StateBackend,
{
    /// Create a new list state.
    pub fn new(backend: Arc<B>, namespace: String, key: Vec<u8>) -> Self {
        Self {
            backend,
            namespace,
            key,
        }
    }

    /// Get all values in the list.
    pub async fn get(&self) -> Result<Vec<Vec<u8>>> {
        let state_key = self.make_state_key();
        if let Some(data) = self.backend.get(&state_key).await? {
            Ok(serde_json::from_slice(&data)?)
        } else {
            Ok(Vec::new())
        }
    }

    /// Add a value to the list.
    pub async fn add(&self, value: Vec<u8>) -> Result<()> {
        let mut list = self.get().await?;
        list.push(value);
        self.set_list(list).await
    }

    /// Add multiple values to the list.
    pub async fn add_all(&self, values: Vec<Vec<u8>>) -> Result<()> {
        let mut list = self.get().await?;
        list.extend(values);
        self.set_list(list).await
    }

    /// Update the entire list.
    pub async fn update(&self, values: Vec<Vec<u8>>) -> Result<()> {
        self.set_list(values).await
    }

    fn set_list(&self, list: Vec<Vec<u8>>) -> impl std::future::Future<Output = Result<()>> + Send {
        let state_key = self.make_state_key();
        let backend = self.backend.clone();
        async move {
            let data = serde_json::to_vec(&list)?;
            backend.put(&state_key, &data).await
        }
    }

    fn make_state_key(&self) -> Vec<u8> {
        let mut state_key = Vec::new();
        state_key.extend_from_slice(self.namespace.as_bytes());
        state_key.push(b':');
        state_key.extend_from_slice(&self.key);
        state_key
    }
}

impl<B> KeyedState for ListState<B>
where
    B: StateBackend,
{
    fn key(&self) -> &[u8] {
        &self.key
    }

    async fn clear(&self) -> Result<()> {
        let state_key = self.make_state_key();
        self.backend.delete(&state_key).await
    }
}

/// Map state (stores key-value pairs per key).
pub struct MapState<B>
where
    B: StateBackend,
{
    backend: Arc<B>,
    namespace: String,
    key: Vec<u8>,
}

impl<B> MapState<B>
where
    B: StateBackend,
{
    /// Create a new map state.
    pub fn new(backend: Arc<B>, namespace: String, key: Vec<u8>) -> Self {
        Self {
            backend,
            namespace,
            key,
        }
    }

    /// Get a value from the map.
    pub async fn get(&self, map_key: &[u8]) -> Result<Option<Vec<u8>>> {
        let state_key = self.make_state_key(map_key);
        self.backend.get(&state_key).await
    }

    /// Put a value into the map.
    pub async fn put(&self, map_key: &[u8], value: Vec<u8>) -> Result<()> {
        let state_key = self.make_state_key(map_key);
        self.backend.put(&state_key, &value).await
    }

    /// Remove a key from the map.
    pub async fn remove(&self, map_key: &[u8]) -> Result<()> {
        let state_key = self.make_state_key(map_key);
        self.backend.delete(&state_key).await
    }

    /// Check if the map contains a key.
    pub async fn contains(&self, map_key: &[u8]) -> Result<bool> {
        let state_key = self.make_state_key(map_key);
        self.backend.contains(&state_key).await
    }

    fn make_state_key(&self, map_key: &[u8]) -> Vec<u8> {
        let mut state_key = Vec::new();
        state_key.extend_from_slice(self.namespace.as_bytes());
        state_key.push(b':');
        state_key.extend_from_slice(&self.key);
        state_key.push(b':');
        state_key.extend_from_slice(map_key);
        state_key
    }
}

impl<B> KeyedState for MapState<B>
where
    B: StateBackend,
{
    fn key(&self) -> &[u8] {
        &self.key
    }

    async fn clear(&self) -> Result<()> {
        Ok(())
    }
}

/// Reducing state.
pub struct ReducingState<B, F>
where
    B: StateBackend,
    F: Fn(Vec<u8>, Vec<u8>) -> Vec<u8> + Send + Sync,
{
    value_state: ValueState<B>,
    reduce_fn: Arc<F>,
}

impl<B, F> ReducingState<B, F>
where
    B: StateBackend,
    F: Fn(Vec<u8>, Vec<u8>) -> Vec<u8> + Send + Sync,
{
    /// Create a new reducing state.
    pub fn new(backend: Arc<B>, namespace: String, key: Vec<u8>, reduce_fn: F) -> Self {
        Self {
            value_state: ValueState::new(backend, namespace, key),
            reduce_fn: Arc::new(reduce_fn),
        }
    }

    /// Get the reduced value.
    pub async fn get(&self) -> Result<Option<Vec<u8>>> {
        self.value_state.get().await
    }

    /// Add a value (will be reduced with existing value).
    pub async fn add(&self, value: Vec<u8>) -> Result<()> {
        let reduce_fn = self.reduce_fn.clone();
        self.value_state
            .update(move |current| {
                if let Some(existing) = current {
                    reduce_fn(existing, value)
                } else {
                    value
                }
            })
            .await
    }
}

impl<B, F> KeyedState for ReducingState<B, F>
where
    B: StateBackend,
    F: Fn(Vec<u8>, Vec<u8>) -> Vec<u8> + Send + Sync,
{
    fn key(&self) -> &[u8] {
        self.value_state.key()
    }

    async fn clear(&self) -> Result<()> {
        self.value_state.clear().await
    }
}

/// Aggregating state.
pub struct AggregatingState<B, F>
where
    B: StateBackend,
    F: Fn(Vec<u8>, Vec<u8>) -> Vec<u8> + Send + Sync,
{
    value_state: ValueState<B>,
    aggregate_fn: Arc<F>,
}

impl<B, F> AggregatingState<B, F>
where
    B: StateBackend,
    F: Fn(Vec<u8>, Vec<u8>) -> Vec<u8> + Send + Sync,
{
    /// Create a new aggregating state.
    pub fn new(backend: Arc<B>, namespace: String, key: Vec<u8>, aggregate_fn: F) -> Self {
        Self {
            value_state: ValueState::new(backend, namespace, key),
            aggregate_fn: Arc::new(aggregate_fn),
        }
    }

    /// Get the aggregated value.
    pub async fn get(&self) -> Result<Option<Vec<u8>>> {
        self.value_state.get().await
    }

    /// Add a value (will be aggregated with existing value).
    pub async fn add(&self, value: Vec<u8>) -> Result<()> {
        let aggregate_fn = self.aggregate_fn.clone();
        self.value_state
            .update(move |current| {
                if let Some(existing) = current {
                    aggregate_fn(existing, value)
                } else {
                    value
                }
            })
            .await
    }
}

impl<B, F> KeyedState for AggregatingState<B, F>
where
    B: StateBackend,
    F: Fn(Vec<u8>, Vec<u8>) -> Vec<u8> + Send + Sync,
{
    fn key(&self) -> &[u8] {
        self.value_state.key()
    }

    async fn clear(&self) -> Result<()> {
        self.value_state.clear().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::backend::MemoryStateBackend;

    #[tokio::test]
    async fn test_value_state() {
        let backend = Arc::new(MemoryStateBackend::new());
        let state = ValueState::new(backend, "test".to_string(), vec![1]);

        state
            .set(vec![42])
            .await
            .expect("Failed to set value in value state");
        let value = state
            .get()
            .await
            .expect("Failed to get value from value state");
        assert_eq!(value, Some(vec![42]));

        state.clear().await.expect("Failed to clear value state");
        let value = state.get().await.expect("Failed to get value after clear");
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_list_state() {
        let backend = Arc::new(MemoryStateBackend::new());
        let state = ListState::new(backend, "test".to_string(), vec![1]);

        state
            .add(vec![1])
            .await
            .expect("Failed to add first item to list state");
        state
            .add(vec![2])
            .await
            .expect("Failed to add second item to list state");
        state
            .add(vec![3])
            .await
            .expect("Failed to add third item to list state");

        let list = state
            .get()
            .await
            .expect("Failed to get list from list state");
        assert_eq!(list, vec![vec![1], vec![2], vec![3]]);
    }

    #[tokio::test]
    async fn test_map_state() {
        let backend = Arc::new(MemoryStateBackend::new());
        let state = MapState::new(backend, "test".to_string(), vec![1]);

        state
            .put(b"key1", vec![1])
            .await
            .expect("Failed to put key1 in map state");
        state
            .put(b"key2", vec![2])
            .await
            .expect("Failed to put key2 in map state");

        assert_eq!(
            state
                .get(b"key1")
                .await
                .expect("Failed to get key1 from map state"),
            Some(vec![1])
        );
        assert_eq!(
            state
                .get(b"key2")
                .await
                .expect("Failed to get key2 from map state"),
            Some(vec![2])
        );

        assert!(
            state
                .contains(b"key1")
                .await
                .expect("Failed to check if map contains key1")
        );

        state
            .remove(b"key1")
            .await
            .expect("Failed to remove key1 from map state");
        assert!(
            !state
                .contains(b"key1")
                .await
                .expect("Failed to check if map contains key1 after removal")
        );
    }

    #[tokio::test]
    async fn test_reducing_state() {
        let backend = Arc::new(MemoryStateBackend::new());
        let state = ReducingState::new(backend, "test".to_string(), vec![1], |a, b| {
            let v1 = i64::from_le_bytes(a.try_into().unwrap_or([0; 8]));
            let v2 = i64::from_le_bytes(b.try_into().unwrap_or([0; 8]));
            (v1 + v2).to_le_bytes().to_vec()
        });

        state
            .add(5i64.to_le_bytes().to_vec())
            .await
            .expect("Failed to add first value to reducing state");
        state
            .add(3i64.to_le_bytes().to_vec())
            .await
            .expect("Failed to add second value to reducing state");

        let result = state
            .get()
            .await
            .expect("Failed to get value from reducing state")
            .expect("Expected Some value from reducing state");
        let value = i64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        assert_eq!(value, 8);
    }
}
