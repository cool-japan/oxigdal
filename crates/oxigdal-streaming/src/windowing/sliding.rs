//! Sliding window implementation.

use super::window::{Window, WindowAssigner};
use crate::core::stream::StreamElement;
use crate::error::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Configuration for sliding windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlidingWindowConfig {
    /// Window size
    pub size: Duration,

    /// Slide interval
    pub slide: Duration,

    /// Window offset (for alignment)
    pub offset: Duration,
}

impl SlidingWindowConfig {
    /// Create a new sliding window configuration.
    pub fn new(size: Duration, slide: Duration) -> Self {
        Self {
            size,
            slide,
            offset: Duration::zero(),
        }
    }

    /// Set the window offset.
    pub fn with_offset(mut self, offset: Duration) -> Self {
        self.offset = offset;
        self
    }
}

/// Sliding window (fixed-size, overlapping windows).
#[derive(Debug, Clone)]
pub struct SlidingWindow {
    config: SlidingWindowConfig,
}

impl SlidingWindow {
    /// Create a new sliding window.
    pub fn new(size: Duration, slide: Duration) -> Self {
        Self {
            config: SlidingWindowConfig::new(size, slide),
        }
    }

    /// Create a new sliding window with offset.
    pub fn with_offset(size: Duration, slide: Duration, offset: Duration) -> Self {
        Self {
            config: SlidingWindowConfig::new(size, slide).with_offset(offset),
        }
    }

    /// Get all windows for a given timestamp.
    pub fn get_windows(&self, timestamp: DateTime<Utc>) -> Result<Vec<Window>> {
        let size_ms = self.config.size.num_milliseconds();
        let slide_ms = self.config.slide.num_milliseconds();
        let offset_ms = self.config.offset.num_milliseconds();

        let timestamp_ms = timestamp.timestamp_millis();
        let adjusted_timestamp = timestamp_ms - offset_ms;

        let mut windows = Vec::new();

        let last_start = (adjusted_timestamp / slide_ms) * slide_ms + offset_ms;

        let num_windows = (size_ms + slide_ms - 1) / slide_ms;

        for i in (0..num_windows).rev() {
            let window_start_ms = last_start - i * slide_ms;
            let window_end_ms = window_start_ms + size_ms;

            if window_end_ms > timestamp_ms {
                let start = DateTime::from_timestamp_millis(window_start_ms).ok_or_else(|| {
                    crate::error::StreamingError::InvalidWindow(
                        "Invalid window start timestamp".to_string(),
                    )
                })?;

                let end = DateTime::from_timestamp_millis(window_end_ms).ok_or_else(|| {
                    crate::error::StreamingError::InvalidWindow(
                        "Invalid window end timestamp".to_string(),
                    )
                })?;

                if timestamp >= start && timestamp < end {
                    windows.push(Window::new(start, end)?);
                }
            }
        }

        Ok(windows)
    }
}

/// Assigner for sliding windows.
pub struct SlidingAssigner {
    window: SlidingWindow,
}

impl SlidingAssigner {
    /// Create a new sliding window assigner.
    pub fn new(size: Duration, slide: Duration) -> Self {
        Self {
            window: SlidingWindow::new(size, slide),
        }
    }

    /// Create a new sliding window assigner with offset.
    pub fn with_offset(size: Duration, slide: Duration, offset: Duration) -> Self {
        Self {
            window: SlidingWindow::with_offset(size, slide, offset),
        }
    }
}

impl WindowAssigner for SlidingAssigner {
    fn assign_windows(&self, element: &StreamElement) -> Result<Vec<Window>> {
        self.window.get_windows(element.event_time)
    }

    fn assigner_type(&self) -> &str {
        "SlidingAssigner"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliding_window() {
        let window = SlidingWindow::new(Duration::seconds(60), Duration::seconds(30));
        let timestamp =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");

        let windows = window
            .get_windows(timestamp)
            .expect("Sliding window calculation should succeed in test");
        assert!(!windows.is_empty());

        for w in &windows {
            assert_eq!(w.duration(), Duration::seconds(60));
            assert!(w.contains(&timestamp));
        }
    }

    #[test]
    fn test_sliding_window_overlap() {
        let window = SlidingWindow::new(Duration::seconds(60), Duration::seconds(20));
        let timestamp =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");

        let windows = window
            .get_windows(timestamp)
            .expect("Sliding window calculation should succeed in test");
        assert!(windows.len() > 1);

        for i in 0..windows.len() - 1 {
            assert!(windows[i].overlaps(&windows[i + 1]));
        }
    }

    #[test]
    fn test_sliding_assigner() {
        let assigner = SlidingAssigner::new(Duration::seconds(60), Duration::seconds(30));
        let elem = StreamElement::new(
            vec![1, 2, 3],
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed"),
        );

        let windows = assigner
            .assign_windows(&elem)
            .expect("Window assignment should succeed in test");
        assert!(!windows.is_empty());

        for w in &windows {
            assert!(w.contains(&elem.event_time));
        }
    }

    #[test]
    fn test_sliding_window_with_offset() {
        let window = SlidingWindow::with_offset(
            Duration::seconds(60),
            Duration::seconds(30),
            Duration::seconds(15),
        );
        let timestamp =
            DateTime::from_timestamp(1000, 0).expect("Test timestamp creation should succeed");

        let windows = window
            .get_windows(timestamp)
            .expect("Sliding window calculation should succeed in test");
        assert!(!windows.is_empty());
    }
}
