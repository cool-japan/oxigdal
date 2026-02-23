//! Advanced GPU memory management for OxiGDAL.
//!
//! This module provides sophisticated memory management strategies including
//! memory pooling, staging buffer management, defragmentation, and VRAM budget tracking.

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::{debug, trace};
use wgpu::{Buffer, BufferDescriptor, BufferUsages};

/// Memory pool configuration.
#[derive(Debug, Clone)]
pub struct MemoryPoolConfig {
    /// Initial pool size in bytes.
    pub initial_size: u64,
    /// Maximum pool size in bytes.
    pub max_size: u64,
    /// Growth factor when expanding pool.
    pub growth_factor: f64,
    /// Enable automatic defragmentation.
    pub auto_defrag: bool,
    /// Defragmentation threshold (fragmentation ratio).
    pub defrag_threshold: f64,
}

impl Default for MemoryPoolConfig {
    fn default() -> Self {
        Self {
            initial_size: 64 * 1024 * 1024,   // 64 MB
            max_size: 2 * 1024 * 1024 * 1024, // 2 GB
            growth_factor: 1.5,
            auto_defrag: true,
            defrag_threshold: 0.3,
        }
    }
}

/// Memory allocation statistics.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Total allocated bytes.
    pub total_allocated: u64,
    /// Total bytes in use.
    pub bytes_in_use: u64,
    /// Total bytes available in pool.
    pub bytes_available: u64,
    /// Number of active allocations.
    pub num_allocations: usize,
    /// Number of pool expansions.
    pub num_expansions: usize,
    /// Number of defragmentations performed.
    pub num_defrags: usize,
    /// Fragmentation ratio (0.0 = no fragmentation, 1.0 = fully fragmented).
    pub fragmentation_ratio: f64,
}

impl MemoryStats {
    /// Calculate memory utilization percentage.
    pub fn utilization(&self) -> f64 {
        if self.total_allocated == 0 {
            return 0.0;
        }
        (self.bytes_in_use as f64 / self.total_allocated as f64) * 100.0
    }

    /// Check if defragmentation is recommended.
    pub fn needs_defrag(&self, threshold: f64) -> bool {
        self.fragmentation_ratio >= threshold
    }
}

/// Memory block in the pool.
#[derive(Debug)]
struct MemoryBlock {
    /// Starting offset in the pool.
    offset: u64,
    /// Size of the block.
    size: u64,
    /// Whether the block is in use.
    in_use: bool,
    /// Block ID for tracking.
    id: u64,
}

impl MemoryBlock {
    fn new(offset: u64, size: u64, id: u64) -> Self {
        Self {
            offset,
            size,
            in_use: false,
            id,
        }
    }

    fn can_fit(&self, size: u64) -> bool {
        !self.in_use && self.size >= size
    }
}

/// GPU memory pool for efficient buffer reuse.
///
/// This pool manages a large buffer and suballocates from it to avoid
/// frequent GPU allocations.
pub struct MemoryPool {
    context: GpuContext,
    config: MemoryPoolConfig,
    buffer: Arc<Buffer>,
    blocks: Vec<MemoryBlock>,
    stats: MemoryStats,
    next_block_id: u64,
}

impl MemoryPool {
    /// Create a new memory pool.
    ///
    /// # Errors
    ///
    /// Returns an error if initial buffer creation fails.
    pub fn new(context: &GpuContext, config: MemoryPoolConfig) -> GpuResult<Self> {
        let buffer = Arc::new(context.device().create_buffer(&BufferDescriptor {
            label: Some("Memory Pool"),
            size: config.initial_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));

        let mut blocks = Vec::new();
        blocks.push(MemoryBlock::new(0, config.initial_size, 0));

        let stats = MemoryStats {
            total_allocated: config.initial_size,
            bytes_available: config.initial_size,
            ..Default::default()
        };

        debug!(
            "Created memory pool: {} MB",
            config.initial_size / (1024 * 1024)
        );

        Ok(Self {
            context: context.clone(),
            config,
            buffer,
            blocks,
            stats,
            next_block_id: 1,
        })
    }

    /// Allocate a block from the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if allocation fails or pool is exhausted.
    pub fn allocate(&mut self, size: u64, alignment: u64) -> GpuResult<MemoryAllocation> {
        let aligned_size = Self::align_size(size, alignment);

        // Find suitable block using first-fit strategy
        if let Some(block_idx) = self.find_free_block(aligned_size) {
            return self.allocate_from_block(block_idx, aligned_size);
        }

        // Try defragmentation if enabled
        if self.config.auto_defrag && self.stats.needs_defrag(self.config.defrag_threshold) {
            self.defragment()?;

            // Try again after defragmentation
            if let Some(block_idx) = self.find_free_block(aligned_size) {
                return self.allocate_from_block(block_idx, aligned_size);
            }
        }

        // Expand pool if possible
        if self.stats.total_allocated < self.config.max_size {
            self.expand_pool(aligned_size)?;

            if let Some(block_idx) = self.find_free_block(aligned_size) {
                return self.allocate_from_block(block_idx, aligned_size);
            }
        }

        Err(GpuError::internal(format!(
            "Failed to allocate {} bytes from pool",
            aligned_size
        )))
    }

    /// Free a memory allocation.
    ///
    /// # Errors
    ///
    /// Returns an error if the block ID is invalid.
    pub fn free(&mut self, allocation: MemoryAllocation) -> GpuResult<()> {
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.id == allocation.block_id)
            .ok_or_else(|| GpuError::invalid_buffer("Invalid block ID"))?;

        if !block.in_use {
            return Err(GpuError::invalid_buffer("Block already freed"));
        }

        block.in_use = false;
        self.stats.bytes_in_use = self.stats.bytes_in_use.saturating_sub(block.size);
        self.stats.bytes_available += block.size;
        self.stats.num_allocations = self.stats.num_allocations.saturating_sub(1);

        trace!("Freed {} bytes from pool", block.size);

        // Try to merge adjacent free blocks
        self.merge_adjacent_blocks();

        Ok(())
    }

    /// Get current memory statistics.
    pub fn stats(&self) -> &MemoryStats {
        &self.stats
    }

    /// Manually trigger defragmentation.
    ///
    /// # Errors
    ///
    /// Returns an error if defragmentation fails.
    pub fn defragment(&mut self) -> GpuResult<()> {
        debug!("Starting memory pool defragmentation");

        // Sort blocks by offset
        self.blocks.sort_by_key(|b| b.offset);

        // Merge all adjacent free blocks
        let mut i = 0;
        while i < self.blocks.len().saturating_sub(1) {
            if !self.blocks[i].in_use && !self.blocks[i + 1].in_use {
                let next_size = self.blocks[i + 1].size;
                self.blocks[i].size += next_size;
                self.blocks.remove(i + 1);
            } else {
                i += 1;
            }
        }

        self.stats.num_defrags += 1;
        self.update_fragmentation_ratio();

        debug!(
            "Defragmentation complete: {} blocks remaining",
            self.blocks.len()
        );

        Ok(())
    }

    /// Reset the entire pool.
    pub fn reset(&mut self) {
        for block in &mut self.blocks {
            block.in_use = false;
        }

        // Merge all blocks into one
        self.blocks.clear();
        self.blocks.push(MemoryBlock::new(
            0,
            self.stats.total_allocated,
            self.next_block_id,
        ));
        self.next_block_id += 1;

        self.stats.bytes_in_use = 0;
        self.stats.bytes_available = self.stats.total_allocated;
        self.stats.num_allocations = 0;
        self.stats.fragmentation_ratio = 0.0;

        debug!("Memory pool reset");
    }

    fn align_size(size: u64, alignment: u64) -> u64 {
        ((size + alignment - 1) / alignment) * alignment
    }

    fn find_free_block(&self, size: u64) -> Option<usize> {
        self.blocks.iter().position(|block| block.can_fit(size))
    }

    fn allocate_from_block(&mut self, block_idx: usize, size: u64) -> GpuResult<MemoryAllocation> {
        let offset = self.blocks[block_idx].offset;
        let block_id = self.blocks[block_idx].id;
        let block_size = self.blocks[block_idx].size;

        // Split block if there's leftover space
        if block_size > size {
            let remaining_size = block_size - size;
            let new_offset = offset + size;

            let new_block = MemoryBlock::new(new_offset, remaining_size, self.next_block_id);
            self.next_block_id += 1;

            self.blocks[block_idx].size = size;
            self.blocks.insert(block_idx + 1, new_block);
        }

        self.blocks[block_idx].in_use = true;

        self.stats.bytes_in_use += size;
        self.stats.bytes_available = self.stats.bytes_available.saturating_sub(size);
        self.stats.num_allocations += 1;

        self.update_fragmentation_ratio();

        trace!("Allocated {} bytes at offset {}", size, offset);

        Ok(MemoryAllocation {
            buffer: Arc::clone(&self.buffer),
            offset,
            size,
            block_id,
        })
    }

    fn expand_pool(&mut self, min_additional_size: u64) -> GpuResult<()> {
        let current_size = self.stats.total_allocated;
        let growth = (current_size as f64 * self.config.growth_factor) as u64;
        let new_size = (current_size + growth.max(min_additional_size)).min(self.config.max_size);

        if new_size <= current_size {
            return Err(GpuError::internal("Cannot expand pool beyond maximum size"));
        }

        let additional_size = new_size - current_size;

        // Create new larger buffer
        let new_buffer = Arc::new(self.context.device().create_buffer(&BufferDescriptor {
            label: Some("Expanded Memory Pool"),
            size: new_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));

        // Copy existing data to new buffer
        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Pool Expansion Copy"),
                });

        encoder.copy_buffer_to_buffer(&self.buffer, 0, &new_buffer, 0, current_size);

        self.context.queue().submit(Some(encoder.finish()));

        // Update buffer and add new free block
        self.buffer = new_buffer;
        self.blocks.push(MemoryBlock::new(
            current_size,
            additional_size,
            self.next_block_id,
        ));
        self.next_block_id += 1;

        self.stats.total_allocated = new_size;
        self.stats.bytes_available += additional_size;
        self.stats.num_expansions += 1;

        debug!(
            "Expanded memory pool: {} MB -> {} MB",
            current_size / (1024 * 1024),
            new_size / (1024 * 1024)
        );

        Ok(())
    }

    fn merge_adjacent_blocks(&mut self) {
        self.blocks.sort_by_key(|b| b.offset);

        let mut i = 0;
        while i < self.blocks.len().saturating_sub(1) {
            if !self.blocks[i].in_use
                && !self.blocks[i + 1].in_use
                && self.blocks[i].offset + self.blocks[i].size == self.blocks[i + 1].offset
            {
                let next_size = self.blocks[i + 1].size;
                self.blocks[i].size += next_size;
                self.blocks.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }

    fn update_fragmentation_ratio(&mut self) {
        let free_blocks = self.blocks.iter().filter(|b| !b.in_use).count();
        let total_blocks = self.blocks.len();

        if total_blocks == 0 {
            self.stats.fragmentation_ratio = 0.0;
        } else {
            self.stats.fragmentation_ratio = free_blocks as f64 / total_blocks as f64;
        }
    }
}

/// A suballocation from the memory pool.
#[derive(Debug, Clone)]
pub struct MemoryAllocation {
    /// The underlying buffer.
    pub buffer: Arc<Buffer>,
    /// Offset into the buffer.
    pub offset: u64,
    /// Size of the allocation.
    pub size: u64,
    /// Block ID for freeing.
    block_id: u64,
}

impl MemoryAllocation {
    /// Get a slice of the buffer for this allocation.
    pub fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(self.offset..self.offset + self.size)
    }
}

/// Staging buffer manager for efficient CPU-GPU transfers.
///
/// Manages a pool of staging buffers to optimize data transfers between
/// CPU and GPU memory.
pub struct StagingBufferManager {
    context: GpuContext,
    upload_buffers: VecDeque<Arc<Buffer>>,
    download_buffers: VecDeque<Arc<Buffer>>,
    buffer_size: u64,
    max_buffers: usize,
    stats: Arc<Mutex<StagingStats>>,
}

#[derive(Debug, Clone, Default)]
struct StagingStats {
    total_uploads: usize,
    total_downloads: usize,
    upload_bytes: u64,
    download_bytes: u64,
    buffer_reuses: usize,
}

impl StagingBufferManager {
    /// Create a new staging buffer manager.
    pub fn new(context: &GpuContext, buffer_size: u64, max_buffers: usize) -> Self {
        Self {
            context: context.clone(),
            upload_buffers: VecDeque::new(),
            download_buffers: VecDeque::new(),
            buffer_size,
            max_buffers,
            stats: Arc::new(Mutex::new(StagingStats::default())),
        }
    }

    /// Get or create an upload buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails.
    pub fn get_upload_buffer(&mut self) -> GpuResult<Arc<Buffer>> {
        if let Some(buffer) = self.upload_buffers.pop_front() {
            if let Ok(mut stats) = self.stats.lock() {
                stats.buffer_reuses += 1;
            }
            Ok(buffer)
        } else {
            self.create_upload_buffer()
        }
    }

    /// Get or create a download buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails.
    pub fn get_download_buffer(&mut self) -> GpuResult<Arc<Buffer>> {
        if let Some(buffer) = self.download_buffers.pop_front() {
            if let Ok(mut stats) = self.stats.lock() {
                stats.buffer_reuses += 1;
            }
            Ok(buffer)
        } else {
            self.create_download_buffer()
        }
    }

    /// Return an upload buffer to the pool.
    pub fn return_upload_buffer(&mut self, buffer: Arc<Buffer>) {
        if self.upload_buffers.len() < self.max_buffers {
            self.upload_buffers.push_back(buffer);
        }
    }

    /// Return a download buffer to the pool.
    pub fn return_download_buffer(&mut self, buffer: Arc<Buffer>) {
        if self.download_buffers.len() < self.max_buffers {
            self.download_buffers.push_back(buffer);
        }
    }

    /// Record an upload operation.
    pub fn record_upload(&self, bytes: u64) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_uploads += 1;
            stats.upload_bytes += bytes;
        }
    }

    /// Record a download operation.
    pub fn record_download(&self, bytes: u64) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_downloads += 1;
            stats.download_bytes += bytes;
        }
    }

    /// Get staging statistics.
    pub fn stats(&self) -> StagingStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Clear all cached buffers.
    pub fn clear(&mut self) {
        self.upload_buffers.clear();
        self.download_buffers.clear();
    }

    fn create_upload_buffer(&self) -> GpuResult<Arc<Buffer>> {
        let buffer = self.context.device().create_buffer(&BufferDescriptor {
            label: Some("Staging Upload Buffer"),
            size: self.buffer_size,
            usage: BufferUsages::MAP_WRITE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        Ok(Arc::new(buffer))
    }

    fn create_download_buffer(&self) -> GpuResult<Arc<Buffer>> {
        let buffer = self.context.device().create_buffer(&BufferDescriptor {
            label: Some("Staging Download Buffer"),
            size: self.buffer_size,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Arc::new(buffer))
    }
}

/// VRAM budget manager to prevent out-of-memory errors.
pub struct VramBudgetManager {
    /// Total VRAM budget in bytes.
    total_budget: u64,
    /// Currently allocated VRAM in bytes.
    allocated: Arc<Mutex<u64>>,
    /// Allocation tracking.
    allocations: Arc<Mutex<HashMap<u64, u64>>>,
    next_id: Arc<Mutex<u64>>,
}

impl VramBudgetManager {
    /// Create a new VRAM budget manager.
    pub fn new(total_budget: u64) -> Self {
        Self {
            total_budget,
            allocated: Arc::new(Mutex::new(0)),
            allocations: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
        }
    }

    /// Try to allocate VRAM budget.
    ///
    /// # Errors
    ///
    /// Returns an error if budget is exceeded.
    pub fn allocate(&self, size: u64) -> GpuResult<u64> {
        let mut allocated = self
            .allocated
            .lock()
            .map_err(|_| GpuError::internal("Lock poisoned"))?;

        if *allocated + size > self.total_budget {
            return Err(GpuError::internal(format!(
                "VRAM budget exceeded: {} + {} > {}",
                *allocated, size, self.total_budget
            )));
        }

        let mut id = self
            .next_id
            .lock()
            .map_err(|_| GpuError::internal("Lock poisoned"))?;
        let allocation_id = *id;
        *id += 1;

        *allocated += size;

        let mut allocations = self
            .allocations
            .lock()
            .map_err(|_| GpuError::internal("Lock poisoned"))?;
        allocations.insert(allocation_id, size);

        trace!("VRAM allocated: {} bytes (total: {})", size, *allocated);

        Ok(allocation_id)
    }

    /// Free VRAM budget.
    ///
    /// # Errors
    ///
    /// Returns an error if allocation ID is invalid.
    pub fn free(&self, allocation_id: u64) -> GpuResult<()> {
        let mut allocations = self
            .allocations
            .lock()
            .map_err(|_| GpuError::internal("Lock poisoned"))?;

        let size = allocations
            .remove(&allocation_id)
            .ok_or_else(|| GpuError::invalid_buffer("Invalid allocation ID"))?;

        let mut allocated = self
            .allocated
            .lock()
            .map_err(|_| GpuError::internal("Lock poisoned"))?;

        *allocated = allocated.saturating_sub(size);

        trace!("VRAM freed: {} bytes (total: {})", size, *allocated);

        Ok(())
    }

    /// Get current allocated amount.
    pub fn allocated(&self) -> u64 {
        self.allocated.lock().map(|a| *a).unwrap_or(0)
    }

    /// Get total budget.
    pub fn budget(&self) -> u64 {
        self.total_budget
    }

    /// Get available budget.
    pub fn available(&self) -> u64 {
        self.total_budget.saturating_sub(self.allocated())
    }

    /// Get utilization percentage.
    pub fn utilization(&self) -> f64 {
        if self.total_budget == 0 {
            return 0.0;
        }
        (self.allocated() as f64 / self.total_budget as f64) * 100.0
    }

    /// Check if allocation would fit in budget.
    pub fn can_allocate(&self, size: u64) -> bool {
        self.allocated() + size <= self.total_budget
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_stats() {
        let stats = MemoryStats {
            total_allocated: 1024,
            bytes_in_use: 512,
            ..Default::default()
        };

        assert_eq!(stats.utilization(), 50.0);
    }

    #[tokio::test]
    async fn test_vram_budget_manager() {
        let manager = VramBudgetManager::new(1024);

        let id1 = manager.allocate(512).unwrap_or_else(|e| panic!("{}", e));
        assert_eq!(manager.allocated(), 512);
        assert_eq!(manager.utilization(), 50.0);

        let id2 = manager.allocate(256).unwrap_or_else(|e| panic!("{}", e));
        assert_eq!(manager.allocated(), 768);

        // Should fail - exceeds budget
        assert!(manager.allocate(512).is_err());

        manager.free(id1).unwrap_or_else(|e| panic!("{}", e));
        assert_eq!(manager.allocated(), 256);

        manager.free(id2).unwrap_or_else(|e| panic!("{}", e));
        assert_eq!(manager.allocated(), 0);
    }

    #[test]
    fn test_memory_pool_config() {
        let config = MemoryPoolConfig::default();
        assert_eq!(config.initial_size, 64 * 1024 * 1024);
        assert_eq!(config.max_size, 2 * 1024 * 1024 * 1024);
    }
}
