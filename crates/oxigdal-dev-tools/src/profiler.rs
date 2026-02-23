//! Performance profiling utilities
//!
//! This module provides tools for profiling OxiGDAL operations including
//! timing, memory usage, and resource consumption tracking.

use crate::{DevToolsError, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;
use comfy_table::{Cell, CellAlignment, Row, Table};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Performance profiler
pub struct Profiler {
    /// Profiler name
    name: String,
    /// Profiling sessions
    sessions: Vec<ProfileSession>,
    /// Current session
    current_session: Option<ProfileSession>,
    /// System information
    system: System,
    /// Process ID
    pid: Pid,
}

/// Profile session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSession {
    /// Session name
    pub name: String,
    /// Start time
    pub start_time: DateTime<Utc>,
    /// End time
    pub end_time: Option<DateTime<Utc>>,
    /// Duration in milliseconds
    pub duration_ms: Option<i64>,
    /// Memory at start (bytes)
    pub memory_start: u64,
    /// Memory at end (bytes)
    pub memory_end: Option<u64>,
    /// Memory delta (bytes)
    pub memory_delta: Option<i64>,
    /// CPU usage percentage
    pub cpu_usage: Option<f32>,
    /// Custom metrics
    pub metrics: HashMap<String, f64>,
}

impl Profiler {
    /// Create a new profiler
    pub fn new(name: impl Into<String>) -> Self {
        let mut system = System::new();
        system.refresh_all();
        let pid = Pid::from_u32(std::process::id());

        Self {
            name: name.into(),
            sessions: Vec::new(),
            current_session: None,
            system,
            pid,
        }
    }

    /// Start profiling
    pub fn start(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::everything(),
        );

        let memory_start = self
            .system
            .process(self.pid)
            .map(|p| p.memory())
            .unwrap_or(0);

        self.current_session = Some(ProfileSession {
            name: self.name.clone(),
            start_time: Utc::now(),
            end_time: None,
            duration_ms: None,
            memory_start,
            memory_end: None,
            memory_delta: None,
            cpu_usage: None,
            metrics: HashMap::new(),
        });
    }

    /// Stop profiling
    pub fn stop(&mut self) {
        if let Some(mut session) = self.current_session.take() {
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[self.pid]),
                true,
                ProcessRefreshKind::everything(),
            );

            let end_time = Utc::now();
            let duration = end_time.signed_duration_since(session.start_time);

            let memory_end = self
                .system
                .process(self.pid)
                .map(|p| p.memory())
                .unwrap_or(0);

            let cpu_usage = self
                .system
                .process(self.pid)
                .map(|p| p.cpu_usage())
                .unwrap_or(0.0);

            session.end_time = Some(end_time);
            session.duration_ms = Some(duration.num_milliseconds());
            session.memory_end = Some(memory_end);
            session.memory_delta = Some(memory_end as i64 - session.memory_start as i64);
            session.cpu_usage = Some(cpu_usage);

            self.sessions.push(session);
        }
    }

    /// Add a custom metric to current session
    pub fn add_metric(&mut self, name: impl Into<String>, value: f64) -> Result<()> {
        if let Some(session) = &mut self.current_session {
            session.metrics.insert(name.into(), value);
            Ok(())
        } else {
            Err(DevToolsError::Profiler("No active session".to_string()))
        }
    }

    /// Get all sessions
    pub fn sessions(&self) -> &[ProfileSession] {
        &self.sessions
    }

    /// Get current session
    pub fn current_session(&self) -> Option<&ProfileSession> {
        self.current_session.as_ref()
    }

    /// Generate report
    pub fn report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!(
            "\n{}\n",
            format!("Profile Report: {}", self.name).bold()
        ));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        if self.sessions.is_empty() {
            report.push_str("No profiling sessions recorded\n");
            return report;
        }

        let mut table = Table::new();
        table.set_header(Row::from(vec![
            Cell::new("Session").set_alignment(CellAlignment::Center),
            Cell::new("Duration (ms)").set_alignment(CellAlignment::Center),
            Cell::new("Memory Δ").set_alignment(CellAlignment::Center),
            Cell::new("CPU %").set_alignment(CellAlignment::Center),
        ]));

        for (i, session) in self.sessions.iter().enumerate() {
            let duration = session
                .duration_ms
                .map(|d| format!("{}", d))
                .unwrap_or_else(|| "N/A".to_string());

            let memory_delta = session
                .memory_delta
                .map(format_bytes)
                .unwrap_or_else(|| "N/A".to_string());

            let cpu = session
                .cpu_usage
                .map(|c| format!("{:.1}", c))
                .unwrap_or_else(|| "N/A".to_string());

            table.add_row(Row::from(vec![
                Cell::new(format!("#{}", i + 1)),
                Cell::new(duration).set_alignment(CellAlignment::Right),
                Cell::new(memory_delta).set_alignment(CellAlignment::Right),
                Cell::new(cpu).set_alignment(CellAlignment::Right),
            ]));
        }

        report.push_str(&table.to_string());
        report.push('\n');

        // Statistics
        let total_duration: i64 = self.sessions.iter().filter_map(|s| s.duration_ms).sum();
        let avg_duration = if !self.sessions.is_empty() {
            total_duration / self.sessions.len() as i64
        } else {
            0
        };

        report.push_str(&format!("\n{}\n", "Statistics:".bold()));
        report.push_str(&format!("  Total sessions: {}\n", self.sessions.len()));
        report.push_str(&format!("  Total duration: {} ms\n", total_duration));
        report.push_str(&format!("  Average duration: {} ms\n", avg_duration));

        report
    }

    /// Export sessions as JSON
    pub fn export_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.sessions)?)
    }

    /// Clear all sessions
    pub fn clear(&mut self) {
        self.sessions.clear();
        self.current_session = None;
    }
}

/// Format bytes with appropriate unit
fn format_bytes(bytes: i64) -> String {
    let abs_bytes = bytes.abs() as f64;
    let sign = if bytes < 0 { "-" } else { "+" };

    if abs_bytes < 1024.0 {
        format!("{}{} B", sign, bytes.abs())
    } else if abs_bytes < 1024.0 * 1024.0 {
        format!("{}{:.2} KB", sign, abs_bytes / 1024.0)
    } else if abs_bytes < 1024.0 * 1024.0 * 1024.0 {
        format!("{}{:.2} MB", sign, abs_bytes / (1024.0 * 1024.0))
    } else {
        format!("{}{:.2} GB", sign, abs_bytes / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Memory profiler for tracking allocations
pub struct MemoryProfiler {
    /// Snapshots
    snapshots: Vec<MemorySnapshot>,
    /// System
    system: System,
    /// Process ID
    pid: Pid,
}

/// Memory snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Snapshot name
    pub name: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Total memory (bytes)
    pub total_memory: u64,
    /// Virtual memory (bytes)
    pub virtual_memory: u64,
}

impl MemoryProfiler {
    /// Create a new memory profiler
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_all();
        let pid = Pid::from_u32(std::process::id());

        Self {
            snapshots: Vec::new(),
            system,
            pid,
        }
    }

    /// Take a snapshot
    pub fn snapshot(&mut self, name: impl Into<String>) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::everything(),
        );

        if let Some(process) = self.system.process(self.pid) {
            self.snapshots.push(MemorySnapshot {
                name: name.into(),
                timestamp: Utc::now(),
                total_memory: process.memory(),
                virtual_memory: process.virtual_memory(),
            });
        }
    }

    /// Get all snapshots
    pub fn snapshots(&self) -> &[MemorySnapshot] {
        &self.snapshots
    }

    /// Generate report
    pub fn report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("\n{}\n", "Memory Profile Report".bold()));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        if self.snapshots.is_empty() {
            report.push_str("No snapshots recorded\n");
            return report;
        }

        let mut table = Table::new();
        table.set_header(Row::from(vec![
            Cell::new("Snapshot").set_alignment(CellAlignment::Center),
            Cell::new("Total Memory").set_alignment(CellAlignment::Center),
            Cell::new("Virtual Memory").set_alignment(CellAlignment::Center),
            Cell::new("Delta").set_alignment(CellAlignment::Center),
        ]));

        for (i, snapshot) in self.snapshots.iter().enumerate() {
            let delta = if i > 0 {
                let prev = &self.snapshots[i - 1];
                let delta = snapshot.total_memory as i64 - prev.total_memory as i64;
                format_bytes(delta)
            } else {
                "N/A".to_string()
            };

            table.add_row(Row::from(vec![
                Cell::new(&snapshot.name),
                Cell::new(format_bytes(snapshot.total_memory as i64))
                    .set_alignment(CellAlignment::Right),
                Cell::new(format_bytes(snapshot.virtual_memory as i64))
                    .set_alignment(CellAlignment::Right),
                Cell::new(delta).set_alignment(CellAlignment::Right),
            ]));
        }

        report.push_str(&table.to_string());
        report.push('\n');

        report
    }

    /// Clear snapshots
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_profiler_creation() {
        let profiler = Profiler::new("test");
        assert!(profiler.sessions().is_empty());
    }

    #[test]
    fn test_profiler_session() {
        let mut profiler = Profiler::new("test");
        profiler.start();
        thread::sleep(StdDuration::from_millis(100));
        profiler.stop();

        assert_eq!(profiler.sessions().len(), 1);
        let session = &profiler.sessions()[0];
        assert!(session.duration_ms.is_some());
    }

    #[test]
    fn test_profiler_metrics() -> Result<()> {
        let mut profiler = Profiler::new("test");
        profiler.start();
        profiler.add_metric("test_metric", 42.0)?;
        profiler.stop();

        let session = &profiler.sessions()[0];
        assert_eq!(session.metrics.get("test_metric"), Some(&42.0));
        Ok(())
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "+512 B");
        assert_eq!(format_bytes(2048), "+2.00 KB");
        assert_eq!(format_bytes(-1024), "-1.00 KB");
    }

    #[test]
    fn test_memory_profiler() {
        let mut profiler = MemoryProfiler::new();
        profiler.snapshot("start");
        profiler.snapshot("end");

        assert_eq!(profiler.snapshots().len(), 2);
    }

    #[test]
    fn test_profiler_report() {
        let mut profiler = Profiler::new("test");
        profiler.start();
        thread::sleep(StdDuration::from_millis(50));
        profiler.stop();

        let report = profiler.report();
        assert!(report.contains("Profile Report"));
    }

    #[test]
    fn test_export_json() -> Result<()> {
        let mut profiler = Profiler::new("test");
        profiler.start();
        profiler.stop();

        let json = profiler.export_json()?;
        assert!(json.contains("name"));
        assert!(json.contains("test"));
        Ok(())
    }
}
