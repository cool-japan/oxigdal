//! Multi-tenancy support.

pub mod isolation;
pub mod quotas;
pub mod tenant;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tenant configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// Tenant ID.
    pub tenant_id: String,
    /// Tenant name.
    pub name: String,
    /// Encryption key ID for this tenant.
    pub encryption_key_id: Option<String>,
    /// Resource quotas.
    pub quotas: HashMap<String, u64>,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
}

impl TenantConfig {
    /// Create new tenant config.
    pub fn new(tenant_id: String, name: String) -> Self {
        Self {
            tenant_id,
            name,
            encryption_key_id: None,
            quotas: HashMap::new(),
            metadata: HashMap::new(),
        }
    }
}
