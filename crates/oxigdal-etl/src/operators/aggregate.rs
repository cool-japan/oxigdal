//! Aggregate operator for stream aggregation
//!
//! This module provides aggregate operators for computing statistics and summaries
//! over streams of data.

use crate::error::{Result, TransformError};
use crate::stream::StreamItem;
use crate::transform::Transform;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Aggregate operator for computing statistics
pub struct AggregateOperator<F>
where
    F: Fn(
            Vec<StreamItem>,
        ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    name: String,
    aggregator: F,
    buffer: Mutex<Vec<StreamItem>>,
    emit_every: usize,
}

impl<F> AggregateOperator<F>
where
    F: Fn(
            Vec<StreamItem>,
        ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    /// Create a new aggregate operator
    pub fn new(name: String, emit_every: usize, aggregator: F) -> Self {
        Self {
            name,
            aggregator,
            buffer: Mutex::new(Vec::new()),
            emit_every,
        }
    }

    /// Flush the buffer and emit aggregated result
    pub async fn flush(&self) -> Result<Option<StreamItem>> {
        let mut buffer = self.buffer.lock().await;
        if buffer.is_empty() {
            return Ok(None);
        }

        let items = buffer.drain(..).collect();
        let result = (self.aggregator)(items).await?;
        Ok(Some(result))
    }
}

#[async_trait]
impl<F> Transform for AggregateOperator<F>
where
    F: Fn(
            Vec<StreamItem>,
        ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let mut buffer = self.buffer.lock().await;
        buffer.push(item);

        if buffer.len() >= self.emit_every {
            let items = buffer.drain(..).collect();
            drop(buffer);
            let result = (self.aggregator)(items).await?;
            Ok(vec![result])
        } else {
            Ok(Vec::new())
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Aggregation functions
pub struct AggregateFunctions;

impl AggregateFunctions {
    /// Sum numeric values from JSON field
    pub fn sum(
        field: String,
    ) -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
    + Send
    + Sync
    + Clone {
        move |items| {
            let field = field.clone();
            Box::pin(async move {
                let mut sum = 0.0f64;
                let mut count = 0usize;

                for item in items {
                    let json: serde_json::Value = serde_json::from_slice(&item)?;
                    if let Some(val) = json.get(&field).and_then(|v| v.as_f64()) {
                        sum += val;
                        count += 1;
                    }
                }

                let result = serde_json::json!({
                    "sum": sum,
                    "count": count,
                    "field": field,
                });

                Ok(serde_json::to_vec(&result)?)
            })
        }
    }

    /// Calculate mean of numeric values
    pub fn mean(
        field: String,
    ) -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
    + Send
    + Sync
    + Clone {
        move |items| {
            let field = field.clone();
            Box::pin(async move {
                let mut sum = 0.0f64;
                let mut count = 0usize;

                for item in items {
                    let json: serde_json::Value = serde_json::from_slice(&item)?;
                    if let Some(val) = json.get(&field).and_then(|v| v.as_f64()) {
                        sum += val;
                        count += 1;
                    }
                }

                if count == 0 {
                    return Err(TransformError::AggregationFailed {
                        message: "No values found".to_string(),
                    }
                    .into());
                }

                let mean = sum / count as f64;

                let result = serde_json::json!({
                    "mean": mean,
                    "count": count,
                    "field": field,
                });

                Ok(serde_json::to_vec(&result)?)
            })
        }
    }

    /// Calculate min/max/mean/stddev statistics
    pub fn stats(
        field: String,
    ) -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
    + Send
    + Sync
    + Clone {
        move |items| {
            let field = field.clone();
            Box::pin(async move {
                let mut values = Vec::new();

                for item in items {
                    let json: serde_json::Value = serde_json::from_slice(&item)?;
                    if let Some(val) = json.get(&field).and_then(|v| v.as_f64()) {
                        values.push(val);
                    }
                }

                if values.is_empty() {
                    return Err(TransformError::AggregationFailed {
                        message: "No values found".to_string(),
                    }
                    .into());
                }

                let count = values.len();
                let sum: f64 = values.iter().sum();
                let mean = sum / count as f64;

                let min = values
                    .iter()
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .copied()
                    .unwrap_or(0.0);

                let max = values
                    .iter()
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .copied()
                    .unwrap_or(0.0);

                // Calculate standard deviation
                let variance: f64 = values
                    .iter()
                    .map(|&v| {
                        let diff = v - mean;
                        diff * diff
                    })
                    .sum::<f64>()
                    / count as f64;
                let stddev = variance.sqrt();

                let result = serde_json::json!({
                    "field": field,
                    "count": count,
                    "sum": sum,
                    "mean": mean,
                    "min": min,
                    "max": max,
                    "stddev": stddev,
                });

                Ok(serde_json::to_vec(&result)?)
            })
        }
    }

    /// Count items by unique values in a field
    pub fn count_by(
        field: String,
    ) -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
    + Send
    + Sync
    + Clone {
        move |items| {
            let field = field.clone();
            Box::pin(async move {
                let mut counts = HashMap::new();

                for item in items {
                    let json: serde_json::Value = serde_json::from_slice(&item)?;
                    if let Some(val) = json.get(&field) {
                        let key = val.to_string();
                        *counts.entry(key).or_insert(0usize) += 1;
                    }
                }

                let result = serde_json::json!({
                    "field": field,
                    "counts": counts,
                    "unique_values": counts.len(),
                });

                Ok(serde_json::to_vec(&result)?)
            })
        }
    }

    /// Collect all items into a JSON array
    pub fn collect() -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<StreamItem>> + Send>,
    > + Send
    + Sync
    + Clone {
        |items| {
            Box::pin(async move {
                let mut values = Vec::new();

                for item in items {
                    let value: serde_json::Value = serde_json::from_slice(&item)?;
                    values.push(value);
                }

                let result = serde_json::Value::Array(values);
                Ok(serde_json::to_vec(&result)?)
            })
        }
    }

    /// Calculate percentiles (p50, p90, p95, p99)
    pub fn percentiles(
        field: String,
    ) -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<StreamItem>> + Send>>
    + Send
    + Sync
    + Clone {
        move |items| {
            let field = field.clone();
            Box::pin(async move {
                let mut values = Vec::new();

                for item in items {
                    let json: serde_json::Value = serde_json::from_slice(&item)?;
                    if let Some(val) = json.get(&field).and_then(|v| v.as_f64()) {
                        values.push(val);
                    }
                }

                if values.is_empty() {
                    return Err(TransformError::AggregationFailed {
                        message: "No values found".to_string(),
                    }
                    .into());
                }

                values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                let count = values.len();
                let p50 = values[count * 50 / 100];
                let p90 = values[count * 90 / 100];
                let p95 = values[count * 95 / 100];
                let p99 = values[count * 99 / 100];

                let result = serde_json::json!({
                    "field": field,
                    "count": count,
                    "p50": p50,
                    "p90": p90,
                    "p95": p95,
                    "p99": p99,
                });

                Ok(serde_json::to_vec(&result)?)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_aggregate_sum() {
        let agg = AggregateOperator::new(
            "sum_test".to_string(),
            3,
            AggregateFunctions::sum("value".to_string()),
        );

        let item1 = serde_json::to_vec(&serde_json::json!({"value": 10.0})).expect("Failed");
        let item2 = serde_json::to_vec(&serde_json::json!({"value": 20.0})).expect("Failed");
        let item3 = serde_json::to_vec(&serde_json::json!({"value": 30.0})).expect("Failed");

        agg.transform(item1).await.expect("Failed");
        agg.transform(item2).await.expect("Failed");
        let result = agg.transform(item3).await.expect("Failed");

        assert_eq!(result.len(), 1);

        let stats: serde_json::Value = serde_json::from_slice(&result[0]).expect("Failed");
        assert_eq!(stats.get("sum").and_then(|v| v.as_f64()), Some(60.0));
        assert_eq!(stats.get("count").and_then(|v| v.as_u64()), Some(3));
    }

    #[tokio::test]
    async fn test_aggregate_mean() {
        let agg = AggregateOperator::new(
            "mean_test".to_string(),
            2,
            AggregateFunctions::mean("value".to_string()),
        );

        let item1 = serde_json::to_vec(&serde_json::json!({"value": 10.0})).expect("Failed");
        let item2 = serde_json::to_vec(&serde_json::json!({"value": 20.0})).expect("Failed");

        agg.transform(item1).await.expect("Failed");
        let result = agg.transform(item2).await.expect("Failed");

        assert_eq!(result.len(), 1);

        let stats: serde_json::Value = serde_json::from_slice(&result[0]).expect("Failed");
        assert_eq!(stats.get("mean").and_then(|v| v.as_f64()), Some(15.0));
    }

    #[tokio::test]
    async fn test_aggregate_stats() {
        let items = vec![
            serde_json::to_vec(&serde_json::json!({"value": 10.0})).expect("Failed"),
            serde_json::to_vec(&serde_json::json!({"value": 20.0})).expect("Failed"),
            serde_json::to_vec(&serde_json::json!({"value": 30.0})).expect("Failed"),
        ];

        let result = AggregateFunctions::stats("value".to_string())(items)
            .await
            .expect("Failed");

        let stats: serde_json::Value = serde_json::from_slice(&result).expect("Failed");
        assert_eq!(stats.get("count").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(stats.get("mean").and_then(|v| v.as_f64()), Some(20.0));
        assert_eq!(stats.get("min").and_then(|v| v.as_f64()), Some(10.0));
        assert_eq!(stats.get("max").and_then(|v| v.as_f64()), Some(30.0));
    }

    #[tokio::test]
    async fn test_count_by() {
        let items = vec![
            serde_json::to_vec(&serde_json::json!({"type": "A"})).expect("Failed"),
            serde_json::to_vec(&serde_json::json!({"type": "B"})).expect("Failed"),
            serde_json::to_vec(&serde_json::json!({"type": "A"})).expect("Failed"),
            serde_json::to_vec(&serde_json::json!({"type": "C"})).expect("Failed"),
        ];

        let result = AggregateFunctions::count_by("type".to_string())(items)
            .await
            .expect("Failed");

        let stats: serde_json::Value = serde_json::from_slice(&result).expect("Failed");
        assert_eq!(stats.get("unique_values").and_then(|v| v.as_u64()), Some(3));
    }

    #[tokio::test]
    async fn test_collect() {
        let items = vec![
            serde_json::to_vec(&serde_json::json!({"id": 1})).expect("Failed"),
            serde_json::to_vec(&serde_json::json!({"id": 2})).expect("Failed"),
        ];

        let result = AggregateFunctions::collect()(items).await.expect("Failed");

        let array: serde_json::Value = serde_json::from_slice(&result).expect("Failed");
        assert!(array.is_array());
        assert_eq!(array.as_array().map(|a| a.len()), Some(2));
    }

    #[tokio::test]
    async fn test_percentiles() {
        let items: Vec<_> = (1..=100)
            .map(|i| serde_json::to_vec(&serde_json::json!({"value": i as f64})).expect("Failed"))
            .collect();

        let result = AggregateFunctions::percentiles("value".to_string())(items)
            .await
            .expect("Failed");

        let stats: serde_json::Value = serde_json::from_slice(&result).expect("Failed");
        assert_eq!(stats.get("count").and_then(|v| v.as_u64()), Some(100));
        // p50 at index 50 out of 100 items (1-100) is 51.0
        assert_eq!(stats.get("p50").and_then(|v| v.as_f64()), Some(51.0));
    }
}
