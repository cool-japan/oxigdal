//! Transformation operators for ETL pipelines
//!
//! This module provides various transformation operators including map, filter,
//! flatmap, reduce, groupby, and custom transformations.

use crate::error::{Result, TransformError};
use crate::stream::StreamItem;
use async_trait::async_trait;
use std::collections::HashMap;
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
    groups: tokio::sync::Mutex<HashMap<String, Vec<StreamItem>>>,
    max_groups: usize,
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
            groups: tokio::sync::Mutex::new(HashMap::new()),
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
        let mut groups = self.groups.lock().await;
        let mut results = Vec::new();

        for (key, items) in groups.drain() {
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
        let mut groups = self.groups.lock().await;

        groups.entry(key.clone()).or_default().push(item);

        // If we have too many groups, flush the oldest one
        if groups.len() > self.max_groups {
            if let Some((old_key, old_items)) = groups.iter().next() {
                let old_key = old_key.clone();
                let old_items = old_items.clone();
                groups.remove(&old_key);
                drop(groups);
                return (self.aggregator)(old_key, old_items).await;
            }
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
