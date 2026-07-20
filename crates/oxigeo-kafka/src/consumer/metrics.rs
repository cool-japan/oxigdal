//! Consumer metrics

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Consumer metrics
#[derive(Debug, Default, Clone)]
pub struct ConsumerMetrics {
    /// Total number of messages received
    pub messages_received: u64,
    /// Total number of messages failed
    pub messages_failed: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Total receive time in microseconds
    pub total_receive_time_us: u64,
    /// Number of commits
    pub commits: u64,
    /// Number of rebalances
    pub rebalances: u64,
    /// Number of partition assignments
    pub partition_assignments: u64,
    /// Number of partition revocations
    pub partition_revocations: u64,
}

impl ConsumerMetrics {
    /// Create new consumer metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a message received
    pub fn record_message_received(&mut self, bytes: usize, duration: Duration) {
        self.messages_received += 1;
        self.bytes_received += bytes as u64;
        self.total_receive_time_us += duration.as_micros() as u64;
    }

    /// Record a message failed
    pub fn record_message_failed(&mut self) {
        self.messages_failed += 1;
    }

    /// Record a commit
    pub fn record_commit(&mut self) {
        self.commits += 1;
    }

    /// Record a rebalance
    pub fn record_rebalance(&mut self) {
        self.rebalances += 1;
    }

    /// Record partition assignment
    pub fn record_partition_assigned(&mut self, count: usize) {
        self.partition_assignments += count as u64;
    }

    /// Record partition revocation
    pub fn record_partition_revoked(&mut self, count: usize) {
        self.partition_revocations += count as u64;
    }

    /// Get average message size
    pub fn average_message_size(&self) -> f64 {
        if self.messages_received == 0 {
            0.0
        } else {
            self.bytes_received as f64 / self.messages_received as f64
        }
    }

    /// Get average receive time
    pub fn average_receive_time(&self) -> Duration {
        Duration::from_micros(
            self.total_receive_time_us
                .checked_div(self.messages_received)
                .unwrap_or(0),
        )
    }

    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.messages_received + self.messages_failed;
        if total == 0 {
            1.0
        } else {
            self.messages_received as f64 / total as f64
        }
    }

    /// Get throughput in messages per second
    pub fn throughput_mps(&self, duration: Duration) -> f64 {
        let duration_secs = duration.as_secs_f64();
        if duration_secs == 0.0 {
            0.0
        } else {
            self.messages_received as f64 / duration_secs
        }
    }

    /// Get throughput in bytes per second
    pub fn throughput_bps(&self, duration: Duration) -> f64 {
        let duration_secs = duration.as_secs_f64();
        if duration_secs == 0.0 {
            0.0
        } else {
            self.bytes_received as f64 / duration_secs
        }
    }

    /// Reset metrics
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Thread-safe consumer metrics
pub struct AtomicConsumerMetrics {
    messages_received: AtomicU64,
    messages_failed: AtomicU64,
    bytes_received: AtomicU64,
    total_receive_time_us: AtomicU64,
    commits: AtomicU64,
    rebalances: AtomicU64,
    partition_assignments: AtomicU64,
    partition_revocations: AtomicU64,
}

impl AtomicConsumerMetrics {
    /// Create new atomic consumer metrics
    pub fn new() -> Self {
        Self {
            messages_received: AtomicU64::new(0),
            messages_failed: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            total_receive_time_us: AtomicU64::new(0),
            commits: AtomicU64::new(0),
            rebalances: AtomicU64::new(0),
            partition_assignments: AtomicU64::new(0),
            partition_revocations: AtomicU64::new(0),
        }
    }

    /// Record a message received
    pub fn record_message_received(&self, bytes: usize, duration: Duration) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
        self.total_receive_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record a message failed
    pub fn record_message_failed(&self) {
        self.messages_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a commit
    pub fn record_commit(&self) {
        self.commits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rebalance
    pub fn record_rebalance(&self) {
        self.rebalances.fetch_add(1, Ordering::Relaxed);
    }

    /// Record partition assignment
    pub fn record_partition_assigned(&self, count: usize) {
        self.partition_assignments
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record partition revocation
    pub fn record_partition_revoked(&self, count: usize) {
        self.partition_revocations
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Get snapshot of metrics
    pub fn snapshot(&self) -> ConsumerMetrics {
        ConsumerMetrics {
            messages_received: self.messages_received.load(Ordering::Relaxed),
            messages_failed: self.messages_failed.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            total_receive_time_us: self.total_receive_time_us.load(Ordering::Relaxed),
            commits: self.commits.load(Ordering::Relaxed),
            rebalances: self.rebalances.load(Ordering::Relaxed),
            partition_assignments: self.partition_assignments.load(Ordering::Relaxed),
            partition_revocations: self.partition_revocations.load(Ordering::Relaxed),
        }
    }

    /// Reset metrics
    pub fn reset(&self) {
        self.messages_received.store(0, Ordering::Relaxed);
        self.messages_failed.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.total_receive_time_us.store(0, Ordering::Relaxed);
        self.commits.store(0, Ordering::Relaxed);
        self.rebalances.store(0, Ordering::Relaxed);
        self.partition_assignments.store(0, Ordering::Relaxed);
        self.partition_revocations.store(0, Ordering::Relaxed);
    }
}

impl Default for AtomicConsumerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Consumer statistics per partition
#[derive(Debug, Clone)]
pub struct ConsumerStats {
    /// Topic name
    pub topic: String,
    /// Partition number
    pub partition: i32,
    /// Current offset
    pub current_offset: i64,
    /// Lag (high water mark - current offset)
    pub lag: i64,
    /// Messages consumed
    pub messages_consumed: u64,
    /// Bytes consumed
    pub bytes_consumed: u64,
}

impl ConsumerStats {
    /// Create new consumer statistics
    pub fn new(topic: String, partition: i32) -> Self {
        Self {
            topic,
            partition,
            current_offset: -1,
            lag: 0,
            messages_consumed: 0,
            bytes_consumed: 0,
        }
    }

    /// Update offset
    pub fn update_offset(&mut self, offset: i64, high_water_mark: i64) {
        self.current_offset = offset;
        self.lag = high_water_mark - offset;
    }

    /// Record message consumed
    pub fn record_message(&mut self, bytes: usize) {
        self.messages_consumed += 1;
        self.bytes_consumed += bytes as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_metrics() {
        let mut metrics = ConsumerMetrics::new();

        metrics.record_message_received(100, Duration::from_millis(10));
        assert_eq!(metrics.messages_received, 1);
        assert_eq!(metrics.bytes_received, 100);

        metrics.record_message_failed();
        assert_eq!(metrics.messages_failed, 1);

        metrics.record_commit();
        assert_eq!(metrics.commits, 1);

        metrics.record_rebalance();
        assert_eq!(metrics.rebalances, 1);
    }

    #[test]
    fn test_consumer_metrics_calculations() {
        let mut metrics = ConsumerMetrics::new();

        metrics.record_message_received(100, Duration::from_millis(10));
        metrics.record_message_received(200, Duration::from_millis(20));

        assert_eq!(metrics.average_message_size(), 150.0);
        assert!(metrics.average_receive_time().as_millis() >= 10);
        assert_eq!(metrics.success_rate(), 1.0);

        metrics.record_message_failed();
        assert_eq!(metrics.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_atomic_consumer_metrics() {
        let metrics = AtomicConsumerMetrics::new();

        metrics.record_message_received(100, Duration::from_millis(10));
        metrics.record_message_received(200, Duration::from_millis(20));
        metrics.record_commit();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.messages_received, 2);
        assert_eq!(snapshot.bytes_received, 300);
        assert_eq!(snapshot.commits, 1);
    }

    #[test]
    fn test_consumer_stats() {
        let mut stats = ConsumerStats::new("test-topic".to_string(), 0);

        assert_eq!(stats.current_offset, -1);
        assert_eq!(stats.lag, 0);

        stats.update_offset(100, 200);
        assert_eq!(stats.current_offset, 100);
        assert_eq!(stats.lag, 100);

        stats.record_message(256);
        assert_eq!(stats.messages_consumed, 1);
        assert_eq!(stats.bytes_consumed, 256);
    }
}
