//! Stream operators for processing data.

use crate::core::stream::{StreamElement, StreamMessage};
use crate::error::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// Base trait for stream operators.
#[async_trait]
pub trait StreamOperator: Send + Sync {
    /// Process a stream message.
    async fn process(&mut self, message: StreamMessage) -> Result<Vec<StreamMessage>>;

    /// Get the operator name.
    fn name(&self) -> &str;

    /// Initialize the operator.
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    /// Finalize the operator.
    async fn finalize(&mut self) -> Result<()> {
        Ok(())
    }
}

/// A source operator that produces stream elements.
#[async_trait]
pub trait SourceOperator: Send + Sync {
    /// Produce the next batch of elements.
    async fn produce(&mut self) -> Result<Vec<StreamMessage>>;

    /// Check if the source has more data.
    async fn has_more(&self) -> bool;

    /// Get the source name.
    fn name(&self) -> &str;
}

/// A sink operator that consumes stream elements.
#[async_trait]
pub trait SinkOperator: Send + Sync {
    /// Consume a batch of elements.
    async fn consume(&mut self, messages: Vec<StreamMessage>) -> Result<()>;

    /// Flush any buffered data.
    async fn flush(&mut self) -> Result<()>;

    /// Get the sink name.
    fn name(&self) -> &str;
}

/// A transform operator that modifies stream elements.
#[async_trait]
pub trait TransformOperator: StreamOperator {
    /// Transform a single element.
    async fn transform(&mut self, element: StreamElement) -> Result<Vec<StreamElement>>;
}

/// A filter operator.
pub struct FilterOperator<F>
where
    F: Fn(&StreamElement) -> bool + Send + Sync,
{
    predicate: Arc<F>,
    name: String,
}

impl<F> FilterOperator<F>
where
    F: Fn(&StreamElement) -> bool + Send + Sync,
{
    /// Create a new filter operator.
    pub fn new(predicate: F, name: String) -> Self {
        Self {
            predicate: Arc::new(predicate),
            name,
        }
    }
}

#[async_trait]
impl<F> StreamOperator for FilterOperator<F>
where
    F: Fn(&StreamElement) -> bool + Send + Sync,
{
    async fn process(&mut self, message: StreamMessage) -> Result<Vec<StreamMessage>> {
        match message {
            StreamMessage::Data(elem) => {
                if (self.predicate)(&elem) {
                    Ok(vec![StreamMessage::Data(elem)])
                } else {
                    Ok(vec![])
                }
            }
            other => Ok(vec![other]),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// A map operator.
pub struct MapOperator<F>
where
    F: Fn(StreamElement) -> StreamElement + Send + Sync,
{
    mapper: Arc<F>,
    name: String,
}

impl<F> MapOperator<F>
where
    F: Fn(StreamElement) -> StreamElement + Send + Sync,
{
    /// Create a new map operator.
    pub fn new(mapper: F, name: String) -> Self {
        Self {
            mapper: Arc::new(mapper),
            name,
        }
    }
}

#[async_trait]
impl<F> StreamOperator for MapOperator<F>
where
    F: Fn(StreamElement) -> StreamElement + Send + Sync,
{
    async fn process(&mut self, message: StreamMessage) -> Result<Vec<StreamMessage>> {
        match message {
            StreamMessage::Data(elem) => {
                let transformed = (self.mapper)(elem);
                Ok(vec![StreamMessage::Data(transformed)])
            }
            other => Ok(vec![other]),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// A flat map operator.
pub struct FlatMapOperator<F>
where
    F: Fn(StreamElement) -> Vec<StreamElement> + Send + Sync,
{
    mapper: Arc<F>,
    name: String,
}

impl<F> FlatMapOperator<F>
where
    F: Fn(StreamElement) -> Vec<StreamElement> + Send + Sync,
{
    /// Create a new flat map operator.
    pub fn new(mapper: F, name: String) -> Self {
        Self {
            mapper: Arc::new(mapper),
            name,
        }
    }
}

#[async_trait]
impl<F> StreamOperator for FlatMapOperator<F>
where
    F: Fn(StreamElement) -> Vec<StreamElement> + Send + Sync,
{
    async fn process(&mut self, message: StreamMessage) -> Result<Vec<StreamMessage>> {
        match message {
            StreamMessage::Data(elem) => {
                let elements = (self.mapper)(elem);
                Ok(elements.into_iter().map(StreamMessage::Data).collect())
            }
            other => Ok(vec![other]),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// A logging sink operator for debugging.
pub struct LoggingSink {
    name: String,
    count: u64,
}

impl LoggingSink {
    /// Create a new logging sink.
    pub fn new(name: String) -> Self {
        Self { name, count: 0 }
    }

    /// Get the number of messages logged.
    pub fn count(&self) -> u64 {
        self.count
    }
}

#[async_trait]
impl SinkOperator for LoggingSink {
    async fn consume(&mut self, messages: Vec<StreamMessage>) -> Result<()> {
        for msg in messages {
            match msg {
                StreamMessage::Data(elem) => {
                    tracing::debug!(
                        sink = %self.name,
                        event_time = %elem.event_time,
                        size = elem.size_bytes(),
                        "Received element"
                    );
                    self.count += 1;
                }
                StreamMessage::Watermark(wm) => {
                    tracing::debug!(sink = %self.name, watermark = %wm, "Received watermark");
                }
                StreamMessage::Checkpoint(id) => {
                    tracing::debug!(sink = %self.name, checkpoint = id, "Received checkpoint");
                }
                StreamMessage::EndOfStream => {
                    tracing::info!(sink = %self.name, "Received end of stream");
                }
            }
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        tracing::debug!(sink = %self.name, "Flushing sink");
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_filter_operator() {
        let mut filter = FilterOperator::new(
            |elem: &StreamElement| elem.data.len() > 2,
            "test_filter".to_string(),
        );

        let elem1 = StreamElement::new(vec![1, 2, 3], Utc::now());
        let elem2 = StreamElement::new(vec![1], Utc::now());

        let result1 = filter
            .process(StreamMessage::Data(elem1))
            .await
            .expect("filter should process element");
        assert_eq!(result1.len(), 1);

        let result2 = filter
            .process(StreamMessage::Data(elem2))
            .await
            .expect("filter should process element");
        assert_eq!(result2.len(), 0);
    }

    #[tokio::test]
    async fn test_map_operator() {
        let mut mapper = MapOperator::new(
            |mut elem: StreamElement| {
                elem.data.push(99);
                elem
            },
            "test_map".to_string(),
        );

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        let result = mapper
            .process(StreamMessage::Data(elem))
            .await
            .expect("map should transform element");

        assert_eq!(result.len(), 1);
        if let StreamMessage::Data(transformed) = &result[0] {
            assert_eq!(transformed.data.len(), 4);
            assert_eq!(transformed.data[3], 99);
        } else {
            panic!("Expected data message");
        }
    }

    #[tokio::test]
    async fn test_flat_map_operator() {
        let mut flat_mapper = FlatMapOperator::new(
            |elem: StreamElement| {
                vec![
                    StreamElement::new(elem.data.clone(), elem.event_time),
                    StreamElement::new(elem.data.clone(), elem.event_time),
                ]
            },
            "test_flatmap".to_string(),
        );

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        let result = flat_mapper
            .process(StreamMessage::Data(elem))
            .await
            .expect("flat_map should process element");

        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_logging_sink() {
        let mut sink = LoggingSink::new("test_sink".to_string());

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        sink.consume(vec![StreamMessage::Data(elem)])
            .await
            .expect("sink should consume element");

        assert_eq!(sink.count(), 1);
    }
}
