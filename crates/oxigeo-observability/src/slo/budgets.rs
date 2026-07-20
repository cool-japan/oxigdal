//! Error budget tracking and management.

use super::ErrorBudget;
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use std::sync::Arc;

/// Error budget tracker.
pub struct BudgetTracker {
    budgets: Arc<RwLock<Vec<TrackedBudget>>>,
}

/// Tracked error budget with history.
#[derive(Debug, Clone)]
pub struct TrackedBudget {
    /// Name of the SLO being tracked.
    pub slo_name: String,
    /// The error budget for this SLO.
    pub budget: ErrorBudget,
    /// Historical snapshots of budget state over time.
    pub history: Vec<BudgetSnapshot>,
}

/// Snapshot of error budget at a point in time.
#[derive(Debug, Clone)]
pub struct BudgetSnapshot {
    /// When this snapshot was taken.
    pub timestamp: DateTime<Utc>,
    /// Amount of error budget consumed at this time.
    pub consumed: f64,
    /// Amount of error budget remaining at this time.
    pub remaining: f64,
    /// Current burn rate (budget consumption per hour).
    pub burn_rate: f64,
}

impl BudgetTracker {
    /// Create a new budget tracker.
    pub fn new() -> Self {
        Self {
            budgets: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a budget to track.
    pub fn track(&self, slo_name: String, budget: ErrorBudget) {
        let tracked = TrackedBudget {
            slo_name,
            budget,
            history: Vec::new(),
        };

        self.budgets.write().push(tracked);
    }

    /// Update a tracked budget.
    pub fn update(&self, slo_name: &str, consumed: f64) {
        let mut budgets = self.budgets.write();

        if let Some(tracked) = budgets.iter_mut().find(|b| b.slo_name == slo_name) {
            tracked.budget.update(consumed);

            // Add snapshot to history
            tracked.history.push(BudgetSnapshot {
                timestamp: Utc::now(),
                consumed: tracked.budget.consumed,
                remaining: tracked.budget.remaining,
                burn_rate: tracked.budget.burn_rate,
            });
        }
    }

    /// Get all tracked budgets.
    pub fn get_all(&self) -> Vec<TrackedBudget> {
        self.budgets.read().clone()
    }

    /// Get budget for SLO.
    pub fn get(&self, slo_name: &str) -> Option<TrackedBudget> {
        self.budgets
            .read()
            .iter()
            .find(|b| b.slo_name == slo_name)
            .cloned()
    }

    /// Calculate projected budget exhaustion time.
    pub fn projected_exhaustion(&self, slo_name: &str) -> Option<DateTime<Utc>> {
        let tracked = self.get(slo_name)?;

        if tracked.budget.burn_rate <= 0.0 || tracked.budget.remaining <= 0.0 {
            return None;
        }

        let hours_until_exhaustion = tracked.budget.remaining / tracked.budget.burn_rate;
        Some(Utc::now() + Duration::hours(hours_until_exhaustion as i64))
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_tracker() {
        let tracker = BudgetTracker::new();
        let budget = ErrorBudget::from_target(99.9);

        tracker.track("test_slo".to_string(), budget);
        tracker.update("test_slo", 0.05);

        let tracked = tracker.get("test_slo").expect("Budget not found");
        assert_eq!(tracked.budget.consumed, 0.05);
        assert_eq!(tracked.history.len(), 1);
    }
}
