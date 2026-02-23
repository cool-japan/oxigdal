//! Integration tests for multi-tenancy.

use oxigdal_security::multitenancy::{
    TenantConfig, isolation::IsolationManager, quotas::QuotaManager, tenant::TenantManager,
};

#[test]
fn test_tenant_management() {
    let manager = TenantManager::new();

    let config = TenantConfig::new("tenant-1".to_string(), "Tenant One".to_string());
    manager.add_tenant(config).expect("Failed to add tenant");

    let retrieved = manager
        .get_tenant("tenant-1")
        .expect("Failed to get tenant");
    assert_eq!(retrieved.name, "Tenant One");
}

#[test]
fn test_resource_isolation() {
    let manager = IsolationManager::new();

    manager
        .assign_resource("tenant-1".to_string(), "resource-1".to_string())
        .expect("Failed to assign resource");

    assert!(manager.owns_resource("tenant-1", "resource-1"));
    assert!(!manager.owns_resource("tenant-2", "resource-1"));
}

#[test]
fn test_quota_management() {
    let manager = QuotaManager::new();

    manager
        .set_quota("tenant-1".to_string(), "storage".to_string(), 100)
        .expect("Failed to set quota");

    assert!(
        manager
            .check_quota("tenant-1", "storage", 50)
            .expect("Check failed")
    );
    assert!(
        !manager
            .check_quota("tenant-1", "storage", 150)
            .expect("Check failed")
    );

    manager
        .use_quota("tenant-1", "storage", 50)
        .expect("Failed to use quota");

    assert!(
        !manager
            .check_quota("tenant-1", "storage", 60)
            .expect("Check failed")
    );
}
