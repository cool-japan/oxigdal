//! Disaster recovery tests.

use oxigdal_ha::dr::{
    DrConfig, orchestration::DrOrchestrator, runbook::Runbook, testing::DrTester,
};

#[tokio::test]
async fn test_dr_failover() {
    let config = DrConfig::default();
    let orchestrator = DrOrchestrator::new(config);

    let result = orchestrator.execute_failover().await.ok();
    assert!(result.is_some());

    let result = result.expect("DR failover execution should succeed");
    assert!(result.success);
    assert!(result.rto_achieved_seconds <= 300);
}

#[tokio::test]
async fn test_dr_runbook() {
    let runbook = Runbook::failover_runbook();

    assert_eq!(runbook.name, "DR Failover");
    assert!(!runbook.steps.is_empty());

    assert!(runbook.execute().await.is_ok());
}

#[tokio::test]
async fn test_dr_testing() {
    let config = DrConfig::default();
    let tester = DrTester::new(config);

    let result = tester.execute_test().await.ok();
    assert!(result.is_some());

    let result = result.expect("DR test execution should succeed");
    assert!(result.success);
    assert!(result.issues.is_empty());
}
