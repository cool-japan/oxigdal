//! Alert routing to different channels.

use super::Alert;
use crate::error::Result;
use parking_lot::RwLock;
use std::sync::Arc;

/// Alert destination.
#[derive(Debug, Clone)]
pub enum Destination {
    /// Email notification destination.
    Email {
        /// List of email addresses to send alerts to.
        addresses: Vec<String>,
    },
    /// Webhook notification destination.
    Webhook {
        /// URL to POST alert data to.
        url: String,
    },
    /// PagerDuty notification destination.
    PagerDuty {
        /// PagerDuty service integration key.
        service_key: String,
    },
    /// Slack notification destination.
    Slack {
        /// Slack incoming webhook URL.
        webhook_url: String,
    },
}

/// Routing rule.
pub struct Route {
    /// Matcher function to determine if this route applies to an alert.
    pub matcher: Box<dyn Fn(&Alert) -> bool + Send + Sync>,
    /// List of destinations to send matched alerts to.
    pub destinations: Vec<Destination>,
}

/// Alert router.
pub struct AlertRouter {
    routes: Arc<RwLock<Vec<Route>>>,
    client: reqwest::Client,
}

impl AlertRouter {
    /// Create a new alert router.
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(Vec::new())),
            client: reqwest::Client::new(),
        }
    }

    /// Add a routing rule.
    pub fn add_route(&mut self, route: Route) {
        self.routes.write().push(route);
    }

    /// Route an alert to configured destinations.
    pub async fn route(&self, alert: &Alert) -> Result<()> {
        // Collect matching destinations while holding the lock briefly
        let destinations: Vec<Destination> = {
            let routes = self.routes.read();
            routes
                .iter()
                .filter(|route| (route.matcher)(alert))
                .flat_map(|route| route.destinations.iter().cloned())
                .collect()
        };

        // Send to all collected destinations without holding the lock
        for destination in &destinations {
            self.send_to_destination(alert, destination).await?;
        }

        Ok(())
    }

    async fn send_to_destination(&self, alert: &Alert, destination: &Destination) -> Result<()> {
        match destination {
            Destination::Webhook { url } => {
                let _ = self.client.post(url).json(alert).send().await?;
            }
            Destination::Slack { webhook_url } => {
                let payload = serde_json::json!({
                    "text": format!("{}: {}", alert.name, alert.message),
                });
                let _ = self.client.post(webhook_url).json(&payload).send().await?;
            }
            Destination::Email { .. } | Destination::PagerDuty { .. } => {
                // Placeholder for email and PagerDuty integration
            }
        }

        Ok(())
    }
}

impl Default for AlertRouter {
    fn default() -> Self {
        Self::new()
    }
}
