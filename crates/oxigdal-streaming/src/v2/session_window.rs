//! Session window processing for geospatial event streams.
//!
//! A session window groups events that occur within `gap_duration` of each other.
//! When a gap larger than `gap_duration` is detected between consecutive events,
//! the current session closes and a new one starts automatically.
//!
//! Additional v2 features:
//! - Configurable **minimum session size** (sessions with fewer than `min_events`
//!   events are silently discarded).
//! - **Maximum session duration** guard: a session that exceeds `max_session_duration`
//!   is force-closed even if no time-gap has been detected.
//! - **`flush()`** to close the currently open session at end of stream.

use std::time::{Duration, SystemTime};

use crate::error::StreamingError;

// ─── StreamEvent ──────────────────────────────────────────────────────────────

/// A single typed event in the stream, carrying a wall-clock timestamp and a
/// monotonically increasing sequence number.
#[derive(Debug, Clone)]
pub struct StreamEvent<T> {
    /// Wall-clock time at which the event occurred.
    pub timestamp: SystemTime,
    /// Application payload.
    pub payload: T,
    /// Sequence number, strictly increasing within a stream.
    pub sequence: u64,
}

impl<T> StreamEvent<T> {
    /// Construct a new `StreamEvent`.
    pub fn new(timestamp: SystemTime, payload: T, sequence: u64) -> Self {
        Self {
            timestamp,
            payload,
            sequence,
        }
    }
}

// ─── SessionWindow ────────────────────────────────────────────────────────────

/// A closed session window containing all events that fell within the session.
#[derive(Debug, Clone)]
pub struct SessionWindow<T> {
    /// Wall-clock timestamp of the first event.
    pub start: SystemTime,
    /// Wall-clock timestamp of the last event.
    pub end: SystemTime,
    /// All events belonging to this session, in arrival order.
    pub events: Vec<StreamEvent<T>>,
    /// Monotonically increasing session identifier (0-based).
    pub session_id: u64,
}

impl<T> SessionWindow<T> {
    /// Duration from first to last event.
    ///
    /// Returns `Duration::ZERO` if `end` is before `start` (clock anomaly).
    pub fn duration(&self) -> Duration {
        self.end
            .duration_since(self.start)
            .unwrap_or(Duration::ZERO)
    }

    /// Number of events in this session.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// `true` if the session contains no events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ─── SessionWindowConfig ──────────────────────────────────────────────────────

/// Configuration for the session window processor.
#[derive(Debug, Clone)]
pub struct SessionWindowConfig {
    /// Maximum gap between consecutive events before the session is closed.
    pub gap_duration: Duration,
    /// Minimum number of events required for a session to be emitted.
    /// Sessions with fewer events are silently discarded.
    pub min_events: usize,
    /// If set, force-close a session whose span exceeds this duration.
    pub max_session_duration: Option<Duration>,
}

impl Default for SessionWindowConfig {
    fn default() -> Self {
        Self {
            gap_duration: Duration::from_secs(30),
            min_events: 1,
            max_session_duration: None,
        }
    }
}

// ─── SessionWindowProcessor ───────────────────────────────────────────────────

/// Stateful processor that groups a time-ordered event stream into session windows.
///
/// Call [`Self::process`] for each incoming event (events **must** arrive in
/// non-decreasing timestamp order). Call [`Self::flush`] at end-of-stream to close
/// any pending session. Completed windows are collected via [`Self::drain_sessions`].
pub struct SessionWindowProcessor<T: Clone> {
    config: SessionWindowConfig,
    /// Events buffered in the currently open session.
    current_session: Option<Vec<StreamEvent<T>>>,
    /// Timestamp of the first event in the current session.
    session_start: Option<SystemTime>,
    /// Timestamp of the most recently processed event.
    last_event_time: Option<SystemTime>,
    /// Counter for generating session IDs.
    next_session_id: u64,
    /// Completed sessions waiting to be drained.
    closed_sessions: Vec<SessionWindow<T>>,
}

impl<T: Clone> SessionWindowProcessor<T> {
    /// Create a new processor with the given configuration.
    pub fn new(config: SessionWindowConfig) -> Self {
        Self {
            config,
            current_session: None,
            session_start: None,
            last_event_time: None,
            next_session_id: 0,
            closed_sessions: Vec::new(),
        }
    }

    /// Process an incoming event.
    ///
    /// A new session is started if:
    /// - There is no open session, **or**
    /// - The gap since the previous event exceeds `gap_duration`, **or**
    /// - The current session has exceeded `max_session_duration`.
    pub fn process(&mut self, event: StreamEvent<T>) -> Result<(), StreamingError> {
        let event_time = event.timestamp;

        // Check whether we should close the current session.
        let gap_exceeded = self.last_event_time.map(|last| {
            event_time.duration_since(last).unwrap_or(Duration::ZERO) > self.config.gap_duration
        });

        let max_exceeded = self
            .session_start
            .zip(self.config.max_session_duration)
            .map(|(start, max)| event_time.duration_since(start).unwrap_or(Duration::ZERO) > max);

        let should_close = gap_exceeded.unwrap_or(false) || max_exceeded.unwrap_or(false);

        if should_close {
            self.close_current_session();
        }

        // Open a new session if none is active.
        if self.current_session.is_none() {
            self.current_session = Some(Vec::new());
            self.session_start = Some(event_time);
        }

        self.last_event_time = Some(event_time);
        if let Some(ref mut session) = self.current_session {
            session.push(event);
        }

        Ok(())
    }

    /// Force-close the currently open session (call at end of stream).
    ///
    /// After flushing, any sessions that meet the `min_events` threshold are
    /// available via [`Self::drain_sessions`].
    pub fn flush(&mut self) {
        self.close_current_session();
    }

    /// Drain and return all completed session windows.
    ///
    /// The internal buffer is cleared; subsequent calls return an empty `Vec`
    /// until more sessions are closed.
    pub fn drain_sessions(&mut self) -> Vec<SessionWindow<T>> {
        std::mem::take(&mut self.closed_sessions)
    }

    /// Number of events buffered in the currently open (not yet closed) session.
    pub fn pending_event_count(&self) -> usize {
        self.current_session.as_ref().map(|s| s.len()).unwrap_or(0)
    }

    /// Total number of sessions that have been closed (includes discarded ones).
    pub fn total_sessions_closed(&self) -> u64 {
        self.next_session_id
    }

    // ── internals ────────────────────────────────────────────────────────────

    fn close_current_session(&mut self) {
        if let (Some(events), Some(start)) =
            (self.current_session.take(), self.session_start.take())
        {
            let session_id = self.next_session_id;
            self.next_session_id += 1;

            if events.len() >= self.config.min_events {
                let end = self.last_event_time.unwrap_or(start);
                self.closed_sessions.push(SessionWindow {
                    start,
                    end,
                    events,
                    session_id,
                });
            }
            // If min_events not met the session is discarded; ID is still consumed.
        }
        self.last_event_time = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn ts(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn event(secs: u64, seq: u64) -> StreamEvent<u32> {
        StreamEvent::new(ts(secs), seq as u32, seq)
    }

    #[test]
    fn test_single_session_from_close_events() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(60),
            min_events: 1,
            max_session_duration: None,
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("process ok");
        proc.process(event(10, 1)).expect("process ok");
        proc.process(event(20, 2)).expect("process ok");
        proc.flush();
        let sessions = proc.drain_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].event_count(), 3);
    }

    #[test]
    fn test_gap_detection_closes_session() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(30),
            min_events: 1,
            max_session_duration: None,
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("process ok");
        // gap of 60 s > 30 s → should close first session
        proc.process(event(60, 1)).expect("process ok");
        proc.flush();
        let sessions = proc.drain_sessions();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_min_events_filter_drops_small_sessions() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(5),
            min_events: 3,
            max_session_duration: None,
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("process ok");
        proc.process(event(1, 1)).expect("process ok");
        // only 2 events < min_events=3
        proc.flush();
        let sessions = proc.drain_sessions();
        assert_eq!(sessions.len(), 0);
    }

    #[test]
    fn test_max_session_duration_force_closes() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(100),
            min_events: 1,
            max_session_duration: Some(Duration::from_secs(50)),
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("process ok");
        // 60 s > max_session_duration=50 s → force close
        proc.process(event(60, 1)).expect("process ok");
        proc.flush();
        let sessions = proc.drain_sessions();
        // two sessions: first force-closed, second from event at t=60
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_flush_closes_open_session() {
        let cfg = SessionWindowConfig::default();
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("process ok");
        assert_eq!(proc.pending_event_count(), 1);
        proc.flush();
        assert_eq!(proc.pending_event_count(), 0);
        let sessions = proc.drain_sessions();
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn test_multiple_sessions_from_gapped_stream() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(10),
            min_events: 1,
            max_session_duration: None,
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        // Session 1
        proc.process(event(0, 0)).expect("ok");
        proc.process(event(5, 1)).expect("ok");
        // gap of 30 s
        // Session 2
        proc.process(event(35, 2)).expect("ok");
        proc.process(event(40, 3)).expect("ok");
        // gap of 60 s
        // Session 3
        proc.process(event(100, 4)).expect("ok");
        proc.flush();
        let sessions = proc.drain_sessions();
        assert_eq!(sessions.len(), 3);
    }

    #[test]
    fn test_session_id_increments() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(5),
            min_events: 1,
            max_session_duration: None,
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("ok");
        proc.process(event(20, 1)).expect("ok"); // gap → closes session 0
        proc.flush(); // closes session 1
        let sessions = proc.drain_sessions();
        assert_eq!(sessions[0].session_id, 0);
        assert_eq!(sessions[1].session_id, 1);
    }

    #[test]
    fn test_session_duration_computation() {
        let cfg = SessionWindowConfig::default();
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(100, 0)).expect("ok");
        proc.process(event(110, 1)).expect("ok");
        proc.flush();
        let sessions = proc.drain_sessions();
        assert_eq!(sessions[0].duration(), Duration::from_secs(10));
    }

    #[test]
    fn test_empty_processor_has_no_sessions() {
        let mut proc: SessionWindowProcessor<u32> = SessionWindowProcessor::new(Default::default());
        proc.flush();
        assert_eq!(proc.drain_sessions().len(), 0);
    }

    #[test]
    fn test_events_within_gap_stay_in_same_session() {
        let cfg = SessionWindowConfig {
            gap_duration: Duration::from_secs(60),
            min_events: 1,
            max_session_duration: None,
        };
        let mut proc = SessionWindowProcessor::new(cfg);
        for i in 0..10u64 {
            proc.process(event(i * 5, i)).expect("ok"); // every 5 s, gap=60 s
        }
        proc.flush();
        let sessions = proc.drain_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].event_count(), 10);
    }

    #[test]
    fn test_pending_event_count_resets_after_flush() {
        let cfg = SessionWindowConfig::default();
        let mut proc = SessionWindowProcessor::new(cfg);
        proc.process(event(0, 0)).expect("ok");
        proc.process(event(1, 1)).expect("ok");
        assert_eq!(proc.pending_event_count(), 2);
        proc.flush();
        assert_eq!(proc.pending_event_count(), 0);
    }
}
