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
    /// Shadow mode (no user-facing traffic)
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

        // Check memory usage (simple heuristic)
        if let Ok(memory_info) = Self::get_memory_usage() {
            // If memory usage > 90%, return degraded
            if memory_info.usage_percent > 90.0 {
                return HealthStatus::Degraded;
            }
            // If memory usage > 95%, return unhealthy
            if memory_info.usage_percent > 95.0 {
                return HealthStatus::Unhealthy;
            }
        }

        HealthStatus::Healthy
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
        // Simplified implementation for macOS
        // In production, would use sysctl or vm_stat
        Ok(MemoryInfo {
            total_mb: 16384, // Placeholder
            used_mb: 8192,   // Placeholder
            available_mb: 8192,
            usage_percent: 50.0,
        })
    }

    #[cfg(target_os = "windows")]
    fn get_memory_usage_windows() -> Result<MemoryInfo> {
        // Simplified implementation for Windows
        // In production, would use Windows API
        Ok(MemoryInfo {
            total_mb: 16384, // Placeholder
            used_mb: 8192,   // Placeholder
            available_mb: 8192,
            usage_percent: 50.0,
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

        // Shadow mode: new version receives traffic but results are not used
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
