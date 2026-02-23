//! Message filtering for selective broadcasting

use crate::protocol::message::{Message, MessageType};
use crate::server::connection::ConnectionId;
use std::collections::HashSet;

/// Filter predicate trait
pub trait FilterPredicate: Send + Sync {
    /// Check if a message should be delivered to a connection
    fn should_deliver(&self, message: &Message, connection_id: &ConnectionId) -> bool;
}

/// Message filter
pub enum MessageFilter {
    /// Accept all messages
    All,
    /// Accept messages of specific types
    MessageType(HashSet<MessageType>),
    /// Accept messages from specific connections
    FromConnections(HashSet<ConnectionId>),
    /// Reject messages from specific connections
    ExcludeConnections(HashSet<ConnectionId>),
    /// Custom filter predicate
    Custom(Box<dyn FilterPredicate>),
}

impl MessageFilter {
    /// Create an "accept all" filter
    pub fn all() -> Self {
        Self::All
    }

    /// Create a filter for specific message types
    pub fn message_types(types: Vec<MessageType>) -> Self {
        Self::MessageType(types.into_iter().collect())
    }

    /// Create a filter for specific connections
    pub fn from_connections(connections: Vec<ConnectionId>) -> Self {
        Self::FromConnections(connections.into_iter().collect())
    }

    /// Create a filter to exclude specific connections
    pub fn exclude_connections(connections: Vec<ConnectionId>) -> Self {
        Self::ExcludeConnections(connections.into_iter().collect())
    }

    /// Check if a message should be delivered
    pub fn should_deliver(&self, message: &Message, connection_id: &ConnectionId) -> bool {
        match self {
            Self::All => true,
            Self::MessageType(types) => types.contains(&message.msg_type),
            Self::FromConnections(conns) => conns.contains(connection_id),
            Self::ExcludeConnections(conns) => !conns.contains(connection_id),
            Self::Custom(predicate) => predicate.should_deliver(message, connection_id),
        }
    }
}

/// Filter chain for combining multiple filters
pub struct FilterChain {
    filters: Vec<MessageFilter>,
    /// If true, all filters must pass (AND). If false, any filter can pass (OR)
    all_must_pass: bool,
}

impl FilterChain {
    /// Create a new filter chain (AND logic)
    pub fn new_and() -> Self {
        Self {
            filters: Vec::new(),
            all_must_pass: true,
        }
    }

    /// Create a new filter chain (OR logic)
    pub fn new_or() -> Self {
        Self {
            filters: Vec::new(),
            all_must_pass: false,
        }
    }

    /// Add a filter to the chain
    pub fn add_filter(mut self, filter: MessageFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Check if a message should be delivered
    pub fn should_deliver(&self, message: &Message, connection_id: &ConnectionId) -> bool {
        if self.filters.is_empty() {
            return true;
        }

        if self.all_must_pass {
            // AND logic - all filters must pass
            self.filters
                .iter()
                .all(|f| f.should_deliver(message, connection_id))
        } else {
            // OR logic - any filter can pass
            self.filters
                .iter()
                .any(|f| f.should_deliver(message, connection_id))
        }
    }

    /// Get filter count
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }
}

/// Geographic bounding box filter
pub struct GeoBboxFilter {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl GeoBboxFilter {
    /// Create a new geographic bounding box filter
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Check if coordinates are within bounds
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

impl FilterPredicate for GeoBboxFilter {
    fn should_deliver(&self, _message: &Message, _connection_id: &ConnectionId) -> bool {
        // In a real implementation, would extract coordinates from message
        // For now, accept all
        true
    }
}

/// Attribute-based filter
#[allow(dead_code)]
pub struct AttributeFilter {
    key: String,
    value: String,
}

impl AttributeFilter {
    /// Create a new attribute filter
    pub fn new(key: String, value: String) -> Self {
        Self { key, value }
    }
}

impl FilterPredicate for AttributeFilter {
    fn should_deliver(&self, _message: &Message, _connection_id: &ConnectionId) -> bool {
        // In a real implementation, would check message attributes
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_filter_all() {
        let filter = MessageFilter::all();
        let msg = Message::ping();
        let conn_id = Uuid::new_v4();

        assert!(filter.should_deliver(&msg, &conn_id));
    }

    #[test]
    fn test_filter_message_type() {
        let filter = MessageFilter::message_types(vec![MessageType::Ping, MessageType::Pong]);
        let ping = Message::ping();
        let pong = Message::pong();
        let data = Message::data(bytes::Bytes::new());
        let conn_id = Uuid::new_v4();

        assert!(filter.should_deliver(&ping, &conn_id));
        assert!(filter.should_deliver(&pong, &conn_id));
        assert!(!filter.should_deliver(&data, &conn_id));
    }

    #[test]
    fn test_filter_from_connections() {
        let conn1 = Uuid::new_v4();
        let conn2 = Uuid::new_v4();
        let conn3 = Uuid::new_v4();

        let filter = MessageFilter::from_connections(vec![conn1, conn2]);
        let msg = Message::ping();

        assert!(filter.should_deliver(&msg, &conn1));
        assert!(filter.should_deliver(&msg, &conn2));
        assert!(!filter.should_deliver(&msg, &conn3));
    }

    #[test]
    fn test_filter_exclude_connections() {
        let conn1 = Uuid::new_v4();
        let conn2 = Uuid::new_v4();

        let filter = MessageFilter::exclude_connections(vec![conn1]);
        let msg = Message::ping();

        assert!(!filter.should_deliver(&msg, &conn1));
        assert!(filter.should_deliver(&msg, &conn2));
    }

    #[test]
    fn test_filter_chain_and() {
        let conn1 = Uuid::new_v4();

        let chain = FilterChain::new_and()
            .add_filter(MessageFilter::message_types(vec![MessageType::Ping]))
            .add_filter(MessageFilter::from_connections(vec![conn1]));

        let ping = Message::ping();
        let pong = Message::pong();

        assert!(chain.should_deliver(&ping, &conn1));
        assert!(!chain.should_deliver(&pong, &conn1));
    }

    #[test]
    fn test_filter_chain_or() {
        let conn1 = Uuid::new_v4();
        let conn2 = Uuid::new_v4();

        let chain = FilterChain::new_or()
            .add_filter(MessageFilter::message_types(vec![MessageType::Ping]))
            .add_filter(MessageFilter::from_connections(vec![conn1]));

        let ping = Message::ping();
        let data = Message::data(bytes::Bytes::new());

        assert!(chain.should_deliver(&ping, &conn1));
        assert!(chain.should_deliver(&ping, &conn2));
        assert!(chain.should_deliver(&data, &conn1));
        assert!(!chain.should_deliver(&data, &conn2));
    }

    #[test]
    fn test_geo_bbox_filter() {
        let filter = GeoBboxFilter::new(-180.0, -90.0, 180.0, 90.0);

        assert!(filter.contains(0.0, 0.0));
        assert!(filter.contains(-122.4, 37.8));
        assert!(!filter.contains(200.0, 100.0));
    }
}
