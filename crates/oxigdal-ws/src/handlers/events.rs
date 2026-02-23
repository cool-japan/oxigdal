//! Event streaming handler.

use crate::error::{Error, Result};
use crate::protocol::{EventType, Message};
use crate::stream::EventData;
use crate::subscription::SubscriptionManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Event streaming handler.
pub struct EventHandler {
    /// Subscription manager
    subscriptions: Arc<SubscriptionManager>,
    /// Client message senders
    client_senders: Arc<DashMap<String, mpsc::UnboundedSender<Message>>>,
    /// Event history for replay
    event_history: Arc<DashMap<EventType, Vec<EventData>>>,
    /// Maximum history size per event type
    max_history_size: usize,
}

impl EventHandler {
    /// Create a new event handler.
    pub fn new(subscriptions: Arc<SubscriptionManager>) -> Self {
        Self {
            subscriptions,
            client_senders: Arc::new(DashMap::new()),
            event_history: Arc::new(DashMap::new()),
            max_history_size: 1000,
        }
    }

    /// Create a new event handler with custom history size.
    pub fn with_history_size(
        subscriptions: Arc<SubscriptionManager>,
        max_history_size: usize,
    ) -> Self {
        Self {
            subscriptions,
            client_senders: Arc::new(DashMap::new()),
            event_history: Arc::new(DashMap::new()),
            max_history_size,
        }
    }

    /// Register a client sender.
    pub fn register_client(&self, client_id: String, sender: mpsc::UnboundedSender<Message>) {
        self.client_senders.insert(client_id, sender);
    }

    /// Unregister a client.
    pub fn unregister_client(&self, client_id: &str) {
        self.client_senders.remove(client_id);
    }

    /// Stream an event to subscribers.
    pub async fn stream_event(&self, event: EventData) -> Result<usize> {
        debug!("Streaming event: type={:?}", event.event_type);

        // Store in history
        self.add_to_history(event.clone());

        // Find matching subscriptions
        let subscriptions = self
            .subscriptions
            .find_event_subscriptions(event.event_type);

        if subscriptions.is_empty() {
            return Ok(0);
        }

        let mut sent_count = 0;

        for subscription in subscriptions {
            let client_id = &subscription.client_id;

            // Send event to client
            if let Some(sender) = self.client_senders.get(client_id) {
                let message = Message::Event {
                    subscription_id: subscription.id.clone(),
                    event_type: event.event_type,
                    payload: event.payload.clone(),
                    timestamp: event.timestamp.to_rfc3339(),
                };

                if sender.send(message).is_ok() {
                    sent_count += 1;
                } else {
                    warn!("Failed to send event to client {}", client_id);
                }
            }
        }

        Ok(sent_count)
    }

    /// Add event to history.
    fn add_to_history(&self, event: EventData) {
        let event_type = event.event_type;

        self.event_history
            .entry(event_type)
            .and_modify(|history| {
                history.push(event.clone());
                // Maintain max size
                if history.len() > self.max_history_size {
                    history.remove(0);
                }
            })
            .or_insert_with(|| vec![event]);
    }

    /// Stream multiple events.
    pub async fn stream_events(&self, events: Vec<EventData>) -> Result<usize> {
        let mut total_sent = 0;

        for event in events {
            total_sent += self.stream_event(event).await?;
        }

        Ok(total_sent)
    }

    /// Notify file change.
    pub async fn notify_file_change(&self, file_path: &str, change_type: &str) -> Result<usize> {
        let payload = serde_json::json!({
            "file_path": file_path,
            "change_type": change_type,
        });

        let event = EventData::new(EventType::FileChange, payload);
        self.stream_event(event).await
    }

    /// Notify processing status.
    pub async fn notify_processing_status(
        &self,
        task_id: &str,
        status: &str,
        progress: Option<f64>,
    ) -> Result<usize> {
        let mut payload = serde_json::json!({
            "task_id": task_id,
            "status": status,
        });

        if let Some(progress_val) = progress {
            payload["progress"] = serde_json::json!(progress_val);
        }

        let event = EventData::new(EventType::ProcessingStatus, payload);
        self.stream_event(event).await
    }

    /// Notify error.
    pub async fn notify_error(
        &self,
        error_message: &str,
        context: Option<serde_json::Value>,
    ) -> Result<usize> {
        let mut payload = serde_json::json!({
            "error": error_message,
        });

        if let Some(ctx) = context {
            payload["context"] = ctx;
        }

        let event = EventData::new(EventType::Error, payload);
        self.stream_event(event).await
    }

    /// Stream progress updates.
    pub async fn stream_progress(
        &self,
        task_id: &str,
        current: usize,
        total: usize,
        message: Option<&str>,
    ) -> Result<usize> {
        let progress = if total > 0 {
            (current as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let mut payload = serde_json::json!({
            "task_id": task_id,
            "current": current,
            "total": total,
            "progress_percent": progress,
        });

        if let Some(msg) = message {
            payload["message"] = serde_json::json!(msg);
        }

        let event = EventData::new(EventType::Progress, payload);
        self.stream_event(event).await
    }

    /// Stream custom event.
    pub async fn stream_custom_event(&self, payload: serde_json::Value) -> Result<usize> {
        let event = EventData::new(EventType::Custom, payload);
        self.stream_event(event).await
    }

    /// Get event history for a specific type.
    pub fn get_history(&self, event_type: EventType) -> Vec<EventData> {
        self.event_history
            .get(&event_type)
            .map(|h| h.clone())
            .unwrap_or_default()
    }

    /// Get recent events of a specific type.
    pub fn get_recent_events(&self, event_type: EventType, count: usize) -> Vec<EventData> {
        if let Some(history) = self.event_history.get(&event_type) {
            let start = if history.len() > count {
                history.len() - count
            } else {
                0
            };
            history[start..].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Replay events to a new subscriber.
    pub async fn replay_events(
        &self,
        client_id: &str,
        event_type: EventType,
        count: Option<usize>,
    ) -> Result<usize> {
        let events = if let Some(n) = count {
            self.get_recent_events(event_type, n)
        } else {
            self.get_history(event_type)
        };

        if events.is_empty() {
            return Ok(0);
        }

        let sender = self
            .client_senders
            .get(client_id)
            .ok_or_else(|| Error::NotFound(format!("Client not found: {}", client_id)))?;

        let mut sent_count = 0;

        for event in events {
            // Find subscription for this client and event type
            let subscriptions = self.subscriptions.get_client_subscriptions(client_id);

            for subscription in subscriptions {
                if subscription.matches_event(event_type) {
                    let message = Message::Event {
                        subscription_id: subscription.id.clone(),
                        event_type: event.event_type,
                        payload: event.payload.clone(),
                        timestamp: event.timestamp.to_rfc3339(),
                    };

                    if sender.send(message).is_ok() {
                        sent_count += 1;
                    }
                    break;
                }
            }
        }

        Ok(sent_count)
    }

    /// Clear event history.
    pub fn clear_history(&self) {
        self.event_history.clear();
    }

    /// Clear history for a specific event type.
    pub fn clear_history_for_type(&self, event_type: EventType) {
        self.event_history.remove(&event_type);
    }

    /// Get total event count across all types.
    pub fn total_event_count(&self) -> usize {
        self.event_history
            .iter()
            .map(|entry| entry.value().len())
            .sum()
    }

    /// Get event count for a specific type.
    pub fn event_count_for_type(&self, event_type: EventType) -> usize {
        self.event_history
            .get(&event_type)
            .map(|h| h.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_handler_creation() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::new(subscriptions);

        assert_eq!(handler.total_event_count(), 0);
        assert_eq!(handler.max_history_size, 1000);
    }

    #[tokio::test]
    async fn test_stream_event() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::new(subscriptions);

        let payload = serde_json::json!({"test": "data"});
        let event = EventData::new(EventType::Custom, payload);

        // Stream event (won't send to anyone as no subscriptions)
        let result = handler.stream_event(event).await;
        assert!(result.is_ok());

        // Check it was added to history
        assert_eq!(handler.event_count_for_type(EventType::Custom), 1);
    }

    #[tokio::test]
    async fn test_notify_file_change() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::new(subscriptions);

        let result = handler
            .notify_file_change("/path/to/file.tif", "modified")
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.event_count_for_type(EventType::FileChange), 1);
    }

    #[tokio::test]
    async fn test_notify_processing_status() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::new(subscriptions);

        let result = handler
            .notify_processing_status("task-123", "running", Some(50.0))
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.event_count_for_type(EventType::ProcessingStatus), 1);
    }

    #[tokio::test]
    async fn test_stream_progress() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::new(subscriptions);

        let result = handler
            .stream_progress("task-123", 50, 100, Some("Processing tiles"))
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.event_count_for_type(EventType::Progress), 1);
    }

    #[tokio::test]
    async fn test_event_history() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::new(subscriptions);

        // Add multiple events
        for i in 0..5 {
            let payload = serde_json::json!({"index": i});
            let event = EventData::new(EventType::Custom, payload);
            let result = handler.stream_event(event).await;
            assert!(result.is_ok());
        }

        assert_eq!(handler.event_count_for_type(EventType::Custom), 5);

        let recent = handler.get_recent_events(EventType::Custom, 3);
        assert_eq!(recent.len(), 3);

        handler.clear_history_for_type(EventType::Custom);
        assert_eq!(handler.event_count_for_type(EventType::Custom), 0);
    }

    #[tokio::test]
    async fn test_history_size_limit() {
        let subscriptions = Arc::new(SubscriptionManager::new());
        let handler = EventHandler::with_history_size(subscriptions, 10);

        // Add more events than max size
        for i in 0..20 {
            let payload = serde_json::json!({"index": i});
            let event = EventData::new(EventType::Custom, payload);
            let result = handler.stream_event(event).await;
            assert!(result.is_ok());
        }

        // Should only keep last 10
        assert_eq!(handler.event_count_for_type(EventType::Custom), 10);
    }
}
