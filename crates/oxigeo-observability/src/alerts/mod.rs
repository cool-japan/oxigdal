//! Comprehensive alert management module for OxiGeo observability.
//!
//! This module provides a full-featured alert management system including:
//! - Alert rule definitions with expression-based conditions
//! - Condition evaluation engine with metric queries
//! - Full alert state machine (pending, firing, resolved)
//! - Advanced alert grouping and deduplication strategies
//! - Multiple notification channels (email, webhook, Slack, etc.)
//! - Complete alert history and tracking
//! - Flexible silencing and muting rules
//!
//! # Example
//!
//! ```no_run
//! use oxigeo_observability::alerts::{
//!     AlertRuleDefinition, ConditionExpression, AlertEngine,
//!     NotificationChannel, SilenceRule,
//! };
//! use std::time::Duration;
//!
//! # async fn example() -> oxigeo_observability::error::Result<()> {
//! # use oxigeo_observability::alerts::{ThresholdOperator, AlertLevel};
//! # use std::sync::Arc;
//! # use std::collections::HashMap;
//! # struct MockProvider;
//! # impl oxigeo_observability::alerts::MetricProvider for MockProvider {
//! #     fn get_metric(&self, _: &str) -> Option<f64> { Some(0.0) }
//! #     fn get_metric_range(&self, _: &str, _: u64) -> Vec<oxigeo_observability::alerts::MetricDataPoint> { vec![] }
//! # }
//! let engine = AlertEngine::new(Arc::new(MockProvider));
//!
//! // Define an alert rule
//! let rule = AlertRuleDefinition::new("high_cpu_usage")
//!     .with_condition(ConditionExpression::Threshold {
//!         metric: "cpu_usage_percent".to_string(),
//!         operator: ThresholdOperator::GreaterThan,
//!         value: 90.0,
//!     })
//!     .with_severity(AlertLevel::Critical)
//!     .with_pending_duration(Duration::from_secs(300))
//!     .with_description("CPU usage exceeded 90%");
//!
//! engine.add_rule(rule)?;
//!
//! // Add notification channel
//! engine.add_notification_channel(NotificationChannel::Slack {
//!     webhook_url: "https://hooks.slack.com/...".to_string(),
//!     channel: "#alerts".to_string(),
//!     username: Some("AlertBot".to_string()),
//! })?;
//!
//! // Evaluate and process alerts
//! engine.evaluate_all().await?;
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};

pub mod channels;
pub mod evaluator;
pub mod grouping;
pub mod history;
pub mod instance;
pub mod manager;
pub mod rules;
pub mod silence;

#[cfg(test)]
mod tests;

// Re-export main types
pub use channels::{NotificationChannel, NotificationSender};
pub use evaluator::{ConditionEvaluator, MetricDataPoint, MetricProvider};
pub use grouping::{AlertGroup, AlertGrouper};
pub use history::{AlertHistory, AlertHistoryEvent, AlertHistoryEventType};
pub use instance::AlertInstance;
pub use manager::AlertEngine;
pub use rules::{AggregationFunction, AlertRuleDefinition, ConditionExpression, ThresholdOperator};
pub use silence::{SilenceManager, SilenceMatcher, SilenceRule};

// ============================================================================
// Alert Level and State Types
// ============================================================================

/// Alert severity level with priority ordering.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum AlertLevel {
    /// Informational alert - lowest priority.
    Info = 0,
    /// Warning alert - potential issue.
    #[default]
    Warning = 1,
    /// Error alert - significant issue.
    Error = 2,
    /// Critical alert - highest priority.
    Critical = 3,
    /// Page alert - requires immediate attention.
    Page = 4,
}

impl AlertLevel {
    /// Get the display name of the alert level.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
            Self::Page => "page",
        }
    }
}

impl std::str::FromStr for AlertLevel {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "info" | "informational" => Ok(Self::Info),
            "warning" | "warn" => Ok(Self::Warning),
            "error" | "err" => Ok(Self::Error),
            "critical" | "crit" => Ok(Self::Critical),
            "page" | "pager" => Ok(Self::Page),
            other => Err(format!("Invalid alert level: {other}")),
        }
    }
}

/// Alert state in the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlertState {
    /// Alert condition is not met - inactive.
    Inactive,
    /// Alert condition met but waiting for pending duration.
    Pending,
    /// Alert is actively firing.
    Firing,
    /// Alert was firing but condition is no longer met.
    Resolved,
    /// Alert has been silenced by a silence rule.
    Silenced,
    /// Alert has been manually acknowledged.
    Acknowledged,
}

impl AlertState {
    /// Check if the alert is in an active state (pending or firing).
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::Firing)
    }

    /// Check if the alert requires attention.
    #[must_use]
    pub const fn requires_attention(&self) -> bool {
        matches!(self, Self::Firing)
    }
}
