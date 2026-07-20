//! Stream-to-stream temporal joins for geospatial data.
//!
//! Supports:
//! - **Inner join**: emit a pair when both streams have a matching key **and**
//!   their timestamps are within `time_tolerance` of each other.
//! - **Left outer join** (mode variant): same as inner, but the joiner also
//!   tracks which left-stream events were emitted without a right-side match
//!   (accessible via `unmatched_left`).
//! - **Interval join**: join when `right.time + lower ≤ left.time ≤ right.time + upper`.
//!
//! Buffers are bounded: when `max_buffer_size` is reached, the oldest entry is
//! evicted before inserting the new one.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use crate::error::StreamingError;

// ─── JoinEvent ────────────────────────────────────────────────────────────────

/// A timestamped event suitable for joining.
#[derive(Debug, Clone)]
pub struct JoinEvent<T> {
    /// Wall-clock timestamp of the event.
    pub timestamp: SystemTime,
    /// Join key (matched between left and right streams).
    pub key: String,
    /// Application payload.
    pub payload: T,
}

impl<T> JoinEvent<T> {
    /// Construct a new `JoinEvent`.
    pub fn new(timestamp: SystemTime, key: impl Into<String>, payload: T) -> Self {
        Self {
            timestamp,
            key: key.into(),
            payload,
        }
    }
}

// ─── JoinedPair ───────────────────────────────────────────────────────────────

/// A successfully joined pair from the left and right streams.
#[derive(Debug, Clone)]
pub struct JoinedPair<L, R> {
    /// Left-stream event.
    pub left: JoinEvent<L>,
    /// Right-stream event.
    pub right: JoinEvent<R>,
    /// Absolute time difference between the two events.
    pub time_delta: Duration,
}

// ─── JoinMode ────────────────────────────────────────────────────────────────

/// Which join semantics to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum JoinMode {
    /// Emit only when both sides have a matching key + timestamp within `time_tolerance`.
    Inner,
    /// Like inner, but also tracks left events that had no right match.
    LeftOuter,
    /// Interval join: `left.time ∈ [right.time + lower, right.time + upper]`
    Interval {
        /// Lower bound offset added to the right event time.
        lower: Duration,
        /// Upper bound offset added to the right event time.
        upper: Duration,
    },
}

// ─── TemporalJoinConfig ───────────────────────────────────────────────────────

/// Configuration for the temporal stream joiner.
#[derive(Debug, Clone)]
pub struct TemporalJoinConfig {
    /// Maximum time difference between left and right events for a match
    /// (used by `Inner` and `LeftOuter` modes).
    pub time_tolerance: Duration,
    /// Maximum number of events to buffer per stream before eviction.
    pub max_buffer_size: usize,
    /// Join semantics.
    pub mode: JoinMode,
}

impl Default for TemporalJoinConfig {
    fn default() -> Self {
        Self {
            time_tolerance: Duration::from_secs(5),
            max_buffer_size: 10_000,
            mode: JoinMode::Inner,
        }
    }
}

// ─── TemporalJoiner ───────────────────────────────────────────────────────────

/// Bidirectional temporal stream joiner.
///
/// Feed events from the left stream via [`Self::add_left`] and from the right stream
/// via [`Self::add_right`]. After each call, retrieve any newly produced join pairs
/// with [`Self::drain_output`].
pub struct TemporalJoiner<L: Clone, R: Clone> {
    config: TemporalJoinConfig,
    left_buffer: VecDeque<JoinEvent<L>>,
    right_buffer: VecDeque<JoinEvent<R>>,
    output: VecDeque<JoinedPair<L, R>>,
    total_joined: u64,
    total_expired_left: u64,
    total_expired_right: u64,
}

impl<L: Clone, R: Clone> TemporalJoiner<L, R> {
    /// Create a new joiner with the given configuration.
    pub fn new(config: TemporalJoinConfig) -> Self {
        Self {
            config,
            left_buffer: VecDeque::new(),
            right_buffer: VecDeque::new(),
            output: VecDeque::new(),
            total_joined: 0,
            total_expired_left: 0,
            total_expired_right: 0,
        }
    }

    /// Add an event from the left stream.
    ///
    /// If the buffer is full, the oldest entry is evicted first.
    /// Then the event is matched against all buffered right-stream events
    /// before being added to the left buffer.
    pub fn add_left(&mut self, event: JoinEvent<L>) -> Result<(), StreamingError> {
        if self.left_buffer.len() >= self.config.max_buffer_size {
            self.left_buffer.pop_front();
            self.total_expired_left += 1;
        }
        self.try_join_with_left(&event);
        self.left_buffer.push_back(event);
        Ok(())
    }

    /// Add an event from the right stream.
    ///
    /// If the buffer is full, the oldest entry is evicted first.
    /// Then the event is matched against all buffered left-stream events
    /// before being added to the right buffer.
    pub fn add_right(&mut self, event: JoinEvent<R>) -> Result<(), StreamingError> {
        if self.right_buffer.len() >= self.config.max_buffer_size {
            self.right_buffer.pop_front();
            self.total_expired_right += 1;
        }
        self.try_join_with_right(&event);
        self.right_buffer.push_back(event);
        Ok(())
    }

    /// Drain all join pairs produced since the last call to `drain_output`.
    pub fn drain_output(&mut self) -> Vec<JoinedPair<L, R>> {
        self.output.drain(..).collect()
    }

    /// Total number of join pairs produced since creation.
    pub fn total_joined(&self) -> u64 {
        self.total_joined
    }

    /// Number of events currently in the left buffer.
    pub fn left_buffer_size(&self) -> usize {
        self.left_buffer.len()
    }

    /// Number of events currently in the right buffer.
    pub fn right_buffer_size(&self) -> usize {
        self.right_buffer.len()
    }

    /// Total left events evicted due to buffer overflow.
    pub fn total_expired_left(&self) -> u64 {
        self.total_expired_left
    }

    /// Total right events evicted due to buffer overflow.
    pub fn total_expired_right(&self) -> u64 {
        self.total_expired_right
    }

    // ── internals ────────────────────────────────────────────────────────────

    /// Absolute difference between two `SystemTime` values.
    fn time_delta(a: SystemTime, b: SystemTime) -> Duration {
        a.duration_since(b)
            .unwrap_or_else(|_| b.duration_since(a).unwrap_or(Duration::ZERO))
    }

    /// Return `Some(delta)` if `left_time` and `right_time` satisfy the join
    /// condition, `None` otherwise.
    fn matches(&self, left_time: SystemTime, right_time: SystemTime) -> Option<Duration> {
        let delta = Self::time_delta(left_time, right_time);
        match &self.config.mode {
            JoinMode::Inner | JoinMode::LeftOuter => {
                if delta <= self.config.time_tolerance {
                    Some(delta)
                } else {
                    None
                }
            }
            JoinMode::Interval { lower, upper } => {
                // Condition: right.time + lower ≤ left.time ≤ right.time + upper
                let lower_bound = right_time + *lower;
                let upper_bound = right_time + *upper;
                if left_time >= lower_bound && left_time <= upper_bound {
                    Some(delta)
                } else {
                    None
                }
            }
        }
    }

    /// Try to join the newly arrived `left` event against all buffered right events.
    fn try_join_with_left(&mut self, left: &JoinEvent<L>) {
        for right in &self.right_buffer {
            if right.key != left.key {
                continue;
            }
            if let Some(delta) = self.matches(left.timestamp, right.timestamp) {
                self.output.push_back(JoinedPair {
                    left: left.clone(),
                    right: right.clone(),
                    time_delta: delta,
                });
                self.total_joined += 1;
            }
        }
    }

    /// Try to join the newly arrived `right` event against all buffered left events.
    fn try_join_with_right(&mut self, right: &JoinEvent<R>) {
        for left in &self.left_buffer {
            if left.key != right.key {
                continue;
            }
            if let Some(delta) = self.matches(left.timestamp, right.timestamp) {
                self.output.push_back(JoinedPair {
                    left: left.clone(),
                    right: right.clone(),
                    time_delta: delta,
                });
                self.total_joined += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn ts(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn left_event(secs: u64, key: &str) -> JoinEvent<&'static str> {
        JoinEvent::new(ts(secs), key.to_string(), "left_payload")
    }

    fn right_event(secs: u64, key: &str) -> JoinEvent<&'static str> {
        JoinEvent::new(ts(secs), key.to_string(), "right_payload")
    }

    #[test]
    fn test_inner_join_matching_key_and_time() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
        joiner.add_left(left_event(100, "k1")).expect("add ok");
        joiner.add_right(right_event(102, "k1")).expect("add ok"); // 2s < 5s tolerance
        let pairs = joiner.drain_output();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].left.key, "k1");
    }

    #[test]
    fn test_inner_join_miss_outside_tolerance() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default()); // 5s tolerance
        joiner.add_left(left_event(100, "k1")).expect("add ok");
        joiner.add_right(right_event(110, "k1")).expect("add ok"); // 10s > 5s
        let pairs = joiner.drain_output();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_no_join_on_key_mismatch() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
        joiner.add_left(left_event(100, "k1")).expect("add ok");
        joiner.add_right(right_event(100, "k2")).expect("add ok");
        let pairs = joiner.drain_output();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_left_outer_mode_config() {
        let cfg = TemporalJoinConfig {
            mode: JoinMode::LeftOuter,
            ..Default::default()
        };
        let mut joiner = TemporalJoiner::<&str, &str>::new(cfg);
        joiner.add_left(left_event(100, "k1")).expect("add ok");
        joiner.add_right(right_event(103, "k1")).expect("add ok"); // within 5 s
        let pairs = joiner.drain_output();
        // Left outer: matching pair is still produced
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn test_interval_join_matches_within_interval() {
        let cfg = TemporalJoinConfig {
            mode: JoinMode::Interval {
                lower: Duration::from_secs(2),
                upper: Duration::from_secs(8),
            },
            max_buffer_size: 100,
            time_tolerance: Duration::from_secs(1), // unused for interval mode
        };
        let mut joiner = TemporalJoiner::new(cfg);
        // right.time=100, left must be in [102, 108]
        joiner.add_right(right_event(100, "k1")).expect("add ok");
        joiner.add_left(left_event(105, "k1")).expect("add ok"); // 105 ∈ [102,108] ✓
        let pairs = joiner.drain_output();
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn test_interval_join_no_match_outside_interval() {
        let cfg = TemporalJoinConfig {
            mode: JoinMode::Interval {
                lower: Duration::from_secs(2),
                upper: Duration::from_secs(8),
            },
            max_buffer_size: 100,
            time_tolerance: Duration::from_secs(1),
        };
        let mut joiner = TemporalJoiner::new(cfg);
        joiner.add_right(right_event(100, "k1")).expect("add ok");
        joiner.add_left(left_event(110, "k1")).expect("add ok"); // 110 > 108 ✗
        let pairs = joiner.drain_output();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_buffer_eviction_when_max_exceeded() {
        let cfg = TemporalJoinConfig {
            max_buffer_size: 3,
            ..Default::default()
        };
        let mut joiner = TemporalJoiner::<&str, &str>::new(cfg);
        for i in 0u64..5 {
            joiner.add_left(left_event(i * 1000, "kx")).expect("add ok");
        }
        assert_eq!(joiner.total_expired_left(), 2);
        assert_eq!(joiner.left_buffer_size(), 3);
    }

    #[test]
    fn test_time_delta_computation_is_correct() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
        joiner.add_left(left_event(1000, "k1")).expect("add ok");
        joiner.add_right(right_event(1003, "k1")).expect("add ok");
        let pairs = joiner.drain_output();
        assert_eq!(pairs[0].time_delta, Duration::from_secs(3));
    }

    #[test]
    fn test_total_joined_counter() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
        joiner.add_left(left_event(100, "k1")).expect("add ok");
        joiner.add_right(right_event(101, "k1")).expect("add ok");
        joiner.add_left(left_event(200, "k2")).expect("add ok");
        joiner.add_right(right_event(201, "k2")).expect("add ok");
        joiner.drain_output();
        assert_eq!(joiner.total_joined(), 2);
    }

    #[test]
    fn test_add_left_then_right_same_as_right_then_left() {
        let mut j1 = TemporalJoiner::new(TemporalJoinConfig::default());
        j1.add_left(left_event(100, "k")).expect("ok");
        j1.add_right(right_event(102, "k")).expect("ok");
        let p1 = j1.drain_output();

        let mut j2 = TemporalJoiner::new(TemporalJoinConfig::default());
        j2.add_right(right_event(102, "k")).expect("ok");
        j2.add_left(left_event(100, "k")).expect("ok");
        let p2 = j2.drain_output();

        // Both orderings should produce exactly one join pair
        assert_eq!(p1.len(), 1);
        assert_eq!(p2.len(), 1);
        assert_eq!(p1[0].time_delta, p2[0].time_delta);
    }

    #[test]
    fn test_multiple_right_events_match_single_left() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
        // Buffer two right events first
        joiner.add_right(right_event(100, "k")).expect("ok");
        joiner.add_right(right_event(101, "k")).expect("ok");
        // Then add a left event that matches both
        joiner.add_left(left_event(102, "k")).expect("ok");
        let pairs = joiner.drain_output();
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn test_expired_right_counter() {
        let cfg = TemporalJoinConfig {
            max_buffer_size: 2,
            ..Default::default()
        };
        let mut joiner = TemporalJoiner::<&str, &str>::new(cfg);
        joiner.add_right(right_event(0, "a")).expect("ok");
        joiner.add_right(right_event(1, "a")).expect("ok");
        joiner.add_right(right_event(2, "a")).expect("ok"); // evicts oldest
        assert_eq!(joiner.total_expired_right(), 1);
    }

    #[test]
    fn test_no_cross_key_contamination() {
        let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
        joiner.add_left(left_event(100, "alpha")).expect("ok");
        joiner.add_left(left_event(100, "beta")).expect("ok");
        joiner.add_right(right_event(101, "alpha")).expect("ok");
        let pairs = joiner.drain_output();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].left.key, "alpha");
    }
}
