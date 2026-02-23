//! Task scheduler for periodic operations

use crate::error::{EdgeError, Result};
use crate::resource::ResourceManager;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;

/// Scheduled task
pub type ScheduledTask = Box<dyn Fn() -> Result<()> + Send + Sync>;

/// Task scheduler
pub struct Scheduler {
    resource_manager: Arc<ResourceManager>,
    heartbeat_interval: Duration,
    running: Arc<AtomicBool>,
    handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl Scheduler {
    /// Create new scheduler
    pub fn new(resource_manager: Arc<ResourceManager>, heartbeat_interval_secs: u64) -> Self {
        Self {
            resource_manager,
            heartbeat_interval: Duration::from_secs(heartbeat_interval_secs),
            running: Arc::new(AtomicBool::new(false)),
            handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the scheduler
    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err(EdgeError::runtime("Scheduler already running"));
        }

        self.running.store(true, Ordering::Relaxed);

        let resource_manager = Arc::clone(&self.resource_manager);
        let heartbeat_interval = self.heartbeat_interval;
        let running = Arc::clone(&self.running);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                // Perform heartbeat checks
                Self::heartbeat(&resource_manager);

                tokio::time::sleep(heartbeat_interval).await;
            }
        });

        let mut handle_lock = self.handle.write();
        *handle_lock = Some(handle);

        Ok(())
    }

    /// Stop the scheduler
    pub async fn stop(&self) -> Result<()> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(false, Ordering::Relaxed);

        // Wait for handle to complete with timeout
        let handle = {
            let mut handle_lock = self.handle.write();
            handle_lock.take()
        };

        if let Some(handle) = handle {
            let timeout_duration = Duration::from_secs(5);
            match tokio::time::timeout(timeout_duration, handle).await {
                Ok(_) => {}
                Err(_) => {
                    tracing::warn!("Scheduler stop timed out after {:?}", timeout_duration);
                }
            }
        }

        Ok(())
    }

    /// Heartbeat function
    fn heartbeat(resource_manager: &ResourceManager) {
        // Collect CPU sample (simplified - in real implementation would use sysinfo)
        let cpu_usage = Self::sample_cpu();
        resource_manager.record_cpu_sample(cpu_usage);

        // Log metrics
        let metrics = resource_manager.metrics();
        tracing::debug!(
            memory_bytes = metrics.memory_bytes,
            cpu_percent = metrics.cpu_percent,
            active_ops = metrics.active_operations,
            "Heartbeat"
        );
    }

    /// Sample CPU usage (simplified)
    fn sample_cpu() -> f64 {
        // In a real implementation, this would use platform-specific APIs
        // For now, return a mock value
        0.0
    }

    /// Check if scheduler is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceConstraints;

    #[tokio::test]
    async fn test_scheduler_lifecycle() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = Arc::new(ResourceManager::new(constraints)?);
        let scheduler = Scheduler::new(manager, 1);

        assert!(!scheduler.is_running());

        scheduler.start().await?;
        assert!(scheduler.is_running());

        tokio::time::sleep(Duration::from_millis(100)).await;

        scheduler.stop().await?;
        assert!(!scheduler.is_running());

        Ok(())
    }

    #[tokio::test]
    async fn test_scheduler_heartbeat() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = Arc::new(ResourceManager::new(constraints)?);
        let scheduler = Scheduler::new(manager, 1);

        scheduler.start().await?;
        tokio::time::sleep(Duration::from_millis(250)).await; // 250ms is enough for test
        scheduler.stop().await?;

        Ok(())
    }
}
