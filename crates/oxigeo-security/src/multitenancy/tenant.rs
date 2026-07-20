//! Tenant management.

use crate::error::{Result, SecurityError};
use crate::multitenancy::TenantConfig;
use dashmap::DashMap;
use std::sync::Arc;

/// Tenant manager.
pub struct TenantManager {
    tenants: Arc<DashMap<String, TenantConfig>>,
}

impl TenantManager {
    /// Create new tenant manager.
    pub fn new() -> Self {
        Self {
            tenants: Arc::new(DashMap::new()),
        }
    }

    /// Add a tenant.
    pub fn add_tenant(&self, config: TenantConfig) -> Result<()> {
        self.tenants.insert(config.tenant_id.clone(), config);
        Ok(())
    }

    /// Get a tenant.
    pub fn get_tenant(&self, tenant_id: &str) -> Result<TenantConfig> {
        self.tenants
            .get(tenant_id)
            .map(|t| t.clone())
            .ok_or_else(|| SecurityError::tenant_not_found(tenant_id))
    }

    /// List all tenants.
    pub fn list_tenants(&self) -> Vec<TenantConfig> {
        self.tenants.iter().map(|t| t.value().clone()).collect()
    }

    /// Remove a tenant.
    pub fn remove_tenant(&self, tenant_id: &str) -> Result<()> {
        self.tenants
            .remove(tenant_id)
            .ok_or_else(|| SecurityError::tenant_not_found(tenant_id))?;
        Ok(())
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new()
    }
}
