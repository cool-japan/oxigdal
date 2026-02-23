//! Work queue for GPU task management.

use super::GpuDevice;
use crate::error::{GpuAdvancedError, Result};
use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

/// Work item for GPU execution
type WorkItem = Box<dyn FnOnce(&GpuDevice) -> Result<()> + Send>;

/// Result sender for work completion
type ResultSender = Sender<Result<()>>;

/// Work queue for a single GPU device
pub struct WorkQueue {
    /// Associated GPU device
    device: Arc<GpuDevice>,
    /// Work sender (wrapped in Option for dropping in Drop impl)
    work_sender: Option<Sender<(WorkItem, ResultSender)>>,
    /// Worker thread handle
    worker_handle: Option<Arc<Mutex<Option<thread::JoinHandle<()>>>>>,
    /// Number of pending tasks
    pending_tasks: Arc<Mutex<usize>>,
}

impl WorkQueue {
    /// Create a new work queue
    pub fn new(device: Arc<GpuDevice>) -> Self {
        let (work_sender, work_receiver) = bounded::<(WorkItem, ResultSender)>(256);
        let device_clone = device.clone();
        let pending_tasks = Arc::new(Mutex::new(0));
        let pending_clone = pending_tasks.clone();

        // Spawn worker thread
        let handle = thread::spawn(move || {
            Self::worker_loop(device_clone, work_receiver, pending_clone);
        });

        Self {
            device,
            work_sender: Some(work_sender),
            worker_handle: Some(Arc::new(Mutex::new(Some(handle)))),
            pending_tasks,
        }
    }

    /// Worker loop for processing tasks
    fn worker_loop(
        device: Arc<GpuDevice>,
        work_receiver: Receiver<(WorkItem, ResultSender)>,
        pending_tasks: Arc<Mutex<usize>>,
    ) {
        while let Ok((work, result_sender)) = work_receiver.recv() {
            let start = Instant::now();

            // Update workload
            device.set_workload(1.0);

            // Execute work
            let result = work(&device);

            // Update workload
            device.set_workload(0.0);

            // Send result
            let _ = result_sender.send(result);

            // Update pending tasks
            {
                let mut pending = pending_tasks.lock();
                *pending = pending.saturating_sub(1);
            }

            let duration = start.elapsed();
            tracing::debug!(
                "Task completed on GPU {} in {:?}",
                device.info.index,
                duration
            );
        }
    }

    /// Submit work to the queue
    pub async fn submit_work<F, T>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&GpuDevice) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let (result_sender, result_receiver) = bounded(1);
        let result_arc = Arc::new(Mutex::new(None));
        let result_clone = result_arc.clone();

        // Wrap work to capture result
        let work_wrapper: WorkItem = Box::new(move |device| {
            let result = work(device);
            match result {
                Ok(value) => {
                    *result_clone.lock() = Some(Ok(value));
                    Ok(())
                }
                Err(e) => {
                    *result_clone.lock() = Some(Err(e));
                    Ok(())
                }
            }
        });

        // Update pending tasks
        {
            let mut pending = self.pending_tasks.lock();
            *pending = pending.saturating_add(1);
        }

        // Send work
        self.work_sender
            .as_ref()
            .ok_or_else(|| GpuAdvancedError::WorkStealingError("Work queue is closed".to_string()))?
            .send((work_wrapper, result_sender))
            .map_err(|e| {
                GpuAdvancedError::WorkStealingError(format!("Failed to send work: {}", e))
            })?;

        // Wait for completion
        let _ = result_receiver
            .recv()
            .map_err(|e| GpuAdvancedError::SyncError(format!("Failed to receive result: {}", e)))?;

        // Extract result
        result_arc
            .lock()
            .take()
            .ok_or_else(|| GpuAdvancedError::SyncError("Result not available".to_string()))?
    }

    /// Get number of pending tasks
    pub fn pending_count(&self) -> usize {
        *self.pending_tasks.lock()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.pending_count() == 0
    }

    /// Get associated device
    pub fn device(&self) -> Arc<GpuDevice> {
        self.device.clone()
    }
}

impl Drop for WorkQueue {
    fn drop(&mut self) {
        // Close the channel by dropping the sender
        // This will cause the worker thread's recv() to return Err,
        // allowing it to exit cleanly
        drop(self.work_sender.take());

        // Wait for worker thread to finish
        if let Some(handle_arc) = self.worker_handle.take() {
            if let Some(handle) = handle_arc.lock().take() {
                let _ = handle.join();
            }
        }
    }
}

/// Work stealing queue for load balancing
pub struct WorkStealingQueue {
    /// Local work queue
    local_queue: Arc<Mutex<Vec<WorkItem>>>,
    /// Steal threshold (steal if more than this many items)
    steal_threshold: usize,
}

impl WorkStealingQueue {
    /// Create a new work stealing queue
    pub fn new(steal_threshold: usize) -> Self {
        Self {
            local_queue: Arc::new(Mutex::new(Vec::new())),
            steal_threshold,
        }
    }

    /// Push work to local queue
    pub fn push(&self, work: WorkItem) {
        let mut queue = self.local_queue.lock();
        queue.push(work);
    }

    /// Pop work from local queue
    pub fn pop(&self) -> Option<WorkItem> {
        let mut queue = self.local_queue.lock();
        queue.pop()
    }

    /// Steal work from this queue (take half if above threshold)
    pub fn steal(&self) -> Vec<WorkItem> {
        let mut queue = self.local_queue.lock();
        let len = queue.len();

        if len <= self.steal_threshold {
            return Vec::new();
        }

        let steal_count = len / 2;
        let split_point = len - steal_count;
        queue.split_off(split_point)
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.local_queue.lock().len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if stealing should be allowed
    pub fn should_allow_stealing(&self) -> bool {
        self.len() > self.steal_threshold
    }
}

/// Batch work submitter for efficient multi-GPU processing
pub struct BatchSubmitter {
    /// Work queues for each device
    queues: Vec<Arc<WorkQueue>>,
    /// Current queue index for round-robin
    current_index: Mutex<usize>,
}

impl BatchSubmitter {
    /// Create a new batch submitter
    pub fn new(queues: Vec<Arc<WorkQueue>>) -> Self {
        Self {
            queues,
            current_index: Mutex::new(0),
        }
    }

    /// Submit batch of work items
    pub async fn submit_batch<F, T>(&self, work_items: Vec<F>) -> Result<Vec<T>>
    where
        F: FnOnce(&GpuDevice) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        if self.queues.is_empty() {
            return Err(GpuAdvancedError::WorkStealingError(
                "No work queues available".to_string(),
            ));
        }

        let mut futures = Vec::new();

        for work in work_items {
            // Select queue in round-robin fashion
            let queue_index = {
                let mut index = self.current_index.lock();
                let current = *index;
                *index = (*index + 1) % self.queues.len();
                current
            };

            let queue = &self.queues[queue_index];
            let future = queue.submit_work(work);
            futures.push(future);
        }

        // Wait for all to complete
        let mut results = Vec::new();
        for future in futures {
            results.push(future.await?);
        }

        Ok(results)
    }

    /// Submit batch with explicit device assignment
    pub async fn submit_batch_to_devices<F, T>(&self, work_items: Vec<(usize, F)>) -> Result<Vec<T>>
    where
        F: FnOnce(&GpuDevice) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let mut futures = Vec::new();

        for (device_index, work) in work_items {
            let queue = self
                .queues
                .get(device_index)
                .ok_or(GpuAdvancedError::InvalidGpuIndex {
                    index: device_index,
                    total: self.queues.len(),
                })?;

            let future = queue.submit_work(work);
            futures.push(future);
        }

        // Wait for all to complete
        let mut results = Vec::new();
        for future in futures {
            results.push(future.await?);
        }

        Ok(results)
    }

    /// Get total pending tasks across all queues
    pub fn total_pending(&self) -> usize {
        self.queues.iter().map(|q| q.pending_count()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_stealing_queue() {
        let queue = WorkStealingQueue::new(10);
        assert!(queue.is_empty());

        let work: WorkItem = Box::new(|_device| Ok(()));
        queue.push(work);
        assert_eq!(queue.len(), 1);

        let popped = queue.pop();
        assert!(popped.is_some());
        assert!(queue.is_empty());
    }

    #[test]
    fn test_work_stealing_threshold() {
        let queue = WorkStealingQueue::new(5);

        // Add items below threshold
        for _ in 0..4 {
            queue.push(Box::new(|_device| Ok(())));
        }
        assert!(!queue.should_allow_stealing());

        // Add items above threshold
        for _ in 0..3 {
            queue.push(Box::new(|_device| Ok(())));
        }
        assert!(queue.should_allow_stealing());
    }
}
