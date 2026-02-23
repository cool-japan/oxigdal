//! Prefetch hints and predictive loading

use crate::chunk::ChunkCoord;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

// ============================================================================
// Prefetch Hints
// ============================================================================

/// Access pattern hint for prefetching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    /// Sequential access pattern (row-major iteration)
    Sequential,
    /// Random access pattern
    Random,
    /// Strided access pattern with a specific stride
    Strided { stride: usize },
    /// Block access pattern (accessing chunks in a block)
    Block { dimensions: usize },
    /// Unknown pattern (use heuristics)
    Unknown,
}

/// Prefetch hint for a chunk
#[derive(Debug, Clone)]
pub struct PrefetchHint {
    /// Chunk coordinate to prefetch
    pub coord: ChunkCoord,
    /// Priority (lower = higher priority)
    pub priority: u32,
    /// Estimated access time (optional)
    pub estimated_access: Option<Duration>,
}

/// Prefetch manager for predictive loading
pub struct PrefetchManager {
    /// Queue of chunks to prefetch
    queue: Arc<RwLock<Vec<PrefetchHint>>>,
    /// Maximum queue size
    max_queue_size: usize,
    /// Access history for pattern detection
    access_history: Arc<RwLock<Vec<ChunkCoord>>>,
    /// Maximum history size
    max_history_size: usize,
    /// Detected access pattern
    detected_pattern: Arc<RwLock<AccessPattern>>,
    /// Prefetch statistics
    stats: Arc<PrefetchStats>,
}

/// Prefetch statistics
#[derive(Debug, Default)]
pub struct PrefetchStats {
    /// Total prefetch requests
    pub total_requests: AtomicU64,
    /// Successful prefetches (hit before actual access)
    pub hits: AtomicU64,
    /// Prefetches that were never used
    pub wasted: AtomicU64,
    /// Prefetches evicted before use
    pub evicted: AtomicU64,
}

impl Default for PrefetchManager {
    fn default() -> Self {
        Self::new(super::config::DEFAULT_PREFETCH_QUEUE_SIZE)
    }
}

impl PrefetchManager {
    /// Creates a new prefetch manager
    #[must_use]
    pub fn new(max_queue_size: usize) -> Self {
        Self {
            queue: Arc::new(RwLock::new(Vec::new())),
            max_queue_size,
            access_history: Arc::new(RwLock::new(Vec::new())),
            max_history_size: 256,
            detected_pattern: Arc::new(RwLock::new(AccessPattern::Unknown)),
            stats: Arc::new(PrefetchStats::default()),
        }
    }

    /// Records a chunk access for pattern detection
    pub fn record_access(&self, coord: &ChunkCoord) {
        if let Ok(mut history) = self.access_history.write() {
            history.push(coord.clone());
            if history.len() > self.max_history_size {
                history.remove(0);
            }

            // Update pattern detection
            if history.len() >= 4 {
                let pattern = self.detect_pattern(&history);
                if let Ok(mut detected) = self.detected_pattern.write() {
                    *detected = pattern;
                }
            }
        }
    }

    /// Detects the access pattern from history
    fn detect_pattern(&self, history: &[ChunkCoord]) -> AccessPattern {
        if history.len() < 4 {
            return AccessPattern::Unknown;
        }

        // Check for sequential pattern
        let mut is_sequential = true;
        for window in history.windows(2) {
            let (prev, curr) = (&window[0], &window[1]);
            if prev.ndim() != curr.ndim() {
                is_sequential = false;
                break;
            }

            // Check if only the last dimension increments by 1
            let prev_slice = prev.as_slice();
            let curr_slice = curr.as_slice();
            let ndim = prev_slice.len();

            let mut diff_count = 0;
            let mut last_diff = 0isize;
            for i in 0..ndim {
                let diff = curr_slice[i] as isize - prev_slice[i] as isize;
                if diff != 0 {
                    diff_count += 1;
                    last_diff = diff;
                }
            }

            if !(diff_count == 1 && last_diff == 1) {
                is_sequential = false;
                break;
            }
        }

        if is_sequential {
            return AccessPattern::Sequential;
        }

        // Check for strided pattern
        if history.len() >= 3 {
            let strides: Vec<_> = history
                .windows(2)
                .map(|w| {
                    w[1].as_slice()
                        .iter()
                        .zip(w[0].as_slice().iter())
                        .map(|(a, b)| (*a as isize).saturating_sub(*b as isize))
                        .sum::<isize>()
                })
                .collect();

            if !strides.is_empty() {
                let first_stride = strides[0];
                if strides.iter().all(|&s| s == first_stride) && first_stride > 1 {
                    return AccessPattern::Strided {
                        stride: first_stride.unsigned_abs(),
                    };
                }
            }
        }

        AccessPattern::Random
    }

    /// Generates prefetch hints based on current access and detected pattern
    pub fn generate_hints(&self, current: &ChunkCoord, count: usize) -> Vec<PrefetchHint> {
        let pattern = self
            .detected_pattern
            .read()
            .map(|p| *p)
            .unwrap_or(AccessPattern::Unknown);

        let mut hints = Vec::with_capacity(count);

        match pattern {
            AccessPattern::Sequential => {
                // Prefetch next chunks in sequence
                let coords = current.as_slice().to_vec();
                for i in 1..=count {
                    let mut next_coords = coords.clone();
                    if let Some(last) = next_coords.last_mut() {
                        *last = last.saturating_add(i);
                    }
                    hints.push(PrefetchHint {
                        coord: ChunkCoord::new_unchecked(next_coords),
                        priority: i as u32,
                        estimated_access: Some(Duration::from_millis(10 * i as u64)),
                    });
                }
            }
            AccessPattern::Strided { stride } => {
                let coords = current.as_slice().to_vec();
                for i in 1..=count {
                    let mut next_coords = coords.clone();
                    if let Some(last) = next_coords.last_mut() {
                        *last = last.saturating_add(stride * i);
                    }
                    hints.push(PrefetchHint {
                        coord: ChunkCoord::new_unchecked(next_coords),
                        priority: i as u32,
                        estimated_access: Some(Duration::from_millis(20 * i as u64)),
                    });
                }
            }
            AccessPattern::Block { dimensions } => {
                // Prefetch neighboring chunks in a block
                self.generate_block_hints(current, count.min(dimensions), &mut hints);
            }
            AccessPattern::Random | AccessPattern::Unknown => {
                // No prefetch for random/unknown patterns
            }
        }

        hints
    }

    /// Generates hints for block access pattern
    fn generate_block_hints(
        &self,
        center: &ChunkCoord,
        radius: usize,
        hints: &mut Vec<PrefetchHint>,
    ) {
        let center_coords = center.as_slice();
        let ndim = center_coords.len();

        if ndim == 0 || radius == 0 {
            return;
        }

        // Generate neighbors within radius
        let mut priority = 1u32;
        for d in 0..ndim {
            for offset in [1isize, -1isize] {
                let new_val = center_coords[d] as isize + offset;
                if new_val >= 0 {
                    let mut new_coords = center_coords.to_vec();
                    new_coords[d] = new_val as usize;
                    hints.push(PrefetchHint {
                        coord: ChunkCoord::new_unchecked(new_coords),
                        priority,
                        estimated_access: Some(Duration::from_millis(5)),
                    });
                    priority += 1;
                }
            }
        }
    }

    /// Adds hints to the prefetch queue
    pub fn enqueue(&self, hints: Vec<PrefetchHint>) {
        if let Ok(mut queue) = self.queue.write() {
            for hint in hints {
                if queue.len() >= self.max_queue_size {
                    self.stats.evicted.fetch_add(1, Ordering::Relaxed);
                    queue.remove(0); // Remove oldest
                }
                queue.push(hint);
                self.stats.total_requests.fetch_add(1, Ordering::Relaxed);
            }

            // Sort by priority
            queue.sort_by_key(|h| h.priority);
        }
    }

    /// Takes the next hint from the queue
    pub fn dequeue(&self) -> Option<PrefetchHint> {
        if let Ok(mut queue) = self.queue.write() {
            if !queue.is_empty() {
                return Some(queue.remove(0));
            }
        }
        None
    }

    /// Records a prefetch hit (prefetched chunk was accessed)
    pub fn record_hit(&self) {
        self.stats.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a wasted prefetch (prefetched chunk was never accessed)
    pub fn record_wasted(&self) {
        self.stats.wasted.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the current queue size
    #[must_use]
    pub fn queue_size(&self) -> usize {
        self.queue.read().map(|q| q.len()).unwrap_or(0)
    }

    /// Returns the detected access pattern
    #[must_use]
    pub fn detected_pattern(&self) -> AccessPattern {
        self.detected_pattern
            .read()
            .map(|p| *p)
            .unwrap_or(AccessPattern::Unknown)
    }

    /// Returns the prefetch statistics
    pub fn stats(&self) -> &PrefetchStats {
        &self.stats
    }

    /// Calculates the hit ratio
    #[must_use]
    pub fn hit_ratio(&self) -> f64 {
        let total = self.stats.total_requests.load(Ordering::Relaxed);
        let hits = self.stats.hits.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        hits as f64 / total as f64
    }

    /// Clears the prefetch queue
    pub fn clear(&self) {
        if let Ok(mut queue) = self.queue.write() {
            queue.clear();
        }
    }
}
