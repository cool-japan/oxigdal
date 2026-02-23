//! Example of SLO monitoring and error budget tracking.

use oxigdal_observability::slo::{
    SloMonitor,
    budgets::BudgetTracker,
    objectives::{AvailabilitySlo, LatencySlo},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create SLO monitor
    let mut monitor = SloMonitor::new();

    // Add SLOs
    monitor.add_slo(AvailabilitySlo::three_nines());
    monitor.add_slo(LatencySlo::p95_100ms());

    println!("Monitoring {} SLOs", monitor.slos().len());

    // Create budget tracker
    let tracker = BudgetTracker::new();

    for slo in monitor.slos() {
        println!("SLO: {} - Target: {}%", slo.name, slo.target);
        tracker.track(slo.name.clone(), slo.error_budget.clone());
    }

    // Simulate budget consumption
    tracker.update("availability_99.9", 0.05);

    if let Some(budget) = tracker.get("availability_99.9") {
        println!(
            "Error budget consumed: {:.2}%",
            budget.budget.exhaustion_percentage()
        );
        println!("Remaining budget: {:.4}%", budget.budget.remaining);
    }

    Ok(())
}
