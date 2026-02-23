//! Intelligent prefetching algorithms for cloud storage
//!
//! This module provides comprehensive prefetching capabilities including:
//!
//! - **Predictive prefetching**: Pattern-based prediction of future accesses
//! - **Spatial locality**: Prefetch adjacent tiles and regions
//! - **Temporal patterns**: Detect time-based access patterns
//! - **Adaptive buffer sizing**: Dynamic buffer allocation based on workload
//! - **Priority-based prefetching**: Prioritize high-value prefetch targets
//! - **Memory-aware prefetching**: Respect memory constraints
//! - **Bandwidth-aware prefetching**: Throttle based on available bandwidth

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[cfg(feature = "async")]
use tokio::sync::RwLock;

use crate::error::Result;

// ============================================================================
// Access Pattern Types and Detection
// ============================================================================

/// Access pattern type for predictive prefetching
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AccessPattern {
    /// Sequential forward access (file_0, file_1, file_2, ...)
    SequentialForward,
    /// Sequential backward access (file_10, file_9, file_8, ...)
    SequentialBackward,
    /// Strided access (file_0, file_2, file_4, ...)
    Strided { stride: i64 },
    /// Random access pattern
    Random,
    /// Spatial access pattern (for tiles in 2D/3D)
    Spatial,
    /// Temporal periodic access (repeated access at intervals)
    TemporalPeriodic { period_ms: u64 },
    /// Burst access (many requests in short time)
    Burst,
    /// Unknown pattern (insufficient data)
    #[default]
    Unknown,
}

/// Priority level for prefetch operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum PrefetchPriority {
    /// Critical prefetch (user is likely waiting)
    Critical = 4,
    /// High priority (very likely to be accessed soon)
    High = 3,
    /// Medium priority (likely to be accessed)
    #[default]
    Medium = 2,
    /// Low priority (might be accessed)
    Low = 1,
    /// Background prefetch (speculative)
    Background = 0,
}

// ============================================================================
// Access Records and History
// ============================================================================

/// Access record for pattern analysis
#[derive(Debug, Clone)]
pub struct AccessRecord {
    /// Accessed key/path
    pub key: String,
    /// Access timestamp
    pub timestamp: Instant,
    /// Access coordinates for spatial data (x, y, z/level)
    pub coordinates: Option<(usize, usize, usize)>,
    /// Size of accessed data in bytes
    pub size: Option<usize>,
    /// Access latency (time to fetch)
    pub latency: Option<Duration>,
    /// Whether this was a cache hit
    pub cache_hit: bool,
}

impl AccessRecord {
    /// Creates a new access record
    #[must_use]
    pub fn new(key: String) -> Self {
        Self {
            key,
            timestamp: Instant::now(),
            coordinates: None,
            size: None,
            latency: None,
            cache_hit: false,
        }
    }

    /// Creates an access record with coordinates
    #[must_use]
    pub fn with_coordinates(key: String, x: usize, y: usize, z: usize) -> Self {
        Self {
            key,
            timestamp: Instant::now(),
            coordinates: Some((x, y, z)),
            size: None,
            latency: None,
            cache_hit: false,
        }
    }

    /// Sets the access size
    #[must_use]
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = Some(size);
        self
    }

    /// Sets the access latency
    #[must_use]
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = Some(latency);
        self
    }

    /// Sets whether this was a cache hit
    #[must_use]
    pub fn with_cache_hit(mut self, hit: bool) -> Self {
        self.cache_hit = hit;
        self
    }
}

/// Prefetch target with priority and metadata
#[derive(Debug, Clone)]
pub struct PrefetchTarget {
    /// Key to prefetch
    pub key: String,
    /// Priority level
    pub priority: PrefetchPriority,
    /// Expected coordinates (for spatial data)
    pub coordinates: Option<(usize, usize, usize)>,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Estimated size in bytes
    pub estimated_size: Option<usize>,
}

impl PrefetchTarget {
    /// Creates a new prefetch target
    #[must_use]
    pub fn new(key: String, priority: PrefetchPriority, confidence: f64) -> Self {
        Self {
            key,
            priority,
            coordinates: None,
            confidence,
            estimated_size: None,
        }
    }

    /// Sets coordinates for spatial data
    #[must_use]
    pub fn with_coordinates(mut self, x: usize, y: usize, z: usize) -> Self {
        self.coordinates = Some((x, y, z));
        self
    }

    /// Sets estimated size
    #[must_use]
    pub fn with_estimated_size(mut self, size: usize) -> Self {
        self.estimated_size = Some(size);
        self
    }
}

// ============================================================================
// Temporal Pattern Detection
// ============================================================================

/// Temporal pattern analyzer for detecting time-based access patterns
#[derive(Debug)]
pub struct TemporalPatternAnalyzer {
    /// Access intervals in milliseconds
    intervals: VecDeque<u64>,
    /// Maximum number of intervals to track
    max_intervals: usize,
    /// Detected periodic interval (if any)
    detected_period: Option<u64>,
    /// Burst detection threshold (accesses per second)
    burst_threshold: f64,
}

impl TemporalPatternAnalyzer {
    /// Creates a new temporal pattern analyzer
    #[must_use]
    pub fn new(max_intervals: usize) -> Self {
        Self {
            intervals: VecDeque::with_capacity(max_intervals),
            max_intervals,
            detected_period: None,
            burst_threshold: 10.0, // 10 accesses per second
        }
    }

    /// Records a new access interval
    pub fn record_interval(&mut self, interval_ms: u64) {
        if self.intervals.len() >= self.max_intervals {
            self.intervals.pop_front();
        }
        self.intervals.push_back(interval_ms);
        self.analyze_periodicity();
    }

    /// Analyzes intervals for periodic patterns
    fn analyze_periodicity(&mut self) {
        if self.intervals.len() < 5 {
            self.detected_period = None;
            return;
        }

        // Calculate mean and standard deviation
        let sum: u64 = self.intervals.iter().sum();
        let mean = sum as f64 / self.intervals.len() as f64;

        let variance: f64 = self
            .intervals
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / self.intervals.len() as f64;

        let std_dev = variance.sqrt();

        // If coefficient of variation is low, we have periodic access
        let cv = std_dev / mean;
        if cv < 0.3 && mean > 100.0 {
            // CV < 30% indicates regularity
            self.detected_period = Some(mean as u64);
        } else {
            self.detected_period = None;
        }
    }

    /// Detects if current access pattern is a burst
    #[must_use]
    pub fn is_burst(&self) -> bool {
        if self.intervals.len() < 3 {
            return false;
        }

        // Check recent intervals (last 5)
        let recent: Vec<_> = self.intervals.iter().rev().take(5).copied().collect();
        if recent.is_empty() {
            return false;
        }

        let avg_interval = recent.iter().sum::<u64>() as f64 / recent.len() as f64;

        // If average interval is less than 1000ms / threshold, it's a burst
        avg_interval < (1000.0 / self.burst_threshold)
    }

    /// Returns the detected periodic interval
    #[must_use]
    pub fn detected_period(&self) -> Option<u64> {
        self.detected_period
    }

    /// Predicts next access time based on periodicity
    #[must_use]
    pub fn predict_next_access(&self, last_access: Instant) -> Option<Instant> {
        self.detected_period
            .map(|period| last_access + Duration::from_millis(period))
    }
}

// ============================================================================
// Spatial Locality Analyzer
// ============================================================================

/// Spatial locality analyzer for tile-based data
#[derive(Debug)]
pub struct SpatialLocalityAnalyzer {
    /// Recent coordinate accesses
    coordinates: VecDeque<(usize, usize, usize)>,
    /// Maximum history size
    max_history: usize,
    /// Movement direction tracking (dx, dy)
    direction: Option<(i64, i64)>,
    /// Zoom level tracking
    zoom_direction: Option<i64>,
}

impl SpatialLocalityAnalyzer {
    /// Creates a new spatial locality analyzer
    #[must_use]
    pub fn new(max_history: usize) -> Self {
        Self {
            coordinates: VecDeque::with_capacity(max_history),
            max_history,
            direction: None,
            zoom_direction: None,
        }
    }

    /// Records a coordinate access
    pub fn record_access(&mut self, x: usize, y: usize, z: usize) {
        // Calculate movement direction from previous access
        if let Some(&(prev_x, prev_y, prev_z)) = self.coordinates.back() {
            let dx = x as i64 - prev_x as i64;
            let dy = y as i64 - prev_y as i64;
            let dz = z as i64 - prev_z as i64;

            // Update direction with exponential moving average
            if let Some((curr_dx, curr_dy)) = self.direction {
                self.direction = Some((
                    (curr_dx + dx) / 2, // Simple average
                    (curr_dy + dy) / 2,
                ));
            } else {
                self.direction = Some((dx, dy));
            }

            self.zoom_direction = if dz != 0 { Some(dz) } else { None };
        }

        if self.coordinates.len() >= self.max_history {
            self.coordinates.pop_front();
        }
        self.coordinates.push_back((x, y, z));
    }

    /// Predicts adjacent tiles to prefetch
    #[must_use]
    pub fn predict_adjacent(&self, count: usize) -> Vec<(usize, usize, usize)> {
        let Some(&(x, y, z)) = self.coordinates.back() else {
            return Vec::new();
        };

        let mut predictions = Vec::with_capacity(count);

        // Add tiles in direction of movement first
        if let Some((dx, dy)) = self.direction {
            // Normalize direction
            let norm_dx = dx.signum();
            let norm_dy = dy.signum();

            // Primary direction tiles
            if norm_dx != 0 {
                let new_x = if norm_dx > 0 {
                    x + 1
                } else {
                    x.saturating_sub(1)
                };
                predictions.push((new_x, y, z));
            }
            if norm_dy != 0 {
                let new_y = if norm_dy > 0 {
                    y + 1
                } else {
                    y.saturating_sub(1)
                };
                predictions.push((x, new_y, z));
            }
            // Diagonal in direction
            if norm_dx != 0 && norm_dy != 0 {
                let new_x = if norm_dx > 0 {
                    x + 1
                } else {
                    x.saturating_sub(1)
                };
                let new_y = if norm_dy > 0 {
                    y + 1
                } else {
                    y.saturating_sub(1)
                };
                predictions.push((new_x, new_y, z));
            }
        }

        // Add remaining adjacent tiles in spiral order
        let spiral_offsets: [(i64, i64); 8] = [
            (1, 0),
            (0, 1),
            (-1, 0),
            (0, -1),
            (1, 1),
            (-1, 1),
            (-1, -1),
            (1, -1),
        ];

        for (ox, oy) in spiral_offsets {
            if predictions.len() >= count {
                break;
            }
            let new_x = (x as i64 + ox).max(0) as usize;
            let new_y = (y as i64 + oy).max(0) as usize;
            let coord = (new_x, new_y, z);
            if !predictions.contains(&coord) {
                predictions.push(coord);
            }
        }

        // Add zoom level transitions if detected
        if let Some(dz) = self.zoom_direction {
            let new_z = (z as i64 + dz).max(0) as usize;
            if predictions.len() < count {
                predictions.push((x, y, new_z));
            }
        }

        predictions.truncate(count);
        predictions
    }

    /// Gets the current movement direction
    #[must_use]
    pub fn movement_direction(&self) -> Option<(i64, i64)> {
        self.direction
    }
}

// ============================================================================
// Pattern Analyzer
// ============================================================================

/// Comprehensive pattern analyzer for predictive prefetching
pub struct PatternAnalyzer {
    /// Recent access history
    history: VecDeque<AccessRecord>,
    /// Maximum history size
    max_history: usize,
    /// Current detected pattern
    current_pattern: AccessPattern,
    /// Temporal pattern analyzer
    temporal: TemporalPatternAnalyzer,
    /// Spatial locality analyzer
    spatial: SpatialLocalityAnalyzer,
    /// Key frequency map for hot spot detection
    key_frequency: HashMap<String, u64>,
    /// Stride detector
    detected_stride: Option<i64>,
}

impl PatternAnalyzer {
    /// Creates a new pattern analyzer
    #[must_use]
    pub fn new(max_history: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_history),
            max_history,
            current_pattern: AccessPattern::Unknown,
            temporal: TemporalPatternAnalyzer::new(50),
            spatial: SpatialLocalityAnalyzer::new(20),
            key_frequency: HashMap::new(),
            detected_stride: None,
        }
    }

    /// Records an access and updates pattern analysis
    pub fn record_access(&mut self, record: AccessRecord) {
        // Update temporal patterns
        if let Some(last) = self.history.back() {
            let interval = record.timestamp.duration_since(last.timestamp);
            self.temporal.record_interval(interval.as_millis() as u64);
        }

        // Update spatial patterns
        if let Some((x, y, z)) = record.coordinates {
            self.spatial.record_access(x, y, z);
        }

        // Update frequency map
        *self.key_frequency.entry(record.key.clone()).or_insert(0) += 1;

        // Add to history
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(record);

        // Analyze patterns
        self.analyze_patterns();
    }

    /// Analyzes all patterns to determine current access pattern
    fn analyze_patterns(&mut self) {
        if self.history.len() < 3 {
            self.current_pattern = AccessPattern::Unknown;
            return;
        }

        // Check for structural patterns first (these take precedence over temporal patterns)

        // Check for spatial pattern
        if self.is_spatial() {
            self.current_pattern = AccessPattern::Spatial;
            return;
        }

        // Check for strided pattern
        if let Some(stride) = self.detect_stride() {
            self.detected_stride = Some(stride);
            self.current_pattern = AccessPattern::Strided { stride };
            return;
        }

        // Check for sequential patterns
        if self.is_sequential_forward() {
            self.current_pattern = AccessPattern::SequentialForward;
            return;
        }

        if self.is_sequential_backward() {
            self.current_pattern = AccessPattern::SequentialBackward;
            return;
        }

        // Check for temporal patterns (only if no structural pattern is found)

        // Check for temporal periodic pattern
        if let Some(period) = self.temporal.detected_period() {
            self.current_pattern = AccessPattern::TemporalPeriodic { period_ms: period };
            return;
        }

        // Check for burst pattern (only if no other pattern is detected)
        // Burst should be a fallback when timing is rapid but there's no other pattern
        if self.temporal.is_burst() {
            self.current_pattern = AccessPattern::Burst;
            return;
        }

        self.current_pattern = AccessPattern::Random;
    }

    /// Checks for forward sequential pattern
    fn is_sequential_forward(&self) -> bool {
        let recent: Vec<_> = self.history.iter().rev().take(5).collect();
        if recent.len() < 3 {
            return false;
        }

        let mut sequential_count = 0;
        for window in recent.windows(2) {
            if let (Some(&a), Some(&b)) = (window.first(), window.get(1)) {
                if let (Some(na), Some(nb)) = (
                    extract_trailing_number(&a.key),
                    extract_trailing_number(&b.key),
                ) {
                    // recent is reversed, so for forward pattern: na - 1 == nb
                    // (e.g., file_3 -> file_2 -> file_1 is forward in original order)
                    if na == nb + 1 {
                        sequential_count += 1;
                    }
                }
            }
        }

        sequential_count >= (recent.len() - 1) / 2
    }

    /// Checks for backward sequential pattern
    fn is_sequential_backward(&self) -> bool {
        let recent: Vec<_> = self.history.iter().rev().take(5).collect();
        if recent.len() < 3 {
            return false;
        }

        let mut backward_count = 0;
        for window in recent.windows(2) {
            if let (Some(&a), Some(&b)) = (window.first(), window.get(1)) {
                if let (Some(na), Some(nb)) = (
                    extract_trailing_number(&a.key),
                    extract_trailing_number(&b.key),
                ) {
                    // recent is reversed, so for backward pattern: na + 1 == nb
                    // (e.g., file_3 -> file_4 -> file_5 is backward in original order)
                    if nb == na + 1 {
                        backward_count += 1;
                    }
                }
            }
        }

        backward_count >= (recent.len() - 1) / 2
    }

    /// Detects strided access pattern
    fn detect_stride(&self) -> Option<i64> {
        let recent: Vec<_> = self.history.iter().rev().take(6).collect();
        if recent.len() < 4 {
            return None;
        }

        // Extract numbers from keys
        let numbers: Vec<i64> = recent
            .iter()
            .filter_map(|r| extract_trailing_number(&r.key))
            .collect();

        if numbers.len() < 4 {
            return None;
        }

        // Calculate differences
        let diffs: Vec<i64> = numbers.windows(2).map(|w| w[0] - w[1]).collect();

        if diffs.is_empty() {
            return None;
        }

        // Check if differences are consistent
        let first_diff = diffs[0];
        if first_diff.abs() <= 1 {
            // Not a stride pattern, just sequential
            return None;
        }

        let consistent = diffs.iter().all(|&d| d == first_diff);
        if consistent { Some(first_diff) } else { None }
    }

    /// Checks for spatial access pattern
    fn is_spatial(&self) -> bool {
        let coords_count = self
            .history
            .iter()
            .rev()
            .take(5)
            .filter(|r| r.coordinates.is_some())
            .count();

        coords_count >= 3
    }

    /// Returns the current detected pattern
    #[must_use]
    pub fn current_pattern(&self) -> AccessPattern {
        self.current_pattern
    }

    /// Predicts next likely accesses with priorities
    #[must_use]
    pub fn predict_next(&self, count: usize) -> Vec<PrefetchTarget> {
        match self.current_pattern {
            AccessPattern::SequentialForward => self.predict_sequential_forward(count),
            AccessPattern::SequentialBackward => self.predict_sequential_backward(count),
            AccessPattern::Strided { stride } => self.predict_strided(count, stride),
            AccessPattern::Spatial => self.predict_spatial(count),
            AccessPattern::Burst | AccessPattern::Random => self.predict_hot_spots(count),
            AccessPattern::TemporalPeriodic { .. } => self.predict_recent(count),
            AccessPattern::Unknown => Vec::new(),
        }
    }

    /// Predicts forward sequential accesses
    fn predict_sequential_forward(&self, count: usize) -> Vec<PrefetchTarget> {
        let Some(last) = self.history.back() else {
            return Vec::new();
        };

        let mut predictions = Vec::new();
        for i in 1..=count {
            if let Some(next_key) = increment_key(&last.key, i as i64) {
                let priority = if i == 1 {
                    PrefetchPriority::Critical
                } else if i <= 3 {
                    PrefetchPriority::High
                } else {
                    PrefetchPriority::Medium
                };
                let confidence = 1.0 - (i as f64 * 0.1).min(0.5);
                predictions.push(PrefetchTarget::new(next_key, priority, confidence));
            }
        }
        predictions
    }

    /// Predicts backward sequential accesses
    fn predict_sequential_backward(&self, count: usize) -> Vec<PrefetchTarget> {
        let Some(last) = self.history.back() else {
            return Vec::new();
        };

        let mut predictions = Vec::new();
        for i in 1..=count {
            if let Some(next_key) = increment_key(&last.key, -(i as i64)) {
                let priority = if i == 1 {
                    PrefetchPriority::Critical
                } else if i <= 3 {
                    PrefetchPriority::High
                } else {
                    PrefetchPriority::Medium
                };
                let confidence = 1.0 - (i as f64 * 0.1).min(0.5);
                predictions.push(PrefetchTarget::new(next_key, priority, confidence));
            }
        }
        predictions
    }

    /// Predicts strided accesses
    fn predict_strided(&self, count: usize, stride: i64) -> Vec<PrefetchTarget> {
        let Some(last) = self.history.back() else {
            return Vec::new();
        };

        let mut predictions = Vec::new();
        for i in 1..=count {
            if let Some(next_key) = increment_key(&last.key, stride * i as i64) {
                let priority = if i == 1 {
                    PrefetchPriority::High
                } else {
                    PrefetchPriority::Medium
                };
                let confidence = 0.8 - (i as f64 * 0.1).min(0.4);
                predictions.push(PrefetchTarget::new(next_key, priority, confidence));
            }
        }
        predictions
    }

    /// Predicts spatial accesses (adjacent tiles)
    fn predict_spatial(&self, count: usize) -> Vec<PrefetchTarget> {
        let coords = self.spatial.predict_adjacent(count);

        coords
            .into_iter()
            .enumerate()
            .map(|(i, (x, y, z))| {
                let key = format!("tile_{x}_{y}_{z}");
                let priority = if i == 0 {
                    PrefetchPriority::High
                } else if i < 4 {
                    PrefetchPriority::Medium
                } else {
                    PrefetchPriority::Low
                };
                let confidence = 0.9 - (i as f64 * 0.1).min(0.5);
                PrefetchTarget::new(key, priority, confidence).with_coordinates(x, y, z)
            })
            .collect()
    }

    /// Predicts based on hot spots (frequently accessed)
    fn predict_hot_spots(&self, count: usize) -> Vec<PrefetchTarget> {
        let mut freq_vec: Vec<_> = self.key_frequency.iter().collect();
        freq_vec.sort_by(|a, b| b.1.cmp(a.1));

        freq_vec
            .into_iter()
            .take(count)
            .map(|(key, freq)| {
                let priority = if *freq > 10 {
                    PrefetchPriority::High
                } else if *freq > 5 {
                    PrefetchPriority::Medium
                } else {
                    PrefetchPriority::Low
                };
                let confidence = (*freq as f64 / 20.0).min(0.8);
                PrefetchTarget::new(key.clone(), priority, confidence)
            })
            .collect()
    }

    /// Predicts based on recent accesses
    fn predict_recent(&self, count: usize) -> Vec<PrefetchTarget> {
        self.history
            .iter()
            .rev()
            .take(count)
            .enumerate()
            .map(|(i, record)| {
                let priority = if i == 0 {
                    PrefetchPriority::Medium
                } else {
                    PrefetchPriority::Low
                };
                PrefetchTarget::new(record.key.clone(), priority, 0.5)
            })
            .collect()
    }
}

// ============================================================================
// Adaptive Buffer Management
// ============================================================================

/// Statistics for adaptive buffer sizing
#[derive(Debug, Default)]
pub struct BufferStats {
    /// Total prefetch operations
    pub total_prefetches: AtomicU64,
    /// Successful prefetches (data was used)
    pub successful_prefetches: AtomicU64,
    /// Wasted prefetches (data was not used)
    pub wasted_prefetches: AtomicU64,
    /// Average prefetch latency in microseconds
    pub avg_latency_us: AtomicU64,
    /// Current buffer utilization percentage
    pub utilization_pct: AtomicU64,
}

impl BufferStats {
    /// Records a prefetch operation
    pub fn record_prefetch(&self, used: bool, latency_us: u64) {
        self.total_prefetches.fetch_add(1, Ordering::Relaxed);
        if used {
            self.successful_prefetches.fetch_add(1, Ordering::Relaxed);
        } else {
            self.wasted_prefetches.fetch_add(1, Ordering::Relaxed);
        }

        // Update average latency (simple moving average)
        let current = self.avg_latency_us.load(Ordering::Relaxed);
        let new_avg = (current * 9 + latency_us) / 10;
        self.avg_latency_us.store(new_avg, Ordering::Relaxed);
    }

    /// Calculates the hit rate
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.total_prefetches.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let successful = self.successful_prefetches.load(Ordering::Relaxed);
        successful as f64 / total as f64
    }
}

/// Adaptive buffer sizer that adjusts buffer size based on workload
pub struct AdaptiveBufferSizer {
    /// Minimum buffer size in bytes
    min_size: usize,
    /// Maximum buffer size in bytes
    max_size: usize,
    /// Current buffer size in bytes
    current_size: AtomicUsize,
    /// Buffer statistics
    stats: Arc<BufferStats>,
    /// Target hit rate
    target_hit_rate: f64,
    /// Last adjustment time
    last_adjustment: RwLock<Instant>,
    /// Adjustment interval
    adjustment_interval: Duration,
}

impl AdaptiveBufferSizer {
    /// Creates a new adaptive buffer sizer
    #[must_use]
    pub fn new(min_size: usize, max_size: usize) -> Self {
        Self {
            min_size,
            max_size,
            current_size: AtomicUsize::new(min_size),
            stats: Arc::new(BufferStats::default()),
            target_hit_rate: 0.7,
            last_adjustment: RwLock::new(Instant::now()),
            adjustment_interval: Duration::from_secs(10),
        }
    }

    /// Gets the current buffer size
    #[must_use]
    pub fn current_size(&self) -> usize {
        self.current_size.load(Ordering::Relaxed)
    }

    /// Gets the buffer statistics
    #[must_use]
    pub fn stats(&self) -> Arc<BufferStats> {
        Arc::clone(&self.stats)
    }

    /// Adjusts the buffer size based on statistics
    pub async fn maybe_adjust(&self) {
        let last = *self.last_adjustment.read().await;
        if last.elapsed() < self.adjustment_interval {
            return;
        }

        let hit_rate = self.stats.hit_rate();
        let current = self.current_size.load(Ordering::Relaxed);

        let new_size = if hit_rate < self.target_hit_rate - 0.1 {
            // Hit rate too low, increase buffer
            (current as f64 * 1.5) as usize
        } else if hit_rate > self.target_hit_rate + 0.1 {
            // Hit rate high enough, we might be able to reduce buffer
            (current as f64 * 0.9) as usize
        } else {
            current
        };

        let clamped = new_size.clamp(self.min_size, self.max_size);
        self.current_size.store(clamped, Ordering::Relaxed);
        *self.last_adjustment.write().await = Instant::now();

        tracing::debug!(
            "Adjusted prefetch buffer: {} -> {} bytes (hit_rate: {:.2})",
            current,
            clamped,
            hit_rate
        );
    }
}

// ============================================================================
// Memory and Bandwidth Awareness
// ============================================================================

/// Memory-aware prefetch controller
pub struct MemoryAwarePrefetcher {
    /// Maximum memory usage for prefetch buffer in bytes
    max_memory: usize,
    /// Current memory usage in bytes
    current_usage: AtomicUsize,
    /// Memory pressure threshold (0.0 - 1.0)
    pressure_threshold: f64,
}

impl MemoryAwarePrefetcher {
    /// Creates a new memory-aware prefetcher
    #[must_use]
    pub fn new(max_memory: usize) -> Self {
        Self {
            max_memory,
            current_usage: AtomicUsize::new(0),
            pressure_threshold: 0.8,
        }
    }

    /// Checks if prefetching is allowed given current memory pressure
    #[must_use]
    pub fn can_prefetch(&self, size: usize) -> bool {
        let current = self.current_usage.load(Ordering::Relaxed);
        let usage_ratio = current as f64 / self.max_memory as f64;

        // Don't prefetch if we're under memory pressure
        if usage_ratio > self.pressure_threshold {
            return false;
        }

        // Check if we have room for this prefetch
        current + size <= self.max_memory
    }

    /// Allocates memory for a prefetch
    pub fn allocate(&self, size: usize) -> bool {
        let current = self.current_usage.load(Ordering::Relaxed);
        if current + size > self.max_memory {
            return false;
        }
        self.current_usage.fetch_add(size, Ordering::Relaxed);
        true
    }

    /// Releases memory from a completed/evicted prefetch
    pub fn release(&self, size: usize) {
        self.current_usage.fetch_sub(size, Ordering::Relaxed);
    }

    /// Gets the current memory usage
    #[must_use]
    pub fn current_usage(&self) -> usize {
        self.current_usage.load(Ordering::Relaxed)
    }

    /// Gets the memory pressure (0.0 - 1.0)
    #[must_use]
    pub fn pressure(&self) -> f64 {
        let current = self.current_usage.load(Ordering::Relaxed);
        current as f64 / self.max_memory as f64
    }
}

/// Bandwidth-aware prefetch controller
pub struct BandwidthAwarePrefetcher {
    /// Maximum bandwidth in bytes per second
    max_bandwidth: usize,
    /// Bandwidth used in current window
    bandwidth_used: AtomicUsize,
    /// Window start time
    window_start: RwLock<Instant>,
    /// Window duration
    window_duration: Duration,
    /// Bandwidth samples for estimation
    samples: RwLock<VecDeque<(Instant, usize)>>,
}

impl BandwidthAwarePrefetcher {
    /// Creates a new bandwidth-aware prefetcher
    #[must_use]
    pub fn new(max_bandwidth: usize) -> Self {
        Self {
            max_bandwidth,
            bandwidth_used: AtomicUsize::new(0),
            window_start: RwLock::new(Instant::now()),
            window_duration: Duration::from_secs(1),
            samples: RwLock::new(VecDeque::with_capacity(100)),
        }
    }

    /// Checks if bandwidth allows prefetching
    pub async fn can_prefetch(&self, size: usize) -> bool {
        self.maybe_reset_window().await;
        let used = self.bandwidth_used.load(Ordering::Relaxed);
        used + size <= self.max_bandwidth
    }

    /// Records bandwidth usage
    pub async fn record_usage(&self, size: usize) {
        self.maybe_reset_window().await;
        self.bandwidth_used.fetch_add(size, Ordering::Relaxed);

        // Record sample for bandwidth estimation
        let mut samples = self.samples.write().await;
        samples.push_back((Instant::now(), size));
        if samples.len() > 100 {
            samples.pop_front();
        }
    }

    /// Resets bandwidth window if needed
    async fn maybe_reset_window(&self) {
        let start = *self.window_start.read().await;
        if start.elapsed() >= self.window_duration {
            self.bandwidth_used.store(0, Ordering::Relaxed);
            *self.window_start.write().await = Instant::now();
        }
    }

    /// Estimates current bandwidth usage rate
    pub async fn estimated_bandwidth(&self) -> usize {
        let samples = self.samples.read().await;
        if samples.len() < 2 {
            return 0;
        }

        let first = samples.front();
        let last = samples.back();

        if let (Some((first_time, _)), Some((last_time, _))) = (first, last) {
            let duration = last_time.duration_since(*first_time);
            if duration.as_secs_f64() > 0.0 {
                let total_bytes: usize = samples.iter().map(|(_, s)| s).sum();
                return (total_bytes as f64 / duration.as_secs_f64()) as usize;
            }
        }
        0
    }

    /// Gets the remaining bandwidth in the current window
    #[must_use]
    pub fn remaining_bandwidth(&self) -> usize {
        let used = self.bandwidth_used.load(Ordering::Relaxed);
        self.max_bandwidth.saturating_sub(used)
    }
}

// ============================================================================
// Prefetch Configuration and Manager
// ============================================================================

/// Prefetch configuration
#[derive(Debug, Clone)]
pub struct PrefetchConfig {
    /// Enable prefetching
    pub enabled: bool,
    /// Number of items to prefetch ahead
    pub prefetch_count: usize,
    /// Maximum concurrent prefetch operations
    pub max_concurrent: usize,
    /// Bandwidth limit in bytes per second (None = unlimited)
    pub bandwidth_limit: Option<usize>,
    /// Memory limit in bytes for prefetch buffer
    pub memory_limit: usize,
    /// Enable pattern-based prediction
    pub pattern_prediction: bool,
    /// Enable adaptive buffer sizing
    pub adaptive_sizing: bool,
    /// Minimum confidence threshold for prefetching
    pub min_confidence: f64,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefetch_count: 5,
            max_concurrent: 4,
            bandwidth_limit: None,
            memory_limit: 64 * 1024 * 1024, // 64 MB
            pattern_prediction: true,
            adaptive_sizing: true,
            min_confidence: 0.3,
        }
    }
}

impl PrefetchConfig {
    /// Creates a new prefetch configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables prefetching
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the prefetch count
    #[must_use]
    pub fn with_prefetch_count(mut self, count: usize) -> Self {
        self.prefetch_count = count;
        self
    }

    /// Sets the maximum concurrent prefetch operations
    #[must_use]
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }

    /// Sets the bandwidth limit
    #[must_use]
    pub fn with_bandwidth_limit(mut self, limit: usize) -> Self {
        self.bandwidth_limit = Some(limit);
        self
    }

    /// Sets the memory limit
    #[must_use]
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = limit;
        self
    }

    /// Enables adaptive sizing
    #[must_use]
    pub fn with_adaptive_sizing(mut self, enabled: bool) -> Self {
        self.adaptive_sizing = enabled;
        self
    }
}

/// Priority queue for prefetch targets
#[cfg(feature = "async")]
pub struct PrefetchQueue {
    /// Queued targets by priority
    queues: RwLock<BTreeMap<PrefetchPriority, VecDeque<PrefetchTarget>>>,
    /// Set of queued keys (for deduplication)
    queued_keys: RwLock<std::collections::HashSet<String>>,
}

#[cfg(feature = "async")]
impl PrefetchQueue {
    /// Creates a new prefetch queue
    #[must_use]
    pub fn new() -> Self {
        Self {
            queues: RwLock::new(BTreeMap::new()),
            queued_keys: RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// Enqueues a prefetch target
    pub async fn enqueue(&self, target: PrefetchTarget) {
        let mut keys = self.queued_keys.write().await;
        if keys.contains(&target.key) {
            return; // Already queued
        }
        keys.insert(target.key.clone());

        let mut queues = self.queues.write().await;
        queues
            .entry(target.priority)
            .or_insert_with(VecDeque::new)
            .push_back(target);
    }

    /// Dequeues the highest priority target
    pub async fn dequeue(&self) -> Option<PrefetchTarget> {
        let mut queues = self.queues.write().await;

        // Iterate in reverse order (highest priority first)
        for (_, queue) in queues.iter_mut().rev() {
            if let Some(target) = queue.pop_front() {
                let mut keys = self.queued_keys.write().await;
                keys.remove(&target.key);
                return Some(target);
            }
        }
        None
    }

    /// Returns the total queue length
    pub async fn len(&self) -> usize {
        self.queued_keys.read().await.len()
    }

    /// Checks if the queue is empty
    pub async fn is_empty(&self) -> bool {
        self.queued_keys.read().await.is_empty()
    }
}

#[cfg(feature = "async")]
impl Default for PrefetchQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Comprehensive prefetch manager
#[cfg(feature = "async")]
pub struct PrefetchManager {
    /// Configuration
    config: PrefetchConfig,
    /// Pattern analyzer
    analyzer: Arc<RwLock<PatternAnalyzer>>,
    /// Prefetch queue
    queue: Arc<PrefetchQueue>,
    /// Memory controller
    memory: Arc<MemoryAwarePrefetcher>,
    /// Bandwidth controller
    bandwidth: Arc<BandwidthAwarePrefetcher>,
    /// Adaptive buffer sizer
    buffer_sizer: Option<Arc<AdaptiveBufferSizer>>,
}

#[cfg(feature = "async")]
impl PrefetchManager {
    /// Creates a new prefetch manager
    #[must_use]
    pub fn new(config: PrefetchConfig) -> Self {
        let memory = Arc::new(MemoryAwarePrefetcher::new(config.memory_limit));

        let bandwidth = Arc::new(BandwidthAwarePrefetcher::new(
            config.bandwidth_limit.unwrap_or(usize::MAX),
        ));

        let buffer_sizer = if config.adaptive_sizing {
            Some(Arc::new(AdaptiveBufferSizer::new(
                config.memory_limit / 4,
                config.memory_limit,
            )))
        } else {
            None
        };

        Self {
            config,
            analyzer: Arc::new(RwLock::new(PatternAnalyzer::new(100))),
            queue: Arc::new(PrefetchQueue::new()),
            memory,
            bandwidth,
            buffer_sizer,
        }
    }

    /// Records an access and triggers prefetching
    pub async fn record_access(&self, record: AccessRecord) -> Vec<PrefetchTarget> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut analyzer = self.analyzer.write().await;
        analyzer.record_access(record);

        if self.config.pattern_prediction {
            let mut predictions = analyzer.predict_next(self.config.prefetch_count);

            // Filter by confidence
            predictions.retain(|t| t.confidence >= self.config.min_confidence);

            // Enqueue predictions
            for target in &predictions {
                self.queue.enqueue(target.clone()).await;
            }

            predictions
        } else {
            Vec::new()
        }
    }

    /// Checks if prefetching is allowed (memory and bandwidth)
    pub async fn can_prefetch(&self, size: usize) -> bool {
        // Check memory
        if !self.memory.can_prefetch(size) {
            return false;
        }

        // Check bandwidth
        if !self.bandwidth.can_prefetch(size).await {
            return false;
        }

        true
    }

    /// Records a completed prefetch
    pub async fn record_prefetch(&self, size: usize, used: bool, latency: Duration) {
        self.bandwidth.record_usage(size).await;

        if let Some(ref sizer) = self.buffer_sizer {
            sizer
                .stats()
                .record_prefetch(used, latency.as_micros() as u64);
            sizer.maybe_adjust().await;
        }
    }

    /// Allocates memory for a prefetch
    pub fn allocate_memory(&self, size: usize) -> bool {
        self.memory.allocate(size)
    }

    /// Releases memory from a prefetch
    pub fn release_memory(&self, size: usize) {
        self.memory.release(size);
    }

    /// Gets the next prefetch target from the queue
    pub async fn next_target(&self) -> Option<PrefetchTarget> {
        self.queue.dequeue().await
    }

    /// Returns the current access pattern
    pub async fn current_pattern(&self) -> AccessPattern {
        self.analyzer.read().await.current_pattern()
    }

    /// Returns the queue length
    pub async fn queue_len(&self) -> usize {
        self.queue.len().await
    }

    /// Gets memory pressure (0.0 - 1.0)
    #[must_use]
    pub fn memory_pressure(&self) -> f64 {
        self.memory.pressure()
    }

    /// Gets remaining bandwidth
    #[must_use]
    pub fn remaining_bandwidth(&self) -> usize {
        self.bandwidth.remaining_bandwidth()
    }

    /// Gets the prefetch hit rate
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        self.buffer_sizer
            .as_ref()
            .map_or(0.0, |s| s.stats().hit_rate())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extracts trailing number from a key
fn extract_trailing_number(key: &str) -> Option<i64> {
    let parts: Vec<&str> = key
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .collect();
    parts
        .iter()
        .rev()
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

/// Increments the trailing number in a key
fn increment_key(key: &str, offset: i64) -> Option<String> {
    let parts: Vec<&str> = key.split('_').collect();

    for i in (0..parts.len()).rev() {
        if let Ok(num) = parts[i].parse::<i64>() {
            let new_num = num + offset;
            if new_num < 0 {
                return None;
            }
            let mut new_parts: Vec<String> = parts.iter().map(|s| (*s).to_string()).collect();
            new_parts[i] = new_num.to_string();
            return Some(new_parts.join("_"));
        }
    }
    None
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_trailing_number() {
        assert_eq!(extract_trailing_number("file_5"), Some(5));
        assert_eq!(extract_trailing_number("tile_10_20_3"), Some(3));
        assert_eq!(extract_trailing_number("data"), None);
    }

    #[test]
    fn test_increment_key() {
        assert_eq!(increment_key("file_5", 1), Some("file_6".to_string()));
        assert_eq!(increment_key("file_5", -1), Some("file_4".to_string()));
        assert_eq!(
            increment_key("tile_10_20", 1),
            Some("tile_10_21".to_string())
        );
        assert_eq!(increment_key("file_0", -1), None);
    }

    #[test]
    fn test_temporal_pattern_analyzer() {
        let mut analyzer = TemporalPatternAnalyzer::new(20);

        // Record consistent intervals
        for _ in 0..10 {
            analyzer.record_interval(1000); // 1 second intervals
        }

        assert!(analyzer.detected_period().is_some());
        let period = analyzer.detected_period();
        assert!(period.is_some_and(|p| (p as i64 - 1000).abs() < 100));
    }

    #[test]
    fn test_temporal_burst_detection() {
        let mut analyzer = TemporalPatternAnalyzer::new(20);

        // Record rapid intervals
        for _ in 0..10 {
            analyzer.record_interval(50); // 50ms intervals = 20/sec
        }

        assert!(analyzer.is_burst());
    }

    #[test]
    fn test_spatial_locality_analyzer() {
        let mut analyzer = SpatialLocalityAnalyzer::new(10);

        // Record a path moving right
        analyzer.record_access(0, 0, 0);
        analyzer.record_access(1, 0, 0);
        analyzer.record_access(2, 0, 0);

        let predictions = analyzer.predict_adjacent(4);
        assert!(!predictions.is_empty());

        // First prediction should be in the direction of movement (right)
        assert!(predictions.iter().any(|&(x, _, _)| x == 3));
    }

    #[test]
    fn test_pattern_analyzer_sequential() {
        let mut analyzer = PatternAnalyzer::new(10);

        analyzer.record_access(AccessRecord::new("file_0".to_string()));
        analyzer.record_access(AccessRecord::new("file_1".to_string()));
        analyzer.record_access(AccessRecord::new("file_2".to_string()));
        analyzer.record_access(AccessRecord::new("file_3".to_string()));

        assert!(matches!(
            analyzer.current_pattern(),
            AccessPattern::SequentialForward | AccessPattern::SequentialBackward
        ));

        let predictions = analyzer.predict_next(3);
        assert!(!predictions.is_empty());
    }

    #[test]
    fn test_pattern_analyzer_spatial() {
        let mut analyzer = PatternAnalyzer::new(10);

        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_0_0_0".to_string(),
            0,
            0,
            0,
        ));
        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_1_0_0".to_string(),
            1,
            0,
            0,
        ));
        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_2_0_0".to_string(),
            2,
            0,
            0,
        ));
        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_3_0_0".to_string(),
            3,
            0,
            0,
        ));

        assert_eq!(analyzer.current_pattern(), AccessPattern::Spatial);

        let predictions = analyzer.predict_next(4);
        assert!(!predictions.is_empty());
    }

    #[test]
    fn test_prefetch_target_priority_ordering() {
        let targets = [
            PrefetchTarget::new("low".to_string(), PrefetchPriority::Low, 0.5),
            PrefetchTarget::new("critical".to_string(), PrefetchPriority::Critical, 0.9),
            PrefetchTarget::new("medium".to_string(), PrefetchPriority::Medium, 0.7),
        ];

        let mut sorted: Vec<_> = targets.iter().map(|t| t.priority).collect();
        sorted.sort();

        assert_eq!(sorted[0], PrefetchPriority::Low);
        assert_eq!(sorted[1], PrefetchPriority::Medium);
        assert_eq!(sorted[2], PrefetchPriority::Critical);
    }

    #[test]
    fn test_memory_aware_prefetcher() {
        let prefetcher = MemoryAwarePrefetcher::new(1000);

        assert!(prefetcher.can_prefetch(500));
        assert!(prefetcher.allocate(500));
        assert_eq!(prefetcher.current_usage(), 500);

        assert!(prefetcher.can_prefetch(400));
        assert!(!prefetcher.can_prefetch(600)); // Would exceed limit

        prefetcher.release(200);
        assert_eq!(prefetcher.current_usage(), 300);
    }

    #[test]
    fn test_buffer_stats() {
        let stats = BufferStats::default();

        stats.record_prefetch(true, 100);
        stats.record_prefetch(true, 200);
        stats.record_prefetch(false, 150);

        assert_eq!(stats.total_prefetches.load(Ordering::Relaxed), 3);
        assert_eq!(stats.successful_prefetches.load(Ordering::Relaxed), 2);
        assert_eq!(stats.wasted_prefetches.load(Ordering::Relaxed), 1);

        let hit_rate = stats.hit_rate();
        assert!((hit_rate - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_prefetch_config_builder() {
        let config = PrefetchConfig::new()
            .with_enabled(true)
            .with_prefetch_count(10)
            .with_max_concurrent(8)
            .with_bandwidth_limit(1_000_000)
            .with_memory_limit(128 * 1024 * 1024)
            .with_adaptive_sizing(true);

        assert!(config.enabled);
        assert_eq!(config.prefetch_count, 10);
        assert_eq!(config.max_concurrent, 8);
        assert_eq!(config.bandwidth_limit, Some(1_000_000));
        assert_eq!(config.memory_limit, 128 * 1024 * 1024);
        assert!(config.adaptive_sizing);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_prefetch_queue() {
        let queue = PrefetchQueue::new();

        queue
            .enqueue(PrefetchTarget::new(
                "low".to_string(),
                PrefetchPriority::Low,
                0.5,
            ))
            .await;
        queue
            .enqueue(PrefetchTarget::new(
                "high".to_string(),
                PrefetchPriority::High,
                0.9,
            ))
            .await;
        queue
            .enqueue(PrefetchTarget::new(
                "medium".to_string(),
                PrefetchPriority::Medium,
                0.7,
            ))
            .await;

        assert_eq!(queue.len().await, 3);

        // Should dequeue highest priority first
        let first = queue.dequeue().await;
        assert!(first.is_some());
        assert_eq!(first.map(|t| t.key), Some("high".to_string()));

        let second = queue.dequeue().await;
        assert!(second.is_some());
        assert_eq!(second.map(|t| t.key), Some("medium".to_string()));
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_prefetch_queue_deduplication() {
        let queue = PrefetchQueue::new();

        queue
            .enqueue(PrefetchTarget::new(
                "key1".to_string(),
                PrefetchPriority::High,
                0.9,
            ))
            .await;
        queue
            .enqueue(PrefetchTarget::new(
                "key1".to_string(),
                PrefetchPriority::Critical,
                0.95,
            ))
            .await;

        // Should only have one entry
        assert_eq!(queue.len().await, 1);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_prefetch_manager() {
        let config = PrefetchConfig::new()
            .with_prefetch_count(3)
            .with_memory_limit(1024 * 1024);
        let manager = PrefetchManager::new(config);

        // Record sequential accesses
        manager
            .record_access(AccessRecord::new("file_0".to_string()))
            .await;
        manager
            .record_access(AccessRecord::new("file_1".to_string()))
            .await;
        manager
            .record_access(AccessRecord::new("file_2".to_string()))
            .await;

        let predictions = manager
            .record_access(AccessRecord::new("file_3".to_string()))
            .await;

        // Should predict sequential pattern
        assert!(!predictions.is_empty());
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_prefetch_manager_memory_pressure() {
        let config = PrefetchConfig::new().with_memory_limit(1000);
        let manager = PrefetchManager::new(config);

        // Allocate most of the memory
        assert!(manager.allocate_memory(800));
        assert!(manager.memory_pressure() > 0.7);

        // Should not allow more prefetching
        assert!(!manager.can_prefetch(300).await);

        // Release memory
        manager.release_memory(500);
        assert!(manager.can_prefetch(300).await);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_bandwidth_aware_prefetcher() {
        let prefetcher = BandwidthAwarePrefetcher::new(1000);

        assert!(prefetcher.can_prefetch(500).await);
        prefetcher.record_usage(500).await;

        assert!(prefetcher.can_prefetch(400).await);
        prefetcher.record_usage(400).await;

        // Should be near limit
        assert!(!prefetcher.can_prefetch(200).await);
        assert!(prefetcher.remaining_bandwidth() < 200);
    }
}
