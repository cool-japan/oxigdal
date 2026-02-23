//! Core window types and traits.

use crate::core::stream::StreamElement;
use crate::error::{Result, StreamingError};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A window of time for grouping events.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Window {
    /// Window start time (inclusive)
    pub start: DateTime<Utc>,

    /// Window end time (exclusive)
    pub end: DateTime<Utc>,
}

impl Window {
    /// Create a new window.
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self> {
        if start >= end {
            return Err(StreamingError::InvalidWindow(
                "Window start must be before end".to_string(),
            ));
        }
        Ok(Self { start, end })
    }

    /// Get the window duration.
    pub fn duration(&self) -> Duration {
        self.end - self.start
    }

    /// Check if a timestamp falls within this window.
    pub fn contains(&self, timestamp: &DateTime<Utc>) -> bool {
        timestamp >= &self.start && timestamp < &self.end
    }

    /// Check if this window overlaps with another.
    pub fn overlaps(&self, other: &Window) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Merge this window with another (if they overlap).
    pub fn merge(&self, other: &Window) -> Option<Window> {
        if self.overlaps(other) {
            let start = self.start.min(other.start);
            let end = self.end.max(other.end);
            Window::new(start, end).ok()
        } else {
            None
        }
    }

    /// Get the maximum timestamp in this window.
    pub fn max_timestamp(&self) -> DateTime<Utc> {
        self.end - Duration::milliseconds(1)
    }
}

/// Assigns elements to windows.
pub trait WindowAssigner: Send + Sync {
    /// Assign an element to one or more windows.
    fn assign_windows(&self, element: &StreamElement) -> Result<Vec<Window>>;

    /// Get the window assigner type name.
    fn assigner_type(&self) -> &str;
}

/// Result of evaluating a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerResult {
    /// Continue accumulating elements
    Continue,

    /// Fire the window (emit results)
    Fire,

    /// Fire and purge the window
    FireAndPurge,

    /// Purge the window without firing
    Purge,
}

/// Determines when a window should emit results.
pub trait WindowTrigger: Send + Sync {
    /// Called when an element is added to a window.
    fn on_element(
        &mut self,
        element: &StreamElement,
        window: &Window,
        state: &WindowState,
    ) -> TriggerResult;

    /// Called when processing time advances.
    fn on_processing_time(&mut self, time: DateTime<Utc>, window: &Window) -> TriggerResult;

    /// Called when event time (watermark) advances.
    fn on_event_time(&mut self, time: DateTime<Utc>, window: &Window) -> TriggerResult;

    /// Called when windows are merged.
    fn on_merge(&mut self, window: &Window, merged_windows: &[Window]) -> TriggerResult;

    /// Clear the trigger state.
    fn clear(&mut self);
}

/// State associated with a window.
#[derive(Debug, Clone)]
pub struct WindowState {
    /// Number of elements in the window
    pub element_count: usize,

    /// Total size in bytes
    pub size_bytes: usize,

    /// Earliest element timestamp
    pub earliest_timestamp: Option<DateTime<Utc>>,

    /// Latest element timestamp
    pub latest_timestamp: Option<DateTime<Utc>>,

    /// Custom state
    pub custom: HashMap<String, Vec<u8>>,
}

impl WindowState {
    /// Create a new empty window state.
    pub fn new() -> Self {
        Self {
            element_count: 0,
            size_bytes: 0,
            earliest_timestamp: None,
            latest_timestamp: None,
            custom: HashMap::new(),
        }
    }

    /// Update state with a new element.
    pub fn add_element(&mut self, element: &StreamElement) {
        self.element_count += 1;
        self.size_bytes += element.size_bytes();

        if let Some(earliest) = self.earliest_timestamp {
            if element.event_time < earliest {
                self.earliest_timestamp = Some(element.event_time);
            }
        } else {
            self.earliest_timestamp = Some(element.event_time);
        }

        if let Some(latest) = self.latest_timestamp {
            if element.event_time > latest {
                self.latest_timestamp = Some(element.event_time);
            }
        } else {
            self.latest_timestamp = Some(element.event_time);
        }
    }

    /// Clear the state.
    pub fn clear(&mut self) {
        self.element_count = 0;
        self.size_bytes = 0;
        self.earliest_timestamp = None;
        self.latest_timestamp = None;
        self.custom.clear();
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self::new()
    }
}

/// Event-time session windows.
pub struct EventTimeSessionWindows {
    gap: Duration,
}

impl EventTimeSessionWindows {
    /// Create a new event-time session windows assigner.
    pub fn with_gap(gap: Duration) -> Self {
        Self { gap }
    }
}

impl WindowAssigner for EventTimeSessionWindows {
    fn assign_windows(&self, element: &StreamElement) -> Result<Vec<Window>> {
        let start = element.event_time;
        let end = start + self.gap;
        Ok(vec![Window::new(start, end)?])
    }

    fn assigner_type(&self) -> &str {
        "EventTimeSessionWindows"
    }
}

/// Processing-time session windows.
pub struct ProcessingTimeSessionWindows {
    gap: Duration,
}

impl ProcessingTimeSessionWindows {
    /// Create a new processing-time session windows assigner.
    pub fn with_gap(gap: Duration) -> Self {
        Self { gap }
    }
}

impl WindowAssigner for ProcessingTimeSessionWindows {
    fn assign_windows(&self, element: &StreamElement) -> Result<Vec<Window>> {
        let start = element.processing_time;
        let end = start + self.gap;
        Ok(vec![Window::new(start, end)?])
    }

    fn assigner_type(&self) -> &str {
        "ProcessingTimeSessionWindows"
    }
}

/// Event-time trigger that fires when watermark passes window end.
pub struct EventTimeTrigger {
    fired_windows: Vec<Window>,
}

impl EventTimeTrigger {
    /// Create a new event-time trigger.
    pub fn new() -> Self {
        Self {
            fired_windows: Vec::new(),
        }
    }
}

impl Default for EventTimeTrigger {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowTrigger for EventTimeTrigger {
    fn on_element(
        &mut self,
        _element: &StreamElement,
        _window: &Window,
        _state: &WindowState,
    ) -> TriggerResult {
        TriggerResult::Continue
    }

    fn on_processing_time(&mut self, _time: DateTime<Utc>, _window: &Window) -> TriggerResult {
        TriggerResult::Continue
    }

    fn on_event_time(&mut self, time: DateTime<Utc>, window: &Window) -> TriggerResult {
        if time >= window.end {
            self.fired_windows.push(window.clone());
            TriggerResult::FireAndPurge
        } else {
            TriggerResult::Continue
        }
    }

    fn on_merge(&mut self, _window: &Window, _merged_windows: &[Window]) -> TriggerResult {
        TriggerResult::Continue
    }

    fn clear(&mut self) {
        self.fired_windows.clear();
    }
}

/// Count-based trigger that fires after a certain number of elements.
pub struct CountTrigger {
    count: usize,
}

impl CountTrigger {
    /// Create a new count trigger.
    pub fn of(count: usize) -> Self {
        Self { count }
    }
}

impl WindowTrigger for CountTrigger {
    fn on_element(
        &mut self,
        _element: &StreamElement,
        _window: &Window,
        state: &WindowState,
    ) -> TriggerResult {
        if state.element_count >= self.count {
            TriggerResult::FireAndPurge
        } else {
            TriggerResult::Continue
        }
    }

    fn on_processing_time(&mut self, _time: DateTime<Utc>, _window: &Window) -> TriggerResult {
        TriggerResult::Continue
    }

    fn on_event_time(&mut self, _time: DateTime<Utc>, _window: &Window) -> TriggerResult {
        TriggerResult::Continue
    }

    fn on_merge(&mut self, _window: &Window, _merged_windows: &[Window]) -> TriggerResult {
        TriggerResult::Continue
    }

    fn clear(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_creation() {
        let start = Utc::now();
        let end = start + Duration::seconds(60);

        let window = Window::new(start, end).expect("Window creation for test should succeed");
        assert_eq!(window.start, start);
        assert_eq!(window.end, end);
        assert_eq!(window.duration(), Duration::seconds(60));
    }

    #[test]
    fn test_window_contains() {
        let start = Utc::now();
        let end = start + Duration::seconds(60);
        let window =
            Window::new(start, end).expect("Window creation for contains test should succeed");

        let inside = start + Duration::seconds(30);
        let outside = end + Duration::seconds(1);

        assert!(window.contains(&inside));
        assert!(!window.contains(&outside));
    }

    #[test]
    fn test_window_overlaps() {
        let start1 = Utc::now();
        let end1 = start1 + Duration::seconds(60);
        let window1 =
            Window::new(start1, end1).expect("Window creation for overlap test should succeed");

        let start2 = start1 + Duration::seconds(30);
        let end2 = start2 + Duration::seconds(60);
        let window2 =
            Window::new(start2, end2).expect("Window creation for overlap test should succeed");

        assert!(window1.overlaps(&window2));
        assert!(window2.overlaps(&window1));
    }

    #[test]
    fn test_window_merge() {
        let start1 = Utc::now();
        let end1 = start1 + Duration::seconds(60);
        let window1 =
            Window::new(start1, end1).expect("Window creation for merge test should succeed");

        let start2 = start1 + Duration::seconds(30);
        let end2 = start2 + Duration::seconds(60);
        let window2 =
            Window::new(start2, end2).expect("Window creation for merge test should succeed");

        let merged = window1
            .merge(&window2)
            .expect("Window merge should succeed in test");
        assert_eq!(merged.start, start1);
        assert_eq!(merged.end, end2);
    }

    #[test]
    fn test_window_state() {
        let mut state = WindowState::new();
        assert_eq!(state.element_count, 0);

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        state.add_element(&elem);

        assert_eq!(state.element_count, 1);
        assert!(state.earliest_timestamp.is_some());
        assert!(state.latest_timestamp.is_some());
    }
}
