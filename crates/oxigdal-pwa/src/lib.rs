//! Progressive Web App capabilities for OxiGDAL.
//!
//! This crate provides comprehensive PWA functionality including:
//! - Service worker integration
//! - Offline caching strategies
//! - Background sync
//! - Push notifications
//! - Web app manifest generation
//! - PWA lifecycle management
//! - Geospatial data caching optimizations
//!
//! # Examples
//!
//! ## Basic Service Worker Registration
//!
//! ```no_run
//! use oxigdal_pwa::service_worker::ServiceWorkerRegistry;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = ServiceWorkerRegistry::with_script_url("/sw.js")
//!     .with_scope("/app");
//!
//! let registration = registry.register().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Caching Strategies
//!
//! ```no_run
//! use oxigdal_pwa::cache::strategies::{CacheStrategy, StrategyType};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a cache-first strategy for static assets
//! let strategy = CacheStrategy::cache_first("static-assets");
//!
//! // Or network-first for API calls
//! let api_strategy = CacheStrategy::network_first("api-cache");
//! # Ok(())
//! # }
//! ```
//!
//! ## Geospatial Tile Caching
//!
//! ```no_run
//! use oxigdal_pwa::cache::geospatial::{GeospatialCache, BoundingBox};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let cache = GeospatialCache::with_defaults();
//!
//! // Cache tiles for an area
//! let bbox = BoundingBox::new(-180.0, -85.0, 180.0, 85.0)?;
//! let tiles = cache.prefetch_tiles(&bbox, 0..5, "https://tiles.example.com").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Push Notifications
//!
//! ```no_run
//! use oxigdal_pwa::notifications::{NotificationManager, NotificationConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = NotificationManager::new();
//!
//! // Request permission
//! let permission = NotificationManager::request_permission().await?;
//!
//! if permission.is_granted() {
//!     let config = NotificationConfig::new("New Data Available")
//!         .with_body("Your geospatial data has been updated")
//!         .with_icon("/icon.png");
//!
//!     manager.show(&config).await?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Web App Manifest
//!
//! ```
//! use oxigdal_pwa::manifest::{ManifestBuilder, DisplayMode};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manifest = ManifestBuilder::geospatial("GeoApp", "Geo")
//!     .description("A powerful geospatial PWA")
//!     .colors("#ffffff", "#007bff")
//!     .add_standard_icons("/icons")
//!     .build();
//!
//! let json = manifest.to_json()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `default`: Basic PWA functionality with console error hooks
//! - `notifications`: Push notification support
//! - `background-sync`: Background synchronization
//! - `geospatial-cache`: Geospatial-specific caching optimizations

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod cache;
pub mod error;
pub mod lifecycle;
pub mod manifest;
pub mod notifications;
pub mod service_worker;
pub mod sync;

// Re-export commonly used types
pub use error::{PwaError, Result};

pub use cache::{
    CacheManager, CacheStorageManager,
    geospatial::{BoundingBox, GeospatialCache, TileCoord},
    strategies::{CacheStrategy, StrategyType},
};

pub use lifecycle::{
    DisplayModeDetection, InstallPrompt, InstallState, PwaLifecycle, UpdateManager,
};

pub use manifest::{
    AppIcon, DisplayMode, ManifestBuilder, Orientation, Screenshot, WebAppManifest,
};

pub use notifications::{
    NotificationAction, NotificationConfig, NotificationManager, Permission,
    PushNotificationManager,
};

pub use service_worker::{
    ServiceWorkerEvents, ServiceWorkerMessaging, ServiceWorkerRegistry, ServiceWorkerScope,
    get_registration, get_service_worker_container, is_service_worker_supported,
    register_service_worker,
};

pub use sync::{BackgroundSync, QueuedOperation, SyncCoordinator, SyncOptions, SyncQueue};

/// Initialize PWA with console error panic hook.
pub fn initialize() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// PWA configuration and initialization.
pub struct PwaConfig {
    /// Service worker script URL
    pub service_worker_url: String,

    /// Service worker scope
    pub scope: Option<String>,

    /// Enable automatic cache management
    pub enable_cache_management: bool,

    /// Enable background sync
    pub enable_background_sync: bool,

    /// Enable notifications
    pub enable_notifications: bool,

    /// Enable geospatial cache optimizations
    pub enable_geospatial_cache: bool,
}

impl Default for PwaConfig {
    fn default() -> Self {
        Self {
            service_worker_url: "/sw.js".to_string(),
            scope: None,
            enable_cache_management: true,
            enable_background_sync: false,
            enable_notifications: false,
            enable_geospatial_cache: true,
        }
    }
}

impl PwaConfig {
    /// Create a new PWA configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the service worker URL.
    pub fn with_service_worker_url(mut self, url: impl Into<String>) -> Self {
        self.service_worker_url = url.into();
        self
    }

    /// Set the scope.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Enable cache management.
    pub fn with_cache_management(mut self, enable: bool) -> Self {
        self.enable_cache_management = enable;
        self
    }

    /// Enable background sync.
    pub fn with_background_sync(mut self, enable: bool) -> Self {
        self.enable_background_sync = enable;
        self
    }

    /// Enable notifications.
    pub fn with_notifications(mut self, enable: bool) -> Self {
        self.enable_notifications = enable;
        self
    }

    /// Enable geospatial cache.
    pub fn with_geospatial_cache(mut self, enable: bool) -> Self {
        self.enable_geospatial_cache = enable;
        self
    }
}

/// Main PWA application manager.
pub struct PwaApp {
    config: PwaConfig,
    lifecycle: PwaLifecycle,
    cache_manager: Option<CacheManager>,
    geospatial_cache: Option<GeospatialCache>,
    notification_manager: Option<NotificationManager>,
    sync_coordinator: Option<SyncCoordinator>,
}

impl PwaApp {
    /// Create a new PWA application.
    pub fn new(config: PwaConfig) -> Self {
        Self {
            config,
            lifecycle: PwaLifecycle::new(),
            cache_manager: None,
            geospatial_cache: None,
            notification_manager: None,
            sync_coordinator: None,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(PwaConfig::default())
    }

    /// Initialize the PWA application.
    pub async fn initialize(&mut self) -> Result<()> {
        // Initialize console error hook
        initialize();

        // Initialize lifecycle
        self.lifecycle.initialize()?;

        // Register service worker
        let registration = register_service_worker(
            &self.config.service_worker_url,
            self.config.scope.as_deref(),
        )
        .await?;

        self.lifecycle.set_registration(registration.clone());

        // Initialize cache management
        if self.config.enable_cache_management {
            self.cache_manager = Some(CacheManager::new("oxigdal-pwa-cache"));
        }

        // Initialize geospatial cache
        if self.config.enable_geospatial_cache {
            self.geospatial_cache = Some(GeospatialCache::with_defaults());
        }

        // Initialize notification manager
        if self.config.enable_notifications {
            self.notification_manager = Some(NotificationManager::new());
        }

        // Initialize sync coordinator
        if self.config.enable_background_sync {
            self.sync_coordinator = Some(SyncCoordinator::new(registration));
        }

        Ok(())
    }

    /// Get the lifecycle manager.
    pub fn lifecycle(&self) -> &PwaLifecycle {
        &self.lifecycle
    }

    /// Get mutable lifecycle manager.
    pub fn lifecycle_mut(&mut self) -> &mut PwaLifecycle {
        &mut self.lifecycle
    }

    /// Get the cache manager.
    pub fn cache_manager(&self) -> Option<&CacheManager> {
        self.cache_manager.as_ref()
    }

    /// Get the geospatial cache.
    pub fn geospatial_cache(&self) -> Option<&GeospatialCache> {
        self.geospatial_cache.as_ref()
    }

    /// Get the notification manager.
    pub fn notification_manager(&self) -> Option<&NotificationManager> {
        self.notification_manager.as_ref()
    }

    /// Get the sync coordinator.
    pub fn sync_coordinator(&self) -> Option<&SyncCoordinator> {
        self.sync_coordinator.as_ref()
    }

    /// Get mutable sync coordinator.
    pub fn sync_coordinator_mut(&mut self) -> Option<&mut SyncCoordinator> {
        self.sync_coordinator.as_mut()
    }

    /// Check if running as PWA.
    pub fn is_pwa(&self) -> bool {
        self.lifecycle.is_pwa()
    }

    /// Check if install prompt is available.
    pub fn can_install(&self) -> bool {
        self.lifecycle.can_install()
    }

    /// Show install prompt.
    pub async fn prompt_install(&mut self) -> Result<bool> {
        self.lifecycle.prompt_install().await
    }

    /// Get the configuration.
    pub fn config(&self) -> &PwaConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pwa_config() {
        let config = PwaConfig::new()
            .with_service_worker_url("/custom-sw.js")
            .with_scope("/app")
            .with_cache_management(true)
            .with_geospatial_cache(true);

        assert_eq!(config.service_worker_url, "/custom-sw.js");
        assert_eq!(config.scope, Some("/app".to_string()));
        assert!(config.enable_cache_management);
        assert!(config.enable_geospatial_cache);
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_pwa_app_creation() {
        let app = PwaApp::with_defaults();
        assert_eq!(app.config.service_worker_url, "/sw.js");
        assert!(app.cache_manager.is_none()); // Not initialized yet
    }

    #[test]
    fn test_pwa_config_builder() {
        let config = PwaConfig::default();
        assert_eq!(config.service_worker_url, "/sw.js");
        assert!(config.enable_cache_management);
        assert!(config.enable_geospatial_cache);
    }
}
