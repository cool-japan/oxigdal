//! Transformation operators for ETL pipelines
//!
//! This module provides various transformation operators including map, filter,
//! flatmap, reduce, groupby, and custom transformations.

use crate::error::{Result, TransformError};
use crate::stream::StreamItem;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;

/// Transformation trait
#[async_trait]
pub trait Transform: Send + Sync {
    /// Transform a single item
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>>;

    /// Get transform name for logging
    fn name(&self) -> &str;

    /// Check if this transform filters items
    fn is_filter(&self) -> bool {
        false
    }
}

/// Map transformation
pub struct MapTransform<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    name: String,
    mapper: F,
}

impl<F> MapTransform<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    /// Create a new map transform
    pub fn new(name: String, mapper: F) -> Self {
        Self { name, mapper }
    }
}

#[async_trait]
impl<F> Transform for MapTransform<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let result = (self.mapper)(item).await?;
        Ok(vec![result])
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Filter transformation
pub struct FilterTransform<F>
where
    F: Fn(&StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<bool>> + Send>> + Send + Sync,
{
    name: String,
    predicate: F,
}

impl<F> FilterTransform<F>
where
    F: Fn(&StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<bool>> + Send>> + Send + Sync,
{
    /// Create a new filter transform
    pub fn new(name: String, predicate: F) -> Self {
        Self { name, predicate }
    }
}

#[async_trait]
impl<F> Transform for FilterTransform<F>
where
    F: Fn(&StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<bool>> + Send>> + Send + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let should_keep = (self.predicate)(&item).await?;
        if should_keep {
            Ok(vec![item])
        } else {
            Ok(Vec::new())
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_filter(&self) -> bool {
        true
    }
}

/// FlatMap transformation (one-to-many)
pub struct FlatMapTransform<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    name: String,
    mapper: F,
}

impl<F> FlatMapTransform<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    /// Create a new flatmap transform
    pub fn new(name: String, mapper: F) -> Self {
        Self { name, mapper }
    }
}

#[async_trait]
impl<F> Transform for FlatMapTransform<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        (self.mapper)(item).await
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Batch transformation (collect N items and process together)
pub struct BatchTransform<F>
where
    F: Fn(
            Vec<StreamItem>,
        ) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    name: String,
    batch_size: usize,
    processor: F,
    buffer: tokio::sync::Mutex<Vec<StreamItem>>,
}

impl<F> BatchTransform<F>
where
    F: Fn(
            Vec<StreamItem>,
        ) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    /// Create a new batch transform
    pub fn new(name: String, batch_size: usize, processor: F) -> Self {
        Self {
            name,
            batch_size,
            processor,
            buffer: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Flush the buffer
    pub async fn flush(&self) -> Result<Vec<StreamItem>> {
        let mut buffer = self.buffer.lock().await;
        if buffer.is_empty() {
            return Ok(Vec::new());
        }

        let batch = buffer.drain(..).collect();
        (self.processor)(batch).await
    }
}

#[async_trait]
impl<F> Transform for BatchTransform<F>
where
    F: Fn(
            Vec<StreamItem>,
        ) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let mut buffer = self.buffer.lock().await;
        buffer.push(item);

        if buffer.len() >= self.batch_size {
            let batch = buffer.drain(..).collect();
            drop(buffer);
            (self.processor)(batch).await
        } else {
            Ok(Vec::new())
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// GroupBy key extractor trait
#[async_trait]
pub trait KeyExtractor: Send + Sync {
    /// Extract key from item
    async fn extract_key(&self, item: &StreamItem) -> Result<String>;
}

/// GroupBy transformation
pub struct GroupByTransform<K, F>
where
    K: KeyExtractor,
    F: Fn(
            String,
            Vec<StreamItem>,
        ) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    name: String,
    key_extractor: K,
    aggregator: F,
    state: tokio::sync::Mutex<GroupByState>,
    max_groups: usize,
}

/// Internal state for [`GroupByTransform`]: the per-key buffered items plus an
/// explicit FIFO of group-creation order.
///
/// `std::collections::HashMap` has no defined iteration order (it is
/// SipHash-randomized), so `groups.iter().next()` cannot be used to find "the
/// oldest" group -- it picks an arbitrary hash-bucket-order entry instead. `order`
/// tracks the sequence in which groups were first created (oldest at the front)
/// so overflow eviction can flush the group that has genuinely been buffering
/// the longest.
struct GroupByState {
    groups: HashMap<String, Vec<StreamItem>>,
    /// Group keys in first-insertion order; kept in lockstep with `groups` (a
    /// key appears here exactly once, for as long as it exists in `groups`).
    order: VecDeque<String>,
}

impl GroupByState {
    fn new() -> Self {
        Self {
            groups: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl<K, F> GroupByTransform<K, F>
where
    K: KeyExtractor,
    F: Fn(
            String,
            Vec<StreamItem>,
        ) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    /// Create a new groupby transform
    pub fn new(name: String, key_extractor: K, aggregator: F) -> Self {
        Self {
            name,
            key_extractor,
            aggregator,
            state: tokio::sync::Mutex::new(GroupByState::new()),
            max_groups: 1000,
        }
    }

    /// Set maximum number of groups
    pub fn max_groups(mut self, max: usize) -> Self {
        self.max_groups = max;
        self
    }

    /// Flush all groups
    pub async fn flush(&self) -> Result<Vec<StreamItem>> {
        let mut state = self.state.lock().await;
        let mut results = Vec::new();

        state.order.clear();
        for (key, items) in state.groups.drain() {
            let group_results = (self.aggregator)(key, items).await?;
            results.extend(group_results);
        }

        Ok(results)
    }
}

#[async_trait]
impl<K, F> Transform for GroupByTransform<K, F>
where
    K: KeyExtractor,
    F: Fn(
            String,
            Vec<StreamItem>,
        ) -> Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let key = self.key_extractor.extract_key(&item).await?;
        let mut state = self.state.lock().await;

        if !state.groups.contains_key(&key) {
            state.order.push_back(key.clone());
        }
        state.groups.entry(key.clone()).or_default().push(item);

        // If we have too many groups, flush the genuinely oldest one (the one
        // that has been buffering the longest), tracked explicitly via `order`
        // rather than relying on `HashMap`'s unspecified iteration order.
        if state.groups.len() > self.max_groups
            && let Some(old_key) = state.order.pop_front()
            && let Some(old_items) = state.groups.remove(&old_key)
        {
            drop(state);
            return (self.aggregator)(old_key, old_items).await;
        }

        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Reduce transformation (aggregation)
pub struct ReduceTransform<F>
where
    F: Fn(
            StreamItem,
            StreamItem,
        ) -> Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    name: String,
    reducer: F,
    accumulator: tokio::sync::Mutex<Option<StreamItem>>,
}

impl<F> ReduceTransform<F>
where
    F: Fn(
            StreamItem,
            StreamItem,
        ) -> Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    /// Create a new reduce transform
    pub fn new(name: String, reducer: F) -> Self {
        Self {
            name,
            reducer,
            accumulator: tokio::sync::Mutex::new(None),
        }
    }

    /// Get the final accumulated value
    pub async fn finalize(&self) -> Result<Option<StreamItem>> {
        let mut acc = self.accumulator.lock().await;
        Ok(acc.take())
    }
}

#[async_trait]
impl<F> Transform for ReduceTransform<F>
where
    F: Fn(
            StreamItem,
            StreamItem,
        ) -> Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let mut acc = self.accumulator.lock().await;

        if let Some(current) = acc.take() {
            let new_acc = (self.reducer)(current, item).await?;
            *acc = Some(new_acc);
        } else {
            *acc = Some(item);
        }

        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// JSON transformation helper
pub struct JsonTransform {
    name: String,
}

impl JsonTransform {
    /// Create a new JSON transform
    pub fn new(name: String) -> Self {
        Self { name }
    }

    /// Parse JSON from bytes
    pub async fn parse(&self, item: StreamItem) -> Result<serde_json::Value> {
        serde_json::from_slice(&item).map_err(|e| {
            TransformError::InvalidInput {
                message: format!("Failed to parse JSON: {}", e),
            }
            .into()
        })
    }

    /// Serialize JSON to bytes
    pub async fn serialize(&self, value: &serde_json::Value) -> Result<StreamItem> {
        serde_json::to_vec(value).map_err(|e| {
            TransformError::Failed {
                message: format!("Failed to serialize JSON: {}", e),
            }
            .into()
        })
    }
}

#[async_trait]
impl Transform for JsonTransform {
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        // Just validate JSON
        let _value = self.parse(item.clone()).await?;
        Ok(vec![item])
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Chain multiple transforms
pub struct ChainTransform {
    name: String,
    transforms: Vec<Box<dyn Transform>>,
}

impl ChainTransform {
    /// Create a new chain transform
    pub fn new(name: String) -> Self {
        Self {
            name,
            transforms: Vec::new(),
        }
    }

    /// Add a transform to the chain
    pub fn with_transform(mut self, transform: Box<dyn Transform>) -> Self {
        self.transforms.push(transform);
        self
    }
}

#[async_trait]
impl Transform for ChainTransform {
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let mut items = vec![item];

        for transform in &self.transforms {
            let mut new_items = Vec::new();
            for item in items {
                let results = transform.transform(item).await?;
                new_items.extend(results);
            }
            items = new_items;
        }

        Ok(items)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_map_transform() {
        let transform = MapTransform::new("double".to_string(), |item| {
            Box::pin(async move {
                let mut result = item.clone();
                result.extend_from_slice(&item);
                Ok(result)
            })
        });

        let result = transform
            .transform(vec![1, 2, 3])
            .await
            .expect("Failed to transform");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], vec![1, 2, 3, 1, 2, 3]);
    }

    #[tokio::test]
    async fn test_filter_transform() {
        let transform = FilterTransform::new("even_length".to_string(), |item| {
            let len = item.len();
            Box::pin(async move { Ok(len % 2 == 0) })
        });

        let result1 = transform
            .transform(vec![1, 2])
            .await
            .expect("Failed to transform");
        assert_eq!(result1.len(), 1);

        let result2 = transform
            .transform(vec![1, 2, 3])
            .await
            .expect("Failed to transform");
        assert_eq!(result2.len(), 0);
    }

    #[tokio::test]
    async fn test_flatmap_transform() {
        let transform = FlatMapTransform::new("split".to_string(), |item| {
            Box::pin(async move {
                let results = item.iter().map(|&b| vec![b]).collect();
                Ok(results)
            })
        });

        let result = transform
            .transform(vec![1, 2, 3])
            .await
            .expect("Failed to transform");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], vec![1]);
        assert_eq!(result[1], vec![2]);
        assert_eq!(result[2], vec![3]);
    }

    #[tokio::test]
    async fn test_batch_transform() {
        let transform = BatchTransform::new("batch3".to_string(), 3, |batch| {
            Box::pin(async move {
                let sum: Vec<u8> = batch.iter().flatten().copied().collect();
                Ok(vec![sum])
            })
        });

        let result1 = transform.transform(vec![1]).await.expect("Failed");
        assert_eq!(result1.len(), 0); // Not enough for batch

        let result2 = transform.transform(vec![2]).await.expect("Failed");
        assert_eq!(result2.len(), 0);

        let result3 = transform.transform(vec![3]).await.expect("Failed");
        assert_eq!(result3.len(), 1); // Batch complete
        assert_eq!(result3[0], vec![1, 2, 3]);
    }

    /// A [`KeyExtractor`] that treats the item's first byte as the group key.
    struct FirstByteKey;

    #[async_trait]
    impl KeyExtractor for FirstByteKey {
        async fn extract_key(&self, item: &StreamItem) -> Result<String> {
            Ok(item
                .first()
                .map(|b| (*b as char).to_string())
                .unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn test_groupby_transform_evicts_oldest_group_first() {
        // Track eviction order via a shared log the aggregator appends to,
        // instead of relying on HashMap's unspecified iteration order.
        let evicted: std::sync::Arc<tokio::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let evicted_for_closure = evicted.clone();

        let transform =
            GroupByTransform::new("groupby".to_string(), FirstByteKey, move |key, items| {
                let evicted = evicted_for_closure.clone();
                Box::pin(async move {
                    evicted.lock().await.push(key);
                    Ok(items)
                })
            })
            .max_groups(3);

        // Create groups "a", "b", "c" in that order (3 distinct keys == max_groups,
        // no eviction yet).
        for key_byte in [b'a', b'b', b'c'] {
            let result = transform
                .transform(vec![key_byte])
                .await
                .expect("transform should not error while under max_groups");
            assert!(result.is_empty(), "no eviction expected yet");
        }
        assert!(evicted.lock().await.is_empty());

        // A 4th distinct key ("d") pushes the group count over max_groups (3):
        // the transform must evict the OLDEST group ("a", created first), not
        // an arbitrary hash-bucket-order group.
        let result = transform
            .transform(vec![b'd'])
            .await
            .expect("transform should evict on overflow, not error");
        assert_eq!(result, vec![vec![b'a']]);
        assert_eq!(evicted.lock().await.as_slice(), ["a".to_string()]);

        // A 5th distinct key ("e") must evict "b" next (the next-oldest
        // surviving group), continuing genuine FIFO order.
        let result = transform
            .transform(vec![b'e'])
            .await
            .expect("transform should evict on overflow, not error");
        assert_eq!(result, vec![vec![b'b']]);
        assert_eq!(
            evicted.lock().await.as_slice(),
            ["a".to_string(), "b".to_string()]
        );

        // Re-inserting into an existing, still-live group ("c") must not
        // re-trigger eviction or perturb order tracking.
        let result = transform
            .transform(vec![b'c'])
            .await
            .expect("transform should not error while at max_groups");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_json_transform() {
        let transform = JsonTransform::new("json".to_string());

        let json = serde_json::json!({"key": "value"});
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = transform.transform(item.clone()).await.expect("Failed");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], item);
    }
}
