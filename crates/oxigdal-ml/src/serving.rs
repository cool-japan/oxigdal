//! Model serving and deployment utilities
//!
//! This module provides production-ready model serving capabilities including
//! model versioning, A/B testing, canary deployments, and load balancing.

use crate::error::{MlError, Result};
// use crate::models::Model;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

/// Model version information
#[derive(Debug, Clone)]
pub struct ModelVersion {
    /// Version identifier
    pub version: String,
    /// Model file path
    pub path: PathBuf,
    /// Deployment timestamp
    pub deployed_at: std::time::SystemTime,
    /// Model metadata
    pub metadata: HashMap<String, String>,
    /// Performance metrics
    pub metrics: VersionMetrics,
}

/// Performance metrics for a model version
#[derive(Debug, Clone, Default)]
pub struct VersionMetrics {
    /// Total requests served
    pub requests: u64,
    /// Average latency in milliseconds
    pub avg_latency_ms: f32,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f32,
    /// Average CPU usage percentage
    pub avg_cpu_usage: f32,
    /// Average memory usage in MB
    pub avg_memory_mb: f32,
}

/// Deployment strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentStrategy {
    /// Direct replacement
    Replace,
    /// Blue-green deployment
    BlueGreen,
    /// Canary deployment with gradual rollout
    Canary {
        /// Initial traffic percentage (0-100)
        initial_percent: u8,
        /// Step size for traffic increase
        step_percent: u8,
    },
    /// A/B testing
    ABTest {
        /// Traffic split percentage for new version
        split_percent: u8,
    },
    /// Shadow mode: the version is recorded as deployed for observation while
    /// the stable version keeps serving all user-facing traffic. The shadow
    /// version never returns user-facing results.
    Shadow,
}

/// Model server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Maximum concurrent requests
    pub max_concurrent: usize,
    /// Request timeout in milliseconds
    pub timeout_ms: u64,
    /// Enable request queuing
    pub enable_queue: bool,
    /// Queue size limit
    pub queue_size: usize,
    /// Enable health checks
    pub health_check: bool,
    /// Health check interval in seconds
    pub health_check_interval_s: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 100,
            timeout_ms: 30000,
            enable_queue: true,
            queue_size: 1000,
            health_check: true,
            health_check_interval_s: 30,
        }
    }
}

/// Model server for production deployment
pub struct ModelServer {
    config: ServerConfig,
    versions: Arc<RwLock<HashMap<String, ModelVersion>>>,
    active_version: Arc<RwLock<String>>,
    routing: Arc<RwLock<RoutingStrategy>>,
}

/// Traffic routing strategy
#[derive(Debug, Clone)]
enum RoutingStrategy {
    /// Single version
    Single {
        /// Version ID
        version: String,
    },
    /// Weighted routing
    Weighted {
        /// Version weights (version -> percentage)
        weights: HashMap<String, u8>,
    },
    /// Canary routing
    Canary {
        /// Stable version
        stable: String,
        /// Canary version
        canary: String,
        /// Canary traffic percentage
        canary_percent: u8,
    },
    /// Shadow deployment: `stable` serves all user-facing traffic while `shadow`
    /// is recorded as deployed for observation only. This records deployment
    /// intent/state; it does not itself mirror live requests (this module has no
    /// request-serving path).
    Shadow {
        /// User-facing stable version
        stable: String,
        /// Shadow version (never serves user-facing results)
        shadow: String,
    },
}

impl ModelServer {
    /// Creates a new model server
    #[must_use]
    pub fn new(config: ServerConfig) -> Self {
        info!("Initializing model server");
        Self {
            config,
            versions: Arc::new(RwLock::new(HashMap::new())),
            active_version: Arc::new(RwLock::new(String::new())),
            routing: Arc::new(RwLock::new(RoutingStrategy::Single {
                version: String::new(),
            })),
        }
    }

    /// Registers a new model version
    ///
    /// # Errors
    /// Returns an error if version registration fails
    pub fn register_version(
        &mut self,
        version_id: &str,
        model_path: PathBuf,
        metadata: HashMap<String, String>,
    ) -> Result<()> {
        info!("Registering model version: {}", version_id);

        if !model_path.exists() {
            return Err(MlError::InvalidConfig(format!(
                "Model file not found: {}",
                model_path.display()
            )));
        }

        let version = ModelVersion {
            version: version_id.to_string(),
            path: model_path,
            deployed_at: std::time::SystemTime::now(),
            metadata,
            metrics: VersionMetrics::default(),
        };

        if let Ok(mut versions) = self.versions.write() {
            versions.insert(version_id.to_string(), version);
        }

        Ok(())
    }

    /// Deploys a model version using the specified strategy
    ///
    /// # Errors
    /// Returns an error if deployment fails
    pub fn deploy(&mut self, version_id: &str, strategy: DeploymentStrategy) -> Result<()> {
        info!(
            "Deploying version {} with strategy {:?}",
            version_id, strategy
        );

        // Verify version exists
        let version_exists = self
            .versions
            .read()
            .map(|v| v.contains_key(version_id))
            .unwrap_or(false);

        if !version_exists {
            return Err(MlError::InvalidConfig(format!(
                "Version not found: {}",
                version_id
            )));
        }

        match strategy {
            DeploymentStrategy::Replace => self.deploy_replace(version_id),
            DeploymentStrategy::BlueGreen => self.deploy_blue_green(version_id),
            DeploymentStrategy::Canary {
                initial_percent,
                step_percent,
            } => self.deploy_canary(version_id, initial_percent, step_percent),
            DeploymentStrategy::ABTest { split_percent } => {
                self.deploy_ab_test(version_id, split_percent)
            }
            DeploymentStrategy::Shadow => self.deploy_shadow(version_id),
        }
    }

    /// Rolls back to a previous version
    ///
    /// # Errors
    /// Returns an error if rollback fails
    pub fn rollback(&mut self, version_id: &str) -> Result<()> {
        info!("Rolling back to version: {}", version_id);
        self.deploy_replace(version_id)
    }

    /// Returns metrics for all versions
    #[must_use]
    pub fn version_metrics(&self) -> HashMap<String, VersionMetrics> {
        self.versions
            .read()
            .map(|versions| {
                versions
                    .iter()
                    .map(|(k, v)| (k.clone(), v.metrics.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the active version
    #[must_use]
    pub fn active_version(&self) -> String {
        self.active_version
            .read()
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Performs health check on active version
    #[must_use]
    pub fn health_check(&self) -> HealthStatus {
        if !self.config.health_check {
            return HealthStatus::Unknown;
        }

        // Check if any model is loaded
        let has_models = self.versions.read().map(|v| !v.is_empty()).unwrap_or(false);

        if !has_models {
            return HealthStatus::Unhealthy;
        }

        // Check if active version exists
        let active_version = self.active_version();
        if active_version.is_empty() {
            return HealthStatus::Degraded;
        }

        // Verify active version is in versions map
        let version_exists = self
            .versions
            .read()
            .map(|v| v.contains_key(&active_version))
            .unwrap_or(false);

        if !version_exists {
            return HealthStatus::Unhealthy;
        }

        // Check live memory pressure. The most severe threshold must be tested
        // first so that > 95% reports Unhealthy rather than being shadowed by the
        // > 90% Degraded branch.
        if let Ok(memory_info) = Self::get_memory_usage() {
            if let Some(status) = Self::memory_pressure_status(memory_info.usage_percent) {
                return status;
            }
        }

        HealthStatus::Healthy
    }

    /// Maps a memory-usage percentage to a degraded/unhealthy health status.
    ///
    /// Returns `None` when usage is within safe limits. Extracted as a pure
    /// function so the degradation thresholds are unit-testable without
    /// fabricating system memory.
    fn memory_pressure_status(usage_percent: f32) -> Option<HealthStatus> {
        if usage_percent > 95.0 {
            Some(HealthStatus::Unhealthy)
        } else if usage_percent > 90.0 {
            Some(HealthStatus::Degraded)
        } else {
            None
        }
    }

    /// Gets current memory usage information
    fn get_memory_usage() -> Result<MemoryInfo> {
        #[cfg(target_os = "linux")]
        {
            Self::get_memory_usage_linux()
        }

        #[cfg(target_os = "macos")]
        {
            Self::get_memory_usage_macos()
        }

        #[cfg(target_os = "windows")]
        {
            Self::get_memory_usage_windows()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            // Default fallback for unsupported platforms
            Ok(MemoryInfo {
                total_mb: 0,
                used_mb: 0,
                available_mb: 0,
                usage_percent: 0.0,
            })
        }
    }

    #[cfg(target_os = "linux")]
    fn get_memory_usage_linux() -> Result<MemoryInfo> {
        use std::fs;

        let meminfo = fs::read_to_string("/proc/meminfo")
            .map_err(|e| MlError::InvalidConfig(format!("Failed to read meminfo: {}", e)))?;

        let mut total = 0u64;
        let mut available = 0u64;

        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total = rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                available = rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
            }
        }

        let total_mb = total / 1024;
        let available_mb = available / 1024;
        let used_mb = total_mb.saturating_sub(available_mb);
        let usage_percent = if total_mb > 0 {
            (used_mb as f32 / total_mb as f32) * 100.0
        } else {
            0.0
        };

        Ok(MemoryInfo {
            total_mb,
            used_mb,
            available_mb,
            usage_percent,
        })
    }

    #[cfg(target_os = "macos")]
    fn get_memory_usage_macos() -> Result<MemoryInfo> {
        // Query live memory via the Pure-Rust `sysinfo` crate (already a
        // dependency), which reads real system statistics on macOS.
        Self::get_memory_usage_sysinfo()
    }

    #[cfg(target_os = "windows")]
    fn get_memory_usage_windows() -> Result<MemoryInfo> {
        // Query live memory via the Pure-Rust `sysinfo` crate (already a
        // dependency), which wraps GlobalMemoryStatusEx on Windows.
        Self::get_memory_usage_sysinfo()
    }

    /// Reads live memory statistics via the Pure-Rust `sysinfo` crate.
    ///
    /// `sysinfo` reports memory in bytes; values are converted to MB. This is
    /// used on platforms without a bespoke reader (macOS, Windows) so that
    /// `health_check` observes real memory pressure instead of a fabricated
    /// constant.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn get_memory_usage_sysinfo() -> Result<MemoryInfo> {
        use sysinfo::System;

        let mut system = System::new();
        system.refresh_memory();

        // sysinfo returns bytes (>= 0.30).
        let total_bytes = system.total_memory();
        if total_bytes == 0 {
            return Err(MlError::InvalidConfig(
                "sysinfo reported zero total memory".to_string(),
            ));
        }
        let available_bytes = system.available_memory();

        let total_mb = total_bytes / (1024 * 1024);
        let available_mb = available_bytes / (1024 * 1024);
        let used_mb = total_mb.saturating_sub(available_mb);
        let usage_percent = ((total_bytes.saturating_sub(available_bytes)) as f64
            / total_bytes as f64
            * 100.0) as f32;

        Ok(MemoryInfo {
            total_mb,
            used_mb,
            available_mb,
            usage_percent,
        })
    }

    // Private deployment methods

    fn deploy_replace(&mut self, version_id: &str) -> Result<()> {
        debug!("Deploying with replace strategy");

        if let Ok(mut active) = self.active_version.write() {
            *active = version_id.to_string();
        }

        if let Ok(mut routing) = self.routing.write() {
            *routing = RoutingStrategy::Single {
                version: version_id.to_string(),
            };
        }

        info!("Version {} deployed successfully", version_id);
        Ok(())
    }

    fn deploy_blue_green(&mut self, version_id: &str) -> Result<()> {
        debug!("Deploying with blue-green strategy");

        // In blue-green, we prepare the new version first
        // Then switch traffic atomically
        self.deploy_replace(version_id)
    }

    fn deploy_canary(
        &mut self,
        version_id: &str,
        initial_percent: u8,
        _step_percent: u8,
    ) -> Result<()> {
        debug!(
            "Deploying with canary strategy ({}% initial)",
            initial_percent
        );

        let stable_version = self.active_version();

        if let Ok(mut routing) = self.routing.write() {
            *routing = RoutingStrategy::Canary {
                stable: stable_version,
                canary: version_id.to_string(),
                canary_percent: initial_percent,
            };
        }

        info!("Canary deployment started for version {}", version_id);
        Ok(())
    }

    fn deploy_ab_test(&mut self, version_id: &str, split_percent: u8) -> Result<()> {
        debug!("Deploying with A/B test ({}% split)", split_percent);

        let stable_version = self.active_version();
        let mut weights = HashMap::new();
        weights.insert(stable_version, 100 - split_percent);
        weights.insert(version_id.to_string(), split_percent);

        if let Ok(mut routing) = self.routing.write() {
            *routing = RoutingStrategy::Weighted { weights };
        }

        info!("A/B test started for version {}", version_id);
        Ok(())
    }

    fn deploy_shadow(&mut self, version_id: &str) -> Result<()> {
        debug!("Deploying in shadow mode");

        // Shadow mode records the new version as deployed-for-observation while
        // the current active (stable) version keeps serving all user-facing
        // traffic. `active_version` is deliberately NOT changed so the shadow
        // version never becomes user-facing. This records deployment state so it
        // is introspectable and consistent with the other deploy_* methods; it
        // does not mirror live requests (no request-serving path exists here).
        let stable_version = self.active_version();

        if let Ok(mut routing) = self.routing.write() {
            *routing = RoutingStrategy::Shadow {
                stable: stable_version,
                shadow: version_id.to_string(),
            };
        }

        info!("Version {} deployed in shadow mode", version_id);
        Ok(())
    }

    /// Increases canary traffic percentage
    ///
    /// # Errors
    /// Returns an error if not in canary mode
    pub fn increase_canary_traffic(&mut self, increment: u8) -> Result<()> {
        let mut routing = self
            .routing
            .write()
            .map_err(|_| MlError::InvalidConfig("Failed to acquire routing lock".to_string()))?;

        match &mut *routing {
            RoutingStrategy::Canary { canary_percent, .. } => {
                *canary_percent = (*canary_percent + increment).min(100);
                info!("Increased canary traffic to {}%", canary_percent);
                Ok(())
            }
            _ => Err(MlError::InvalidConfig(
                "Not in canary deployment mode".to_string(),
            )),
        }
    }

    /// Promotes canary to stable
    ///
    /// # Errors
    /// Returns an error if not in canary mode
    pub fn promote_canary(&mut self) -> Result<()> {
        let routing = self
            .routing
            .read()
            .map_err(|_| MlError::InvalidConfig("Failed to acquire routing lock".to_string()))?;

        if let RoutingStrategy::Canary { canary, .. } = &*routing {
            let canary_version = canary.clone();
            drop(routing); // Release read lock
            self.deploy_replace(&canary_version)?;
            info!("Canary promoted to stable");
            Ok(())
        } else {
            Err(MlError::InvalidConfig(
                "Not in canary deployment mode".to_string(),
            ))
        }
    }
}

/// Health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Service is healthy
    Healthy,
    /// Service is degraded but operational
    Degraded,
    /// Service is unhealthy
    Unhealthy,
    /// Health status unknown
    Unknown,
}

/// Memory usage information
#[derive(Debug, Clone)]
struct MemoryInfo {
    /// Total memory in MB
    total_mb: u64,
    /// Used memory in MB
    used_mb: u64,
    /// Available memory in MB
    available_mb: u64,
    /// Memory usage percentage
    usage_percent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.max_concurrent, 100);
        assert_eq!(config.timeout_ms, 30000);
        assert!(config.enable_queue);
    }

    #[test]
    fn test_deployment_strategy_variants() {
        let strategies = vec![
            DeploymentStrategy::Replace,
            DeploymentStrategy::BlueGreen,
            DeploymentStrategy::Canary {
                initial_percent: 10,
                step_percent: 10,
            },
            DeploymentStrategy::ABTest { split_percent: 50 },
            DeploymentStrategy::Shadow,
        ];

        for strategy in strategies {
            // Just verify they can be created
            let _ = format!("{:?}", strategy);
        }
    }

    #[test]
    fn test_model_server_creation() {
        let config = ServerConfig::default();
        let server = ModelServer::new(config);
        assert_eq!(server.active_version(), "");
    }

    #[test]
    fn test_health_status() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Degraded);
    }

    #[test]
    fn test_memory_pressure_status_thresholds() {
        // Below 90%: healthy (None).
        assert_eq!(ModelServer::memory_pressure_status(50.0), None);
        assert_eq!(ModelServer::memory_pressure_status(90.0), None);
        // Between 90% and 95%: degraded.
        assert_eq!(
            ModelServer::memory_pressure_status(92.0),
            Some(HealthStatus::Degraded)
        );
        // Above 95%: unhealthy (must NOT be shadowed by the degraded branch).
        assert_eq!(
            ModelServer::memory_pressure_status(97.0),
            Some(HealthStatus::Unhealthy)
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn test_get_memory_usage_is_live_not_constant() {
        // On supported platforms, memory stats must be real (non-zero total,
        // usage within 0..=100), not the old fabricated 50% / 16384 MB constant.
        let info = ModelServer::get_memory_usage().expect("memory usage query");
        assert!(info.total_mb > 0, "total memory should be positive");
        assert!(
            info.usage_percent >= 0.0 && info.usage_percent <= 100.0,
            "usage_percent out of range: {}",
            info.usage_percent
        );
        assert!(info.available_mb <= info.total_mb);
    }

    #[test]
    fn test_deploy_shadow_records_state() {
        use std::io::Write;

        // register_version requires the model file to exist on disk.
        let dir = std::env::temp_dir();
        let stable_path = dir.join("oxigdal_ml_shadow_stable.onnx");
        let shadow_path = dir.join("oxigdal_ml_shadow_new.onnx");
        for p in [&stable_path, &shadow_path] {
            let mut f = std::fs::File::create(p).expect("create temp model file");
            f.write_all(b"onnx").expect("write temp model");
        }

        let mut server = ModelServer::new(ServerConfig::default());
        server
            .register_version("v1", stable_path.clone(), HashMap::new())
            .expect("register v1");
        server
            .register_version("v2", shadow_path.clone(), HashMap::new())
            .expect("register v2");

        // v1 becomes the active/stable version.
        server
            .deploy("v1", DeploymentStrategy::Replace)
            .expect("deploy v1");
        assert_eq!(server.active_version(), "v1");

        // Deploy v2 in shadow mode: routing state must reflect it, and the active
        // (user-facing) version must stay v1.
        server
            .deploy("v2", DeploymentStrategy::Shadow)
            .expect("deploy v2 shadow");
        assert_eq!(
            server.active_version(),
            "v1",
            "shadow must not become active"
        );

        let routing = server.routing.read().expect("read routing");
        match &*routing {
            RoutingStrategy::Shadow { stable, shadow } => {
                assert_eq!(stable, "v1");
                assert_eq!(shadow, "v2");
            }
            other => panic!("expected Shadow routing, got {:?}", other),
        }
        drop(routing);

        let _ = std::fs::remove_file(stable_path);
        let _ = std::fs::remove_file(shadow_path);
    }

    #[test]
    fn test_version_metrics() {
        let metrics = VersionMetrics {
            requests: 1000,
            avg_latency_ms: 50.0,
            success_rate: 0.99,
            avg_cpu_usage: 45.0,
            avg_memory_mb: 512.0,
        };

        assert_eq!(metrics.requests, 1000);
        assert!((metrics.success_rate - 0.99).abs() < 0.01);
    }
}
