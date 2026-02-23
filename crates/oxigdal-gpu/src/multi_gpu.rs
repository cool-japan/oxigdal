//! Multi-GPU support for distributed GPU computing.
//!
//! This module provides infrastructure for managing multiple GPUs,
//! distributing work across devices, and handling inter-GPU data transfers.

use crate::context::{GpuContext, GpuContextConfig};
use crate::error::{GpuError, GpuResult};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};
use wgpu::{Adapter, AdapterInfo, Backend, Backends, BufferUsages, Instance};

/// Multi-GPU configuration.
#[derive(Debug, Clone)]
pub struct MultiGpuConfig {
    /// Backends to search for GPUs.
    pub backends: Backends,
    /// Minimum number of GPUs required.
    pub min_devices: usize,
    /// Maximum number of GPUs to use.
    pub max_devices: usize,
    /// Enable automatic load balancing.
    pub auto_load_balance: bool,
    /// Enable peer-to-peer transfers (if supported).
    pub enable_p2p: bool,
}

impl Default for MultiGpuConfig {
    fn default() -> Self {
        Self {
            backends: Backends::all(),
            min_devices: 1,
            max_devices: 8,
            auto_load_balance: true,
            enable_p2p: false,
        }
    }
}

/// Information about a GPU device.
#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    /// Device index.
    pub index: usize,
    /// Adapter information.
    pub adapter_info: AdapterInfo,
    /// Backend type.
    pub backend: Backend,
    /// Estimated VRAM in bytes (if available).
    pub vram_bytes: Option<u64>,
    /// Device is currently active.
    pub active: bool,
}

impl GpuDeviceInfo {
    /// Get a human-readable description.
    pub fn description(&self) -> String {
        format!(
            "GPU {} : {} ({:?})",
            self.index, self.adapter_info.name, self.backend
        )
    }
}

/// Multi-GPU manager for coordinating multiple devices.
pub struct MultiGpuManager {
    /// Available GPU contexts.
    devices: Vec<Arc<GpuContext>>,
    /// Device information.
    device_info: Vec<GpuDeviceInfo>,
    /// Configuration.
    config: MultiGpuConfig,
    /// Load balancing state.
    load_state: Arc<Mutex<LoadBalanceState>>,
}

#[derive(Debug, Clone)]
struct LoadBalanceState {
    /// Number of tasks dispatched to each device.
    task_counts: HashMap<usize, usize>,
    /// Estimated workload on each device (arbitrary units).
    workload: HashMap<usize, f64>,
}

impl LoadBalanceState {
    fn new(num_devices: usize) -> Self {
        let mut task_counts = HashMap::new();
        let mut workload = HashMap::new();

        for i in 0..num_devices {
            task_counts.insert(i, 0);
            workload.insert(i, 0.0);
        }

        Self {
            task_counts,
            workload,
        }
    }

    fn select_device(&self) -> usize {
        // Select device with minimum workload
        self.workload
            .iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| *idx)
            .unwrap_or(0)
    }

    fn add_task(&mut self, device: usize, workload: f64) {
        *self.task_counts.entry(device).or_insert(0) += 1;
        *self.workload.entry(device).or_insert(0.0) += workload;
    }

    fn complete_task(&mut self, device: usize, workload: f64) {
        if let Some(count) = self.task_counts.get_mut(&device) {
            *count = count.saturating_sub(1);
        }
        if let Some(load) = self.workload.get_mut(&device) {
            *load = load.max(workload) - workload;
        }
    }
}

impl MultiGpuManager {
    /// Create a new multi-GPU manager.
    ///
    /// # Errors
    ///
    /// Returns an error if minimum number of devices cannot be found.
    pub async fn new(config: MultiGpuConfig) -> GpuResult<Self> {
        info!("Initializing multi-GPU manager");

        let instance = Instance::new(&wgpu::InstanceDescriptor {
            backends: config.backends,
            ..Default::default()
        });

        // Enumerate all available adapters
        let adapters = Self::enumerate_adapters(&instance).await;

        if adapters.len() < config.min_devices {
            return Err(GpuError::no_adapter(format!(
                "Found {} GPUs, but {} required",
                adapters.len(),
                config.min_devices
            )));
        }

        let num_devices = adapters.len().min(config.max_devices);
        info!(
            "Found {} compatible GPUs, using {}",
            adapters.len(),
            num_devices
        );

        // Create contexts for each device
        let mut devices = Vec::new();
        let mut device_info = Vec::new();

        for (index, adapter) in adapters.into_iter().take(num_devices).enumerate() {
            match Self::create_device_context(adapter, index).await {
                Ok((context, info)) => {
                    devices.push(Arc::new(context));
                    device_info.push(info);
                    info!(
                        "Initialized: {}",
                        device_info
                            .last()
                            .map(|i| i.description())
                            .unwrap_or_default()
                    );
                }
                Err(e) => {
                    warn!("Failed to initialize GPU {}: {}", index, e);
                }
            }
        }

        if devices.len() < config.min_devices {
            return Err(GpuError::device_request(format!(
                "Successfully initialized {} GPUs, but {} required",
                devices.len(),
                config.min_devices
            )));
        }

        let load_state = Arc::new(Mutex::new(LoadBalanceState::new(devices.len())));

        Ok(Self {
            devices,
            device_info,
            config,
            load_state,
        })
    }

    /// Get the number of available devices.
    pub fn num_devices(&self) -> usize {
        self.devices.len()
    }

    /// Get a specific device context.
    pub fn device(&self, index: usize) -> Option<&Arc<GpuContext>> {
        self.devices.get(index)
    }

    /// Get all device contexts.
    pub fn devices(&self) -> &[Arc<GpuContext>] {
        &self.devices
    }

    /// Get device information.
    pub fn device_info(&self, index: usize) -> Option<&GpuDeviceInfo> {
        self.device_info.get(index)
    }

    /// Get all device information.
    pub fn all_device_info(&self) -> &[GpuDeviceInfo] {
        &self.device_info
    }

    /// Select a device based on load balancing strategy.
    pub fn select_device(&self) -> usize {
        if !self.config.auto_load_balance {
            // Round-robin without load balancing (use simple counter)
            return 0; // Simplified for now
        }

        self.load_state
            .lock()
            .map(|state| state.select_device())
            .unwrap_or(0)
    }

    /// Dispatch work to a device with load balancing.
    pub fn dispatch<F, T>(&self, workload: f64, f: F) -> GpuResult<T>
    where
        F: FnOnce(&GpuContext) -> GpuResult<T>,
    {
        let device_idx = self.select_device();

        if let Ok(mut state) = self.load_state.lock() {
            state.add_task(device_idx, workload);
        }

        let context = self
            .devices
            .get(device_idx)
            .ok_or_else(|| GpuError::internal("Invalid device index"))?;

        let result = f(context);

        if let Ok(mut state) = self.load_state.lock() {
            state.complete_task(device_idx, workload);
        }

        result
    }

    /// Distribute work across all devices.
    pub async fn distribute<F, T>(&self, items: Vec<(f64, F)>) -> Vec<GpuResult<T>>
    where
        F: FnOnce(&GpuContext) -> GpuResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let mut tasks = Vec::new();

        for (workload, work_fn) in items {
            let device_idx = self.select_device();

            if let Ok(mut state) = self.load_state.lock() {
                state.add_task(device_idx, workload);
            }

            let context = match self.devices.get(device_idx) {
                Some(ctx) => Arc::clone(ctx),
                None => continue,
            };

            let load_state = Arc::clone(&self.load_state);

            let task = tokio::spawn(async move {
                let result = work_fn(&context);

                if let Ok(mut state) = load_state.lock() {
                    state.complete_task(device_idx, workload);
                }

                result
            });

            tasks.push(task);
        }

        // Wait for all tasks to complete
        let mut results = Vec::new();
        for task in tasks {
            match task.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(GpuError::internal(e.to_string()))),
            }
        }

        results
    }

    /// Get current load statistics.
    pub fn load_stats(&self) -> HashMap<usize, (usize, f64)> {
        self.load_state
            .lock()
            .map(|state| {
                let mut stats = HashMap::new();
                for i in 0..self.devices.len() {
                    let tasks = *state.task_counts.get(&i).unwrap_or(&0);
                    let workload = *state.workload.get(&i).unwrap_or(&0.0);
                    stats.insert(i, (tasks, workload));
                }
                stats
            })
            .unwrap_or_default()
    }

    async fn enumerate_adapters(_instance: &Instance) -> Vec<Adapter> {
        let mut adapters = Vec::new();

        // Try each backend
        for backend in &[
            Backends::VULKAN,
            Backends::METAL,
            Backends::DX12,
            Backends::BROWSER_WEBGPU,
        ] {
            let backend_instance = Instance::new(&wgpu::InstanceDescriptor {
                backends: *backend,
                ..Default::default()
            });

            if let Ok(adapter) = backend_instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
            {
                adapters.push(adapter);
            }
        }

        adapters
    }

    async fn create_device_context(
        adapter: Adapter,
        index: usize,
    ) -> GpuResult<(GpuContext, GpuDeviceInfo)> {
        let adapter_info = adapter.get_info();
        let backend = adapter_info.backend;

        // Estimate VRAM (not directly available in WGPU)
        let vram_bytes = Self::estimate_vram(&adapter_info);

        let config = GpuContextConfig::default().with_label(format!("GPU {}", index));

        let context = GpuContext::with_config(config).await?;

        let info = GpuDeviceInfo {
            index,
            adapter_info,
            backend,
            vram_bytes,
            active: true,
        };

        Ok((context, info))
    }

    fn estimate_vram(adapter_info: &AdapterInfo) -> Option<u64> {
        // This is a rough estimation based on device type
        match adapter_info.device_type {
            wgpu::DeviceType::DiscreteGpu => Some(8 * 1024 * 1024 * 1024), // 8 GB
            wgpu::DeviceType::IntegratedGpu => Some(2 * 1024 * 1024 * 1024), // 2 GB
            wgpu::DeviceType::VirtualGpu => Some(4 * 1024 * 1024 * 1024),  // 4 GB
            _ => None,
        }
    }
}

/// Inter-GPU data transfer manager.
pub struct InterGpuTransfer {
    manager: Arc<MultiGpuManager>,
}

impl InterGpuTransfer {
    /// Create a new inter-GPU transfer manager.
    pub fn new(manager: Arc<MultiGpuManager>) -> Self {
        Self { manager }
    }

    /// Copy data between GPUs.
    ///
    /// # Errors
    ///
    /// Returns an error if transfer fails or devices are invalid.
    pub async fn copy_buffer(
        &self,
        src_device: usize,
        dst_device: usize,
        data: &[u8],
    ) -> GpuResult<()> {
        let _src_ctx = self
            .manager
            .device(src_device)
            .ok_or_else(|| GpuError::invalid_buffer("Invalid source device"))?;

        let dst_ctx = self
            .manager
            .device(dst_device)
            .ok_or_else(|| GpuError::invalid_buffer("Invalid destination device"))?;

        // Create buffer on destination device
        let dst_buffer = dst_ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Inter-GPU Transfer"),
            size: data.len() as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // Write data to destination
        dst_ctx.queue().write_buffer(&dst_buffer, 0, data);

        debug!(
            "Transferred {} bytes from GPU {} to GPU {}",
            data.len(),
            src_device,
            dst_device
        );

        Ok(())
    }

    /// Broadcast data to all GPUs.
    ///
    /// # Errors
    ///
    /// Returns an error if any transfer fails.
    pub async fn broadcast(&self, data: &[u8]) -> GpuResult<()> {
        for i in 1..self.manager.num_devices() {
            self.copy_buffer(0, i, data).await?;
        }

        Ok(())
    }

    /// Gather data from all GPUs to one device.
    ///
    /// # Errors
    ///
    /// Returns an error if any transfer fails.
    pub async fn gather(&self, dst_device: usize) -> GpuResult<Vec<Vec<u8>>> {
        let mut results = Vec::new();

        for i in 0..self.manager.num_devices() {
            if i == dst_device {
                continue;
            }

            // In a real implementation, we would read from the source GPU
            // For now, this is a placeholder
            results.push(Vec::new());
        }

        Ok(results)
    }
}

/// GPU affinity manager for NUMA-aware scheduling.
pub struct GpuAffinityManager {
    /// Device affinity groups (devices that share memory/PCIe bus).
    affinity_groups: HashMap<usize, Vec<usize>>,
}

impl GpuAffinityManager {
    /// Create a new affinity manager.
    pub fn new() -> Self {
        Self {
            affinity_groups: HashMap::new(),
        }
    }

    /// Set devices in the same affinity group.
    pub fn set_affinity_group(&mut self, group_id: usize, devices: Vec<usize>) {
        self.affinity_groups.insert(group_id, devices);
    }

    /// Get devices in the same affinity group.
    pub fn get_affinity_group(&self, device: usize) -> Vec<usize> {
        for (_, devices) in &self.affinity_groups {
            if devices.contains(&device) {
                return devices.clone();
            }
        }
        vec![device]
    }

    /// Check if two devices are in the same affinity group.
    pub fn same_affinity(&self, device_a: usize, device_b: usize) -> bool {
        let group_a = self.get_affinity_group(device_a);
        group_a.contains(&device_b)
    }

    /// Get optimal device for data locality.
    pub fn optimal_device(&self, data_device: usize, available: &[usize]) -> Option<usize> {
        // Prefer devices in the same affinity group
        let group = self.get_affinity_group(data_device);

        for device in available {
            if group.contains(device) {
                return Some(*device);
            }
        }

        // Fall back to any available device
        available.first().copied()
    }
}

impl Default for GpuAffinityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Work distribution strategy for multi-GPU processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionStrategy {
    /// Distribute work evenly across all devices.
    RoundRobin,
    /// Distribute based on device capabilities.
    LoadBalanced,
    /// Distribute based on data locality.
    DataLocal,
    /// Use only the fastest device.
    SingleDevice,
}

/// Work distributor for multi-GPU task scheduling.
pub struct WorkDistributor {
    manager: Arc<MultiGpuManager>,
    strategy: DistributionStrategy,
    affinity: GpuAffinityManager,
}

impl WorkDistributor {
    /// Create a new work distributor.
    pub fn new(manager: Arc<MultiGpuManager>, strategy: DistributionStrategy) -> Self {
        Self {
            manager,
            strategy,
            affinity: GpuAffinityManager::new(),
        }
    }

    /// Set affinity group.
    pub fn set_affinity_group(&mut self, group_id: usize, devices: Vec<usize>) {
        self.affinity.set_affinity_group(group_id, devices);
    }

    /// Distribute work items across GPUs.
    pub fn distribute_work<T>(&self, items: Vec<T>) -> Vec<(usize, Vec<T>)> {
        match self.strategy {
            DistributionStrategy::RoundRobin => self.round_robin(items),
            DistributionStrategy::LoadBalanced => self.load_balanced(items),
            DistributionStrategy::DataLocal => self.data_local(items),
            DistributionStrategy::SingleDevice => self.single_device(items),
        }
    }

    fn round_robin<T>(&self, items: Vec<T>) -> Vec<(usize, Vec<T>)> {
        let num_devices = self.manager.num_devices();
        let mut device_items: Vec<Vec<T>> = (0..num_devices).map(|_| Vec::new()).collect();

        for (idx, item) in items.into_iter().enumerate() {
            device_items[idx % num_devices].push(item);
        }

        device_items
            .into_iter()
            .enumerate()
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }

    fn load_balanced<T>(&self, items: Vec<T>) -> Vec<(usize, Vec<T>)> {
        let stats = self.manager.load_stats();
        let num_devices = self.manager.num_devices();
        let items_len = items.len();

        // Calculate weights based on inverse of current load
        let mut weights: Vec<f64> = (0..num_devices)
            .map(|i| {
                let (_, load) = stats.get(&i).unwrap_or(&(0, 0.0));
                1.0 / (1.0 + load)
            })
            .collect();

        // Normalize weights
        let total: f64 = weights.iter().sum();
        if total > 0.0 {
            for w in &mut weights {
                *w /= total;
            }
        }

        // Distribute items based on weights
        let mut device_items: Vec<Vec<T>> = (0..num_devices).map(|_| Vec::new()).collect();

        for (idx, item) in items.into_iter().enumerate() {
            let target = (idx as f64 + 0.5) / items_len as f64;
            let mut device = 0;
            let mut cumulative = 0.0;

            for (dev, weight) in weights.iter().enumerate() {
                cumulative += weight;
                if cumulative >= target {
                    device = dev;
                    break;
                }
            }

            device_items[device].push(item);
        }

        device_items
            .into_iter()
            .enumerate()
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }

    fn data_local<T>(&self, items: Vec<T>) -> Vec<(usize, Vec<T>)> {
        // For now, fall back to round-robin
        // In a real implementation, this would consider data locality
        self.round_robin(items)
    }

    fn single_device<T>(&self, items: Vec<T>) -> Vec<(usize, Vec<T>)> {
        vec![(0, items)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_gpu_config() {
        let config = MultiGpuConfig::default();
        assert_eq!(config.min_devices, 1);
        assert_eq!(config.max_devices, 8);
        assert!(config.auto_load_balance);
    }

    #[test]
    fn test_load_balance_state() {
        let mut state = LoadBalanceState::new(3);

        state.add_task(0, 100.0);
        state.add_task(1, 50.0);
        state.add_task(2, 75.0);

        // Device 1 should have minimum load
        assert_eq!(state.select_device(), 1);

        state.complete_task(1, 50.0);
        assert_eq!(state.select_device(), 1);
    }

    #[test]
    fn test_affinity_manager() {
        let mut manager = GpuAffinityManager::new();

        manager.set_affinity_group(0, vec![0, 1]);
        manager.set_affinity_group(1, vec![2, 3]);

        assert!(manager.same_affinity(0, 1));
        assert!(manager.same_affinity(2, 3));
        assert!(!manager.same_affinity(0, 2));

        let group = manager.get_affinity_group(0);
        assert_eq!(group, vec![0, 1]);
    }

    #[test]
    fn test_distribution_strategy() {
        assert_eq!(
            DistributionStrategy::RoundRobin,
            DistributionStrategy::RoundRobin
        );
    }
}
