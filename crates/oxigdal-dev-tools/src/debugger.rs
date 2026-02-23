//! Debugging utilities
//!
//! This module provides debugging helpers for OxiGDAL development including
//! logging, tracing, and data inspection.

use crate::Result;
use colored::Colorize;
use comfy_table::{Cell, Row, Table};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Debug level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DebugLevel {
    /// Trace level
    Trace,
    /// Debug level
    Debug,
    /// Info level
    Info,
    /// Warn level
    Warn,
    /// Error level
    Error,
}

impl fmt::Display for DebugLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trace => write!(f, "TRACE"),
            Self::Debug => write!(f, "DEBUG"),
            Self::Info => write!(f, "INFO "),
            Self::Warn => write!(f, "WARN "),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Debug message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugMessage {
    /// Message level
    pub level: DebugLevel,
    /// Message text
    pub message: String,
    /// Source location
    pub location: Option<String>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Additional context
    pub context: HashMap<String, String>,
}

impl DebugMessage {
    /// Create a new debug message
    pub fn new(level: DebugLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            location: None,
            timestamp: chrono::Utc::now(),
            context: HashMap::new(),
        }
    }

    /// Set location
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Add context
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Format as colored string
    pub fn format_colored(&self) -> String {
        let level_str = match self.level {
            DebugLevel::Trace => format!("{}", self.level).dimmed(),
            DebugLevel::Debug => format!("{}", self.level).cyan(),
            DebugLevel::Info => format!("{}", self.level).green(),
            DebugLevel::Warn => format!("{}", self.level).yellow(),
            DebugLevel::Error => format!("{}", self.level).red().bold(),
        };

        let time = self.timestamp.format("%H:%M:%S%.3f");
        let location = self
            .location
            .as_ref()
            .map(|l| format!(" [{}]", l))
            .unwrap_or_default();

        format!("{} {} {}{}", time, level_str, self.message, location)
    }
}

/// Debugger for collecting and analyzing debug information
pub struct Debugger {
    /// Debug messages
    messages: Vec<DebugMessage>,
    /// Current debug level filter
    level_filter: DebugLevel,
    /// Maximum messages to store
    max_messages: usize,
}

impl Debugger {
    /// Create a new debugger
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            level_filter: DebugLevel::Debug,
            max_messages: 1000,
        }
    }

    /// Set level filter
    pub fn set_level_filter(&mut self, level: DebugLevel) {
        self.level_filter = level;
    }

    /// Set maximum messages
    pub fn set_max_messages(&mut self, max: usize) {
        self.max_messages = max;
        if self.messages.len() > max {
            self.messages.drain(0..(self.messages.len() - max));
        }
    }

    /// Log a message
    pub fn log(&mut self, message: DebugMessage) {
        if message.level >= self.level_filter {
            self.messages.push(message);

            // Trim if exceeds max
            if self.messages.len() > self.max_messages {
                self.messages.remove(0);
            }
        }
    }

    /// Log trace message
    pub fn trace(&mut self, message: impl Into<String>) {
        self.log(DebugMessage::new(DebugLevel::Trace, message));
    }

    /// Log debug message
    pub fn debug(&mut self, message: impl Into<String>) {
        self.log(DebugMessage::new(DebugLevel::Debug, message));
    }

    /// Log info message
    pub fn info(&mut self, message: impl Into<String>) {
        self.log(DebugMessage::new(DebugLevel::Info, message));
    }

    /// Log warn message
    pub fn warn(&mut self, message: impl Into<String>) {
        self.log(DebugMessage::new(DebugLevel::Warn, message));
    }

    /// Log error message
    pub fn error(&mut self, message: impl Into<String>) {
        self.log(DebugMessage::new(DebugLevel::Error, message));
    }

    /// Get all messages
    pub fn messages(&self) -> &[DebugMessage] {
        &self.messages
    }

    /// Get messages by level
    pub fn messages_by_level(&self, level: DebugLevel) -> Vec<&DebugMessage> {
        self.messages.iter().filter(|m| m.level == level).collect()
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Generate report
    pub fn report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("\n{}\n", "Debug Report".bold()));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        if self.messages.is_empty() {
            report.push_str("No debug messages\n");
            return report;
        }

        // Count by level
        let mut counts = HashMap::new();
        for msg in &self.messages {
            *counts.entry(msg.level).or_insert(0) += 1;
        }

        report.push_str(&format!("{}\n", "Message counts:".bold()));
        for level in [
            DebugLevel::Trace,
            DebugLevel::Debug,
            DebugLevel::Info,
            DebugLevel::Warn,
            DebugLevel::Error,
        ] {
            let count = counts.get(&level).unwrap_or(&0);
            report.push_str(&format!("  {}: {}\n", level, count));
        }

        report.push_str(&format!("\n{}\n", "Recent messages:".bold()));
        let recent_count = self.messages.len().min(20);
        for msg in &self.messages[self.messages.len() - recent_count..] {
            report.push_str(&format!("  {}\n", msg.format_colored()));
        }

        report
    }

    /// Export as JSON
    pub fn export_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.messages)?)
    }
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

/// Data inspector for examining runtime data
pub struct DataInspector;

impl DataInspector {
    /// Inspect array data
    pub fn inspect_array(data: &[f64], name: &str) -> String {
        let mut report = String::new();
        report.push_str(&format!(
            "\n{}\n",
            format!("Array Inspection: {}", name).bold()
        ));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        if data.is_empty() {
            report.push_str("Empty array\n");
            return report;
        }

        let len = data.len();
        let (min, max) = data
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &v| {
                (min.min(v), max.max(v))
            });
        let sum: f64 = data.iter().sum();
        let mean = sum / len as f64;

        let variance = data.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / len as f64;
        let std_dev = variance.sqrt();

        let mut table = Table::new();
        table.add_row(Row::from(vec![
            Cell::new("Length"),
            Cell::new(format!("{}", len)),
        ]));
        table.add_row(Row::from(vec![
            Cell::new("Min"),
            Cell::new(format!("{:.6}", min)),
        ]));
        table.add_row(Row::from(vec![
            Cell::new("Max"),
            Cell::new(format!("{:.6}", max)),
        ]));
        table.add_row(Row::from(vec![
            Cell::new("Mean"),
            Cell::new(format!("{:.6}", mean)),
        ]));
        table.add_row(Row::from(vec![
            Cell::new("Std Dev"),
            Cell::new(format!("{:.6}", std_dev)),
        ]));

        report.push_str(&table.to_string());
        report.push('\n');

        // Show sample values
        report.push_str(&format!("\n{}\n", "Sample values:".bold()));
        let sample_count = len.min(10);
        for (i, &v) in data.iter().take(sample_count).enumerate() {
            report.push_str(&format!("  [{}] = {:.6}\n", i, v));
        }

        if len > sample_count {
            report.push_str(&format!("  ... ({} more values)\n", len - sample_count));
        }

        report
    }

    /// Inspect map/dictionary data
    pub fn inspect_map(data: &HashMap<String, String>, name: &str) -> String {
        let mut report = String::new();
        report.push_str(&format!(
            "\n{}\n",
            format!("Map Inspection: {}", name).bold()
        ));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        if data.is_empty() {
            report.push_str("Empty map\n");
            return report;
        }

        report.push_str(&format!("Entries: {}\n\n", data.len()));

        let mut table = Table::new();
        table.set_header(Row::from(vec![Cell::new("Key"), Cell::new("Value")]));

        let mut keys: Vec<_> = data.keys().collect();
        keys.sort();

        for key in keys {
            if let Some(value) = data.get(key) {
                let display_value = if value.len() > 50 {
                    format!("{}...", &value[..50])
                } else {
                    value.clone()
                };
                table.add_row(Row::from(vec![Cell::new(key), Cell::new(display_value)]));
            }
        }

        report.push_str(&table.to_string());
        report.push('\n');

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_level_ordering() {
        assert!(DebugLevel::Trace < DebugLevel::Debug);
        assert!(DebugLevel::Debug < DebugLevel::Info);
        assert!(DebugLevel::Info < DebugLevel::Warn);
        assert!(DebugLevel::Warn < DebugLevel::Error);
    }

    #[test]
    fn test_debug_message_creation() {
        let msg = DebugMessage::new(DebugLevel::Info, "test message");
        assert_eq!(msg.level, DebugLevel::Info);
        assert_eq!(msg.message, "test message");
    }

    #[test]
    fn test_debugger_logging() {
        let mut debugger = Debugger::new();
        debugger.info("info message");
        debugger.warn("warn message");

        assert_eq!(debugger.messages().len(), 2);
    }

    #[test]
    fn test_debugger_level_filter() {
        let mut debugger = Debugger::new();
        debugger.set_level_filter(DebugLevel::Warn);

        debugger.debug("debug message");
        debugger.warn("warn message");
        debugger.error("error message");

        // Only warn and error should be logged
        assert_eq!(debugger.messages().len(), 2);
    }

    #[test]
    fn test_debugger_max_messages() {
        let mut debugger = Debugger::new();
        debugger.set_max_messages(5);

        for i in 0..10 {
            debugger.info(format!("message {}", i));
        }

        assert_eq!(debugger.messages().len(), 5);
    }

    #[test]
    fn test_debugger_messages_by_level() {
        let mut debugger = Debugger::new();
        debugger.info("info1");
        debugger.warn("warn1");
        debugger.info("info2");

        let info_msgs = debugger.messages_by_level(DebugLevel::Info);
        assert_eq!(info_msgs.len(), 2);
    }

    #[test]
    fn test_data_inspector_array() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let report = DataInspector::inspect_array(&data, "test");
        assert!(report.contains("Array Inspection"));
        assert!(report.contains("Length"));
    }

    #[test]
    fn test_data_inspector_empty_array() {
        let data: Vec<f64> = vec![];
        let report = DataInspector::inspect_array(&data, "empty");
        assert!(report.contains("Empty array"));
    }

    #[test]
    fn test_data_inspector_map() {
        let mut data = HashMap::new();
        data.insert("key1".to_string(), "value1".to_string());
        data.insert("key2".to_string(), "value2".to_string());

        let report = DataInspector::inspect_map(&data, "test");
        assert!(report.contains("Map Inspection"));
        assert!(report.contains("key1"));
    }
}
