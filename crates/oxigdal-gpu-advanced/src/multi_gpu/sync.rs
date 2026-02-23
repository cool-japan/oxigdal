//! Advanced multi-GPU synchronization primitives.
//!
//! This module provides sophisticated synchronization mechanisms for coordinating
//! operations across multiple GPUs including barriers, events, and cross-GPU transfers.

use crate::error::{GpuAdvancedError, Result};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, Semaphore};
use wgpu::{Buffer, Device, Queue};

/// Multi-GPU synchronization manager
#[derive(Clone)]
pub struct SyncManager {
    devices: Arc<Vec<Arc<Device>>>,
    queues: Arc<Vec<Arc<Queue>>>,
    barriers: Arc<RwLock<HashMap<String, Arc<Barrier>>>>,
    events: Arc<RwLock<HashMap<String, Arc<Event>>>>,
    fence_pool: Arc<Mutex<FencePool>>,
}

impl SyncManager {
    /// Create a new synchronization manager
    pub fn new(devices: Vec<Arc<Device>>, queues: Vec<Arc<Queue>>) -> Result<Self> {
        if devices.len() != queues.len() {
            return Err(GpuAdvancedError::InvalidConfiguration(
                "Device and queue count mismatch".to_string(),
            ));
        }

        Ok(Self {
            devices: Arc::new(devices),
            queues: Arc::new(queues),
            barriers: Arc::new(RwLock::new(HashMap::new())),
            events: Arc::new(RwLock::new(HashMap::new())),
            fence_pool: Arc::new(Mutex::new(FencePool::new())),
        })
    }

    /// Create a barrier for N GPUs
    pub fn create_barrier(&self, name: &str, gpu_count: usize) -> Result<Arc<Barrier>> {
        if gpu_count == 0 || gpu_count > self.devices.len() {
            return Err(GpuAdvancedError::ConfigError(format!(
                "Invalid GPU count {} for barrier (available: {})",
                gpu_count,
                self.devices.len()
            )));
        }

        let barrier = Arc::new(Barrier::new(gpu_count));
        self.barriers
            .write()
            .insert(name.to_string(), barrier.clone());
        Ok(barrier)
    }

    /// Get an existing barrier
    pub fn get_barrier(&self, name: &str) -> Option<Arc<Barrier>> {
        self.barriers.read().get(name).cloned()
    }

    /// Create an event for GPU-to-GPU synchronization
    pub fn create_event(&self, name: &str) -> Arc<Event> {
        let event = Arc::new(Event::new());
        self.events.write().insert(name.to_string(), event.clone());
        event
    }

    /// Get an existing event
    pub fn get_event(&self, name: &str) -> Option<Arc<Event>> {
        self.events.read().get(name).cloned()
    }

    /// Transfer data between GPUs
    pub async fn transfer_between_gpus(
        &self,
        src_gpu_idx: usize,
        dst_gpu_idx: usize,
        src_buffer: &Buffer,
        dst_buffer: &Buffer,
        size: u64,
    ) -> Result<Duration> {
        if src_gpu_idx >= self.devices.len() || dst_gpu_idx >= self.devices.len() {
            return Err(GpuAdvancedError::InvalidConfiguration(
                "GPU index out of bounds".to_string(),
            ));
        }

        let start = Instant::now();

        // Create staging buffer on host
        let staging_buffer = self.devices[src_gpu_idx].create_buffer(&wgpu::BufferDescriptor {
            label: Some("cross_gpu_staging"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Copy from source GPU to staging
        let mut encoder =
            self.devices[src_gpu_idx].create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cross_gpu_copy_src"),
            });
        encoder.copy_buffer_to_buffer(src_buffer, 0, &staging_buffer, 0, size);
        self.queues[src_gpu_idx].submit(Some(encoder.finish()));

        // Wait for copy to complete
        let slice = staging_buffer.slice(..);
        let (tx, rx) = futures::channel::oneshot::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        // wgpu 28 polls automatically - no explicit poll needed

        rx.await
            .map_err(|_| GpuAdvancedError::SyncError("Transfer channel closed".to_string()))?
            .map_err(|e| GpuAdvancedError::SyncError(format!("Map async failed: {:?}", e)))?;

        // Read from staging
        let data = slice.get_mapped_range();
        let vec_data: Vec<u8> = data.to_vec();
        drop(data);
        staging_buffer.unmap();

        // Create destination staging buffer
        let dst_staging = self.devices[dst_gpu_idx].create_buffer(&wgpu::BufferDescriptor {
            label: Some("cross_gpu_staging_dst"),
            size,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: true,
        });

        // Write to destination staging
        {
            let mut mapped = dst_staging.slice(..).get_mapped_range_mut();
            mapped.copy_from_slice(&vec_data);
        }
        dst_staging.unmap();

        // Copy from staging to destination GPU
        let mut encoder =
            self.devices[dst_gpu_idx].create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cross_gpu_copy_dst"),
            });
        encoder.copy_buffer_to_buffer(&dst_staging, 0, dst_buffer, 0, size);
        self.queues[dst_gpu_idx].submit(Some(encoder.finish()));

        // wgpu 28 polls automatically - submission completes asynchronously

        Ok(start.elapsed())
    }

    /// Acquire a fence from the pool
    pub fn acquire_fence(&self) -> Fence {
        self.fence_pool.lock().acquire()
    }

    /// Release a fence back to the pool
    pub fn release_fence(&self, fence: Fence) {
        self.fence_pool.lock().release(fence);
    }

    /// Number of available GPUs
    pub fn gpu_count(&self) -> usize {
        self.devices.len()
    }
}

/// Barrier synchronization primitive for multiple GPUs
pub struct Barrier {
    count: usize,
    arrived: Mutex<usize>,
    generation: Mutex<usize>,
    notify: Notify,
}

impl Barrier {
    /// Create a new barrier
    pub fn new(count: usize) -> Self {
        Self {
            count,
            arrived: Mutex::new(0),
            generation: Mutex::new(0),
            notify: Notify::new(),
        }
    }

    /// Wait at the barrier
    pub async fn wait(&self) -> Result<()> {
        let current_gen = *self.generation.lock();

        let arrived = {
            let mut arrived = self.arrived.lock();
            *arrived += 1;
            *arrived
        };

        if arrived == self.count {
            // Last one to arrive, reset and notify all
            {
                let mut arrived = self.arrived.lock();
                *arrived = 0;
            }
            {
                let mut gen_val = self.generation.lock();
                *gen_val += 1;
            }
            self.notify.notify_waiters();
            Ok(())
        } else {
            // Wait for notification
            loop {
                self.notify.notified().await;
                let gen_val = *self.generation.lock();
                if gen_val > current_gen {
                    break;
                }
            }
            Ok(())
        }
    }

    /// Wait with timeout
    pub async fn wait_timeout(&self, timeout: Duration) -> Result<bool> {
        let wait_future = self.wait();
        match tokio::time::timeout(timeout, wait_future).await {
            Ok(Ok(())) => Ok(true),
            Ok(Err(e)) => Err(e),
            Err(_) => Ok(false), // Timeout
        }
    }

    /// Get the barrier count
    pub fn count(&self) -> usize {
        self.count
    }

    /// Get the current number of waiting threads
    pub fn waiting(&self) -> usize {
        *self.arrived.lock()
    }
}

/// Event for GPU-to-GPU signaling
pub struct Event {
    signaled: Mutex<bool>,
    notify: Notify,
    timestamp: Mutex<Option<Instant>>,
}

impl Event {
    /// Create a new event
    pub fn new() -> Self {
        Self {
            signaled: Mutex::new(false),
            notify: Notify::new(),
            timestamp: Mutex::new(None),
        }
    }

    /// Signal the event
    pub fn signal(&self) {
        *self.signaled.lock() = true;
        *self.timestamp.lock() = Some(Instant::now());
        self.notify.notify_waiters();
    }

    /// Reset the event
    pub fn reset(&self) {
        *self.signaled.lock() = false;
        *self.timestamp.lock() = None;
    }

    /// Wait for the event to be signaled
    pub async fn wait(&self) {
        if *self.signaled.lock() {
            return;
        }
        self.notify.notified().await;
    }

    /// Wait with timeout
    pub async fn wait_timeout(&self, timeout: Duration) -> bool {
        if *self.signaled.lock() {
            return true;
        }
        tokio::time::timeout(timeout, self.notify.notified())
            .await
            .is_ok()
    }

    /// Check if the event is signaled
    pub fn is_signaled(&self) -> bool {
        *self.signaled.lock()
    }

    /// Get the timestamp when the event was signaled
    pub fn timestamp(&self) -> Option<Instant> {
        *self.timestamp.lock()
    }
}

impl Default for Event {
    fn default() -> Self {
        Self::new()
    }
}

/// Fence for command buffer synchronization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fence {
    id: u64,
}

impl Fence {
    fn new(id: u64) -> Self {
        Self { id }
    }

    /// Get the fence ID
    pub fn id(&self) -> u64 {
        self.id
    }
}

/// Pool of reusable fences
struct FencePool {
    next_id: u64,
    available: Vec<Fence>,
    max_pool_size: usize,
}

impl FencePool {
    fn new() -> Self {
        Self {
            next_id: 0,
            available: Vec::new(),
            max_pool_size: 256,
        }
    }

    fn acquire(&mut self) -> Fence {
        if let Some(fence) = self.available.pop() {
            fence
        } else {
            let fence = Fence::new(self.next_id);
            self.next_id += 1;
            fence
        }
    }

    fn release(&mut self, fence: Fence) {
        if self.available.len() < self.max_pool_size {
            self.available.push(fence);
        }
    }
}

/// Semaphore for controlling concurrent GPU access
pub struct GpuSemaphore {
    inner: Arc<Semaphore>,
}

impl GpuSemaphore {
    /// Create a new semaphore with the given permit count
    pub fn new(permits: usize) -> Self {
        Self {
            inner: Arc::new(Semaphore::new(permits)),
        }
    }

    /// Acquire a permit
    pub async fn acquire(&self) -> Result<SemaphoreGuard<'_>> {
        let permit =
            self.inner.acquire().await.map_err(|e| {
                GpuAdvancedError::SyncError(format!("Semaphore acquire failed: {}", e))
            })?;
        Ok(SemaphoreGuard { _permit: permit })
    }

    /// Try to acquire a permit without waiting
    pub fn try_acquire(&self) -> Option<SemaphoreGuard<'_>> {
        self.inner
            .try_acquire()
            .ok()
            .map(|permit| SemaphoreGuard { _permit: permit })
    }

    /// Get available permits
    pub fn available_permits(&self) -> usize {
        self.inner.available_permits()
    }
}

impl Clone for GpuSemaphore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// RAII guard for semaphore permit
pub struct SemaphoreGuard<'a> {
    _permit: tokio::sync::SemaphorePermit<'a>,
}

/// Synchronization statistics
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Number of barrier waits
    pub barrier_waits: u64,
    /// Number of event signals
    pub event_signals: u64,
    /// Number of cross-GPU transfers
    pub cross_gpu_transfers: u64,
    /// Total transfer time
    pub total_transfer_time: Duration,
    /// Total bytes transferred
    pub total_bytes_transferred: u64,
}

impl SyncStats {
    /// Calculate average transfer bandwidth in GB/s
    pub fn average_bandwidth_gbs(&self) -> Option<f64> {
        if self.total_transfer_time > Duration::ZERO && self.total_bytes_transferred > 0 {
            let bytes_per_sec =
                self.total_bytes_transferred as f64 / self.total_transfer_time.as_secs_f64();
            Some(bytes_per_sec / 1_000_000_000.0)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_barrier() {
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();

        for i in 0..3 {
            let b = barrier.clone();
            let handle = tokio::spawn(async move {
                println!("Task {} waiting at barrier", i);
                b.wait().await.ok();
                println!("Task {} passed barrier", i);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.ok();
        }

        assert_eq!(barrier.waiting(), 0);
    }

    #[tokio::test]
    async fn test_event() {
        let event = Arc::new(Event::new());
        assert!(!event.is_signaled());

        let e = event.clone();
        let handle = tokio::spawn(async move {
            e.wait().await;
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        event.signal();
        assert!(event.is_signaled());

        handle.await.ok();
    }

    #[tokio::test]
    async fn test_semaphore() {
        let sem = GpuSemaphore::new(2);
        assert_eq!(sem.available_permits(), 2);

        let _guard1 = sem.acquire().await.ok();
        assert_eq!(sem.available_permits(), 1);

        let _guard2 = sem.acquire().await.ok();
        assert_eq!(sem.available_permits(), 0);

        drop(_guard1);
        assert_eq!(sem.available_permits(), 1);
    }

    #[test]
    fn test_fence_pool() {
        let mut pool = FencePool::new();
        let f1 = pool.acquire();
        let f2 = pool.acquire();

        assert_ne!(f1.id(), f2.id());

        pool.release(f1);
        let f3 = pool.acquire();
        assert_eq!(f1, f3);
    }
}
