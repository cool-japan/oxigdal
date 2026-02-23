//! Reduce, fold, and scan operations.

use crate::core::stream::StreamElement;
use crate::error::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for keyed state storage (key -> value bytes).
type KeyedState = HashMap<Option<Vec<u8>>, Vec<u8>>;

/// Function for reducing elements.
pub trait ReduceFunction: Send + Sync {
    /// Reduce two values into one.
    fn reduce(&self, value1: Vec<u8>, value2: Vec<u8>) -> Vec<u8>;
}

/// Reduce operator.
pub struct ReduceOperator<F>
where
    F: ReduceFunction,
{
    reduce_fn: Arc<F>,
    state: Arc<RwLock<KeyedState>>,
}

impl<F> ReduceOperator<F>
where
    F: ReduceFunction,
{
    /// Create a new reduce operator.
    pub fn new(reduce_fn: F) -> Self {
        Self {
            reduce_fn: Arc::new(reduce_fn),
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process an element.
    pub async fn process(&self, element: StreamElement) -> Result<Option<StreamElement>> {
        let mut state = self.state.write().await;

        let key = element.key.clone();
        let current = state.entry(key.clone()).or_insert_with(Vec::new);

        if current.is_empty() {
            *current = element.data;
            Ok(None)
        } else {
            let reduced = self.reduce_fn.reduce(current.clone(), element.data);
            *current = reduced.clone();

            Ok(Some(StreamElement {
                data: reduced,
                event_time: element.event_time,
                processing_time: element.processing_time,
                key,
                metadata: element.metadata,
            }))
        }
    }

    /// Get the current state for a key.
    pub async fn get_state(&self, key: Option<Vec<u8>>) -> Option<Vec<u8>> {
        self.state.read().await.get(&key).cloned()
    }

    /// Clear all state.
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }
}

/// Function for folding elements with an accumulator.
pub trait FoldFunction: Send + Sync {
    /// Fold a value into the accumulator.
    fn fold(&self, accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8>;
}

/// Fold operator.
pub struct FoldOperator<F>
where
    F: FoldFunction,
{
    fold_fn: Arc<F>,
    initial_value: Vec<u8>,
    state: Arc<RwLock<KeyedState>>,
}

impl<F> FoldOperator<F>
where
    F: FoldFunction,
{
    /// Create a new fold operator.
    pub fn new(fold_fn: F, initial_value: Vec<u8>) -> Self {
        Self {
            fold_fn: Arc::new(fold_fn),
            initial_value,
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process an element.
    pub async fn process(&self, element: StreamElement) -> Result<StreamElement> {
        let mut state = self.state.write().await;

        let key = element.key.clone();
        let current = state
            .entry(key.clone())
            .or_insert_with(|| self.initial_value.clone());

        let folded = self.fold_fn.fold(current.clone(), element.data);
        *current = folded.clone();

        Ok(StreamElement {
            data: folded,
            event_time: element.event_time,
            processing_time: element.processing_time,
            key,
            metadata: element.metadata,
        })
    }

    /// Get the current state for a key.
    pub async fn get_state(&self, key: Option<Vec<u8>>) -> Vec<u8> {
        self.state
            .read()
            .await
            .get(&key)
            .cloned()
            .unwrap_or_else(|| self.initial_value.clone())
    }

    /// Clear all state.
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }
}

/// Scan operator (like fold but emits intermediate results).
pub struct ScanOperator<F>
where
    F: FoldFunction,
{
    fold_fn: Arc<F>,
    initial_value: Vec<u8>,
    state: Arc<RwLock<KeyedState>>,
}

impl<F> ScanOperator<F>
where
    F: FoldFunction,
{
    /// Create a new scan operator.
    pub fn new(fold_fn: F, initial_value: Vec<u8>) -> Self {
        Self {
            fold_fn: Arc::new(fold_fn),
            initial_value,
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Process an element.
    pub async fn process(&self, element: StreamElement) -> Result<Vec<StreamElement>> {
        let mut state = self.state.write().await;

        let key = element.key.clone();
        let current = state
            .entry(key.clone())
            .or_insert_with(|| self.initial_value.clone());

        let scanned = self.fold_fn.fold(current.clone(), element.data);
        *current = scanned.clone();

        Ok(vec![StreamElement {
            data: scanned,
            event_time: element.event_time,
            processing_time: element.processing_time,
            key,
            metadata: element.metadata,
        }])
    }

    /// Clear all state.
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }
}

/// Simple sum reduce function.
pub struct SumReduce;

impl ReduceFunction for SumReduce {
    fn reduce(&self, value1: Vec<u8>, value2: Vec<u8>) -> Vec<u8> {
        let v1 = i64::from_le_bytes(value1.try_into().unwrap_or([0; 8]));
        let v2 = i64::from_le_bytes(value2.try_into().unwrap_or([0; 8]));
        (v1 + v2).to_le_bytes().to_vec()
    }
}

/// Simple concatenation fold function.
pub struct ConcatFold;

impl FoldFunction for ConcatFold {
    fn fold(&self, mut accumulator: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
        accumulator.extend(value);
        accumulator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_reduce_operator() {
        let operator = ReduceOperator::new(SumReduce);

        let elem1 = StreamElement::new(5i64.to_le_bytes().to_vec(), Utc::now());
        let elem2 = StreamElement::new(3i64.to_le_bytes().to_vec(), Utc::now());

        let result1 = operator
            .process(elem1)
            .await
            .expect("Failed to process first element in reduce operator test");
        assert!(result1.is_none());

        let result2 = operator
            .process(elem2)
            .await
            .expect("Failed to process second element in reduce operator test");
        assert!(result2.is_some());

        let value = i64::from_le_bytes(
            result2
                .expect("Result2 should contain a value after reduce operation")
                .data
                .try_into()
                .unwrap_or([0; 8]),
        );
        assert_eq!(value, 8);
    }

    #[tokio::test]
    async fn test_fold_operator() {
        let operator = FoldOperator::new(ConcatFold, vec![]);

        let elem1 = StreamElement::new(vec![1, 2], Utc::now());
        let elem2 = StreamElement::new(vec![3, 4], Utc::now());

        let result1 = operator
            .process(elem1)
            .await
            .expect("Failed to process first element in fold operator test");
        assert_eq!(result1.data, vec![1, 2]);

        let result2 = operator
            .process(elem2)
            .await
            .expect("Failed to process second element in fold operator test");
        assert_eq!(result2.data, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_scan_operator() {
        let operator = ScanOperator::new(ConcatFold, vec![]);

        let elem1 = StreamElement::new(vec![1, 2], Utc::now());
        let elem2 = StreamElement::new(vec![3, 4], Utc::now());

        let results1 = operator
            .process(elem1)
            .await
            .expect("Failed to process first element in scan operator test");
        assert_eq!(results1.len(), 1);
        assert_eq!(results1[0].data, vec![1, 2]);

        let results2 = operator
            .process(elem2)
            .await
            .expect("Failed to process second element in scan operator test");
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].data, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_reduce_with_keys() {
        let operator = ReduceOperator::new(SumReduce);

        let elem1 = StreamElement::new(5i64.to_le_bytes().to_vec(), Utc::now()).with_key(vec![1]);
        let elem2 = StreamElement::new(3i64.to_le_bytes().to_vec(), Utc::now()).with_key(vec![1]);
        let elem3 = StreamElement::new(10i64.to_le_bytes().to_vec(), Utc::now()).with_key(vec![2]);

        operator
            .process(elem1)
            .await
            .expect("Failed to process first keyed element");
        operator
            .process(elem2)
            .await
            .expect("Failed to process second keyed element");
        operator
            .process(elem3)
            .await
            .expect("Failed to process third keyed element");

        let state1 = operator
            .get_state(Some(vec![1]))
            .await
            .expect("Failed to get state for key [1]");
        let value1 = i64::from_le_bytes(state1.try_into().unwrap_or([0; 8]));
        assert_eq!(value1, 8);

        let state2 = operator
            .get_state(Some(vec![2]))
            .await
            .expect("Failed to get state for key [2]");
        let value2 = i64::from_le_bytes(state2.try_into().unwrap_or([0; 8]));
        assert_eq!(value2, 10);
    }
}
