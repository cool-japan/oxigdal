//! SLO/SLA monitoring and error budget tracking.

pub mod budgets;
pub mod objectives;
pub mod reports;

use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Service Level Objective definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slo {
    /// SLO name.
    pub name: String,

    /// SLO description.
    pub description: String,

    /// Target percentage (e.g., 99.9 for 99.9% availability).
    pub target: f64,

    /// SLI (Service Level Indicator) query.
    pub sli_query: String,

    /// Time window for evaluation.
    pub time_window: TimeWindow,

    /// Error budget.
    pub error_budget: ErrorBudget,
}

/// Time window for SLO evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimeWindow {
    /// Rolling window (e.g., last 30 days).
    Rolling(Duration),

    /// Calendar-based window (e.g., monthly).
    Calendar(CalendarWindow),
}

/// Calendar-based time window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CalendarWindow {
    /// Daily calendar window (resets at midnight).
    Daily,
    /// Weekly calendar window (resets on Monday).
    Weekly,
    /// Monthly calendar window (resets on first of month).
    Monthly,
    /// Quarterly calendar window (resets at quarter start).
    Quarterly,
    /// Yearly calendar window (resets on January 1st).
    Yearly,
}

/// Error budget for SLO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBudget {
    /// Total error budget (derived from target).
    pub total: f64,

    /// Consumed error budget.
    pub consumed: f64,

    /// Remaining error budget.
    pub remaining: f64,

    /// Burn rate (rate at which budget is being consumed).
    pub burn_rate: f64,
}

impl ErrorBudget {
    /// Create a new error budget from target percentage.
    pub fn from_target(target: f64) -> Self {
        let total = 100.0 - target;
        Self {
            total,
            consumed: 0.0,
            remaining: total,
            burn_rate: 0.0,
        }
    }

    /// Update error budget with new consumption.
    pub fn update(&mut self, consumed: f64) {
        self.consumed = consumed;
        self.remaining = self.total - consumed;
    }

    /// Calculate burn rate based on time window.
    pub fn calculate_burn_rate(&mut self, window_hours: f64) {
        if window_hours > 0.0 {
            self.burn_rate = self.consumed / window_hours;
        }
    }

    /// Check if error budget is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining <= 0.0
    }

    /// Get budget exhaustion percentage.
    pub fn exhaustion_percentage(&self) -> f64 {
        if self.total > 0.0 {
            (self.consumed / self.total) * 100.0
        } else {
            0.0
        }
    }
}

/// SLO status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloStatus {
    /// SLO name.
    pub name: String,

    /// Current achievement (percentage).
    pub achievement: f64,

    /// Target percentage.
    pub target: f64,

    /// Whether SLO is met.
    pub is_met: bool,

    /// Error budget status.
    pub error_budget: ErrorBudget,

    /// Timestamp of evaluation.
    pub evaluated_at: DateTime<Utc>,
}

impl SloStatus {
    /// Create a new SLO status.
    pub fn new(name: String, achievement: f64, target: f64, error_budget: ErrorBudget) -> Self {
        Self {
            name,
            achievement,
            target,
            is_met: achievement >= target,
            error_budget,
            evaluated_at: Utc::now(),
        }
    }
}

/// SLO monitor for tracking multiple SLOs.
pub struct SloMonitor {
    slos: Vec<Slo>,
}

impl SloMonitor {
    /// Create a new SLO monitor.
    pub fn new() -> Self {
        Self { slos: Vec::new() }
    }

    /// Add an SLO to monitor.
    pub fn add_slo(&mut self, slo: Slo) {
        self.slos.push(slo);
    }

    /// Get all SLOs.
    pub fn slos(&self) -> &[Slo] {
        &self.slos
    }

    /// Evaluate all SLOs.
    pub fn evaluate_all(&self) -> Result<Vec<SloStatus>> {
        // Placeholder - would query actual metrics
        Ok(Vec::new())
    }
}

impl Default for SloMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_budget() {
        let mut budget = ErrorBudget::from_target(99.9);
        assert!((budget.total - 0.1).abs() < 1e-10);
        assert!((budget.remaining - 0.1).abs() < 1e-10);

        budget.update(0.05);
        assert_eq!(budget.consumed, 0.05);
        assert!((budget.remaining - 0.05).abs() < 1e-10);
        assert!(!budget.is_exhausted());

        budget.update(0.15);
        assert!(budget.is_exhausted());
    }

    #[test]
    fn test_slo_status() {
        let budget = ErrorBudget::from_target(99.9);
        let status = SloStatus::new("test_slo".to_string(), 99.95, 99.9, budget);

        assert!(status.is_met);
        assert_eq!(status.achievement, 99.95);
    }
}
