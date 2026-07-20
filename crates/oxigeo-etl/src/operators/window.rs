//! Window operator for sliding and tumbling windows
//!
//! This module provides window operators for time-based and count-based windowing.

use crate::error::{Result, TransformError};
use crate::stream::StreamItem;
use crate::transform::Transform;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

/// Window type
#[derive(Debug, Clone)]
pub enum WindowType {
    /// Tumbling window (non-overlapping, fixed size)
    Tumbling {
        /// Window size
        size: WindowSize,
    },
    /// Sliding window (overlapping, fixed size with slide interval)
    Sliding {
        /// Window size
        size: WindowSize,
        /// Slide interval
        slide: WindowSize,
    },
    /// Session window (gaps between items)
    Session {
        /// Gap duration
        gap: Duration,
    },
}

/// Window size
#[derive(Debug, Clone)]
pub enum WindowSize {
    /// Count-based window
    Count(usize),
    /// Time-based window
    Time(Duration),
}

/// Windowed item with timestamp
#[derive(Debug, Clone)]
struct WindowedItem {
    data: StreamItem,
    timestamp: SystemTime,
}

/// Window operator
pub struct WindowOperator<F>
where
    F: Fn(
            Vec<StreamItem>,
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    name: String,
    window_type: WindowType,
    aggregator: F,
    buffer: Mutex<VecDeque<WindowedItem>>,
}

impl<F> WindowOperator<F>
where
    F: Fn(
            Vec<StreamItem>,
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    /// Create a new window operator
    pub fn new(name: String, window_type: WindowType, aggregator: F) -> Self {
        Self {
            name,
            window_type,
            aggregator,
            buffer: Mutex::new(VecDeque::new()),
        }
    }

    /// Create a tumbling count window
    pub fn tumbling_count(count: usize, aggregator: F) -> Self {
        Self::new(
            format!("tumbling_count_{}", count),
            WindowType::Tumbling {
                size: WindowSize::Count(count),
            },
            aggregator,
        )
    }

    /// Create a tumbling time window
    pub fn tumbling_time(duration: Duration, aggregator: F) -> Self {
        Self::new(
            format!("tumbling_time_{}s", duration.as_secs()),
            WindowType::Tumbling {
                size: WindowSize::Time(duration),
            },
            aggregator,
        )
    }

    /// Create a sliding count window
    pub fn sliding_count(size: usize, slide: usize, aggregator: F) -> Self {
        Self::new(
            format!("sliding_count_{}_{}", size, slide),
            WindowType::Sliding {
                size: WindowSize::Count(size),
                slide: WindowSize::Count(slide),
            },
            aggregator,
        )
    }

    /// Create a sliding time window
    pub fn sliding_time(size: Duration, slide: Duration, aggregator: F) -> Self {
        Self::new(
            format!("sliding_time_{}s_{}s", size.as_secs(), slide.as_secs()),
            WindowType::Sliding {
                size: WindowSize::Time(size),
                slide: WindowSize::Time(slide),
            },
            aggregator,
        )
    }

    /// Create a session window
    pub fn session(gap: Duration, aggregator: F) -> Self {
        Self::new(
            format!("session_{}s", gap.as_secs()),
            WindowType::Session { gap },
            aggregator,
        )
    }

    /// Check if window should be triggered
    async fn should_trigger(&self, buffer: &VecDeque<WindowedItem>) -> bool {
        match &self.window_type {
            WindowType::Tumbling { size } => match size {
                WindowSize::Count(count) => buffer.len() >= *count,
                WindowSize::Time(duration) => {
                    if let (Some(first), Some(last)) = (buffer.front(), buffer.back()) {
                        last.timestamp
                            .duration_since(first.timestamp)
                            .ok()
                            .map(|d| d >= *duration)
                            .unwrap_or(false)
                    } else {
                        false
                    }
                }
            },
            WindowType::Sliding { size, .. } => match size {
                WindowSize::Count(count) => buffer.len() >= *count,
                WindowSize::Time(duration) => {
                    if let (Some(first), Some(last)) = (buffer.front(), buffer.back()) {
                        last.timestamp
                            .duration_since(first.timestamp)
                            .ok()
                            .map(|d| d >= *duration)
                            .unwrap_or(false)
                    } else {
                        false
                    }
                }
            },
            WindowType::Session { gap } => {
                if buffer.len() < 2 {
                    return false;
                }

                // Check if gap between last two items exceeds session gap
                let len = buffer.len();
                if let (Some(prev), Some(current)) = (buffer.get(len - 2), buffer.get(len - 1)) {
                    current
                        .timestamp
                        .duration_since(prev.timestamp)
                        .ok()
                        .map(|d| d >= *gap)
                        .unwrap_or(false)
                } else {
                    false
                }
            }
        }
    }

    /// Extract window from buffer
    async fn extract_window(&self, buffer: &mut VecDeque<WindowedItem>) -> Vec<StreamItem> {
        match &self.window_type {
            WindowType::Tumbling { size } => {
                let count = match size {
                    WindowSize::Count(c) => *c,
                    WindowSize::Time(_) => buffer.len(),
                };

                buffer
                    .drain(..count.min(buffer.len()))
                    .map(|item| item.data)
                    .collect()
            }
            WindowType::Sliding { size, slide } => {
                let window_size = match size {
                    WindowSize::Count(c) => *c,
                    WindowSize::Time(_) => buffer.len(),
                };

                let slide_count = match slide {
                    WindowSize::Count(c) => *c,
                    WindowSize::Time(_) => 1,
                };

                let items: Vec<StreamItem> = buffer
                    .iter()
                    .take(window_size)
                    .map(|item| item.data.clone())
                    .collect();

                // Slide the window
                buffer.drain(..slide_count.min(buffer.len()));

                items
            }
            WindowType::Session { .. } => {
                // Take all items up to the gap
                buffer.drain(..).map(|item| item.data).collect()
            }
        }
    }

    /// Flush remaining items in buffer
    pub async fn flush(&self) -> Result<Vec<StreamItem>> {
        let mut buffer = self.buffer.lock().await;
        if buffer.is_empty() {
            return Ok(Vec::new());
        }

        let items = self.extract_window(&mut buffer).await;
        (self.aggregator)(items).await
    }
}

#[async_trait]
impl<F> Transform for WindowOperator<F>
where
    F: Fn(
            Vec<StreamItem>,
        )
            -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
        + Send
        + Sync,
{
    async fn transform(&self, item: StreamItem) -> Result<Vec<StreamItem>> {
        let mut buffer = self.buffer.lock().await;

        // Add item to buffer with timestamp
        buffer.push_back(WindowedItem {
            data: item,
            timestamp: SystemTime::now(),
        });

        // Check if window should be triggered
        if self.should_trigger(&buffer).await {
            let window_items = self.extract_window(&mut buffer).await;
            drop(buffer);
            (self.aggregator)(window_items).await
        } else {
            Ok(Vec::new())
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Window aggregation functions
pub struct WindowAggregator;

impl WindowAggregator {
    /// Concatenate all items in window
    #[allow(clippy::type_complexity)]
    pub fn concat() -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>,
    > + Send
    + Sync
    + Clone {
        |items| {
            Box::pin(async move {
                let mut result = Vec::new();
                for item in items {
                    result.extend_from_slice(&item);
                }
                Ok(vec![result])
            })
        }
    }

    /// Count items in window
    #[allow(clippy::type_complexity)]
    pub fn count() -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>,
    > + Send
    + Sync
    + Clone {
        |items| {
            Box::pin(async move {
                let count = items.len();
                let json = serde_json::json!({"count": count});
                Ok(vec![serde_json::to_vec(&json)?])
            })
        }
    }

    /// Create JSON array from all items
    #[allow(clippy::type_complexity)]
    pub fn to_array() -> impl Fn(
        Vec<StreamItem>,
    ) -> std::pin::Pin<
        Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>,
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
                let array = serde_json::Value::Array(values);
                Ok(vec![serde_json::to_vec(&array)?])
            })
        }
    }

    /// Calculate statistics on numeric field
    #[allow(clippy::type_complexity)]
    pub fn stats(
        field: String,
    ) -> impl Fn(
        Vec<StreamItem>,
    )
        -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Vec<StreamItem>>> + Send>>
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
                    return Err(TransformError::WindowFailed {
                        message: "No numeric values found".to_string(),
                    }
                    .into());
                }

                let sum: f64 = values.iter().sum();
                let count = values.len() as f64;
                let mean = sum / count;
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

                let stats = serde_json::json!({
                    "count": count,
                    "sum": sum,
                    "mean": mean,
                    "min": min,
                    "max": max,
                });

                Ok(vec![serde_json::to_vec(&stats)?])
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tumbling_count_window() {
        let window = WindowOperator::tumbling_count(3, WindowAggregator::count());

        let result1 = window.transform(vec![1]).await.expect("Failed");
        assert_eq!(result1.len(), 0); // Not enough items

        let result2 = window.transform(vec![2]).await.expect("Failed");
        assert_eq!(result2.len(), 0);

        let result3 = window.transform(vec![3]).await.expect("Failed");
        assert_eq!(result3.len(), 1); // Window triggered

        let stats: serde_json::Value =
            serde_json::from_slice(&result3[0]).expect("Failed to parse");
        assert_eq!(stats.get("count").and_then(|v| v.as_u64()), Some(3));
    }

    #[tokio::test]
    async fn test_window_flush() {
        let window = WindowOperator::tumbling_count(10, WindowAggregator::count());

        window.transform(vec![1]).await.expect("Failed");
        window.transform(vec![2]).await.expect("Failed");

        let flushed = window.flush().await.expect("Failed to flush");
        assert_eq!(flushed.len(), 1);

        let stats: serde_json::Value =
            serde_json::from_slice(&flushed[0]).expect("Failed to parse");
        assert_eq!(stats.get("count").and_then(|v| v.as_u64()), Some(2));
    }

    #[tokio::test]
    async fn test_window_concat() {
        let window = WindowOperator::tumbling_count(2, WindowAggregator::concat());

        window.transform(vec![1, 2]).await.expect("Failed");
        let result = window.transform(vec![3, 4]).await.expect("Failed");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], vec![1, 2, 3, 4]);
    }
}
