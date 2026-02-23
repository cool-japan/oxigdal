//! Multi-GPU orchestration and management.
//!
//! This module provides advanced multi-GPU support including:
//! - Automatic GPU detection and selection
//! - Load balancing across GPUs
//! - Work stealing between GPUs
//! - GPU affinity and pinning

pub mod affinity;
pub mod device_manager;
pub mod load_balancer;
pub mod sync;
pub mod work_queue;

use crate::error::{GpuAdvancedError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use wgpu::{Adapter, Device, Queue};

/// GPU device information
#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    /// Device index
    pub index: usize,
    /// Device name
    pub name: String,
    /// Backend type (Vulkan, Metal, DX12, etc.)
    pub backend: wgpu::Backend,
    /// Device type (DiscreteGpu, IntegratedGpu, VirtualGpu, Cpu)
    pub device_type: wgpu::DeviceType,
    /// Maximum buffer size
    pub max_buffer_size: u64,
    /// Maximum texture dimension 1D
    pub max_texture_dimension_1d: u32,
    /// Maximum texture dimension 2D
    pub max_texture_dimension_2d: u32,
    /// Maximum texture dimension 3D
    pub max_texture_dimension_3d: u32,
    /// Maximum compute workgroup size X
    pub max_compute_workgroup_size_x: u32,
    /// Maximum compute workgroup size Y
    pub max_compute_workgroup_size_y: u32,
    /// Maximum compute workgroup size Z
    pub max_compute_workgroup_size_z: u32,
    /// Maximum compute workgroups per dimension
    pub max_compute_workgroups_per_dimension: u32,
    /// Maximum bind groups
    pub max_bind_groups: u32,
    /// Memory size (estimated)
    pub memory_size: Option<u64>,
}

/// GPU device with associated resources
pub struct GpuDevice {
    /// Device info
    pub info: GpuDeviceInfo,
    /// WGPU adapter
    pub adapter: Arc<Adapter>,
    /// WGPU device
    pub device: Arc<Device>,
    /// WGPU queue
    pub queue: Arc<Queue>,
    /// Current memory usage
    pub memory_usage: Arc<RwLock<u64>>,
    /// Current workload (0.0 to 1.0)
    pub workload: Arc<RwLock<f32>>,
}

impl GpuDevice {
    /// Create a new GPU device
    pub fn new(index: usize, adapter: Adapter, device: Device, queue: Queue) -> Result<Self> {
        let info = adapter.get_info();
        let limits = device.limits();

        let device_info = GpuDeviceInfo {
            index,
            name: info.name.clone(),
            backend: info.backend,
            device_type: info.device_type,
            max_buffer_size: limits.max_buffer_size,
            max_texture_dimension_1d: limits.max_texture_dimension_1d,
            max_texture_dimension_2d: limits.max_texture_dimension_2d,
            max_texture_dimension_3d: limits.max_texture_dimension_3d,
            max_compute_workgroup_size_x: limits.max_compute_workgroup_size_x,
            max_compute_workgroup_size_y: limits.max_compute_workgroup_size_y,
            max_compute_workgroup_size_z: limits.max_compute_workgroup_size_z,
            max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension,
            max_bind_groups: limits.max_bind_groups,
            memory_size: None, // Could be estimated from limits
        };

        Ok(Self {
            info: device_info,
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            memory_usage: Arc::new(RwLock::new(0)),
            workload: Arc::new(RwLock::new(0.0)),
        })
    }

    /// Get current memory usage
    pub fn get_memory_usage(&self) -> u64 {
        *self.memory_usage.read()
    }

    /// Update memory usage
    pub fn update_memory_usage(&self, delta: i64) {
        let mut usage = self.memory_usage.write();
        if delta >= 0 {
            *usage = usage.saturating_add(delta as u64);
        } else {
            *usage = usage.saturating_sub((-delta) as u64);
        }
    }

    /// Get current workload
    pub fn get_workload(&self) -> f32 {
        *self.workload.read()
    }

    /// Set workload
    pub fn set_workload(&self, workload: f32) {
        *self.workload.write() = workload.clamp(0.0, 1.0);
    }

    /// Check if device is available (low workload)
    pub fn is_available(&self) -> bool {
        self.get_workload() < 0.8
    }

    /// Get device score for selection (higher is better)
    pub fn get_score(&self) -> f32 {
        // Score based on device type and current workload
        let type_score = match self.info.device_type {
            wgpu::DeviceType::DiscreteGpu => 1.0,
            wgpu::DeviceType::IntegratedGpu => 0.7,
            wgpu::DeviceType::VirtualGpu => 0.5,
            wgpu::DeviceType::Cpu => 0.3,
            wgpu::DeviceType::Other => 0.1,
        };

        let workload = self.get_workload();
        type_score * (1.0 - workload)
    }
}

/// Multi-GPU manager
pub struct MultiGpuManager {
    /// Available GPU devices
    devices: Vec<Arc<GpuDevice>>,
    /// Device selection strategy (reserved for future dynamic strategy switching)
    #[allow(dead_code)]
    strategy: SelectionStrategy,
    /// Work queues per device
    work_queues: DashMap<usize, Arc<work_queue::WorkQueue>>,
    /// Load balancer
    load_balancer: Arc<load_balancer::LoadBalancer>,
}

/// Device selection strategy
#[derive(Debug, Clone, Copy)]
pub enum SelectionStrategy {
    /// Round-robin selection
    RoundRobin,
    /// Select least loaded device
    LeastLoaded,
    /// Select device with highest score
    BestScore,
    /// Affinity-based selection
    Affinity,
}

impl MultiGpuManager {
    /// Create a new multi-GPU manager
    pub async fn new(strategy: SelectionStrategy) -> Result<Self> {
        let devices = Self::enumerate_devices().await?;

        if devices.is_empty() {
            return Err(GpuAdvancedError::GpuNotFound(
                "No compatible GPU devices found".to_string(),
            ));
        }

        let work_queues = DashMap::new();
        for device in &devices {
            work_queues.insert(
                device.info.index,
                Arc::new(work_queue::WorkQueue::new(device.clone())),
            );
        }

        let load_balancer = Arc::new(load_balancer::LoadBalancer::new(devices.clone(), strategy));

        Ok(Self {
            devices,
            strategy,
            work_queues,
            load_balancer,
        })
    }

    /// Enumerate all available GPU devices
    async fn enumerate_devices() -> Result<Vec<Arc<GpuDevice>>> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let mut devices = Vec::new();
        let mut index = 0;

        // Try each backend separately to avoid potential hangs
        for _backend in &[
            wgpu::Backends::VULKAN,
            wgpu::Backends::METAL,
            wgpu::Backends::DX12,
            wgpu::Backends::GL,
        ] {
            if let Ok(adapter) = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
            {
                let info = adapter.get_info();

                // Skip CPU adapters by default
                if info.device_type == wgpu::DeviceType::Cpu {
                    continue;
                }

                // Skip if we already have this adapter (avoid duplicates)
                if devices.iter().any(|d: &Arc<GpuDevice>| {
                    let d_info = &d.info;
                    d_info.name == info.name && d_info.backend == info.backend
                }) {
                    continue;
                }

                // Request device
                let (device, queue) = match adapter
                    .request_device(&wgpu::DeviceDescriptor {
                        label: Some(&format!("GPU Device {}", index)),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::default(),
                        memory_hints: wgpu::MemoryHints::Performance,
                        experimental_features: wgpu::ExperimentalFeatures::disabled(),
                        trace: wgpu::Trace::Off,
                    })
                    .await
                {
                    Ok((device, queue)) => (device, queue),
                    Err(e) => {
                        tracing::warn!("Failed to request device {}: {}", index, e);
                        continue;
                    }
                };

                let gpu_device = GpuDevice::new(index, adapter, device, queue)?;
                devices.push(Arc::new(gpu_device));
                index += 1;
            }
        }

        Ok(devices)
    }

    /// Get total number of GPUs
    pub fn gpu_count(&self) -> usize {
        self.devices.len()
    }

    /// Get GPU by index
    pub fn get_gpu(&self, index: usize) -> Result<Arc<GpuDevice>> {
        self.devices
            .get(index)
            .cloned()
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index,
                total: self.devices.len(),
            })
    }

    /// Get all GPUs
    pub fn get_all_gpus(&self) -> &[Arc<GpuDevice>] {
        &self.devices
    }

    /// Select best GPU for a task
    pub fn select_gpu(&self) -> Result<Arc<GpuDevice>> {
        self.load_balancer.select_device()
    }

    /// Select GPU with specific requirements
    pub fn select_gpu_with_requirements(
        &self,
        min_memory: Option<u64>,
        preferred_type: Option<wgpu::DeviceType>,
    ) -> Result<Arc<GpuDevice>> {
        let mut candidates: Vec<_> = self
            .devices
            .iter()
            .filter(|device| {
                if let Some(min_mem) = min_memory {
                    if let Some(mem_size) = device.info.memory_size {
                        if mem_size < min_mem {
                            return false;
                        }
                    }
                }

                if let Some(pref_type) = preferred_type {
                    if device.info.device_type != pref_type {
                        return false;
                    }
                }

                device.is_available()
            })
            .collect();

        if candidates.is_empty() {
            return Err(GpuAdvancedError::GpuNotFound(
                "No GPU matching requirements".to_string(),
            ));
        }

        // Sort by score
        candidates.sort_by(|a, b| {
            b.get_score()
                .partial_cmp(&a.get_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(candidates[0].clone())
    }

    /// Get work queue for a GPU
    pub fn get_work_queue(&self, index: usize) -> Result<Arc<work_queue::WorkQueue>> {
        self.work_queues
            .get(&index)
            .map(|q| q.clone())
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index,
                total: self.devices.len(),
            })
    }

    /// Submit work to best available GPU
    pub async fn submit_work<F, T>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&GpuDevice) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let device = self.select_gpu()?;
        let queue = self.get_work_queue(device.info.index)?;
        queue.submit_work(work).await
    }

    /// Get load balancer
    pub fn get_load_balancer(&self) -> Arc<load_balancer::LoadBalancer> {
        self.load_balancer.clone()
    }

    /// Print GPU information
    pub fn print_gpu_info(&self) {
        println!("Multi-GPU Manager - {} devices found", self.devices.len());
        for device in &self.devices {
            println!(
                "  GPU {}: {} ({:?}, {:?})",
                device.info.index, device.info.name, device.info.backend, device.info.device_type
            );
            println!("    Max buffer size: {} bytes", device.info.max_buffer_size);
            println!(
                "    Max texture 2D: {}x{}",
                device.info.max_texture_dimension_2d, device.info.max_texture_dimension_2d
            );
            println!(
                "    Max workgroup size: {}x{}x{}",
                device.info.max_compute_workgroup_size_x,
                device.info.max_compute_workgroup_size_y,
                device.info.max_compute_workgroup_size_z
            );
            println!(
                "    Current workload: {:.1}%",
                device.get_workload() * 100.0
            );
            println!("    Memory usage: {} bytes", device.get_memory_usage());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multi_gpu_manager_creation() {
        let result = MultiGpuManager::new(SelectionStrategy::LeastLoaded).await;

        // This might fail if no GPU is available, which is ok in CI
        match result {
            Ok(manager) => {
                assert!(manager.gpu_count() > 0);
                manager.print_gpu_info();
            }
            Err(e) => {
                println!("No GPU available: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_gpu_selection() {
        let result = MultiGpuManager::new(SelectionStrategy::BestScore).await;

        if let Ok(manager) = result {
            let gpu = manager.select_gpu();
            assert!(gpu.is_ok());

            if let Ok(gpu) = gpu {
                println!("Selected GPU: {}", gpu.info.name);
                assert!(gpu.get_score() >= 0.0);
            }
        }
    }
}
