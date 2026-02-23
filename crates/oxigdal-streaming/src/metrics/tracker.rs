//! Performance tracking for streaming operations.

use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Type alias for throughput interval samples (timestamp, element_count, byte_count).
type IntervalSamples = VecDeque<(DateTime<Utc>, u64, u64)>;

/// Performance tracker for streaming operations.
pub struct PerformanceTracker {
    start_time: Instant,
    checkpoints: Arc<RwLock<Vec<(String, Instant)>>>,
    enabled: Arc<RwLock<bool>>,
}

impl PerformanceTracker {
    /// Create a new performance tracker.
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            checkpoints: Arc::new(RwLock::new(Vec::new())),
            enabled: Arc::new(RwLock::new(true)),
        }
    }

    /// Enable tracking.
    pub async fn enable(&self) {
        *self.enabled.write().await = true;
    }

    /// Disable tracking.
    pub async fn disable(&self) {
        *self.enabled.write().await = false;
    }

    /// Record a checkpoint.
    pub async fn checkpoint(&self, name: String) -> Result<()> {
        if !*self.enabled.read().await {
            return Ok(());
        }

        let mut checkpoints = self.checkpoints.write().await;
        checkpoints.push((name, Instant::now()));

        Ok(())
    }

    /// Get elapsed time since start.
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Get all checkpoints.
    pub async fn get_checkpoints(&self) -> Vec<(String, std::time::Duration)> {
        let checkpoints = self.checkpoints.read().await;
        let start = self.start_time;

        checkpoints
            .iter()
            .map(|(name, instant)| {
                let duration = instant.duration_since(start);
                (name.clone(), duration)
            })
            .collect()
    }

    /// Clear all checkpoints.
    pub async fn clear(&self) {
        self.checkpoints.write().await.clear();
    }

    /// Reset the tracker.
    pub async fn reset(&self) {
        self.clear().await;
    }
}

impl Default for PerformanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Latency tracker with histogram support.
pub struct LatencyTracker {
    samples: Arc<RwLock<VecDeque<std::time::Duration>>>,
    max_samples: usize,
    buckets: Vec<std::time::Duration>,
    histogram: Arc<RwLock<Vec<u64>>>,
}

impl LatencyTracker {
    /// Create a new latency tracker.
    pub fn new(max_samples: usize) -> Self {
        let buckets = vec![
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(500),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(5),
        ];

        let histogram = vec![0; buckets.len()];

        Self {
            samples: Arc::new(RwLock::new(VecDeque::with_capacity(max_samples))),
            max_samples,
            buckets,
            histogram: Arc::new(RwLock::new(histogram)),
        }
    }

    /// Record a latency sample.
    pub async fn record(&self, latency: std::time::Duration) {
        let mut samples = self.samples.write().await;

        if samples.len() >= self.max_samples {
            samples.pop_front();
        }

        samples.push_back(latency);

        let mut histogram = self.histogram.write().await;
        for (i, &bucket) in self.buckets.iter().enumerate() {
            if latency <= bucket {
                histogram[i] += 1;
            }
        }
    }

    /// Get the average latency.
    pub async fn average(&self) -> Option<std::time::Duration> {
        let samples = self.samples.read().await;

        if samples.is_empty() {
            return None;
        }

        let sum: std::time::Duration = samples.iter().sum();
        Some(sum / samples.len() as u32)
    }

    /// Get the minimum latency.
    pub async fn min(&self) -> Option<std::time::Duration> {
        let samples = self.samples.read().await;
        samples.iter().min().copied()
    }

    /// Get the maximum latency.
    pub async fn max(&self) -> Option<std::time::Duration> {
        let samples = self.samples.read().await;
        samples.iter().max().copied()
    }

    /// Get the median latency.
    pub async fn median(&self) -> Option<std::time::Duration> {
        let samples = self.samples.read().await;

        if samples.is_empty() {
            return None;
        }

        let mut sorted: Vec<_> = samples.iter().copied().collect();
        sorted.sort();

        Some(sorted[sorted.len() / 2])
    }

    /// Get the 95th percentile latency.
    pub async fn p95(&self) -> Option<std::time::Duration> {
        self.percentile(0.95).await
    }

    /// Get the 99th percentile latency.
    pub async fn p99(&self) -> Option<std::time::Duration> {
        self.percentile(0.99).await
    }

    /// Get a specific percentile.
    pub async fn percentile(&self, p: f64) -> Option<std::time::Duration> {
        let samples = self.samples.read().await;

        if samples.is_empty() {
            return None;
        }

        let mut sorted: Vec<_> = samples.iter().copied().collect();
        sorted.sort();

        let index = ((sorted.len() as f64 * p) as usize).min(sorted.len() - 1);
        Some(sorted[index])
    }

    /// Get the histogram.
    pub async fn histogram(&self) -> Vec<(std::time::Duration, u64)> {
        let histogram = self.histogram.read().await;

        self.buckets
            .iter()
            .zip(histogram.iter())
            .map(|(&bucket, &count)| (bucket, count))
            .collect()
    }

    /// Clear all samples.
    pub async fn clear(&self) {
        self.samples.write().await.clear();
        *self.histogram.write().await = vec![0; self.buckets.len()];
    }
}

/// Throughput tracker.
pub struct ThroughputTracker {
    start_time: DateTime<Utc>,
    element_count: Arc<RwLock<u64>>,
    byte_count: Arc<RwLock<u64>>,
    interval_samples: Arc<RwLock<IntervalSamples>>,
    interval_duration: Duration,
    max_intervals: usize,
}

impl ThroughputTracker {
    /// Create a new throughput tracker.
    pub fn new(interval_duration: Duration, max_intervals: usize) -> Self {
        Self {
            start_time: Utc::now(),
            element_count: Arc::new(RwLock::new(0)),
            byte_count: Arc::new(RwLock::new(0)),
            interval_samples: Arc::new(RwLock::new(VecDeque::with_capacity(max_intervals))),
            interval_duration,
            max_intervals,
        }
    }

    /// Record elements processed.
    pub async fn record_elements(&self, count: u64) {
        *self.element_count.write().await += count;
    }

    /// Record bytes processed.
    pub async fn record_bytes(&self, bytes: u64) {
        *self.byte_count.write().await += bytes;
    }

    /// Record both elements and bytes.
    pub async fn record(&self, elements: u64, bytes: u64) {
        self.record_elements(elements).await;
        self.record_bytes(bytes).await;
    }

    /// Take an interval snapshot.
    pub async fn snapshot(&self) {
        let now = Utc::now();
        let elements = *self.element_count.read().await;
        let bytes = *self.byte_count.read().await;

        let mut samples = self.interval_samples.write().await;

        if samples.len() >= self.max_intervals {
            samples.pop_front();
        }

        samples.push_back((now, elements, bytes));
    }

    /// Get the overall throughput (elements per second).
    pub async fn elements_per_second(&self) -> f64 {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds() as f64 / 1000.0;
        let count = *self.element_count.read().await as f64;

        if elapsed > 0.0 { count / elapsed } else { 0.0 }
    }

    /// Get the overall throughput (bytes per second).
    pub async fn bytes_per_second(&self) -> f64 {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds() as f64 / 1000.0;
        let bytes = *self.byte_count.read().await as f64;

        if elapsed > 0.0 { bytes / elapsed } else { 0.0 }
    }

    /// Get the average throughput over recent intervals.
    pub async fn average_elements_per_second(&self) -> f64 {
        let samples = self.interval_samples.read().await;

        if samples.len() < 2 {
            return 0.0;
        }

        let first = &samples[0];
        let last = &samples[samples.len() - 1];

        let elapsed = (last.0 - first.0).num_milliseconds() as f64 / 1000.0;
        let elements = (last.1 - first.1) as f64;

        if elapsed > 0.0 {
            elements / elapsed
        } else {
            0.0
        }
    }

    /// Get the configured interval duration.
    pub fn interval_duration(&self) -> Duration {
        self.interval_duration
    }

    /// Get the peak throughput.
    pub async fn peak_elements_per_second(&self) -> f64 {
        let samples = self.interval_samples.read().await;

        if samples.len() < 2 {
            return 0.0;
        }

        let mut max_rate: f64 = 0.0;

        // Convert VecDeque to Vec to use windows()
        let samples_vec: Vec<_> = samples.iter().copied().collect();

        for window in samples_vec.windows(2) {
            let (t1, e1, _) = &window[0];
            let (t2, e2, _) = &window[1];

            let elapsed = (*t2 - *t1).num_milliseconds() as f64 / 1000.0;
            let elements = (e2 - e1) as f64;

            if elapsed > 0.0 {
                let rate = elements / elapsed;
                max_rate = max_rate.max(rate);
            }
        }

        max_rate
    }

    /// Clear all counters.
    pub async fn clear(&self) {
        *self.element_count.write().await = 0;
        *self.byte_count.write().await = 0;
        self.interval_samples.write().await.clear();
    }

    /// Reset the tracker.
    pub async fn reset(&self) {
        self.clear().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_performance_tracker() {
        let tracker = PerformanceTracker::new();

        tracker
            .checkpoint("start".to_string())
            .await
            .expect("Failed to record start checkpoint in test");
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        tracker
            .checkpoint("middle".to_string())
            .await
            .expect("Failed to record middle checkpoint in test");
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        tracker
            .checkpoint("end".to_string())
            .await
            .expect("Failed to record end checkpoint in test");

        let checkpoints = tracker.get_checkpoints().await;
        assert_eq!(checkpoints.len(), 3);
    }

    #[tokio::test]
    async fn test_latency_tracker() {
        let tracker = LatencyTracker::new(100);

        tracker.record(std::time::Duration::from_millis(10)).await;
        tracker.record(std::time::Duration::from_millis(20)).await;
        tracker.record(std::time::Duration::from_millis(30)).await;

        let avg = tracker
            .average()
            .await
            .expect("Failed to get average latency in test");
        assert!(avg >= std::time::Duration::from_millis(19));
        assert!(avg <= std::time::Duration::from_millis(21));

        let min = tracker
            .min()
            .await
            .expect("Failed to get minimum latency in test");
        assert_eq!(min, std::time::Duration::from_millis(10));

        let max = tracker
            .max()
            .await
            .expect("Failed to get maximum latency in test");
        assert_eq!(max, std::time::Duration::from_millis(30));
    }

    #[tokio::test]
    async fn test_throughput_tracker() {
        let tracker = ThroughputTracker::new(Duration::seconds(1), 10);

        tracker.record(100, 1000).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let eps = tracker.elements_per_second().await;
        assert!(eps > 0.0);

        let bps = tracker.bytes_per_second().await;
        assert!(bps > 0.0);
    }

    #[tokio::test]
    async fn test_latency_percentiles() {
        let tracker = LatencyTracker::new(100);

        for i in 1..=100 {
            tracker.record(std::time::Duration::from_millis(i)).await;
        }

        let p95 = tracker
            .p95()
            .await
            .expect("Failed to get 95th percentile latency in test");
        assert!(p95 >= std::time::Duration::from_millis(94));
        assert!(p95 <= std::time::Duration::from_millis(96));

        let p99 = tracker
            .p99()
            .await
            .expect("Failed to get 99th percentile latency in test");
        assert!(p99 >= std::time::Duration::from_millis(98));
    }
}
