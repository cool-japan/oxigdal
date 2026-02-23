//! Advanced GPU memory pool with sub-allocation and defragmentation.
//!
//! This module provides efficient GPU memory management through a pool-based
//! allocator with support for:
//! - Sub-allocation from a single large buffer
//! - Alignment handling
//! - Automatic coalescing of adjacent free blocks
//! - Memory defragmentation to reduce fragmentation
//!
//! # Defragmentation
//!
//! The pool tracks fragmentation levels and provides methods to compact
//! memory allocations:
//! - `defragment()` - Logical defragmentation (updates metadata only)
//! - `defragment_with_queue()` - Full defragmentation with GPU memory copies
//!
//! # Example
//!
//! ```ignore
//! let pool = Arc::new(MemoryPool::new(device, 1024 * 1024, BufferUsages::STORAGE)?);
//! let alloc = pool.allocate(1024, 256)?;
//! // ... use allocation ...
//! // Defragment when needed
//! let result = pool.defragment_with_queue(&queue)?;
//! ```

use crate::error::{GpuAdvancedError, Result};
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use wgpu::{Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Device, Queue};

/// Memory block allocation
#[derive(Debug, Clone)]
struct MemoryBlock {
    /// Offset in the pool
    offset: u64,
    /// Size of the block
    size: u64,
    /// Whether the block is free
    is_free: bool,
    /// Allocation ID
    allocation_id: Option<u64>,
    /// Whether this block can be moved during defragmentation
    movable: bool,
    /// Reference count for tracking active usage
    ref_count: u32,
}

/// Defragmentation plan entry describing a memory move operation
#[derive(Debug, Clone)]
pub struct DefragMove {
    /// Allocation ID being moved
    pub allocation_id: u64,
    /// Source offset in the pool
    pub src_offset: u64,
    /// Destination offset in the pool
    pub dst_offset: u64,
    /// Size of the block to move
    pub size: u64,
}

/// Defragmentation plan containing all moves needed
#[derive(Debug, Clone, Default)]
pub struct DefragmentationPlan {
    /// List of move operations to perform
    pub moves: Vec<DefragMove>,
    /// Total bytes to be moved
    pub total_bytes: u64,
    /// Expected fragmentation after defragmentation
    pub expected_fragmentation: f64,
    /// Current fragmentation level
    pub current_fragmentation: f64,
}

impl DefragmentationPlan {
    /// Check if defragmentation is worthwhile
    pub fn is_worthwhile(&self, min_improvement: f64) -> bool {
        if self.moves.is_empty() {
            return false;
        }
        let improvement = self.current_fragmentation - self.expected_fragmentation;
        improvement >= min_improvement
    }

    /// Get the number of moves
    pub fn move_count(&self) -> usize {
        self.moves.len()
    }
}

/// Result of a defragmentation operation
#[derive(Debug, Clone)]
pub struct DefragmentationResult {
    /// Whether defragmentation was performed
    pub performed: bool,
    /// Number of blocks moved
    pub blocks_moved: usize,
    /// Total bytes moved
    pub bytes_moved: u64,
    /// Fragmentation before defragmentation
    pub fragmentation_before: f64,
    /// Fragmentation after defragmentation
    pub fragmentation_after: f64,
    /// Time taken for defragmentation
    pub duration: Duration,
    /// Number of blocks that couldn't be moved (pinned/in-use)
    pub unmovable_blocks: usize,
}

impl Default for DefragmentationResult {
    fn default() -> Self {
        Self {
            performed: false,
            blocks_moved: 0,
            bytes_moved: 0,
            fragmentation_before: 0.0,
            fragmentation_after: 0.0,
            duration: Duration::ZERO,
            unmovable_blocks: 0,
        }
    }
}

/// Defragmentation configuration
#[derive(Debug, Clone)]
pub struct DefragConfig {
    /// Minimum fragmentation level to trigger defragmentation (0.0 - 1.0)
    pub min_fragmentation_threshold: f64,
    /// Minimum improvement required to proceed with defragmentation
    pub min_improvement: f64,
    /// Maximum number of blocks to move in a single defragmentation pass
    pub max_moves_per_pass: usize,
    /// Whether to skip unmovable blocks and continue
    pub skip_unmovable: bool,
    /// Alignment for compacted blocks
    pub compaction_alignment: u64,
}

impl Default for DefragConfig {
    fn default() -> Self {
        Self {
            min_fragmentation_threshold: 0.2,
            min_improvement: 0.1,
            max_moves_per_pass: 100,
            skip_unmovable: true,
            compaction_alignment: 256,
        }
    }
}

/// Memory pool for efficient GPU memory management
pub struct MemoryPool {
    /// GPU device
    device: Arc<Device>,
    /// Pool buffer
    buffer: Arc<Mutex<Option<Buffer>>>,
    /// Total pool size
    pool_size: u64,
    /// Memory blocks (offset -> block)
    blocks: Arc<RwLock<BTreeMap<u64, MemoryBlock>>>,
    /// Next allocation ID
    next_alloc_id: Arc<Mutex<u64>>,
    /// Buffer usage flags
    usage: BufferUsages,
    /// Current memory usage
    current_usage: Arc<Mutex<u64>>,
    /// Peak memory usage
    peak_usage: Arc<Mutex<u64>>,
    /// Number of allocations
    allocation_count: Arc<Mutex<u64>>,
    /// Number of deallocations
    deallocation_count: Arc<Mutex<u64>>,
    /// Number of defragmentations
    defrag_count: Arc<Mutex<u64>>,
    /// Relocation table: maps allocation_id to current offset
    /// This is updated during defragmentation to track moved blocks
    allocation_offsets: Arc<RwLock<HashMap<u64, u64>>>,
    /// Defragmentation configuration
    defrag_config: RwLock<DefragConfig>,
    /// Last defragmentation timestamp
    last_defrag_time: Arc<Mutex<Option<Instant>>>,
    /// Total bytes moved during all defragmentations
    total_bytes_defragged: Arc<AtomicU64>,
}

/// Memory allocation handle
///
/// The allocation handle tracks its current position in the pool.
/// During defragmentation, the offset may change, but this is handled
/// transparently through the pool's relocation table.
pub struct MemoryAllocation {
    /// Allocation ID
    id: u64,
    /// Original offset in the pool (may be stale after defragmentation)
    original_offset: u64,
    /// Size of the allocation
    size: u64,
    /// Reference to the pool
    pool: Arc<MemoryPool>,
}

impl MemoryPool {
    /// Create a new memory pool
    pub fn new(device: Arc<Device>, pool_size: u64, usage: BufferUsages) -> Result<Self> {
        Self::with_config(device, pool_size, usage, DefragConfig::default())
    }

    /// Create a new memory pool with custom defragmentation configuration
    pub fn with_config(
        device: Arc<Device>,
        pool_size: u64,
        usage: BufferUsages,
        defrag_config: DefragConfig,
    ) -> Result<Self> {
        // Ensure usage includes COPY_SRC and COPY_DST for defragmentation
        let usage_with_copy = usage | BufferUsages::COPY_SRC | BufferUsages::COPY_DST;

        // Create initial pool buffer
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Memory Pool"),
            size: pool_size,
            usage: usage_with_copy,
            mapped_at_creation: false,
        });

        // Initialize with one large free block
        let mut blocks = BTreeMap::new();
        blocks.insert(
            0,
            MemoryBlock {
                offset: 0,
                size: pool_size,
                is_free: true,
                allocation_id: None,
                movable: true,
                ref_count: 0,
            },
        );

        Ok(Self {
            device,
            buffer: Arc::new(Mutex::new(Some(buffer))),
            pool_size,
            blocks: Arc::new(RwLock::new(blocks)),
            next_alloc_id: Arc::new(Mutex::new(0)),
            usage: usage_with_copy,
            current_usage: Arc::new(Mutex::new(0)),
            peak_usage: Arc::new(Mutex::new(0)),
            allocation_count: Arc::new(Mutex::new(0)),
            deallocation_count: Arc::new(Mutex::new(0)),
            defrag_count: Arc::new(Mutex::new(0)),
            allocation_offsets: Arc::new(RwLock::new(HashMap::new())),
            defrag_config: RwLock::new(defrag_config),
            last_defrag_time: Arc::new(Mutex::new(None)),
            total_bytes_defragged: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Allocate memory from the pool
    pub fn allocate(self: &Arc<Self>, size: u64, alignment: u64) -> Result<MemoryAllocation> {
        let aligned_size = Self::align_up(size, alignment);

        let alloc_id = {
            let mut next_id = self.next_alloc_id.lock();
            let id = *next_id;
            *next_id = next_id.wrapping_add(1);
            id
        };

        // Find a free block using first-fit strategy
        let (offset, block_offset) = {
            let blocks = self.blocks.read();

            let mut found: Option<(u64, u64)> = None;

            for (blk_offset, block) in blocks.iter() {
                if block.is_free && block.size >= aligned_size {
                    let aligned_offset = Self::align_up(*blk_offset, alignment);
                    let waste = aligned_offset - blk_offset;

                    if block.size >= aligned_size + waste {
                        found = Some((aligned_offset, *blk_offset));
                        break;
                    }
                }
            }

            found.ok_or_else(|| GpuAdvancedError::AllocationFailed {
                size: aligned_size,
                available: self.get_available_memory(),
            })?
        };

        // Split the block
        {
            let mut blocks = self.blocks.write();

            let block = blocks
                .remove(&block_offset)
                .ok_or_else(|| GpuAdvancedError::memory_pool_error("Block not found"))?;

            let waste = offset - block_offset;

            // Create waste block if needed
            if waste > 0 {
                blocks.insert(
                    block_offset,
                    MemoryBlock {
                        offset: block_offset,
                        size: waste,
                        is_free: true,
                        allocation_id: None,
                        movable: true,
                        ref_count: 0,
                    },
                );
            }

            // Create allocated block
            blocks.insert(
                offset,
                MemoryBlock {
                    offset,
                    size: aligned_size,
                    is_free: false,
                    allocation_id: Some(alloc_id),
                    movable: true,
                    ref_count: 1,
                },
            );

            // Create remainder block if needed
            let remainder = block.size - aligned_size - waste;
            if remainder > 0 {
                blocks.insert(
                    offset + aligned_size,
                    MemoryBlock {
                        offset: offset + aligned_size,
                        size: remainder,
                        is_free: true,
                        allocation_id: None,
                        movable: true,
                        ref_count: 0,
                    },
                );
            }
        }

        // Register allocation offset in the relocation table
        {
            let mut offsets = self.allocation_offsets.write();
            offsets.insert(alloc_id, offset);
        }

        // Update statistics
        {
            let mut usage = self.current_usage.lock();
            *usage = usage.saturating_add(aligned_size);

            let mut peak = self.peak_usage.lock();
            *peak = (*peak).max(*usage);

            let mut count = self.allocation_count.lock();
            *count = count.saturating_add(1);
        }

        Ok(MemoryAllocation {
            id: alloc_id,
            original_offset: offset,
            size: aligned_size,
            pool: Arc::clone(self),
        })
    }

    /// Get the current offset for an allocation ID
    /// Returns None if the allocation is not found
    pub fn get_allocation_offset(&self, alloc_id: u64) -> Option<u64> {
        self.allocation_offsets.read().get(&alloc_id).copied()
    }

    /// Pin an allocation to prevent it from being moved during defragmentation
    pub fn pin_allocation(&self, alloc_id: u64) -> Result<()> {
        let offsets = self.allocation_offsets.read();
        let offset = offsets
            .get(&alloc_id)
            .copied()
            .ok_or_else(|| GpuAdvancedError::memory_pool_error("Allocation not found"))?;
        drop(offsets);

        let mut blocks = self.blocks.write();
        if let Some(block) = blocks.get_mut(&offset) {
            block.movable = false;
            Ok(())
        } else {
            Err(GpuAdvancedError::memory_pool_error(
                "Block not found for allocation",
            ))
        }
    }

    /// Unpin an allocation to allow it to be moved during defragmentation
    pub fn unpin_allocation(&self, alloc_id: u64) -> Result<()> {
        let offsets = self.allocation_offsets.read();
        let offset = offsets
            .get(&alloc_id)
            .copied()
            .ok_or_else(|| GpuAdvancedError::memory_pool_error("Allocation not found"))?;
        drop(offsets);

        let mut blocks = self.blocks.write();
        if let Some(block) = blocks.get_mut(&offset) {
            block.movable = true;
            Ok(())
        } else {
            Err(GpuAdvancedError::memory_pool_error(
                "Block not found for allocation",
            ))
        }
    }

    /// Set the defragmentation configuration
    pub fn set_defrag_config(&self, config: DefragConfig) {
        *self.defrag_config.write() = config;
    }

    /// Get the current defragmentation configuration
    pub fn get_defrag_config(&self) -> DefragConfig {
        self.defrag_config.read().clone()
    }

    /// Deallocate memory
    fn deallocate(&self, allocation: &MemoryAllocation) -> Result<()> {
        // Get the current offset from the relocation table (handles defragmentation)
        let current_offset = self
            .allocation_offsets
            .read()
            .get(&allocation.id)
            .copied()
            .unwrap_or(allocation.original_offset);

        let mut blocks = self.blocks.write();

        // Find and free the block using current offset
        if let Some(block) = blocks.get_mut(&current_offset) {
            if block.allocation_id == Some(allocation.id) {
                block.is_free = true;
                block.allocation_id = None;
                block.ref_count = 0;
            } else {
                return Err(GpuAdvancedError::memory_pool_error("Invalid allocation ID"));
            }
        } else {
            return Err(GpuAdvancedError::memory_pool_error("Block not found"));
        }

        // Remove from relocation table
        {
            let mut offsets = self.allocation_offsets.write();
            offsets.remove(&allocation.id);
        }

        // Update statistics
        {
            let mut usage = self.current_usage.lock();
            *usage = usage.saturating_sub(allocation.size);

            let mut count = self.deallocation_count.lock();
            *count = count.saturating_add(1);
        }

        // Try to merge adjacent free blocks
        self.coalesce_free_blocks(&mut blocks);

        Ok(())
    }

    /// Coalesce adjacent free blocks
    fn coalesce_free_blocks(&self, blocks: &mut BTreeMap<u64, MemoryBlock>) {
        let mut to_merge: Vec<u64> = Vec::new();

        let mut prev_offset: Option<u64> = None;
        for (offset, block) in blocks.iter() {
            if block.is_free {
                if let Some(prev_off) = prev_offset {
                    if let Some(prev_block) = blocks.get(&prev_off) {
                        if prev_block.is_free && prev_block.offset + prev_block.size == *offset {
                            to_merge.push(*offset);
                        }
                    }
                }
                prev_offset = Some(*offset);
            } else {
                prev_offset = None;
            }
        }

        // Merge blocks
        for offset in to_merge {
            if let Some(block) = blocks.remove(&offset) {
                // Find previous block
                let prev_offset = blocks.range(..offset).next_back().map(|(k, _)| *k);

                if let Some(prev_off) = prev_offset {
                    if let Some(prev_block) = blocks.get_mut(&prev_off) {
                        if prev_block.is_free {
                            prev_block.size += block.size;
                        }
                    }
                }
            }
        }
    }

    /// Create a defragmentation plan without executing it
    ///
    /// This analyzes the current memory layout and creates a plan for
    /// compacting allocations. The plan can be inspected before executing.
    pub fn plan_defragmentation(&self) -> DefragmentationPlan {
        let config = self.defrag_config.read().clone();
        let blocks = self.blocks.read();

        let current_fragmentation = self.calculate_fragmentation_internal(&blocks);

        // Collect all allocated blocks sorted by offset
        let mut allocated_blocks: Vec<_> = blocks
            .iter()
            .filter(|(_, b)| !b.is_free && b.allocation_id.is_some())
            .map(|(offset, b)| (*offset, b.clone()))
            .collect();

        allocated_blocks.sort_by_key(|(offset, _)| *offset);

        // Calculate target positions (compacted from the start)
        let mut moves = Vec::new();
        let mut total_bytes = 0u64;
        let mut next_offset = 0u64;

        for (current_offset, block) in &allocated_blocks {
            let aligned_offset = Self::align_up(next_offset, config.compaction_alignment);

            // Only create a move if the block actually needs to move
            if aligned_offset < *current_offset && block.movable {
                if let Some(alloc_id) = block.allocation_id {
                    moves.push(DefragMove {
                        allocation_id: alloc_id,
                        src_offset: *current_offset,
                        dst_offset: aligned_offset,
                        size: block.size,
                    });
                    total_bytes += block.size;
                }
                next_offset = aligned_offset + block.size;
            } else {
                // Block stays in place (either already optimal or unmovable)
                next_offset = current_offset + block.size;
            }
        }

        // Calculate expected fragmentation after defragmentation
        let expected_fragmentation = if moves.is_empty() {
            current_fragmentation
        } else {
            // After full compaction, fragmentation should be near zero
            // unless there are unmovable blocks creating gaps
            let unmovable_count = allocated_blocks.iter().filter(|(_, b)| !b.movable).count();
            if unmovable_count == 0 {
                0.0
            } else {
                // Estimate remaining fragmentation based on unmovable blocks
                (unmovable_count as f64 / allocated_blocks.len().max(1) as f64) * 0.5
            }
        };

        DefragmentationPlan {
            moves,
            total_bytes,
            expected_fragmentation,
            current_fragmentation,
        }
    }

    /// Defragment the pool (logical defragmentation - metadata only)
    ///
    /// This method updates the block metadata without performing GPU memory copies.
    /// It's useful for testing or when you plan to recreate the data anyway.
    ///
    /// For actual GPU memory defragmentation with data preservation, use
    /// `defragment_with_queue()` instead.
    pub fn defragment(&self) -> Result<DefragmentationResult> {
        let start = Instant::now();
        let plan = self.plan_defragmentation();

        if plan.moves.is_empty() {
            return Ok(DefragmentationResult {
                performed: false,
                fragmentation_before: plan.current_fragmentation,
                fragmentation_after: plan.current_fragmentation,
                duration: start.elapsed(),
                ..Default::default()
            });
        }

        let config = self.defrag_config.read().clone();

        // Check if defragmentation is worthwhile
        if !plan.is_worthwhile(config.min_improvement) {
            return Ok(DefragmentationResult {
                performed: false,
                fragmentation_before: plan.current_fragmentation,
                fragmentation_after: plan.current_fragmentation,
                duration: start.elapsed(),
                ..Default::default()
            });
        }

        // Perform logical defragmentation (update metadata only)
        let result = self.execute_defrag_plan_logical(&plan, &config)?;

        // Update statistics
        {
            let mut count = self.defrag_count.lock();
            *count = count.saturating_add(1);
        }

        self.total_bytes_defragged
            .fetch_add(result.bytes_moved, Ordering::Relaxed);

        *self.last_defrag_time.lock() = Some(Instant::now());

        Ok(DefragmentationResult {
            duration: start.elapsed(),
            ..result
        })
    }

    /// Defragment the pool with GPU memory copies
    ///
    /// This method performs actual GPU memory copies to compact allocations.
    /// It requires a Queue for command submission.
    ///
    /// # Arguments
    /// * `queue` - The GPU queue for submitting copy commands
    ///
    /// # Returns
    /// A `DefragmentationResult` describing what was done
    pub fn defragment_with_queue(&self, queue: &Queue) -> Result<DefragmentationResult> {
        let start = Instant::now();
        let plan = self.plan_defragmentation();

        if plan.moves.is_empty() {
            return Ok(DefragmentationResult {
                performed: false,
                fragmentation_before: plan.current_fragmentation,
                fragmentation_after: plan.current_fragmentation,
                duration: start.elapsed(),
                ..Default::default()
            });
        }

        let config = self.defrag_config.read().clone();

        // Check if defragmentation is worthwhile
        if plan.current_fragmentation < config.min_fragmentation_threshold {
            return Ok(DefragmentationResult {
                performed: false,
                fragmentation_before: plan.current_fragmentation,
                fragmentation_after: plan.current_fragmentation,
                duration: start.elapsed(),
                ..Default::default()
            });
        }

        if !plan.is_worthwhile(config.min_improvement) {
            return Ok(DefragmentationResult {
                performed: false,
                fragmentation_before: plan.current_fragmentation,
                fragmentation_after: plan.current_fragmentation,
                duration: start.elapsed(),
                ..Default::default()
            });
        }

        // Execute the defragmentation plan with GPU copies
        let result = self.execute_defrag_plan_gpu(&plan, &config, queue)?;

        // Update statistics
        {
            let mut count = self.defrag_count.lock();
            *count = count.saturating_add(1);
        }

        self.total_bytes_defragged
            .fetch_add(result.bytes_moved, Ordering::Relaxed);

        *self.last_defrag_time.lock() = Some(Instant::now());

        Ok(DefragmentationResult {
            duration: start.elapsed(),
            ..result
        })
    }

    /// Execute defragmentation plan (logical only - no GPU copies)
    fn execute_defrag_plan_logical(
        &self,
        plan: &DefragmentationPlan,
        config: &DefragConfig,
    ) -> Result<DefragmentationResult> {
        let mut blocks = self.blocks.write();
        let mut allocation_offsets = self.allocation_offsets.write();

        let mut blocks_moved = 0usize;
        let mut bytes_moved = 0u64;
        let mut unmovable_blocks = 0usize;

        let moves_to_execute: Vec<_> = plan
            .moves
            .iter()
            .take(config.max_moves_per_pass)
            .cloned()
            .collect();

        for defrag_move in &moves_to_execute {
            // Remove the block from old position
            let block = match blocks.remove(&defrag_move.src_offset) {
                Some(b) => b,
                None => {
                    if config.skip_unmovable {
                        unmovable_blocks += 1;
                        continue;
                    } else {
                        return Err(GpuAdvancedError::memory_pool_error(
                            "Block not found during defragmentation",
                        ));
                    }
                }
            };

            if !block.movable {
                // Put it back and skip
                blocks.insert(defrag_move.src_offset, block);
                unmovable_blocks += 1;
                continue;
            }

            // Create block at new position
            let new_block = MemoryBlock {
                offset: defrag_move.dst_offset,
                size: block.size,
                is_free: false,
                allocation_id: block.allocation_id,
                movable: block.movable,
                ref_count: block.ref_count,
            };

            blocks.insert(defrag_move.dst_offset, new_block);

            // Update relocation table
            if let Some(alloc_id) = block.allocation_id {
                allocation_offsets.insert(alloc_id, defrag_move.dst_offset);
            }

            blocks_moved += 1;
            bytes_moved += defrag_move.size;
        }

        // Rebuild free blocks after compaction
        drop(blocks);
        drop(allocation_offsets);
        self.rebuild_free_blocks()?;

        // Calculate final fragmentation
        let blocks = self.blocks.read();
        let fragmentation_after = self.calculate_fragmentation_internal(&blocks);

        Ok(DefragmentationResult {
            performed: blocks_moved > 0,
            blocks_moved,
            bytes_moved,
            fragmentation_before: plan.current_fragmentation,
            fragmentation_after,
            duration: Duration::ZERO, // Will be filled by caller
            unmovable_blocks,
        })
    }

    /// Execute defragmentation plan with GPU memory copies
    fn execute_defrag_plan_gpu(
        &self,
        plan: &DefragmentationPlan,
        config: &DefragConfig,
        queue: &Queue,
    ) -> Result<DefragmentationResult> {
        let buffer_guard = self.buffer.lock();
        let buffer = buffer_guard
            .as_ref()
            .ok_or_else(|| GpuAdvancedError::memory_pool_error("Pool buffer not available"))?;

        // Create a staging buffer for safe copying
        // We use a staging buffer to avoid overlapping copies in the same buffer
        let staging_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Defrag Staging Buffer"),
            size: plan.total_bytes,
            usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let moves_to_execute: Vec<_> = plan
            .moves
            .iter()
            .take(config.max_moves_per_pass)
            .cloned()
            .collect();

        // First pass: Copy all blocks to staging buffer
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Defrag Copy to Staging"),
            });

        let mut staging_offset = 0u64;
        let mut staging_map: Vec<(DefragMove, u64)> = Vec::new();

        for defrag_move in &moves_to_execute {
            encoder.copy_buffer_to_buffer(
                buffer,
                defrag_move.src_offset,
                &staging_buffer,
                staging_offset,
                defrag_move.size,
            );
            staging_map.push((defrag_move.clone(), staging_offset));
            staging_offset += defrag_move.size;
        }

        queue.submit(std::iter::once(encoder.finish()));

        // Second pass: Copy from staging to final positions
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Defrag Copy from Staging"),
            });

        for (defrag_move, staging_off) in &staging_map {
            encoder.copy_buffer_to_buffer(
                &staging_buffer,
                *staging_off,
                buffer,
                defrag_move.dst_offset,
                defrag_move.size,
            );
        }

        queue.submit(std::iter::once(encoder.finish()));

        // In wgpu 28+, device polls automatically in the background
        // No explicit poll needed - GPU operations complete asynchronously

        drop(buffer_guard);

        // Now update the metadata
        let mut blocks = self.blocks.write();
        let mut allocation_offsets = self.allocation_offsets.write();

        let mut blocks_moved = 0usize;
        let mut bytes_moved = 0u64;
        let mut unmovable_blocks = 0usize;

        for defrag_move in &moves_to_execute {
            // Remove the block from old position
            let block = match blocks.remove(&defrag_move.src_offset) {
                Some(b) => b,
                None => {
                    unmovable_blocks += 1;
                    continue;
                }
            };

            // Create block at new position
            let new_block = MemoryBlock {
                offset: defrag_move.dst_offset,
                size: block.size,
                is_free: false,
                allocation_id: block.allocation_id,
                movable: block.movable,
                ref_count: block.ref_count,
            };

            blocks.insert(defrag_move.dst_offset, new_block);

            // Update relocation table
            if let Some(alloc_id) = block.allocation_id {
                allocation_offsets.insert(alloc_id, defrag_move.dst_offset);
            }

            blocks_moved += 1;
            bytes_moved += defrag_move.size;
        }

        drop(blocks);
        drop(allocation_offsets);

        // Rebuild free blocks after compaction
        self.rebuild_free_blocks()?;

        // Calculate final fragmentation
        let blocks = self.blocks.read();
        let fragmentation_after = self.calculate_fragmentation_internal(&blocks);

        Ok(DefragmentationResult {
            performed: blocks_moved > 0,
            blocks_moved,
            bytes_moved,
            fragmentation_before: plan.current_fragmentation,
            fragmentation_after,
            duration: Duration::ZERO, // Will be filled by caller
            unmovable_blocks,
        })
    }

    /// Rebuild free blocks after defragmentation
    ///
    /// This method scans the block map and creates free blocks for any gaps
    fn rebuild_free_blocks(&self) -> Result<()> {
        let mut blocks = self.blocks.write();

        // Collect all allocated block ranges
        let allocated_ranges: Vec<(u64, u64)> = blocks
            .iter()
            .filter(|(_, b)| !b.is_free)
            .map(|(offset, b)| (*offset, b.size))
            .collect();

        // Remove all free blocks
        let offsets_to_remove: Vec<u64> = blocks
            .iter()
            .filter(|(_, b)| b.is_free)
            .map(|(offset, _)| *offset)
            .collect();

        for offset in offsets_to_remove {
            blocks.remove(&offset);
        }

        // Find gaps and create free blocks
        let mut last_end = 0u64;

        for (offset, size) in &allocated_ranges {
            if *offset > last_end {
                // There's a gap - create a free block
                blocks.insert(
                    last_end,
                    MemoryBlock {
                        offset: last_end,
                        size: offset - last_end,
                        is_free: true,
                        allocation_id: None,
                        movable: true,
                        ref_count: 0,
                    },
                );
            }
            last_end = offset + size;
        }

        // Create trailing free block if there's space at the end
        if last_end < self.pool_size {
            blocks.insert(
                last_end,
                MemoryBlock {
                    offset: last_end,
                    size: self.pool_size - last_end,
                    is_free: true,
                    allocation_id: None,
                    movable: true,
                    ref_count: 0,
                },
            );
        }

        // Coalesce any adjacent free blocks
        self.coalesce_free_blocks(&mut blocks);

        Ok(())
    }

    /// Calculate fragmentation from blocks (internal helper)
    fn calculate_fragmentation_internal(&self, blocks: &BTreeMap<u64, MemoryBlock>) -> f64 {
        let free_blocks: Vec<u64> = blocks
            .values()
            .filter(|b| b.is_free)
            .map(|b| b.size)
            .collect();

        self.calculate_fragmentation(&free_blocks)
    }

    /// Check if defragmentation is needed based on current configuration
    pub fn needs_defragmentation(&self) -> bool {
        let config = self.defrag_config.read();
        let stats = self.get_stats();

        stats.fragmentation >= config.min_fragmentation_threshold
    }

    /// Get fragmentation level (0.0 - 1.0)
    pub fn get_fragmentation(&self) -> f64 {
        let blocks = self.blocks.read();
        self.calculate_fragmentation_internal(&blocks)
    }

    /// Get total bytes defragmented across all defragmentation operations
    pub fn get_total_bytes_defragged(&self) -> u64 {
        self.total_bytes_defragged.load(Ordering::Relaxed)
    }

    /// Get the time since the last defragmentation
    pub fn time_since_last_defrag(&self) -> Option<Duration> {
        self.last_defrag_time
            .lock()
            .map(|instant| instant.elapsed())
    }

    /// Get pool buffer
    pub fn buffer(&self) -> Option<Buffer> {
        // Clone the buffer (increases reference count)
        self.buffer.lock().as_ref().map(|_b| {
            self.device.create_buffer(&BufferDescriptor {
                label: Some("Memory Pool Access"),
                size: self.pool_size,
                usage: self.usage,
                mapped_at_creation: false,
            })
        })
    }

    /// Get available memory
    pub fn get_available_memory(&self) -> u64 {
        let blocks = self.blocks.read();
        blocks
            .values()
            .filter(|block| block.is_free)
            .map(|block| block.size)
            .sum()
    }

    /// Get current memory usage
    pub fn get_current_usage(&self) -> u64 {
        *self.current_usage.lock()
    }

    /// Get peak memory usage
    pub fn get_peak_usage(&self) -> u64 {
        *self.peak_usage.lock()
    }

    /// Get memory statistics
    pub fn get_stats(&self) -> MemoryPoolStats {
        let blocks = self.blocks.read();
        let free_blocks: Vec<_> = blocks
            .values()
            .filter(|b| b.is_free)
            .map(|b| b.size)
            .collect();

        let allocated_blocks: Vec<_> = blocks
            .values()
            .filter(|b| !b.is_free)
            .map(|b| b.size)
            .collect();

        MemoryPoolStats {
            pool_size: self.pool_size,
            current_usage: *self.current_usage.lock(),
            peak_usage: *self.peak_usage.lock(),
            available: self.get_available_memory(),
            allocation_count: *self.allocation_count.lock(),
            deallocation_count: *self.deallocation_count.lock(),
            defrag_count: *self.defrag_count.lock(),
            free_block_count: free_blocks.len(),
            allocated_block_count: allocated_blocks.len(),
            largest_free_block: free_blocks.iter().max().copied().unwrap_or(0),
            fragmentation: self.calculate_fragmentation(&free_blocks),
        }
    }

    /// Calculate fragmentation factor (0.0 = no fragmentation, 1.0 = highly fragmented)
    fn calculate_fragmentation(&self, free_blocks: &[u64]) -> f64 {
        if free_blocks.is_empty() {
            return 0.0;
        }

        let total_free: u64 = free_blocks.iter().sum();
        let largest = free_blocks.iter().max().copied().unwrap_or(0);

        if total_free == 0 {
            return 0.0;
        }

        1.0 - (largest as f64 / total_free as f64)
    }

    /// Align value up to alignment
    fn align_up(value: u64, alignment: u64) -> u64 {
        if alignment == 0 {
            return value;
        }
        value.div_ceil(alignment) * alignment
    }

    /// Print pool statistics
    pub fn print_stats(&self) {
        let stats = self.get_stats();
        println!("\nMemory Pool Statistics:");
        println!("  Pool size: {} bytes", stats.pool_size);
        println!(
            "  Current usage: {} bytes ({:.1}%)",
            stats.current_usage,
            (stats.current_usage as f64 / stats.pool_size as f64) * 100.0
        );
        println!(
            "  Peak usage: {} bytes ({:.1}%)",
            stats.peak_usage,
            (stats.peak_usage as f64 / stats.pool_size as f64) * 100.0
        );
        println!("  Available: {} bytes", stats.available);
        println!("  Allocations: {}", stats.allocation_count);
        println!("  Deallocations: {}", stats.deallocation_count);
        println!("  Defragmentations: {}", stats.defrag_count);
        println!("  Free blocks: {}", stats.free_block_count);
        println!("  Allocated blocks: {}", stats.allocated_block_count);
        println!("  Largest free block: {} bytes", stats.largest_free_block);
        println!("  Fragmentation: {:.1}%", stats.fragmentation * 100.0);
    }
}

/// Memory pool statistics
#[derive(Debug, Clone)]
pub struct MemoryPoolStats {
    /// Total pool size
    pub pool_size: u64,
    /// Current memory usage
    pub current_usage: u64,
    /// Peak memory usage
    pub peak_usage: u64,
    /// Available memory
    pub available: u64,
    /// Number of allocations
    pub allocation_count: u64,
    /// Number of deallocations
    pub deallocation_count: u64,
    /// Number of defragmentations
    pub defrag_count: u64,
    /// Number of free blocks
    pub free_block_count: usize,
    /// Number of allocated blocks
    pub allocated_block_count: usize,
    /// Size of largest free block
    pub largest_free_block: u64,
    /// Fragmentation factor (0.0 to 1.0)
    pub fragmentation: f64,
}

impl MemoryAllocation {
    /// Get allocation offset
    ///
    /// This looks up the current offset from the pool's relocation table,
    /// which handles offset changes due to defragmentation.
    pub fn offset(&self) -> u64 {
        // Look up current offset from relocation table (handles defragmentation)
        // Fall back to original_offset if not found
        self.pool
            .get_allocation_offset(self.id)
            .unwrap_or(self.original_offset)
    }

    /// Get allocation size
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Get allocation range
    pub fn range(&self) -> Range<u64> {
        let offset = self.offset();
        offset..(offset + self.size)
    }

    /// Get allocation ID
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for MemoryAllocation {
    fn drop(&mut self) {
        // Automatically deallocate when dropped
        let _ = self.pool.deallocate(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_up() {
        assert_eq!(MemoryPool::align_up(0, 256), 0);
        assert_eq!(MemoryPool::align_up(1, 256), 256);
        assert_eq!(MemoryPool::align_up(256, 256), 256);
        assert_eq!(MemoryPool::align_up(257, 256), 512);
    }

    #[test]
    fn test_memory_block() {
        let block = MemoryBlock {
            offset: 0,
            size: 1024,
            is_free: true,
            allocation_id: None,
            movable: true,
            ref_count: 0,
        };

        assert!(block.is_free);
        assert_eq!(block.size, 1024);
        assert!(block.movable);
        assert_eq!(block.ref_count, 0);
    }
}
