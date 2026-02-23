//! Workflow logging system.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Log level enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogLevel {
    /// Trace level (most verbose).
    Trace,
    /// Debug level.
    Debug,
    /// Info level.
    Info,
    /// Warning level.
    Warn,
    /// Error level.
    Error,
    /// Fatal level (least verbose).
    Fatal,
}

impl LogLevel {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

/// Log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Log timestamp.
    pub timestamp: DateTime<Utc>,
    /// Log level.
    pub level: LogLevel,
    /// Workflow ID.
    pub workflow_id: String,
    /// Task ID (if applicable).
    pub task_id: Option<String>,
    /// Log message.
    pub message: String,
    /// Additional context fields.
    pub context: std::collections::HashMap<String, String>,
}

impl LogEntry {
    /// Create a new log entry.
    pub fn new<S1: Into<String>, S2: Into<String>>(
        level: LogLevel,
        workflow_id: S1,
        message: S2,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            workflow_id: workflow_id.into(),
            task_id: None,
            message: message.into(),
            context: std::collections::HashMap::new(),
        }
    }

    /// Set the task ID.
    pub fn with_task_id<S: Into<String>>(mut self, task_id: S) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Add context field.
    pub fn with_context<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Format the log entry as a string.
    pub fn format(&self) -> String {
        let task_info = self
            .task_id
            .as_ref()
            .map(|id| format!(" [task:{}]", id))
            .unwrap_or_default();

        let context_info = if self.context.is_empty() {
            String::new()
        } else {
            let mut parts: Vec<String> = self
                .context
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            parts.sort();
            format!(" {{{}}}", parts.join(", "))
        };

        format!(
            "[{}] {} [workflow:{}]{}{} {}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
            self.level.as_str(),
            self.workflow_id,
            task_info,
            context_info,
            self.message
        )
    }
}

/// Workflow logger.
pub struct WorkflowLogger {
    logs: Arc<DashMap<String, Arc<RwLock<VecDeque<LogEntry>>>>>,
    max_logs_per_workflow: usize,
    min_level: LogLevel,
}

impl WorkflowLogger {
    /// Create a new workflow logger.
    pub fn new() -> Self {
        Self {
            logs: Arc::new(DashMap::new()),
            max_logs_per_workflow: 10000,
            min_level: LogLevel::Info,
        }
    }

    /// Create a logger with custom configuration.
    pub fn with_config(max_logs_per_workflow: usize, min_level: LogLevel) -> Self {
        Self {
            logs: Arc::new(DashMap::new()),
            max_logs_per_workflow,
            min_level,
        }
    }

    /// Set the minimum log level.
    pub fn set_min_level(&mut self, level: LogLevel) {
        self.min_level = level;
    }

    /// Log a message.
    pub async fn log(&self, entry: LogEntry) {
        if entry.level < self.min_level {
            return;
        }

        let workflow_id = entry.workflow_id.clone();
        let logs = self
            .logs
            .entry(workflow_id)
            .or_insert_with(|| Arc::new(RwLock::new(VecDeque::new())));

        let mut log_queue = logs.write().await;

        // Maintain max size
        if log_queue.len() >= self.max_logs_per_workflow {
            log_queue.pop_front();
        }

        log_queue.push_back(entry);
    }

    /// Log a trace message.
    pub async fn trace<S1: Into<String>, S2: Into<String>>(&self, workflow_id: S1, message: S2) {
        self.log(LogEntry::new(
            LogLevel::Trace,
            workflow_id.into(),
            message.into(),
        ))
        .await;
    }

    /// Log a debug message.
    pub async fn debug<S1: Into<String>, S2: Into<String>>(&self, workflow_id: S1, message: S2) {
        self.log(LogEntry::new(
            LogLevel::Debug,
            workflow_id.into(),
            message.into(),
        ))
        .await;
    }

    /// Log an info message.
    pub async fn info<S1: Into<String>, S2: Into<String>>(&self, workflow_id: S1, message: S2) {
        self.log(LogEntry::new(
            LogLevel::Info,
            workflow_id.into(),
            message.into(),
        ))
        .await;
    }

    /// Log a warning message.
    pub async fn warn<S1: Into<String>, S2: Into<String>>(&self, workflow_id: S1, message: S2) {
        self.log(LogEntry::new(
            LogLevel::Warn,
            workflow_id.into(),
            message.into(),
        ))
        .await;
    }

    /// Log an error message.
    pub async fn error<S1: Into<String>, S2: Into<String>>(&self, workflow_id: S1, message: S2) {
        self.log(LogEntry::new(
            LogLevel::Error,
            workflow_id.into(),
            message.into(),
        ))
        .await;
    }

    /// Log a fatal message.
    pub async fn fatal<S1: Into<String>, S2: Into<String>>(&self, workflow_id: S1, message: S2) {
        self.log(LogEntry::new(
            LogLevel::Fatal,
            workflow_id.into(),
            message.into(),
        ))
        .await;
    }

    /// Get logs for a workflow.
    pub async fn get_logs(&self, workflow_id: &str) -> Vec<LogEntry> {
        if let Some(logs) = self.logs.get(workflow_id) {
            let log_queue = logs.read().await;
            log_queue.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Get logs for a workflow with filtering.
    pub async fn get_logs_filtered(
        &self,
        workflow_id: &str,
        min_level: LogLevel,
        limit: Option<usize>,
    ) -> Vec<LogEntry> {
        if let Some(logs) = self.logs.get(workflow_id) {
            let log_queue = logs.read().await;
            let mut filtered: Vec<LogEntry> = log_queue
                .iter()
                .filter(|entry| entry.level >= min_level)
                .cloned()
                .collect();

            if let Some(limit_count) = limit {
                let start = if filtered.len() > limit_count {
                    filtered.len() - limit_count
                } else {
                    0
                };
                filtered = filtered[start..].to_vec();
            }

            filtered
        } else {
            Vec::new()
        }
    }

    /// Get logs for a specific task.
    pub async fn get_task_logs(&self, workflow_id: &str, task_id: &str) -> Vec<LogEntry> {
        if let Some(logs) = self.logs.get(workflow_id) {
            let log_queue = logs.read().await;
            log_queue
                .iter()
                .filter(|entry| entry.task_id.as_deref() == Some(task_id))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Clear logs for a workflow.
    pub async fn clear_logs(&self, workflow_id: &str) {
        if let Some(logs) = self.logs.get(workflow_id) {
            let mut log_queue = logs.write().await;
            log_queue.clear();
        }
    }

    /// Clear all logs.
    pub fn clear_all_logs(&self) {
        self.logs.clear();
    }

    /// Get log count for a workflow.
    pub async fn get_log_count(&self, workflow_id: &str) -> usize {
        if let Some(logs) = self.logs.get(workflow_id) {
            let log_queue = logs.read().await;
            log_queue.len()
        } else {
            0
        }
    }

    /// Get total log count across all workflows.
    pub async fn get_total_log_count(&self) -> usize {
        let mut total = 0;
        for entry in self.logs.iter() {
            let log_queue = entry.value().read().await;
            total += log_queue.len();
        }
        total
    }

    /// Export logs to JSON.
    pub async fn export_logs_json(&self, workflow_id: &str) -> Result<String, serde_json::Error> {
        let logs = self.get_logs(workflow_id).await;
        serde_json::to_string_pretty(&logs)
    }
}

impl Default for WorkflowLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_logger_creation() {
        let logger = WorkflowLogger::new();
        assert_eq!(logger.get_log_count("workflow1").await, 0);
    }

    #[tokio::test]
    async fn test_logging() {
        let logger = WorkflowLogger::new();

        logger.info("workflow1", "Test message").await;

        let logs = logger.get_logs("workflow1").await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "Test message");
    }

    #[tokio::test]
    async fn test_log_levels() {
        let logger = WorkflowLogger::with_config(100, LogLevel::Warn);

        logger.info("workflow1", "Info message").await;
        logger.warn("workflow1", "Warning message").await;
        logger.error("workflow1", "Error message").await;

        let logs = logger.get_logs("workflow1").await;
        // Only warn and error should be logged
        assert_eq!(logs.len(), 2);
    }

    #[tokio::test]
    async fn test_log_filtering() {
        let logger = WorkflowLogger::new();

        logger.info("workflow1", "Info").await;
        logger.warn("workflow1", "Warning").await;
        logger.error("workflow1", "Error").await;

        let filtered = logger
            .get_logs_filtered("workflow1", LogLevel::Warn, None)
            .await;

        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn test_task_logs() {
        let logger = WorkflowLogger::new();

        let entry =
            LogEntry::new(LogLevel::Info, "workflow1", "Task message").with_task_id("task1");

        logger.log(entry).await;

        let task_logs = logger.get_task_logs("workflow1", "task1").await;
        assert_eq!(task_logs.len(), 1);
    }

    #[test]
    fn test_log_entry_format() {
        let entry = LogEntry::new(LogLevel::Info, "workflow1", "Test message")
            .with_task_id("task1")
            .with_context("key", "value");

        let formatted = entry.format();
        assert!(formatted.contains("INFO"));
        assert!(formatted.contains("workflow1"));
        assert!(formatted.contains("task1"));
        assert!(formatted.contains("Test message"));
    }
}
