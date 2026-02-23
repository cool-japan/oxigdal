//! Tests for SLO monitoring.

use oxigdal_observability::slo::{
    ErrorBudget, SloMonitor,
    budgets::BudgetTracker,
    objectives::{AvailabilitySlo, LatencySlo},
};

#[test]
fn test_error_budget() {
    let mut budget = ErrorBudget::from_target(99.9);
    assert!((budget.total - 0.1).abs() < 1e-10);

    budget.update(0.05);
    assert!((budget.remaining - 0.05).abs() < 1e-10);
    assert!(!budget.is_exhausted());

    budget.calculate_burn_rate(24.0);
    assert!(budget.burn_rate > 0.0);
}

#[test]
fn test_slo_objectives() {
    let slo = AvailabilitySlo::three_nines();
    assert_eq!(slo.target, 99.9);

    let slo = LatencySlo::p95_100ms();
    assert_eq!(slo.target, 95.0);
}

#[test]
fn test_budget_tracker() {
    let tracker = BudgetTracker::new();
    let budget = ErrorBudget::from_target(99.9);

    tracker.track("test_slo".to_string(), budget);
    tracker.update("test_slo", 0.05);

    let tracked = tracker.get("test_slo").expect("Budget not found");
    assert_eq!(tracked.budget.consumed, 0.05);
}

#[test]
fn test_slo_monitor() {
    let mut monitor = SloMonitor::new();
    monitor.add_slo(AvailabilitySlo::three_nines());

    assert_eq!(monitor.slos().len(), 1);
}
