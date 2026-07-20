//! Vulkan-specific optimizations for cross-platform GPU computing.
//!
//! This module provides Vulkan-specific features including subgroup operations,
//! timeline semaphores, and push constants.

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use std::collections::HashMap;
use tracing::{debug, info};

/// Vulkan optimization configuration.
#[derive(Debug, Clone)]
pub struct VulkanOptimizationConfig {
    /// Enable subgroup operations.
    pub enable_subgroup_ops: bool,
    /// Enable push constants.
    pub enable_push_constants: bool,
    /// Enable timeline semaphores for synchronization.
    pub enable_timeline_semaphores: bool,
    /// Descriptor set pool size.
    pub descriptor_pool_size: u32,
    /// Enable async compute.
    pub enable_async_compute: bool,
}

impl Default for VulkanOptimizationConfig {
    fn default() -> Self {
        Self {
            enable_subgroup_ops: true,
            enable_push_constants: true,
            enable_timeline_semaphores: true,
            descriptor_pool_size: 1000,
            enable_async_compute: true,
        }
    }
}

/// Vulkan feature detector.
pub struct VulkanFeatureDetector {
    features: VulkanFeatures,
}

#[derive(Debug, Clone)]
pub struct VulkanFeatures {
    /// Subgroup size (wave size).
    pub subgroup_size: u32,
    /// Supports subgroup arithmetic operations.
    pub subgroup_arithmetic: bool,
    /// Supports subgroup ballot.
    pub subgroup_ballot: bool,
    /// Supports subgroup shuffle.
    pub subgroup_shuffle: bool,
    /// Supports timeline semaphores.
    pub timeline_semaphores: bool,
    /// Maximum push constants size.
    pub max_push_constants_size: u32,
    /// Supports async compute.
    pub async_compute: bool,
}

impl Default for VulkanFeatures {
    fn default() -> Self {
        Self {
            subgroup_size: 32,
            subgroup_arithmetic: true,
            subgroup_ballot: true,
            subgroup_shuffle: true,
            timeline_semaphores: true,
            max_push_constants_size: 128,
            async_compute: true,
        }
    }
}

impl VulkanFeatureDetector {
    /// Create a new feature detector.
    pub fn new(context: &GpuContext) -> Self {
        let features = Self::detect_features(context);
        info!(
            "Vulkan features: subgroup_size={}, arithmetic={}, ballot={}, shuffle={}",
            features.subgroup_size,
            features.subgroup_arithmetic,
            features.subgroup_ballot,
            features.subgroup_shuffle
        );

        Self { features }
    }

    /// Get detected features.
    pub fn features(&self) -> &VulkanFeatures {
        &self.features
    }

    fn detect_features(context: &GpuContext) -> VulkanFeatures {
        // Query the *actual* device feature set rather than assuming a
        // conservative default.  `wgpu::Features::SUBGROUP` is the portable
        // signal that the driver exposes subgroup arithmetic / ballot / shuffle
        // intrinsics in compute shaders; when it is absent we fall back to the
        // workgroup-shared-memory emulation emitted by [`SubgroupOptimizer`].
        let device_features = context.device().features();
        let subgroup = device_features.contains(wgpu::Features::SUBGROUP);

        // The adapter reports the min/max hardware subgroup (wave) width.  We
        // use the max as the representative size for the generated
        // `SUBGROUP_SIZE` constant (32 on most desktop and Apple GPUs); guard
        // against a bogus zero.
        let adapter_info = context.adapter_info();
        let subgroup_size = adapter_info.subgroup_max_size.max(1);

        VulkanFeatures {
            subgroup_size,
            subgroup_arithmetic: subgroup,
            subgroup_ballot: subgroup,
            subgroup_shuffle: subgroup,
            timeline_semaphores: true,
            max_push_constants_size: 128,
            async_compute: true,
        }
    }
}

/// Vulkan subgroup operations optimizer.
pub struct SubgroupOptimizer {
    features: VulkanFeatures,
    config: VulkanOptimizationConfig,
}

impl SubgroupOptimizer {
    /// Create a new subgroup optimizer.
    pub fn new(features: VulkanFeatures, config: VulkanOptimizationConfig) -> Self {
        Self { features, config }
    }

    /// Optimize shader code with subgroup operations.
    ///
    /// Appends a set of `subgroup_*` WGSL helper functions to `shader_code`.
    /// The concrete implementation depends on the detected features:
    ///
    /// * When `VulkanFeatures::subgroup_arithmetic` (respectively
    ///   `VulkanFeatures::subgroup_ballot`) is `true` — i.e. the device was
    ///   created with `wgpu::Features::SUBGROUP` — the helpers wrap the
    ///   **native** WGSL subgroup built-ins (`subgroupAdd`, `subgroupShuffle`,
    ///   `subgroupBallot`, …).  These reduce/scan across the hardware
    ///   *subgroup* (wave) and are the fast path.
    /// * Otherwise the helpers fall back to a **workgroup-shared-memory
    ///   emulation** built on `workgroupBarrier()`.  The emulation reduces
    ///   across the whole *workgroup* (not a subgroup) and is correct for a 1-D
    ///   workgroup of up to [`Self::EMU_MAX`] invocations.
    ///
    /// Every generated helper takes `(…, lid: u32, n: u32)` where `lid` is the
    /// caller's `local_invocation_index` and `n` is the number of active
    /// invocations in the workgroup.  The native helpers ignore `lid`/`n`; the
    /// emulated helpers use them to index the shared scratch buffers.  This
    /// uniform signature lets shader authors switch paths without editing call
    /// sites.
    pub fn optimize_shader(&self, shader_code: &str) -> String {
        if !self.config.enable_subgroup_ops {
            return shader_code.to_string();
        }

        let mut prologue = String::new();

        // Add subgroup size constant.
        if !shader_code.contains("SUBGROUP_SIZE") {
            prologue.push_str(&format!(
                "const SUBGROUP_SIZE: u32 = {}u;\n",
                self.features.subgroup_size
            ));
        }

        // If either helper family will use the emulation path, declare the
        // shared scratch buffers exactly once at module scope.
        let needs_emulation = !self.features.subgroup_arithmetic || !self.features.subgroup_ballot;
        if needs_emulation {
            prologue.push_str(Self::emulation_scratch_decl());
        }

        let mut helpers = String::new();
        helpers.push_str(&Self::subgroup_arithmetic_helpers(
            self.features.subgroup_arithmetic,
        ));
        helpers.push_str(&Self::subgroup_ballot_helpers(
            self.features.subgroup_ballot,
        ));

        // Emit the module-scope prologue and helper definitions *before* the
        // user shader so every `subgroup_*` call is preceded by its definition.
        format!("{prologue}{helpers}\n{shader_code}")
    }

    /// Upper bound on the workgroup size supported by the emulated helpers.
    ///
    /// The emulation scratch buffers are sized to this many `f32`/`u32`
    /// elements; the `n` argument passed to each helper must not exceed it.
    pub const EMU_MAX: u32 = 256;

    /// Module-scope declarations backing the emulated subgroup helpers.
    fn emulation_scratch_decl() -> &'static str {
        r#"
// Workgroup-shared scratch backing the emulated subgroup helpers.
const SUBGROUP_EMU_MAX: u32 = 256u;
var<workgroup> sg_emu_scratch: array<f32, SUBGROUP_EMU_MAX>;
var<workgroup> sg_emu_flags: array<u32, SUBGROUP_EMU_MAX>;
"#
    }

    /// Emit the six subgroup arithmetic helpers.
    ///
    /// `native == true` maps each helper onto the corresponding WGSL subgroup
    /// built-in.  `native == false` emits a `workgroupBarrier()`-synchronised
    /// tree reduction / prefix scan across the workgroup with identical
    /// semantics *within a workgroup*.
    fn subgroup_arithmetic_helpers(native: bool) -> String {
        if native {
            r#"
// Native subgroup arithmetic (device created with Features::SUBGROUP).
// Reductions/scans run across the hardware subgroup; `lid`/`n` are unused.
fn subgroup_add(value: f32, lid: u32, n: u32) -> f32 { return subgroupAdd(value); }
fn subgroup_mul(value: f32, lid: u32, n: u32) -> f32 { return subgroupMul(value); }
fn subgroup_min(value: f32, lid: u32, n: u32) -> f32 { return subgroupMin(value); }
fn subgroup_max(value: f32, lid: u32, n: u32) -> f32 { return subgroupMax(value); }
fn subgroup_inclusive_add(value: f32, lid: u32, n: u32) -> f32 { return subgroupInclusiveAdd(value); }
fn subgroup_exclusive_add(value: f32, lid: u32, n: u32) -> f32 { return subgroupExclusiveAdd(value); }
"#
            .to_string()
        } else {
            r#"
// Emulated subgroup arithmetic — workgroup-wide reduction/scan via shared
// memory + workgroupBarrier().  Semantics match the native builtins but over
// the whole 1-D workgroup (n active invocations, n <= SUBGROUP_EMU_MAX).
fn subgroup_add(value: f32, lid: u32, n: u32) -> f32 {
    sg_emu_scratch[lid] = value;
    workgroupBarrier();
    var stride = 1u;
    loop {
        if (stride >= n) { break; }
        let idx = lid * stride * 2u;
        if (idx + stride < n) {
            sg_emu_scratch[idx] = sg_emu_scratch[idx] + sg_emu_scratch[idx + stride];
        }
        stride = stride * 2u;
        workgroupBarrier();
    }
    let result = sg_emu_scratch[0];
    workgroupBarrier();
    return result;
}
fn subgroup_mul(value: f32, lid: u32, n: u32) -> f32 {
    sg_emu_scratch[lid] = value;
    workgroupBarrier();
    var stride = 1u;
    loop {
        if (stride >= n) { break; }
        let idx = lid * stride * 2u;
        if (idx + stride < n) {
            sg_emu_scratch[idx] = sg_emu_scratch[idx] * sg_emu_scratch[idx + stride];
        }
        stride = stride * 2u;
        workgroupBarrier();
    }
    let result = sg_emu_scratch[0];
    workgroupBarrier();
    return result;
}
fn subgroup_min(value: f32, lid: u32, n: u32) -> f32 {
    sg_emu_scratch[lid] = value;
    workgroupBarrier();
    var stride = 1u;
    loop {
        if (stride >= n) { break; }
        let idx = lid * stride * 2u;
        if (idx + stride < n) {
            sg_emu_scratch[idx] = min(sg_emu_scratch[idx], sg_emu_scratch[idx + stride]);
        }
        stride = stride * 2u;
        workgroupBarrier();
    }
    let result = sg_emu_scratch[0];
    workgroupBarrier();
    return result;
}
fn subgroup_max(value: f32, lid: u32, n: u32) -> f32 {
    sg_emu_scratch[lid] = value;
    workgroupBarrier();
    var stride = 1u;
    loop {
        if (stride >= n) { break; }
        let idx = lid * stride * 2u;
        if (idx + stride < n) {
            sg_emu_scratch[idx] = max(sg_emu_scratch[idx], sg_emu_scratch[idx + stride]);
        }
        stride = stride * 2u;
        workgroupBarrier();
    }
    let result = sg_emu_scratch[0];
    workgroupBarrier();
    return result;
}
fn subgroup_inclusive_add(value: f32, lid: u32, n: u32) -> f32 {
    sg_emu_scratch[lid] = value;
    workgroupBarrier();
    var acc = 0.0;
    for (var i = 0u; i <= lid; i = i + 1u) {
        acc = acc + sg_emu_scratch[i];
    }
    workgroupBarrier();
    return acc;
}
fn subgroup_exclusive_add(value: f32, lid: u32, n: u32) -> f32 {
    sg_emu_scratch[lid] = value;
    workgroupBarrier();
    var acc = 0.0;
    for (var i = 0u; i < lid; i = i + 1u) {
        acc = acc + sg_emu_scratch[i];
    }
    workgroupBarrier();
    return acc;
}
"#
            .to_string()
        }
    }

    /// Emit the subgroup ballot / vote helpers.
    ///
    /// `subgroup_ballot` returns the **number of invocations** whose predicate
    /// is `true` (a popcount of the ballot), which is well-defined and has the
    /// same `u32` type on both paths — a raw per-lane bit-mask is only
    /// meaningful within a single hardware subgroup and is therefore not
    /// exposed by the emulation.
    fn subgroup_ballot_helpers(native: bool) -> String {
        if native {
            r#"
// Native subgroup ballot / vote (device created with Features::SUBGROUP).
fn subgroup_all(predicate: bool, lid: u32, n: u32) -> bool { return subgroupAll(predicate); }
fn subgroup_any(predicate: bool, lid: u32, n: u32) -> bool { return subgroupAny(predicate); }
fn subgroup_ballot(predicate: bool, lid: u32, n: u32) -> u32 {
    let b = subgroupBallot(predicate);
    return countOneBits(b.x) + countOneBits(b.y) + countOneBits(b.z) + countOneBits(b.w);
}
"#
            .to_string()
        } else {
            r#"
// Emulated subgroup ballot / vote — workgroup-wide via shared flags buffer.
fn subgroup_all(predicate: bool, lid: u32, n: u32) -> bool {
    sg_emu_flags[lid] = select(0u, 1u, predicate);
    workgroupBarrier();
    var acc = 1u;
    for (var i = 0u; i < n; i = i + 1u) { acc = acc & sg_emu_flags[i]; }
    workgroupBarrier();
    return acc != 0u;
}
fn subgroup_any(predicate: bool, lid: u32, n: u32) -> bool {
    sg_emu_flags[lid] = select(0u, 1u, predicate);
    workgroupBarrier();
    var acc = 0u;
    for (var i = 0u; i < n; i = i + 1u) { acc = acc | sg_emu_flags[i]; }
    workgroupBarrier();
    return acc != 0u;
}
fn subgroup_ballot(predicate: bool, lid: u32, n: u32) -> u32 {
    sg_emu_flags[lid] = select(0u, 1u, predicate);
    workgroupBarrier();
    var acc = 0u;
    for (var i = 0u; i < n; i = i + 1u) { acc = acc + sg_emu_flags[i]; }
    workgroupBarrier();
    return acc;
}
"#
            .to_string()
        }
    }
}

/// Vulkan push constants manager for fast parameter updates.
pub struct PushConstantsManager {
    max_size: u32,
    constants: HashMap<String, PushConstant>,
}

#[derive(Debug, Clone)]
struct PushConstant {
    name: String,
    offset: u32,
    size: u32,
    data: Vec<u8>,
}

impl PushConstantsManager {
    /// Create a new push constants manager.
    pub fn new(max_size: u32) -> Self {
        Self {
            max_size,
            constants: HashMap::new(),
        }
    }

    /// Register a push constant.
    ///
    /// # Errors
    ///
    /// Returns an error if constant exceeds max size.
    pub fn register(&mut self, name: String, size: u32) -> GpuResult<()> {
        let offset = self.calculate_next_offset();

        if offset + size > self.max_size {
            return Err(GpuError::invalid_buffer(format!(
                "Push constant exceeds maximum size: {} + {} > {}",
                offset, size, self.max_size
            )));
        }

        self.constants.insert(
            name.clone(),
            PushConstant {
                name,
                offset,
                size,
                data: vec![0; size as usize],
            },
        );

        Ok(())
    }

    /// Update push constant data.
    ///
    /// # Errors
    ///
    /// Returns an error if constant not found or data size mismatch.
    pub fn update(&mut self, name: &str, data: &[u8]) -> GpuResult<()> {
        let constant = self
            .constants
            .get_mut(name)
            .ok_or_else(|| GpuError::invalid_buffer("Push constant not found"))?;

        if data.len() != constant.size as usize {
            return Err(GpuError::invalid_buffer("Data size mismatch"));
        }

        constant.data.copy_from_slice(data);

        debug!("Updated push constant '{}' ({} bytes)", name, data.len());

        Ok(())
    }

    /// Get total size of all push constants.
    pub fn total_size(&self) -> u32 {
        self.constants.values().map(|c| c.size).sum()
    }

    fn calculate_next_offset(&self) -> u32 {
        self.constants
            .values()
            .map(|c| c.offset + c.size)
            .max()
            .unwrap_or(0)
    }
}

/// Descriptor set pool manager for Vulkan.
pub struct DescriptorSetPool {
    pool_size: u32,
    allocated: u32,
    free_sets: Vec<u32>,
}

impl DescriptorSetPool {
    /// Create a new descriptor set pool.
    pub fn new(pool_size: u32) -> Self {
        Self {
            pool_size,
            allocated: 0,
            free_sets: Vec::new(),
        }
    }

    /// Allocate a descriptor set.
    ///
    /// # Errors
    ///
    /// Returns an error if pool is exhausted.
    pub fn allocate(&mut self) -> GpuResult<u32> {
        if let Some(set_id) = self.free_sets.pop() {
            debug!("Reused descriptor set {}", set_id);
            return Ok(set_id);
        }

        if self.allocated >= self.pool_size {
            return Err(GpuError::internal(
                "Descriptor set pool exhausted".to_string(),
            ));
        }

        let set_id = self.allocated;
        self.allocated += 1;

        debug!("Allocated descriptor set {}", set_id);

        Ok(set_id)
    }

    /// Free a descriptor set.
    pub fn free(&mut self, set_id: u32) {
        if set_id < self.allocated {
            self.free_sets.push(set_id);
            debug!("Freed descriptor set {}", set_id);
        }
    }

    /// Reset the entire pool.
    pub fn reset(&mut self) {
        self.free_sets.clear();
        for i in 0..self.allocated {
            self.free_sets.push(i);
        }
        debug!("Reset descriptor set pool");
    }

    /// Get pool statistics.
    pub fn stats(&self) -> (u32, u32, usize) {
        (self.pool_size, self.allocated, self.free_sets.len())
    }
}

/// Timeline semaphore manager for async synchronization.
pub struct TimelineSemaphoreManager {
    semaphores: HashMap<u32, TimelineSemaphore>,
    next_id: u32,
}

#[derive(Debug, Clone)]
struct TimelineSemaphore {
    id: u32,
    value: u64,
    name: String,
}

impl TimelineSemaphoreManager {
    /// Create a new timeline semaphore manager.
    pub fn new() -> Self {
        Self {
            semaphores: HashMap::new(),
            next_id: 0,
        }
    }

    /// Create a timeline semaphore.
    pub fn create(&mut self, name: String, initial_value: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        self.semaphores.insert(
            id,
            TimelineSemaphore {
                id,
                value: initial_value,
                name: name.clone(),
            },
        );

        debug!("Created timeline semaphore '{}' (ID: {})", name, id);

        id
    }

    /// Signal a semaphore with a new value.
    ///
    /// # Errors
    ///
    /// Returns an error if semaphore not found.
    pub fn signal(&mut self, id: u32, value: u64) -> GpuResult<()> {
        let sem = self
            .semaphores
            .get_mut(&id)
            .ok_or_else(|| GpuError::internal("Semaphore not found"))?;

        sem.value = value;

        debug!("Signaled semaphore '{}' with value {}", sem.name, value);

        Ok(())
    }

    /// Wait for a semaphore to reach a value.
    ///
    /// # Errors
    ///
    /// Returns an error if semaphore not found.
    pub fn wait(&self, id: u32, value: u64) -> GpuResult<bool> {
        let sem = self
            .semaphores
            .get(&id)
            .ok_or_else(|| GpuError::internal("Semaphore not found"))?;

        Ok(sem.value >= value)
    }

    /// Get current semaphore value.
    ///
    /// # Errors
    ///
    /// Returns an error if semaphore not found.
    pub fn get_value(&self, id: u32) -> GpuResult<u64> {
        let sem = self
            .semaphores
            .get(&id)
            .ok_or_else(|| GpuError::internal("Semaphore not found"))?;

        Ok(sem.value)
    }

    /// Destroy a semaphore.
    pub fn destroy(&mut self, id: u32) {
        if let Some(sem) = self.semaphores.remove(&id) {
            debug!("Destroyed timeline semaphore '{}'", sem.name);
        }
    }
}

impl Default for TimelineSemaphoreManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Vulkan async compute queue manager.
pub struct AsyncComputeQueue {
    compute_queue: Option<QueueHandle>,
    graphics_queue: Option<QueueHandle>,
    transfer_queue: Option<QueueHandle>,
}

#[derive(Debug, Clone)]
struct QueueHandle {
    family_index: u32,
    queue_index: u32,
}

impl AsyncComputeQueue {
    /// Create a new async compute queue manager.
    pub fn new() -> Self {
        Self {
            compute_queue: Some(QueueHandle {
                family_index: 0,
                queue_index: 0,
            }),
            graphics_queue: Some(QueueHandle {
                family_index: 0,
                queue_index: 0,
            }),
            transfer_queue: None,
        }
    }

    /// Check if async compute is available.
    pub fn is_available(&self) -> bool {
        self.compute_queue.is_some()
    }

    /// Submit a recorded command payload to the async compute queue.
    ///
    /// # Note
    ///
    /// This crate executes all GPU work through `wgpu`, which exposes a
    /// **single unified queue** and does not surface the separate
    /// async-compute / graphics / transfer queue families that raw Vulkan
    /// does. The `&[u8]` command payload accepted here also has no
    /// representation in `wgpu`'s command-buffer model, so genuine
    /// asynchronous multi-queue submission cannot be performed on this
    /// backend. Rather than silently return `Ok(())` and let callers believe
    /// GPU work was dispatched, this reports an explicit error. Use
    /// [`crate::GpuContext`] / [`crate::ComputePipeline`] for real execution.
    ///
    /// # Errors
    ///
    /// Always returns [`GpuError::unsupported_operation`]: either no compute
    /// queue family was detected, or async multi-queue submission is not
    /// implemented on the `wgpu` backend.
    pub fn submit_compute(&self, _commands: &[u8]) -> GpuResult<()> {
        if self.compute_queue.is_none() {
            return Err(GpuError::unsupported_operation(
                "Compute queue not available".to_string(),
            ));
        }

        Err(GpuError::unsupported_operation(
            "async Vulkan compute-queue submission is not implemented on the wgpu backend; \
             use GpuContext/ComputePipeline for real GPU execution"
                .to_string(),
        ))
    }

    /// Submit a recorded command payload to the async graphics queue.
    ///
    /// # Note
    ///
    /// See [`AsyncComputeQueue::submit_compute`] — the same `wgpu`
    /// single-queue limitation applies; this never silently succeeds.
    ///
    /// # Errors
    ///
    /// Always returns [`GpuError::unsupported_operation`].
    pub fn submit_graphics(&self, _commands: &[u8]) -> GpuResult<()> {
        if self.graphics_queue.is_none() {
            return Err(GpuError::unsupported_operation(
                "Graphics queue not available".to_string(),
            ));
        }

        Err(GpuError::unsupported_operation(
            "async Vulkan graphics-queue submission is not implemented on the wgpu backend; \
             use GpuContext/ComputePipeline for real GPU execution"
                .to_string(),
        ))
    }

    /// Submit a recorded command payload to the async transfer queue.
    ///
    /// # Note
    ///
    /// See [`AsyncComputeQueue::submit_compute`] — the same `wgpu`
    /// single-queue limitation applies; this never silently succeeds. When no
    /// dedicated transfer queue is present it defers to the graphics queue,
    /// which also reports the unsupported-operation error.
    ///
    /// # Errors
    ///
    /// Always returns [`GpuError::unsupported_operation`].
    pub fn submit_transfer(&self, _commands: &[u8]) -> GpuResult<()> {
        if self.transfer_queue.is_none() {
            // Fall back to graphics queue (which also reports the honest error).
            return self.submit_graphics(_commands);
        }

        Err(GpuError::unsupported_operation(
            "async Vulkan transfer-queue submission is not implemented on the wgpu backend; \
             use GpuContext/ComputePipeline for real GPU execution"
                .to_string(),
        ))
    }
}

impl Default for AsyncComputeQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vulkan_features() {
        let features = VulkanFeatures::default();
        assert_eq!(features.subgroup_size, 32);
        assert!(features.subgroup_arithmetic);
        assert!(features.subgroup_ballot);
    }

    #[test]
    fn test_push_constants_manager() {
        let mut manager = PushConstantsManager::new(256);

        manager
            .register("view_matrix".to_string(), 64)
            .expect("Failed to register");
        manager
            .register("light_pos".to_string(), 16)
            .expect("Failed to register");

        let data = vec![0u8; 64];
        manager
            .update("view_matrix", &data)
            .expect("Failed to update");

        assert!(manager.total_size() <= 256);
    }

    #[test]
    fn test_descriptor_set_pool() {
        let mut pool = DescriptorSetPool::new(10);

        let set1 = pool.allocate().expect("Failed to allocate");
        let _set2 = pool.allocate().expect("Failed to allocate");

        pool.free(set1);

        let set3 = pool.allocate().expect("Failed to allocate");
        assert_eq!(set3, set1); // Should reuse freed set

        let (pool_size, allocated, free) = pool.stats();
        assert_eq!(pool_size, 10);
        assert_eq!(allocated, 2);
        assert_eq!(free, 0);
    }

    #[test]
    fn test_timeline_semaphore() {
        let mut manager = TimelineSemaphoreManager::new();

        let sem = manager.create("test_sem".to_string(), 0);

        manager.signal(sem, 5).expect("Failed to signal");

        assert_eq!(manager.get_value(sem).expect("Failed to get value"), 5);
        assert!(manager.wait(sem, 3).expect("Failed to wait"));
        assert!(manager.wait(sem, 5).expect("Failed to wait"));
    }

    #[test]
    fn test_async_compute_queue() {
        let queue = AsyncComputeQueue::new();
        // A compute queue family is detected...
        assert!(queue.is_available());

        // ...but async multi-queue submission is not implemented on the wgpu
        // backend, so submission must report an explicit error rather than
        // silently pretend the GPU work executed.
        let commands = vec![0u8; 64];
        let compute_err = queue.submit_compute(&commands);
        assert!(
            matches!(compute_err, Err(GpuError::UnsupportedOperation { .. })),
            "submit_compute must return an explicit unsupported-operation error, got {compute_err:?}"
        );

        let graphics_err = queue.submit_graphics(&commands);
        assert!(
            matches!(graphics_err, Err(GpuError::UnsupportedOperation { .. })),
            "submit_graphics must return an explicit unsupported-operation error, got {graphics_err:?}"
        );

        // No dedicated transfer queue -> falls back to graphics, still an error.
        let transfer_err = queue.submit_transfer(&commands);
        assert!(
            matches!(transfer_err, Err(GpuError::UnsupportedOperation { .. })),
            "submit_transfer must return an explicit unsupported-operation error, got {transfer_err:?}"
        );
    }
}
