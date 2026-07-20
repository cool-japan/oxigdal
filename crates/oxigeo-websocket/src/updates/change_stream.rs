//! Change stream processing for real-time updates

use crate::error::Result;
use crate::protocol::message::{ChangePayload, ChangeType, Message, MessageType, Payload};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// Change stream configuration
#[derive(Debug, Clone)]
pub struct ChangeStreamConfig {
    /// Maximum buffer size
    pub max_buffer_size: usize,
    /// Enable change deduplication
    pub enable_deduplication: bool,
    /// Broadcast channel capacity
    pub broadcast_capacity: usize,
}

impl Default for ChangeStreamConfig {
    fn default() -> Self {
        Self {
            max_buffer_size: 10_000,
            enable_deduplication: true,
            broadcast_capacity: 1000,
        }
    }
}

/// Change event
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    /// Change ID (monotonically increasing)
    pub change_id: u64,
    /// Collection/layer name
    pub collection: String,
    /// Change type
    pub change_type: ChangeType,
    /// Document/feature ID
    pub document_id: String,
    /// Change data
    pub data: Option<serde_json::Value>,
    /// Timestamp
    pub timestamp: i64,
}

impl ChangeEvent {
    /// Create a new change event
    pub fn new(
        change_id: u64,
        collection: String,
        change_type: ChangeType,
        document_id: String,
        data: Option<serde_json::Value>,
    ) -> Self {
        Self {
            change_id,
            collection,
            change_type,
            document_id,
            data,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Convert to message
    pub fn to_message(&self) -> Message {
        let payload = Payload::ChangeEvent(ChangePayload {
            change_id: self.change_id,
            collection: self.collection.clone(),
            change_type: self.change_type,
            document_id: self.document_id.clone(),
            data: self.data.clone(),
        });

        Message::new(MessageType::ChangeStream, payload)
    }
}

/// Change stream
pub struct ChangeStream {
    name: String,
    config: ChangeStreamConfig,
    buffer: Arc<RwLock<VecDeque<ChangeEvent>>>,
    next_change_id: Arc<AtomicU64>,
    tx: broadcast::Sender<ChangeEvent>,
    stats: Arc<ChangeStreamStats>,
}

/// Change stream statistics
struct ChangeStreamStats {
    total_events: AtomicU64,
    created_events: AtomicU64,
    updated_events: AtomicU64,
    deleted_events: AtomicU64,
    dropped_events: AtomicU64,
    deduplicated_events: AtomicU64,
}

impl ChangeStream {
    /// Create a new change stream
    pub fn new(name: String, config: ChangeStreamConfig) -> Self {
        let (tx, _) = broadcast::channel(config.broadcast_capacity);

        Self {
            name,
            config,
            buffer: Arc::new(RwLock::new(VecDeque::new())),
            next_change_id: Arc::new(AtomicU64::new(1)),
            tx,
            stats: Arc::new(ChangeStreamStats {
                total_events: AtomicU64::new(0),
                created_events: AtomicU64::new(0),
                updated_events: AtomicU64::new(0),
                deleted_events: AtomicU64::new(0),
                dropped_events: AtomicU64::new(0),
                deduplicated_events: AtomicU64::new(0),
            }),
        }
    }

    /// Get stream name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Add a change event
    pub fn add_event(
        &self,
        collection: String,
        change_type: ChangeType,
        document_id: String,
        data: Option<serde_json::Value>,
    ) -> Result<u64> {
        let change_id = self.next_change_id.fetch_add(1, Ordering::Relaxed);

        let event = ChangeEvent::new(change_id, collection, change_type, document_id, data);

        // Update statistics
        self.stats.total_events.fetch_add(1, Ordering::Relaxed);
        match change_type {
            ChangeType::Created => {
                self.stats.created_events.fetch_add(1, Ordering::Relaxed);
            }
            ChangeType::Updated => {
                self.stats.updated_events.fetch_add(1, Ordering::Relaxed);
            }
            ChangeType::Deleted => {
                self.stats.deleted_events.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Check for deduplication
        if self.config.enable_deduplication && self.is_duplicate(&event) {
            self.stats
                .deduplicated_events
                .fetch_add(1, Ordering::Relaxed);
            return Ok(change_id);
        }

        // Add to buffer
        let mut buffer = self.buffer.write();
        if buffer.len() >= self.config.max_buffer_size {
            buffer.pop_front();
            self.stats.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
        buffer.push_back(event.clone());
        drop(buffer);

        // Broadcast to subscribers
        let _ = self.tx.send(event);

        Ok(change_id)
    }

    /// Check if an event is a duplicate
    fn is_duplicate(&self, event: &ChangeEvent) -> bool {
        let buffer = self.buffer.read();

        // Check last few events for duplicates (same collection and document)
        buffer.iter().rev().take(10).any(|e| {
            e.collection == event.collection
                && e.document_id == event.document_id
                && e.change_type == event.change_type
                && e.timestamp.abs_diff(event.timestamp) < 1000 // Within 1 second
        })
    }

    /// Subscribe to change events
    pub fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.tx.subscribe()
    }

    /// Get buffered events
    pub fn get_events(&self, since_change_id: Option<u64>) -> Vec<ChangeEvent> {
        let buffer = self.buffer.read();

        if let Some(since_id) = since_change_id {
            buffer
                .iter()
                .filter(|e| e.change_id > since_id)
                .cloned()
                .collect()
        } else {
            buffer.iter().cloned().collect()
        }
    }

    /// Get buffer size
    pub fn buffer_size(&self) -> usize {
        self.buffer.read().len()
    }

    /// Clear buffer
    pub fn clear(&self) {
        self.buffer.write().clear();
    }

    /// Get statistics
    pub fn stats(&self) -> ChangeStreamStatsSnapshot {
        ChangeStreamStatsSnapshot {
            name: self.name.clone(),
            total_events: self.stats.total_events.load(Ordering::Relaxed),
            created_events: self.stats.created_events.load(Ordering::Relaxed),
            updated_events: self.stats.updated_events.load(Ordering::Relaxed),
            deleted_events: self.stats.deleted_events.load(Ordering::Relaxed),
            dropped_events: self.stats.dropped_events.load(Ordering::Relaxed),
            deduplicated_events: self.stats.deduplicated_events.load(Ordering::Relaxed),
            buffer_size: self.buffer_size(),
        }
    }
}

/// Change stream statistics snapshot
#[derive(Debug, Clone)]
pub struct ChangeStreamStatsSnapshot {
    /// Stream name
    pub name: String,
    /// Total events
    pub total_events: u64,
    /// Created events
    pub created_events: u64,
    /// Updated events
    pub updated_events: u64,
    /// Deleted events
    pub deleted_events: u64,
    /// Dropped events
    pub dropped_events: u64,
    /// Deduplicated events
    pub deduplicated_events: u64,
    /// Current buffer size
    pub buffer_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_event() {
        let event = ChangeEvent::new(
            1,
            "collection".to_string(),
            ChangeType::Created,
            "doc1".to_string(),
            None,
        );

        assert_eq!(event.change_id, 1);
        assert_eq!(event.collection, "collection");
        assert_eq!(event.change_type, ChangeType::Created);
    }

    #[test]
    fn test_change_stream() {
        let config = ChangeStreamConfig::default();
        let stream = ChangeStream::new("test".to_string(), config);

        assert_eq!(stream.name(), "test");
        assert_eq!(stream.buffer_size(), 0);
    }

    #[test]
    fn test_change_stream_add_event() -> Result<()> {
        let config = ChangeStreamConfig::default();
        let stream = ChangeStream::new("test".to_string(), config);

        let change_id = stream.add_event(
            "collection".to_string(),
            ChangeType::Created,
            "doc1".to_string(),
            None,
        )?;

        assert_eq!(change_id, 1);
        assert_eq!(stream.buffer_size(), 1);
        Ok(())
    }

    #[test]
    fn test_change_stream_get_events() -> Result<()> {
        let config = ChangeStreamConfig::default();
        let stream = ChangeStream::new("test".to_string(), config);

        stream.add_event(
            "collection".to_string(),
            ChangeType::Created,
            "doc1".to_string(),
            None,
        )?;

        stream.add_event(
            "collection".to_string(),
            ChangeType::Updated,
            "doc1".to_string(),
            None,
        )?;

        let events = stream.get_events(None);
        assert_eq!(events.len(), 2);

        let events_since = stream.get_events(Some(1));
        assert_eq!(events_since.len(), 1);
        Ok(())
    }

    #[test]
    fn test_change_stream_stats() -> Result<()> {
        let config = ChangeStreamConfig::default();
        let stream = ChangeStream::new("test".to_string(), config);

        stream.add_event(
            "collection".to_string(),
            ChangeType::Created,
            "doc1".to_string(),
            None,
        )?;

        stream.add_event(
            "collection".to_string(),
            ChangeType::Updated,
            "doc2".to_string(),
            None,
        )?;

        stream.add_event(
            "collection".to_string(),
            ChangeType::Deleted,
            "doc3".to_string(),
            None,
        )?;

        let stats = stream.stats();
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.created_events, 1);
        assert_eq!(stats.updated_events, 1);
        assert_eq!(stats.deleted_events, 1);
        Ok(())
    }

    #[test]
    fn test_change_stream_max_buffer() -> Result<()> {
        let config = ChangeStreamConfig {
            max_buffer_size: 2,
            ..Default::default()
        };
        let stream = ChangeStream::new("test".to_string(), config);

        stream.add_event("c".to_string(), ChangeType::Created, "d1".to_string(), None)?;
        stream.add_event("c".to_string(), ChangeType::Created, "d2".to_string(), None)?;
        stream.add_event("c".to_string(), ChangeType::Created, "d3".to_string(), None)?;

        // Buffer should only contain 2 events
        assert_eq!(stream.buffer_size(), 2);

        let stats = stream.stats();
        assert_eq!(stats.dropped_events, 1);
        Ok(())
    }
}
