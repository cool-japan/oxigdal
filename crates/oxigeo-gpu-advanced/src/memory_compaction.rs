//! GPU memory defragmentation and compaction.
//!
//! This module provides memory compaction strategies to reduce fragmentation
//! and improve memory utilization in long-running GPU applications.

use crate::error::{GpuAdvancedError, Result};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{BufferUsages, Device, Queue};

/// Memory compaction manager
pub struct MemoryCompactor {
    /// Device for GPU memory operations (buffer allocation + copy encoding).
    device: Arc<Device>,
    /// Queue for GPU command submission (physical buffer migration).
    queue: Arc<Queue>,
    allocations: Arc<RwLock<AllocationMap>>,
    /// Optional physical backing buffer whose contents are migrated when
    /// compaction runs. When `None`, compaction is a bookkeeping-only
    /// offset-remap of the allocation table (no data movement to lie about).
    backing_buffer: Arc<RwLock<Option<Arc<wgpu::Buffer>>>>,
    config: CompactionConfig,
    stats: Arc<RwLock<CompactionStats>>,
}

impl MemoryCompactor {
    /// Create a new memory compactor
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, config: CompactionConfig) -> Self {
        Self {
            device,
            queue,
            allocations: Arc::new(RwLock::new(AllocationMap::new())),
            backing_buffer: Arc::new(RwLock::new(None)),
            config,
            stats: Arc::new(RwLock::new(CompactionStats::default())),
        }
    }

    /// Register the physical backing buffer that compaction should migrate.
    ///
    /// When a backing buffer is set, [`compact`](Self::compact) performs a real
    /// GPU→GPU copy of each live allocation into a fresh, contiguous buffer via
    /// `copy_buffer_to_buffer`, then swaps it in. The buffer must carry
    /// [`BufferUsages::COPY_SRC`]; the freshly allocated replacement is created
    /// with `COPY_DST | COPY_SRC | STORAGE`.
    pub fn set_backing_buffer(&self, buffer: Arc<wgpu::Buffer>) {
        *self.backing_buffer.write() = Some(buffer);
    }

    /// Return the current backing buffer, if one has been registered.
    pub fn backing_buffer(&self) -> Option<Arc<wgpu::Buffer>> {
        self.backing_buffer.read().clone()
    }

    /// Register an allocation
    pub fn register_allocation(&self, id: u64, offset: u64, size: u64, active: bool) {
        let mut allocs = self.allocations.write();
        allocs.insert(
            id,
            AllocationInfo {
                offset,
                size,
                active,
                last_access: Instant::now(),
            },
        );
    }

    /// Unregister an allocation
    pub fn unregister_allocation(&self, id: u64) {
        let mut allocs = self.allocations.write();
        allocs.remove(id);
    }

    /// Detect fragmentation
    pub fn detect_fragmentation(&self) -> FragmentationInfo {
        self.allocations.read().fragmentation()
    }

    /// Check if compaction is needed
    pub fn needs_compaction(&self) -> bool {
        let frag = self.detect_fragmentation();

        frag.fragmentation_ratio > self.config.fragmentation_threshold
            || frag.fragment_count > self.config.max_fragments
    }

    /// Perform memory compaction
    pub async fn compact(&self) -> Result<CompactionResult> {
        let start = Instant::now();

        // Detect fragmentation
        let before = self.detect_fragmentation();

        if !self.should_compact(&before) {
            return Ok(CompactionResult {
                success: false,
                duration: start.elapsed(),
                before: before.clone(),
                after: before,
                bytes_moved: 0,
                allocations_moved: 0,
            });
        }

        // Perform compaction based on strategy
        let result = match self.config.strategy {
            CompactionStrategy::Copy => self.compact_by_copy().await?,
            CompactionStrategy::InPlace => self.compact_in_place().await?,
            CompactionStrategy::Hybrid => self.compact_hybrid().await?,
        };

        // Update statistics
        let mut stats = self.stats.write();
        stats.total_compactions += 1;
        stats.total_duration += result.duration;
        stats.total_bytes_moved += result.bytes_moved;
        stats.last_compaction = Some(Instant::now());

        Ok(result)
    }

    /// Check if compaction should proceed
    fn should_compact(&self, frag: &FragmentationInfo) -> bool {
        if frag.fragmentation_ratio < self.config.fragmentation_threshold {
            return false;
        }

        // Check minimum interval
        let stats = self.stats.read();
        if let Some(last) = stats.last_compaction
            && last.elapsed() < self.config.min_compact_interval
        {
            return false;
        }

        true
    }

    /// Compact by copying live allocations into a fresh, contiguous buffer.
    ///
    /// This mutates the allocation table so that every live allocation is packed
    /// sequentially from offset 0 (eliminating inter-allocation gaps) and, when
    /// a physical backing buffer has been registered via
    /// [`set_backing_buffer`](Self::set_backing_buffer), issues real
    /// `copy_buffer_to_buffer` commands to migrate the data to the new layout.
    /// The `after` fragmentation figure is computed from the *mutated* table, so
    /// it reflects the true post-compaction state rather than a hardcoded zero.
    async fn compact_by_copy(&self) -> Result<CompactionResult> {
        let start = Instant::now();
        let before = self.detect_fragmentation();

        // Remap the allocation table to a contiguous layout.
        let (bytes_moved, allocations_moved, moves, new_total) = {
            let mut allocs = self.allocations.write();
            allocs.compact_offsets()
        };

        // Physically migrate data if a real backing buffer is registered.
        self.migrate_backing_buffer(new_total, &moves)?;

        // Recompute fragmentation from the mutated table (never fabricated).
        let after = self.detect_fragmentation();

        Ok(CompactionResult {
            success: true,
            duration: start.elapsed(),
            before,
            after,
            bytes_moved,
            allocations_moved,
        })
    }

    /// Compact in-place by remapping allocation offsets to a contiguous layout.
    ///
    /// For the bookkeeping model this performs the same offset packing as
    /// [`compact_by_copy`](Self::compact_by_copy); when a backing buffer is
    /// registered it migrates data into a fresh buffer (overlapping in-place GPU
    /// copies are unsafe, so a temporary destination is always used).
    async fn compact_in_place(&self) -> Result<CompactionResult> {
        // The safe realization of "in place" for GPU buffers is identical to the
        // copy strategy (a fresh destination avoids overlapping copies).
        self.compact_by_copy().await
    }

    /// Hybrid compaction strategy
    async fn compact_hybrid(&self) -> Result<CompactionResult> {
        let before = self.detect_fragmentation();

        // Use copy for high fragmentation, in-place for low
        if before.fragmentation_ratio > 0.5 {
            self.compact_by_copy().await
        } else {
            self.compact_in_place().await
        }
    }

    /// Physically migrate the registered backing buffer to a contiguous layout.
    ///
    /// Allocates a new buffer of `new_total` bytes, copies each moved region
    /// from the old buffer to its new offset, submits the commands, waits for
    /// completion, and swaps the buffer in. A no-op when no backing buffer is
    /// registered (pure bookkeeping compaction).
    ///
    /// # Errors
    ///
    /// Returns an error if any offset/size is not `COPY_BUFFER_ALIGNMENT`-
    /// aligned (a GPU copy requirement) or if the device poll fails.
    fn migrate_backing_buffer(&self, new_total: u64, moves: &[BufferMove]) -> Result<()> {
        let old_buffer = match self.backing_buffer() {
            Some(b) => b,
            // Bookkeeping-only compaction: nothing physical to move.
            None => return Ok(()),
        };

        if new_total == 0 {
            return Ok(());
        }

        // Validate alignment before touching the GPU.
        let align = wgpu::COPY_BUFFER_ALIGNMENT;
        for mv in moves {
            if mv.new_offset % align != 0 || mv.old_offset % align != 0 || mv.size % align != 0 {
                return Err(GpuAdvancedError::MemoryPoolError(format!(
                    "compaction requires {align}-byte aligned offsets/sizes; \
                     got old={} new={} size={}",
                    mv.old_offset, mv.new_offset, mv.size
                )));
            }
        }

        let aligned_total = new_total.div_ceil(align) * align;
        let new_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compacted_backing_buffer"),
            size: aligned_total,
            usage: BufferUsages::COPY_DST | BufferUsages::COPY_SRC | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compaction_encoder"),
            });
        for mv in moves {
            encoder.copy_buffer_to_buffer(
                &old_buffer,
                mv.old_offset,
                &new_buffer,
                mv.new_offset,
                mv.size,
            );
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| {
                GpuAdvancedError::MemoryPoolError(format!("compaction device poll failed: {e:?}"))
            })?;

        *self.backing_buffer.write() = Some(Arc::new(new_buffer));
        Ok(())
    }

    /// Get compaction statistics
    pub fn get_stats(&self) -> CompactionStats {
        self.stats.read().clone()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        *stats = CompactionStats::default();
    }
}

/// Memory allocation information
#[derive(Debug, Clone)]
struct AllocationInfo {
    offset: u64,
    size: u64,
    active: bool,
    /// Last access time (reserved for LRU eviction policies)
    #[allow(dead_code)]
    last_access: Instant,
}

/// A single region relocation produced by compaction: copy `size` bytes from
/// `old_offset` to `new_offset` in the backing buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BufferMove {
    old_offset: u64,
    new_offset: u64,
    size: u64,
}

/// Map of allocations
struct AllocationMap {
    allocations: BTreeMap<u64, AllocationInfo>,
}

impl AllocationMap {
    fn new() -> Self {
        Self {
            allocations: BTreeMap::new(),
        }
    }

    fn insert(&mut self, id: u64, info: AllocationInfo) {
        self.allocations.insert(id, info);
    }

    fn remove(&mut self, id: u64) {
        self.allocations.remove(&id);
    }

    fn sorted_allocations(&self) -> Vec<AllocationInfo> {
        let mut allocs: Vec<_> = self.allocations.values().cloned().collect();
        allocs.sort_by_key(|a| a.offset);
        allocs
    }

    /// Compute fragmentation over the currently-active allocations.
    fn fragmentation(&self) -> FragmentationInfo {
        let sorted = self.sorted_allocations();

        if sorted.is_empty() {
            return FragmentationInfo {
                total_size: 0,
                used_size: 0,
                wasted_size: 0,
                fragment_count: 0,
                largest_fragment: 0,
                fragmentation_ratio: 0.0,
            };
        }

        let mut used_size = 0u64;
        let mut wasted_size = 0u64;
        let mut fragment_count = 0usize;
        let mut largest_fragment = 0u64;
        let mut last_end = 0u64;

        for info in sorted.iter() {
            if info.active {
                let gap = info.offset.saturating_sub(last_end);

                if gap > 0 {
                    wasted_size += gap;
                    fragment_count += 1;
                    largest_fragment = largest_fragment.max(gap);
                }

                used_size += info.size;
                last_end = info.offset + info.size;
            }
        }

        let total_size = last_end;

        let fragmentation_ratio = if total_size > 0 {
            wasted_size as f64 / total_size as f64
        } else {
            0.0
        };

        FragmentationInfo {
            total_size,
            used_size,
            wasted_size,
            fragment_count,
            largest_fragment,
            fragmentation_ratio,
        }
    }

    /// Pack all live allocations into a contiguous layout starting at offset 0.
    ///
    /// Inactive allocations (freed space being reclaimed) are dropped from the
    /// table. Active allocations keep their relative order (sorted by current
    /// offset) and receive new sequential offsets. Returns:
    /// `(bytes_moved, allocations_moved, all_live_moves, new_total)` where
    /// `bytes_moved` / `allocations_moved` count only entries whose offset
    /// actually changed, and `all_live_moves` lists *every* surviving live
    /// allocation's `(old → new)` mapping (needed to repopulate a fresh backing
    /// buffer), and `new_total` is the packed byte size.
    fn compact_offsets(&mut self) -> (u64, usize, Vec<BufferMove>, u64) {
        // Reclaim inactive allocations.
        let dead: Vec<u64> = self
            .allocations
            .iter()
            .filter(|(_, info)| !info.active)
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            self.allocations.remove(&id);
        }

        // Order surviving allocations by their current offset.
        let mut ids: Vec<u64> = self.allocations.keys().copied().collect();
        ids.sort_by_key(|id| self.allocations.get(id).map(|i| i.offset).unwrap_or(0));

        let mut cursor = 0u64;
        let mut bytes_moved = 0u64;
        let mut allocations_moved = 0usize;
        let mut moves = Vec::with_capacity(ids.len());

        for id in ids {
            if let Some(info) = self.allocations.get_mut(&id) {
                let old = info.offset;
                let size = info.size;
                if old != cursor {
                    bytes_moved += size;
                    allocations_moved += 1;
                }
                moves.push(BufferMove {
                    old_offset: old,
                    new_offset: cursor,
                    size,
                });
                info.offset = cursor;
                cursor += size;
            }
        }

        (bytes_moved, allocations_moved, moves, cursor)
    }
}

/// Fragmentation information
#[derive(Debug, Clone)]
pub struct FragmentationInfo {
    /// Total memory span
    pub total_size: u64,
    /// Actually used memory
    pub used_size: u64,
    /// Wasted memory (gaps)
    pub wasted_size: u64,
    /// Number of fragments
    pub fragment_count: usize,
    /// Largest single fragment
    pub largest_fragment: u64,
    /// Fragmentation ratio (0.0 - 1.0)
    pub fragmentation_ratio: f64,
}

/// Compaction result
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// Whether compaction was successful
    pub success: bool,
    /// Time taken
    pub duration: Duration,
    /// Fragmentation before
    pub before: FragmentationInfo,
    /// Fragmentation after
    pub after: FragmentationInfo,
    /// Bytes moved during compaction
    pub bytes_moved: u64,
    /// Number of allocations moved
    pub allocations_moved: usize,
}

/// Compaction configuration
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Compaction strategy
    pub strategy: CompactionStrategy,
    /// Fragmentation threshold to trigger compaction (0.0 - 1.0)
    pub fragmentation_threshold: f64,
    /// Maximum number of fragments before compaction
    pub max_fragments: usize,
    /// Minimum interval between compactions
    pub min_compact_interval: Duration,
    /// Enable automatic compaction
    pub auto_compact: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            strategy: CompactionStrategy::Hybrid,
            fragmentation_threshold: 0.3,
            max_fragments: 100,
            min_compact_interval: Duration::from_secs(60),
            auto_compact: false,
        }
    }
}

/// Compaction strategy
#[derive(Debug, Clone, Copy)]
pub enum CompactionStrategy {
    /// Copy to new buffer
    Copy,
    /// Compact in-place
    InPlace,
    /// Hybrid approach
    Hybrid,
}

/// Compaction statistics
#[derive(Debug, Clone, Default)]
pub struct CompactionStats {
    /// Total number of compactions performed
    pub total_compactions: u64,
    /// Total time spent compacting
    pub total_duration: Duration,
    /// Total bytes moved
    pub total_bytes_moved: u64,
    /// Last compaction time
    pub last_compaction: Option<Instant>,
}

impl CompactionStats {
    /// Calculate average compaction duration
    pub fn average_duration(&self) -> Option<Duration> {
        if self.total_compactions > 0 {
            Some(self.total_duration / self.total_compactions as u32)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragmentation_detection() {
        let mut map = AllocationMap::new();

        // Create fragmented allocations
        map.insert(
            1,
            AllocationInfo {
                offset: 0,
                size: 100,
                active: true,
                last_access: Instant::now(),
            },
        );
        map.insert(
            2,
            AllocationInfo {
                offset: 200, // Gap of 100
                size: 100,
                active: true,
                last_access: Instant::now(),
            },
        );
        map.insert(
            3,
            AllocationInfo {
                offset: 400, // Gap of 100
                size: 100,
                active: true,
                last_access: Instant::now(),
            },
        );

        let sorted = map.sorted_allocations();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].offset, 0);
        assert_eq!(sorted[1].offset, 200);
        assert_eq!(sorted[2].offset, 400);
    }

    #[test]
    fn test_compaction_config_default() {
        let config = CompactionConfig::default();
        assert_eq!(config.fragmentation_threshold, 0.3);
        assert_eq!(config.max_fragments, 100);
        assert!(!config.auto_compact);
    }

    fn alloc(offset: u64, size: u64, active: bool) -> AllocationInfo {
        AllocationInfo {
            offset,
            size,
            active,
            last_access: Instant::now(),
        }
    }

    #[test]
    fn test_compact_offsets_eliminates_gaps() {
        // Three active allocations with 100-byte gaps between them.
        let mut map = AllocationMap::new();
        map.insert(1, alloc(0, 100, true));
        map.insert(2, alloc(200, 100, true));
        map.insert(3, alloc(400, 100, true));

        let before = map.fragmentation();
        assert_eq!(before.wasted_size, 200, "two 100-byte gaps expected");
        assert!(before.fragmentation_ratio > 0.0);

        let (bytes_moved, allocations_moved, moves, new_total) = map.compact_offsets();

        // Allocations 2 and 3 must be relocated; allocation 1 stays at 0.
        assert_eq!(allocations_moved, 2);
        assert_eq!(bytes_moved, 200);
        assert_eq!(new_total, 300);
        assert_eq!(
            moves.len(),
            3,
            "every live allocation is listed for migration"
        );

        // The mutated table must now be gap-free.
        let after = map.fragmentation();
        assert_eq!(
            after.wasted_size, 0,
            "compaction must genuinely eliminate wasted space"
        );
        assert_eq!(after.fragmentation_ratio, 0.0);

        // Offsets are contiguous: 0, 100, 200.
        let sorted = map.sorted_allocations();
        assert_eq!(sorted[0].offset, 0);
        assert_eq!(sorted[1].offset, 100);
        assert_eq!(sorted[2].offset, 200);
    }

    #[test]
    fn test_compact_offsets_reclaims_inactive() {
        // An inactive allocation sits between two active ones; compaction must
        // drop it and pack the survivors.
        let mut map = AllocationMap::new();
        map.insert(1, alloc(0, 100, true));
        map.insert(2, alloc(100, 100, false)); // freed / inactive
        map.insert(3, alloc(200, 100, true));

        let (_bytes, _moved, moves, new_total) = map.compact_offsets();

        // Only the two active allocations survive.
        assert_eq!(moves.len(), 2);
        assert_eq!(new_total, 200);
        let sorted = map.sorted_allocations();
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].offset, 0);
        assert_eq!(sorted[1].offset, 100);
        assert!(sorted.iter().all(|a| a.active));
    }

    /// Acquire a raw `(Device, Queue)` pair for GPU-gated tests, or `None` when
    /// no adapter is available.
    async fn try_device_queue() -> Option<(Arc<Device>, Arc<Queue>)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .ok()?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    }

    /// GPU-gated: register a real backing buffer, fragment it, compact, and
    /// verify the migrated data matches the pre-compaction contents at the new
    /// offsets. Requires GPU hardware.
    #[tokio::test]
    #[ignore = "requires GPU hardware"]
    async fn test_compact_physical_migration_preserves_data() {
        let (dev, queue) = match try_device_queue().await {
            Some(pair) => pair,
            None => return, // No GPU available.
        };

        // Backing buffer: [A=16 bytes][gap=16][B=16 bytes].
        let a_bytes: Vec<u8> = (0u8..16).collect();
        let b_bytes: Vec<u8> = (32u8..48).collect();
        let mut initial = vec![0u8; 48];
        initial[0..16].copy_from_slice(&a_bytes);
        initial[32..48].copy_from_slice(&b_bytes);

        let backing = Arc::new(dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test_backing"),
            size: 48,
            usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST | BufferUsages::STORAGE,
            mapped_at_creation: false,
        }));
        queue.write_buffer(&backing, 0, &initial);

        let config = CompactionConfig {
            fragmentation_threshold: 0.0,
            min_compact_interval: Duration::from_secs(0),
            ..CompactionConfig::default()
        };
        let compactor = MemoryCompactor::new(dev.clone(), queue.clone(), config);
        compactor.set_backing_buffer(Arc::clone(&backing));
        compactor.register_allocation(1, 0, 16, true);
        compactor.register_allocation(2, 32, 16, true);

        let result = compactor.compact().await.expect("compaction must succeed");
        assert!(result.success);
        assert_eq!(result.after.wasted_size, 0);

        // Read back the migrated buffer and verify A at 0, B at 16.
        let migrated = compactor.backing_buffer().expect("buffer present");
        let staging = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: 32,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = dev.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_buffer_to_buffer(&migrated, 0, &staging, 0, 32);
        queue.submit(std::iter::once(enc.finish()));
        let _ = dev.poll(wgpu::PollType::wait_indefinitely());
        let (tx, rx) = futures::channel::oneshot::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = dev.poll(wgpu::PollType::wait_indefinitely());
        rx.await.expect("channel").expect("map");
        let data = staging
            .slice(..)
            .get_mapped_range()
            .expect("range")
            .to_vec();
        assert_eq!(&data[0..16], &a_bytes[..], "A must migrate to offset 0");
        assert_eq!(&data[16..32], &b_bytes[..], "B must migrate to offset 16");
    }
}
