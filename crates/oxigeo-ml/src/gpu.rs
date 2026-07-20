//! GPU acceleration for ML inference
//!
//! This module provides GPU acceleration support across multiple backends
//! including CUDA, ROCm, DirectML, Vulkan, and WebGPU.
//!
//! # Safety
//!
//! This module requires unsafe code for FFI operations with GPU libraries.
//! All unsafe operations are carefully reviewed and documented.

#![allow(unsafe_code)]

use crate::error::{InferenceError, MlError, Result};
use tracing::{debug, info};

/// GPU backend types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    /// NVIDIA CUDA
    Cuda,
    /// AMD ROCm
    Rocm,
    /// DirectML (Windows)
    DirectMl,
    /// Vulkan (cross-platform)
    Vulkan,
    /// WebGPU (web/browser)
    WebGpu,
    /// Metal (Apple)
    Metal,
    /// OpenCL (cross-platform)
    OpenCl,
}

impl GpuBackend {
    /// Returns the backend name
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cuda => "CUDA",
            Self::Rocm => "ROCm",
            Self::DirectMl => "DirectML",
            Self::Vulkan => "Vulkan",
            Self::WebGpu => "WebGPU",
            Self::Metal => "Metal",
            Self::OpenCl => "OpenCL",
        }
    }

    /// Checks if the backend is available on the current platform
    #[must_use]
    pub fn is_available(&self) -> bool {
        match self {
            Self::Cuda => check_cuda_available(),
            Self::Rocm => check_rocm_available(),
            Self::DirectMl => check_directml_available(),
            Self::Vulkan => check_vulkan_available(),
            Self::WebGpu => check_webgpu_available(),
            Self::Metal => check_metal_available(),
            Self::OpenCl => check_opencl_available(),
        }
    }
}

/// GPU device information
#[derive(Debug, Clone)]
pub struct GpuDevice {
    /// Device ID
    pub id: usize,
    /// Device name
    pub name: String,
    /// Total memory in bytes
    pub total_memory: usize,
    /// Free memory in bytes
    pub free_memory: usize,
    /// Compute capability (CUDA) or equivalent
    pub compute_capability: String,
    /// Backend type
    pub backend: GpuBackend,
}

impl GpuDevice {
    /// Returns memory utilization percentage
    #[must_use]
    pub fn memory_utilization(&self) -> f32 {
        if self.total_memory > 0 {
            let used = self.total_memory.saturating_sub(self.free_memory);
            (used as f32 / self.total_memory as f32) * 100.0
        } else {
            0.0
        }
    }

    /// Checks if the device has sufficient free memory
    #[must_use]
    pub fn has_sufficient_memory(&self, required_bytes: usize) -> bool {
        self.free_memory >= required_bytes
    }
}

/// GPU configuration
#[derive(Debug, Clone)]
pub struct GpuConfig {
    /// Preferred backend (None for auto-select)
    pub backend: Option<GpuBackend>,
    /// Device ID (None for auto-select)
    pub device_id: Option<usize>,
    /// Enable mixed precision (FP16)
    pub mixed_precision: bool,
    /// Enable tensor cores (CUDA)
    pub tensor_cores: bool,
    /// Memory growth (allocate on demand)
    pub memory_growth: bool,
    /// Per-process GPU memory fraction
    pub memory_fraction: f32,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            backend: None,
            device_id: None,
            mixed_precision: true,
            tensor_cores: true,
            memory_growth: true,
            memory_fraction: 0.9,
        }
    }
}

impl GpuConfig {
    /// Creates a configuration builder
    #[must_use]
    pub fn builder() -> GpuConfigBuilder {
        GpuConfigBuilder::default()
    }
}

/// Builder for GPU configuration
#[derive(Debug, Default)]
pub struct GpuConfigBuilder {
    backend: Option<GpuBackend>,
    device_id: Option<usize>,
    mixed_precision: Option<bool>,
    tensor_cores: Option<bool>,
    memory_growth: Option<bool>,
    memory_fraction: Option<f32>,
}

impl GpuConfigBuilder {
    /// Sets the GPU backend
    #[must_use]
    pub fn backend(mut self, backend: GpuBackend) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Sets the device ID
    #[must_use]
    pub fn device_id(mut self, id: usize) -> Self {
        self.device_id = Some(id);
        self
    }

    /// Enables mixed precision
    #[must_use]
    pub fn mixed_precision(mut self, enable: bool) -> Self {
        self.mixed_precision = Some(enable);
        self
    }

    /// Enables tensor cores
    #[must_use]
    pub fn tensor_cores(mut self, enable: bool) -> Self {
        self.tensor_cores = Some(enable);
        self
    }

    /// Enables memory growth
    #[must_use]
    pub fn memory_growth(mut self, enable: bool) -> Self {
        self.memory_growth = Some(enable);
        self
    }

    /// Sets memory fraction
    #[must_use]
    pub fn memory_fraction(mut self, fraction: f32) -> Self {
        self.memory_fraction = Some(fraction.clamp(0.1, 1.0));
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> GpuConfig {
        GpuConfig {
            backend: self.backend,
            device_id: self.device_id,
            mixed_precision: self.mixed_precision.unwrap_or(true),
            tensor_cores: self.tensor_cores.unwrap_or(true),
            memory_growth: self.memory_growth.unwrap_or(true),
            memory_fraction: self.memory_fraction.unwrap_or(0.9),
        }
    }
}

/// Lists available GPU devices
///
/// # Errors
/// Returns an error if device enumeration fails
pub fn list_devices() -> Result<Vec<GpuDevice>> {
    info!("Enumerating GPU devices");

    let mut devices = Vec::new();

    // Try each backend
    for backend in &[
        GpuBackend::Cuda,
        GpuBackend::Rocm,
        GpuBackend::Metal,
        GpuBackend::Vulkan,
        GpuBackend::DirectMl,
    ] {
        if backend.is_available() {
            devices.extend(list_devices_for_backend(*backend)?);
        }
    }

    info!("Found {} GPU device(s)", devices.len());
    Ok(devices)
}

/// Lists devices for a specific backend
fn list_devices_for_backend(backend: GpuBackend) -> Result<Vec<GpuDevice>> {
    debug!("Enumerating devices for backend: {}", backend.name());

    match backend {
        GpuBackend::Cuda => enumerate_cuda_devices(),
        GpuBackend::Metal => enumerate_metal_devices(),
        GpuBackend::Vulkan => enumerate_vulkan_devices(),
        GpuBackend::OpenCl => enumerate_opencl_devices(),
        GpuBackend::Rocm => enumerate_rocm_devices(),
        GpuBackend::DirectMl => enumerate_directml_devices(),
        GpuBackend::WebGpu => enumerate_webgpu_devices(),
    }
}

/// Selects the best available GPU device
///
/// # Errors
/// Returns an error if no GPU is available
pub fn select_device(config: &GpuConfig) -> Result<GpuDevice> {
    let devices = list_devices()?;

    if devices.is_empty() {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: "No GPU devices found".to_string(),
        }));
    }

    // Filter by backend if specified
    let filtered: Vec<_> = if let Some(backend) = config.backend {
        devices
            .into_iter()
            .filter(|d| d.backend == backend)
            .collect()
    } else {
        devices
    };

    if filtered.is_empty() {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: "No GPU devices match the specified backend".to_string(),
        }));
    }

    // Select by device ID or pick the one with most free memory
    let device = if let Some(id) = config.device_id {
        filtered.into_iter().find(|d| d.id == id).ok_or_else(|| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Device ID {} not found", id),
            })
        })?
    } else {
        filtered
            .into_iter()
            .max_by_key(|d| d.free_memory)
            .ok_or_else(|| {
                MlError::Inference(InferenceError::GpuNotAvailable {
                    message: "Failed to select GPU device".to_string(),
                })
            })?
    };

    info!(
        "Selected GPU: {} (free memory: {} MB)",
        device.name,
        device.free_memory / (1024 * 1024)
    );

    Ok(device)
}

// ============================================================================
// CUDA Backend Implementation
// ============================================================================

/// Checks if CUDA is available on the system
fn check_cuda_available() -> bool {
    #[cfg(feature = "cuda")]
    {
        cuda_check_runtime()
    }
    #[cfg(not(feature = "cuda"))]
    false
}

/// Checks for CUDA runtime library
#[cfg(feature = "cuda")]
fn cuda_check_runtime() -> bool {
    use libloading::Library;

    // Try to load CUDA runtime library
    let lib_names = if cfg!(target_os = "windows") {
        vec!["nvcuda.dll", "cudart64_110.dll", "cudart64_12.dll"]
    } else if cfg!(target_os = "macos") {
        vec!["libcuda.dylib", "/usr/local/cuda/lib/libcuda.dylib"]
    } else {
        vec![
            "libcuda.so",
            "libcuda.so.1",
            "/usr/lib/x86_64-linux-gnu/libcuda.so",
            "/usr/local/cuda/lib64/libcuda.so",
        ]
    };

    for lib_name in lib_names {
        if unsafe { Library::new(lib_name) }.is_ok() {
            debug!("Found CUDA runtime library: {}", lib_name);
            return true;
        }
    }

    debug!("CUDA runtime library not found");
    false
}

/// Enumerates CUDA devices
fn enumerate_cuda_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(feature = "cuda")]
    {
        cuda_enumerate_devices_impl()
    }
    #[cfg(not(feature = "cuda"))]
    {
        Ok(Vec::new())
    }
}

#[cfg(feature = "cuda")]
fn cuda_enumerate_devices_impl() -> Result<Vec<GpuDevice>> {
    use libloading::{Library, Symbol};
    use std::ffi::{c_char, c_int};

    // CUDA type definitions
    type CUdevice = c_int;
    type CUresult = c_int;

    // CUDA device attributes
    const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: c_int = 75;
    const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: c_int = 76;

    // Try to load CUDA runtime
    let lib_names = if cfg!(target_os = "windows") {
        vec!["nvcuda.dll"]
    } else if cfg!(target_os = "macos") {
        vec!["libcuda.dylib"]
    } else {
        vec!["libcuda.so.1", "libcuda.so"]
    };

    let lib = lib_names
        .iter()
        .find_map(|name| unsafe { Library::new(*name).ok() })
        .ok_or_else(|| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: "CUDA library not found".to_string(),
            })
        })?;

    // Load cuInit function
    let cu_init: Symbol<unsafe extern "C" fn(c_int) -> CUresult> = unsafe { lib.get(b"cuInit\0") }
        .map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to load cuInit: {}", e),
            })
        })?;

    // Initialize CUDA
    let result = unsafe { cu_init(0) };
    if result != 0 {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("CUDA initialization failed with code: {}", result),
        }));
    }

    // Load cuDeviceGetCount function
    let cu_device_get_count: Symbol<unsafe extern "C" fn(*mut c_int) -> CUresult> =
        unsafe { lib.get(b"cuDeviceGetCount\0") }.map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to load cuDeviceGetCount: {}", e),
            })
        })?;

    // Get device count
    let mut count: c_int = 0;
    let result = unsafe { cu_device_get_count(&mut count) };
    if result != 0 {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to get CUDA device count: {}", result),
        }));
    }

    debug!("Found {} CUDA device(s)", count);

    // Load additional CUDA functions for device property queries
    let cu_device_get: Symbol<unsafe extern "C" fn(*mut CUdevice, c_int) -> CUresult> =
        unsafe { lib.get(b"cuDeviceGet\0") }.map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to load cuDeviceGet: {}", e),
            })
        })?;

    let cu_device_get_name: Symbol<unsafe extern "C" fn(*mut c_char, c_int, CUdevice) -> CUresult> =
        unsafe { lib.get(b"cuDeviceGetName\0") }.map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to load cuDeviceGetName: {}", e),
            })
        })?;

    let cu_device_total_mem: Symbol<unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult> =
        unsafe { lib.get(b"cuDeviceTotalMem_v2\0") }
            .or_else(|_| unsafe { lib.get(b"cuDeviceTotalMem\0") })
            .map_err(|e| {
                MlError::Inference(InferenceError::GpuNotAvailable {
                    message: format!("Failed to load cuDeviceTotalMem: {}", e),
                })
            })?;

    let cu_device_get_attribute: Symbol<
        unsafe extern "C" fn(*mut c_int, c_int, CUdevice) -> CUresult,
    > = unsafe { lib.get(b"cuDeviceGetAttribute\0") }.map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to load cuDeviceGetAttribute: {}", e),
        })
    })?;

    // Enumerate devices and query their properties
    let mut devices = Vec::new();
    for i in 0..count {
        // Get device handle
        let mut device: CUdevice = 0;
        let result = unsafe { cu_device_get(&mut device, i) };
        if result != 0 {
            debug!("Failed to get CUDA device {}: error code {}", i, result);
            continue;
        }

        // Query device name
        let mut name_buf = [0i8; 256];
        let result = unsafe { cu_device_get_name(name_buf.as_mut_ptr(), 256, device) };
        let device_name = if result == 0 {
            unsafe {
                std::ffi::CStr::from_ptr(name_buf.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            format!("NVIDIA CUDA Device {}", i)
        };

        // Query total memory
        let mut total_mem: usize = 0;
        let result = unsafe { cu_device_total_mem(&mut total_mem, device) };
        if result != 0 {
            debug!(
                "Failed to get total memory for device {}: error code {}",
                i, result
            );
            total_mem = 0;
        }

        // Note: cuMemGetInfo requires a CUDA context to be active, which is not
        // available during enumeration. We'll report total memory and assume
        // most is free for simplicity. Applications should check actual free
        // memory after context creation.
        let free_mem = (total_mem as f64 * 0.95) as usize; // Estimate 95% available

        // Query compute capability
        let mut major: c_int = 0;
        let mut minor: c_int = 0;
        let result_major = unsafe {
            cu_device_get_attribute(
                &mut major,
                CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
                device,
            )
        };
        let result_minor = unsafe {
            cu_device_get_attribute(
                &mut minor,
                CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
                device,
            )
        };

        let compute_capability = if result_major == 0 && result_minor == 0 {
            format!("{}.{}", major, minor)
        } else {
            "Unknown".to_string()
        };

        devices.push(GpuDevice {
            id: i as usize,
            name: device_name,
            total_memory: total_mem,
            free_memory: free_mem,
            compute_capability,
            backend: GpuBackend::Cuda,
        });
    }

    Ok(devices)
}

// ============================================================================
// Metal Backend Implementation (macOS)
// ============================================================================

/// Checks if Metal is available on the system
fn check_metal_available() -> bool {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    {
        metal_check_available()
    }
    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    false
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_check_available() -> bool {
    // Metal is always available on macOS 10.11+
    // We can verify by trying to create a device
    !metal::Device::all().is_empty()
}

/// Enumerates Metal devices
fn enumerate_metal_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    {
        metal_enumerate_devices_impl()
    }
    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    {
        Ok(Vec::new())
    }
}

#[cfg(all(feature = "metal", target_os = "macos"))]
fn metal_enumerate_devices_impl() -> Result<Vec<GpuDevice>> {
    let devices = metal::Device::all();

    if devices.is_empty() {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: "No Metal devices found".to_string(),
        }));
    }

    let mut gpu_devices = Vec::new();

    for (idx, device) in devices.iter().enumerate() {
        let name = device.name().to_string();
        let total_memory = device.recommended_max_working_set_size() as usize;
        let free_memory = total_memory; // Metal doesn't provide free memory directly

        gpu_devices.push(GpuDevice {
            id: idx,
            name,
            total_memory,
            free_memory,
            compute_capability: "Metal".to_string(),
            backend: GpuBackend::Metal,
        });
    }

    debug!("Found {} Metal device(s)", gpu_devices.len());
    Ok(gpu_devices)
}

// ============================================================================
// Vulkan Backend Implementation
// ============================================================================

/// Checks if Vulkan is available on the system
fn check_vulkan_available() -> bool {
    #[cfg(feature = "vulkan")]
    {
        vulkan_check_available()
    }
    #[cfg(not(feature = "vulkan"))]
    false
}

#[cfg(feature = "vulkan")]
fn vulkan_check_available() -> bool {
    use ash::{Entry, vk};

    // Try to load Vulkan
    if let Ok(entry) = unsafe { Entry::load() } {
        // Try to create instance
        let app_info = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 0, 0));

        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);

        if let Ok(instance) = unsafe { entry.create_instance(&create_info, None) } {
            unsafe { instance.destroy_instance(None) };
            return true;
        }
    }

    false
}

/// Enumerates Vulkan devices
fn enumerate_vulkan_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(feature = "vulkan")]
    {
        vulkan_enumerate_devices_impl()
    }
    #[cfg(not(feature = "vulkan"))]
    {
        Ok(Vec::new())
    }
}

#[cfg(feature = "vulkan")]
fn vulkan_enumerate_devices_impl() -> Result<Vec<GpuDevice>> {
    use ash::{Entry, vk};

    let entry = unsafe { Entry::load() }.map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to load Vulkan: {}", e),
        })
    })?;

    let app_info = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 0, 0));

    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);

    let instance = unsafe { entry.create_instance(&create_info, None) }.map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to create Vulkan instance: {}", e),
        })
    })?;

    let physical_devices = unsafe { instance.enumerate_physical_devices() }.map_err(|e| {
        unsafe { instance.destroy_instance(None) };
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to enumerate Vulkan devices: {}", e),
        })
    })?;

    let mut devices = Vec::new();
    for (idx, physical_device) in physical_devices.iter().enumerate() {
        let properties = unsafe { instance.get_physical_device_properties(*physical_device) };
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(*physical_device) };

        let device_name = unsafe {
            std::ffi::CStr::from_ptr(properties.device_name.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        // Calculate total memory from memory heaps
        let total_memory = memory_properties
            .memory_heaps
            .iter()
            .take(memory_properties.memory_heap_count as usize)
            .filter(|heap| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
            .map(|heap| heap.size as usize)
            .sum();

        devices.push(GpuDevice {
            id: idx,
            name: device_name,
            total_memory,
            free_memory: total_memory, // Vulkan doesn't provide free memory directly
            compute_capability: format!(
                "Vulkan {}.{}",
                vk::api_version_major(properties.api_version),
                vk::api_version_minor(properties.api_version)
            ),
            backend: GpuBackend::Vulkan,
        });
    }

    unsafe { instance.destroy_instance(None) };
    debug!("Found {} Vulkan device(s)", devices.len());
    Ok(devices)
}

// ============================================================================
// OpenCL Backend Implementation
// ============================================================================

/// Checks if OpenCL is available on the system
fn check_opencl_available() -> bool {
    #[cfg(feature = "opencl")]
    {
        opencl_check_available()
    }
    #[cfg(not(feature = "opencl"))]
    false
}

#[cfg(feature = "opencl")]
fn opencl_check_available() -> bool {
    use opencl3::platform::get_platforms;

    if let Ok(platforms) = get_platforms() {
        return !platforms.is_empty();
    }
    false
}

/// Enumerates OpenCL devices
fn enumerate_opencl_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(feature = "opencl")]
    {
        opencl_enumerate_devices_impl()
    }
    #[cfg(not(feature = "opencl"))]
    {
        Ok(Vec::new())
    }
}

#[cfg(feature = "opencl")]
fn opencl_enumerate_devices_impl() -> Result<Vec<GpuDevice>> {
    use opencl3::device::{CL_DEVICE_TYPE_GPU, Device, get_all_devices};

    let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU).map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to get OpenCL devices: {:?}", e),
        })
    })?;

    let mut devices = Vec::new();
    for (idx, device_id) in device_ids.iter().enumerate() {
        let device = Device::new(*device_id);

        let name = device.name().map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to get device name: {:?}", e),
            })
        })?;

        let total_memory = device.global_mem_size().map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to get device memory: {:?}", e),
            })
        })? as usize;

        devices.push(GpuDevice {
            id: idx,
            name,
            total_memory,
            free_memory: total_memory, // OpenCL doesn't provide free memory directly
            compute_capability: "OpenCL".to_string(),
            backend: GpuBackend::OpenCl,
        });
    }

    debug!("Found {} OpenCL device(s)", devices.len());
    Ok(devices)
}

// ============================================================================
// ROCm Backend Implementation
// ============================================================================

/// Checks if ROCm is available on the system
fn check_rocm_available() -> bool {
    #[cfg(feature = "rocm")]
    {
        rocm_check_runtime()
    }
    #[cfg(not(feature = "rocm"))]
    false
}

#[cfg(feature = "rocm")]
fn rocm_check_runtime() -> bool {
    use libloading::Library;

    // Try to load ROCm runtime library
    let lib_names = if cfg!(target_os = "linux") {
        vec![
            "libamdhip64.so",
            "libamdhip64.so.6",
            "/opt/rocm/lib/libamdhip64.so",
        ]
    } else {
        vec![]
    };

    for lib_name in lib_names {
        if unsafe { Library::new(lib_name) }.is_ok() {
            debug!("Found ROCm runtime library: {}", lib_name);
            return true;
        }
    }

    debug!("ROCm runtime library not found");
    false
}

/// Enumerates ROCm devices
fn enumerate_rocm_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(feature = "rocm")]
    {
        rocm_enumerate_devices_impl()
    }
    #[cfg(not(feature = "rocm"))]
    {
        Ok(Vec::new())
    }
}

#[cfg(feature = "rocm")]
fn rocm_enumerate_devices_impl() -> Result<Vec<GpuDevice>> {
    use libloading::{Library, Symbol};
    use std::ffi::{c_char, c_int};

    // HIP type definitions
    type HipError = c_int;
    const HIP_SUCCESS: HipError = 0;

    // Simplified hipDeviceProp_t structure (only fields we need)
    #[repr(C)]
    #[allow(non_snake_case)]
    struct HipDeviceProp {
        name: [c_char; 256],
        totalGlobalMem: usize,
        sharedMemPerBlock: usize,
        regsPerBlock: c_int,
        warpSize: c_int,
        memPitch: usize,
        maxThreadsPerBlock: c_int,
        maxThreadsDim: [c_int; 3],
        maxGridSize: [c_int; 3],
        clockRate: c_int,
        totalConstMem: usize,
        major: c_int,
        minor: c_int,
        textureAlignment: usize,
        deviceOverlap: c_int,
        multiProcessorCount: c_int,
        kernelExecTimeoutEnabled: c_int,
        integrated: c_int,
        canMapHostMemory: c_int,
        computeMode: c_int,
        maxTexture1D: c_int,
        maxTexture2D: [c_int; 2],
        maxTexture3D: [c_int; 3],
        // Note: Full structure has more fields, but we only need these
        // The actual memory layout may vary by HIP version
        gcnArchName: [c_char; 256],
    }

    // Try to load ROCm HIP runtime library
    let lib_names = if cfg!(target_os = "linux") {
        vec![
            "libamdhip64.so",
            "libamdhip64.so.6",
            "/opt/rocm/lib/libamdhip64.so",
        ]
    } else {
        vec![]
    };

    let lib = lib_names
        .iter()
        .find_map(|name| unsafe { Library::new(*name).ok() })
        .ok_or_else(|| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: "ROCm HIP library not found".to_string(),
            })
        })?;

    // Load hipGetDeviceCount function
    let hip_get_device_count: Symbol<unsafe extern "C" fn(*mut c_int) -> HipError> =
        unsafe { lib.get(b"hipGetDeviceCount\0") }.map_err(|e| {
            MlError::Inference(InferenceError::GpuNotAvailable {
                message: format!("Failed to load hipGetDeviceCount: {}", e),
            })
        })?;

    // Get device count
    let mut count: c_int = 0;
    let result = unsafe { hip_get_device_count(&mut count) };
    if result != HIP_SUCCESS {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("hipGetDeviceCount failed with error code: {}", result),
        }));
    }

    debug!("Found {} ROCm device(s)", count);

    // Load hipGetDeviceProperties function
    let hip_get_device_properties: Symbol<
        unsafe extern "C" fn(*mut HipDeviceProp, c_int) -> HipError,
    > = unsafe { lib.get(b"hipGetDeviceProperties\0") }.map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to load hipGetDeviceProperties: {}", e),
        })
    })?;

    // Load hipMemGetInfo function (optional - may not be available without active context)
    let hip_mem_get_info: Option<Symbol<unsafe extern "C" fn(*mut usize, *mut usize) -> HipError>> =
        unsafe { lib.get(b"hipMemGetInfo\0").ok() };

    // Load hipSetDevice to set device context (optional)
    let hip_set_device: Option<Symbol<unsafe extern "C" fn(c_int) -> HipError>> =
        unsafe { lib.get(b"hipSetDevice\0").ok() };

    // Enumerate devices and query their properties
    let mut devices = Vec::new();
    for i in 0..count {
        // Query device properties
        let mut props: HipDeviceProp = unsafe { std::mem::zeroed() };
        let result = unsafe { hip_get_device_properties(&mut props, i) };
        if result != HIP_SUCCESS {
            debug!(
                "Failed to get properties for ROCm device {}: error code {}",
                i, result
            );
            continue;
        }

        // Extract device name
        let device_name = unsafe {
            std::ffi::CStr::from_ptr(props.name.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        let total_memory = props.totalGlobalMem;

        // Try to get free memory if possible
        let free_memory = if let (Some(set_dev), Some(get_info)) =
            (hip_set_device.as_ref(), hip_mem_get_info.as_ref())
        {
            // Try to set device context
            let set_result = unsafe { set_dev(i) };
            if set_result == HIP_SUCCESS {
                let mut free: usize = 0;
                let mut total: usize = 0;
                let result = unsafe { get_info(&mut free, &mut total) };
                if result == HIP_SUCCESS {
                    free
                } else {
                    // Estimate 95% available if query fails
                    (total_memory as f64 * 0.95) as usize
                }
            } else {
                // Estimate 95% available if context creation fails
                (total_memory as f64 * 0.95) as usize
            }
        } else {
            // Estimate 95% available if functions not available
            (total_memory as f64 * 0.95) as usize
        };

        // Extract GCN architecture name (compute capability equivalent)
        let compute_capability = unsafe {
            let gcn_name = std::ffi::CStr::from_ptr(props.gcnArchName.as_ptr())
                .to_string_lossy()
                .into_owned();
            if gcn_name.is_empty() {
                format!("{}.{}", props.major, props.minor)
            } else {
                gcn_name
            }
        };

        devices.push(GpuDevice {
            id: i as usize,
            name: device_name,
            total_memory,
            free_memory,
            compute_capability,
            backend: GpuBackend::Rocm,
        });
    }

    Ok(devices)
}

// ============================================================================
// DirectML Backend Implementation (Windows)
// ============================================================================

/// Checks if DirectML is available on the system
fn check_directml_available() -> bool {
    #[cfg(all(feature = "directml", target_os = "windows"))]
    {
        directml_check_runtime()
    }
    #[cfg(not(all(feature = "directml", target_os = "windows")))]
    false
}

#[cfg(all(feature = "directml", target_os = "windows"))]
fn directml_check_runtime() -> bool {
    use libloading::Library;

    // Check Windows version - DirectML requires Windows 10 1903+ (build 18362)
    // Using RtlGetVersion as GetVersionEx is deprecated and may return incorrect values
    #[cfg(windows)]
    {
        use std::mem;

        #[repr(C)]
        struct OsVersionInfoExW {
            dw_os_version_info_size: u32,
            dw_major_version: u32,
            dw_minor_version: u32,
            dw_build_number: u32,
            dw_platform_id: u32,
            sz_csd_version: [u16; 128],
            w_service_pack_major: u16,
            w_service_pack_minor: u16,
            w_suite_mask: u16,
            w_product_type: u8,
            w_reserved: u8,
        }

        // Try to load ntdll.dll and get version info
        if let Ok(ntdll) = unsafe { Library::new("ntdll.dll") } {
            type RtlGetVersion = unsafe extern "system" fn(*mut OsVersionInfoExW) -> i32;
            if let Ok(rtl_get_version) = unsafe { ntdll.get::<RtlGetVersion>(b"RtlGetVersion\0") } {
                let mut version_info: OsVersionInfoExW = unsafe { mem::zeroed() };
                version_info.dw_os_version_info_size = mem::size_of::<OsVersionInfoExW>() as u32;

                let status = unsafe { rtl_get_version(&mut version_info) };
                if status == 0 {
                    // Check for Windows 10 1903+ (10.0.18362)
                    let is_compatible = version_info.dw_major_version > 10
                        || (version_info.dw_major_version == 10
                            && version_info.dw_build_number >= 18362);

                    if !is_compatible {
                        debug!(
                            "Windows version {}.{}.{} is too old for DirectML (requires 10.0.18362+)",
                            version_info.dw_major_version,
                            version_info.dw_minor_version,
                            version_info.dw_build_number
                        );
                        return false;
                    }
                }
            }
        }
    }

    // Try to load DirectML.dll
    let directml_names = vec!["DirectML.dll", "DirectML.Debug.dll"];

    for lib_name in directml_names {
        if let Ok(lib) = unsafe { Library::new(lib_name) } {
            // Verify minimum DirectML version by checking exports
            let has_v1_0 = unsafe { lib.get::<*const ()>(b"DMLCreateDevice\0") }.is_ok();
            let has_v1_1 = unsafe { lib.get::<*const ()>(b"DMLCreateDevice1\0") }.is_ok();

            if has_v1_0 || has_v1_1 {
                debug!("Found DirectML library: {}", lib_name);
                return true;
            } else {
                debug!(
                    "DirectML library {} found but missing required exports",
                    lib_name
                );
            }
        }
    }

    debug!("DirectML library not found");
    false
}

/// Enumerates DirectML devices
fn enumerate_directml_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(all(feature = "directml", target_os = "windows"))]
    {
        directml_enumerate_devices_impl()
    }
    #[cfg(not(all(feature = "directml", target_os = "windows")))]
    {
        Ok(Vec::new())
    }
}

#[cfg(all(feature = "directml", target_os = "windows"))]
fn directml_enumerate_devices_impl() -> Result<Vec<GpuDevice>> {
    use libloading::{Library, Symbol};
    use std::ffi::c_void;

    // COM GUID structure
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    // DXGI_ADAPTER_DESC1 structure (simplified)
    #[repr(C)]
    struct DxgiAdapterDesc1 {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        sub_sys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        adapter_luid: [u8; 8],
        flags: u32,
    }

    // HRESULT type
    type HResult = i32;
    const S_OK: HResult = 0;
    const DXGI_ERROR_NOT_FOUND: HResult = -2005270526; // 0x887A0002

    // Feature level
    const D3D_FEATURE_LEVEL_11_0: u32 = 0xb000;

    // IID for DXGI interfaces
    const IID_IDXGI_FACTORY1: Guid = Guid {
        data1: 0x770aae78,
        data2: 0xf26f,
        data3: 0x4dba,
        data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
    };

    // Try to load dxgi.dll
    let dxgi_lib = unsafe { Library::new("dxgi.dll") }.map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to load dxgi.dll: {}", e),
        })
    })?;

    // Load CreateDXGIFactory1
    let create_factory: Symbol<
        unsafe extern "system" fn(*const Guid, *mut *mut c_void) -> HResult,
    > = unsafe { dxgi_lib.get(b"CreateDXGIFactory1\0") }.map_err(|e| {
        MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to load CreateDXGIFactory1: {}", e),
        })
    })?;

    // Create DXGI factory
    let mut factory: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { create_factory(&IID_IDXGI_FACTORY1, &mut factory) };
    if hr != S_OK || factory.is_null() {
        return Err(MlError::Inference(InferenceError::GpuNotAvailable {
            message: format!("Failed to create DXGI factory: HRESULT = 0x{:08X}", hr),
        }));
    }

    // IDXGIFactory1 vtable offsets (simplified)
    // EnumAdapters1 is at offset 7 in the vtable
    type EnumAdapters1Fn = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HResult;

    let vtable = unsafe { *(factory as *const *const c_void) };
    let enum_adapters1: EnumAdapters1Fn =
        unsafe { std::mem::transmute(*((vtable as *const *const c_void).offset(7))) };

    let mut devices = Vec::new();
    let mut adapter_index = 0u32;

    loop {
        let mut adapter: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { enum_adapters1(factory, adapter_index, &mut adapter) };

        if hr == DXGI_ERROR_NOT_FOUND {
            break;
        }

        if hr != S_OK || adapter.is_null() {
            break;
        }

        // IDXGIAdapter1::GetDesc1 is at offset 10 in the vtable
        type GetDesc1Fn = unsafe extern "system" fn(*mut c_void, *mut DxgiAdapterDesc1) -> HResult;

        let adapter_vtable = unsafe { *(adapter as *const *const c_void) };
        let get_desc1: GetDesc1Fn =
            unsafe { std::mem::transmute(*((adapter_vtable as *const *const c_void).offset(10))) };

        let mut desc: DxgiAdapterDesc1 = unsafe { std::mem::zeroed() };
        let hr = unsafe { get_desc1(adapter, &mut desc) };

        if hr == S_OK {
            // Convert UTF-16 description to String
            let description = String::from_utf16_lossy(&desc.description)
                .trim_end_matches('\0')
                .to_string();

            // Only include adapters with dedicated video memory (discrete GPUs)
            if desc.dedicated_video_memory > 0 {
                devices.push(GpuDevice {
                    id: adapter_index as usize,
                    name: description,
                    total_memory: desc.dedicated_video_memory,
                    free_memory: (desc.dedicated_video_memory as f64 * 0.95) as usize,
                    compute_capability: "DirectML".to_string(),
                    backend: GpuBackend::DirectMl,
                });
            }
        }

        // Release adapter
        type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
        let release: ReleaseFn = unsafe {
            let adapter_vtable = *(adapter as *const *const c_void);
            std::mem::transmute(*((adapter_vtable as *const *const c_void).offset(2)))
        };
        unsafe { release(adapter) };

        adapter_index += 1;
    }

    // Release factory
    type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
    let release: ReleaseFn =
        unsafe { std::mem::transmute(*((vtable as *const *const c_void).offset(2))) };
    unsafe { release(factory) };

    debug!("Found {} DirectML-compatible device(s)", devices.len());
    Ok(devices)
}

// ============================================================================
// WebGPU Backend Implementation
// ============================================================================

/// Checks if WebGPU is available
fn check_webgpu_available() -> bool {
    #[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
    {
        webgpu_check_available()
    }
    #[cfg(not(all(feature = "webgpu", target_arch = "wasm32")))]
    false
}

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
fn webgpu_check_available() -> bool {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;
    use web_sys::window;

    // Get the window object
    let window = match window() {
        Some(w) => w,
        None => {
            debug!("WebGPU: window object not available");
            return false;
        }
    };

    // Get the navigator object
    let navigator = window.navigator();
    let navigator_val = JsValue::from(&navigator);

    // Check if navigator has a 'gpu' property
    let gpu_key = JsValue::from_str("gpu");
    match Reflect::has(&navigator_val, &gpu_key) {
        Ok(has_gpu) => {
            if !has_gpu {
                debug!("WebGPU: navigator.gpu property not found");
                return false;
            }
        }
        Err(e) => {
            debug!("WebGPU: Failed to check for gpu property: {:?}", e);
            return false;
        }
    }

    // Get the gpu object
    let gpu = match Reflect::get(&navigator_val, &gpu_key) {
        Ok(g) => g,
        Err(e) => {
            debug!("WebGPU: Failed to get gpu object: {:?}", e);
            return false;
        }
    };

    // Check if gpu is defined (not null/undefined)
    if gpu.is_null() || gpu.is_undefined() {
        debug!("WebGPU: navigator.gpu is null or undefined");
        return false;
    }

    // Check if requestAdapter exists on the gpu object
    let request_adapter_key = JsValue::from_str("requestAdapter");
    match Reflect::has(&gpu, &request_adapter_key) {
        Ok(has_request_adapter) => {
            if !has_request_adapter {
                debug!("WebGPU: requestAdapter method not found");
                return false;
            }
        }
        Err(e) => {
            debug!("WebGPU: Failed to check for requestAdapter: {:?}", e);
            return false;
        }
    }

    debug!("WebGPU is available");
    true
}

/// Enumerates WebGPU devices
fn enumerate_webgpu_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
    {
        // WebGPU device enumeration requires async operations which cannot be
        // done in a synchronous context. Instead, we return a placeholder device
        // if WebGPU is available. Applications should use the async WebGPU APIs
        // directly for proper adapter enumeration.
        //
        // For a proper implementation, applications should:
        // 1. Use wasm-bindgen-futures to handle Promises
        // 2. Call navigator.gpu.requestAdapter() with different power preferences
        // 3. Query adapter.info for device information
        // 4. Query adapter.limits for capability information
        //
        // Since this function is synchronous and WebGPU is inherently async,
        // we provide a simplified implementation that checks availability.
        if webgpu_check_available() {
            debug!("WebGPU is available (async enumeration not supported in sync context)");
            Ok(vec![GpuDevice {
                id: 0,
                name: "WebGPU Device (async enumeration required)".to_string(),
                total_memory: 0, // Unknown without async query
                free_memory: 0,  // Unknown without async query
                compute_capability: "WebGPU".to_string(),
                backend: GpuBackend::WebGpu,
            }])
        } else {
            Ok(Vec::new())
        }
    }
    #[cfg(not(all(feature = "webgpu", target_arch = "wasm32")))]
    {
        Ok(Vec::new())
    }
}

/// GPU memory statistics
#[derive(Debug, Clone, Default)]
pub struct GpuMemoryStats {
    /// Total memory allocated in bytes
    pub allocated: usize,
    /// Peak memory usage in bytes
    pub peak: usize,
    /// Number of allocations
    pub num_allocations: usize,
    /// Number of deallocations
    pub num_deallocations: usize,
}

impl GpuMemoryStats {
    /// Returns the current memory usage
    #[must_use]
    pub fn current_usage(&self) -> usize {
        self.allocated
    }

    /// Returns the number of active allocations
    #[must_use]
    pub fn active_allocations(&self) -> usize {
        self.num_allocations.saturating_sub(self.num_deallocations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_config_builder() {
        let config = GpuConfig::builder()
            .backend(GpuBackend::Cuda)
            .device_id(1)
            .mixed_precision(false)
            .tensor_cores(false)
            .memory_growth(false)
            .memory_fraction(0.8)
            .build();

        assert_eq!(config.backend, Some(GpuBackend::Cuda));
        assert_eq!(config.device_id, Some(1));
        assert!(!config.mixed_precision);
        assert!(!config.tensor_cores);
        assert!(!config.memory_growth);
        assert!((config.memory_fraction - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_memory_fraction_clamping() {
        let config1 = GpuConfig::builder().memory_fraction(1.5).build();
        assert!((config1.memory_fraction - 1.0).abs() < 1e-6);

        let config2 = GpuConfig::builder().memory_fraction(-0.5).build();
        assert!((config2.memory_fraction - 0.1).abs() < 1e-6);
    }

    #[test]
    fn test_gpu_device_memory_utilization() {
        let device = GpuDevice {
            id: 0,
            name: "Test GPU".to_string(),
            total_memory: 8_000_000_000, // 8 GB
            free_memory: 2_000_000_000,  // 2 GB free
            compute_capability: "8.0".to_string(),
            backend: GpuBackend::Cuda,
        };

        // 6 GB used out of 8 GB = 75%
        let utilization = device.memory_utilization();
        assert!((utilization - 75.0).abs() < 1.0);

        assert!(device.has_sufficient_memory(1_000_000_000)); // 1 GB
        assert!(!device.has_sufficient_memory(3_000_000_000)); // 3 GB
    }

    #[test]
    fn test_backend_names() {
        assert_eq!(GpuBackend::Cuda.name(), "CUDA");
        assert_eq!(GpuBackend::Rocm.name(), "ROCm");
        assert_eq!(GpuBackend::DirectMl.name(), "DirectML");
        assert_eq!(GpuBackend::Vulkan.name(), "Vulkan");
        assert_eq!(GpuBackend::WebGpu.name(), "WebGPU");
        assert_eq!(GpuBackend::Metal.name(), "Metal");
        assert_eq!(GpuBackend::OpenCl.name(), "OpenCL");
    }

    #[test]
    fn test_gpu_memory_stats() {
        let stats = GpuMemoryStats {
            allocated: 1000,
            peak: 1500,
            num_allocations: 10,
            num_deallocations: 3,
        };

        assert_eq!(stats.current_usage(), 1000);
        assert_eq!(stats.active_allocations(), 7);
    }

    #[test]
    fn test_backend_availability() {
        // Test that backend availability checks don't panic
        let _cuda_available = GpuBackend::Cuda.is_available();
        let _metal_available = GpuBackend::Metal.is_available();
        let _vulkan_available = GpuBackend::Vulkan.is_available();
        let _opencl_available = GpuBackend::OpenCl.is_available();
        let _rocm_available = GpuBackend::Rocm.is_available();
        let _directml_available = GpuBackend::DirectMl.is_available();
        let _webgpu_available = GpuBackend::WebGpu.is_available();
    }

    #[test]
    fn test_list_devices() {
        // Test that list_devices doesn't panic (may return empty on systems without GPUs)
        let result = list_devices();
        assert!(result.is_ok());
        let devices = result.ok().unwrap_or_default();

        // If devices are found, verify their properties
        for device in devices {
            assert!(!device.name.is_empty());
            // total_memory is always a valid u64 value
            let _ = device.total_memory;
        }
    }

    #[test]
    fn test_select_device_no_gpu() {
        let config = GpuConfig::default();
        // This may fail on systems without GPU, which is expected
        let _result = select_device(&config);
    }

    #[test]
    fn test_device_enumeration_without_features() {
        // Without features enabled, these should return empty vectors
        #[cfg(not(feature = "cuda"))]
        {
            let cuda_devices = enumerate_cuda_devices();
            assert!(cuda_devices.is_ok());
            assert!(cuda_devices.ok().is_none_or(|d| d.is_empty()));
        }

        #[cfg(not(feature = "metal"))]
        {
            let metal_devices = enumerate_metal_devices();
            assert!(metal_devices.is_ok());
            assert!(metal_devices.ok().is_none_or(|d| d.is_empty()));
        }

        #[cfg(not(feature = "vulkan"))]
        {
            let vulkan_devices = enumerate_vulkan_devices();
            assert!(vulkan_devices.is_ok());
            assert!(vulkan_devices.ok().is_none_or(|d| d.is_empty()));
        }

        #[cfg(not(feature = "opencl"))]
        {
            let opencl_devices = enumerate_opencl_devices();
            assert!(opencl_devices.is_ok());
            assert!(opencl_devices.ok().is_none_or(|d| d.is_empty()));
        }
    }

    #[test]
    #[cfg(all(feature = "metal", target_os = "macos"))]
    fn test_metal_enumeration() {
        // On macOS with Metal feature, we should be able to enumerate devices
        let devices = enumerate_metal_devices();
        assert!(devices.is_ok());

        if let Ok(devs) = devices {
            if !devs.is_empty() {
                for device in devs {
                    assert!(!device.name.is_empty());
                    assert_eq!(device.backend, GpuBackend::Metal);
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "cuda")]
    fn test_cuda_detection() {
        // Test CUDA detection without panicking
        let available = check_cuda_available();

        // If CUDA is available, try to enumerate devices
        if available {
            let devices = enumerate_cuda_devices();
            assert!(devices.is_ok());

            if let Ok(devs) = devices {
                for device in devs {
                    assert_eq!(device.backend, GpuBackend::Cuda);
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "vulkan")]
    fn test_vulkan_detection() {
        // Test Vulkan detection without panicking
        let available = check_vulkan_available();

        // If Vulkan is available, try to enumerate devices
        if available {
            let devices = enumerate_vulkan_devices();
            assert!(devices.is_ok());

            if let Ok(devs) = devices {
                for device in devs {
                    assert_eq!(device.backend, GpuBackend::Vulkan);
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "opencl")]
    fn test_opencl_detection() {
        // Test OpenCL detection without panicking
        let available = check_opencl_available();

        // If OpenCL is available, try to enumerate devices
        if available {
            let devices = enumerate_opencl_devices();
            assert!(devices.is_ok());

            if let Ok(devs) = devices {
                for device in devs {
                    assert_eq!(device.backend, GpuBackend::OpenCl);
                }
            }
        }
    }

    #[test]
    fn test_device_selection_with_backend_filter() {
        let devices = list_devices().ok().unwrap_or_default();

        if !devices.is_empty() {
            let first_backend = devices[0].backend;
            let config = GpuConfig::builder().backend(first_backend).build();

            let result = select_device(&config);

            if result.is_ok() {
                let device = result.ok().unwrap_or_else(|| devices[0].clone());
                assert_eq!(device.backend, first_backend);
            }
        }
    }

    #[test]
    fn test_zero_memory_device_utilization() {
        let device = GpuDevice {
            id: 0,
            name: "Zero Memory Device".to_string(),
            total_memory: 0,
            free_memory: 0,
            compute_capability: "0.0".to_string(),
            backend: GpuBackend::Cuda,
        };

        assert_eq!(device.memory_utilization(), 0.0);
        assert!(!device.has_sufficient_memory(1));
    }
}
