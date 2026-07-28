//! Disaster recovery tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_ha::dr::control_plane::{InMemoryDrControlPlane, RegionState};
use oxigeo_ha::dr::{
    DrConfig, DrExecutor, DrProbe, orchestration::DrOrchestrator, runbook::Runbook,
    testing::DrTester,
};
use std::sync::Arc;

fn wired_plane() -> Arc<InMemoryDrControlPlane> {
    let plane = Arc::new(InMemoryDrControlPlane::new());
    plane.register_region(
        "us-east-1",
        RegionState {
            is_primary: true,
            accepts_traffic: true,
            data_watermark: 100,
            ..Default::default()
        },
    );
    plane.register_region(
        "us-west-2",
        RegionState {
            data_watermark: 100,
            ..Default::default()
        },
    );
    plane
}

#[tokio::test]
async fn test_dr_failover_performs_real_cutover() {
    let plane = wired_plane();
    let orchestrator = DrOrchestrator::new(DrConfig::default());
    orchestrator.set_executor(Arc::clone(&plane) as Arc<dyn DrExecutor>);

    let result = orchestrator
        .execute_failover()
        .await
        .expect("DR failover execution should succeed");
    assert!(result.success);
    assert!(result.rto_achieved_seconds <= 300);

    // The DR region really became the primary and receives traffic.
    assert_eq!(plane.traffic_target().as_deref(), Some("us-west-2"));
    assert!(plane.region_state("us-west-2").unwrap().is_primary);
}

#[tokio::test]
async fn test_dr_failover_without_executor_errors() {
    let orchestrator = DrOrchestrator::new(DrConfig::default());
    assert!(orchestrator.execute_failover().await.is_err());
}

#[tokio::test]
async fn test_dr_runbook() {
    let runbook = Runbook::failover_runbook();

    assert_eq!(runbook.name, "DR Failover");
    assert!(!runbook.steps.is_empty());

    assert!(runbook.execute().await.is_ok());
}

#[tokio::test]
async fn test_dr_testing_healthy() {
    let plane = wired_plane();
    let tester = DrTester::new(DrConfig::default());
    tester.set_probe(Arc::clone(&plane) as Arc<dyn DrProbe>);

    let result = tester
        .execute_test()
        .await
        .expect("DR test execution should succeed");
    assert!(result.success, "issues: {:?}", result.issues);
    assert!(result.issues.is_empty());
}

#[tokio::test]
async fn test_dr_testing_detects_unreachable_dr() {
    let plane = wired_plane();
    plane.set_reachable("us-west-2", false).unwrap();
    let tester = DrTester::new(DrConfig::default());
    tester.set_probe(Arc::clone(&plane) as Arc<dyn DrProbe>);

    let result = tester.execute_test().await.unwrap();
    assert!(!result.success);
    assert!(!result.issues.is_empty());
}
