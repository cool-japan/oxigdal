//! Field resolvers and DataLoader implementations.

use dashmap::DashMap;
use std::sync::Arc;

/// DataLoader for batching and caching.
pub struct DataLoader<K, V>
where
    K: std::hash::Hash + Eq + Clone,
    V: Clone,
{
    cache: Arc<DashMap<K, V>>,
}

impl<K, V> DataLoader<K, V>
where
    K: std::hash::Hash + Eq + Clone,
    V: Clone,
{
    /// Creates a new DataLoader.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
        }
    }

    /// Loads a value by key.
    pub async fn load(&self, key: K, loader: impl Fn(&K) -> V) -> V {
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }

        let value = loader(&key);
        self.cache.insert(key.clone(), value.clone());
        value
    }

    /// Loads multiple values by keys.
    pub async fn load_many(&self, keys: Vec<K>, loader: impl Fn(&[K]) -> Vec<V>) -> Vec<V> {
        let uncached_keys: Vec<K> = keys
            .iter()
            .filter(|k| !self.cache.contains_key(k))
            .cloned()
            .collect();

        if !uncached_keys.is_empty() {
            let values = loader(&uncached_keys);
            for (key, value) in uncached_keys.iter().zip(values.iter()) {
                self.cache.insert(key.clone(), value.clone());
            }
        }

        keys.iter()
            .filter_map(|k| self.cache.get(k).map(|v| v.clone()))
            .collect()
    }

    /// Clears the cache.
    pub fn clear(&self) {
        self.cache.clear();
    }
}

impl<K, V> Default for DataLoader<K, V>
where
    K: std::hash::Hash + Eq + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dataloader() {
        let loader = DataLoader::<String, String>::new();

        let value = loader
            .load("key1".to_string(), |k| format!("value_{}", k))
            .await;
        assert_eq!(value, "value_key1");

        // Second load should use cache
        let value2 = loader
            .load("key1".to_string(), |_k| "different".to_string())
            .await;
        assert_eq!(value2, "value_key1"); // Still cached value
    }
}
