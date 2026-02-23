//! Comprehensive alert management module for OxiGDAL observability.
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
//! use oxigdal_observability::alerts::{
//!     AlertRuleDefinition, ConditionExpression, AlertEngine,
//!     NotificationChannel, SilenceRule,
//! };
//! use std::time::Duration;
//!
//! # async fn example() -> oxigdal_observability::error::Result<()> {
//! # use oxigdal_observability::alerts::{ThresholdOperator, AlertLevel};
//! # use std::sync::Arc;
//! # use std::collections::HashMap;
//! # struct MockProvider;
//! # impl oxigdal_observability::alerts::MetricProvider for MockProvider {
//! #     fn get_metric(&self, _: &str) -> Option<f64> { Some(0.0) }
//! #     fn get_metric_range(&self, _: &str, _: u64) -> Vec<oxigdal_observability::alerts::MetricDataPoint> { vec![] }
//! # }
//! let mut engine = AlertEngine::new(Arc::new(MockProvider));
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

pub mod rules;
pub mod instance;
pub mod channels;
pub mod silence;
pub mod history;
pub mod grouping;
pub mod evaluator;
pub mod manager;

#[cfg(test)]
mod tests;

// Re-export main types
pub use rules::{
    AggregationFunction, AlertRuleDefinition, ConditionExpression, ThresholdOperator,
};
pub use instance::AlertInstance;
pub use channels::{NotificationChannel, NotificationSender};
pub use silence::{SilenceMatcher, SilenceManager, SilenceRule};
pub use history::{AlertHistory, AlertHistoryEvent, AlertHistoryEventType};
pub use grouping::{AlertGroup, AlertGrouper};
pub use evaluator::{ConditionEvaluator, MetricDataPoint, MetricProvider};
pub use manager::AlertEngine;

// ============================================================================
// Alert Level and State Types
// ============================================================================

/// Alert severity level with priority ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AlertLevel {
    /// Informational alert - lowest priority.
    Info = 0,
    /// Warning alert - potential issue.
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

    /// Parse from string representation.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "info" | "informational" => Some(Self::Info),
            "warning" | "warn" => Some(Self::Warning),
            "error" | "err" => Some(Self::Error),
            "critical" | "crit" => Some(Self::Critical),
            "page" | "pager" => Some(Self::Page),
            _ => None,
        }
    }
}

impl Default for AlertLevel {
    fn default() -> Self {
        Self::Warning
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
