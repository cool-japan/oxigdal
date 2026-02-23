//! Service worker registration management.

use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};
use web_sys::{ServiceWorker, ServiceWorkerRegistration, ServiceWorkerState};

/// Service worker registration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationConfig {
    /// URL of the service worker script
    pub script_url: String,

    /// Scope of the service worker
    pub scope: Option<String>,

    /// Update via cache mode
    pub update_via_cache: UpdateViaCache,

    /// Type of service worker
    pub worker_type: WorkerType,
}

/// Update via cache mode for service workers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UpdateViaCache {
    /// Import scripts will be cached
    Imports,

    /// All resources will be cached
    All,

    /// Nothing will be cached
    None,
}

impl UpdateViaCache {
    /// Convert to JavaScript string value.
    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateViaCache::Imports => "imports",
            UpdateViaCache::All => "all",
            UpdateViaCache::None => "none",
        }
    }
}

/// Type of service worker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkerType {
    /// Classic worker
    Classic,

    /// Module worker
    Module,
}

impl WorkerType {
    /// Convert to JavaScript string value.
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerType::Classic => "classic",
            WorkerType::Module => "module",
        }
    }
}

impl Default for RegistrationConfig {
    fn default() -> Self {
        Self {
            script_url: "/sw.js".to_string(),
            scope: None,
            update_via_cache: UpdateViaCache::Imports,
            worker_type: WorkerType::Classic,
        }
    }
}

/// Service worker registry for managing registrations.
pub struct ServiceWorkerRegistry {
    config: RegistrationConfig,
}

impl ServiceWorkerRegistry {
    /// Create a new service worker registry with configuration.
    pub fn new(config: RegistrationConfig) -> Self {
        Self { config }
    }

    /// Create a registry with default configuration.
    pub fn with_defaults() -> Self {
        Self {
            config: RegistrationConfig::default(),
        }
    }

    /// Create a registry with a specific script URL.
    pub fn with_script_url(script_url: impl Into<String>) -> Self {
        Self {
            config: RegistrationConfig {
                script_url: script_url.into(),
                ..Default::default()
            },
        }
    }

    /// Set the scope for the service worker.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.config.scope = Some(scope.into());
        self
    }

    /// Set the update via cache mode.
    pub fn with_update_via_cache(mut self, mode: UpdateViaCache) -> Self {
        self.config.update_via_cache = mode;
        self
    }

    /// Set the worker type.
    pub fn with_worker_type(mut self, worker_type: WorkerType) -> Self {
        self.config.worker_type = worker_type;
        self
    }

    /// Register the service worker.
    pub async fn register(&self) -> Result<ServiceWorkerRegistration> {
        super::register_service_worker(&self.config.script_url, self.config.scope.as_deref()).await
    }

    /// Get the current registration.
    pub async fn get_registration(&self) -> Result<Option<ServiceWorkerRegistration>> {
        super::get_registration(self.config.scope.as_deref()).await
    }

    /// Update the registration if it exists.
    pub async fn update(&self) -> Result<Option<ServiceWorkerRegistration>> {
        if let Some(registration) = self.get_registration().await? {
            let updated = super::update_registration(&registration).await?;
            Ok(Some(updated))
        } else {
            Ok(None)
        }
    }

    /// Unregister the service worker.
    pub async fn unregister(&self) -> Result<bool> {
        if let Some(registration) = self.get_registration().await? {
            super::unregister_service_worker(&registration).await
        } else {
            Ok(false)
        }
    }
}

/// Get the state of a service worker.
pub fn get_service_worker_state(worker: &ServiceWorker) -> ServiceWorkerState {
    worker.state()
}

/// Check if a service worker is in a specific state.
pub fn is_service_worker_state(worker: &ServiceWorker, state: ServiceWorkerState) -> bool {
    worker.state() == state
}

/// Wait for a service worker to become active.
pub async fn wait_for_active(registration: &ServiceWorkerRegistration) -> Result<ServiceWorker> {
    // Check if already active
    if let Some(active) = registration.active() {
        return Ok(active);
    }

    // Check if installing or waiting
    if let Some(_worker) = registration.installing().or_else(|| registration.waiting()) {
        // In a real implementation, we would wait for the state change event
        // For now, we'll return an error if not active
        return Err(PwaError::InvalidState(
            "Service worker is not active yet".to_string(),
        ));
    }

    Err(PwaError::InvalidState(
        "No service worker found in registration".to_string(),
    ))
}

/// Registration status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationStatus {
    /// Whether a service worker is registered
    pub is_registered: bool,

    /// Active service worker scope
    pub scope: Option<String>,

    /// Whether an update is available
    pub update_available: bool,

    /// Current state
    pub state: String,
}

impl RegistrationStatus {
    /// Get the current registration status.
    pub async fn current() -> Result<Self> {
        let registration = super::get_registration(None).await?;

        if let Some(reg) = registration {
            let scope = reg.scope();
            let active = reg.active();
            let installing = reg.installing();
            let waiting = reg.waiting();

            let state = if active.is_some() {
                "active".to_string()
            } else if waiting.is_some() {
                "waiting".to_string()
            } else if installing.is_some() {
                "installing".to_string()
            } else {
                "unknown".to_string()
            };

            Ok(Self {
                is_registered: true,
                scope: Some(scope),
                update_available: waiting.is_some(),
                state,
            })
        } else {
            Ok(Self {
                is_registered: false,
                scope: None,
                update_available: false,
                state: "none".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registration_config_default() {
        let config = RegistrationConfig::default();
        assert_eq!(config.script_url, "/sw.js");
        assert!(config.scope.is_none());
    }

    #[test]
    fn test_update_via_cache_str() {
        assert_eq!(UpdateViaCache::Imports.as_str(), "imports");
        assert_eq!(UpdateViaCache::All.as_str(), "all");
        assert_eq!(UpdateViaCache::None.as_str(), "none");
    }

    #[test]
    fn test_worker_type_str() {
        assert_eq!(WorkerType::Classic.as_str(), "classic");
        assert_eq!(WorkerType::Module.as_str(), "module");
    }

    #[test]
    fn test_registry_builder() {
        let registry = ServiceWorkerRegistry::with_script_url("/custom-sw.js")
            .with_scope("/app")
            .with_update_via_cache(UpdateViaCache::All)
            .with_worker_type(WorkerType::Module);

        assert_eq!(registry.config.script_url, "/custom-sw.js");
        assert_eq!(registry.config.scope, Some("/app".to_string()));
    }
}
