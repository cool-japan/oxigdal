//! Join operations for streaming data.

use crate::core::stream::StreamElement;
use crate::error::{Result, StreamingError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type of join operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinType {
    /// Inner join
    Inner,
    /// Left outer join
    LeftOuter,
    /// Right outer join
    RightOuter,
    /// Full outer join
    FullOuter,
}

/// Configuration for join operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinConfig {
    /// Type of join
    pub join_type: JoinType,

    /// Maximum buffer size per key
    pub max_buffer_size: usize,

    /// Time-to-live for buffered elements (in seconds)
    pub ttl_seconds: i64,

    /// Enable cleanup of expired elements
    pub enable_cleanup: bool,
}

impl Default for JoinConfig {
    fn default() -> Self {
        Self {
            join_type: JoinType::Inner,
            max_buffer_size: 1000,
            ttl_seconds: 300,
            enable_cleanup: true,
        }
    }
}

/// Join operator for two streams.
pub struct JoinOperator {
    config: JoinConfig,
    left_buffer: Arc<RwLock<HashMap<Vec<u8>, VecDeque<StreamElement>>>>,
    right_buffer: Arc<RwLock<HashMap<Vec<u8>, VecDeque<StreamElement>>>>,
}

impl JoinOperator {
    /// Create a new join operator.
    pub fn new(config: JoinConfig) -> Self {
        Self {
            config,
            left_buffer: Arc::new(RwLock::new(HashMap::new())),
            right_buffer: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process a left element.
    pub async fn process_left(&self, element: StreamElement) -> Result<Vec<StreamElement>> {
        let key = element
            .key
            .clone()
            .ok_or_else(|| StreamingError::JoinError("Left element must have a key".to_string()))?;

        let mut results = Vec::new();
        let right_buffer = self.right_buffer.read().await;

        if let Some(right_elements) = right_buffer.get(&key) {
            for right_elem in right_elements {
                let joined = self.join_elements(&element, right_elem)?;
                results.push(joined);
            }
        }

        drop(right_buffer);

        if self.config.join_type == JoinType::LeftOuter && results.is_empty() {
            results.push(element.clone());
        }

        let mut left_buffer = self.left_buffer.write().await;
        let buffer = left_buffer.entry(key).or_insert_with(VecDeque::new);

        if buffer.len() >= self.config.max_buffer_size {
            buffer.pop_front();
        }

        buffer.push_back(element);

        if self.config.enable_cleanup {
            self.cleanup_expired_left(&mut left_buffer);
        }

        Ok(results)
    }

    /// Process a right element.
    pub async fn process_right(&self, element: StreamElement) -> Result<Vec<StreamElement>> {
        let key = element.key.clone().ok_or_else(|| {
            StreamingError::JoinError("Right element must have a key".to_string())
        })?;

        let mut results = Vec::new();
        let left_buffer = self.left_buffer.read().await;

        if let Some(left_elements) = left_buffer.get(&key) {
            for left_elem in left_elements {
                let joined = self.join_elements(left_elem, &element)?;
                results.push(joined);
            }
        }

        drop(left_buffer);

        if self.config.join_type == JoinType::RightOuter && results.is_empty() {
            results.push(element.clone());
        }

        let mut right_buffer = self.right_buffer.write().await;
        let buffer = right_buffer.entry(key).or_insert_with(VecDeque::new);

        if buffer.len() >= self.config.max_buffer_size {
            buffer.pop_front();
        }

        buffer.push_back(element);

        if self.config.enable_cleanup {
            self.cleanup_expired_right(&mut right_buffer);
        }

        Ok(results)
    }

    /// Join two elements.
    fn join_elements(&self, left: &StreamElement, right: &StreamElement) -> Result<StreamElement> {
        let mut joined_data = Vec::new();
        joined_data.extend_from_slice(&left.data);
        joined_data.extend_from_slice(&right.data);

        Ok(StreamElement {
            data: joined_data,
            event_time: left.event_time.max(right.event_time),
            processing_time: Utc::now(),
            key: left.key.clone(),
            metadata: left.metadata.clone(),
        })
    }

    /// Cleanup expired elements from left buffer.
    fn cleanup_expired_left(&self, buffer: &mut HashMap<Vec<u8>, VecDeque<StreamElement>>) {
        let now = Utc::now();
        let ttl_seconds = self.config.ttl_seconds;

        for queue in buffer.values_mut() {
            queue.retain(|elem| {
                let age = now.signed_duration_since(elem.event_time);
                age.num_seconds() < ttl_seconds
            });
        }
    }

    /// Cleanup expired elements from right buffer.
    fn cleanup_expired_right(&self, buffer: &mut HashMap<Vec<u8>, VecDeque<StreamElement>>) {
        let now = Utc::now();
        let ttl_seconds = self.config.ttl_seconds;

        for queue in buffer.values_mut() {
            queue.retain(|elem| {
                let age = now.signed_duration_since(elem.event_time);
                age.num_seconds() < ttl_seconds
            });
        }
    }

    /// Clear all buffers.
    pub async fn clear(&self) {
        self.left_buffer.write().await.clear();
        self.right_buffer.write().await.clear();
    }
}

/// CoGroup operator for two streams.
pub struct CoGroupOperator {
    left_buffer: Arc<RwLock<HashMap<Vec<u8>, Vec<StreamElement>>>>,
    right_buffer: Arc<RwLock<HashMap<Vec<u8>, Vec<StreamElement>>>>,
}

impl CoGroupOperator {
    /// Create a new cogroup operator.
    pub fn new() -> Self {
        Self {
            left_buffer: Arc::new(RwLock::new(HashMap::new())),
            right_buffer: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a left element.
    pub async fn add_left(&self, element: StreamElement) -> Result<()> {
        let key = element
            .key
            .clone()
            .ok_or_else(|| StreamingError::JoinError("Element must have a key".to_string()))?;

        let mut buffer = self.left_buffer.write().await;
        buffer.entry(key).or_insert_with(Vec::new).push(element);

        Ok(())
    }

    /// Add a right element.
    pub async fn add_right(&self, element: StreamElement) -> Result<()> {
        let key = element
            .key
            .clone()
            .ok_or_else(|| StreamingError::JoinError("Element must have a key".to_string()))?;

        let mut buffer = self.right_buffer.write().await;
        buffer.entry(key).or_insert_with(Vec::new).push(element);

        Ok(())
    }

    /// Get cogroup results for a key.
    pub async fn get_results(&self, key: &[u8]) -> (Vec<StreamElement>, Vec<StreamElement>) {
        let left_buffer = self.left_buffer.read().await;
        let right_buffer = self.right_buffer.read().await;

        let left = left_buffer.get(key).cloned().unwrap_or_else(Vec::new);
        let right = right_buffer.get(key).cloned().unwrap_or_else(Vec::new);

        (left, right)
    }

    /// Clear buffers.
    pub async fn clear(&self) {
        self.left_buffer.write().await.clear();
        self.right_buffer.write().await.clear();
    }
}

impl Default for CoGroupOperator {
    fn default() -> Self {
        Self::new()
    }
}

/// Interval join operator.
pub struct IntervalJoin {
    lower_bound_seconds: i64,
    upper_bound_seconds: i64,
    left_buffer: Arc<RwLock<HashMap<Vec<u8>, VecDeque<StreamElement>>>>,
    right_buffer: Arc<RwLock<HashMap<Vec<u8>, VecDeque<StreamElement>>>>,
}

impl IntervalJoin {
    /// Create a new interval join.
    pub fn new(lower_bound_seconds: i64, upper_bound_seconds: i64) -> Self {
        Self {
            lower_bound_seconds,
            upper_bound_seconds,
            left_buffer: Arc::new(RwLock::new(HashMap::new())),
            right_buffer: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process a left element.
    pub async fn process_left(&self, element: StreamElement) -> Result<Vec<StreamElement>> {
        let key = element
            .key
            .clone()
            .ok_or_else(|| StreamingError::JoinError("Element must have a key".to_string()))?;

        let mut results = Vec::new();
        let right_buffer = self.right_buffer.read().await;

        if let Some(right_elements) = right_buffer.get(&key) {
            for right_elem in right_elements {
                if self.in_interval(&element, right_elem) {
                    let mut joined_data = Vec::new();
                    joined_data.extend_from_slice(&element.data);
                    joined_data.extend_from_slice(&right_elem.data);

                    results.push(StreamElement {
                        data: joined_data,
                        event_time: element.event_time.max(right_elem.event_time),
                        processing_time: Utc::now(),
                        key: Some(key.clone()),
                        metadata: element.metadata.clone(),
                    });
                }
            }
        }

        drop(right_buffer);

        let mut left_buffer = self.left_buffer.write().await;
        left_buffer
            .entry(key)
            .or_insert_with(VecDeque::new)
            .push_back(element);

        Ok(results)
    }

    /// Process a right element.
    pub async fn process_right(&self, element: StreamElement) -> Result<Vec<StreamElement>> {
        let key = element
            .key
            .clone()
            .ok_or_else(|| StreamingError::JoinError("Element must have a key".to_string()))?;

        let mut results = Vec::new();
        let left_buffer = self.left_buffer.read().await;

        if let Some(left_elements) = left_buffer.get(&key) {
            for left_elem in left_elements {
                if self.in_interval(left_elem, &element) {
                    let mut joined_data = Vec::new();
                    joined_data.extend_from_slice(&left_elem.data);
                    joined_data.extend_from_slice(&element.data);

                    results.push(StreamElement {
                        data: joined_data,
                        event_time: left_elem.event_time.max(element.event_time),
                        processing_time: Utc::now(),
                        key: Some(key.clone()),
                        metadata: left_elem.metadata.clone(),
                    });
                }
            }
        }

        drop(left_buffer);

        let mut right_buffer = self.right_buffer.write().await;
        right_buffer
            .entry(key)
            .or_insert_with(VecDeque::new)
            .push_back(element);

        Ok(results)
    }

    /// Check if two elements are within the join interval.
    fn in_interval(&self, left: &StreamElement, right: &StreamElement) -> bool {
        let time_diff = right.event_time.signed_duration_since(left.event_time);
        let time_diff_seconds = time_diff.num_seconds();
        time_diff_seconds >= self.lower_bound_seconds
            && time_diff_seconds <= self.upper_bound_seconds
    }

    /// Clear buffers.
    pub async fn clear(&self) {
        self.left_buffer.write().await.clear();
        self.right_buffer.write().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_join_operator() {
        let config = JoinConfig::default();
        let join = JoinOperator::new(config);

        let left = StreamElement::new(vec![1, 2], Utc::now()).with_key(vec![1]);
        let right = StreamElement::new(vec![3, 4], Utc::now()).with_key(vec![1]);

        join.process_left(left)
            .await
            .expect("process_left should succeed in test");
        let results = join
            .process_right(right)
            .await
            .expect("process_right should succeed in test");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_cogroup_operator() {
        let cogroup = CoGroupOperator::new();

        let left = StreamElement::new(vec![1, 2], Utc::now()).with_key(vec![1]);
        let right = StreamElement::new(vec![3, 4], Utc::now()).with_key(vec![1]);

        cogroup
            .add_left(left)
            .await
            .expect("add_left should succeed in test");
        cogroup
            .add_right(right)
            .await
            .expect("add_right should succeed in test");

        let (left_elems, right_elems) = cogroup.get_results(&[1]).await;
        assert_eq!(left_elems.len(), 1);
        assert_eq!(right_elems.len(), 1);
    }

    #[tokio::test]
    async fn test_interval_join() {
        let join = IntervalJoin::new(0, 10);

        let left = StreamElement::new(vec![1, 2], Utc::now()).with_key(vec![1]);
        let right_time = Utc::now() + chrono::Duration::seconds(5);
        let right = StreamElement::new(vec![3, 4], right_time).with_key(vec![1]);

        join.process_left(left)
            .await
            .expect("process_left should succeed in test");
        let results = join
            .process_right(right)
            .await
            .expect("process_right should succeed in test");

        assert!(!results.is_empty());
    }
}
