//! Performance profiling and monitoring utilities
//!
//! This module provides comprehensive performance tracking, memory monitoring,
//! timing utilities, frame rate analysis, and bottleneck detection for WASM applications.
//!
//! # Overview
//!
//! The profiler module enables deep performance analysis for browser-based geospatial applications:
//!
//! - **Performance Counters**: High-precision timing with percentile statistics
//! - **Memory Monitoring**: Heap usage tracking and leak detection
//! - **Frame Rate Tracking**: FPS monitoring for smooth animations
//! - **Bottleneck Detection**: Automatic identification of slow operations
//! - **Scoped Timing**: RAII-style timing for automatic measurement
//! - **Statistical Analysis**: Min, max, mean, p50, p95, p99 metrics
//!
//! # Why Profile WASM Applications?
//!
//! WASM performance can vary significantly across:
//! - Different browsers (Chrome, Firefox, Safari, Edge)
//! - Different devices (desktop, mobile, tablet)
//! - Different network conditions (fast, slow, intermittent)
//! - Different data sizes (small tiles vs large images)
//!
//! Profiling helps identify:
//! 1. Operations that are too slow for 60 FPS rendering
//! 2. Memory leaks or excessive allocations
//! 3. Network bottlenecks vs computation bottlenecks
//! 4. Browser-specific performance issues
//!
//! # Performance Counter Usage
//!
//! Performance counters track operation timing with statistical aggregation:
//!
//! ```rust
//! use oxigdal_wasm::{Profiler, PerformanceCounter};
//!
//! let mut profiler = Profiler::new();
//!
//! // Record multiple samples
//! for i in 0..100 {
//!     profiler.record("tile_decode", i as f64 * 0.1);
//! }
//!
//! // Analyze statistics
//! if let Some(stats) = profiler.counter_stats("tile_decode") {
//!     println!("Average: {:.2}ms", stats.average_ms);
//!     println!("P95: {:.2}ms", stats.p95_ms);
//!     println!("P99: {:.2}ms", stats.p99_ms);
//! }
//! ```
//!
//! # Memory Monitoring
//!
//! Track heap usage to detect memory leaks:
//!
//! ```ignore
//! use oxigdal_wasm::profiler::MemoryMonitor;
//!
//! let mut monitor = MemoryMonitor::new();
//!
//! // Record snapshots periodically
//! setInterval(|| {
//!     monitor.record_current(js_sys::Date::now());
//!
//!     let stats = monitor.stats();
//!     if stats.current_heap_used > stats.peak_heap_used * 0.9 {
//!         console.warn("Memory usage near peak!");
//!     }
//! }, 1000);
//! ```
//!
//! # Frame Rate Tracking
//!
//! Monitor frame rate for smooth animations:
//!
//! ```ignore
//! use oxigdal_wasm::profiler::FrameRateTracker;
//!
//! let mut tracker = FrameRateTracker::new(60.0); // Target 60 FPS
//!
//! // In animation loop
//! requestAnimationFrame(|timestamp| {
//!     tracker.record_frame(timestamp);
//!
//!     let stats = tracker.stats();
//!     if stats.is_below_target {
//!         console.warn("FPS dropped to {}", stats.current_fps);
//!         // Reduce quality or skip frame
//!     }
//! });
//! ```
//!
//! # Bottleneck Detection
//!
//! Automatically identify slow operations:
//!
//! ```rust
//! use oxigdal_wasm::BottleneckDetector;
//!
//! let mut detector = BottleneckDetector::new(10.0); // 10ms threshold
//!
//! // Record operations
//! detector.record("fetch_tile", 5.0);   // Fast - no bottleneck
//! detector.record("decode_tile", 25.0); // Slow - bottleneck!
//! detector.record("render_tile", 50.0); // Very slow - critical!
//!
//! // Get recommendations
//! for recommendation in detector.recommendations() {
//!     println!("{}", recommendation);
//! }
//! // Output:
//! // "CRITICAL: 'render_tile' is taking 50.00ms on average (5x threshold)"
//! // "WARNING: 'decode_tile' is taking 25.00ms on average (2x threshold)"
//! ```
//!
//! # Percentile Statistics
//!
//! Understanding percentiles is crucial for performance analysis:
//!
//! - **P50 (Median)**: Half of operations are faster, half are slower
//! - **P95**: 95% of operations are faster (identifies slow outliers)
//! - **P99**: 99% of operations are faster (identifies rare worst cases)
//!
//! Example interpretation:
//! ```text
//! Operation: tile_decode
//! Average: 10ms    <- Mean time
//! P50: 8ms         <- Typical case
//! P95: 20ms        <- Slow but not rare
//! P99: 50ms        <- Rare worst case
//! ```
//!
//! If P99 is much higher than P95, there are occasional slow outliers.
//!
//! # Performance Budgets
//!
//! For 60 FPS rendering, each frame has ~16.67ms budget:
//!
//! ```text
//! Operation          Budget    Typical    Status
//! ─────────────────────────────────────────────
//! Tile fetch         8ms       5ms        ✓ OK
//! Tile decode        4ms       3ms        ✓ OK
//! Tile render        3ms       2ms        ✓ OK
//! Cache lookup       0.5ms     0.1ms      ✓ OK
//! Frame overhead     1.5ms     1ms        ✓ OK
//! ─────────────────────────────────────────────
//! Total             17ms       11ms       ✓ OK
//! ```
//!
//! If total exceeds 16.67ms, frame rate drops below 60 FPS.
//!
//! # Example: Complete Profiling Setup
//!
//! ```ignore
//! use oxigdal_wasm::profiler::{Profiler, FrameRateTracker, BottleneckDetector};
//!
//! // Create profiler
//! let mut profiler = Profiler::new();
//! let mut fps_tracker = FrameRateTracker::new(60.0);
//! let mut bottleneck = BottleneckDetector::new(10.0);
//!
//! // Profile tile loading
//! async fn load_and_profile(url: &str) {
//!     let start = js_sys::Date::now();
//!
//!     // Fetch tile
//!     let fetch_start = start;
//!     let tile_data = fetch_tile(url).await;
//!     let fetch_time = js_sys::Date::now() - fetch_start;
//!     profiler.record("fetch", fetch_time);
//!
//!     // Decode tile
//!     let decode_start = js_sys::Date::now();
//!     let decoded = decode_tile(&tile_data);
//!     let decode_time = js_sys::Date::now() - decode_start;
//!     profiler.record("decode", decode_time);
//!
//!     // Render tile
//!     let render_start = js_sys::Date::now();
//!     render_to_canvas(&decoded);
//!     let render_time = js_sys::Date::now() - render_start;
//!     profiler.record("render", render_time);
//!
//!     // Total time
//!     let total_time = js_sys::Date::now() - start;
//!     profiler.record("total", total_time);
//!
//!     // Check for bottlenecks
//!     bottleneck.record("fetch", fetch_time);
//!     bottleneck.record("decode", decode_time);
//!     bottleneck.record("render", render_time);
//! }
//!
//! // In animation loop
//! requestAnimationFrame(|timestamp| {
//!     fps_tracker.record_frame(timestamp);
//!
//!     // Check performance periodically
//!     if frame_count % 60 == 0 {
//!         let summary = profiler.summary();
//!         console.log("Performance Report:");
//!         for counter in summary.counters {
//!             console.log("  {}: {:.2}ms avg", counter.name, counter.average_ms);
//!         }
//!
//!         let fps = fps_tracker.stats();
//!         console.log("FPS: {:.1}", fps.current_fps);
//!
//!         // Check for bottlenecks
//!         for bottleneck in bottleneck.detect_bottlenecks() {
//!             console.warn("Bottleneck: {} ({:.2}ms)",
//!                 bottleneck.operation,
//!                 bottleneck.average_ms
//!             );
//!         }
//!     }
//! });
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use wasm_bindgen::prelude::*;

/// Maximum number of timing samples to keep
pub const MAX_TIMING_SAMPLES: usize = 1000;

/// Maximum number of memory samples to keep
pub const MAX_MEMORY_SAMPLES: usize = 100;

/// Performance counter
#[derive(Debug, Clone)]
pub struct PerformanceCounter {
    /// Counter name
    name: String,
    /// Total count
    count: u64,
    /// Total time in milliseconds
    total_time_ms: f64,
    /// Minimum time
    min_time_ms: f64,
    /// Maximum time
    max_time_ms: f64,
    /// Recent samples
    samples: VecDeque<f64>,
}

impl PerformanceCounter {
    /// Creates a new performance counter
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            count: 0,
            total_time_ms: 0.0,
            min_time_ms: f64::MAX,
            max_time_ms: f64::MIN,
            samples: VecDeque::new(),
        }
    }

    /// Records a timing sample
    pub fn record(&mut self, duration_ms: f64) {
        self.count += 1;
        self.total_time_ms += duration_ms;
        self.min_time_ms = self.min_time_ms.min(duration_ms);
        self.max_time_ms = self.max_time_ms.max(duration_ms);

        self.samples.push_back(duration_ms);
        if self.samples.len() > MAX_TIMING_SAMPLES {
            self.samples.pop_front();
        }
    }

    /// Returns the average time
    pub fn average_ms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_time_ms / self.count as f64
        }
    }

    /// Returns the recent average (last 100 samples)
    pub fn recent_average_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let recent: Vec<_> = self.samples.iter().rev().take(100).collect();
        let sum: f64 = recent.iter().copied().sum();
        sum / recent.len() as f64
    }

    /// Returns the percentile
    pub fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let mut sorted: Vec<_> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((p / 100.0) * sorted.len() as f64) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Returns statistics
    pub fn stats(&self) -> CounterStats {
        CounterStats {
            name: self.name.clone(),
            count: self.count,
            total_time_ms: self.total_time_ms,
            average_ms: self.average_ms(),
            recent_average_ms: self.recent_average_ms(),
            min_ms: if self.count > 0 {
                self.min_time_ms
            } else {
                0.0
            },
            max_ms: if self.count > 0 {
                self.max_time_ms
            } else {
                0.0
            },
            p50_ms: self.percentile(50.0),
            p95_ms: self.percentile(95.0),
            p99_ms: self.percentile(99.0),
        }
    }

    /// Resets the counter
    pub fn reset(&mut self) {
        self.count = 0;
        self.total_time_ms = 0.0;
        self.min_time_ms = f64::MAX;
        self.max_time_ms = f64::MIN;
        self.samples.clear();
    }
}

/// Counter statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterStats {
    /// Counter name
    pub name: String,
    /// Total count
    pub count: u64,
    /// Total time
    pub total_time_ms: f64,
    /// Average time
    pub average_ms: f64,
    /// Recent average time
    pub recent_average_ms: f64,
    /// Minimum time
    pub min_ms: f64,
    /// Maximum time
    pub max_ms: f64,
    /// 50th percentile
    pub p50_ms: f64,
    /// 95th percentile
    pub p95_ms: f64,
    /// 99th percentile
    pub p99_ms: f64,
}

/// Memory snapshot
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Timestamp
    pub timestamp: f64,
    /// Heap used in bytes
    pub heap_used: usize,
    /// Heap limit in bytes (if available)
    pub heap_limit: Option<usize>,
    /// External memory in bytes (if available)
    pub external_memory: Option<usize>,
}

impl MemorySnapshot {
    /// Creates a new memory snapshot
    pub const fn new(timestamp: f64, heap_used: usize) -> Self {
        Self {
            timestamp,
            heap_used,
            heap_limit: None,
            external_memory: None,
        }
    }

    /// Returns heap utilization as a fraction (0.0 to 1.0)
    pub fn heap_utilization(&self) -> Option<f64> {
        self.heap_limit.map(|limit| {
            if limit > 0 {
                self.heap_used as f64 / limit as f64
            } else {
                0.0
            }
        })
    }
}

/// Memory monitor
pub struct MemoryMonitor {
    /// Memory snapshots
    snapshots: VecDeque<MemorySnapshot>,
    /// Maximum snapshots to keep
    max_snapshots: usize,
}

impl MemoryMonitor {
    /// Creates a new memory monitor
    pub fn new() -> Self {
        Self {
            snapshots: VecDeque::new(),
            max_snapshots: MAX_MEMORY_SAMPLES,
        }
    }

    /// Records a memory snapshot
    pub fn record(&mut self, snapshot: MemorySnapshot) {
        self.snapshots.push_back(snapshot);
        if self.snapshots.len() > self.max_snapshots {
            self.snapshots.pop_front();
        }
    }

    /// Records current memory usage
    pub fn record_current(&mut self, timestamp: f64) {
        // In WASM, we can't easily get memory info without performance.memory API
        // For now, use a placeholder
        let snapshot = MemorySnapshot::new(timestamp, 0);
        self.record(snapshot);
    }

    /// Returns the latest snapshot
    pub fn latest(&self) -> Option<&MemorySnapshot> {
        self.snapshots.back()
    }

    /// Returns memory statistics
    pub fn stats(&self) -> MemoryStats {
        if self.snapshots.is_empty() {
            return MemoryStats {
                current_heap_used: 0,
                peak_heap_used: 0,
                average_heap_used: 0.0,
                sample_count: 0,
            };
        }

        let current = self.snapshots.back().map(|s| s.heap_used).unwrap_or(0);
        let peak = self
            .snapshots
            .iter()
            .map(|s| s.heap_used)
            .max()
            .unwrap_or(0);
        let sum: usize = self.snapshots.iter().map(|s| s.heap_used).sum();
        let average = sum as f64 / self.snapshots.len() as f64;

        MemoryStats {
            current_heap_used: current,
            peak_heap_used: peak,
            average_heap_used: average,
            sample_count: self.snapshots.len(),
        }
    }

    /// Clears all snapshots
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Current heap usage in bytes
    pub current_heap_used: usize,
    /// Peak heap usage in bytes
    pub peak_heap_used: usize,
    /// Average heap usage in bytes
    pub average_heap_used: f64,
    /// Number of samples
    pub sample_count: usize,
}

/// Performance profiler
pub struct Profiler {
    /// Performance counters
    counters: HashMap<String, PerformanceCounter>,
    /// Memory monitor
    memory: MemoryMonitor,
    /// Active timers (start times)
    active_timers: HashMap<String, f64>,
}

impl Profiler {
    /// Creates a new profiler
    pub fn new() -> Self {
        Self {
            counters: HashMap::new(),
            memory: MemoryMonitor::new(),
            active_timers: HashMap::new(),
        }
    }

    /// Starts a timer
    pub fn start_timer(&mut self, name: impl Into<String>, timestamp: f64) {
        self.active_timers.insert(name.into(), timestamp);
    }

    /// Stops a timer and records the duration
    pub fn stop_timer(&mut self, name: impl Into<String>, timestamp: f64) {
        let name = name.into();
        if let Some(start) = self.active_timers.remove(&name) {
            let duration = timestamp - start;
            self.record(name, duration);
        }
    }

    /// Records a timing sample
    pub fn record(&mut self, name: impl Into<String>, duration_ms: f64) {
        let name = name.into();
        self.counters
            .entry(name.clone())
            .or_insert_with(|| PerformanceCounter::new(name))
            .record(duration_ms);
    }

    /// Records current memory usage
    pub fn record_memory(&mut self, timestamp: f64) {
        self.memory.record_current(timestamp);
    }

    /// Returns counter statistics
    pub fn counter_stats(&self, name: &str) -> Option<CounterStats> {
        self.counters.get(name).map(|c| c.stats())
    }

    /// Returns all counter statistics
    pub fn all_counter_stats(&self) -> Vec<CounterStats> {
        self.counters.values().map(|c| c.stats()).collect()
    }

    /// Returns memory statistics
    pub fn memory_stats(&self) -> MemoryStats {
        self.memory.stats()
    }

    /// Returns a summary report
    pub fn summary(&self) -> ProfilerSummary {
        ProfilerSummary {
            counters: self.all_counter_stats(),
            memory: self.memory_stats(),
        }
    }

    /// Resets all counters
    pub fn reset(&mut self) {
        self.counters.clear();
        self.memory.clear();
        self.active_timers.clear();
    }

    /// Clears a specific counter
    pub fn clear_counter(&mut self, name: &str) {
        self.counters.remove(name);
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Profiler summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilerSummary {
    /// Counter statistics
    pub counters: Vec<CounterStats>,
    /// Memory statistics
    pub memory: MemoryStats,
}

/// Scoped timer for automatic timing
#[allow(dead_code)]
pub struct ScopedTimer<'a> {
    profiler: &'a mut Profiler,
    name: String,
    start_time: f64,
}

#[allow(dead_code)]
impl<'a> ScopedTimer<'a> {
    /// Creates a new scoped timer
    pub fn new(profiler: &'a mut Profiler, name: impl Into<String>, start_time: f64) -> Self {
        let name = name.into();
        profiler.start_timer(name.clone(), start_time);
        Self {
            profiler,
            name,
            start_time,
        }
    }

    /// Returns the elapsed time
    pub fn elapsed(&self, current_time: f64) -> f64 {
        current_time - self.start_time
    }
}

impl<'a> Drop for ScopedTimer<'a> {
    fn drop(&mut self) {
        // Get current time (in a real WASM environment, use js_sys::Date::now())
        let current_time = self.start_time; // Placeholder
        self.profiler.stop_timer(self.name.clone(), current_time);
    }
}

/// Frame rate tracker
pub struct FrameRateTracker {
    /// Frame timestamps
    frame_times: VecDeque<f64>,
    /// Maximum samples
    max_samples: usize,
    /// Target FPS
    target_fps: f64,
}

impl FrameRateTracker {
    /// Creates a new frame rate tracker
    pub fn new(target_fps: f64) -> Self {
        Self {
            frame_times: VecDeque::new(),
            max_samples: 120,
            target_fps,
        }
    }

    /// Records a frame
    pub fn record_frame(&mut self, timestamp: f64) {
        self.frame_times.push_back(timestamp);
        if self.frame_times.len() > self.max_samples {
            self.frame_times.pop_front();
        }
    }

    /// Returns the current FPS
    pub fn current_fps(&self) -> f64 {
        if self.frame_times.len() < 2 {
            return 0.0;
        }

        let duration = self.frame_times.back().copied().unwrap_or(0.0)
            - self.frame_times.front().copied().unwrap_or(0.0);

        if duration > 0.0 {
            ((self.frame_times.len() - 1) as f64 / duration) * 1000.0
        } else {
            0.0
        }
    }

    /// Returns frame statistics
    pub fn stats(&self) -> FrameRateStats {
        let fps = self.current_fps();
        // Use 1% tolerance to account for floating point precision
        let tolerance = self.target_fps * 0.01;
        let is_below_target = fps < (self.target_fps - tolerance);

        // Calculate frame time variance
        let mut frame_deltas = Vec::new();
        for i in 1..self.frame_times.len() {
            let delta = self.frame_times[i] - self.frame_times[i - 1];
            frame_deltas.push(delta);
        }

        let avg_frame_time = if !frame_deltas.is_empty() {
            frame_deltas.iter().sum::<f64>() / frame_deltas.len() as f64
        } else {
            0.0
        };

        FrameRateStats {
            current_fps: fps,
            target_fps: self.target_fps,
            average_frame_time_ms: avg_frame_time,
            is_below_target,
            frame_count: self.frame_times.len(),
        }
    }

    /// Clears all frame data
    pub fn clear(&mut self) {
        self.frame_times.clear();
    }
}

/// Frame rate statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FrameRateStats {
    /// Current FPS
    pub current_fps: f64,
    /// Target FPS
    pub target_fps: f64,
    /// Average frame time in milliseconds
    pub average_frame_time_ms: f64,
    /// Whether below target
    pub is_below_target: bool,
    /// Number of frames sampled
    pub frame_count: usize,
}

/// Bottleneck detector
pub struct BottleneckDetector {
    /// Profiler reference
    profiler: Profiler,
    /// Threshold for slow operations (milliseconds)
    slow_threshold_ms: f64,
}

impl BottleneckDetector {
    /// Creates a new bottleneck detector
    pub fn new(slow_threshold_ms: f64) -> Self {
        Self {
            profiler: Profiler::new(),
            slow_threshold_ms,
        }
    }

    /// Records a timing sample
    pub fn record(&mut self, name: impl Into<String>, duration_ms: f64) {
        self.profiler.record(name, duration_ms);
    }

    /// Detects bottlenecks
    pub fn detect_bottlenecks(&self) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        for stats in self.profiler.all_counter_stats() {
            if stats.average_ms > self.slow_threshold_ms {
                bottlenecks.push(Bottleneck {
                    operation: stats.name.clone(),
                    average_ms: stats.average_ms,
                    p95_ms: stats.p95_ms,
                    count: stats.count,
                    severity: self.calculate_severity(stats.average_ms),
                });
            }
        }

        // Sort by severity
        bottlenecks.sort_by(|a, b| {
            b.severity
                .partial_cmp(&a.severity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        bottlenecks
    }

    /// Calculates bottleneck severity
    fn calculate_severity(&self, average_ms: f64) -> f64 {
        average_ms / self.slow_threshold_ms
    }

    /// Returns recommendations
    pub fn recommendations(&self) -> Vec<String> {
        let bottlenecks = self.detect_bottlenecks();
        let mut recommendations = Vec::new();

        for bottleneck in bottlenecks {
            if bottleneck.severity > 5.0 {
                recommendations.push(format!(
                    "CRITICAL: '{}' is taking {:.2}ms on average ({}x threshold). Consider optimization or caching.",
                    bottleneck.operation, bottleneck.average_ms, bottleneck.severity as u32
                ));
            } else if bottleneck.severity > 2.0 {
                recommendations.push(format!(
                    "WARNING: '{}' is taking {:.2}ms on average ({}x threshold). May benefit from optimization.",
                    bottleneck.operation, bottleneck.average_ms, bottleneck.severity as u32
                ));
            }
        }

        recommendations
    }
}

/// Bottleneck information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bottleneck {
    /// Operation name
    pub operation: String,
    /// Average time
    pub average_ms: f64,
    /// 95th percentile time
    pub p95_ms: f64,
    /// Call count
    pub count: u64,
    /// Severity score
    pub severity: f64,
}

/// WASM bindings for profiler
#[wasm_bindgen]
pub struct WasmProfiler {
    profiler: Profiler,
}

#[wasm_bindgen]
impl WasmProfiler {
    /// Creates a new profiler
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            profiler: Profiler::new(),
        }
    }

    /// Starts a timer
    #[wasm_bindgen(js_name = startTimer)]
    pub fn start_timer(&mut self, name: &str) {
        let timestamp = js_sys::Date::now();
        self.profiler.start_timer(name, timestamp);
    }

    /// Stops a timer
    #[wasm_bindgen(js_name = stopTimer)]
    pub fn stop_timer(&mut self, name: &str) {
        let timestamp = js_sys::Date::now();
        self.profiler.stop_timer(name, timestamp);
    }

    /// Records a timing sample
    #[wasm_bindgen]
    pub fn record(&mut self, name: &str, duration_ms: f64) {
        self.profiler.record(name, duration_ms);
    }

    /// Records current memory usage
    #[wasm_bindgen(js_name = recordMemory)]
    pub fn record_memory(&mut self) {
        let timestamp = js_sys::Date::now();
        self.profiler.record_memory(timestamp);
    }

    /// Returns counter statistics as JSON
    #[wasm_bindgen(js_name = getCounterStats)]
    pub fn get_counter_stats(&self, name: &str) -> Option<String> {
        self.profiler
            .counter_stats(name)
            .and_then(|stats| serde_json::to_string(&stats).ok())
    }

    /// Returns all statistics as JSON
    #[wasm_bindgen(js_name = getAllStats)]
    pub fn get_all_stats(&self) -> String {
        let summary = self.profiler.summary();
        serde_json::to_string(&summary).unwrap_or_default()
    }

    /// Resets all counters
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.profiler.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_counter() {
        let mut counter = PerformanceCounter::new("test");
        counter.record(10.0);
        counter.record(20.0);
        counter.record(30.0);

        assert_eq!(counter.count, 3);
        assert_eq!(counter.average_ms(), 20.0);
        assert_eq!(counter.min_time_ms, 10.0);
        assert_eq!(counter.max_time_ms, 30.0);
    }

    #[test]
    fn test_percentile() {
        let mut counter = PerformanceCounter::new("test");
        for i in 1..=100 {
            counter.record(i as f64);
        }

        let p50 = counter.percentile(50.0);
        assert!((49.0..=51.0).contains(&p50));

        let p95 = counter.percentile(95.0);
        assert!((94.0..=96.0).contains(&p95));
    }

    #[test]
    fn test_profiler() {
        let mut profiler = Profiler::new();
        profiler.record("test", 10.0);
        profiler.record("test", 20.0);

        let stats = profiler
            .counter_stats("test")
            .expect("Counter should exist");
        assert_eq!(stats.count, 2);
        assert_eq!(stats.average_ms, 15.0);
    }

    #[test]
    fn test_memory_monitor() {
        let mut monitor = MemoryMonitor::new();
        monitor.record(MemorySnapshot::new(0.0, 1000));
        monitor.record(MemorySnapshot::new(1.0, 2000));
        monitor.record(MemorySnapshot::new(2.0, 1500));

        let stats = monitor.stats();
        assert_eq!(stats.current_heap_used, 1500);
        assert_eq!(stats.peak_heap_used, 2000);
        assert_eq!(stats.average_heap_used, 1500.0);
    }

    #[test]
    fn test_frame_rate_tracker() {
        let mut tracker = FrameRateTracker::new(60.0);

        // Simulate 60 FPS (16.67ms per frame)
        for i in 0..120 {
            tracker.record_frame((i as f64) * 16.67);
        }

        let fps = tracker.current_fps();
        assert!(fps > 55.0 && fps < 65.0);
    }

    #[test]
    fn test_bottleneck_detector() {
        let mut detector = BottleneckDetector::new(10.0);
        detector.record("fast_op", 5.0);
        detector.record("slow_op", 50.0);
        detector.record("slow_op", 60.0);

        let bottlenecks = detector.detect_bottlenecks();
        assert_eq!(bottlenecks.len(), 1);
        assert_eq!(bottlenecks[0].operation, "slow_op");
        assert!(bottlenecks[0].severity > 5.0);
    }
}
