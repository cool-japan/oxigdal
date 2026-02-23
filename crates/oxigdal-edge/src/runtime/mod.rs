//! Lightweight edge runtime for resource-constrained devices

pub mod executor;
pub mod scheduler;

use crate::cache::{Cache, CacheConfig};
use crate::compression::{AdaptiveCompressor, CompressionLevel};
use crate::error::{EdgeError, Result};
use crate::resource::{HealthStatus, ResourceConstraints, ResourceManager};
use crate::sync::{SyncManager, SyncStrategy};
use futures::Future;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub use executor::Executor;
pub use scheduler::Scheduler;

/// Runtime operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMode {
    /// Online mode with cloud connectivity
    Online,
    /// Offline mode with local processing only
    Offline,
    /// Hybrid mode that adapts based on connectivity
    Hybrid,
}

/// Edge runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeConfig {
    /// Runtime mode
    pub mode: RuntimeMode,
    /// Resource constraints
    pub constraints: ResourceConstraints,
    /// Cache configuration
    pub cache_config: CacheConfig,
    /// Compression level
    pub compression_level: CompressionLevel,
    /// Sync strategy
    pub sync_strategy: SyncStrategy,
    /// Data directory
    pub data_dir: PathBuf,
    /// Enable metrics collection
    pub enable_metrics: bool,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            mode: RuntimeMode::Hybrid,
            constraints: ResourceConstraints::default(),
            cache_config: CacheConfig::default(),
            compression_level: CompressionLevel::Balanced,
            sync_strategy: SyncStrategy::Incremental,
            data_dir: PathBuf::from(".oxigdal_edge"),
            enable_metrics: true,
            heartbeat_interval_secs: 60,
        }
    }
}

impl EdgeConfig {
    /// Create minimal configuration for embedded devices
    pub fn minimal() -> Self {
        Self {
            mode: RuntimeMode::Offline,
            constraints: ResourceConstraints::minimal(),
            cache_config: CacheConfig::minimal(),
            compression_level: CompressionLevel::Fast,
            sync_strategy: SyncStrategy::Manual,
            data_dir: PathBuf::from(".edge_minimal"),
            enable_metrics: false,
            heartbeat_interval_secs: 1, // 1 second for faster tests
        }
    }

    /// Create configuration for offline-first mode
    pub fn offline_first() -> Self {
        Self {
            mode: RuntimeMode::Offline,
            constraints: ResourceConstraints::moderate(),
            cache_config: CacheConfig::offline_first(),
            compression_level: CompressionLevel::Balanced,
            sync_strategy: SyncStrategy::Batch,
            data_dir: PathBuf::from(".edge_offline"),
            enable_metrics: true,
            heartbeat_interval_secs: 5, // 5 seconds for faster tests
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        self.constraints.validate()?;

        if self.heartbeat_interval_secs == 0 {
            return Err(EdgeError::invalid_config(
                "heartbeat_interval_secs must be > 0",
            ));
        }

        Ok(())
    }
}

/// Edge runtime state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Runtime is initializing
    Initializing,
    /// Runtime is running
    Running,
    /// Runtime is paused
    Paused,
    /// Runtime is shutting down
    ShuttingDown,
    /// Runtime is stopped
    Stopped,
}

/// Edge runtime for resource-constrained devices
pub struct EdgeRuntime {
    config: EdgeConfig,
    state: Arc<RwLock<RuntimeState>>,
    resource_manager: Arc<ResourceManager>,
    cache: Arc<Cache>,
    compressor: Arc<AdaptiveCompressor>,
    sync_manager: Option<Arc<SyncManager>>,
    executor: Arc<Executor>,
    scheduler: Arc<Scheduler>,
}

impl EdgeRuntime {
    /// Create new edge runtime
    pub async fn new(config: EdgeConfig) -> Result<Self> {
        config.validate()?;

        // Create data directory
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| EdgeError::runtime(format!("Failed to create data dir: {}", e)))?;

        // Initialize resource manager
        let resource_manager = Arc::new(ResourceManager::new(config.constraints.clone())?);

        // Initialize cache
        let cache = Arc::new(Cache::new(config.cache_config.clone())?);

        // Initialize compressor
        let compressor = Arc::new(AdaptiveCompressor::new(config.compression_level));

        // Initialize sync manager if not in offline mode
        let sync_manager = if config.mode != RuntimeMode::Offline {
            Some(Arc::new(SyncManager::new(
                config.sync_strategy,
                Arc::clone(&cache),
            )?))
        } else {
            None
        };

        // Initialize executor
        let executor = Arc::new(Executor::new(Arc::clone(&resource_manager)));

        // Initialize scheduler
        let scheduler = Arc::new(Scheduler::new(
            Arc::clone(&resource_manager),
            config.heartbeat_interval_secs,
        ));

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(RuntimeState::Initializing)),
            resource_manager,
            cache,
            compressor,
            sync_manager,
            executor,
            scheduler,
        })
    }

    /// Start the runtime
    pub async fn start(&self) -> Result<()> {
        {
            let mut state = self.state.write();
            if *state != RuntimeState::Initializing && *state != RuntimeState::Stopped {
                return Err(EdgeError::runtime("Runtime already started"));
            }
            *state = RuntimeState::Running;
        } // Guard dropped here automatically

        // Start scheduler
        self.scheduler.start().await?;

        // Start sync manager if available
        if let Some(sync_manager) = &self.sync_manager {
            sync_manager.start().await?;
        }

        Ok(())
    }

    /// Stop the runtime
    pub async fn stop(&self) -> Result<()> {
        {
            let mut state = self.state.write();
            *state = RuntimeState::ShuttingDown;
        } // Guard dropped here automatically

        // Stop sync manager
        if let Some(sync_manager) = &self.sync_manager {
            sync_manager.stop().await?;
        }

        // Stop scheduler
        self.scheduler.stop().await?;

        {
            let mut state = self.state.write();
            *state = RuntimeState::Stopped;
        } // Guard dropped here automatically

        Ok(())
    }

    /// Pause the runtime
    pub async fn pause(&self) -> Result<()> {
        let mut state = self.state.write();
        if *state != RuntimeState::Running {
            return Err(EdgeError::runtime("Runtime not running"));
        }

        *state = RuntimeState::Paused;
        Ok(())
    }

    /// Resume the runtime
    pub async fn resume(&self) -> Result<()> {
        let mut state = self.state.write();
        if *state != RuntimeState::Paused {
            return Err(EdgeError::runtime("Runtime not paused"));
        }

        *state = RuntimeState::Running;
        Ok(())
    }

    /// Get current runtime state
    pub fn state(&self) -> RuntimeState {
        *self.state.read()
    }

    /// Execute a task
    pub async fn execute<F, T>(&self, task: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        if *self.state.read() != RuntimeState::Running {
            return Err(EdgeError::runtime("Runtime not running"));
        }

        self.executor.execute(task).await
    }

    /// Get cache reference
    pub fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }

    /// Get compressor reference
    pub fn compressor(&self) -> &Arc<AdaptiveCompressor> {
        &self.compressor
    }

    /// Get resource manager reference
    pub fn resource_manager(&self) -> &Arc<ResourceManager> {
        &self.resource_manager
    }

    /// Get sync manager reference
    pub fn sync_manager(&self) -> Option<&Arc<SyncManager>> {
        self.sync_manager.as_ref()
    }

    /// Check health status
    pub fn health(&self) -> HealthStatus {
        self.resource_manager.health_check()
    }

    /// Get configuration
    pub fn config(&self) -> &EdgeConfig {
        &self.config
    }

    /// Wait for healthy state
    pub async fn wait_healthy(&self, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();

        loop {
            if self.health() == HealthStatus::Healthy {
                return Ok(());
            }

            if start.elapsed() > timeout {
                return Err(EdgeError::timeout("Health check timeout"));
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_creation() {
        let config = EdgeConfig::minimal();
        let runtime = EdgeRuntime::new(config).await;
        assert!(runtime.is_ok());
    }

    #[tokio::test]
    async fn test_runtime_lifecycle() -> Result<()> {
        let config = EdgeConfig::minimal();
        let runtime = EdgeRuntime::new(config).await?;

        assert_eq!(runtime.state(), RuntimeState::Initializing);

        runtime.start().await?;
        assert_eq!(runtime.state(), RuntimeState::Running);

        runtime.pause().await?;
        assert_eq!(runtime.state(), RuntimeState::Paused);

        runtime.resume().await?;
        assert_eq!(runtime.state(), RuntimeState::Running);

        runtime.stop().await?;
        assert_eq!(runtime.state(), RuntimeState::Stopped);

        Ok(())
    }

    #[tokio::test]
    async fn test_runtime_execute() -> Result<()> {
        let config = EdgeConfig::minimal();
        let runtime = EdgeRuntime::new(config).await?;
        runtime.start().await?;

        let result = runtime.execute(async { Ok(42) }).await?;

        assert_eq!(result, 42);

        runtime.stop().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_runtime_health() -> Result<()> {
        let config = EdgeConfig::minimal();
        let runtime = EdgeRuntime::new(config).await?;

        assert_eq!(runtime.health(), HealthStatus::Healthy);

        Ok(())
    }

    #[tokio::test]
    async fn test_config_validation() {
        let mut config = EdgeConfig::minimal();
        config.heartbeat_interval_secs = 0;

        assert!(config.validate().is_err());
    }
}
