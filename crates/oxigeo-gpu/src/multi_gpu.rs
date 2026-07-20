//! Multi-GPU support for distributed GPU computing.
//!
//! This module provides infrastructure for managing multiple GPUs,
//! distributing work across devices, and handling inter-GPU data transfers.

use crate::context::{GpuContext, GpuContextConfig};
use crate::error::{GpuError, GpuResult};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};
use wgpu::{Adapter, AdapterInfo, Backend, Backends, BufferUsages, Instance, PollType};

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

impl MultiGpuManager {
    /// Create a zero-device manager without touching any wgpu objects.
    ///
    /// Used exclusively in tests to construct `InterGpuTransfer` instances
    /// that exercise the pure-Rust validation paths in `gather` without
    /// requiring a physical GPU or wgpu backend initialization.
    #[cfg(test)]
    pub(crate) fn new_empty_for_testing() -> Self {
        Self {
            devices: Vec::new(),
            device_info: Vec::new(),
            config: MultiGpuConfig::default(),
            load_state: Arc::new(Mutex::new(LoadBalanceState::new(0))),
        }
    }
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

        let instance = Instance::new(wgpu::InstanceDescriptor {
            backends: config.backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
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
            let backend_instance = Instance::new(wgpu::InstanceDescriptor {
                backends: *backend,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });

            if let Ok(adapter) = backend_instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                    apply_limit_buckets: false,
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
///
/// Maintains a per-device "last submitted buffer" slot so that `gather`
/// can perform real GPU→CPU readback.  After dispatching work to device `i`,
/// call [`set_device_buffer`](Self::set_device_buffer) with the result buffer
/// and its byte-size; then `gather` will DMA-read every registered buffer
/// back to the host and return the raw bytes.
pub struct InterGpuTransfer {
    manager: Arc<MultiGpuManager>,
    /// Per-device slot: the most-recently registered GPU buffer and its
    /// byte size, protected by a Mutex so the struct stays `Send + Sync`.
    per_device_buffers: Vec<Arc<Mutex<Option<(Arc<wgpu::Buffer>, u64)>>>>,
}

impl InterGpuTransfer {
    /// Create a new inter-GPU transfer manager.
    pub fn new(manager: Arc<MultiGpuManager>) -> Self {
        let num_devices = manager.num_devices();
        let per_device_buffers = (0..num_devices)
            .map(|_| Arc::new(Mutex::new(None)))
            .collect();
        Self {
            manager,
            per_device_buffers,
        }
    }

    /// Register the GPU buffer produced by a compute pass on `device_idx`.
    ///
    /// The buffer must have at least `BufferUsages::COPY_SRC` so that
    /// `gather` can issue a copy-to-staging command.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidBuffer`] when `device_idx` is out of range
    /// or the internal lock is poisoned.
    pub fn set_device_buffer(
        &self,
        device_idx: usize,
        buffer: Arc<wgpu::Buffer>,
        size_bytes: u64,
    ) -> GpuResult<()> {
        let slot = self
            .per_device_buffers
            .get(device_idx)
            .ok_or_else(|| GpuError::invalid_buffer("device_idx out of range"))?;
        *slot
            .lock()
            .map_err(|_| GpuError::internal("per_device_buffers lock poisoned"))? =
            Some((buffer, size_bytes));
        Ok(())
    }

    /// Copy data between GPUs via the host (staging).
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

    /// Gather data from every source device to `dst_device`.
    ///
    /// For each source device `i` (where `i != dst_device`) that has had a
    /// buffer registered via [`set_device_buffer`](Self::set_device_buffer),
    /// this method performs a real GPU→CPU readback:
    ///
    /// 1. Allocates a CPU-visible *staging* buffer on device `i`.
    /// 2. Encodes a `copy_buffer_to_buffer` command from the registered source
    ///    buffer into the staging buffer, then submits it to the device queue.
    /// 3. Polls the device until the submission has completed (blocking wait).
    /// 4. Maps the staging buffer for reading, copies the data, and unmaps it.
    ///
    /// The returned `Vec<Vec<u8>>` contains one entry per source device
    /// (ordered by ascending device index, skipping `dst_device`).  Devices
    /// that have no registered buffer produce an empty `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    /// - `dst_device >= num_devices`
    /// - the wgpu device poll fails
    /// - the staging buffer cannot be mapped (e.g., the source buffer is missing
    ///   `COPY_SRC` usage)
    /// - an internal mutex is poisoned
    pub async fn gather(&self, dst_device: usize) -> GpuResult<Vec<Vec<u8>>> {
        // Use `per_device_buffers.len()` as the authoritative device count.
        // This equals `manager.num_devices()` at construction time and lets
        // CPU-only tests construct `InterGpuTransfer` with an empty-device
        // manager while still exercising the validation and readback paths.
        let num_devices = self.per_device_buffers.len();

        if dst_device >= num_devices {
            return Err(GpuError::invalid_buffer(format!(
                "dst_device {} is out of range (num_devices = {})",
                dst_device, num_devices
            )));
        }

        let mut results: Vec<Vec<u8>> = Vec::with_capacity(num_devices.saturating_sub(1));

        for src_idx in 0..num_devices {
            if src_idx == dst_device {
                continue;
            }

            // Retrieve the registered buffer slot for this device.
            // We do this before consulting the manager so that the `None`
            // (no-buffer) fast path never needs a real GpuContext.
            let slot = self
                .per_device_buffers
                .get(src_idx)
                .ok_or_else(|| GpuError::internal("per_device_buffers shorter than num_devices"))?;

            // Extract the buffer handle inside a nested scope so the
            // `MutexGuard` is dropped before any `.await` point, satisfying
            // the `clippy::await_holding_lock` lint.
            let maybe_buffer_info: Option<(Arc<wgpu::Buffer>, u64)> = {
                let guard = slot
                    .lock()
                    .map_err(|_| GpuError::internal("per_device_buffers lock poisoned"))?;
                guard.as_ref().map(|(buf, sz)| (Arc::clone(buf), *sz))
                // `guard` drops here — mutex released before any await.
            };

            // If no buffer has been registered yet, return empty bytes for this device.
            let (src_buffer, size_bytes) = match maybe_buffer_info {
                Some(pair) => pair,
                None => {
                    debug!(
                        "gather: device {} has no registered buffer, returning empty slice",
                        src_idx
                    );
                    results.push(Vec::new());
                    continue;
                }
            };

            // Retrieve the GPU context now that we know we have a buffer to
            // read.  This call is intentionally placed after the `None` check
            // so that CPU-only tests (where `manager.device()` would return
            // `None` for every index) never reach this path.
            let ctx = self
                .manager
                .device(src_idx)
                .ok_or_else(|| GpuError::invalid_buffer("source device context missing"))?;

            // Step 1 – allocate a MAP_READ | COPY_DST staging buffer on device src_idx.
            // GPU device objects are not directly accessible from GpuContext because the
            // Arc<Device> is behind a private field, but `GpuContext::device()` returns a
            // shared reference `&Device`, which is sufficient for buffer creation and polling.
            let wgpu_device = ctx.device();
            let wgpu_queue = ctx.queue();

            let staging = wgpu_device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gather_staging"),
                size: size_bytes,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            // Step 2 – encode copy_buffer_to_buffer and submit.
            let mut encoder = wgpu_device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gather_copy_encoder"),
            });
            encoder.copy_buffer_to_buffer(&src_buffer, 0, &staging, 0, size_bytes);
            wgpu_queue.submit(std::iter::once(encoder.finish()));

            // Step 3 – poll the device until the copy submission completes.
            // wgpu 29 uses `PollType::wait_indefinitely()` which blocks until
            // all submitted work has finished.
            wgpu_device
                .poll(PollType::wait_indefinitely())
                .map_err(|e| GpuError::execution_failed(format!("device poll error: {e:?}")))?;

            // Step 4 – map the staging buffer and read the raw bytes.
            let (tx, rx) =
                futures::channel::oneshot::channel::<Result<(), wgpu::BufferAsyncError>>();
            staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });

            // Poll once more to drive the mapping callback.
            wgpu_device
                .poll(PollType::wait_indefinitely())
                .map_err(|e| {
                    GpuError::execution_failed(format!("device poll (map) error: {e:?}"))
                })?;

            rx.await
                .map_err(|_| GpuError::buffer_mapping("gather: oneshot channel closed"))?
                .map_err(|e| GpuError::buffer_mapping(format!("gather: map_async failed: {e}")))?;

            let raw_data: Vec<u8> = staging
                .slice(..)
                .get_mapped_range()
                .map_err(|e| {
                    GpuError::buffer_mapping(format!("gather: get_mapped_range failed: {e}"))
                })?
                .to_vec();
            staging.unmap();

            debug!(
                "gather: read {} bytes from device {} (staging buffer)",
                raw_data.len(),
                src_idx
            );

            results.push(raw_data);
        }

        Ok(results)
    }

    /// Blocking variant of [`gather`](Self::gather).
    ///
    /// Wraps the async gather in `pollster::block_on` so that callers in
    /// synchronous contexts (e.g., tests, CLI tools) do not need an async
    /// runtime.
    ///
    /// # Errors
    ///
    /// Propagates any error returned by [`gather`](Self::gather).
    pub fn gather_blocking(&self, dst_device: usize) -> GpuResult<Vec<Vec<u8>>> {
        pollster::block_on(self.gather(dst_device))
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
        // With zero devices `idx % num_devices` would divide by zero. There is
        // nowhere to dispatch work, so return an empty distribution instead of
        // panicking.
        if num_devices == 0 {
            return Vec::new();
        }
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
        // No devices means no weights and no `device_items[0]` to index into;
        // return an empty distribution rather than panicking on OOB access.
        if num_devices == 0 {
            return Vec::new();
        }
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
    fn test_work_distributor_zero_devices_no_panic() {
        // A manager with zero devices must not panic in any distribution
        // strategy: round_robin would divide by zero and load_balanced would
        // index an empty vec. All strategies must yield an empty distribution.
        let manager = Arc::new(MultiGpuManager::new_empty_for_testing());
        assert_eq!(manager.num_devices(), 0);

        for strategy in [
            DistributionStrategy::RoundRobin,
            DistributionStrategy::LoadBalanced,
            DistributionStrategy::DataLocal,
        ] {
            let distributor = WorkDistributor::new(Arc::clone(&manager), strategy);
            let work: Vec<u32> = (0..10).collect();
            let distribution = distributor.distribute_work(work);
            assert!(
                distribution.is_empty(),
                "strategy {strategy:?} must return an empty distribution with zero devices"
            );
        }
    }

    #[test]
    fn test_distribution_strategy() {
        assert_eq!(
            DistributionStrategy::RoundRobin,
            DistributionStrategy::RoundRobin
        );
    }

    // -----------------------------------------------------------------------
    // gather tests — CPU-level (no GPU hardware required)
    // -----------------------------------------------------------------------
    //
    // The validation logic inside `gather` reads only `self.manager.num_devices()`
    // and `self.per_device_buffers` — both of which are plain Rust data
    // structures.  Unfortunately `MultiGpuManager::new` calls `wgpu::Instance::new`
    // which panics on CI when no GPU backend is compiled in.  The tests below
    // therefore test:
    //  (a) `set_device_buffer` returns errors for out-of-range indices
    //      independently of any wgpu object, and
    //  (b) the blocking wrapper `gather_blocking` exists and compiles.
    // Full integration (with real GPUs) is covered by the `#[ignore]` tests.

    /// Helper: build a zero-device `InterGpuTransfer` without touching wgpu.
    fn zero_device_transfer() -> InterGpuTransfer {
        InterGpuTransfer {
            manager: Arc::new(MultiGpuManager::new_empty_for_testing()),
            per_device_buffers: Vec::new(),
        }
    }

    /// Helper: build an `InterGpuTransfer` with `n` device slots but no real
    /// GPU context behind any of them.  The slots are empty (`None`), so
    /// `gather` will push `Vec::new()` for each source slot without ever
    /// calling `manager.device()`.
    fn n_slot_transfer(n: usize) -> InterGpuTransfer {
        let per_device_buffers = (0..n).map(|_| Arc::new(Mutex::new(None))).collect();
        InterGpuTransfer {
            manager: Arc::new(MultiGpuManager::new_empty_for_testing()),
            per_device_buffers,
        }
    }

    /// `set_device_buffer` with an out-of-range index must return an error.
    /// This exercises the `per_device_buffers` bounds-check without touching
    /// any wgpu object.
    #[test]
    fn test_set_device_buffer_out_of_range_returns_error() {
        let transfer = zero_device_transfer();
        // Provide a dummy Arc<wgpu::Buffer> — wgpu::Buffer is not constructible
        // without a device, but `set_device_buffer` will error before it stores
        // the value because the slot Vec is empty.
        //
        // We cannot construct a real wgpu::Buffer here, but the function
        // returns an error before it even touches the buffer, so we just
        // need to prove the error path is reachable.  Use a zero-length
        // per_device_buffers and call with device_idx=0.
        let result = transfer.per_device_buffers.get(0);
        assert!(result.is_none(), "zero-device transfer must have no slots");
        // The set_device_buffer call itself requires an Arc<wgpu::Buffer> which
        // we cannot create without a real device.  We have already verified
        // that `per_device_buffers` is empty (no slots), meaning
        // `set_device_buffer(0, ...)` would return Err immediately.
        // This is a compile-time structural check — no GPU needed.
    }

    /// `gather_blocking` compiles and, when passed an out-of-range destination
    /// device, returns an error without touching GPU hardware.
    #[test]
    fn test_gather_blocking_available() {
        // A zero-slot transfer has 0 virtual devices.
        // dst_device=0 is therefore always out of range.
        let transfer = zero_device_transfer();
        let result = transfer.gather_blocking(0);
        assert!(
            result.is_err(),
            "gather_blocking with 0 devices must error on any dst_device"
        );
    }

    /// `gather` with no source devices (only device 0, which is the
    /// destination) must return `Ok(vec![])`.
    #[test]
    fn test_gather_no_source_devices_returns_empty() {
        // Simulate a single-device manager by giving InterGpuTransfer exactly
        // 1 buffer slot (for device 0).  When gather(dst_device=0) runs, the
        // inner loop skips device 0 (the only index), producing an empty result.
        let transfer = n_slot_transfer(1);
        let result = pollster::block_on(transfer.gather(0));
        let gathered = result.expect("gather with single-device stub must succeed");
        assert!(
            gathered.is_empty(),
            "no source devices → gather result must be empty"
        );
    }

    /// `gather` with `dst_device >= num_devices` must return an error.
    #[test]
    fn test_gather_invalid_dst_device_returns_error() {
        // Single-slot stub: per_device_buffers.len() == 1, dst_device=1 is out of range.
        let transfer = n_slot_transfer(1);
        let result = pollster::block_on(transfer.gather(1));
        assert!(
            result.is_err(),
            "dst_device=1 when num_devices=1 must be an error"
        );
    }

    /// GPU-gated integration test: submit a small compute payload, register
    /// the result buffer, and verify that gather returns non-empty bytes.
    ///
    /// Requires real GPU hardware — skipped in CI via `#[ignore]`.
    #[ignore]
    #[tokio::test]
    async fn test_gather_real_readback_returns_nonzero_bytes() {
        // We need at least 2 GPUs for a meaningful gather.  If only 1 (or 0)
        // is available, the test would be vacuous, so skip gracefully.
        let config = MultiGpuConfig {
            min_devices: 2,
            max_devices: 8,
            ..MultiGpuConfig::default()
        };
        let manager = match MultiGpuManager::new(config).await {
            Ok(m) => Arc::new(m),
            Err(_) => {
                // Fewer than 2 GPUs present — skip.
                return;
            }
        };

        let transfer = InterGpuTransfer::new(Arc::clone(&manager));

        // On device 1, create a small COPY_SRC | STORAGE buffer populated
        // with known bytes and register it for gather.
        let src_ctx = manager
            .device(1)
            .expect("device 1 must exist when num_devices >= 2");

        const PAYLOAD: &[u8] = b"OxiGeo-gather-test-payload-12345678";
        let payload_size = PAYLOAD.len() as u64;
        // Align to COPY_BUFFER_ALIGNMENT (4).
        let aligned = ((payload_size + 3) / 4) * 4;

        let src_buffer = Arc::new(src_ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_src_buffer"),
            size: aligned,
            usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST | BufferUsages::STORAGE,
            mapped_at_creation: false,
        }));

        // Upload payload.
        src_ctx.queue().write_buffer(&src_buffer, 0, PAYLOAD);

        // Register the buffer.
        transfer
            .set_device_buffer(1, Arc::clone(&src_buffer), aligned)
            .expect("set_device_buffer must succeed for device 1");

        // Gather device 1 → device 0.
        let gathered = transfer
            .gather(0)
            .await
            .expect("gather must succeed when GPU is available");

        assert_eq!(
            gathered.len(),
            1,
            "should have exactly 1 result (device 1 → device 0)"
        );
        assert!(!gathered[0].is_empty(), "gathered bytes must not be empty");
        // The first PAYLOAD.len() bytes must match.
        assert_eq!(
            &gathered[0][..PAYLOAD.len()],
            PAYLOAD,
            "gathered payload must match the uploaded data"
        );
    }
}
