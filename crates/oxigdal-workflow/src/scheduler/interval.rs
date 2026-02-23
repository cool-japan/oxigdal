//! Interval-based workflow scheduling.

use crate::error::{Result, WorkflowError};
use crate::scheduler::SchedulerConfig;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration as StdDuration;

/// Interval schedule definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntervalSchedule {
    /// Interval duration in seconds.
    pub interval_secs: u64,
    /// Start time (defaults to now).
    pub start_time: Option<DateTime<Utc>>,
    /// End time (optional).
    pub end_time: Option<DateTime<Utc>>,
    /// Maximum number of executions (optional).
    pub max_executions: Option<usize>,
    /// Current execution count.
    pub execution_count: usize,
    /// Description of the schedule.
    pub description: Option<String>,
}

impl IntervalSchedule {
    /// Create a new interval schedule with the given interval in seconds.
    pub fn new(interval_secs: u64) -> Result<Self> {
        if interval_secs == 0 {
            return Err(WorkflowError::invalid_parameter(
                "interval_secs",
                "Interval must be greater than 0",
            ));
        }

        Ok(Self {
            interval_secs,
            start_time: None,
            end_time: None,
            max_executions: None,
            execution_count: 0,
            description: None,
        })
    }

    /// Create an interval schedule from a standard duration.
    pub fn from_duration(duration: StdDuration) -> Result<Self> {
        let secs = duration.as_secs();
        if secs == 0 {
            return Err(WorkflowError::invalid_parameter(
                "duration",
                "Duration must be greater than 0",
            ));
        }
        Self::new(secs)
    }

    /// Set the start time for this schedule.
    pub fn with_start_time(mut self, start_time: DateTime<Utc>) -> Self {
        self.start_time = Some(start_time);
        self
    }

    /// Set the end time for this schedule.
    pub fn with_end_time(mut self, end_time: DateTime<Utc>) -> Self {
        self.end_time = Some(end_time);
        self
    }

    /// Set the maximum number of executions.
    pub fn with_max_executions(mut self, max: usize) -> Self {
        self.max_executions = Some(max);
        self
    }

    /// Set the description.
    pub fn with_description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Calculate the next execution time from the given datetime.
    pub fn next_execution_from(&self, from: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
        // Check if max executions reached
        if let Some(max) = self.max_executions {
            if self.execution_count >= max {
                return Ok(None);
            }
        }

        let start = self.start_time.unwrap_or(from);

        // If current time is before start time, return start time
        if from < start {
            return Ok(Some(start));
        }

        // Calculate next execution
        let duration = Duration::try_seconds(self.interval_secs as i64)
            .ok_or_else(|| WorkflowError::internal("Duration overflow"))?;
        let next = from + duration;

        // Check if next execution is beyond end time
        if let Some(end) = self.end_time {
            if next > end {
                return Ok(None);
            }
        }

        Ok(Some(next))
    }

    /// Calculate all execution times within a given range.
    pub fn executions_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        max_count: usize,
    ) -> Result<Vec<DateTime<Utc>>> {
        let mut executions = Vec::new();
        let mut current = self.start_time.unwrap_or(start);
        let duration = Duration::try_seconds(self.interval_secs as i64)
            .ok_or_else(|| WorkflowError::internal("Duration overflow"))?;

        while current <= end && executions.len() < max_count {
            if current >= start {
                executions.push(current);
            }
            current += duration;

            // Check max executions
            if let Some(max) = self.max_executions {
                if executions.len() >= max {
                    break;
                }
            }

            // Check end time
            if let Some(end_time) = self.end_time {
                if current > end_time {
                    break;
                }
            }
        }

        Ok(executions)
    }

    /// Check if the schedule is still active.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        // Check if before start time
        if let Some(start) = self.start_time {
            if now < start {
                return false;
            }
        }

        // Check if after end time
        if let Some(end) = self.end_time {
            if now > end {
                return false;
            }
        }

        // Check if max executions reached
        if let Some(max) = self.max_executions {
            if self.execution_count >= max {
                return false;
            }
        }

        true
    }

    /// Increment the execution count.
    pub fn increment_execution_count(&mut self) {
        self.execution_count += 1;
    }
}

/// Interval scheduler for managing interval-based workflow executions.
pub struct IntervalScheduler {
    config: SchedulerConfig,
}

impl IntervalScheduler {
    /// Create a new interval scheduler.
    pub fn new(config: SchedulerConfig) -> Self {
        Self { config }
    }

    /// Calculate the next execution time for an interval.
    pub fn calculate_next_execution(
        &self,
        interval_secs: u64,
        last_execution: Option<DateTime<Utc>>,
    ) -> Result<DateTime<Utc>> {
        let now = Utc::now();
        let last = last_execution.unwrap_or(now);
        let duration = Duration::try_seconds(interval_secs as i64)
            .ok_or_else(|| WorkflowError::internal("Duration overflow"))?;
        Ok(last + duration)
    }

    /// Calculate missed executions for an interval schedule.
    pub fn calculate_missed_executions(
        &self,
        interval_secs: u64,
        last_execution: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>> {
        if !self.config.handle_missed_executions {
            return Ok(Vec::new());
        }

        let mut missed = Vec::new();
        let duration = Duration::try_seconds(interval_secs as i64)
            .ok_or_else(|| WorkflowError::internal("Duration overflow"))?;
        let mut current = last_execution + duration;

        while current < now && missed.len() < self.config.max_missed_executions {
            missed.push(current);
            current += duration;
        }

        Ok(missed)
    }

    /// Validate interval configuration.
    pub fn validate_interval(interval_secs: u64) -> Result<()> {
        if interval_secs == 0 {
            return Err(WorkflowError::invalid_parameter(
                "interval_secs",
                "Interval must be greater than 0",
            ));
        }

        // Reasonable maximum (1 year in seconds)
        const MAX_INTERVAL: u64 = 365 * 24 * 60 * 60;
        if interval_secs > MAX_INTERVAL {
            return Err(WorkflowError::invalid_parameter(
                "interval_secs",
                format!(
                    "Interval must be less than {} seconds (1 year)",
                    MAX_INTERVAL
                ),
            ));
        }

        Ok(())
    }
}

/// Common interval patterns.
pub struct IntervalPatterns;

impl IntervalPatterns {
    /// Every 10 seconds.
    pub fn every_10_seconds() -> u64 {
        10
    }

    /// Every 30 seconds.
    pub fn every_30_seconds() -> u64 {
        30
    }

    /// Every minute.
    pub fn every_minute() -> u64 {
        60
    }

    /// Every 5 minutes.
    pub fn every_5_minutes() -> u64 {
        5 * 60
    }

    /// Every 15 minutes.
    pub fn every_15_minutes() -> u64 {
        15 * 60
    }

    /// Every 30 minutes.
    pub fn every_30_minutes() -> u64 {
        30 * 60
    }

    /// Every hour.
    pub fn every_hour() -> u64 {
        60 * 60
    }

    /// Every 6 hours.
    pub fn every_6_hours() -> u64 {
        6 * 60 * 60
    }

    /// Every 12 hours.
    pub fn every_12_hours() -> u64 {
        12 * 60 * 60
    }

    /// Every day.
    pub fn every_day() -> u64 {
        24 * 60 * 60
    }

    /// Every week.
    pub fn every_week() -> u64 {
        7 * 24 * 60 * 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interval_schedule_creation() {
        let schedule = IntervalSchedule::new(60).expect("Failed to create schedule");
        assert_eq!(schedule.interval_secs, 60);
    }

    #[test]
    fn test_invalid_interval() {
        let result = IntervalSchedule::new(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_next_execution() {
        let schedule = IntervalSchedule::new(60).expect("Failed to create schedule");
        let now = Utc::now();
        let next = schedule
            .next_execution_from(now)
            .expect("Failed to calculate next execution");
        assert!(next.is_some());
    }

    #[test]
    fn test_max_executions() {
        let mut schedule = IntervalSchedule::new(60)
            .expect("Failed to create schedule")
            .with_max_executions(3);

        assert!(schedule.is_active(Utc::now()));

        schedule.increment_execution_count();
        schedule.increment_execution_count();
        schedule.increment_execution_count();

        assert!(!schedule.is_active(Utc::now()));
    }

    #[test]
    fn test_executions_in_range() {
        let schedule = IntervalSchedule::new(3600).expect("Failed to create schedule");
        let start = Utc::now();
        let end = start + Duration::try_hours(5).expect("Duration overflow");

        let executions = schedule
            .executions_in_range(start, end, 10)
            .expect("Failed to get executions");

        assert!(!executions.is_empty());
        assert!(executions.len() <= 10);
    }

    #[test]
    fn test_interval_patterns() {
        assert_eq!(IntervalPatterns::every_minute(), 60);
        assert_eq!(IntervalPatterns::every_hour(), 3600);
        assert_eq!(IntervalPatterns::every_day(), 86400);
    }

    #[test]
    fn test_validate_interval() {
        assert!(IntervalScheduler::validate_interval(60).is_ok());
        assert!(IntervalScheduler::validate_interval(0).is_err());
    }

    #[test]
    fn test_from_duration() {
        let duration = StdDuration::from_secs(120);
        let schedule = IntervalSchedule::from_duration(duration).expect("Failed to create");
        assert_eq!(schedule.interval_secs, 120);
    }
}
