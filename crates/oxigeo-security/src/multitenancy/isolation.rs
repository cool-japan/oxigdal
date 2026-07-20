//! Resource isolation between tenants.

use crate::error::Result;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;

/// Resource isolation manager.
pub struct IsolationManager {
    /// Tenant to resources mapping.
    tenant_resources: Arc<DashMap<String, HashSet<String>>>,
}

impl IsolationManager {
    /// Create new isolation manager.
    pub fn new() -> Self {
        Self {
            tenant_resources: Arc::new(DashMap::new()),
        }
    }

    /// Assign resource to tenant.
    pub fn assign_resource(&self, tenant_id: String, resource_id: String) -> Result<()> {
        self.tenant_resources
            .entry(tenant_id)
            .or_default()
            .insert(resource_id);
        Ok(())
    }

    /// Check if tenant owns resource.
    pub fn owns_resource(&self, tenant_id: &str, resource_id: &str) -> bool {
        self.tenant_resources
            .get(tenant_id)
            .is_some_and(|resources| resources.contains(resource_id))
    }

    /// Get all resources for tenant.
    pub fn get_resources(&self, tenant_id: &str) -> Vec<String> {
        self.tenant_resources
            .get(tenant_id)
            .map(|resources| resources.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for IsolationManager {
    fn default() -> Self {
        Self::new()
    }
}
