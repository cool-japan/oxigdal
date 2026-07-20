//! Tumbling window implementation.

use super::window::{Window, WindowAssigner};
use crate::core::stream::StreamElement;
use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Configuration for tumbling windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TumblingWindowConfig {
    /// Window size
    pub size: Duration,

    /// Window offset (for alignment)
    pub offset: Duration,
}

impl TumblingWindowConfig {
    /// Create a new tumbling window configuration.
    pub fn new(size: Duration) -> Self {
        Self {
            size,
            offset: Duration::zero(),
        }
    }

    /// Set the window offset.
    pub fn with_offset(mut self, offset: Duration) -> Self {
        self.offset = offset;
        self
    }
}

/// Tumbling window (fixed, non-overlapping windows).
#[derive(Debug, Clone)]
pub struct TumblingWindow {
    config: TumblingWindowConfig,
}

impl TumblingWindow {
    /// Create a new tumbling window.
    pub fn new(size: Duration) -> Self {
        Self {
            config: TumblingWindowConfig::new(size),
        }
    }

    /// Create a new tumbling window with offset.
    pub fn with_offset(size: Duration, offset: Duration) -> Self {
        Self {
            config: TumblingWindowConfig::new(size).with_offset(offset),
        }
    }

    /// Get the window for a given timestamp.
    pub fn get_window(&self, timestamp: DateTime<Utc>) -> Result<Window> {
        let size_ms = self.config.size.num_milliseconds();
        let offset_ms = self.config.offset.num_milliseconds();

        let timestamp_ms = timestamp.timestamp_millis();
        let adjusted_timestamp = timestamp_ms - offset_ms;

        let window_start_ms = (adjusted_timestamp / size_ms) * size_ms + offset_ms;
        let window_end_ms = window_start_ms + size_ms;

        let start = DateTime::from_timestamp_millis(window_start_ms).ok_or_else(|| {
            crate::error::StreamingError::InvalidWindow(
                "Invalid window start timestamp".to_string(),
            )
        })?;

        let end = DateTime::from_timestamp_millis(window_end_ms).ok_or_else(|| {
            crate::error::StreamingError::InvalidWindow("Invalid window end timestamp".to_string())
        })?;

        Window::new(start, end)
    }
}

/// Assigner for tumbling windows.
pub struct TumblingAssigner {
    window: TumblingWindow,
}

impl TumblingAssigner {
    /// Create a new tumbling window assigner.
    pub fn new(size: Duration) -> Self {
        Self {
            window: TumblingWindow::new(size),
        }
    }

    /// Create a new tumbling window assigner with offset.
    pub fn with_offset(size: Duration, offset: Duration) -> Self {
        Self {
            window: TumblingWindow::with_offset(size, offset),
        }
    }
}

impl WindowAssigner for TumblingAssigner {
    fn assign_windows(&self, element: &StreamElement) -> Result<Vec<Window>> {
        let window = self.window.get_window(element.event_time)?;
        Ok(vec![window])
    }

    fn assigner_type(&self) -> &str {
        "TumblingAssigner"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tumbling_window() {
        let window = TumblingWindow::new(Duration::seconds(60));
        let timestamp =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");

        let w = window
            .get_window(timestamp)
            .expect("Tumbling window calculation should succeed in test");
        assert_eq!(w.duration(), Duration::seconds(60));
        assert!(w.contains(&timestamp));
    }

    #[test]
    fn test_tumbling_window_with_offset() {
        let window = TumblingWindow::with_offset(Duration::seconds(60), Duration::seconds(15));
        let timestamp =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");

        let w = window
            .get_window(timestamp)
            .expect("Tumbling window calculation should succeed in test");
        assert_eq!(w.duration(), Duration::seconds(60));
    }

    #[test]
    fn test_tumbling_assigner() {
        let assigner = TumblingAssigner::new(Duration::seconds(60));
        let elem = StreamElement::new(
            vec![1, 2, 3],
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed"),
        );

        let windows = assigner
            .assign_windows(&elem)
            .expect("Window assignment should succeed in test");
        assert_eq!(windows.len(), 1);
        assert!(windows[0].contains(&elem.event_time));
    }

    #[test]
    fn test_non_overlapping_windows() {
        let window = TumblingWindow::new(Duration::seconds(60));

        let ts1 =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");
        let ts2 = ts1 + Duration::seconds(70);

        let w1 = window
            .get_window(ts1)
            .expect("Tumbling window calculation should succeed in test");
        let w2 = window
            .get_window(ts2)
            .expect("Tumbling window calculation should succeed in test");

        assert!(!w1.overlaps(&w2));
        assert!(!w2.overlaps(&w1));
    }
}
