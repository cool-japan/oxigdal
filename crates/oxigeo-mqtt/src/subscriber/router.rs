//! Topic routing for message handlers

use crate::client::MessageHandler;
use crate::error::{MqttError, Result};
use crate::subscriber::Subscriber;
use crate::types::{Message, QoS, TopicFilter};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::{debug, warn};

/// Router configuration
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Default QoS for subscriptions
    pub default_qos: QoS,
    /// Enable wildcard routing
    pub enable_wildcards: bool,
    /// Maximum routes
    pub max_routes: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            default_qos: QoS::AtMostOnce,
            enable_wildcards: true,
            max_routes: 1000,
        }
    }
}

impl RouterConfig {
    /// Create new router configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set default QoS
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.default_qos = qos;
        self
    }

    /// Enable or disable wildcard routing
    pub fn with_wildcards(mut self, enable: bool) -> Self {
        self.enable_wildcards = enable;
        self
    }

    /// Set maximum routes
    pub fn with_max_routes(mut self, max: usize) -> Self {
        self.max_routes = max;
        self
    }
}

/// Topic router for managing multiple subscriptions
pub struct TopicRouter {
    /// Subscriber
    subscriber: Arc<Subscriber>,
    /// Configuration
    config: RouterConfig,
    /// Route handlers
    routes: Arc<DashMap<String, Arc<dyn MessageHandler>>>,
    /// Master handler
    master_handler: Arc<RouterHandler>,
}

impl TopicRouter {
    /// Create a new topic router
    pub fn new(subscriber: Arc<Subscriber>, config: RouterConfig) -> Self {
        let routes = Arc::new(DashMap::new());
        let master_handler = Arc::new(RouterHandler {
            routes: Arc::clone(&routes),
            config: config.clone(),
        });

        Self {
            subscriber,
            config,
            routes,
            master_handler,
        }
    }

    /// Add a route
    pub async fn add_route<H>(&self, pattern: impl Into<String>, handler: H) -> Result<()>
    where
        H: MessageHandler + 'static,
    {
        let pattern = pattern.into();

        // Check route limit
        if self.routes.len() >= self.config.max_routes {
            return Err(MqttError::Internal(format!(
                "Maximum routes ({}) reached",
                self.config.max_routes
            )));
        }

        // Subscribe if first route for this pattern
        if !self.routes.contains_key(&pattern) {
            let filter = TopicFilter::new(pattern.clone(), self.config.default_qos);
            self.subscriber
                .subscribe(
                    filter,
                    Arc::clone(&self.master_handler) as Arc<dyn MessageHandler>,
                )
                .await?;
        }

        self.routes.insert(pattern.clone(), Arc::new(handler));
        debug!("Added route: {}", pattern);

        Ok(())
    }

    /// Remove a route
    pub async fn remove_route(&self, pattern: &str) -> Result<()> {
        if self.routes.remove(pattern).is_some() {
            // Unsubscribe if no more routes for this pattern
            self.subscriber.unsubscribe(pattern).await?;
            debug!("Removed route: {}", pattern);
        }
        Ok(())
    }

    /// Get all routes
    pub fn routes(&self) -> Vec<String> {
        self.routes.iter().map(|e| e.key().clone()).collect()
    }

    /// Get route count
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Clear all routes
    pub async fn clear_routes(&self) -> Result<()> {
        let patterns: Vec<String> = self.routes.iter().map(|e| e.key().clone()).collect();

        for pattern in patterns {
            self.remove_route(&pattern).await?;
        }

        Ok(())
    }

    /// Get configuration
    pub fn config(&self) -> &RouterConfig {
        &self.config
    }
}

/// Internal router handler
struct RouterHandler {
    /// Routes
    routes: Arc<DashMap<String, Arc<dyn MessageHandler>>>,
    /// Configuration
    config: RouterConfig,
}

#[async_trait]
impl MessageHandler for RouterHandler {
    async fn handle_message(&self, message: Message) -> Result<()> {
        let topic = &message.topic;
        let mut handled = false;

        // Try exact match first
        if let Some(handler) = self.routes.get(topic) {
            handler.handle_message(message.clone()).await?;
            handled = true;
        }

        // Try wildcard matches if enabled - collect first to avoid lifetime issues
        if self.config.enable_wildcards {
            let wildcard_entries: Vec<(String, Arc<dyn MessageHandler>)> = self
                .routes
                .iter()
                .filter(|entry| entry.key() != topic)
                .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
                .collect();

            for (pattern, handler) in wildcard_entries {
                let filter = TopicFilter::new(pattern, QoS::AtMostOnce);
                if filter.matches(topic) {
                    handler.handle_message(message.clone()).await?;
                    handled = true;
                }
            }
        }

        if !handled {
            warn!("No route found for topic: {}", topic);
        }

        Ok(())
    }
}

/// Route builder for fluent API
#[allow(dead_code)]
pub struct RouteBuilder<'a> {
    /// Router
    router: &'a TopicRouter,
    /// Pattern
    pattern: String,
}

// Public API for fluent route building
#[allow(dead_code)]
impl<'a> RouteBuilder<'a> {
    /// Create a new route builder
    pub fn new(router: &'a TopicRouter, pattern: impl Into<String>) -> Self {
        Self {
            router,
            pattern: pattern.into(),
        }
    }

    /// Set the handler
    pub async fn handler<H>(self, handler: H) -> Result<()>
    where
        H: MessageHandler + 'static,
    {
        self.router.add_route(self.pattern, handler).await
    }

    /// Set a callback handler
    pub async fn callback<F>(self, callback: F) -> Result<()>
    where
        F: Fn(Message) -> Result<()> + Send + Sync + 'static,
    {
        use crate::subscriber::SimpleHandler;
        let handler = SimpleHandler::new(callback);
        self.router.add_route(self.pattern, handler).await
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, MqttClient};
    use crate::subscriber::SubscriberConfig;
    use crate::types::ConnectionOptions;

    #[tokio::test]
    async fn test_router_creation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-router");
        let client_config = ClientConfig::new(conn_opts);
        let client = MqttClient::new(client_config).expect("Failed to create client");
        let client = Arc::new(client);

        let sub_config = SubscriberConfig::new();
        let subscriber = Arc::new(Subscriber::new(client, sub_config));

        let router_config = RouterConfig::new();
        let router = TopicRouter::new(subscriber, router_config);

        assert_eq!(router.route_count(), 0);
        assert!(router.routes().is_empty());
    }

    #[test]
    fn test_router_config() {
        let config = RouterConfig::new()
            .with_qos(QoS::ExactlyOnce)
            .with_wildcards(false)
            .with_max_routes(500);

        assert_eq!(config.default_qos, QoS::ExactlyOnce);
        assert!(!config.enable_wildcards);
        assert_eq!(config.max_routes, 500);
    }

    #[tokio::test]
    async fn test_route_pattern_matching() {
        let filter1 = TopicFilter::new("sensor/+/temperature", QoS::AtMostOnce);
        assert!(filter1.matches("sensor/1/temperature"));
        assert!(filter1.matches("sensor/2/temperature"));
        assert!(!filter1.matches("sensor/1/humidity"));

        let filter2 = TopicFilter::new("sensor/#", QoS::AtMostOnce);
        assert!(filter2.matches("sensor/1/temperature"));
        assert!(filter2.matches("sensor/1/2/temperature"));
    }
}
