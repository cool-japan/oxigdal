//! Producer metrics

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Producer metrics
#[derive(Debug, Default, Clone)]
pub struct ProducerMetrics {
    /// Total number of messages sent
    pub messages_sent: u64,
    /// Total number of messages failed
    pub messages_failed: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total send time in microseconds
    pub total_send_time_us: u64,
    /// Number of batches sent
    pub batches_sent: u64,
    /// Number of retries
    pub retries: u64,
}

impl ProducerMetrics {
    /// Create new producer metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a message sent
    pub fn record_message_sent(&mut self, bytes: usize, duration: Duration) {
        self.messages_sent += 1;
        self.bytes_sent += bytes as u64;
        self.total_send_time_us += duration.as_micros() as u64;
    }

    /// Record a message failed
    pub fn record_message_failed(&mut self) {
        self.messages_failed += 1;
    }

    /// Record a batch sent
    pub fn record_batch_sent(&mut self, messages: usize, bytes: usize) {
        self.batches_sent += 1;
        self.messages_sent += messages as u64;
        self.bytes_sent += bytes as u64;
    }

    /// Record a retry
    pub fn record_retry(&mut self) {
        self.retries += 1;
    }

    /// Get average message size
    pub fn average_message_size(&self) -> f64 {
        if self.messages_sent == 0 {
            0.0
        } else {
            self.bytes_sent as f64 / self.messages_sent as f64
        }
    }

    /// Get average send time
    pub fn average_send_time(&self) -> Duration {
        if self.messages_sent == 0 {
            Duration::from_secs(0)
        } else {
            Duration::from_micros(self.total_send_time_us / self.messages_sent)
        }
    }

    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.messages_sent + self.messages_failed;
        if total == 0 {
            1.0
        } else {
            self.messages_sent as f64 / total as f64
        }
    }

    /// Get throughput in messages per second
    pub fn throughput_mps(&self, duration: Duration) -> f64 {
        let duration_secs = duration.as_secs_f64();
        if duration_secs == 0.0 {
            0.0
        } else {
            self.messages_sent as f64 / duration_secs
        }
    }

    /// Get throughput in bytes per second
    pub fn throughput_bps(&self, duration: Duration) -> f64 {
        let duration_secs = duration.as_secs_f64();
        if duration_secs == 0.0 {
            0.0
        } else {
            self.bytes_sent as f64 / duration_secs
        }
    }

    /// Reset metrics
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Thread-safe producer metrics
pub struct AtomicProducerMetrics {
    messages_sent: AtomicU64,
    messages_failed: AtomicU64,
    bytes_sent: AtomicU64,
    total_send_time_us: AtomicU64,
    batches_sent: AtomicU64,
    retries: AtomicU64,
}

impl AtomicProducerMetrics {
    /// Create new atomic producer metrics
    pub fn new() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            messages_failed: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            total_send_time_us: AtomicU64::new(0),
            batches_sent: AtomicU64::new(0),
            retries: AtomicU64::new(0),
        }
    }

    /// Record a message sent
    pub fn record_message_sent(&self, bytes: usize, duration: Duration) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
        self.total_send_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record a message failed
    pub fn record_message_failed(&self) {
        self.messages_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a batch sent
    pub fn record_batch_sent(&self, messages: usize, bytes: usize) {
        self.batches_sent.fetch_add(1, Ordering::Relaxed);
        self.messages_sent
            .fetch_add(messages as u64, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Record a retry
    pub fn record_retry(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot of metrics
    pub fn snapshot(&self) -> ProducerMetrics {
        ProducerMetrics {
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_failed: self.messages_failed.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            total_send_time_us: self.total_send_time_us.load(Ordering::Relaxed),
            batches_sent: self.batches_sent.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
        }
    }

    /// Reset metrics
    pub fn reset(&self) {
        self.messages_sent.store(0, Ordering::Relaxed);
        self.messages_failed.store(0, Ordering::Relaxed);
        self.bytes_sent.store(0, Ordering::Relaxed);
        self.total_send_time_us.store(0, Ordering::Relaxed);
        self.batches_sent.store(0, Ordering::Relaxed);
        self.retries.store(0, Ordering::Relaxed);
    }
}

impl Default for AtomicProducerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Message statistics
#[derive(Debug, Clone)]
pub struct MessageStats {
    /// Message count
    pub count: usize,
    /// Total bytes
    pub bytes: usize,
    /// Minimum message size
    pub min_size: usize,
    /// Maximum message size
    pub max_size: usize,
    /// Average message size
    pub avg_size: f64,
}

impl MessageStats {
    /// Create new message statistics
    pub fn new() -> Self {
        Self {
            count: 0,
            bytes: 0,
            min_size: usize::MAX,
            max_size: 0,
            avg_size: 0.0,
        }
    }

    /// Add a message to statistics
    pub fn add_message(&mut self, size: usize) {
        self.count += 1;
        self.bytes += size;
        self.min_size = self.min_size.min(size);
        self.max_size = self.max_size.max(size);
        self.avg_size = self.bytes as f64 / self.count as f64;
    }

    /// Merge with another stats
    pub fn merge(&mut self, other: &MessageStats) {
        self.count += other.count;
        self.bytes += other.bytes;
        self.min_size = self.min_size.min(other.min_size);
        self.max_size = self.max_size.max(other.max_size);
        if self.count > 0 {
            self.avg_size = self.bytes as f64 / self.count as f64;
        }
    }
}

impl Default for MessageStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_metrics() {
        let mut metrics = ProducerMetrics::new();

        metrics.record_message_sent(100, Duration::from_millis(10));
        assert_eq!(metrics.messages_sent, 1);
        assert_eq!(metrics.bytes_sent, 100);

        metrics.record_message_failed();
        assert_eq!(metrics.messages_failed, 1);

        metrics.record_batch_sent(10, 1000);
        assert_eq!(metrics.batches_sent, 1);
        assert_eq!(metrics.messages_sent, 11);
        assert_eq!(metrics.bytes_sent, 1100);
    }

    #[test]
    fn test_producer_metrics_calculations() {
        let mut metrics = ProducerMetrics::new();

        metrics.record_message_sent(100, Duration::from_millis(10));
        metrics.record_message_sent(200, Duration::from_millis(20));

        assert_eq!(metrics.average_message_size(), 150.0);
        assert!(metrics.average_send_time().as_millis() >= 10);
        assert_eq!(metrics.success_rate(), 1.0);

        metrics.record_message_failed();
        assert_eq!(metrics.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_atomic_producer_metrics() {
        let metrics = AtomicProducerMetrics::new();

        metrics.record_message_sent(100, Duration::from_millis(10));
        metrics.record_message_sent(200, Duration::from_millis(20));
        metrics.record_batch_sent(10, 1000);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.messages_sent, 12);
        assert_eq!(snapshot.bytes_sent, 1300);
        assert_eq!(snapshot.batches_sent, 1);
    }

    #[test]
    fn test_message_stats() {
        let mut stats = MessageStats::new();

        stats.add_message(100);
        stats.add_message(200);
        stats.add_message(300);

        assert_eq!(stats.count, 3);
        assert_eq!(stats.bytes, 600);
        assert_eq!(stats.min_size, 100);
        assert_eq!(stats.max_size, 300);
        assert_eq!(stats.avg_size, 200.0);
    }

    #[test]
    fn test_message_stats_merge() {
        let mut stats1 = MessageStats::new();
        stats1.add_message(100);
        stats1.add_message(200);

        let mut stats2 = MessageStats::new();
        stats2.add_message(300);
        stats2.add_message(400);

        stats1.merge(&stats2);
        assert_eq!(stats1.count, 4);
        assert_eq!(stats1.bytes, 1000);
        assert_eq!(stats1.min_size, 100);
        assert_eq!(stats1.max_size, 400);
        assert_eq!(stats1.avg_size, 250.0);
    }
}
