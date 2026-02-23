//! GPU affinity and pinning management.

use super::GpuDevice;
use dashmap::DashMap;
use std::sync::Arc;
use std::thread::ThreadId;

/// GPU affinity manager for thread-GPU binding
pub struct AffinityManager {
    /// Thread to GPU mapping
    thread_gpu_map: DashMap<ThreadId, usize>,
    /// GPU to threads mapping
    gpu_threads_map: DashMap<usize, Vec<ThreadId>>,
    /// Available GPU devices
    devices: Vec<Arc<GpuDevice>>,
}

impl AffinityManager {
    /// Create a new affinity manager
    pub fn new(devices: Vec<Arc<GpuDevice>>) -> Self {
        Self {
            thread_gpu_map: DashMap::new(),
            gpu_threads_map: DashMap::new(),
            devices,
        }
    }

    /// Pin current thread to a specific GPU
    pub fn pin_thread(&self, gpu_index: usize) -> Result<(), String> {
        if gpu_index >= self.devices.len() {
            return Err(format!(
                "Invalid GPU index: {} (total: {})",
                gpu_index,
                self.devices.len()
            ));
        }

        let thread_id = std::thread::current().id();

        // Update thread->GPU mapping
        self.thread_gpu_map.insert(thread_id, gpu_index);

        // Update GPU->threads mapping
        self.gpu_threads_map
            .entry(gpu_index)
            .or_default()
            .push(thread_id);

        Ok(())
    }

    /// Unpin current thread
    pub fn unpin_thread(&self) {
        let thread_id = std::thread::current().id();

        if let Some((_, gpu_index)) = self.thread_gpu_map.remove(&thread_id) {
            // Remove thread from GPU->threads mapping
            if let Some(mut threads) = self.gpu_threads_map.get_mut(&gpu_index) {
                threads.retain(|&tid| tid != thread_id);
            }
        }
    }

    /// Get GPU index for current thread (if pinned)
    pub fn get_thread_gpu(&self) -> Option<usize> {
        let thread_id = std::thread::current().id();
        self.thread_gpu_map.get(&thread_id).map(|v| *v)
    }

    /// Get device for current thread (if pinned)
    pub fn get_thread_device(&self) -> Option<Arc<GpuDevice>> {
        self.get_thread_gpu()
            .and_then(|idx| self.devices.get(idx).cloned())
    }

    /// Get all threads pinned to a GPU
    pub fn get_gpu_threads(&self, gpu_index: usize) -> Vec<ThreadId> {
        self.gpu_threads_map
            .get(&gpu_index)
            .map(|threads| threads.clone())
            .unwrap_or_default()
    }

    /// Auto-pin current thread to least loaded GPU
    pub fn auto_pin_thread(&self) -> Result<usize, String> {
        if self.devices.is_empty() {
            return Err("No GPU devices available".to_string());
        }

        // Find GPU with fewest pinned threads
        let mut min_threads = usize::MAX;
        let mut best_gpu = 0;

        for i in 0..self.devices.len() {
            let thread_count = self
                .gpu_threads_map
                .get(&i)
                .map(|threads| threads.len())
                .unwrap_or(0);

            if thread_count < min_threads {
                min_threads = thread_count;
                best_gpu = i;
            }
        }

        self.pin_thread(best_gpu)?;
        Ok(best_gpu)
    }

    /// Get affinity statistics
    pub fn get_stats(&self) -> AffinityStats {
        let mut threads_per_gpu = vec![0; self.devices.len()];
        let mut total_threads = 0;

        for entry in self.gpu_threads_map.iter() {
            let gpu_index = *entry.key();
            let threads = entry.value();
            let count = threads.len();
            if let Some(slot) = threads_per_gpu.get_mut(gpu_index) {
                *slot = count;
            }
            total_threads += count;
        }

        AffinityStats {
            threads_per_gpu,
            total_threads,
            total_gpus: self.devices.len(),
        }
    }

    /// Print affinity information
    pub fn print_affinity_info(&self) {
        let stats = self.get_stats();
        println!("\nGPU Affinity Information:");
        println!("  Total GPUs: {}", stats.total_gpus);
        println!("  Total pinned threads: {}", stats.total_threads);

        for (gpu_index, thread_count) in stats.threads_per_gpu.iter().enumerate() {
            if let Some(device) = self.devices.get(gpu_index) {
                println!(
                    "  GPU {}: {} - {} thread(s) pinned",
                    gpu_index, device.info.name, thread_count
                );
            }
        }
    }

    /// Clear all affinity mappings
    pub fn clear_all(&self) {
        self.thread_gpu_map.clear();
        self.gpu_threads_map.clear();
    }

    /// Get total number of devices
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

/// Affinity statistics
#[derive(Debug, Clone)]
pub struct AffinityStats {
    /// Number of threads per GPU
    pub threads_per_gpu: Vec<usize>,
    /// Total number of pinned threads
    pub total_threads: usize,
    /// Total number of GPUs
    pub total_gpus: usize,
}

impl AffinityStats {
    /// Get average threads per GPU
    pub fn avg_threads_per_gpu(&self) -> f64 {
        if self.total_gpus == 0 {
            0.0
        } else {
            (self.total_threads as f64) / (self.total_gpus as f64)
        }
    }

    /// Get load balance factor (0.0 = perfect balance, 1.0 = worst imbalance)
    pub fn load_balance_factor(&self) -> f64 {
        if self.total_gpus == 0 || self.total_threads == 0 {
            return 0.0;
        }

        let avg = self.avg_threads_per_gpu();
        let variance: f64 = self
            .threads_per_gpu
            .iter()
            .map(|&count| {
                let diff = (count as f64) - avg;
                diff * diff
            })
            .sum::<f64>()
            / (self.total_gpus as f64);

        let std_dev = variance.sqrt();
        std_dev / (avg + 1.0) // Add 1.0 to avoid division by zero
    }
}

/// RAII guard for automatic thread unpinning
pub struct AffinityGuard<'a> {
    manager: &'a AffinityManager,
}

impl<'a> AffinityGuard<'a> {
    /// Create a new affinity guard
    pub fn new(manager: &'a AffinityManager, gpu_index: usize) -> Result<Self, String> {
        manager.pin_thread(gpu_index)?;
        Ok(Self { manager })
    }

    /// Auto-pin to best available GPU
    pub fn auto(manager: &'a AffinityManager) -> Result<Self, String> {
        manager.auto_pin_thread()?;
        Ok(Self { manager })
    }
}

impl Drop for AffinityGuard<'_> {
    fn drop(&mut self) {
        self.manager.unpin_thread();
    }
}

/// Thread pool with GPU affinity
pub struct AffinityThreadPool {
    /// Affinity manager
    affinity: Arc<AffinityManager>,
    /// Thread handles
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl AffinityThreadPool {
    /// Create a new thread pool with GPU affinity
    pub fn new(affinity: Arc<AffinityManager>, threads_per_gpu: usize) -> Self {
        let mut handles = Vec::new();
        let gpu_count = affinity.device_count();

        for gpu_index in 0..gpu_count {
            for _ in 0..threads_per_gpu {
                let affinity_clone = affinity.clone();

                let handle = std::thread::spawn(move || {
                    // Pin thread to GPU
                    if let Err(e) = affinity_clone.pin_thread(gpu_index) {
                        eprintln!("Failed to pin thread to GPU {}: {}", gpu_index, e);
                        return;
                    }

                    // Thread work loop would go here
                    // For now, just demonstrate the pinning
                    tracing::info!("Thread pinned to GPU {}", gpu_index);
                });

                handles.push(handle);
            }
        }

        Self { affinity, handles }
    }

    /// Wait for all threads to complete
    pub fn join(self) {
        for handle in self.handles {
            let _ = handle.join();
        }
    }

    /// Get affinity manager
    pub fn affinity(&self) -> Arc<AffinityManager> {
        self.affinity.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_affinity_stats() {
        let stats = AffinityStats {
            threads_per_gpu: vec![2, 2, 2],
            total_threads: 6,
            total_gpus: 3,
        };

        assert_eq!(stats.avg_threads_per_gpu(), 2.0);
        assert!(stats.load_balance_factor() < 0.01); // Perfect balance
    }

    #[test]
    fn test_affinity_stats_imbalance() {
        let stats = AffinityStats {
            threads_per_gpu: vec![5, 1, 0],
            total_threads: 6,
            total_gpus: 3,
        };

        assert_eq!(stats.avg_threads_per_gpu(), 2.0);
        assert!(stats.load_balance_factor() > 0.5); // Significant imbalance
    }
}
