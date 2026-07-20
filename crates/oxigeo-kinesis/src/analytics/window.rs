//! Window types for Kinesis Analytics

use serde::{Deserialize, Serialize};
use std::fmt;

/// Window type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowType {
    /// Tumbling window (fixed size, non-overlapping)
    Tumbling(TumblingWindow),
    /// Sliding window (fixed size, overlapping)
    Sliding(SlidingWindow),
    /// Session window (gap-based)
    Session(SessionWindow),
}

impl WindowType {
    /// Converts to SQL window clause
    pub fn to_sql(&self, stream_name: &str) -> String {
        match self {
            Self::Tumbling(w) => w.to_sql(stream_name),
            Self::Sliding(w) => w.to_sql(stream_name),
            Self::Session(w) => w.to_sql(stream_name),
        }
    }
}

impl fmt::Display for WindowType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tumbling(w) => write!(f, "Tumbling({})", w),
            Self::Sliding(w) => write!(f, "Sliding({})", w),
            Self::Session(w) => write!(f, "Session({})", w),
        }
    }
}

/// Tumbling window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TumblingWindow {
    /// Window size in seconds
    pub size_seconds: i64,
    /// Partition by columns
    pub partition_by: Vec<String>,
}

impl TumblingWindow {
    /// Creates a new tumbling window
    pub fn new(size_seconds: i64) -> Self {
        Self {
            size_seconds,
            partition_by: Vec::new(),
        }
    }

    /// Adds a partition by column
    pub fn partition_by(mut self, column: impl Into<String>) -> Self {
        self.partition_by.push(column.into());
        self
    }

    /// Converts to SQL window clause
    pub fn to_sql(&self, _stream_name: &str) -> String {
        let mut sql = format!("WINDOW TUMBLING (SIZE {} SECOND", self.size_seconds);

        if !self.partition_by.is_empty() {
            sql.push_str(", PARTITION BY ");
            sql.push_str(&self.partition_by.join(", "));
        }

        sql.push(')');
        sql
    }
}

impl fmt::Display for TumblingWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "size={}s", self.size_seconds)?;
        if !self.partition_by.is_empty() {
            write!(f, ", partition_by={}", self.partition_by.join(","))?;
        }
        Ok(())
    }
}

/// Sliding window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlidingWindow {
    /// Window size in seconds
    pub size_seconds: i64,
    /// Slide interval in seconds
    pub slide_seconds: i64,
    /// Partition by columns
    pub partition_by: Vec<String>,
}

impl SlidingWindow {
    /// Creates a new sliding window
    pub fn new(size_seconds: i64, slide_seconds: i64) -> Self {
        Self {
            size_seconds,
            slide_seconds,
            partition_by: Vec::new(),
        }
    }

    /// Adds a partition by column
    pub fn partition_by(mut self, column: impl Into<String>) -> Self {
        self.partition_by.push(column.into());
        self
    }

    /// Converts to SQL window clause
    pub fn to_sql(&self, _stream_name: &str) -> String {
        let mut sql = format!(
            "WINDOW SLIDING (SIZE {} SECOND, ADVANCE BY {} SECOND",
            self.size_seconds, self.slide_seconds
        );

        if !self.partition_by.is_empty() {
            sql.push_str(", PARTITION BY ");
            sql.push_str(&self.partition_by.join(", "));
        }

        sql.push(')');
        sql
    }
}

impl fmt::Display for SlidingWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "size={}s, slide={}s",
            self.size_seconds, self.slide_seconds
        )?;
        if !self.partition_by.is_empty() {
            write!(f, ", partition_by={}", self.partition_by.join(","))?;
        }
        Ok(())
    }
}

/// Session window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionWindow {
    /// Gap duration in seconds
    pub gap_seconds: i64,
    /// Partition by columns
    pub partition_by: Vec<String>,
}

impl SessionWindow {
    /// Creates a new session window
    pub fn new(gap_seconds: i64) -> Self {
        Self {
            gap_seconds,
            partition_by: Vec::new(),
        }
    }

    /// Adds a partition by column
    pub fn partition_by(mut self, column: impl Into<String>) -> Self {
        self.partition_by.push(column.into());
        self
    }

    /// Converts to SQL window clause
    pub fn to_sql(&self, _stream_name: &str) -> String {
        let mut sql = format!("WINDOW SESSION (GAP {} SECOND", self.gap_seconds);

        if !self.partition_by.is_empty() {
            sql.push_str(", PARTITION BY ");
            sql.push_str(&self.partition_by.join(", "));
        }

        sql.push(')');
        sql
    }
}

impl fmt::Display for SessionWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gap={}s", self.gap_seconds)?;
        if !self.partition_by.is_empty() {
            write!(f, ", partition_by={}", self.partition_by.join(","))?;
        }
        Ok(())
    }
}

/// Window builder for common window patterns
pub struct WindowBuilder;

impl WindowBuilder {
    /// Creates a tumbling window of 1 minute
    pub fn tumbling_1min() -> WindowType {
        WindowType::Tumbling(TumblingWindow::new(60))
    }

    /// Creates a tumbling window of 5 minutes
    pub fn tumbling_5min() -> WindowType {
        WindowType::Tumbling(TumblingWindow::new(300))
    }

    /// Creates a tumbling window of 1 hour
    pub fn tumbling_1hour() -> WindowType {
        WindowType::Tumbling(TumblingWindow::new(3600))
    }

    /// Creates a sliding window of 5 minutes with 1 minute slide
    pub fn sliding_5min_1min() -> WindowType {
        WindowType::Sliding(SlidingWindow::new(300, 60))
    }

    /// Creates a sliding window of 1 hour with 5 minute slide
    pub fn sliding_1hour_5min() -> WindowType {
        WindowType::Sliding(SlidingWindow::new(3600, 300))
    }

    /// Creates a session window with 5 minute gap
    pub fn session_5min() -> WindowType {
        WindowType::Session(SessionWindow::new(300))
    }

    /// Creates a session window with 30 minute gap
    pub fn session_30min() -> WindowType {
        WindowType::Session(SessionWindow::new(1800))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tumbling_window() {
        let window = TumblingWindow::new(60);
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("TUMBLING"));
        assert!(sql.contains("SIZE 60 SECOND"));
    }

    #[test]
    fn test_tumbling_window_with_partition() {
        let window = TumblingWindow::new(60).partition_by("userId");
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("PARTITION BY userId"));
    }

    #[test]
    fn test_sliding_window() {
        let window = SlidingWindow::new(300, 60);
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("SLIDING"));
        assert!(sql.contains("SIZE 300 SECOND"));
        assert!(sql.contains("ADVANCE BY 60 SECOND"));
    }

    #[test]
    fn test_sliding_window_with_partition() {
        let window = SlidingWindow::new(300, 60)
            .partition_by("sensorId")
            .partition_by("location");
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("PARTITION BY sensorId, location"));
    }

    #[test]
    fn test_session_window() {
        let window = SessionWindow::new(300);
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("SESSION"));
        assert!(sql.contains("GAP 300 SECOND"));
    }

    #[test]
    fn test_session_window_with_partition() {
        let window = SessionWindow::new(300).partition_by("userId");
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("PARTITION BY userId"));
    }

    #[test]
    fn test_window_builder() {
        let w1 = WindowBuilder::tumbling_1min();
        assert!(matches!(w1, WindowType::Tumbling(_)));

        let w2 = WindowBuilder::sliding_5min_1min();
        assert!(matches!(w2, WindowType::Sliding(_)));

        let w3 = WindowBuilder::session_5min();
        assert!(matches!(w3, WindowType::Session(_)));
    }

    #[test]
    fn test_window_display() {
        let tumbling = TumblingWindow::new(60);
        assert_eq!(format!("{}", tumbling), "size=60s");

        let sliding = SlidingWindow::new(300, 60);
        assert_eq!(format!("{}", sliding), "size=300s, slide=60s");

        let session = SessionWindow::new(300);
        assert_eq!(format!("{}", session), "gap=300s");
    }

    #[test]
    fn test_window_type_to_sql() {
        let window = WindowType::Tumbling(TumblingWindow::new(60));
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("TUMBLING"));

        let window = WindowType::Sliding(SlidingWindow::new(300, 60));
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("SLIDING"));

        let window = WindowType::Session(SessionWindow::new(300));
        let sql = window.to_sql("INPUT_STREAM");
        assert!(sql.contains("SESSION"));
    }
}
