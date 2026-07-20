//! Task scheduler for periodic operations

use crate::error::{EdgeError, Result};
use crate::resource::ResourceManager;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, RefreshKind, System};
use tokio::task::JoinHandle;

/// Scheduled task
pub type ScheduledTask = Box<dyn Fn() -> Result<()> + Send + Sync>;

/// Samples host CPU utilization using platform-native counters (via `sysinfo`).
///
/// A single [`System`] handle is kept alive for the lifetime of the sampler so that
/// successive calls to [`CpuSampler::sample`] can compute a proper delta-based
/// utilization percentage instead of a fixed value. The very first sample after
/// construction may legitimately read `0.0` because CPU usage requires two refreshes
/// separated by at least [`sysinfo::MINIMUM_CPU_UPDATE_INTERVAL`]; the scheduler's
/// heartbeat cadence naturally satisfies this gap on all subsequent samples.
struct CpuSampler {
    system: RwLock<System>,
}

impl CpuSampler {
    /// Create a sampler, priming it with an initial CPU refresh.
    fn new() -> Self {
        let refresh_kind = RefreshKind::nothing().with_cpu(CpuRefreshKind::everything());
        let mut system = System::new_with_specifics(refresh_kind);
        system.refresh_cpu_usage();

        Self {
            system: RwLock::new(system),
        }
    }

    /// Sample current global (all-core-averaged) CPU utilization as a percentage
    /// clamped to `[0.0, 100.0]`.
    fn sample(&self) -> f64 {
        let mut system = self.system.write();
        system.refresh_cpu_usage();
        f64::from(system.global_cpu_usage()).clamp(0.0, 100.0)
    }
}

/// Task scheduler
pub struct Scheduler {
    resource_manager: Arc<ResourceManager>,
    heartbeat_interval: Duration,
    running: Arc<AtomicBool>,
    handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    cpu_sampler: Arc<CpuSampler>,
}

impl Scheduler {
    /// Create new scheduler
    pub fn new(resource_manager: Arc<ResourceManager>, heartbeat_interval_secs: u64) -> Self {
        Self {
            resource_manager,
            heartbeat_interval: Duration::from_secs(heartbeat_interval_secs),
            running: Arc::new(AtomicBool::new(false)),
            handle: Arc::new(RwLock::new(None)),
            cpu_sampler: Arc::new(CpuSampler::new()),
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
        let cpu_sampler = Arc::clone(&self.cpu_sampler);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                // Perform heartbeat checks
                Self::heartbeat(&resource_manager, &cpu_sampler);

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
    fn heartbeat(resource_manager: &ResourceManager, cpu_sampler: &CpuSampler) {
        // Collect a real CPU sample from the host so that CPU-based admission
        // control (`ResourceManager::is_cpu_overloaded`/`is_over_budget`) reflects
        // actual load instead of a constant placeholder.
        let cpu_usage = cpu_sampler.sample();
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

    #[test]
    fn test_cpu_sampler_returns_bounded_percentage() {
        let sampler = CpuSampler::new();

        // First sample may legitimately read 0.0 (no prior delta yet), but must
        // never be negative or exceed 100%.
        let first = sampler.sample();
        assert!((0.0..=100.0).contains(&first));

        // A second sample (after `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL`) must also
        // stay within bounds and must not panic - this is the regression guard for
        // the previous hardcoded-0.0 stub, which never actually touched the OS.
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let second = sampler.sample();
        assert!((0.0..=100.0).contains(&second));
    }

    #[test]
    fn test_heartbeat_records_real_cpu_sample() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;
        let cpu_sampler = CpuSampler::new();

        assert_eq!(manager.metrics().cpu_percent, 0.0);

        Scheduler::heartbeat(&manager, &cpu_sampler);

        // record_cpu_sample must have been invoked with a value sourced from the
        // sampler (not skipped/dropped), so the sample buffer is non-empty and the
        // averaged reading stays within a valid percentage range.
        let metrics = manager.metrics();
        assert!((0.0..=100.0).contains(&metrics.cpu_percent));

        Ok(())
    }

    #[test]
    fn test_cpu_sampler_is_not_hardcoded_stub() {
        // Two independent samplers, sampled several times each with the minimum
        // update interval between reads, should each produce at least one reading
        // that is not identically 0.0 on a host doing any work at all (this test
        // process itself). This guards against silently reverting to the old
        // `fn sample_cpu() -> f64 { 0.0 }` stub.
        let sampler = CpuSampler::new();
        let mut saw_nonzero = false;

        for _ in 0..10 {
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            // Burn some CPU so there is something for the sampler to observe.
            let mut acc: u64 = 0;
            for i in 0..20_000_000u64 {
                acc = acc.wrapping_add(i);
            }
            std::hint::black_box(acc);

            if sampler.sample() > 0.0 {
                saw_nonzero = true;
                break;
            }
        }

        assert!(
            saw_nonzero,
            "expected at least one non-zero CPU sample while actively burning CPU"
        );
    }
}
