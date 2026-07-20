//! Session window implementation.

use super::window::{Window, WindowAssigner};
use crate::core::stream::StreamElement;
use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuration for session windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionWindowConfig {
    /// Gap duration between sessions
    pub gap: Duration,

    /// Maximum session duration (optional)
    pub max_duration: Option<Duration>,
}

impl SessionWindowConfig {
    /// Create a new session window configuration.
    pub fn new(gap: Duration) -> Self {
        Self {
            gap,
            max_duration: None,
        }
    }

    /// Set the maximum session duration.
    pub fn with_max_duration(mut self, max_duration: Duration) -> Self {
        self.max_duration = Some(max_duration);
        self
    }
}

/// Session window (dynamic windows based on activity).
#[derive(Debug)]
pub struct SessionWindow {
    config: SessionWindowConfig,
    sessions: BTreeMap<DateTime<Utc>, Window>,
}

impl SessionWindow {
    /// Create a new session window.
    pub fn new(gap: Duration) -> Self {
        Self {
            config: SessionWindowConfig::new(gap),
            sessions: BTreeMap::new(),
        }
    }

    /// Create a new session window with maximum duration.
    pub fn with_max_duration(gap: Duration, max_duration: Duration) -> Self {
        Self {
            config: SessionWindowConfig::new(gap).with_max_duration(max_duration),
            sessions: BTreeMap::new(),
        }
    }

    /// Assign an element to a session window.
    pub fn assign(&mut self, timestamp: DateTime<Utc>) -> Result<Window> {
        let mut merged_window = None;
        let mut windows_to_remove = Vec::new();

        for (start, window) in &self.sessions {
            if timestamp >= window.start && timestamp <= window.end {
                merged_window = Some(window.clone());
                windows_to_remove.push(*start);
            } else if timestamp > window.end && timestamp - window.end < self.config.gap {
                let new_end = timestamp + self.config.gap;
                let mut new_window = Window::new(window.start, new_end)?;

                if let Some(max_dur) = self.config.max_duration
                    && new_window.duration() > max_dur
                {
                    new_window = Window::new(new_window.end - max_dur, new_window.end)?;
                }

                if let Some(existing) = merged_window {
                    merged_window = existing.merge(&new_window);
                } else {
                    merged_window = Some(new_window);
                }

                windows_to_remove.push(*start);
            } else if timestamp < window.start && window.start - timestamp < self.config.gap {
                // Backward extension: a late / out-of-order event arrives with a
                // timestamp earlier than an existing session's start, but within
                // the inactivity gap. The session must be extended backward to
                // begin at the new (earlier) event rather than fragmenting into
                // a disjoint overlapping session.
                let new_start = timestamp;
                let mut new_window = Window::new(new_start, window.end)?;

                if let Some(max_dur) = self.config.max_duration
                    && new_window.duration() > max_dur
                {
                    new_window = Window::new(new_window.end - max_dur, new_window.end)?;
                }

                if let Some(existing) = merged_window {
                    merged_window = existing.merge(&new_window);
                } else {
                    merged_window = Some(new_window);
                }

                windows_to_remove.push(*start);
            }
        }

        for start in windows_to_remove {
            self.sessions.remove(&start);
        }

        let result_window = if let Some(window) = merged_window {
            window
        } else {
            Window::new(timestamp, timestamp + self.config.gap)?
        };

        self.sessions
            .insert(result_window.start, result_window.clone());

        Ok(result_window)
    }

    /// Get all active sessions.
    pub fn active_sessions(&self) -> Vec<Window> {
        self.sessions.values().cloned().collect()
    }

    /// Clear expired sessions.
    pub fn clear_expired(&mut self, watermark: DateTime<Utc>) {
        self.sessions.retain(|_, window| window.end > watermark);
    }
}

/// Assigner for session windows.
pub struct SessionAssigner {
    config: SessionWindowConfig,
}

impl SessionAssigner {
    /// Create a new session window assigner.
    pub fn new(gap: Duration) -> Self {
        Self {
            config: SessionWindowConfig::new(gap),
        }
    }

    /// Create a new session window assigner with maximum duration.
    pub fn with_max_duration(gap: Duration, max_duration: Duration) -> Self {
        Self {
            config: SessionWindowConfig::new(gap).with_max_duration(max_duration),
        }
    }
}

impl WindowAssigner for SessionAssigner {
    fn assign_windows(&self, element: &StreamElement) -> Result<Vec<Window>> {
        let start = element.event_time;
        let end = start + self.config.gap;
        Ok(vec![Window::new(start, end)?])
    }

    fn assigner_type(&self) -> &str {
        "SessionAssigner"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_window() {
        let mut window = SessionWindow::new(Duration::seconds(60));
        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");

        let w1 = window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");
        assert!(w1.contains(&ts1));
        assert_eq!(w1.duration(), Duration::seconds(60));
    }

    #[test]
    fn test_session_window_merge() {
        let mut window = SessionWindow::new(Duration::seconds(60));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        let ts2 = ts1 + Duration::seconds(30);

        let _w1 = window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");
        let w2 = window
            .assign(ts2)
            .expect("Session window assignment should succeed in test");

        assert!(w2.contains(&ts1));
        assert!(w2.contains(&ts2));
    }

    #[test]
    fn test_session_window_separate() {
        let mut window = SessionWindow::new(Duration::seconds(60));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        let ts2 = ts1 + Duration::seconds(120);

        let w1 = window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");
        let w2 = window
            .assign(ts2)
            .expect("Session window assignment should succeed in test");

        assert!(!w1.contains(&ts2));
        assert!(!w2.contains(&ts1));
    }

    #[test]
    fn test_session_window_late_arrival() {
        // Out-of-order (late) event earlier than an existing session start, but
        // within the gap, must merge backward into a single session.
        let mut window = SessionWindow::new(Duration::seconds(60));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        // Establishes session [1000, 1060).
        window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");

        // A late event 10s before the session start, within the 60s gap.
        let ts_late = ts1 - Duration::seconds(10);
        let merged = window
            .assign(ts_late)
            .expect("Session window assignment should succeed in test");

        // The merged session must cover both the late event and the original.
        assert!(
            merged.contains(&ts_late),
            "merged session must contain the late event"
        );
        assert!(
            merged.contains(&ts1),
            "merged session must contain the original event"
        );

        // There must be exactly one session, not two disjoint/overlapping ones.
        assert_eq!(
            window.active_sessions().len(),
            1,
            "late arrival within gap must not fragment into a new session"
        );

        // The single session must start at the late event.
        let sessions = window.active_sessions();
        assert_eq!(sessions[0].start, ts_late);
        assert_eq!(sessions[0].end, ts1 + Duration::seconds(60));
    }

    #[test]
    fn test_session_window_late_arrival_beyond_gap() {
        // A late event further back than the gap must NOT merge; it stays a
        // separate session (symmetric to test_session_window_separate).
        let mut window = SessionWindow::new(Duration::seconds(60));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");

        let ts_far = ts1 - Duration::seconds(120);
        let w = window
            .assign(ts_far)
            .expect("Session window assignment should succeed in test");

        assert!(!w.contains(&ts1));
        assert_eq!(window.active_sessions().len(), 2);
    }

    #[test]
    fn test_session_window_max_duration() {
        let mut window =
            SessionWindow::with_max_duration(Duration::seconds(10), Duration::seconds(100));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");

        let ts2 = ts1 + Duration::seconds(200);
        let w = window
            .assign(ts2)
            .expect("Session window assignment should succeed in test");

        assert!(w.duration() <= Duration::seconds(100));
    }

    #[test]
    fn test_session_assigner() {
        let assigner = SessionAssigner::new(Duration::seconds(60));
        let elem = StreamElement::new(
            vec![1, 2, 3],
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed"),
        );

        let windows = assigner
            .assign_windows(&elem)
            .expect("Session window assigner should succeed in test");
        assert_eq!(windows.len(), 1);
        assert!(windows[0].contains(&elem.event_time));
    }

    #[test]
    fn test_clear_expired() {
        let mut window = SessionWindow::new(Duration::seconds(60));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        let ts2 = ts1 + Duration::seconds(200);

        window
            .assign(ts1)
            .expect("Session window assignment should succeed in test");
        window
            .assign(ts2)
            .expect("Session window assignment should succeed in test");

        assert_eq!(window.active_sessions().len(), 2);

        let watermark = ts1 + Duration::seconds(100);
        window.clear_expired(watermark);

        assert_eq!(window.active_sessions().len(), 1);
    }
}
