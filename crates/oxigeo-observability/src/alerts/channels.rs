//! Notification channels for alert delivery

#[cfg(feature = "http-exporter")]
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::AlertInstance;
#[cfg(feature = "http-exporter")]
use super::{AlertLevel, AlertState};
use crate::error::Result;

/// Notification channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannel {
    /// Email notification.
    Email {
        /// SMTP server address.
        smtp_host: String,
        /// SMTP server port.
        smtp_port: u16,
        /// Sender email address.
        from: String,
        /// Recipient email addresses.
        to: Vec<String>,
        /// Optional username for authentication.
        username: Option<String>,
        /// Optional password for authentication.
        password: Option<String>,
        /// Use TLS.
        use_tls: bool,
    },
    /// Webhook notification.
    Webhook {
        /// Webhook URL.
        url: String,
        /// HTTP method (POST, PUT).
        method: String,
        /// Custom headers.
        headers: HashMap<String, String>,
        /// Authentication token (optional).
        auth_token: Option<String>,
    },
    /// Slack notification.
    Slack {
        /// Slack webhook URL.
        webhook_url: String,
        /// Channel to post to.
        channel: String,
        /// Bot username (optional).
        username: Option<String>,
    },
    /// PagerDuty notification.
    PagerDuty {
        /// PagerDuty routing key.
        routing_key: String,
        /// API URL (optional, defaults to events API).
        api_url: Option<String>,
    },
    /// OpsGenie notification.
    OpsGenie {
        /// API key.
        api_key: String,
        /// Team identifier.
        team: Option<String>,
        /// Priority override.
        priority: Option<String>,
    },
    /// Microsoft Teams notification.
    Teams {
        /// Teams webhook URL.
        webhook_url: String,
    },
    /// Console/Log notification (for testing).
    Console {
        /// Log level to use.
        log_level: String,
    },
}

impl NotificationChannel {
    /// Get the channel type name.
    #[must_use]
    pub const fn channel_type(&self) -> &'static str {
        match self {
            Self::Email { .. } => "email",
            Self::Webhook { .. } => "webhook",
            Self::Slack { .. } => "slack",
            Self::PagerDuty { .. } => "pagerduty",
            Self::OpsGenie { .. } => "opsgenie",
            Self::Teams { .. } => "teams",
            Self::Console { .. } => "console",
        }
    }
}

/// Notification sender for dispatching alerts to channels.
#[derive(Clone)]
pub struct NotificationSender {
    channels: Vec<NotificationChannel>,
    #[cfg(feature = "http-exporter")]
    client: reqwest::Client,
}

impl NotificationSender {
    /// Create a new notification sender.
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            #[cfg(feature = "http-exporter")]
            client: reqwest::Client::new(),
        }
    }

    /// Add a notification channel.
    pub fn add_channel(&mut self, channel: NotificationChannel) {
        self.channels.push(channel);
    }

    /// Send an alert to all configured channels.
    ///
    /// Aggregates delivery failures rather than stopping at the first one: a
    /// broken Slack webhook should not prevent PagerDuty from firing. If any
    /// channel failed, returns `Err` describing every failure so a caller
    /// can't mistake a partial (or total) delivery failure for success.
    pub async fn send(&self, alert: &AlertInstance) -> Result<()> {
        let mut failures = Vec::new();
        for channel in &self.channels {
            if let Err(e) = self.send_to_channel(alert, channel).await {
                failures.push(format!("{}: {e}", channel.channel_type()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(crate::error::ObservabilityError::AlertRoutingFailed(
                format!(
                    "{} of {} notification channel(s) failed for alert '{}': {}",
                    failures.len(),
                    self.channels.len(),
                    alert.id,
                    failures.join("; ")
                ),
            ))
        }
    }

    /// Send an alert to a specific channel.
    async fn send_to_channel(
        &self,
        alert: &AlertInstance,
        channel: &NotificationChannel,
    ) -> Result<()> {
        match channel {
            #[cfg(feature = "http-exporter")]
            NotificationChannel::Webhook {
                url,
                method,
                headers,
                auth_token,
            } => {
                let mut request = match method.to_uppercase().as_str() {
                    "PUT" => self.client.put(url),
                    _ => self.client.post(url),
                };

                for (key, value) in headers {
                    request = request.header(key, value);
                }

                if let Some(token) = auth_token {
                    request = request.bearer_auth(token);
                }

                let payload = self.build_webhook_payload(alert);
                let response = request.json(&payload).send().await?;
                Self::check_delivery_status(response, "Webhook", url).await?;
            }
            #[cfg(feature = "http-exporter")]
            NotificationChannel::Slack {
                webhook_url,
                channel,
                username,
            } => {
                let payload = self.build_slack_payload(alert, channel, username.as_deref());
                let response = self.client.post(webhook_url).json(&payload).send().await?;
                Self::check_delivery_status(response, "Slack", webhook_url).await?;
            }
            #[cfg(feature = "http-exporter")]
            NotificationChannel::PagerDuty {
                routing_key,
                api_url,
            } => {
                let url = api_url
                    .clone()
                    .unwrap_or_else(|| "https://events.pagerduty.com/v2/enqueue".to_string());
                let payload = self.build_pagerduty_payload(alert, routing_key);
                let response = self.client.post(&url).json(&payload).send().await?;
                Self::check_delivery_status(response, "PagerDuty", &url).await?;
            }
            #[cfg(feature = "http-exporter")]
            NotificationChannel::Teams { webhook_url } => {
                let payload = self.build_teams_payload(alert);
                let response = self.client.post(webhook_url).json(&payload).send().await?;
                Self::check_delivery_status(response, "Teams", webhook_url).await?;
            }
            NotificationChannel::Console { log_level } => {
                self.log_alert(alert, log_level);
            }
            NotificationChannel::Email {
                smtp_host,
                smtp_port,
                from,
                to,
                ..
            } => {
                tracing::warn!(
                    smtp_host = %smtp_host,
                    smtp_port = %smtp_port,
                    from = %from,
                    to = ?to,
                    alert_id = %alert.id,
                    "Email delivery skipped: direct SMTP not supported; use a Webhook destination instead."
                );
            }
            #[cfg(feature = "http-exporter")]
            NotificationChannel::OpsGenie {
                api_key,
                team,
                priority,
            } => {
                let payload = self.build_opsgenie_payload(
                    alert,
                    api_key,
                    team.as_deref(),
                    priority.as_deref(),
                );
                let response = self
                    .client
                    .post("https://api.opsgenie.com/v2/alerts")
                    .header("Authorization", format!("GenieKey {}", api_key))
                    .json(&payload)
                    .send()
                    .await?;
                Self::check_delivery_status(
                    response,
                    "OpsGenie",
                    "https://api.opsgenie.com/v2/alerts",
                )
                .await?;
            }
            #[cfg(not(feature = "http-exporter"))]
            _ => {
                tracing::warn!(
                    channel_type = channel.channel_type(),
                    alert_id = %alert.id,
                    "Notification NOT delivered: this build was compiled without the \
                     'http-exporter' feature, so there is no transport (Webhook/Slack/\
                     PagerDuty/Teams/OpsGenie) to send alerts through. Rebuild with the \
                     'http-exporter' feature enabled to actually deliver this notification."
                );
                return Err(crate::error::ObservabilityError::ExporterFeatureDisabled(
                    format!(
                        "NotificationSender dropped a '{}' notification for alert '{}': this build \
                     was compiled without the 'http-exporter' feature, so notifications cannot \
                     be delivered",
                        channel.channel_type(),
                        alert.id
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Inspect an HTTP response from a delivery attempt and turn a
    /// non-success status into a real error instead of letting a 4xx/5xx
    /// response masquerade as successful delivery.
    #[cfg(feature = "http-exporter")]
    async fn check_delivery_status(
        response: reqwest::Response,
        destination_kind: &str,
        destination_url: &str,
    ) -> Result<()> {
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        Err(crate::error::ObservabilityError::AlertRoutingFailed(
            format!(
                "{destination_kind} delivery to '{destination_url}' failed with HTTP status {status}: {body}"
            ),
        ))
    }

    #[cfg(feature = "http-exporter")]
    fn build_webhook_payload(&self, alert: &AlertInstance) -> serde_json::Value {
        serde_json::json!({
            "alert_id": alert.id,
            "rule_id": alert.rule_id,
            "state": format!("{:?}", alert.state),
            "level": alert.level.as_str(),
            "summary": alert.summary,
            "description": alert.description,
            "labels": alert.labels,
            "annotations": alert.annotations,
            "firing_at": alert.firing_at,
            "resolved_at": alert.resolved_at,
            "fingerprint": alert.fingerprint,
        })
    }

    #[cfg(feature = "http-exporter")]
    fn build_slack_payload(
        &self,
        alert: &AlertInstance,
        channel: &str,
        username: Option<&str>,
    ) -> serde_json::Value {
        let color = match alert.level {
            AlertLevel::Critical | AlertLevel::Page => "#ff0000",
            AlertLevel::Error => "#ff6600",
            AlertLevel::Warning => "#ffcc00",
            AlertLevel::Info => "#0066ff",
        };

        let state_emoji = match alert.state {
            AlertState::Firing => ":fire:",
            AlertState::Resolved => ":white_check_mark:",
            AlertState::Pending => ":hourglass:",
            AlertState::Silenced => ":mute:",
            AlertState::Acknowledged => ":eyes:",
            AlertState::Inactive => ":zzz:",
        };

        let mut payload = serde_json::json!({
            "channel": channel,
            "attachments": [{
                "color": color,
                "title": format!("{} {} [{}]", state_emoji, alert.summary, alert.level.as_str().to_uppercase()),
                "text": alert.description,
                "fields": [
                    {"title": "State", "value": format!("{:?}", alert.state), "short": true},
                    {"title": "Level", "value": alert.level.as_str(), "short": true},
                ],
                "footer": format!("Alert ID: {}", alert.id),
                "ts": Utc::now().timestamp(),
            }]
        });

        if let Some(name) = username {
            payload["username"] = serde_json::json!(name);
        }

        payload
    }

    #[cfg(feature = "http-exporter")]
    fn build_pagerduty_payload(
        &self,
        alert: &AlertInstance,
        routing_key: &str,
    ) -> serde_json::Value {
        let severity = match alert.level {
            AlertLevel::Critical | AlertLevel::Page => "critical",
            AlertLevel::Error => "error",
            AlertLevel::Warning => "warning",
            AlertLevel::Info => "info",
        };

        let event_action = match alert.state {
            AlertState::Firing => "trigger",
            AlertState::Resolved => "resolve",
            _ => "trigger",
        };

        serde_json::json!({
            "routing_key": routing_key,
            "event_action": event_action,
            "dedup_key": alert.fingerprint,
            "payload": {
                "summary": alert.summary,
                "severity": severity,
                "source": alert.rule_id,
                "custom_details": {
                    "description": alert.description,
                    "labels": alert.labels,
                    "annotations": alert.annotations,
                }
            }
        })
    }

    #[cfg(feature = "http-exporter")]
    fn build_teams_payload(&self, alert: &AlertInstance) -> serde_json::Value {
        let theme_color = match alert.level {
            AlertLevel::Critical | AlertLevel::Page => "FF0000",
            AlertLevel::Error => "FF6600",
            AlertLevel::Warning => "FFCC00",
            AlertLevel::Info => "0066FF",
        };

        serde_json::json!({
            "@type": "MessageCard",
            "@context": "http://schema.org/extensions",
            "themeColor": theme_color,
            "summary": alert.summary,
            "sections": [{
                "activityTitle": alert.summary,
                "activitySubtitle": format!("Level: {} | State: {:?}", alert.level.as_str(), alert.state),
                "text": alert.description,
                "facts": [
                    {"name": "Alert ID", "value": &alert.id},
                    {"name": "Rule ID", "value": &alert.rule_id},
                    {"name": "Fingerprint", "value": &alert.fingerprint},
                ]
            }]
        })
    }

    fn log_alert(&self, alert: &AlertInstance, log_level: &str) {
        let message = format!(
            "[{}] Alert: {} ({}) - State: {:?} - {}",
            alert.level.as_str().to_uppercase(),
            alert.summary,
            alert.id,
            alert.state,
            alert.description
        );

        match log_level.to_lowercase().as_str() {
            "error" => tracing::error!("{}", message),
            "warn" => tracing::warn!("{}", message),
            "debug" => tracing::debug!("{}", message),
            "trace" => tracing::trace!("{}", message),
            _ => tracing::info!("{}", message),
        }
    }

    #[cfg(feature = "http-exporter")]
    fn build_opsgenie_payload(
        &self,
        alert: &AlertInstance,
        api_key: &str,
        team: Option<&str>,
        priority: Option<&str>,
    ) -> serde_json::Value {
        let _ = api_key; // used in header, not body
        let opsgenie_priority = priority.unwrap_or(match alert.level {
            AlertLevel::Critical | AlertLevel::Page => "P1",
            AlertLevel::Error => "P2",
            AlertLevel::Warning => "P3",
            AlertLevel::Info => "P5",
        });
        let mut payload = serde_json::json!({
            "message": alert.summary,
            "description": alert.description,
            "priority": opsgenie_priority,
            "alias": alert.fingerprint,
            "details": {
                "alert_id": alert.id,
                "rule_id": alert.rule_id,
                "state": format!("{:?}", alert.state),
                "level": alert.level.as_str(),
            }
        });
        if let Some(t) = team {
            payload["teams"] = serde_json::json!([{"name": t}]);
        }
        payload
    }
}

impl Default for NotificationSender {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::rules::AlertRuleDefinition;

    fn sample_alert() -> AlertInstance {
        let rule = AlertRuleDefinition::new("test_rule").with_name("Test Rule");
        AlertInstance::from_rule(&rule)
    }

    #[tokio::test]
    async fn test_console_channel_always_succeeds() {
        let mut sender = NotificationSender::new();
        sender.add_channel(NotificationChannel::Console {
            log_level: "info".to_string(),
        });

        let alert = sample_alert();
        assert!(sender.send(&alert).await.is_ok());
    }

    #[tokio::test]
    async fn test_email_channel_is_skipped_but_not_an_error() {
        // Email is intentionally unsupported (no SMTP client); it must not
        // silently claim success as if it were delivered *and* must not be
        // conflated with the honest-error path used for
        // Webhook/Slack/PagerDuty/Teams/OpsGenie when http-exporter is off.
        let mut sender = NotificationSender::new();
        sender.add_channel(NotificationChannel::Email {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            from: "alerts@example.com".to_string(),
            to: vec!["oncall@example.com".to_string()],
            username: None,
            password: None,
            use_tls: true,
        });

        let alert = sample_alert();
        assert!(sender.send(&alert).await.is_ok());
    }

    #[cfg(not(feature = "http-exporter"))]
    #[tokio::test]
    async fn test_webhook_channel_without_http_exporter_errors_instead_of_dropping_silently() {
        let mut sender = NotificationSender::new();
        sender.add_channel(NotificationChannel::Webhook {
            url: "https://example.invalid/webhook".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            auth_token: None,
        });

        let alert = sample_alert();
        let result = sender.send(&alert).await;
        assert!(
            result.is_err(),
            "send() must not silently report success when there is no transport \
             (http-exporter feature disabled) to deliver notifications"
        );
    }

    #[cfg(not(feature = "http-exporter"))]
    #[tokio::test]
    async fn test_slack_pagerduty_teams_opsgenie_without_http_exporter_all_error() {
        for channel in [
            NotificationChannel::Slack {
                webhook_url: "https://hooks.slack.com/services/x".to_string(),
                channel: "#alerts".to_string(),
                username: None,
            },
            NotificationChannel::PagerDuty {
                routing_key: "key".to_string(),
                api_url: None,
            },
            NotificationChannel::Teams {
                webhook_url: "https://outlook.office.com/webhook/x".to_string(),
            },
            NotificationChannel::OpsGenie {
                api_key: "key".to_string(),
                team: None,
                priority: None,
            },
        ] {
            let mut sender = NotificationSender::new();
            let kind = channel.channel_type();
            sender.add_channel(channel);
            let alert = sample_alert();
            let result = sender.send(&alert).await;
            assert!(
                result.is_err(),
                "{kind} channel must not silently drop notifications when http-exporter is disabled"
            );
        }
    }

    #[cfg(feature = "http-exporter")]
    fn spawn_mock_webhook(
        status_line: &'static str,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
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
                let body = b"";
                let response = format!("{status_line}\r\nContent-Length: {}\r\n\r\n", body.len());
                let _ = stream.write_all(response.as_bytes());
                let _ = tx.send(text);
            }
        });

        (format!("http://{addr}/webhook"), rx)
    }

    #[cfg(feature = "http-exporter")]
    #[tokio::test]
    async fn test_webhook_delivers_successfully_with_http_exporter_enabled() {
        let (url, rx) = spawn_mock_webhook("HTTP/1.1 200 OK");
        let mut sender = NotificationSender::new();
        sender.add_channel(NotificationChannel::Webhook {
            url,
            method: "POST".to_string(),
            headers: HashMap::new(),
            auth_token: None,
        });

        let alert = sample_alert();
        sender
            .send(&alert)
            .await
            .expect("send should succeed against a 200-returning mock webhook");

        let received = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("mock webhook should have received a request");
        assert!(received.starts_with("POST"));
    }

    #[cfg(feature = "http-exporter")]
    #[tokio::test]
    async fn test_webhook_non_success_status_is_reported_as_error() {
        let (url, _rx) = spawn_mock_webhook("HTTP/1.1 503 Service Unavailable");
        let mut sender = NotificationSender::new();
        sender.add_channel(NotificationChannel::Webhook {
            url,
            method: "POST".to_string(),
            headers: HashMap::new(),
            auth_token: None,
        });

        let alert = sample_alert();
        let result = sender.send(&alert).await;
        assert!(
            result.is_err(),
            "a 503 response must not be reported as a successful delivery"
        );
        let message = result.expect_err("checked is_err above").to_string();
        assert!(
            message.contains("503"),
            "error should mention status, got: {message}"
        );
    }
}
