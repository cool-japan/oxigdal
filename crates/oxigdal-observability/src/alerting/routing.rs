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
    #[cfg(feature = "http-exporter")]
    client: reqwest::Client,
}

impl AlertRouter {
    /// Create a new alert router.
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(Vec::new())),
            #[cfg(feature = "http-exporter")]
            client: reqwest::Client::new(),
        }
    }

    /// Add a routing rule.
    pub fn add_route(&mut self, route: Route) {
        self.routes.write().push(route);
    }

    /// Route an alert to configured destinations.
    ///
    /// Actually delivering alerts to Webhook/Slack/PagerDuty destinations requires this crate
    /// to be built with the `http-exporter` Cargo feature enabled (it pulls in `reqwest`).
    /// When `http-exporter` is disabled, every matched destination is undeliverable: this
    /// method logs a `tracing::warn!` for each dropped destination and returns
    /// [`crate::error::ObservabilityError::ExporterFeatureDisabled`] rather than silently
    /// reporting success, so callers can detect that no alert was actually sent.
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
        #[cfg(feature = "http-exporter")]
        {
            for destination in &destinations {
                self.send_to_destination(alert, destination).await?;
            }
            Ok(())
        }

        #[cfg(not(feature = "http-exporter"))]
        {
            if destinations.is_empty() {
                return Ok(());
            }

            for destination in &destinations {
                tracing::warn!(
                    alert_name = %alert.name,
                    destination = ?destination,
                    "Alert destination NOT delivered: this build was compiled without the \
                     'http-exporter' feature, so there is no transport (Webhook/Slack/\
                     PagerDuty/Email) to send alerts through. Rebuild with the 'http-exporter' \
                     feature enabled to actually deliver alerts."
                );
            }

            Err(crate::error::ObservabilityError::ExporterFeatureDisabled(
                format!(
                    "AlertRouter::route dropped {} destination(s) for alert '{}': this build was \
                 compiled without the 'http-exporter' feature, so alerts cannot be delivered",
                    destinations.len(),
                    alert.name
                ),
            ))
        }
    }

    #[cfg(feature = "http-exporter")]
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
            Destination::Email { addresses } => {
                tracing::warn!(
                    recipients = ?addresses,
                    alert_name = %alert.name,
                    "Email delivery skipped: direct SMTP is not supported in http-exporter mode; \
                     configure a Webhook destination pointing to an SMTP relay instead."
                );
            }
            Destination::PagerDuty { service_key } => {
                let payload = serde_json::json!({
                    "routing_key": service_key,
                    "event_action": "trigger",
                    "payload": {
                        "summary": format!("{}: {}", alert.name, alert.message),
                        "severity": match alert.severity {
                            crate::alerting::AlertSeverity::Critical | crate::alerting::AlertSeverity::High => "critical",
                            crate::alerting::AlertSeverity::Medium => "warning",
                            crate::alerting::AlertSeverity::Low | crate::alerting::AlertSeverity::Info => "info",
                        },
                        "source": "oxigdal-observability",
                    }
                });
                let _ = self
                    .client
                    .post("https://events.pagerduty.com/v2/enqueue")
                    .json(&payload)
                    .send()
                    .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerting::AlertSeverity;

    fn sample_alert() -> Alert {
        Alert::new(
            "test_alert".to_string(),
            AlertSeverity::Critical,
            "something broke".to_string(),
        )
    }

    #[tokio::test]
    async fn test_route_with_no_matching_routes_is_ok() {
        let router = AlertRouter::new();
        let alert = sample_alert();
        assert!(router.route(&alert).await.is_ok());
    }

    #[cfg(not(feature = "http-exporter"))]
    #[tokio::test]
    async fn test_route_without_http_exporter_feature_errors_instead_of_dropping_silently() {
        let mut router = AlertRouter::new();
        router.add_route(Route {
            matcher: Box::new(|_alert| true),
            destinations: vec![Destination::Webhook {
                url: "https://example.invalid/webhook".to_string(),
            }],
        });

        let alert = sample_alert();
        let result = router.route(&alert).await;
        assert!(
            result.is_err(),
            "route() must not silently report success when there is no transport \
             (http-exporter feature disabled) to deliver alerts"
        );
    }

    #[cfg(feature = "http-exporter")]
    fn spawn_mock_webhook() -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock webhook");
        let addr = listener.local_addr().expect("mock webhook local addr");
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                let mut buf = [0u8; 8192];
                let mut received = Vec::new();
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => received.extend_from_slice(&buf[..n]),
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&received).to_string();
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                let _ = tx.send(text);
            }
        });

        (format!("http://{addr}/webhook"), rx)
    }

    #[cfg(feature = "http-exporter")]
    #[tokio::test]
    async fn test_route_delivers_to_webhook_when_http_exporter_enabled() {
        let (url, rx) = spawn_mock_webhook();
        let mut router = AlertRouter::new();
        router.add_route(Route {
            matcher: Box::new(|_alert| true),
            destinations: vec![Destination::Webhook { url }],
        });

        let alert = sample_alert();
        router
            .route(&alert)
            .await
            .expect("route should succeed and POST to the mock webhook");

        let received = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("mock webhook should have received a request");
        assert!(received.starts_with("POST"), "request was: {received}");
        assert!(
            received.contains("test_alert"),
            "request body should reference the alert name, was: {received}"
        );
    }
}
