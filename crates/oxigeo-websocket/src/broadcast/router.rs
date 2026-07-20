//! Message routing and distribution

use crate::protocol::message::{Message, MessageType};
use crate::server::connection::ConnectionId;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Routing strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Broadcast to all connections
    Broadcast,
    /// Send to specific connection
    Direct,
    /// Send to connections in a room
    Room,
    /// Send to topic subscribers
    Topic,
    /// Custom routing logic
    Custom,
}

/// Routing rule
#[derive(Clone)]
pub struct RoutingRule {
    /// Rule name
    pub name: String,
    /// Message type to match
    pub message_type: Option<MessageType>,
    /// Routing strategy
    pub strategy: RoutingStrategy,
    /// Target (room name, topic, etc.)
    pub target: Option<String>,
    /// Priority (higher priority rules are evaluated first)
    pub priority: i32,
}

impl RoutingRule {
    /// Create a new routing rule
    pub fn new(name: String, strategy: RoutingStrategy) -> Self {
        Self {
            name,
            message_type: None,
            strategy,
            target: None,
            priority: 0,
        }
    }

    /// Set message type filter
    pub fn with_message_type(mut self, msg_type: MessageType) -> Self {
        self.message_type = Some(msg_type);
        self
    }

    /// Set target
    pub fn with_target(mut self, target: String) -> Self {
        self.target = Some(target);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Check if this rule matches a message
    pub fn matches(&self, message: &Message) -> bool {
        if let Some(msg_type) = self.message_type {
            message.msg_type == msg_type
        } else {
            true
        }
    }
}

/// Route result
pub struct RouteResult {
    /// Routing strategy to use
    pub strategy: RoutingStrategy,
    /// Target connections
    pub targets: Vec<ConnectionId>,
    /// Target identifier (room name, topic, etc.)
    pub target_id: Option<String>,
}

impl RouteResult {
    /// Create a broadcast route
    pub fn broadcast() -> Self {
        Self {
            strategy: RoutingStrategy::Broadcast,
            targets: Vec::new(),
            target_id: None,
        }
    }

    /// Create a direct route
    pub fn direct(target: ConnectionId) -> Self {
        Self {
            strategy: RoutingStrategy::Direct,
            targets: vec![target],
            target_id: None,
        }
    }

    /// Create a room route
    pub fn room(room_name: String) -> Self {
        Self {
            strategy: RoutingStrategy::Room,
            targets: Vec::new(),
            target_id: Some(room_name),
        }
    }

    /// Create a topic route
    pub fn topic(topic_name: String) -> Self {
        Self {
            strategy: RoutingStrategy::Topic,
            targets: Vec::new(),
            target_id: Some(topic_name),
        }
    }
}

/// Message router
pub struct MessageRouter {
    rules: Arc<RwLock<Vec<RoutingRule>>>,
    default_strategy: RoutingStrategy,
}

impl MessageRouter {
    /// Create a new message router
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            default_strategy: RoutingStrategy::Broadcast,
        }
    }

    /// Create a router with a default strategy
    pub fn with_default_strategy(strategy: RoutingStrategy) -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            default_strategy: strategy,
        }
    }

    /// Add a routing rule
    pub fn add_rule(&self, rule: RoutingRule) {
        let mut rules = self.rules.write();
        rules.push(rule);
        // Sort by priority (highest first)
        rules.sort_by_key(|x| std::cmp::Reverse(x.priority));
    }

    /// Remove a routing rule by name
    pub fn remove_rule(&self, name: &str) -> bool {
        let mut rules = self.rules.write();
        if let Some(pos) = rules.iter().position(|r| r.name == name) {
            rules.remove(pos);
            true
        } else {
            false
        }
    }

    /// Route a message
    pub fn route(&self, message: &Message) -> RouteResult {
        let rules = self.rules.read();

        // Find first matching rule
        for rule in rules.iter() {
            if rule.matches(message) {
                return match rule.strategy {
                    RoutingStrategy::Broadcast => RouteResult::broadcast(),
                    RoutingStrategy::Room => {
                        if let Some(target) = &rule.target {
                            RouteResult::room(target.clone())
                        } else {
                            RouteResult::broadcast()
                        }
                    }
                    RoutingStrategy::Topic => {
                        if let Some(target) = &rule.target {
                            RouteResult::topic(target.clone())
                        } else {
                            RouteResult::broadcast()
                        }
                    }
                    _ => RouteResult::broadcast(),
                };
            }
        }

        // Use default strategy if no rule matches
        match self.default_strategy {
            RoutingStrategy::Broadcast => RouteResult::broadcast(),
            _ => RouteResult::broadcast(),
        }
    }

    /// Get all rules
    pub fn rules(&self) -> Vec<RoutingRule> {
        self.rules.read().clone()
    }

    /// Clear all rules
    pub fn clear_rules(&self) {
        self.rules.write().clear();
    }

    /// Get rule count
    pub fn rule_count(&self) -> usize {
        self.rules.read().len()
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Routing table for connection-to-connection routing
pub struct RoutingTable {
    /// Mapping of source connections to target connections
    routes: Arc<RwLock<HashMap<ConnectionId, Vec<ConnectionId>>>>,
}

impl RoutingTable {
    /// Create a new routing table
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a route
    pub fn add_route(&self, source: ConnectionId, target: ConnectionId) {
        let mut routes = self.routes.write();
        routes.entry(source).or_default().push(target);
    }

    /// Remove a route
    pub fn remove_route(&self, source: &ConnectionId, target: &ConnectionId) {
        let mut routes = self.routes.write();
        if let Some(targets) = routes.get_mut(source) {
            targets.retain(|t| t != target);
        }
    }

    /// Get targets for a source
    pub fn get_targets(&self, source: &ConnectionId) -> Vec<ConnectionId> {
        let routes = self.routes.read();
        routes.get(source).cloned().unwrap_or_default()
    }

    /// Remove all routes for a connection
    pub fn remove_connection(&self, connection: &ConnectionId) {
        let mut routes = self.routes.write();
        routes.remove(connection);

        // Also remove from targets
        for targets in routes.values_mut() {
            targets.retain(|t| t != connection);
        }
    }

    /// Clear all routes
    pub fn clear(&self) {
        self.routes.write().clear();
    }

    /// Get route count
    pub fn route_count(&self) -> usize {
        self.routes.read().len()
    }
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_rule() {
        let rule = RoutingRule::new("test".to_string(), RoutingStrategy::Broadcast)
            .with_message_type(MessageType::Ping)
            .with_priority(10);

        assert_eq!(rule.name, "test");
        assert_eq!(rule.message_type, Some(MessageType::Ping));
        assert_eq!(rule.priority, 10);
    }

    #[test]
    fn test_routing_rule_matches() {
        let rule = RoutingRule::new("test".to_string(), RoutingStrategy::Broadcast)
            .with_message_type(MessageType::Ping);

        let ping = Message::ping();
        let pong = Message::pong();

        assert!(rule.matches(&ping));
        assert!(!rule.matches(&pong));
    }

    #[test]
    fn test_message_router() {
        let router = MessageRouter::new();
        assert_eq!(router.rule_count(), 0);

        let rule = RoutingRule::new("test".to_string(), RoutingStrategy::Broadcast);
        router.add_rule(rule);

        assert_eq!(router.rule_count(), 1);
    }

    #[test]
    fn test_router_route() {
        let router = MessageRouter::new();

        let rule = RoutingRule::new("ping_room".to_string(), RoutingStrategy::Room)
            .with_message_type(MessageType::Ping)
            .with_target("lobby".to_string());

        router.add_rule(rule);

        let ping = Message::ping();
        let result = router.route(&ping);

        assert_eq!(result.strategy, RoutingStrategy::Room);
        assert_eq!(result.target_id, Some("lobby".to_string()));
    }

    #[test]
    fn test_router_priority() {
        let router = MessageRouter::new();

        let rule1 =
            RoutingRule::new("low".to_string(), RoutingStrategy::Broadcast).with_priority(1);

        let rule2 = RoutingRule::new("high".to_string(), RoutingStrategy::Room)
            .with_priority(10)
            .with_target("test".to_string());

        router.add_rule(rule1);
        router.add_rule(rule2);

        let msg = Message::ping();
        let result = router.route(&msg);

        // High priority rule should match first
        assert_eq!(result.strategy, RoutingStrategy::Room);
    }

    #[test]
    fn test_routing_table() {
        let table = RoutingTable::new();
        let source = uuid::Uuid::new_v4();
        let target1 = uuid::Uuid::new_v4();
        let target2 = uuid::Uuid::new_v4();

        table.add_route(source, target1);
        table.add_route(source, target2);

        let targets = table.get_targets(&source);
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn test_routing_table_remove() {
        let table = RoutingTable::new();
        let source = uuid::Uuid::new_v4();
        let target = uuid::Uuid::new_v4();

        table.add_route(source, target);
        table.remove_route(&source, &target);

        let targets = table.get_targets(&source);
        assert_eq!(targets.len(), 0);
    }
}
