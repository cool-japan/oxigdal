//! Notification channels for alert delivery

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;
use super::{AlertInstance, AlertLevel, AlertState};

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
pub struct NotificationSender {
    channels: Vec<NotificationChannel>,
    client: reqwest::Client,
}

impl NotificationSender {
    /// Create a new notification sender.
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            client: reqwest::Client::new(),
        }
    }

    /// Add a notification channel.
    pub fn add_channel(&mut self, channel: NotificationChannel) {
        self.channels.push(channel);
    }

    /// Send an alert to all configured channels.
    pub async fn send(&self, alert: &AlertInstance) -> Result<()> {
        for channel in &self.channels {
            self.send_to_channel(alert, channel).await?;
        }
        Ok(())
    }

    /// Send an alert to a specific channel.
    async fn send_to_channel(
        &self,
        alert: &AlertInstance,
        channel: &NotificationChannel,
    ) -> Result<()> {
        match channel {
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
                request.json(&payload).send().await?;
            }
            NotificationChannel::Slack {
                webhook_url,
                channel,
                username,
            } => {
                let payload = self.build_slack_payload(alert, channel, username.as_deref());
                self.client.post(webhook_url).json(&payload).send().await?;
            }
            NotificationChannel::PagerDuty {
                routing_key,
                api_url,
            } => {
                let url = api_url
                    .clone()
                    .unwrap_or_else(|| "https://events.pagerduty.com/v2/enqueue".to_string());
                let payload = self.build_pagerduty_payload(alert, routing_key);
                self.client.post(&url).json(&payload).send().await?;
            }
            NotificationChannel::Teams { webhook_url } => {
                let payload = self.build_teams_payload(alert);
                self.client.post(webhook_url).json(&payload).send().await?;
            }
            NotificationChannel::Console { log_level } => {
                self.log_alert(alert, log_level);
            }
            NotificationChannel::Email { .. } | NotificationChannel::OpsGenie { .. } => {
                // Email and OpsGenie would require additional dependencies
                // Placeholder for now
            }
        }
        Ok(())
    }

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
}

impl Default for NotificationSender {
    fn default() -> Self {
        Self::new()
    }
}
