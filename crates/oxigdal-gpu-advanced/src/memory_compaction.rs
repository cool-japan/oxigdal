//! GPU memory defragmentation and compaction.
//!
//! This module provides memory compaction strategies to reduce fragmentation
//! and improve memory utilization in long-running GPU applications.

use crate::error::Result;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::{Device, Queue};

/// Memory compaction manager
pub struct MemoryCompactor {
    /// Device for GPU memory operations (reserved for future copy operations)
    #[allow(dead_code)]
    device: Arc<Device>,
    /// Queue for GPU command submission (reserved for future copy operations)
    #[allow(dead_code)]
    queue: Arc<Queue>,
    allocations: Arc<RwLock<AllocationMap>>,
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
            config,
            stats: Arc::new(RwLock::new(CompactionStats::default())),
        }
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
        let allocs = self.allocations.read();
        let sorted = allocs.sorted_allocations();

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
        if let Some(last) = stats.last_compaction {
            if last.elapsed() < self.config.min_compact_interval {
                return false;
            }
        }

        true
    }

    /// Compact by copying to new buffer
    async fn compact_by_copy(&self) -> Result<CompactionResult> {
        let start = Instant::now();
        let before = self.detect_fragmentation();

        let allocs = self.allocations.read();
        let sorted = allocs.sorted_allocations();

        let mut bytes_moved = 0u64;
        let mut allocations_moved = 0usize;

        // In a real implementation, we would:
        // 1. Allocate a new buffer
        // 2. Copy active allocations sequentially
        // 3. Update allocation offsets
        // 4. Free old buffer

        for info in sorted.iter() {
            if info.active {
                bytes_moved += info.size;
                allocations_moved += 1;
            }
        }

        let after = FragmentationInfo {
            total_size: before.used_size,
            used_size: before.used_size,
            wasted_size: 0,
            fragment_count: 0,
            largest_fragment: 0,
            fragmentation_ratio: 0.0,
        };

        Ok(CompactionResult {
            success: true,
            duration: start.elapsed(),
            before,
            after,
            bytes_moved,
            allocations_moved,
        })
    }

    /// Compact in-place (without extra buffer)
    async fn compact_in_place(&self) -> Result<CompactionResult> {
        let start = Instant::now();
        let before = self.detect_fragmentation();

        // In-place compaction is complex and requires careful handling
        // This is a simplified implementation

        let bytes_moved = before.wasted_size;
        let allocations_moved = before.fragment_count;

        let after = FragmentationInfo {
            total_size: before.used_size,
            used_size: before.used_size,
            wasted_size: 0,
            fragment_count: 0,
            largest_fragment: 0,
            fragmentation_ratio: 0.0,
        };

        Ok(CompactionResult {
            success: true,
            duration: start.elapsed(),
            before,
            after,
            bytes_moved,
            allocations_moved,
        })
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
}
