//! Aggregate operations for streaming data.

use crate::core::stream::StreamElement;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for keyed state storage (key -> value bytes).
type KeyedState = HashMap<Option<Vec<u8>>, Vec<u8>>;

/// Trait for aggregate functions.
pub trait AggregateFunction: Send + Sync {
    /// Create initial accumulator.
    fn create_accumulator(&self) -> Vec<u8>;

    /// Add a value to the accumulator.
    fn add(&self, accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8>;

    /// Get the result from the accumulator.
    fn get_result(&self, accumulator: Vec<u8>) -> Vec<u8>;

    /// Merge two accumulators.
    fn merge(&self, acc1: Vec<u8>, acc2: Vec<u8>) -> Vec<u8>;
}

/// Aggregate operator.
pub struct AggregateOperator<F>
where
    F: AggregateFunction,
{
    aggregate_fn: Arc<F>,
    state: Arc<RwLock<KeyedState>>,
}

impl<F> AggregateOperator<F>
where
    F: AggregateFunction,
{
    /// Create a new aggregate operator.
    pub fn new(aggregate_fn: F) -> Self {
        Self {
            aggregate_fn: Arc::new(aggregate_fn),
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process an element.
    pub async fn process(&self, element: StreamElement) -> Result<StreamElement> {
        let mut state = self.state.write().await;

        let key = element.key.clone();
        let current = state
            .entry(key.clone())
            .or_insert_with(|| self.aggregate_fn.create_accumulator());

        let updated = self.aggregate_fn.add(current.clone(), element.data);
        *current = updated.clone();

        let result = self.aggregate_fn.get_result(updated);

        Ok(StreamElement {
            data: result,
            event_time: element.event_time,
            processing_time: element.processing_time,
            key,
            metadata: element.metadata,
        })
    }

    /// Get the current result for a key.
    pub async fn get_result(&self, key: Option<Vec<u8>>) -> Vec<u8> {
        let state = self.state.read().await;
        state
            .get(&key)
            .map(|acc| self.aggregate_fn.get_result(acc.clone()))
            .unwrap_or_else(|| {
                self.aggregate_fn
                    .get_result(self.aggregate_fn.create_accumulator())
            })
    }

    /// Clear all state.
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }
}

/// Count aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountAggregate;

impl AggregateFunction for CountAggregate {
    fn create_accumulator(&self) -> Vec<u8> {
        0i64.to_le_bytes().to_vec()
    }

    fn add(&self, accumulator: Vec<u8>, _value: Vec<u8>) -> Vec<u8> {
        let count = i64::from_le_bytes(accumulator.try_into().unwrap_or([0; 8]));
        (count + 1).to_le_bytes().to_vec()
    }

    fn get_result(&self, accumulator: Vec<u8>) -> Vec<u8> {
        accumulator
    }

    fn merge(&self, acc1: Vec<u8>, acc2: Vec<u8>) -> Vec<u8> {
        let count1 = i64::from_le_bytes(acc1.try_into().unwrap_or([0; 8]));
        let count2 = i64::from_le_bytes(acc2.try_into().unwrap_or([0; 8]));
        (count1 + count2).to_le_bytes().to_vec()
    }
}

/// Sum aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SumAggregate;

impl AggregateFunction for SumAggregate {
    fn create_accumulator(&self) -> Vec<u8> {
        0i64.to_le_bytes().to_vec()
    }

    fn add(&self, accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
        let acc = i64::from_le_bytes(accumulator.try_into().unwrap_or([0; 8]));
        let val = i64::from_le_bytes(value.try_into().unwrap_or([0; 8]));
        (acc + val).to_le_bytes().to_vec()
    }

    fn get_result(&self, accumulator: Vec<u8>) -> Vec<u8> {
        accumulator
    }

    fn merge(&self, acc1: Vec<u8>, acc2: Vec<u8>) -> Vec<u8> {
        let sum1 = i64::from_le_bytes(acc1.try_into().unwrap_or([0; 8]));
        let sum2 = i64::from_le_bytes(acc2.try_into().unwrap_or([0; 8]));
        (sum1 + sum2).to_le_bytes().to_vec()
    }
}

/// Average aggregate (stores sum and count).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvgAggregate;

impl AggregateFunction for AvgAggregate {
    fn create_accumulator(&self) -> Vec<u8> {
        let mut acc = Vec::new();
        acc.extend_from_slice(&0i64.to_le_bytes());
        acc.extend_from_slice(&0i64.to_le_bytes());
        acc
    }

    fn add(&self, accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
        let sum = i64::from_le_bytes(accumulator[0..8].try_into().unwrap_or([0; 8]));
        let count = i64::from_le_bytes(accumulator[8..16].try_into().unwrap_or([0; 8]));
        let val = i64::from_le_bytes(value.try_into().unwrap_or([0; 8]));

        let mut result = Vec::new();
        result.extend_from_slice(&(sum + val).to_le_bytes());
        result.extend_from_slice(&(count + 1).to_le_bytes());
        result
    }

    fn get_result(&self, accumulator: Vec<u8>) -> Vec<u8> {
        let sum = i64::from_le_bytes(accumulator[0..8].try_into().unwrap_or([0; 8]));
        let count = i64::from_le_bytes(accumulator[8..16].try_into().unwrap_or([0; 8]));

        if count == 0 {
            0i64.to_le_bytes().to_vec()
        } else {
            (sum / count).to_le_bytes().to_vec()
        }
    }

    fn merge(&self, acc1: Vec<u8>, acc2: Vec<u8>) -> Vec<u8> {
        let sum1 = i64::from_le_bytes(acc1[0..8].try_into().unwrap_or([0; 8]));
        let count1 = i64::from_le_bytes(acc1[8..16].try_into().unwrap_or([0; 8]));
        let sum2 = i64::from_le_bytes(acc2[0..8].try_into().unwrap_or([0; 8]));
        let count2 = i64::from_le_bytes(acc2[8..16].try_into().unwrap_or([0; 8]));

        let mut result = Vec::new();
        result.extend_from_slice(&(sum1 + sum2).to_le_bytes());
        result.extend_from_slice(&(count1 + count2).to_le_bytes());
        result
    }
}

/// Min aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinAggregate;

impl AggregateFunction for MinAggregate {
    fn create_accumulator(&self) -> Vec<u8> {
        i64::MAX.to_le_bytes().to_vec()
    }

    fn add(&self, accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
        let acc = i64::from_le_bytes(accumulator.try_into().unwrap_or([0; 8]));
        let val = i64::from_le_bytes(value.try_into().unwrap_or([0; 8]));
        acc.min(val).to_le_bytes().to_vec()
    }

    fn get_result(&self, accumulator: Vec<u8>) -> Vec<u8> {
        accumulator
    }

    fn merge(&self, acc1: Vec<u8>, acc2: Vec<u8>) -> Vec<u8> {
        let min1 = i64::from_le_bytes(acc1.try_into().unwrap_or([0; 8]));
        let min2 = i64::from_le_bytes(acc2.try_into().unwrap_or([0; 8]));
        min1.min(min2).to_le_bytes().to_vec()
    }
}

/// Max aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaxAggregate;

impl AggregateFunction for MaxAggregate {
    fn create_accumulator(&self) -> Vec<u8> {
        i64::MIN.to_le_bytes().to_vec()
    }

    fn add(&self, accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
        let acc = i64::from_le_bytes(accumulator.try_into().unwrap_or([0; 8]));
        let val = i64::from_le_bytes(value.try_into().unwrap_or([0; 8]));
        acc.max(val).to_le_bytes().to_vec()
    }

    fn get_result(&self, accumulator: Vec<u8>) -> Vec<u8> {
        accumulator
    }

    fn merge(&self, acc1: Vec<u8>, acc2: Vec<u8>) -> Vec<u8> {
        let max1 = i64::from_le_bytes(acc1.try_into().unwrap_or([0; 8]));
        let max2 = i64::from_le_bytes(acc2.try_into().unwrap_or([0; 8]));
        max1.max(max2).to_le_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_count_aggregate() {
        let operator = AggregateOperator::new(CountAggregate);

        for i in 0..5 {
            let elem = StreamElement::new(vec![i], Utc::now());
            operator
                .process(elem)
                .await
                .expect("aggregate processing should succeed in test");
        }

        let result = operator.get_result(None).await;
        let count = i64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        assert_eq!(count, 5);
    }

    #[tokio::test]
    async fn test_sum_aggregate() {
        let operator = AggregateOperator::new(SumAggregate);

        for i in 1..=5 {
            let elem = StreamElement::new((i as i64).to_le_bytes().to_vec(), Utc::now());
            operator
                .process(elem)
                .await
                .expect("aggregate processing should succeed in test");
        }

        let result = operator.get_result(None).await;
        let sum = i64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        assert_eq!(sum, 15);
    }

    #[tokio::test]
    async fn test_avg_aggregate() {
        let operator = AggregateOperator::new(AvgAggregate);

        for i in 1..=5 {
            let elem = StreamElement::new((i as i64).to_le_bytes().to_vec(), Utc::now());
            operator
                .process(elem)
                .await
                .expect("aggregate processing should succeed in test");
        }

        let result = operator.get_result(None).await;
        let avg = i64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        assert_eq!(avg, 3);
    }

    #[tokio::test]
    async fn test_min_aggregate() {
        let operator = AggregateOperator::new(MinAggregate);

        for i in [5, 2, 8, 1, 9] {
            let elem = StreamElement::new((i as i64).to_le_bytes().to_vec(), Utc::now());
            operator
                .process(elem)
                .await
                .expect("aggregate processing should succeed in test");
        }

        let result = operator.get_result(None).await;
        let min = i64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        assert_eq!(min, 1);
    }

    #[tokio::test]
    async fn test_max_aggregate() {
        let operator = AggregateOperator::new(MaxAggregate);

        for i in [5, 2, 8, 1, 9] {
            let elem = StreamElement::new((i as i64).to_le_bytes().to_vec(), Utc::now());
            operator
                .process(elem)
                .await
                .expect("aggregate processing should succeed in test");
        }

        let result = operator.get_result(None).await;
        let max = i64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        assert_eq!(max, 9);
    }
}
