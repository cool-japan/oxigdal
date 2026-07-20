//! Message handler implementations

use crate::client::MessageHandler;
use crate::error::Result;
use crate::types::Message;
use async_trait::async_trait;
use std::sync::Arc;

/// Simple callback-based message handler
pub struct SimpleHandler<F>
where
    F: Fn(Message) -> Result<()> + Send + Sync,
{
    /// Callback function
    callback: F,
}

impl<F> SimpleHandler<F>
where
    F: Fn(Message) -> Result<()> + Send + Sync,
{
    /// Create a new simple handler
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

#[async_trait]
impl<F> MessageHandler for SimpleHandler<F>
where
    F: Fn(Message) -> Result<()> + Send + Sync,
{
    async fn handle_message(&self, message: Message) -> Result<()> {
        (self.callback)(message)
    }
}

impl<F> Clone for SimpleHandler<F>
where
    F: Fn(Message) -> Result<()> + Send + Sync + Clone,
{
    fn clone(&self) -> Self {
        Self {
            callback: self.callback.clone(),
        }
    }
}

/// Message callback trait
pub trait MessageCallback: Send + Sync {
    /// Handle a message
    fn on_message(&self, message: Message) -> Result<()>;
}

/// Handler for MessageCallback trait
#[allow(dead_code)]
pub struct CallbackHandler<C: MessageCallback> {
    /// Callback implementation
    callback: Arc<C>,
}

// Public API for callback-based message handling
#[allow(dead_code)]
impl<C: MessageCallback> CallbackHandler<C> {
    /// Create a new callback handler
    pub fn new(callback: C) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }
}

#[async_trait]
impl<C: MessageCallback + 'static> MessageHandler for CallbackHandler<C> {
    async fn handle_message(&self, message: Message) -> Result<()> {
        self.callback.on_message(message)
    }
}

/// JSON message handler
#[allow(dead_code)]
pub struct JsonHandler<T, F>
where
    T: for<'de> serde::Deserialize<'de> + Send + Sync,
    F: Fn(T) -> Result<()> + Send + Sync,
{
    /// Callback function
    callback: F,
    /// Phantom data
    _phantom: std::marker::PhantomData<T>,
}

// Public API for JSON message handling
#[allow(dead_code)]
impl<T, F> JsonHandler<T, F>
where
    T: for<'de> serde::Deserialize<'de> + Send + Sync,
    F: Fn(T) -> Result<()> + Send + Sync,
{
    /// Create a new JSON handler
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, F> MessageHandler for JsonHandler<T, F>
where
    T: for<'de> serde::Deserialize<'de> + Send + Sync,
    F: Fn(T) -> Result<()> + Send + Sync,
{
    async fn handle_message(&self, message: Message) -> Result<()> {
        let payload: T = serde_json::from_slice(&message.payload)?;
        (self.callback)(payload)
    }
}

/// Filtered message handler
#[allow(dead_code)]
pub struct FilteredHandler<H, P>
where
    H: MessageHandler,
    P: Fn(&Message) -> bool + Send + Sync,
{
    /// Inner handler
    handler: H,
    /// Predicate function
    predicate: P,
}

// Public API for filtered message handling
#[allow(dead_code)]
impl<H, P> FilteredHandler<H, P>
where
    H: MessageHandler,
    P: Fn(&Message) -> bool + Send + Sync,
{
    /// Create a new filtered handler
    pub fn new(handler: H, predicate: P) -> Self {
        Self { handler, predicate }
    }
}

#[async_trait]
impl<H, P> MessageHandler for FilteredHandler<H, P>
where
    H: MessageHandler,
    P: Fn(&Message) -> bool + Send + Sync,
{
    async fn handle_message(&self, message: Message) -> Result<()> {
        if (self.predicate)(&message) {
            self.handler.handle_message(message).await
        } else {
            Ok(())
        }
    }
}

/// Logging message handler (wrapper)
#[allow(dead_code)]
pub struct LoggingHandler<H: MessageHandler> {
    /// Inner handler
    handler: H,
    /// Log prefix
    prefix: String,
}

// Public API for logging handler wrapper
#[allow(dead_code)]
impl<H: MessageHandler> LoggingHandler<H> {
    /// Create a new logging handler
    pub fn new(handler: H, prefix: impl Into<String>) -> Self {
        Self {
            handler,
            prefix: prefix.into(),
        }
    }
}

#[async_trait]
impl<H: MessageHandler> MessageHandler for LoggingHandler<H> {
    async fn handle_message(&self, message: Message) -> Result<()> {
        tracing::info!(
            "{} Received message on topic '{}' ({} bytes)",
            self.prefix,
            message.topic,
            message.size()
        );

        let result = self.handler.handle_message(message).await;

        if let Err(ref e) = result {
            tracing::error!("{} Handler error: {}", self.prefix, e);
        }

        result
    }
}

/// Metrics-collecting message handler (wrapper)
#[allow(dead_code)]
pub struct MetricsHandler<H: MessageHandler> {
    /// Inner handler
    handler: H,
    /// Message counter
    message_count: Arc<std::sync::atomic::AtomicU64>,
    /// Byte counter
    byte_count: Arc<std::sync::atomic::AtomicU64>,
    /// Error counter
    error_count: Arc<std::sync::atomic::AtomicU64>,
}

// Public API for metrics-collecting handler wrapper
#[allow(dead_code)]
impl<H: MessageHandler> MetricsHandler<H> {
    /// Create a new metrics handler
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            message_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            byte_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            error_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Get message count
    pub fn message_count(&self) -> u64 {
        self.message_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get byte count
    pub fn byte_count(&self) -> u64 {
        self.byte_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get error count
    pub fn error_count(&self) -> u64 {
        self.error_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Reset metrics
    pub fn reset_metrics(&self) {
        self.message_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.byte_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.error_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

#[async_trait]
impl<H: MessageHandler> MessageHandler for MetricsHandler<H> {
    async fn handle_message(&self, message: Message) -> Result<()> {
        self.message_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.byte_count
            .fetch_add(message.size() as u64, std::sync::atomic::Ordering::Relaxed);

        let result = self.handler.handle_message(message).await;

        if result.is_err() {
            self.error_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::QoS;

    #[tokio::test]
    async fn test_simple_handler() {
        let handler = SimpleHandler::new(|msg: Message| {
            assert_eq!(msg.topic, "test/topic");
            Ok(())
        });

        let message = Message::new("test/topic", b"test".to_vec());
        assert!(handler.handle_message(message).await.is_ok());
    }

    #[tokio::test]
    async fn test_filtered_handler() {
        let inner = SimpleHandler::new(|_: Message| Ok(()));
        let handler = FilteredHandler::new(inner, |msg: &Message| msg.qos == QoS::AtLeastOnce);

        let msg1 = Message::new("test", b"data".to_vec()).with_qos(QoS::AtLeastOnce);
        let msg2 = Message::new("test", b"data".to_vec()).with_qos(QoS::AtMostOnce);

        assert!(handler.handle_message(msg1).await.is_ok());
        assert!(handler.handle_message(msg2).await.is_ok()); // Should be filtered but not error
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        let inner = SimpleHandler::new(|_: Message| Ok(()));
        let handler = MetricsHandler::new(inner);

        assert_eq!(handler.message_count(), 0);
        assert_eq!(handler.byte_count(), 0);

        let message = Message::new("test", b"hello".to_vec());
        handler.handle_message(message).await.ok();

        assert_eq!(handler.message_count(), 1);
        assert_eq!(handler.byte_count(), 5);

        handler.reset_metrics();
        assert_eq!(handler.message_count(), 0);
    }

    #[derive(serde::Deserialize)]
    struct TestData {
        value: i32,
    }

    #[tokio::test]
    async fn test_json_handler() {
        let handler = JsonHandler::new(|data: TestData| {
            assert_eq!(data.value, 42);
            Ok(())
        });

        let json = r#"{"value":42}"#;
        let message = Message::new("test", json.as_bytes().to_vec());

        assert!(handler.handle_message(message).await.is_ok());
    }
}
