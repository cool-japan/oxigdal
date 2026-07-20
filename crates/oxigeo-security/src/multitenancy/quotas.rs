//! Per-tenant quotas.

use crate::error::{Result, SecurityError};
use dashmap::DashMap;
use std::sync::Arc;

/// Quota entry: (limit, used counter).
type QuotaEntry = (u64, Arc<parking_lot::RwLock<u64>>);

/// Quota key: (tenant_id, resource_type).
type QuotaKey = (String, String);

/// Quota manager.
pub struct QuotaManager {
    /// Tenant quotas (resource_type -> (limit, used)).
    quotas: Arc<DashMap<QuotaKey, QuotaEntry>>,
}

impl QuotaManager {
    /// Create new quota manager.
    pub fn new() -> Self {
        Self {
            quotas: Arc::new(DashMap::new()),
        }
    }

    /// Set quota for tenant.
    pub fn set_quota(&self, tenant_id: String, resource_type: String, limit: u64) -> Result<()> {
        self.quotas.insert(
            (tenant_id, resource_type),
            (limit, Arc::new(parking_lot::RwLock::new(0))),
        );
        Ok(())
    }

    /// Check if quota available.
    pub fn check_quota(&self, tenant_id: &str, resource_type: &str, amount: u64) -> Result<bool> {
        let key = (tenant_id.to_string(), resource_type.to_string());
        if let Some(entry) = self.quotas.get(&key) {
            let (limit, used) = entry.value();
            let current = *used.read();
            Ok(current + amount <= *limit)
        } else {
            Ok(true) // No quota set = unlimited
        }
    }

    /// Use quota.
    pub fn use_quota(&self, tenant_id: &str, resource_type: &str, amount: u64) -> Result<()> {
        let key = (tenant_id.to_string(), resource_type.to_string());
        if let Some(entry) = self.quotas.get(&key) {
            let (limit, used) = entry.value();
            let mut current = used.write();
            if *current + amount > *limit {
                return Err(SecurityError::quota_exceeded(format!(
                    "Quota exceeded for {} ({})",
                    tenant_id, resource_type
                )));
            }
            *current += amount;
        }
        Ok(())
    }

    /// Release quota.
    pub fn release_quota(&self, tenant_id: &str, resource_type: &str, amount: u64) -> Result<()> {
        let key = (tenant_id.to_string(), resource_type.to_string());
        if let Some(entry) = self.quotas.get(&key) {
            let (_, used) = entry.value();
            let mut current = used.write();
            *current = current.saturating_sub(amount);
        }
        Ok(())
    }
}

impl Default for QuotaManager {
    fn default() -> Self {
        Self::new()
    }
}
