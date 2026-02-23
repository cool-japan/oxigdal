//! Join operator for combining streams
//!
//! This module provides join operators for combining data from multiple sources.

use crate::error::{Result, TransformError};
use crate::stream::StreamItem;
use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Join type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    /// Inner join (only matching items)
    Inner,
    /// Left join (all items from left stream)
    Left,
    /// Right join (all items from right stream)
    Right,
    /// Full outer join (all items from both streams)
    Full,
}

/// Join key extractor trait
#[async_trait]
pub trait JoinKeyExtractor: Send + Sync {
    /// Extract join key from item
    async fn extract_key(&self, item: &StreamItem) -> Result<String>;
}

/// Simple JSON field key extractor
pub struct JsonFieldExtractor {
    field: String,
}

impl JsonFieldExtractor {
    /// Create a new JSON field extractor
    pub fn new(field: String) -> Self {
        Self { field }
    }
}

#[async_trait]
impl JoinKeyExtractor for JsonFieldExtractor {
    async fn extract_key(&self, item: &StreamItem) -> Result<String> {
        let json: serde_json::Value =
            serde_json::from_slice(item).map_err(|e| TransformError::InvalidInput {
                message: format!("Failed to parse JSON: {}", e),
            })?;

        let key = json
            .get(&self.field)
            .ok_or_else(|| TransformError::MissingField {
                field: self.field.clone(),
            })?
            .to_string();

        Ok(key)
    }
}

/// Join state for tracking items from both sides
struct JoinState {
    left_items: DashMap<String, Vec<StreamItem>>,
    right_items: DashMap<String, Vec<StreamItem>>,
    left_timestamps: DashMap<String, SystemTime>,
    right_timestamps: DashMap<String, SystemTime>,
}

impl JoinState {
    fn new() -> Self {
        Self {
            left_items: DashMap::new(),
            right_items: DashMap::new(),
            left_timestamps: DashMap::new(),
            right_timestamps: DashMap::new(),
        }
    }

    /// Add item to left side
    fn add_left(&self, key: String, item: StreamItem) {
        self.left_items.entry(key.clone()).or_default().push(item);
        self.left_timestamps.insert(key, SystemTime::now());
    }

    /// Add item to right side
    fn add_right(&self, key: String, item: StreamItem) {
        self.right_items.entry(key.clone()).or_default().push(item);
        self.right_timestamps.insert(key, SystemTime::now());
    }

    /// Get matching items for a key
    fn get_matches(&self, key: &str) -> (Option<Vec<StreamItem>>, Option<Vec<StreamItem>>) {
        let left = self.left_items.get(key).map(|v| v.clone());
        let right = self.right_items.get(key).map(|v| v.clone());
        (left, right)
    }

    /// Clean up old entries
    fn cleanup(&self, max_age: Duration) {
        let now = SystemTime::now();

        // Clean left side
        let mut to_remove = Vec::new();
        for entry in self.left_timestamps.iter() {
            if let Ok(age) = now.duration_since(*entry.value()) {
                if age > max_age {
                    to_remove.push(entry.key().clone());
                }
            }
        }
        for key in to_remove {
            self.left_items.remove(&key);
            self.left_timestamps.remove(&key);
        }

        // Clean right side
        let mut to_remove = Vec::new();
        for entry in self.right_timestamps.iter() {
            if let Ok(age) = now.duration_since(*entry.value()) {
                if age > max_age {
                    to_remove.push(entry.key().clone());
                }
            }
        }
        for key in to_remove {
            self.right_items.remove(&key);
            self.right_timestamps.remove(&key);
        }
    }
}

/// Stream-to-stream join operator
pub struct JoinOperator<L, R, F>
where
    L: JoinKeyExtractor,
    R: JoinKeyExtractor,
    F: Fn(
            Option<StreamItem>,
            Option<StreamItem>,
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Option<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    name: String,
    join_type: JoinType,
    left_extractor: L,
    right_extractor: R,
    joiner: F,
    state: Arc<JoinState>,
    max_age: Duration,
    last_cleanup: tokio::sync::Mutex<SystemTime>,
}

impl<L, R, F> JoinOperator<L, R, F>
where
    L: JoinKeyExtractor,
    R: JoinKeyExtractor,
    F: Fn(
            Option<StreamItem>,
            Option<StreamItem>,
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Option<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    /// Create a new join operator
    pub fn new(
        name: String,
        join_type: JoinType,
        left_extractor: L,
        right_extractor: R,
        joiner: F,
    ) -> Self {
        Self {
            name,
            join_type,
            left_extractor,
            right_extractor,
            joiner,
            state: Arc::new(JoinState::new()),
            max_age: Duration::from_secs(300), // 5 minutes default
            last_cleanup: tokio::sync::Mutex::new(SystemTime::now()),
        }
    }

    /// Set maximum age for cached items
    pub fn max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age;
        self
    }

    /// Process left stream item
    pub async fn process_left(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let key = self.left_extractor.extract_key(&item).await?;
        self.state.add_left(key.clone(), item);

        self.maybe_cleanup().await;

        // Check for matches on right side
        let (left_items, right_items) = self.state.get_matches(&key);

        self.perform_join(left_items, right_items).await
    }

    /// Process right stream item
    pub async fn process_right(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let key = self.right_extractor.extract_key(&item).await?;
        self.state.add_right(key.clone(), item);

        self.maybe_cleanup().await;

        // Check for matches on left side
        let (left_items, right_items) = self.state.get_matches(&key);

        self.perform_join(left_items, right_items).await
    }

    /// Perform the join based on join type
    async fn perform_join(
        &self,
        left_items: Option<Vec<StreamItem>>,
        right_items: Option<Vec<StreamItem>>,
    ) -> Result<Vec<StreamItem>> {
        let mut results = Vec::new();

        match self.join_type {
            JoinType::Inner => {
                if let (Some(left), Some(right)) = (&left_items, &right_items) {
                    for l in left {
                        for r in right {
                            if let Some(joined) =
                                (self.joiner)(Some(l.clone()), Some(r.clone())).await?
                            {
                                results.push(joined);
                            }
                        }
                    }
                }
            }
            JoinType::Left => {
                if let Some(left) = &left_items {
                    for l in left {
                        if let Some(right) = &right_items {
                            for r in right {
                                if let Some(joined) =
                                    (self.joiner)(Some(l.clone()), Some(r.clone())).await?
                                {
                                    results.push(joined);
                                }
                            }
                        } else if let Some(joined) = (self.joiner)(Some(l.clone()), None).await? {
                            results.push(joined);
                        }
                    }
                }
            }
            JoinType::Right => {
                if let Some(right) = &right_items {
                    for r in right {
                        if let Some(left) = &left_items {
                            for l in left {
                                if let Some(joined) =
                                    (self.joiner)(Some(l.clone()), Some(r.clone())).await?
                                {
                                    results.push(joined);
                                }
                            }
                        } else if let Some(joined) = (self.joiner)(None, Some(r.clone())).await? {
                            results.push(joined);
                        }
                    }
                }
            }
            JoinType::Full => {
                // Full outer join
                match (&left_items, &right_items) {
                    (Some(left), Some(right)) => {
                        for l in left {
                            for r in right {
                                if let Some(joined) =
                                    (self.joiner)(Some(l.clone()), Some(r.clone())).await?
                                {
                                    results.push(joined);
                                }
                            }
                        }
                    }
                    (Some(left), None) => {
                        for l in left {
                            if let Some(joined) = (self.joiner)(Some(l.clone()), None).await? {
                                results.push(joined);
                            }
                        }
                    }
                    (None, Some(right)) => {
                        for r in right {
                            if let Some(joined) = (self.joiner)(None, Some(r.clone())).await? {
                                results.push(joined);
                            }
                        }
                    }
                    (None, None) => {}
                }
            }
        }

        Ok(results)
    }

    /// Maybe perform cleanup if enough time has passed
    async fn maybe_cleanup(&self) {
        let mut last_cleanup = self.last_cleanup.lock().await;
        let now = SystemTime::now();

        if let Ok(elapsed) = now.duration_since(*last_cleanup) {
            if elapsed > Duration::from_secs(60) {
                self.state.cleanup(self.max_age);
                *last_cleanup = now;
            }
        }
    }

    /// Get the name of this join operator
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Join functions for combining items
pub struct JoinFunctions;

impl JoinFunctions {
    /// Merge JSON objects
    #[allow(clippy::type_complexity)]
    pub fn merge_json() -> impl Fn(
        Option<StreamItem>,
        Option<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<Option<StreamItem>>> + Send>,
    > + Send
    + Sync
    + Clone {
        |left, right| {
            Box::pin(async move {
                let mut merged = serde_json::Map::new();

                if let Some(left_item) = left {
                    let left_json: serde_json::Value = serde_json::from_slice(&left_item)?;
                    if let Some(obj) = left_json.as_object() {
                        for (k, v) in obj {
                            merged.insert(format!("left_{}", k), v.clone());
                        }
                    }
                }

                if let Some(right_item) = right {
                    let right_json: serde_json::Value = serde_json::from_slice(&right_item)?;
                    if let Some(obj) = right_json.as_object() {
                        for (k, v) in obj {
                            merged.insert(format!("right_{}", k), v.clone());
                        }
                    }
                }

                let result = serde_json::Value::Object(merged);
                Ok(Some(serde_json::to_vec(&result)?))
            })
        }
    }

    /// Spatial join (intersects)
    #[allow(clippy::type_complexity)]
    pub fn spatial_intersects() -> impl Fn(
        Option<StreamItem>,
        Option<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<Option<StreamItem>>> + Send>,
    > + Send
    + Sync
    + Clone {
        |left, right| {
            Box::pin(async move {
                // In real implementation, use oxigdal-core for geometry operations
                // For now, just merge if both exist
                if left.is_some() && right.is_some() {
                    Self::merge_json()(left, right).await
                } else {
                    Ok(None)
                }
            })
        }
    }

    /// Custom join with field mapping
    #[allow(clippy::type_complexity)]
    pub fn custom_merge(
        left_prefix: String,
        right_prefix: String,
    ) -> impl Fn(
        Option<StreamItem>,
        Option<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<Option<StreamItem>>> + Send>,
    > + Send
    + Sync
    + Clone {
        move |left, right| {
            let left_prefix = left_prefix.clone();
            let right_prefix = right_prefix.clone();

            Box::pin(async move {
                let mut merged = HashMap::new();

                if let Some(left_item) = left {
                    let left_json: serde_json::Value = serde_json::from_slice(&left_item)?;
                    merged.insert(left_prefix, left_json);
                }

                if let Some(right_item) = right {
                    let right_json: serde_json::Value = serde_json::from_slice(&right_item)?;
                    merged.insert(right_prefix, right_json);
                }

                Ok(Some(serde_json::to_vec(&merged)?))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_json_field_extractor() {
        let extractor = JsonFieldExtractor::new("id".to_string());

        let json = serde_json::json!({"id": "123", "name": "test"});
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let key = extractor
            .extract_key(&item)
            .await
            .expect("Failed to extract");
        assert!(key.contains("123"));
    }

    #[tokio::test]
    async fn test_inner_join() {
        let left_extractor = JsonFieldExtractor::new("id".to_string());
        let right_extractor = JsonFieldExtractor::new("id".to_string());

        let join = JoinOperator::new(
            "test_join".to_string(),
            JoinType::Inner,
            left_extractor,
            right_extractor,
            JoinFunctions::merge_json(),
        );

        let left_json = serde_json::json!({"id": "1", "name": "Alice"});
        let left_item = serde_json::to_vec(&left_json).expect("Failed");

        let right_json = serde_json::json!({"id": "1", "age": 30});
        let right_item = serde_json::to_vec(&right_json).expect("Failed");

        // Process left first
        let result1 = join.process_left(left_item).await.expect("Failed");
        assert_eq!(result1.len(), 0); // No match yet

        // Process right - should match
        let result2 = join.process_right(right_item).await.expect("Failed");
        assert_eq!(result2.len(), 1);

        let joined: serde_json::Value =
            serde_json::from_slice(&result2[0]).expect("Failed to parse");
        assert!(joined.get("left_id").is_some());
        assert!(joined.get("right_id").is_some());
    }

    #[tokio::test]
    async fn test_join_state() {
        let state = JoinState::new();

        state.add_left("key1".to_string(), vec![1, 2, 3]);
        state.add_right("key1".to_string(), vec![4, 5, 6]);

        let (left, right) = state.get_matches("key1");
        assert!(left.is_some());
        assert!(right.is_some());

        let (no_left, no_right) = state.get_matches("key2");
        assert!(no_left.is_none());
        assert!(no_right.is_none());
    }
}
